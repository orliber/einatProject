//! The case view, the chat and the paragraphs of the draft.

use super::*;

impl Core {
    pub fn case_detail(&mut self, case_id: &str) -> Result<CaseDetail, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        // What she usually changes in the AI's wording (ST9); only with the profile on.
        let habits = if style::style_model(self.vault_ref()?)?.is_some() {
            self.style_habits().unwrap_or_default()
        } else {
            dv_style::EditModel::default()
        };
        let v = self.vault_ref()?;
        let meta = v.case_meta(case_id)?;
        let identities = v.identities(case_id)?;
        let inputs = v.inputs(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let routing = Core::material_routing(v, &structure, case_id, &inputs)?;
        let sheets = score_sheets(v, case_id, &inputs)?;
        let style_model = style::style_model(v)?;
        let retention_default = v
            .list_cases()?
            .into_iter()
            .chain(v.list_trash()?)
            .find(|c| c.id == case_id)
            .map(|c| retention::default_until(c.created_at, c.updated_at, c.meta.age.as_ref()))
            .unwrap_or_default();
        let sortable: Vec<&str> = sorting::sortable(&structure)
            .iter()
            .map(|s| s.key.as_str())
            .collect();
        let with_versions = v.drafts_with_versions(case_id)?;
        // One name for one thing (ST10): hers from her past reports, else the report's own.
        let her_terms = style::style_terms(v)?;
        let mut report_texts = Vec::new();
        for s in structure.sections() {
            report_texts.extend(
                v.drafts(case_id, &s.key)?
                    .into_iter()
                    .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
                    .map(|d| d.text_tagged),
            );
        }
        let report_terms = dv_style::terms::usual(report_texts.iter().map(String::as_str), 1);
        let mut sections = Vec::new();
        for part in &structure.parts {
            for s in &part.sections {
                let paragraphs: Vec<ParagraphView> = v
                    .drafts(case_id, &s.key)?
                    .into_iter()
                    .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
                    .map(|d| ParagraphView {
                        has_versions: with_versions.contains(&d.id),
                        id: d.id,
                        text: restore(&d.text_tagged, &identities, practitioner.as_deref()),
                        status: d.status,
                        by_ai: d.author == Author::Ai,
                        sources: d
                            .source_refs
                            .iter()
                            .map(|r| {
                                inputs.iter().find(|i| &i.id == r).map_or_else(
                                    || r.clone(),
                                    |i| format!("{} · {}", i.kind.label_he(), i.title),
                                )
                            })
                            .collect(),
                        // Every score in the text against the score table, each time (AI-6).
                        // And an age that is not the child's age in the details.
                        warnings: dv_domain::check_scores(&d.text_tagged, &sheets)
                            .into_iter()
                            .chain(dv_domain::check_age(&d.text_tagged, meta.age))
                            .chain(dv_style::terms::notes(
                                &d.text_tagged,
                                &her_terms,
                                &report_terms,
                            ))
                            .collect(),
                        style_note: style_model
                            .as_ref()
                            .filter(|_| d.author == Author::Ai && d.status == DraftStatus::Proposed)
                            .and_then(|m| style::style_note(m, &s.key, &d.text_tagged)),
                        style: style_model
                            .as_ref()
                            .filter(|_| d.author == Author::Ai && d.status == DraftStatus::Proposed)
                            .map(|m| {
                                let shown =
                                    |t: &str| restore(t, &identities, practitioner.as_deref());
                                StyleHint {
                                    fit: m.fidelity(&s.key, &d.text_tagged),
                                    details: m.differences(&s.key, &d.text_tagged),
                                    ask: m.closer_ask(&s.key, &d.text_tagged),
                                    marks: habits
                                        .marks(&d.text_tagged)
                                        .into_iter()
                                        .map(|k| StyleMark {
                                            from: shown(&k.from),
                                            to: shown(&k.to),
                                        })
                                        .collect(),
                                }
                            }),
                        replaces: d.replaces,
                    })
                    .collect();
                let source_count =
                    u32::try_from(routing.iter().filter(|r| r.feeds.contains(&s.key)).count())
                        .unwrap_or(0);
                let approved = paragraphs.iter().any(|p| p.status == DraftStatus::Approved);
                sections.push(SectionView {
                    key: s.key.clone(),
                    title: s.title.clone(),
                    part: part.title.clone(),
                    source_count,
                    sortable: sortable.contains(&s.key.as_str()),
                    paragraphs,
                    approved,
                });
            }
        }
        Ok(CaseDetail {
            id: case_id.to_owned(),
            meta,
            identities,
            inputs,
            routing,
            sections,
            retention_default,
        })
    }

    pub fn chat(&mut self, case_id: &str, section_key: &str) -> Result<Vec<ChatView>, CoreError> {
        let v = self.vault_ref()?;
        let identities = v.identities(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        Ok(v.messages(case_id, section_key)?
            .into_iter()
            .map(|m| ChatView {
                role: if m.role == ChatRole::User {
                    "user".to_owned()
                } else {
                    "assistant".to_owned()
                },
                text: restore(&m.text_tagged, &identities, practitioner.as_deref()),
                hidden: m.hidden,
                demo: m.demo,
            })
            .collect())
    }

    pub fn approve_paragraph(&mut self, case_id: &str, draft_id: &str) -> Result<(), CoreError> {
        // Approved as written: what the AI wrote was accepted (learned once, while waiting).
        let draft = style::find_draft(self.vault_ref()?, case_id, draft_id)?
            .filter(|d| d.status == DraftStatus::Proposed);
        self.vault_mut()?
            .set_draft_status(case_id, draft_id, DraftStatus::Approved)?;
        self.learn_from_approval(&draft.into_iter().collect::<Vec<_>>());
        Ok(())
    }

    /// "אישור כל הטיוטה": every paragraph still waiting in the section, as it stands.
    pub fn approve_section(&mut self, case_id: &str, section_key: &str) -> Result<u32, CoreError> {
        let v = self.vault_mut()?;
        let waiting: Vec<dv_domain::DraftParagraph> = v
            .drafts(case_id, section_key)?
            .into_iter()
            .filter(|d| d.status == DraftStatus::Proposed)
            .collect();
        for d in &waiting {
            v.set_draft_status(case_id, &d.id, DraftStatus::Approved)?;
        }
        self.learn_from_approval(&waiting);
        Ok(u32::try_from(waiting.len()).unwrap_or(u32::MAX))
    }

    pub fn reject_paragraph(&mut self, case_id: &str, draft_id: &str) -> Result<(), CoreError> {
        Ok(self
            .vault_mut()?
            .set_draft_status(case_id, draft_id, DraftStatus::Rejected)?)
    }

    /// Manual edit: the text (with real names) is stored tagged.
    pub fn edit_paragraph(
        &mut self,
        case_id: &str,
        draft_id: &str,
        text: &str,
    ) -> Result<(), CoreError> {
        // All the text deleted: the paragraph goes away (D-036).
        if text.trim().is_empty() {
            return self.reject_paragraph(case_id, draft_id);
        }
        let tagged = self.learn(case_id, text)?.tagged;
        let before = style::find_draft(self.vault_ref()?, case_id, draft_id)?;
        self.vault_mut()?.edit_draft(case_id, draft_id, &tagged)?;
        self.learn_from_draft_edit(before, &tagged);
        Ok(())
    }

    /// A paragraph's earlier wordings, newest first, names restored (D-046).
    pub fn paragraph_versions(
        &mut self,
        case_id: &str,
        draft_id: &str,
    ) -> Result<Vec<ParagraphVersionView>, CoreError> {
        let v = self.vault_ref()?;
        let identities = v.identities(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        Ok(v.draft_versions(case_id, draft_id)?
            .into_iter()
            .map(|d| ParagraphVersionView {
                id: d.id,
                saved_at: d.saved_at,
                by_ai: d.author == Author::Ai,
                text: restore(&d.text_tagged, &identities, practitioner.as_deref()),
            })
            .collect())
    }

    /// Bring back an earlier wording; the current one is kept as a version (D-046).
    pub fn restore_paragraph_version(
        &mut self,
        case_id: &str,
        draft_id: &str,
        version_id: &str,
    ) -> Result<(), CoreError> {
        Ok(self
            .vault_mut()?
            .restore_draft_version(case_id, draft_id, version_id)?)
    }

    pub fn add_own_paragraph(
        &mut self,
        case_id: &str,
        section_key: &str,
        text: &str,
    ) -> Result<(), CoreError> {
        self.add_own_paragraph_at(case_id, section_key, text, None)
    }

    /// Her own paragraph, stored tagged and approved. `at`: `None` = at the end of the section,
    /// `Some(None)` = first, `Some(Some(id))` = right after that paragraph (D-036).
    pub fn add_own_paragraph_at(
        &mut self,
        case_id: &str,
        section_key: &str,
        text: &str,
        at: Option<Option<&str>>,
    ) -> Result<(), CoreError> {
        let tagged = self.learn(case_id, text)?.tagged;
        let v = self.vault_mut()?;
        let d = v.add_draft(case_id, section_key, &tagged, Author::User, &[])?;
        v.set_draft_status(case_id, &d.id, DraftStatus::Approved)?;
        if let Some(after) = at {
            v.place_draft(case_id, &d.id, after)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ AI: prepare → approve → send
}
