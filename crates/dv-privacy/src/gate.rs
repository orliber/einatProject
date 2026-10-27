//! The final gate (layer 7, fail-closed). The only place a [`ClearedPayload`] can be made,
//! and `dv-egress` accepts nothing else. CI checks that no other file constructs one.

use std::collections::HashSet;
use std::fmt;

use aws_lc_rs::digest;
use dv_domain::PRACTITIONER_TAG;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::lexicon::LEXICON;
use crate::masks::TAG_RE;
use crate::matcher::{quote_bytes, quote_opened, strip_quote_tail};
use crate::patterns;
use crate::pipeline::{
    arabic_identity_matches, declared_reads_as_word, identity_index, other_case_word, past_name,
    reads_as_word, PrivacyContext, Target,
};
use crate::text::{normalize, prefix_splits, tokenize, Token};

/// Generic replacements produced by the pipeline; always allowed in outgoing text.
pub const GENERIC_TAGS: &[&str] = &[
    "[ת.ז.]",
    "[טלפון]",
    "[דוא\"ל]",
    "[קישור]",
    "[מספר]",
    "[תאריך]",
    "[שנה]",
    "[כתובת]",
    "[יישוב_אחר]",
    "[בית_חולים]",
    PRACTITIONER_TAG,
];

/// A tag the gate lets through: one of the case's, a generic one, a "missing" note, or a
/// score of a past report in the fixed form (round 32: a number and fixed table words only).
fn tag_allowed(tag: &str, case_tags: &HashSet<String>) -> bool {
    case_tags.contains(tag)
        || crate::masks::is_generic_tag(tag)
        || crate::masks::is_missing_note(tag)
        || dv_domain::is_score_tag(tag)
}

/// The text with every tag the gate would refuse replaced by `with(tag)`, or `None` when it
/// has none. For the case's own stored text only: a tag left there by a name she brought
/// back ("להחזיר"), whose row is gone, would otherwise block every later request of the case.
/// The replacement is checked by the gate like any other text.
#[must_use]
pub fn replace_foreign_tags(
    text: &str,
    case_tags: &HashSet<String>,
    with: impl Fn(&str) -> String,
) -> Option<String> {
    let re = TAG_RE.as_ref()?;
    let mut changed = false;
    // The regex copies the text between matches itself, so no offset is cut by hand.
    let out = re.replace_all(text, |caps: &regex::Captures<'_>| {
        let tag = caps.get(0).map_or("", |m| m.as_str());
        // Her kept words (D-126) stay in her stored paragraph; they are never sent.
        if tag_allowed(tag, case_tags) || crate::masks::is_kept_words_tag(tag) {
            tag.to_owned()
        } else {
            changed = true;
            with(tag)
        }
    });
    changed.then(|| out.into_owned())
}

/// The plain word of a tag: `[מוסד_1]` reads "מוסד", `[ילד_גן_2]` "ילד גן". Hebrew joins
/// its prefixes to it ("ב[מוסד_1]" becomes "במוסד").
#[must_use]
pub fn tag_word(tag: &str) -> String {
    let inner = tag.trim_start_matches('[').trim_end_matches(']');
    let inner = inner
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches('_');
    inner.replace('_', " ")
}

/// Request bytes that passed every check. Fields are private: only this module builds one.
pub struct ClearedPayload {
    case_id: String,
    body: Vec<u8>,
    sha256: String,
}

impl ClearedPayload {
    #[must_use]
    pub fn case_id(&self) -> &str {
        &self.case_id
    }

    /// Exactly the bytes to send; never re-serialized after the gate.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// What the psychologist's approval is bound to.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl fmt::Debug for ClearedPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClearedPayload")
            .field("case_id", &self.case_id)
            .field("sha256", &self.sha256)
            .finish_non_exhaustive()
    }
}

/// Why the gate refused. `code` goes to the audit log; `detail` is shown only locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BlockReason {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
#[error("sending was blocked: {reasons:?}")]
#[ts(export)]
pub struct Blocked {
    pub reasons: Vec<BlockReason>,
}

#[derive(Debug)]
pub struct GateRequest<'a> {
    /// The complete API request body (system prompt, history, message, schema).
    pub body: &'a serde_json::Value,
    pub ctx: &'a PrivacyContext<'a>,
    /// Tags that belong to this case (`[ילד]`, `[גננת]`, …).
    pub case_tags: &'a HashSet<String>,
    pub unresolved_suspects: usize,
    /// Trap strings used by tests; any occurrence blocks.
    pub canaries: &'a [String],
    pub max_bytes: usize,
}

fn strings<'v>(value: &'v serde_json::Value, out: &mut Vec<&'v str>) {
    match value {
        serde_json::Value::String(s) => out.push(s),
        serde_json::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                out.push(k);
                strings(v, out);
            }
        }
        _ => {}
    }
}

fn reason(code: &str, message: &str, detail: Option<String>) -> BlockReason {
    BlockReason {
        code: code.to_owned(),
        message: message.to_owned(),
        detail,
    }
}

/// A name the gate refuses in `text`: a declared or kept name (`identity`), a lexicon first
/// name (`unknown_name`) or a name from her past reports (`past_report_name`), with its byte
/// span. The pipeline runs exactly this on its own output and hides what it finds (D-050), so
/// what leaves the filter passes here: a refusal on a name is a bug, never a dead end for her.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
}

pub(crate) fn name_findings(text: &str, ctx: &PrivacyContext<'_>) -> Vec<Finding> {
    let tokens = tokenize(text);
    let mut out = Vec::new();
    for m in identity_index(ctx).find(text, &tokens) {
        // "שני ההורים", "בגיל 3": the same certain readings the pipeline leaves as words.
        if declared_reads_as_word(text, &tokens, m.first_token, m.last_token, m.prefix) {
            continue;
        }
        // Another case's name read as the word it also is, as the pipeline reads it.
        if matches!(&m.payload, Target::Identity { case_id, .. } if *case_id != ctx.case_id)
            && other_case_word(text, &tokens, ctx, m.first_token, m.last_token, m.prefix)
        {
            continue;
        }
        // An ordinary word the psychologist confirmed ("שאלון" when a child is "אלון").
        // A token the match does not point at is no confirmed word: the finding stands.
        let confirmed_word = |whole: &Token| {
            LEXICON.common_words.contains(&whole.norm) && (ctx.allowlisted)(&whole.norm)
        };
        if m.prefix > 0 && tokens.get(m.first_token).is_some_and(confirmed_word) {
            continue;
        }
        out.push(Finding {
            start: m.start,
            end: m.end,
            code: "identity",
        });
    }
    for (start, end, _) in arabic_identity_matches(text, &tokens, ctx) {
        out.push(Finding {
            start,
            end,
            code: "identity",
        });
    }
    for t in &tokens {
        // The word as written, and without a closing quote glued to it ("'יובל'").
        let (bare, quoted) = strip_quote_tail(&t.norm, quote_opened(text, t, 0));
        let mut forms = vec![(t.norm.as_str(), t.end)];
        if quoted > 0 {
            forms.push((bare, t.end - quote_bytes(text, t, quoted)));
        }
        let mut found = |end: usize, code: &'static str| {
            if !out.iter().any(|f| f.start == t.start && f.code == code) {
                out.push(Finding {
                    start: t.start,
                    end,
                    code,
                });
            }
        };
        for (norm, end) in forms {
            // Like the pipeline: "כרים" is a word, not כ + רים.
            let is_name = prefix_splits(norm).iter().any(|(p, h)| {
                LEXICON.first_names.contains(h)
                    && !(ctx.allowlisted)(h)
                    && !(*p > 0 && reads_as_word(norm, h))
            });
            if is_name && !(ctx.allowlisted)(norm) && !LEXICON.common_words.contains(norm) {
                found(end, "unknown_name");
            }
            let past = prefix_splits(norm)
                .iter()
                .any(|(p, h)| past_name(ctx, h) && !(*p > 0 && reads_as_word(norm, h)));
            if past && !(ctx.allowlisted)(norm) {
                found(end, "past_report_name");
            }
        }
    }
    out
}

/// Scan the whole request. Any finding blocks; there is no override in code.
pub fn clear(req: &GateRequest<'_>) -> Result<ClearedPayload, Blocked> {
    let mut reasons = Vec::new();
    if req.unresolved_suspects > 0 {
        reasons.push(reason(
            "unresolved_suspects",
            "יש חשדות שעוד לא הוחלט לגביהם",
            Some(req.unresolved_suspects.to_string()),
        ));
    }
    let body = serde_json::to_vec(req.body).map_err(|e| Blocked {
        reasons: vec![reason("serialize", "שגיאה פנימית", Some(e.to_string()))],
    })?;
    if body.len() > req.max_bytes {
        reasons.push(reason(
            "too_large",
            "הבקשה גדולה מהמותר",
            Some(body.len().to_string()),
        ));
    }

    let mut texts = Vec::new();
    strings(req.body, &mut texts);
    for text in texts {
        for f in name_findings(text, req.ctx) {
            let message = match f.code {
                "identity" => "נמצא שם מוצהר בטקסט היוצא",
                "unknown_name" => "נמצא שם שלא הוחלט לגביו",
                _ => "נמצא שם מדוח ישן",
            };
            // The finding is refused whatever its detail; one that cannot be cut out of the
            // text (never expected) is refused without it.
            reasons.push(reason(
                f.code,
                message,
                text.get(f.start..f.end).map(str::to_owned),
            ));
        }
        // The case's tags and generic ones blanked, so the number inside a tag
        // ("[מספר_זהות_1] מרץ") is not read as a date. A "[חסר: …]" note or a score is read.
        let blanked = patterns::blank_tags(text, |t| {
            req.case_tags.contains(t) || crate::masks::is_generic_tag(t)
        });
        match patterns::find(&blanked, req.ctx.today) {
            Ok(hits) => {
                for h in hits {
                    reasons.push(reason(
                        "pattern",
                        "נמצא פרט מזהה (מספר, תאריך, כתובת…)",
                        Some(h.kind.label_he().to_owned()),
                    ));
                }
            }
            Err(e) => reasons.push(reason(
                "internal",
                "שגיאה פנימית בסריקה",
                Some(e.to_string()),
            )),
        }
        // An address field ("כתובת: מסדה 33, חיפה"), as the filter reads it: the filter hides
        // each part whole, or the tag of what it hid there first ("שכונת [יישוב_אחר]") stays
        // in it, so a part without any tag was never filtered.
        // A part that cannot be read (never expected) was not shown to be filtered: refused.
        for (start, end) in crate::pipeline::address_field_spans(text) {
            let tagged = text
                .get(start..end)
                .is_some_and(|part| TAG_RE.as_ref().is_some_and(|re| re.is_match(part)));
            if !tagged {
                reasons.push(reason(
                    "pattern",
                    "נמצא פרט מזהה (מספר, תאריך, כתובת…)",
                    Some("כתובת".to_owned()),
                ));
            }
        }
        if let Some(re) = TAG_RE.as_ref() {
            for m in re.find_iter(text) {
                let tag = m.as_str();
                if !tag_allowed(tag, req.case_tags) {
                    reasons.push(reason(
                        "foreign_tag",
                        "תגית שלא שייכת לתיק הזה",
                        Some(tag.to_owned()),
                    ));
                }
            }
        }
        let norm = normalize(text);
        for canary in req.canaries {
            if norm.contains(&normalize(canary)) {
                reasons.push(reason("canary", "נמצאה מחרוזת מלכודת", None));
            }
        }
    }

    if reasons.is_empty() {
        let sha256 = digest::digest(&digest::SHA256, &body)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(ClearedPayload {
            case_id: req.ctx.case_id.to_owned(),
            body,
            sha256,
        })
    } else {
        reasons.dedup();
        Err(Blocked { reasons })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn foreign_tags_in_her_own_text_become_plain_words() {
        let tags: HashSet<String> = ["[ילד]".to_owned()].into_iter().collect();
        let text = "[ילד] למד ב[מוסד_1] עם [ילד_גן_2] ו[מספר] ילדים [חסר: גיל].";
        assert_eq!(
            replace_foreign_tags(text, &tags, tag_word).as_deref(),
            Some("[ילד] למד במוסד עם ילד גן ו[מספר] ילדים [חסר: גיל].")
        );
        assert_eq!(replace_foreign_tags("[ילד] משחק.", &tags, tag_word), None);
    }

    #[test]
    fn an_address_field_the_filter_missed_is_refused() {
        let body = serde_json::json!({"messages": [{"role": "user", "content": "פרטים\nכתובת: מסדה 33, חיפה\n"}]});
        let filtered = serde_json::json!({"messages": [{"role": "user", "content": "פרטים\nכתובת: [כתובת], [יישוב_אחר]\n"}]});
        let tags = HashSet::new();
        let none = |_: &str| false;
        let ctx = crate::PrivacyContext {
            case_id: "c1",
            identities: &[],
            practitioner: &[],
            allowlisted: &none,
            confirmed_names: &none,
            past_names: &none,
            today: (2026, 10, 12),
            masks: None,
        };
        let req = |body| GateRequest {
            body,
            ctx: &ctx,
            case_tags: &tags,
            unresolved_suspects: 0,
            canaries: &[],
            max_bytes: 100_000,
        };
        let refused = clear(&req(&body)).err().unwrap();
        assert!(refused.reasons.iter().any(|r| r.code == "pattern"));
        assert!(clear(&req(&filtered)).is_ok());
    }
}
