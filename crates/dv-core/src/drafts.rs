//! The case view, the chat and the paragraphs of the draft.

use super::*;

/// Sections that sum up the body rather than add to it (G2).
pub(crate) const CLOSING_SECTIONS: &[&str] = &[
    "summary",
    "diagnoses",
    "recommendations",
    "signature",
    "dsm",
];

impl Core {
    pub fn case_detail(&mut self, case_id: &str) -> Result<CaseDetail, CoreError> {
        let structure = structure_for(self.vault_ref()?, case_id)?;
        // Each case she opens is in the log once a session (L3; regulation 10's access record).
        if !self.opened_cases.contains(case_id) {
            self.vault_mut()?.record(
                AuditEvent::CaseOpened,
                Some(case_id),
                &serde_json::json!({}),
            )?;
            self.opened_cases.insert(case_id.to_owned());
        }
        // What she usually changes in the AI's wording (ST9); only with the profile on.
        let habits = if style::style_model(self.vault_ref()?)?.is_some() {
            self.style_habits().unwrap_or_default()
        } else {
            dv_style::EditModel::default()
        };
        let sample = self.is_sample(case_id)?;
        let v = self.vault_ref()?;
        let meta = v.case_meta(case_id)?;
        let identities = v.identities(case_id)?;
        let masks = v.masks(case_id)?;
        let inputs = v.inputs(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let routing = Core::material_routing(v, &structure, case_id, &inputs)?;
        // Round 31: what no material feeds yet, and the order of the previous assessments.
        let missing: Vec<String> = sorting::sortable(&structure)
            .iter()
            .filter(|s| !routing.iter().any(|r| r.feeds.contains(&s.key)))
            .map(|s| s.title.clone())
            .collect();
        let with_record: Vec<&dv_domain::CaseInput> = inputs
            .iter()
            .filter(|i| {
                routing
                    .iter()
                    .any(|r| r.input_id == i.id && r.record.is_some())
            })
            .collect();
        let keys = with_record
            .iter()
            .map(|i| sorting::prior_key(v, case_id, i))
            .collect::<Result<Vec<_>, _>>()?;
        let prior_order: Vec<String> = dv_domain::record_order(&keys)
            .into_iter()
            .filter_map(|k| with_record.get(k).map(|i| i.id.clone()))
            .collect();
        let sheets = score_sheets(v, case_id, &inputs)?;
        let style_model = style::style_model(v)?;
        let interpretations = style::InterpretationCheck::new(v, &sheets)?;
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
        // Each section's paragraphs are read (and decrypted) once, for the checks across the
        // report and for the view (deep review 3.6).
        let mut shown: std::collections::HashMap<String, Vec<dv_domain::DraftParagraph>> =
            std::collections::HashMap::new();
        let mut report: Vec<(String, String, String)> = Vec::new();
        for s in structure.sections() {
            let drafts: Vec<_> = v
                .drafts(case_id, &s.key)?
                .into_iter()
                .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
                .collect();
            report.extend(
                drafts
                    .iter()
                    .map(|d| (s.key.clone(), d.id.clone(), d.text_tagged.clone())),
            );
            shown.insert(s.key.clone(), drafts);
        }
        let report_texts: Vec<String> = report.iter().map(|(_, _, t)| t.clone()).collect();
        // What the body of the report says (G2): the sections a recommendation rests on.
        let body: Vec<&str> = report
            .iter()
            .filter(|(k, _, _)| !CLOSING_SECTIONS.contains(&k.as_str()))
            .map(|(_, _, t)| t.as_str())
            .collect();
        let report_terms = dv_style::terms::usual(report_texts.iter().map(String::as_str), 1);
        let mut sections = Vec::new();
        for part in &structure.parts {
            for s in &part.sections {
                let paragraphs: Vec<ParagraphView> = shown
                    .remove(&s.key)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|d| ParagraphView {
                        has_versions: with_versions.contains(&d.id),
                        id: d.id,
                        text: shown_paragraph(
                            &d.text_tagged,
                            d.author,
                            &identities,
                            practitioner.as_deref(),
                            &masks,
                        ),
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
                            .chain(dv_domain::check_gender(&d.text_tagged, meta.child_gender))
                            .chain(dv_domain::check_grade(
                                &d.text_tagged,
                                &report
                                    .iter()
                                    .filter(|(_, _, t)| *t != d.text_tagged)
                                    .map(|(_, _, t)| t.as_str())
                                    .collect::<Vec<_>>(),
                            ))
                            .chain(if s.key == "recommendations" {
                                dv_domain::check_recommendations(&d.text_tagged, &body)
                            } else {
                                Vec::new()
                            })
                            // A level in the DSM section that is not the one she set (round 43).
                            .chain(if s.key == "dsm" {
                                dv_domain::check_dsm_severity(
                                    &d.text_tagged,
                                    meta.dsm_severity.as_ref(),
                                )
                            } else {
                                Vec::new()
                            })
                            .chain(dv_style::terms::notes(
                                &d.text_tagged,
                                &her_terms,
                                &report_terms,
                            ))
                            .chain(interpretations.notes(&d.text_tagged))
                            .collect(),
                        style_note: style_model
                            .as_ref()
                            .filter(|_| d.author == Author::Ai && d.status == DraftStatus::Proposed)
                            .and_then(|m| style::style_note(m, &s.key, &d.text_tagged)),
                        style: style_model
                            .as_ref()
                            .filter(|_| d.author == Author::Ai && d.status == DraftStatus::Proposed)
                            .map(|m| {
                                let shown = |t: &str| {
                                    restore(t, &identities, practitioner.as_deref(), &masks)
                                };
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
                let guide = guides::guide_for(v, &meta, s)?;
                sections.push(SectionView {
                    key: s.key.clone(),
                    title: s.title.clone(),
                    part: part.title.clone(),
                    source_count,
                    sortable: sortable.contains(&s.key.as_str()),
                    paragraphs,
                    approved,
                    mode: s.mode,
                    guide: guide.general,
                    guide_is_default: guide.is_default,
                    guide_learned_from: guide.learned_from,
                    case_guide: guide.case,
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
            sample,
            missing,
            prior_order,
        })
    }

    pub fn chat(&mut self, case_id: &str, section_key: &str) -> Result<Vec<ChatView>, CoreError> {
        let v = self.vault_ref()?;
        let identities = v.identities(case_id)?;
        let masks = v.masks(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        Ok(v.messages(case_id, section_key)?
            .into_iter()
            .map(|m| ChatView {
                role: if m.role == ChatRole::User {
                    "user".to_owned()
                } else {
                    "assistant".to_owned()
                },
                text: restore(&m.text_tagged, &identities, practitioner.as_deref(), &masks),
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

    /// "✓ לאשר" on the report page: the paragraphs she sees waiting, in one call, learned
    /// from once. Approving them one by one loaded and saved the style learning for each
    /// paragraph, which made a long section slow to approve (D-136).
    pub fn approve_paragraphs(
        &mut self,
        case_id: &str,
        draft_ids: &[String],
    ) -> Result<u32, CoreError> {
        let mut waiting = Vec::new();
        for id in draft_ids {
            if let Some(d) = style::find_draft(self.vault_ref()?, case_id, id)?
                .filter(|d| d.status == DraftStatus::Proposed)
            {
                waiting.push(d);
            }
        }
        let v = self.vault_mut()?;
        for d in &waiting {
            v.set_draft_status(case_id, &d.id, DraftStatus::Approved)?;
        }
        self.learn_from_approval(&waiting);
        Ok(u32::try_from(waiting.len()).unwrap_or(u32::MAX))
    }

    /// "✓ לאשר את כל הטיוטות" (D-139): of the paragraphs she saw waiting, only those still
    /// clean as the core sees them now: no note of any kind, and not a new wording of an
    /// approved paragraph. What the screen showed may be older than the case.
    pub fn approve_clean_paragraphs(
        &mut self,
        case_id: &str,
        draft_ids: &[String],
    ) -> Result<u32, CoreError> {
        let clean: std::collections::HashSet<String> = self
            .case_detail(case_id)?
            .sections
            .into_iter()
            .flat_map(|s| s.paragraphs)
            .filter(|p| {
                p.status == DraftStatus::Proposed && p.warnings.is_empty() && p.replaces.is_none()
            })
            .map(|p| p.id)
            .collect();
        let ids: Vec<String> = draft_ids
            .iter()
            .filter(|id| clean.contains(id.as_str()))
            .cloned()
            .collect();
        self.approve_paragraphs(case_id, &ids)
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

    /// Manual edit: the text (with real names) is stored tagged, every other word of hers
    /// kept (D-126).
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
        let tagged = self.learn_to_keep(case_id, text)?;
        let before = style::find_draft(self.vault_ref()?, case_id, draft_id)?;
        self.vault_mut()?.edit_draft(case_id, draft_id, &tagged)?;
        self.learn_from_draft_edit(before, &tagged);
        Ok(())
    }

    /// Her edit, saved from an editor that was opened on `opened_with` (the text as she saw it).
    /// If the paragraph changed meanwhile (an editor left open while another one saved, or a
    /// new wording came in), her text is still saved and the newer wording is kept among the
    /// earlier versions, as every edit keeps the one before it (D-046). Returns `true` then, so
    /// the page can tell her that two wordings exist and where the other one is.
    pub fn edit_paragraph_opened(
        &mut self,
        case_id: &str,
        draft_id: &str,
        text: &str,
        opened_with: Option<&str>,
    ) -> Result<bool, CoreError> {
        let changed_meanwhile = match opened_with {
            None => false,
            Some(seen) => {
                let v = self.vault_ref()?;
                match style::find_draft(v, case_id, draft_id)? {
                    None => false,
                    Some(d) => {
                        let identities = v.identities(case_id)?;
                        let masks = v.masks(case_id)?;
                        let practitioner = v.practitioner()?.names.first().cloned();
                        let now = shown_paragraph(
                            &d.text_tagged,
                            d.author,
                            &identities,
                            practitioner.as_deref(),
                            &masks,
                        );
                        now.trim() != seen.trim()
                    }
                }
            }
        };
        self.edit_paragraph(case_id, draft_id, text)?;
        Ok(changed_meanwhile && !text.trim().is_empty())
    }

    /// A paragraph's earlier wordings, newest first, names restored (D-046).
    pub fn paragraph_versions(
        &mut self,
        case_id: &str,
        draft_id: &str,
    ) -> Result<Vec<ParagraphVersionView>, CoreError> {
        let v = self.vault_ref()?;
        let identities = v.identities(case_id)?;
        let masks = v.masks(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        Ok(v.draft_versions(case_id, draft_id)?
            .into_iter()
            .map(|d| ParagraphVersionView {
                id: d.id,
                saved_at: d.saved_at,
                by_ai: d.author == Author::Ai,
                text: shown_paragraph(
                    &d.text_tagged,
                    d.author,
                    &identities,
                    practitioner.as_deref(),
                    &masks,
                ),
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
        let tagged = self.learn_to_keep(case_id, text)?;
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

/// A stored paragraph (or an earlier wording of one) as she sees it and exports it: hers with
/// her kept words as she wrote them (D-126), Claude's with every other tag put back.
pub(crate) fn shown_paragraph(
    text_tagged: &str,
    author: Author,
    identities: &[dv_domain::Identity],
    practitioner: Option<&str>,
    masks: &[dv_domain::MaskEntry],
) -> String {
    if author == Author::User {
        dv_privacy::restore::restore_hers(text_tagged, identities, practitioner, masks)
    } else {
        restore(text_tagged, identities, practitioner, masks)
    }
}

/// A stored paragraph as it goes into a request (D-126): hers back as she wrote it, so the
/// filter reads her words in their own context and hides exactly what it hid when she wrote
/// them; Claude's as stored. A kept-words tag anywhere else is never put back: the filter
/// reads it as plain words and the gate refuses it.
pub(crate) fn paragraph_to_send(
    v: &Vault,
    case_id: &str,
    d: &dv_domain::DraftParagraph,
) -> Result<String, CoreError> {
    if d.author != Author::User {
        return Ok(d.text_tagged.clone());
    }
    Ok(dv_privacy::restore::her_text_to_send(
        &d.text_tagged,
        &v.identities(case_id)?,
        v.practitioner()?.names.first().map(String::as_str),
        &v.masks(case_id)?,
    ))
}
