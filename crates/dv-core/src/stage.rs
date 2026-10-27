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
        let (case_id, kept_as, details) = out.pending.kind.trace(demo, ai);
        let v = self.vault_mut()?;
        if let (Some(case_id), Some(kept_as)) = (case_id, kept_as) {
            let payload = &out.pending.payload;
            let payload_text = String::from_utf8_lossy(payload.body()).into_owned();
            v.add_transmission(
                case_id,
                kept_as,
                payload.sha256(),
                if demo { "demo" } else { &out.model },
                &payload_text,
            )?;
        }
        v.record(AuditEvent::Send, case_id, &details)?;
        Ok(())
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
        )
    )
}
