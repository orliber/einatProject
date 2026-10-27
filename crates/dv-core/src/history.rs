//! "מה יצא מהמחשב" (round 27, D-062): every request kept with a case, newest first, the
//! exact text that left and what was hidden in it. It is the check after the fact that
//! stands in for showing each request first. Read only: nothing here sends or changes a thing.
//!
//! What was hidden is read from the request itself: the tags in its messages that stand for
//! this case's people (also the earlier rows a role change keeps), the psychologist, or the
//! case's mask book. Nothing new is stored for it. The fixed instructions are not scanned:
//! their example tags hide nothing.

use std::collections::hash_map::Entry;

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

/// Where the next page of the history starts: the last request read (D-123).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SentCursor {
    #[ts(type = "number")]
    pub at: i64,
    pub id: String,
}

/// One page of the history, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SentPage {
    pub items: Vec<SentItem>,
    /// Pass back as `before` for the next page; none when every request was read. With a
    /// search, a page can stop at [`SENT_SCAN`] requests read before it found a full page, so
    /// one search never holds the vault for a year of requests at once.
    pub next: Option<SentCursor>,
}

/// Requests shown on one page.
pub const SENT_PAGE: usize = 50;
/// Requests read (opened and searched) for one page at most.
pub const SENT_SCAN: usize = 500;
/// Requests opened from the vault at a time while a searched page fills.
const SENT_BATCH: usize = 100;

/// What each tag of a case stands for on the computer.
struct CaseWords {
    code: String,
    words: HashMap<String, String>,
}

/// `practitioner`: the psychologist's first name, read once for every case of a page.
fn case_words(
    v: &Vault,
    case_id: &str,
    practitioner: Option<&str>,
) -> Result<CaseWords, CoreError> {
    let mut words: HashMap<String, String> = v
        .identities(case_id)?
        .into_iter()
        .map(|i| (i.tag, i.value))
        .collect();
    for m in v.masks(case_id)? {
        words.entry(m.tag).or_insert(m.original);
    }
    if let Some(name) = practitioner {
        words
            .entry(dv_domain::PRACTITIONER_TAG.to_owned())
            .or_insert_with(|| name.to_owned());
    }
    Ok(CaseWords {
        code: v.case_meta(case_id)?.code,
        words,
    })
}

fn practitioner_name(v: &Vault) -> Result<Option<String>, CoreError> {
    Ok(v.practitioner()?.names.first().cloned())
}

impl Core {
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

    /// One page of the requests kept with a case (one case, or all of them), newest first,
    /// from `before` on. `place` narrows to one section; `query` keeps the requests whose text
    /// holds it, and those in which it was hidden (marked, since it did not leave).
    ///
    /// The case and the place narrow in the vault's SQL; only the rows read are opened, and
    /// each case's words are read once for the page (D-123).
    pub fn sent_history(
        &mut self,
        case_id: Option<&str>,
        place: Option<&str>,
        query: &str,
        before: Option<SentCursor>,
    ) -> Result<SentPage, CoreError> {
        let v = self.vault_ref()?;
        let practitioner = practitioner_name(v)?;
        let q = query.trim();
        let mut words: HashMap<String, CaseWords> = HashMap::new();
        let mut items = Vec::new();
        let mut cursor = before;
        let mut read = 0;
        loop {
            // Without a search every request read is shown: read only what fills the page.
            let room = if q.is_empty() {
                SENT_PAGE - items.len()
            } else {
                SENT_BATCH
            };
            let want = u32::try_from((SENT_SCAN - read).min(room)).unwrap_or(u32::MAX);
            let batch = v.transmissions_newest(
                case_id,
                place,
                cursor.as_ref().map(|c| (c.at, c.id.as_str())),
                want,
            )?;
            let ended = batch.len() < want as usize;
            for t in batch {
                read += 1;
                cursor = Some(SentCursor {
                    at: t.created_at,
                    id: t.id.clone(),
                });
                let w = match words.entry(t.case_id.clone()) {
                    Entry::Occupied(e) => e.into_mut(),
                    Entry::Vacant(e) => {
                        e.insert(case_words(v, &t.case_id, practitioner.as_deref())?)
                    }
                };
                let (mut item, parts) = Self::sent_item(&t, w);
                if !q.is_empty() && !parts.iter().any(|p| p.text.contains(q)) {
                    if !item.hidden.iter().any(|h| h.word.contains(q)) {
                        continue;
                    }
                    item.hidden_match = true;
                }
                items.push(item);
                if items.len() == SENT_PAGE {
                    break;
                }
            }
            if ended && items.len() < SENT_PAGE {
                return Ok(SentPage { items, next: None });
            }
            if items.len() == SENT_PAGE || read >= SENT_SCAN {
                // A full page, or as much as one page reads: is there anything older?
                let next = match cursor {
                    Some(c) if v.transmissions_older(case_id, place, (c.at, &c.id))? => Some(c),
                    _ => None,
                };
                return Ok(SentPage { items, next });
            }
        }
    }

    /// How many requests left since `since` (seconds), of every case: the count on the
    /// privacy page, without reading the history it counts (D-123).
    pub fn sent_count(&mut self, since: i64) -> Result<u32, CoreError> {
        Ok(self.vault_ref()?.transmissions_since(since)?)
    }

    /// One request in full, and whether its kept bytes still match the hash taken when it left.
    pub fn sent_text(&mut self, case_id: &str, id: &str) -> Result<SentText, CoreError> {
        let v = self.vault_ref()?;
        let t = v
            .transmission(case_id, id)?
            .ok_or_else(|| CoreError::NotFound("הבקשה הזו".to_owned()))?;
        let words = case_words(v, case_id, practitioner_name(v)?.as_deref())?;
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
