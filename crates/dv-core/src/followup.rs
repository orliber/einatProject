//! Follow-up assessments (D-029): a new case linked to the one before. The names come along
//! (the same family), the consent does not (a new assessment needs its own), and the scores
//! of both are compared by fixed rules.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use dv_domain::{
    compare, comparison_text, CaseMeta, ComparisonRow, IdentityInput, InputKind, ScoreSheet,
};

use crate::{Core, CoreError};

/// Title of the comparison material; saving again replaces it.
const COMPARISON_TITLE: &str = "השוואה לאבחון הקודם";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FollowUpView {
    pub previous_id: String,
    pub previous_code: String,
    /// The previous assessment is in the recycle bin or erased: nothing to compare.
    pub previous_gone: bool,
    pub rows: Vec<ComparisonRow>,
    /// The comparison is already a material of this case.
    pub in_materials: bool,
}

impl Core {
    fn sheets(&mut self, case_id: &str) -> Result<Vec<ScoreSheet>, CoreError> {
        let v = self.vault_ref()?;
        let mut out = Vec::new();
        for i in v.inputs(case_id)? {
            if i.kind != InputKind::TestScores {
                continue;
            }
            if let Some(data) = v.input_data(case_id, &i.id)? {
                if let Ok(sheet) = serde_json::from_str::<ScoreSheet>(&data) {
                    out.push(sheet);
                }
            }
        }
        Ok(out)
    }

    /// Open a follow-up assessment of `previous_id`: same names and folder, a new consent.
    pub fn create_follow_up(&mut self, previous_id: &str) -> Result<String, CoreError> {
        let v = self.vault_ref()?;
        let before = v.case_meta(previous_id)?;
        let people: Vec<IdentityInput> = v
            .identities(previous_id)?
            .into_iter()
            .map(|i| IdentityInput {
                id: None,
                role: i.role,
                value: i.value,
                aliases: i.aliases,
            })
            .collect();
        let folder = v
            .list_cases()?
            .into_iter()
            .find(|c| c.id == previous_id)
            .and_then(|c| c.folder_id);
        let meta = CaseMeta {
            code: format!("{} · מעקב", before.code),
            child_gender: before.child_gender,
            follows: Some(previous_id.to_owned()),
            ..CaseMeta::default()
        };
        let id = self.create_case(meta, people)?;
        if let Some(f) = folder {
            self.vault_mut()?.move_case(&id, Some(&f))?;
        }
        Ok(id)
    }

    /// The comparison with the assessment before, for a follow-up case.
    pub fn follow_up(&mut self, case_id: &str) -> Result<Option<FollowUpView>, CoreError> {
        let v = self.vault_ref()?;
        let Some(previous_id) = v.case_meta(case_id)?.follows else {
            return Ok(None);
        };
        let in_materials = v
            .inputs(case_id)?
            .iter()
            .any(|i| i.title == COMPARISON_TITLE);
        let alive = v.list_cases()?.iter().any(|c| c.id == previous_id);
        if !alive {
            return Ok(Some(FollowUpView {
                previous_id,
                previous_code: String::new(),
                previous_gone: true,
                rows: Vec::new(),
                in_materials,
            }));
        }
        let previous_code = v.case_meta(&previous_id)?.code;
        let rows = compare(&self.sheets(&previous_id)?, &self.sheets(case_id)?);
        Ok(Some(FollowUpView {
            previous_id,
            previous_code,
            previous_gone: false,
            rows,
            in_materials,
        }))
    }

    /// Put the comparison into the case's materials (replacing an earlier one), so the
    /// sections are written from the same numbers she sees.
    pub fn add_comparison_material(
        &mut self,
        case_id: &str,
    ) -> Result<dv_domain::CaseInput, CoreError> {
        let view = self
            .follow_up(case_id)?
            .filter(|f| !f.rows.is_empty())
            .ok_or_else(|| {
                CoreError::Refused("אין ציונים משותפים לשני האבחונים להשוואה.".to_owned())
            })?;
        let text = comparison_text(&view.rows);
        let existing = self
            .vault_ref()?
            .inputs(case_id)?
            .into_iter()
            .find(|i| i.title == COMPARISON_TITLE);
        match existing {
            Some(i) => {
                self.update_input(case_id, &i.id, COMPARISON_TITLE, &text)?;
                Ok(dv_domain::CaseInput { content: text, ..i })
            }
            None => self.add_input(case_id, InputKind::TestScores, COMPARISON_TITLE, &text),
        }
    }
}
