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
    ///    anything about the reply can fail.
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
                self.restore_pending(out);
                return Err(e);
            }
        };
        self.note_usage(&out.pending.payload, &response, demo);
        let Outgoing { pending, model, .. } = out;
        let Pending { payload, kind } = pending;
        let ai = provider_of(&model).display_name();
        let (case_id, kept_as, details) = kind.trace(demo, ai);
        let v = self.vault_mut()?;
        if let (Some(case_id), Some(kept_as)) = (case_id, kept_as) {
            let payload_text = String::from_utf8_lossy(payload.body()).into_owned();
            v.add_transmission(
                case_id,
                kept_as,
                payload.sha256(),
                if demo { "demo" } else { &model },
                &payload_text,
            )?;
        }
        v.record(AuditEvent::Send, case_id, &details)?;
        Ok(Received {
            kind,
            response,
            demo,
            model,
        })
    }
}
