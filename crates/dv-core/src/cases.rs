//! Cases, their materials and the score sheets.

use super::*;

impl Core {
    pub fn list_cases(&mut self) -> Result<Vec<CaseSummary>, CoreError> {
        let cases = self.vault_ref()?.list_cases()?;
        self.with_failures(cases)
    }

    pub fn create_case(
        &mut self,
        meta: CaseMeta,
        identities: Vec<IdentityInput>,
    ) -> Result<String, CoreError> {
        check_meta(&meta)?;
        let v = self.vault_mut()?;
        let id = v.create_case(&meta)?;
        v.set_identities(&id, &identities)?;
        Ok(id)
    }

    pub fn update_case(&mut self, case_id: &str, mut meta: CaseMeta) -> Result<(), CoreError> {
        check_meta(&meta)?;
        // Which assessment a follow-up belongs to is set when it is opened, never edited.
        meta.follows = self.vault_ref()?.case_meta(case_id)?.follows;
        Ok(self.vault_mut()?.update_case_meta(case_id, &meta)?)
    }

    pub fn set_identities(
        &mut self,
        case_id: &str,
        identities: Vec<IdentityInput>,
    ) -> Result<Vec<dv_domain::Identity>, CoreError> {
        Ok(self.vault_mut()?.set_identities(case_id, &identities)?)
    }

    pub fn add_input(
        &mut self,
        case_id: &str,
        kind: InputKind,
        title: &str,
        content: &str,
    ) -> Result<dv_domain::CaseInput, CoreError> {
        let input = self.vault_mut()?.add_input(case_id, kind, title, content)?;
        self.learn_input(case_id, title, content)?;
        Ok(input)
    }

    /// A material's new names are kept with the case when it is saved, so every later text
    /// (a request, another material) hides them under the same tag without asking.
    pub(crate) fn learn_input(
        &mut self,
        case_id: &str,
        title: &str,
        content: &str,
    ) -> Result<(), CoreError> {
        self.learn(case_id, title)?;
        self.learn(case_id, content)?;
        Ok(())
    }

    pub fn update_input(
        &mut self,
        case_id: &str,
        input_id: &str,
        title: &str,
        content: &str,
    ) -> Result<(), CoreError> {
        let before = self
            .vault_ref()?
            .inputs(case_id)?
            .into_iter()
            .find(|i| i.id == input_id)
            .map(|i| i.content);
        self.vault_mut()?
            .update_input(case_id, input_id, title, content)?;
        if before.as_deref() != Some(content) {
            self.forget_sorting(case_id, input_id)?;
        }
        self.learn_input(case_id, title, content)
    }

    /// The instruments with their fixed tables (knowledge/instruments.md).
    #[must_use]
    pub fn score_instruments() -> Vec<dv_domain::Instrument> {
        dv_domain::instruments()
    }

    /// The exact text [`Core::save_scores`] would store, for the live preview while typing.
    pub fn preview_scores(sheet: &dv_domain::ScoreSheet) -> Result<String, CoreError> {
        dv_domain::format_sheet(sheet).map_err(CoreError::Refused)
    }

    /// Save entered scores as a "test scores" material: the text (with ranges computed from the
    /// fixed tables) is what feeds the sections; the sheet itself is kept sealed for editing.
    pub fn save_scores(
        &mut self,
        case_id: &str,
        input_id: Option<&str>,
        sheet: &dv_domain::ScoreSheet,
    ) -> Result<dv_domain::CaseInput, CoreError> {
        if sheet.entries.is_empty() {
            return Err(CoreError::Refused("לא הוזנו ציונים.".into()));
        }
        let text = dv_domain::format_sheet(sheet).map_err(CoreError::Refused)?;
        let title = dv_domain::instruments()
            .into_iter()
            .find(|i| i.key == sheet.instrument)
            .map(|i| format!("ציוני {}", i.name))
            .unwrap_or_default();
        let data = serde_json::to_string(sheet).map_err(|e| CoreError::Internal(e.to_string()))?;
        let v = self.vault_mut()?;
        let input = match input_id {
            Some(id) => {
                let existing = v
                    .inputs(case_id)?
                    .into_iter()
                    .find(|i| i.id == id && i.kind == InputKind::TestScores)
                    .ok_or_else(|| CoreError::NotFound("טבלת הציונים".into()))?;
                v.update_input(case_id, id, &title, &text)?;
                dv_domain::CaseInput {
                    title,
                    content: text,
                    ..existing
                }
            }
            None => v.add_input(case_id, InputKind::TestScores, &title, &text)?,
        };
        v.set_input_data(case_id, &input.id, &data)?;
        self.learn_input(case_id, &input.title, &input.content)?;
        Ok(input)
    }

    /// The sheet behind a scores material, if it was entered in the score table.
    pub fn score_sheet(
        &mut self,
        case_id: &str,
        input_id: &str,
    ) -> Result<Option<dv_domain::ScoreSheet>, CoreError> {
        let Some(data) = self.vault_ref()?.input_data(case_id, input_id)? else {
            return Ok(None);
        };
        serde_json::from_str(&data)
            .map(Some)
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Deletes a material. The last one deleted can be put back with `restore_input` until
    /// another is deleted or the vault locks; it is held in memory only, still sealed.
    pub fn delete_input(&mut self, case_id: &str, input_id: &str) -> Result<(), CoreError> {
        let removed = self.vault_mut()?.take_input(case_id, input_id)?;
        if removed.is_some() {
            self.removed_input = removed;
        }
        Ok(())
    }

    /// Undoes the last `delete_input` of this material.
    pub fn restore_input(&mut self, case_id: &str, input_id: &str) -> Result<(), CoreError> {
        let held = self
            .removed_input
            .take_if(|r| r.case_id == case_id && r.id == input_id);
        let Some(row) = held else {
            return Err(CoreError::Refused(
                "אי אפשר להחזיר את החומר הזה עוד. אפשר להוסיף אותו מחדש.".to_owned(),
            ));
        };
        self.vault_mut()?.restore_input(&row)?;
        Ok(())
    }
}
