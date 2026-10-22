//! The check of the whole report (G2, stage two; AI-5; D-097).
//!
//! The local stage reads each paragraph against the details and the score table (age, gender,
//! grade, a recommendation with no ground, every number: `case_detail`). This one reads the
//! report as a whole: once no draft is waiting, ONE request with the approved sections only,
//! filtered and cleared by the gate like every other, asks where two sections contradict each
//! other, where the same thing is said twice, and where a finding is in the summary but not in
//! the body. The answer is a list of notes to read, each pointing at its sections; nothing is
//! rewritten, approved or removed, and no score is judged by the model.
//!
//! Off by default: it is one more request per report, so she turns it on in settings, where
//! its estimated cost is shown first.

use super::*;
use crate::drafts::CLOSING_SECTIONS;

/// "1" when she turned the check on. Absent or anything else: off.
const REPORT_CHECK_KEY: &str = "report_check_stage";

/// A report as long as most: what the cost shown before turning it on is estimated from.
/// About 40,000 characters of Hebrew report and the fixed rules, and a list of notes back.
const TYPICAL_INPUT_TOKENS: u32 = 20_000;
const TYPICAL_OUTPUT_TOKENS: u32 = 2_000;

/// Said in place of a note when a line of the report did not go (D-082).
const LEFT_OUT_REPORT_NOTE: &str =
    "שורה מהדוח לא נשלחה לבדיקה, כי הסינון לא היה בטוח שאין בה שם. מה שכתוב בה לא נבדק.";

/// The switch in settings, with what one check costs (estimated before it is ever turned on).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct ReportCheckSetting {
    pub on: bool,
    /// Estimated cents (US dollars) for one check of a report of usual length, with the model
    /// chosen now. `None`: the model has no known price.
    pub cents_per_check: Option<u32>,
}

/// One note about the report, for the finish screen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct ReportCheckNote {
    pub kind: dv_ai::ReportNoteKind,
    /// The keys of the sections it points at, in report order. Empty: no place to jump to.
    pub sections: Vec<String>,
    /// Plain text, names put back. Shown, never stored or sent.
    pub note: String,
}

/// What the check said, about the sections that were sent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct ReportCheckResult {
    pub notes: Vec<ReportCheckNote>,
    /// The keys of the sections that were read, in report order.
    pub checked: Vec<String>,
    pub demo: bool,
}

impl Core {
    /// Is the check of the whole report on? (Off by default.)
    pub fn report_check_on(&mut self) -> Result<bool, CoreError> {
        Ok(self.vault_ref()?.setting(REPORT_CHECK_KEY)?.as_deref() == Some("1"))
    }

    pub fn set_report_check(&mut self, on: bool) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(REPORT_CHECK_KEY, if on { "1" } else { "0" })?;
        Ok(())
    }

    /// The switch and the cost of one check, for settings.
    pub fn report_check_setting(&mut self) -> Result<ReportCheckSetting, CoreError> {
        let on = self.report_check_on()?;
        let model = self.model_for(Task::Draft)?;
        Ok(ReportCheckSetting {
            on,
            cents_per_check: crate::usage::estimate_cents(
                &model.model,
                TYPICAL_INPUT_TOKENS,
                TYPICAL_OUTPUT_TOKENS,
            ),
        })
    }

    /// Build the one request that reads the approved report as a whole.
    pub fn prepare_report_check(&mut self, case_id: &str) -> Result<Prepared, CoreError> {
        self.prepare_report_check_once(case_id, false)
    }

    fn prepare_report_check_once(
        &mut self,
        case_id: &str,
        learned: bool,
    ) -> Result<Prepared, CoreError> {
        if !self.report_check_on()? {
            return Err(CoreError::Needs(
                "בדיקת הדוח כולו כבויה. אפשר להפעיל אותה בהגדרות.".to_owned(),
                ErrorAction::settings("ai"),
            ));
        }
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let model = self.model_for(Task::Draft)?;
        let data = self.privacy_data(case_id)?;
        let hints = self.case_hints(case_id, &[])?;
        let demo_mode = self.demo_mode()?;

        let (input, review, keys) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let lookups = data.lookups(v);
            let ctx = lookups.masked_context();
            let mut review = Review::default();
            let mut sections = Vec::new();
            let mut keys = Vec::new();
            let mut waiting = Vec::new();
            // The report's sections only: never a letter, never the signature, never a
            // material, a conversation or a draft not approved.
            for s in structure.sections().filter(|s| s.key != "signature") {
                let drafts = v.drafts(case_id, &s.key)?;
                // A new draft waiting for her: the report is not approved yet. A proposed
                // rewrite of an approved paragraph leaves the approved one standing.
                if drafts
                    .iter()
                    .any(|d| d.status == DraftStatus::Proposed && d.replaces.is_none())
                {
                    waiting.push(s.title.clone());
                }
                let text: Vec<String> = drafts
                    .into_iter()
                    .filter(|d| d.status == DraftStatus::Approved)
                    .map(|d| d.text_tagged)
                    .collect();
                if text.is_empty() {
                    continue;
                }
                let out = hints
                    .filter(&text.join("\n"), &ctx)
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                review.add(format!("סעיף מאושר · {}", s.title), &out);
                sections.push(dv_ai::ReportCheckSection {
                    title: model_title(s, &ctx)?,
                    text_tagged: out.tagged,
                    closing: CLOSING_SECTIONS.contains(&s.key.as_str()),
                });
                keys.push(s.key.clone());
            }
            if !waiting.is_empty() {
                return Err(CoreError::Refused(format!(
                    "הדוח נבדק כולו אחרי שכל הטיוטות אושרו. עוד מחכות לאישור: {}.",
                    waiting.join(", ")
                )));
            }
            if sections.len() < 2 {
                return Err(CoreError::Refused(
                    "בדיקת הדוח כולו משווה בין סעיפים, וצריך לפחות שני סעיפים מאושרים.".to_owned(),
                ));
            }
            (dv_ai::ReportCheckInput { sections }, review, keys)
        };

        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let (body, _ids) = dv_ai::build_report_check_request(&model, &input, &nonce);
        let mut prepared = review.into_case_prepared(demo_mode, &data);
        let kind = PendingKind::ReportCheck {
            case_id: case_id.to_owned(),
            section_keys: keys,
        };
        if self.gate(&data, &body, kind, &mut prepared, !learned)? {
            return self.prepare_report_check_once(case_id, true);
        }
        Ok(prepared)
    }

    /// Send exactly what was approved and read the notes (the shell sends off the lock, with
    /// `begin_send` and `finish_report_check`).
    pub fn send_report_check(&mut self, approval_id: &str) -> Result<ReportCheckResult, CoreError> {
        let out = self.begin_send(approval_id, SendKind::ReportCheck)?;
        let response = out.transmit();
        self.finish_report_check(out, response)
    }

    pub fn finish_report_check(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<ReportCheckResult, CoreError> {
        let left_out = crate::send::had_left_out(&out);
        let crate::stage::Received {
            kind,
            response,
            demo,
            ..
        } = self.receive(out, response, crate::stage::Stage::ReportCheck)?;
        let PendingKind::ReportCheck {
            case_id,
            section_keys,
        } = kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let ids: Vec<String> = (1..=section_keys.len()).map(|i| format!("R{i}")).collect();
        let raw = dv_ai::parse_report_check(&response, &ids)?;
        let v = self.vault_ref()?;
        let identities = v.identities(&case_id)?;
        let masks = v.masks(&case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let key_of = |id: &str| {
            ids.iter()
                .position(|i| i == id)
                .and_then(|n| section_keys.get(n).cloned())
        };
        let mut notes: Vec<ReportCheckNote> = raw
            .into_iter()
            .map(|n| ReportCheckNote {
                kind: n.kind,
                sections: n.sections.iter().filter_map(|s| key_of(s)).collect(),
                note: restore(&n.note, &identities, practitioner.as_deref(), &masks),
            })
            .collect();
        if left_out {
            notes.push(ReportCheckNote {
                kind: dv_ai::ReportNoteKind::Other,
                sections: Vec::new(),
                note: LEFT_OUT_REPORT_NOTE.to_owned(),
            });
        }
        Ok(ReportCheckResult {
            notes,
            checked: section_keys,
            demo,
        })
    }
}
