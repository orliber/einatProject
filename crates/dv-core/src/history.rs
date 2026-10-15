//! "מה יצא מהמחשב" (round 27, D-062): every request kept with a case, newest first, the
//! exact text that left and what was hidden in it. It is the check after the fact that
//! stands in for showing each request first. Read only: nothing here sends or changes a thing.
//!
//! What was hidden is read from the request itself: the tags in its messages that stand for
//! this case's people (also the earlier rows a role change keeps), the psychologist, or the
//! case's mask book. Nothing new is stored for it. The fixed instructions are not scanned:
//! their example tags hide nothing.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::*;

/// One request that left the computer with a case's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SentItem {
    pub id: String,
    pub case_id: String,
    /// The case's code, as on its card.
    pub case_code: String,
    /// What it was for: a section key, `sorting` or `consult`.
    pub place: String,
    #[ts(type = "number")]
    pub at: i64,
    /// Who got it, by name ("Claude"); empty for a demo answer, where nothing left.
    pub ai: String,
    pub demo: bool,
    pub sha256: String,
    pub bytes: u32,
    pub hidden: Vec<HiddenWord>,
    /// With a search: the word was hidden in this request (it did not leave), rather than
    /// found in the text that did.
    pub hidden_match: bool,
}

/// A tag in a request and what it stood for on the computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HiddenWord {
    pub tag: String,
    /// The words it hid; empty for a tag that keeps no words (a phone number, an ID).
    pub word: String,
}

/// One request in full: the text in parts as she can read it, and the exact bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SentText {
    pub item: SentItem,
    pub parts: Vec<SentPart>,
    /// The request exactly as it left (JSON).
    pub exact: String,
    /// The kept bytes still hash to what was recorded when it left.
    pub hash_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SentPart {
    /// "הוראות קבועות", "בקשה", "תשובה קודמת".
    pub label: String,
    pub text: String,
}

/// The fixed instructions, then each message, as plain text.
fn parts_of(body: &Value) -> Vec<SentPart> {
    let text_of = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n"),
            _ => String::new(),
        }
    };
    let mut parts = Vec::new();
    if let Some(system) = body.get("system") {
        let text = text_of(system);
        if !text.is_empty() {
            parts.push(SentPart {
                label: "הוראות קבועות".to_owned(),
                text,
            });
        }
    }
    for m in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let label = match m.get("role").and_then(Value::as_str) {
            Some("assistant") => "תשובה קודמת",
            _ => "בקשה",
        };
        parts.push(SentPart {
            label: label.to_owned(),
            text: m.get("content").map(text_of).unwrap_or_default(),
        });
    }
    parts
}

/// What each tag of a case stands for on the computer.
struct CaseWords {
    code: String,
    words: HashMap<String, String>,
}

impl Core {
    fn case_words(&mut self, case_id: &str) -> Result<CaseWords, CoreError> {
        let v = self.vault_ref()?;
        let mut words: HashMap<String, String> = v
            .identities(case_id)?
            .into_iter()
            .map(|i| (i.tag, i.value))
            .collect();
        for m in v.masks(case_id)? {
            words.entry(m.tag).or_insert(m.original);
        }
        if let Some(name) = v.practitioner()?.names.first() {
            words
                .entry(dv_domain::PRACTITIONER_TAG.to_owned())
                .or_insert_with(|| name.clone());
        }
        Ok(CaseWords {
            code: v.case_meta(case_id)?.code,
            words,
        })
    }

    fn sent_item(t: &dv_domain::Transmission, words: &CaseWords) -> (SentItem, Vec<SentPart>) {
        let parts = serde_json::from_str::<Value>(&t.payload_tagged)
            .map(|b| parts_of(&b))
            .unwrap_or_default();
        let mut hidden: Vec<HiddenWord> = Vec::new();
        for p in parts.iter().filter(|p| p.label != "הוראות קבועות") {
            for tag in dv_privacy::restore::remaining_tags(&p.text) {
                let word = words.words.get(&tag).cloned();
                let ours = word.is_some() || dv_privacy::is_generic_tag(&tag);
                if ours && !hidden.iter().any(|h| h.tag == tag) {
                    hidden.push(HiddenWord {
                        tag,
                        word: word.unwrap_or_default(),
                    });
                }
            }
        }
        let demo = t.model == "demo";
        let item = SentItem {
            id: t.id.clone(),
            case_id: t.case_id.clone(),
            case_code: words.code.clone(),
            place: t.section_key.clone(),
            at: t.created_at,
            ai: if demo {
                String::new()
            } else {
                provider_of(&t.model).display_name().to_owned()
            },
            demo,
            sha256: t.payload_sha256.clone(),
            bytes: u32::try_from(t.payload_tagged.len()).unwrap_or(u32::MAX),
            hidden,
            hidden_match: false,
        };
        (item, parts)
    }

    /// Every request kept with a case (one case, or all of them), newest first. `place`
    /// narrows to one section; `query` keeps the requests whose text holds it, and those in
    /// which it was hidden (marked, since it did not leave).
    pub fn sent_history(
        &mut self,
        case_id: Option<&str>,
        place: Option<&str>,
        query: &str,
    ) -> Result<Vec<SentItem>, CoreError> {
        let cases: Vec<String> = match case_id {
            Some(c) => vec![c.to_owned()],
            None => self
                .vault_ref()?
                .list_cases()?
                .into_iter()
                .map(|c| c.id)
                .collect(),
        };
        let q = query.trim();
        let mut out = Vec::new();
        for case in cases {
            let words = self.case_words(&case)?;
            for t in self.vault_ref()?.transmissions(&case)? {
                if place.is_some_and(|p| p != t.section_key) {
                    continue;
                }
                let (mut item, parts) = Self::sent_item(&t, &words);
                if !q.is_empty() && !parts.iter().any(|p| p.text.contains(q)) {
                    if !item.hidden.iter().any(|h| h.word.contains(q)) {
                        continue;
                    }
                    item.hidden_match = true;
                }
                out.push(item);
            }
        }
        out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
        Ok(out)
    }

    /// One request in full, and whether its kept bytes still match the hash taken when it left.
    pub fn sent_text(&mut self, case_id: &str, id: &str) -> Result<SentText, CoreError> {
        let words = self.case_words(case_id)?;
        let t = self
            .vault_ref()?
            .transmissions(case_id)?
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| CoreError::NotFound("הבקשה הזו".to_owned()))?;
        let (item, parts) = Self::sent_item(&t, &words);
        let hash_ok = dv_vault::crypto::sha256_hex(t.payload_tagged.as_bytes()) == t.payload_sha256;
        Ok(SentText {
            item,
            parts,
            exact: t.payload_tagged,
            hash_ok,
        })
    }
}
