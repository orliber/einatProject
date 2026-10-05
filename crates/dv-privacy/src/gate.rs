//! The final gate (layer 7, fail-closed). The only place a [`ClearedPayload`] can be made,
//! and `dv-egress` accepts nothing else. CI checks that no other file constructs one.

use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

use aws_lc_rs::digest;
use dv_domain::PRACTITIONER_TAG;
use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::lexicon::LEXICON;
use crate::patterns;
use crate::pipeline::{
    arabic_identity_matches, declared_reads_as_word, identity_index, past_name, reads_as_word,
    PrivacyContext,
};
use crate::text::{normalize, prefix_splits, tokenize};

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

static TAG_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\[[^\[\]\n]{1,40}\]").ok());

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

    let index = identity_index(req.ctx);
    let mut texts = Vec::new();
    strings(req.body, &mut texts);
    for text in texts {
        let tokens = tokenize(text);
        for m in index.find(text, &tokens) {
            // "שני ההורים", "בגיל 3": the same certain readings the pipeline leaves as words.
            if declared_reads_as_word(text, &tokens, m.first_token, m.last_token, m.prefix) {
                continue;
            }
            // An ordinary word the psychologist confirmed ("שאלון" when a child is "אלון").
            let whole = &tokens[m.first_token];
            if m.prefix > 0
                && LEXICON.common_words.contains(&whole.norm)
                && (req.ctx.allowlisted)(&whole.norm)
            {
                continue;
            }
            reasons.push(reason(
                "identity",
                "נמצא שם מוצהר בטקסט היוצא",
                Some(text[m.start..m.end].to_owned()),
            ));
        }
        for (start, end, _) in arabic_identity_matches(text, &tokens, req.ctx) {
            reasons.push(reason(
                "identity",
                "נמצא שם מוצהר בטקסט היוצא",
                Some(text[start..end].to_owned()),
            ));
        }
        match patterns::find(text, req.ctx.today) {
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
        for t in &tokens {
            // Like the pipeline: "כרים" is a word, not כ + רים.
            let is_name = prefix_splits(&t.norm).iter().any(|(p, h)| {
                LEXICON.first_names.contains(h)
                    && !(req.ctx.allowlisted)(h)
                    && !(*p > 0 && reads_as_word(&t.norm, h))
            });
            if is_name && !(req.ctx.allowlisted)(&t.norm) && !LEXICON.common_words.contains(&t.norm)
            {
                reasons.push(reason(
                    "unknown_name",
                    "נמצא שם שלא הוחלט לגביו",
                    Some(text[t.start..t.end].to_owned()),
                ));
            }
        }
        for t in &tokens {
            let past = prefix_splits(&t.norm)
                .iter()
                .any(|(p, h)| past_name(req.ctx, h) && !(*p > 0 && reads_as_word(&t.norm, h)));
            if past && !(req.ctx.allowlisted)(&t.norm) {
                reasons.push(reason(
                    "past_report_name",
                    "נמצא שם מדוח ישן",
                    Some(text[t.start..t.end].to_owned()),
                ));
            }
        }
        if let Some(re) = TAG_RE.as_ref() {
            for m in re.find_iter(text) {
                let tag = m.as_str();
                let allowed = req.case_tags.contains(tag)
                    || GENERIC_TAGS.contains(&tag)
                    || tag.starts_with("[חסר");
                if !allowed {
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
