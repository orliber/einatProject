//! Cases, their materials and the score sheets.

use super::*;

/// The id of the made-up case (U3).
const SAMPLE_KEY: &str = "sample_case_id";

/// The made-up case's materials. Every word is invented.
const SAMPLE_INPUTS: &[(InputKind, &str, &str)] = &[
    (
        InputKind::Intake,
        "שיחה עם ההורים",
        "ההורים פנו בשל קושי של תמר להישאר בפעילות אחת לאורך זמן בגן. לדבריהם היא ילדה חברותית ושמחה, אוהבת לצייר ולשמוע סיפורים. ההתפתחות המוטורית והשפתית תקינה לדבריהם. בבית היא מתקשה לסיים משימה כשיש רעש סביבה, ומבקשת עזרה בהתארגנות בבוקר.",
    ),
    (
        InputKind::Kindergarten,
        "שיחה עם הגננת",
        "הגננת מתארת ילדה נעימה ומעורבת חברתית. במפגש הבוקר היא מתקשה לשבת עד הסוף וקמה כמה פעמים. בפינת היצירה היא מתמידה זמן רב. היא נעזרת בתזכורת קצרה כדי לעבור בין פעילויות.",
    ),
    (
        InputKind::Observation,
        "התרשמות מהמפגש",
        "תמר נכנסה לחדר בחיוך ונענתה בשמחה למשימות. במשימות קצרות עבדה בריכוז ובקצב טוב. במשימות ארוכות יותר הסתכלה סביב ונזקקה להכוונה כדי לחזור למשימה. היא הגיבה היטב לעידוד ולחלוקת המשימה לשלבים.",
    ),
];

impl Core {
    pub fn list_cases(&mut self) -> Result<Vec<CaseSummary>, CoreError> {
        let cases = self.vault_ref()?.list_cases()?;
        self.with_failures(cases)
    }

    pub fn create_case(
        &mut self,
        mut meta: CaseMeta,
        identities: Vec<IdentityInput>,
    ) -> Result<String, CoreError> {
        check_meta(&meta)?;
        // Every new case is in the institute's report (D-066); a template is never chosen
        // from outside.
        meta.template = Some(dv_domain::INSTITUTE_TEMPLATE.to_owned());
        // A case's own guide lines are written in it, one section at a time.
        meta.guides = None;
        meta.dsm_severity = None;
        meta.letter_left_out = None;
        let v = self.vault_mut()?;
        let id = v.create_case(&meta)?;
        v.set_identities(&id, &identities)?;
        Ok(id)
    }

    /// A made-up case to try the program on (U3): an invented child and three invented
    /// materials. Opened once; while it is still in the case list, the same one is returned.
    pub fn create_sample_case(&mut self) -> Result<String, CoreError> {
        let existing = self.vault_ref()?.setting(SAMPLE_KEY)?;
        if let Some(id) = existing {
            let live = self
                .vault_ref()?
                .list_cases()?
                .iter()
                .any(|c| c.id == id && c.deleted_at.is_none());
            if live {
                return Ok(id);
            }
        }
        let meta = CaseMeta {
            code: "דוגמה".into(),
            age: Some(dv_domain::Age {
                years: 5,
                months: 6,
            }),
            child_gender: Some(dv_domain::GrammaticalGender::Female),
            current_section: None,
            retention_until: None,
            consent: Some(dv_domain::Consent {
                given_on: dates::iso(dates::today()),
                form_version: "תיק לדוגמה".into(),
                given_by: "תיק בדוי".into(),
            }),
            follows: None,
            template: None,
            guides: None,
            dsm_severity: None,
            letter_left_out: None,
        };
        let id = self.create_case(
            meta,
            vec![IdentityInput {
                id: None,
                role: dv_domain::Role::Child,
                value: "תמר".into(),
                aliases: vec![],
            }],
        )?;
        for (kind, title, text) in SAMPLE_INPUTS {
            self.add_input(&id, *kind, title, text)?;
        }
        self.vault_mut()?.set_setting(SAMPLE_KEY, &id)?;
        Ok(id)
    }

    /// Is `case_id` the made-up case?
    pub(crate) fn is_sample(&mut self, case_id: &str) -> Result<bool, CoreError> {
        Ok(self.vault_ref()?.setting(SAMPLE_KEY)?.as_deref() == Some(case_id))
    }

    pub fn update_case(&mut self, case_id: &str, mut meta: CaseMeta) -> Result<(), CoreError> {
        check_meta(&meta)?;
        // Which assessment a follow-up belongs to, and the case's template, are set when it is
        // opened, never edited. Its guide lines have their own command (`set_case_guide`).
        let kept = self.vault_ref()?.case_meta(case_id)?;
        meta.follows = kept.follows;
        meta.template = kept.template;
        meta.guides = kept.guides;
        meta.dsm_severity = kept.dsm_severity;
        meta.letter_left_out = kept.letter_left_out;
        self.vault_mut()?.update_case_meta(case_id, &meta)?;
        self.forget_pending(case_id);
        Ok(())
    }

    /// The approved recommendations left out of the letters (round 44): the paragraph ids
    /// she unmarked, or `None` for none. Kept as what she left out, so a recommendation
    /// approved or rewritten later is in until she unmarks it (review of stage 6, R2). An id
    /// that is not an approved recommendation of the case is refused.
    pub fn set_letter_left_out(
        &mut self,
        case_id: &str,
        ids: Option<Vec<String>>,
    ) -> Result<(), CoreError> {
        let v = self.vault_ref()?;
        if let Some(ids) = &ids {
            let approved: Vec<String> = v
                .drafts(case_id, "recommendations")?
                .into_iter()
                .filter(|d| d.status == DraftStatus::Approved)
                .map(|d| d.id)
                .collect();
            if ids.iter().any(|id| !approved.contains(id)) {
                return Err(CoreError::NotFound("ההמלצה".to_owned()));
            }
        }
        let mut meta = v.case_meta(case_id)?;
        meta.letter_left_out = ids;
        self.vault_mut()?.update_case_meta(case_id, &meta)?;
        self.forget_pending(case_id);
        Ok(())
    }

    /// The DSM-5 severity she set for the case (round 43), one level (1–3) for each domain.
    /// `None` for both: not set. A level outside 1–3 is refused.
    pub fn set_dsm_severity(
        &mut self,
        case_id: &str,
        social: Option<u8>,
        restricted: Option<u8>,
    ) -> Result<(), CoreError> {
        let severity = dv_domain::DsmSeverity::new(social, restricted);
        if severity.social != social || severity.restricted != restricted {
            return Err(CoreError::Refused("רמת חומרה היא 1, 2 או 3.".to_owned()));
        }
        let v = self.vault_ref()?;
        let mut meta = v.case_meta(case_id)?;
        meta.dsm_severity = severity.is_set().then_some(severity);
        self.vault_mut()?.update_case_meta(case_id, &meta)?;
        self.forget_pending(case_id);
        Ok(())
    }

    pub fn set_identities(
        &mut self,
        case_id: &str,
        identities: Vec<IdentityInput>,
    ) -> Result<Vec<dv_domain::Identity>, CoreError> {
        // The vault skips a person with no name. Here that is refused when it would take a
        // name out of the filter without her saying so: a person she already has, saved with
        // the name emptied, or a list that leaves the case with no child when it had one. The
        // window refuses this too; the rule is kept here, where every caller passes.
        let nameless = |i: &IdentityInput| i.value.trim().is_empty();
        if identities.iter().any(|i| i.id.is_some() && nameless(i)) {
            return Err(CoreError::Refused(
                "שם לא יכול להישאר ריק. כדי להוציא אדם מהתיק מוחקים אותו מהרשימה.".to_owned(),
            ));
        }
        let had_child = self
            .vault_ref()?
            .identities(case_id)?
            .iter()
            .any(|i| i.role == Role::Child);
        let has_child = identities
            .iter()
            .any(|i| i.role == Role::Child && !nameless(i));
        if had_child && !has_child {
            return Err(CoreError::Refused(
                "אי אפשר לשמור תיק בלי שם הילד: בלי השם הוא לא יוסתר בטקסטים שיוצאים.".to_owned(),
            ));
        }
        let saved = self.vault_mut()?.set_identities(case_id, &identities)?;
        self.forget_pending(case_id);
        Ok(saved)
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
            .map(|i| (i.title, i.content));
        self.vault_mut()?
            .update_input(case_id, input_id, title, content)?;
        if before.as_ref().map(|(_, c)| c.as_str()) != Some(content) {
            self.forget_sorting(case_id, input_id)?;
        }
        // She may have taken a sentence out: nothing prepared with the old text goes, neither
        // a request waiting to be sent nor one still being filtered.
        if before.as_ref().map(|(t, c)| (t.as_str(), c.as_str())) != Some((title, content)) {
            self.withdraw_for_material(case_id);
        }
        self.learn_input(case_id, title, content)
    }

    /// The exact text [`Core::save_scores`] would store, for the live preview while typing.
    /// Not for an instrument she does not give (D-098).
    pub fn preview_scores(&mut self, sheet: &dv_domain::ScoreSheet) -> Result<String, CoreError> {
        self.preview_scores_in(None, sheet)
    }

    /// [`Core::preview_scores`] for the sheet of a case: with the child's age, as it will be
    /// saved (D-190).
    pub fn preview_scores_in(
        &mut self,
        case_id: Option<&str>,
        sheet: &dv_domain::ScoreSheet,
    ) -> Result<String, CoreError> {
        if let Ok(v) = self.vault_ref() {
            crate::instruments::check_given(v, &sheet.instrument)?;
        }
        let sheet = self.with_basis(case_id, sheet)?;
        dv_domain::format_sheet_with(&sheet, self.sheet_habits()?).map_err(CoreError::Refused)
    }

    /// The sheet with what its relative places are computed with recorded on it (D-190): her
    /// critical values now and the case's age. Whatever basis came with the sheet is replaced;
    /// none while locked.
    fn with_basis(
        &mut self,
        case_id: Option<&str>,
        sheet: &dv_domain::ScoreSheet,
    ) -> Result<dv_domain::ScoreSheet, CoreError> {
        let mut sheet = sheet.clone();
        sheet.basis = match self.vault_ref() {
            Ok(v) => {
                let age_months = match case_id {
                    Some(id) => v
                        .case_meta(id)?
                        .age
                        .map(|a| u32::from(a.years) * 12 + u32::from(a.months)),
                    None => None,
                };
                Some(dv_domain::RelativeBasis {
                    critical: crate::instruments::critical_values(v)?,
                    age_months,
                })
            }
            Err(_) => None,
        };
        Ok(sheet)
    }

    /// What she leaves out beside scores in her reports (K12); nothing while locked.
    fn sheet_habits(&mut self) -> Result<dv_domain::SheetHabits, CoreError> {
        match self.vault_ref() {
            Ok(v) => crate::style::sheet_habits(v),
            Err(_) => Ok(dv_domain::SheetHabits::default()),
        }
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
        // An instrument she does not give is not entered (D-098).
        crate::instruments::check_given(self.vault_ref()?, &sheet.instrument)?;
        // The basis is stored with the sheet, so its text and every later use of it agree.
        let sheet = &self.with_basis(Some(case_id), sheet)?;
        let text = dv_domain::format_sheet_with(sheet, self.sheet_habits()?)
            .map_err(CoreError::Refused)?;
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
        if input_id.is_some() {
            // As with any material changed: nothing prepared with the old scores goes.
            self.withdraw_for_material(case_id);
        }
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
            // A request that holds the deleted material does not go.
            self.withdraw_for_material(case_id);
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
