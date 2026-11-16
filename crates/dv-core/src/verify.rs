//! The "check against the sources" stage (design: plans/ארכיטקטורה - פיצול הליבה וצנרת שלבים).
//!
//! A separate request after drafting: the proposed paragraphs and the sources they were
//! written from go out together, filtered and cleared like any other request, with an
//! approval of its own. The answer only adds a verdict per paragraph; nothing is rewritten,
//! approved or removed. It is on unless she turns it off (round 41): it runs by itself in the
//! background after "write the whole report", and its cost shows in the usage page.

use super::*;

/// "0" when she turned the check off.
const VERIFY_KEY: &str = "verify_stage";

/// The answer of the check, for the paragraphs that were sent (in the order of `draft_ids`;
/// `checks[i].paragraph` is `P{i+1}`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct VerifyResult {
    pub draft_ids: Vec<String>,
    pub checks: Vec<dv_ai::ParagraphCheck>,
    pub demo: bool,
}

impl Core {
    /// Is the check against the sources on? (On by default.)
    pub fn verify_stage_on(&mut self) -> Result<bool, CoreError> {
        Ok(self.vault_ref()?.setting(VERIFY_KEY)?.as_deref() != Some("0"))
    }

    pub fn set_verify_stage(&mut self, on: bool) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(VERIFY_KEY, if on { "1" } else { "0" })?;
        Ok(())
    }

    /// Send exactly what was approved and read the verdicts.
    pub fn send_verify(&mut self, approval_id: &str) -> Result<VerifyResult, CoreError> {
        let out = self.begin_send(approval_id, SendKind::Verify)?;
        let response = out.transmit();
        self.finish_verify(out, response)
    }

    pub fn finish_verify(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<VerifyResult, CoreError> {
        let left_out = crate::send::had_left_out(&out);
        let crate::stage::Received {
            kind,
            response,
            demo,
            ..
        } = self.receive(out, response, crate::stage::Stage::Verify)?;
        let PendingKind::Verify {
            case_id,
            section_key: _,
            draft_ids,
            wordings,
            source_ids,
        } = kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let v = self.vault_mut()?;
        let paragraph_ids: Vec<String> = (1..=draft_ids.len()).map(|i| format!("P{i}")).collect();
        let mut checks = dv_ai::parse_verify(&response, &paragraph_ids, &source_ids)?;
        // Notes may carry tags; she reads the names. A note is shown, never stored or sent.
        let identities = v.identities(&case_id)?;
        let masks = v.masks(&case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        for c in &mut checks {
            for n in &mut c.notes {
                n.note = restore(&n.note, &identities, practitioner.as_deref(), &masks);
            }
            // A source line left out (D-082) may be why a paragraph is not supported.
            if left_out && c.verdict != dv_ai::Verdict::Supported {
                c.notes.push(dv_ai::VerifyNote {
                    note: crate::send::LEFT_OUT_NOTE.to_owned(),
                    source_refs: Vec::new(),
                });
            }
        }
        // D-161: the verdicts are kept for the session with the paragraphs, by the stored
        // wording each has now, so the report page shows them after any reload and "approve
        // all" sees them.
        // A verdict is about the wording that was sent: a paragraph worded again or edited
        // while the check was out was not checked, and the verdict says nothing about it.
        let mut said: Vec<(String, String, Vec<ParagraphNote>)> = Vec::new();
        for ((draft_id, sent), c) in draft_ids.iter().zip(&wordings).zip(&mut checks) {
            let Some(d) = style::find_draft(v, &case_id, draft_id)? else {
                continue;
            };
            if d.text_tagged != *sent {
                c.verdict = dv_ai::Verdict::Unchecked;
                c.notes.clear();
                continue;
            }
            // The demo check reads for numbers only and calls everything else "partial":
            // that is "not read", not a finding, and it holds nothing back.
            let notes = if demo && c.verdict == dv_ai::Verdict::Partial {
                Vec::new()
            } else {
                verdict_notes(c)
            };
            said.push((draft_id.clone(), d.text_tagged, notes));
        }
        for (draft_id, wording, notes) in said {
            self.notes.keep_verdict(&case_id, &draft_id, wording, notes);
        }
        Ok(VerifyResult {
            draft_ids,
            checks,
            demo,
        })
    }
}

/// A verdict worth her look, as notes under the paragraph: what the check wrote, or one
/// plain line when it wrote nothing. Supported and not checked say nothing.
fn verdict_notes(check: &dv_ai::ParagraphCheck) -> Vec<ParagraphNote> {
    let (kind, plain) = match check.verdict {
        dv_ai::Verdict::Contradicted => (
            NoteKind::VerifyContradicted,
            "הפסקה סותרת את מה שכתוב בחומרים",
        ),
        dv_ai::Verdict::Unsupported => (NoteKind::VerifyUnsupported, "לא נמצא לפסקה בסיס בחומרים"),
        dv_ai::Verdict::Partial => (NoteKind::VerifyPartial, "רק חלק מהפסקה נשען על החומרים"),
        dv_ai::Verdict::Supported | dv_ai::Verdict::Unchecked => return Vec::new(),
    };
    let mut notes: Vec<ParagraphNote> = Vec::new();
    for n in &check.notes {
        let note = ParagraphNote::new(kind, n.note.trim());
        if !note.text.is_empty() && !notes.contains(&note) {
            notes.push(note);
        }
    }
    if notes.is_empty() {
        notes.push(ParagraphNote::new(kind, plain));
    }
    notes
}
