//! The "check against the sources" stage (design: plans/ארכיטקטורה - פיצול הליבה וצנרת שלבים).
//!
//! A separate request after drafting: the proposed paragraphs and the sources they were
//! written from go out together, filtered and cleared like any other request, with an
//! approval of its own. The answer only adds a verdict per paragraph; nothing is rewritten,
//! approved or removed. It is on unless she turns it off (round 41): it runs by itself in the
//! background after "write the whole report", and its cost shows in the usage page.

use super::*;
use crate::send::collect_sources;

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

    /// Build the request that checks the section's proposed paragraphs against its sources.
    pub fn prepare_verify(
        &mut self,
        case_id: &str,
        section_key: &str,
    ) -> Result<Prepared, CoreError> {
        self.prepare_verify_once(case_id, section_key, false)
    }

    fn prepare_verify_once(
        &mut self,
        case_id: &str,
        section_key: &str,
        learned: bool,
    ) -> Result<Prepared, CoreError> {
        if !self.verify_stage_on()? {
            return Err(CoreError::Needs(
                "הבדיקה מול המקורות כבויה. אפשר להפעיל אותה בהגדרות.".to_owned(),
                ErrorAction::settings("ai"),
            ));
        }
        if DERIVED_SECTIONS.contains(&section_key) || Letter::of(section_key).is_some() {
            return Err(CoreError::Refused(
                "הבדיקה מול המקורות מיועדת לסעיפים שנכתבים מחומרי התיק.".to_owned(),
            ));
        }
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let section = structure
            .section(section_key)
            .ok_or_else(|| CoreError::NotFound(section_key.to_owned()))?
            .clone();
        // Her own sections (round 30, D-068): nothing of the case is sent for them, the check
        // against the sources included (the review of stage 3).
        if section.mode == dv_domain::SectionMode::Own {
            return Err(CoreError::Refused(
                "את כותבת את הסעיף הזה בעצמך, ולכן הוא לא נבדק מול החומרים ושום חומר לא נשלח בשבילו."
                    .to_owned(),
            ));
        }
        let model = self.model_for(Task::Draft)?;
        let data = self.privacy_data(case_id)?;
        let hints = self.case_hints(case_id, &[])?;
        let demo_mode = self.demo_mode()?;

        let (input, review, draft_ids) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let lookups = data.lookups(v);
            let ctx = lookups.masked_context();
            let mut review = Review::default();
            let (sources, _rows) = collect_sources(
                v,
                &structure,
                &ctx,
                &hints,
                case_id,
                section_key,
                &mut review,
            )?;
            if sources.is_empty() {
                return Err(CoreError::Refused(
                    "אין לסעיף הזה חומרים לבדוק מולם.".to_owned(),
                ));
            }
            let mut paragraphs = Vec::new();
            let mut draft_ids = Vec::new();
            for d in v
                .drafts(case_id, section_key)?
                .into_iter()
                .filter(|d| d.status == DraftStatus::Proposed)
            {
                let out =
                    filter(&d.text_tagged, &ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
                review.add("פסקה לבדיקה".to_owned(), &out);
                paragraphs.push(out.tagged);
                draft_ids.push(d.id);
            }
            if paragraphs.is_empty() {
                return Err(CoreError::Refused(
                    "אין טיוטות מוצעות לבדיקה בסעיף הזה.".to_owned(),
                ));
            }
            let input = dv_ai::VerifyInput {
                section_title: model_title(&section, &ctx)?,
                sources,
                paragraphs,
            };
            (input, review, draft_ids)
        };

        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let (body, _paragraph_ids, source_ids) =
            dv_ai::build_verify_request(&model, &input, &nonce);
        let mut prepared = review.into_case_prepared(demo_mode, &data);
        let kind = PendingKind::Verify {
            case_id: case_id.to_owned(),
            section_key: section_key.to_owned(),
            draft_ids,
            source_ids,
        };
        if self.gate(&data, &body, kind, &mut prepared, !learned)? {
            return self.prepare_verify_once(case_id, section_key, true);
        }
        Ok(prepared)
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
        Ok(VerifyResult {
            draft_ids,
            checks,
            demo,
        })
    }
}
