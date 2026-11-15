//! What every request to the AI owes once its answer is back, whatever step sent it: the
//! draft of a section, the check against the sources, the sorting of materials, a question,
//! the style analysis. One place, so a new step cannot skip one.
//!
//! The way out is already one road: a request is built, filtered and cleared only in `gate`
//! (the only place a `Pending` with a `ClearedPayload` is made), approved by its hash, and
//! taken out by `begin_send` (key, zero-retention agreement, monthly ceiling). `receive` is
//! the road back.

use super::*;

/// Which step a request belongs to: the same kind an approval is checked against when it is
/// taken out (`begin_send`), so the road out and the road back name a step one way (A1).
pub(crate) type Stage = SendKind;

impl PendingKind {
    pub(crate) fn stage(&self) -> Stage {
        self.send_kind()
    }

    /// The case this request is about, if any.
    pub(crate) fn case(&self) -> Option<&str> {
        self.trace(false, "").0
    }

    /// The case and the log's words for this request; and, for a request about a case's
    /// text, where the exact bytes sent are kept ("what left the computer" in the case).
    fn trace(&self, demo: bool, ai: &str) -> (Option<&str>, Option<&str>, Value) {
        match self {
            PendingKind::Section {
                case_id,
                section_key,
                ..
            } => (
                Some(case_id),
                Some(section_key),
                serde_json::json!({ "section": section_key, "demo": demo, "ai": ai }),
            ),
            PendingKind::Verify {
                case_id,
                section_key,
                ..
            } => (
                Some(case_id),
                Some(section_key),
                serde_json::json!({ "section": section_key, "stage": "verify", "demo": demo, "ai": ai }),
            ),
            PendingKind::ReportCheck { case_id, .. } => (
                Some(case_id),
                Some("report_check"),
                serde_json::json!({ "stage": "report_check", "demo": demo, "ai": ai }),
            ),
            PendingKind::Sort { case_id, .. } => (
                Some(case_id),
                Some("sorting"),
                serde_json::json!({ "sorting": true, "demo": demo, "ai": ai }),
            ),
            // A question from inside a case is kept with it too (round 27): the case's text
            // may be in it. A question without a case has no case to keep it with.
            PendingKind::Consult { case_id, .. } => (
                case_id.as_deref(),
                case_id.as_ref().map(|_| "consult"),
                serde_json::json!({ "consult": true, "demo": demo, "ai": ai }),
            ),
            PendingKind::StyleAnalysis { .. } => (
                None,
                None,
                serde_json::json!({ "style": "analysis", "demo": demo }),
            ),
            PendingKind::StyleSynthesis { .. } => (
                None,
                None,
                serde_json::json!({ "style": "profile", "demo": demo }),
            ),
            PendingKind::Interpretation { .. } => (
                None,
                None,
                serde_json::json!({ "style": "interpretation", "demo": demo }),
            ),
        }
    }
}

/// An answer that is in, counted and recorded; the step reads it.
pub(crate) struct Received {
    pub kind: PendingKind,
    pub response: Value,
    pub demo: bool,
    pub model: String,
}

impl Core {
    /// The steps every answer goes through before a step reads it:
    /// 1. it is the step's own kind of request;
    /// 2. a failure is not a send: the approval stays for a retry;
    /// 3. the answer counts toward the month (numbers only);
    /// 4. the request is recorded (the exact bytes with the case, and the log) before
    ///    anything about the reply can fail;
    /// 5. a kept-words tag in the answer ("[מילים_1]", D-126) is written as plain words: the
    ///    model never saw one, and one it wrote must never stand for her words.
    pub(crate) fn receive(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
        stage: Stage,
    ) -> Result<Received, CoreError> {
        if out.pending.kind.stage() != stage {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        }
        let (response, demo) = match response {
            Ok(r) => r,
            Err(e) => {
                // An error the service gave, or a connection lost on the way, may come after
                // the bytes reached it: the request is recorded as sent, since this history
                // stands in for the window before sending (the review of stage 2).
                if may_have_left(&e) {
                    self.record_sent(&out, false)?;
                }
                self.restore_pending(out);
                return Err(e);
            }
        };
        self.note_usage(&out.pending.payload, &response, demo);
        self.record_sent(&out, demo)?;
        // Recorded as sent in any case (it left); applied only in the session it was approved
        // in (a lock in between ends it, as for every other job), and only to a case that is
        // still at work: one in the recycle bin takes nothing new.
        self.same_session(out.session)?;
        if let Some(case_id) = out.pending.kind.case() {
            if self.vault_ref()?.case_in_trash(case_id)? {
                return Err(CoreError::Refused(
                    "התיק הועבר לסל המחזור בינתיים, והתשובה לא נשמרה בו.".to_owned(),
                ));
            }
        }
        let Outgoing { pending, model, .. } = out;
        let Pending { kind, .. } = pending;
        let mut response = response;
        plain_kept_words_in(&mut response);
        Ok(Received {
            kind,
            response,
            demo,
            model,
        })
    }
}

impl Core {
    /// The request as it left: the exact bytes with the case, and the log.
    fn record_sent(&mut self, out: &Outgoing, demo: bool) -> Result<(), CoreError> {
        let ai = provider_of(&out.model).display_name();
        let (case_id, kept_as, mut details) = out.pending.kind.trace(demo, ai);
        // The same bytes went out twice (D-143): the log says so; the bytes are kept once.
        if out.again.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(fields) = details.as_object_mut() {
                fields.insert("sent_twice".to_owned(), Value::Bool(true));
            }
        }
        let payload = &out.pending.payload;
        let sent = SentRecord {
            case_id: case_id.map(str::to_owned),
            kept_as: kept_as.map(str::to_owned),
            sha256: payload.sha256().to_owned(),
            model: if demo {
                "demo".to_owned()
            } else {
                out.model.clone()
            },
            payload_text: String::from_utf8_lossy(payload.body()).into_owned(),
            details,
        };
        if self.vault.is_none() {
            // The vault locked while the request was with the AI (she walked away, the lid
            // closed): it left all the same, and the history that stands in for reviewing each
            // request must show it. Written at the next opening.
            self.owe(OwedSend::Sent(sent));
            return Ok(());
        }
        self.write_sent(&sent)
    }

    fn write_sent(&mut self, sent: &SentRecord) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        if let (Some(case_id), Some(kept_as)) = (sent.case_id.as_deref(), sent.kept_as.as_deref()) {
            v.add_transmission(
                case_id,
                kept_as,
                &sent.sha256,
                &sent.model,
                &sent.payload_text,
            )?;
        }
        v.record(AuditEvent::Send, sent.case_id.as_deref(), &sent.details)?;
        Ok(())
    }

    pub(crate) fn owe(&mut self, owed: OwedSend) {
        // A bound, never expected to be met: a request takes minutes and a lock ends new ones.
        if self.owed_sends.len() < OWED_MAX {
            self.owed_sends.push(owed);
        }
    }

    /// Right after the vault opens: what left while it was locked is written down. A case
    /// that is gone by now (deleted for good, another vault restored) keeps its line in the
    /// log without the bytes.
    pub(crate) fn settle_owed_sends(&mut self) {
        for owed in std::mem::take(&mut self.owed_sends) {
            match owed {
                OwedSend::Usage { key, model, tokens } => self.count_usage(&key, &model, tokens),
                OwedSend::Sent(sent) => {
                    if self.write_sent(&sent).is_err() {
                        if let Ok(v) = self.vault_mut() {
                            let _ = v.record(AuditEvent::Send, None, &sent.details);
                        }
                    }
                }
            }
        }
    }
}

const OWED_MAX: usize = 64;

/// A request as it left, to be written in the case's history and in the log.
pub(crate) struct SentRecord {
    case_id: Option<String>,
    kept_as: Option<String>,
    sha256: String,
    model: String,
    payload_text: String,
    details: Value,
}

/// What a request that came back while the vault was locked still owes the records.
pub(crate) enum OwedSend {
    Sent(SentRecord),
    Usage {
        key: String,
        model: String,
        tokens: crate::usage::Tokens,
    },
}

impl std::fmt::Debug for OwedSend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwedSend")
    }
}

/// Every text of an answer, wherever it sits in it, with its kept-words tags made plain.
fn plain_kept_words_in(value: &mut Value) {
    match value {
        Value::String(s) => {
            if let std::borrow::Cow::Owned(plain) = dv_privacy::restore::plain_kept_words(s) {
                *s = plain;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(plain_kept_words_in),
        Value::Object(fields) => fields.values_mut().for_each(plain_kept_words_in),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// A failure that can come after the request reached the service: an answer it gave (a key
/// refused, too many requests, busy, refused, an answer that could not be read) or a
/// connection lost on the way. A refusal on this computer (the rules, no key, the emergency
/// stop, the size, the daily limit, an open breaker, the secure connection, a local model)
/// sent nothing.
fn may_have_left(e: &CoreError) -> bool {
    matches!(
        e,
        CoreError::Egress(
            EgressError::Unauthorized
                | EgressError::RateLimited
                | EgressError::Unavailable(_)
                | EgressError::Overloaded
                | EgressError::Billing(_)
                | EgressError::Forbidden(_)
                | EgressError::ModelGone(_)
                | EgressError::InvalidRequest(_)
                | EgressError::Rejected(..)
                | EgressError::Protocol(_)
                | EgressError::Offline
                | EgressError::Interrupted(_)
        )
    )
}
