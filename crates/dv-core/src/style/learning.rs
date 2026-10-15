//! What is learned from her edits, approvals and final reports, and the suggestions made of it (D-052).

use super::*;
use crate::structure_for;

impl Core {
    /// Learn from her edit of the AI's paragraph in `section`: the changes she made, and her
    /// final wording into the style model. Local only (D-052).
    pub(crate) fn learn_from_edit(
        &mut self,
        section: &str,
        before: &str,
        after: &str,
    ) -> Result<(), CoreError> {
        let mut learning = self.learning()?;
        learning.observe_edit(section, before, after);
        let mut local = local_style(self.vault_ref()?)?;
        local.note_fit(section, before);
        local.model.observe_edit(section, after);
        let v = self.vault_mut()?;
        v.save_style_learning(&json(&learning)?)?;
        v.set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        Ok(())
    }

    /// Her manual edit of a paragraph Claude wrote: learn from it (never blocks the edit).
    pub(crate) fn learn_from_draft_edit(&mut self, old: Option<DraftParagraph>, new_tagged: &str) {
        if let Some(old) = old.filter(|d| d.author == Author::Ai) {
            // Learning is a convenience: an error here must not lose her edit.
            let _ = self.learn_from_edit(&old.section_key, &old.text_tagged, new_tagged);
        }
    }

    /// She approved the AI's paragraphs as written: the phrases in them were accepted. Never
    /// blocks the approval.
    pub(crate) fn learn_from_approval(&mut self, approved: &[DraftParagraph]) {
        let ai: Vec<&DraftParagraph> = approved.iter().filter(|d| d.author == Author::Ai).collect();
        if ai.is_empty() {
            return;
        }
        let Ok(mut learning) = self.learning() else {
            return;
        };
        for d in &ai {
            learning.observe_approval(&d.text_tagged);
        }
        if let (Ok(doc), Ok(v)) = (json(&learning), self.vault_mut()) {
            let _ = v.save_style_learning(&doc);
        }
        // How close they were to her writing, as written.
        let Ok(mut local) = self.vault_ref().and_then(local_style) else {
            return;
        };
        for d in &ai {
            local.note_fit(&d.section_key, &d.text_tagged);
        }
        if let (Ok(doc), Ok(v)) = (json(&local), self.vault_mut()) {
            let _ = v.set_secret(LOCAL_STYLE_KEY, &doc);
        }
    }

    /// The report she finished (exported): every approved paragraph teaches the style model
    /// at full weight, once per case. Numbers only; never blocks the export.
    pub(crate) fn learn_from_final(&mut self, case_id: &str) {
        let _ = self.try_learn_from_final(case_id);
    }

    pub(super) fn try_learn_from_final(&mut self, case_id: &str) -> Result<(), CoreError> {
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let v = self.vault_ref()?;
        let mut parts: Vec<(String, String)> = Vec::new();
        for section in structure.sections().filter(|s| s.key != "signature") {
            for d in v.drafts(case_id, &section.key)? {
                if d.status == DraftStatus::Approved {
                    parts.push((section.key.clone(), d.text_tagged));
                }
            }
        }
        let key = final_key(v, case_id);
        let mut local = local_style(v)?;
        local.model.observe_final(
            &key,
            parts.iter().map(|(s, t)| (Some(s.as_str()), t.as_str())),
        );
        self.vault_mut()?
            .set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        Ok(())
    }

    /// A case erased for good no longer teaches the style model. Never blocks the erasure.
    pub(crate) fn forget_final_style(&mut self, case_id: &str) {
        let Ok(v) = self.vault_ref() else {
            return;
        };
        let key = final_key(v, case_id);
        let Ok(mut local) = local_style(v) else {
            return;
        };
        if local.model.forget_final(&key) {
            if let (Ok(doc), Ok(v)) = (json(&local), self.vault_mut()) {
                let _ = v.set_secret(LOCAL_STYLE_KEY, &doc);
            }
        }
    }

    /// Forget what was learned from her edits and finished reports (the suggestions, the
    /// figures and the closeness record). Her past reports and her profile stay.
    pub fn forget_style_learning(&mut self) -> Result<(), CoreError> {
        let mut local = local_style(self.vault_ref()?)?;
        local.model.forget_learning();
        local.draft_fit.clear();
        local.fit_months.clear();
        let v = self.vault_mut()?;
        v.save_style_learning(&json(&Learning::default())?)?;
        v.set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        v.record(
            AuditEvent::SettingsChanged,
            None,
            &serde_json::json!({ "key": "style_learning_forgotten" }),
        )?;
        Ok(())
    }

    /// Add a suggestion to the profile in use, as a new version (her click is the approval).
    pub fn accept_style_suggestion(&mut self, id: &str) -> Result<StyleProfileView, CoreError> {
        let mut learning = self.learning()?;
        let local = local_style(self.vault_ref()?)?;
        let not_found = || CoreError::NotFound("ההצעה".to_owned());
        let item = if let Some(key) = id.strip_prefix(SHAPE_ID) {
            let shape = local
                .shapes
                .iter()
                .find(|s| shape_key(s) == key)
                .ok_or_else(not_found)?;
            learning.dismissed_phrases.push(id.to_owned());
            let verb = match shape.place {
                dv_style::Place::Opening => "לפתוח",
                dv_style::Place::Closing => "לסיים",
            };
            (
                Some(shape.section.clone()),
                StyleKind::Rule,
                format!("{verb} את הסעיף ב«{}…»", shape.text),
                StyleOrigin::Reports,
                shape.reports,
            )
        } else if let Some(norm) = id.strip_prefix(FORMULA_ID) {
            let f = local
                .formulas
                .iter()
                .find(|f| f.norm == norm)
                .ok_or_else(not_found)?;
            learning.dismissed_phrases.push(id.to_owned());
            (
                f.section.clone(),
                StyleKind::Template,
                f.text.clone(),
                StyleOrigin::Reports,
                f.reports,
            )
        } else if let Some(norm) = id.strip_prefix(PHRASE_ID) {
            let p = local
                .phrases
                .iter()
                .find(|p| p.norm == norm)
                .ok_or_else(not_found)?;
            learning.dismissed_phrases.push(p.norm.clone());
            (
                p.section.clone(),
                StyleKind::Phrase,
                p.text.clone(),
                StyleOrigin::Reports,
                p.reports,
            )
        } else {
            let section = learning
                .suggestions()
                .into_iter()
                .find(|s| s.id == id)
                .and_then(|s| s.section);
            let pair = learning.find_mut(id).ok_or_else(not_found)?;
            pair.accepted = true;
            let (kind, text) = if pair.is_deletion() {
                (StyleKind::Avoid, format!("לא לכתוב «{}»", pair.from))
            } else {
                (
                    StyleKind::Rule,
                    format!("לכתוב «{}» ולא «{}»", pair.to, pair.from),
                )
            };
            (section, kind, text, StyleOrigin::Edits, 0)
        };
        let (section, kind, text, origin, support) = item;
        let mut stored = self.active_profile()?.unwrap_or(StoredProfile {
            profile: StyleProfile::default(),
            dropped: 0,
            demo: false,
        });
        stored.profile.items.push(StyleItem {
            id: dv_vault::crypto::random_id()?,
            section,
            kind,
            text,
            enabled: true,
            origin,
            support,
        });
        self.check_items(&mut stored.profile)?;
        let v = self.vault_mut()?;
        let (new_id, _) = v.add_style_profile("active", &json(&stored)?)?;
        v.save_style_learning(&json(&learning)?)?;
        let active = v
            .style_profiles()?
            .into_iter()
            .find(|p| p.id == new_id)
            .ok_or_else(|| CoreError::Internal("profile".to_owned()))?;
        Self::profile_view(active)
    }

    pub fn dismiss_style_suggestion(&mut self, id: &str) -> Result<(), CoreError> {
        let mut learning = self.learning()?;
        if id.starts_with(SHAPE_ID) || id.starts_with(FORMULA_ID) {
            learning.dismissed_phrases.push(id.to_owned());
        } else if let Some(norm) = id.strip_prefix(PHRASE_ID) {
            learning.dismissed_phrases.push(norm.to_owned());
        } else {
            learning
                .find_mut(id)
                .ok_or_else(|| CoreError::NotFound("ההצעה".to_owned()))?
                .dismissed = true;
        }
        self.vault_mut()?.save_style_learning(&json(&learning)?)?;
        Ok(())
    }
}

/// Suggestion ids of phrases from her reports start with this.
pub(super) const PHRASE_ID: &str = "phrase:";
/// ...and of the openings and closings of her sections with this (the whole id is kept when
/// she answers, among the dismissed phrases).
pub(super) const SHAPE_ID: &str = "shape:";
/// ...and of her fixed wordings with this (kept whole when she answers, like a shape's).
pub(super) const FORMULA_ID: &str = "formula:";

pub(super) fn shape_key(s: &dv_style::Shape) -> String {
    let place = match s.place {
        dv_style::Place::Opening => "opening",
        dv_style::Place::Closing => "closing",
    };
    format!("{place}:{}:{}", s.section, s.norm)
}

/// What to offer her: her usual edits, then phrases of her reports not yet in the profile.
pub(super) fn suggestions(
    learning: &Learning,
    local: &LocalStyle,
    active: Option<&StyleProfileView>,
) -> Vec<StyleSuggestion> {
    let mut out: Vec<StyleSuggestion> = learning
        .suggestions()
        .into_iter()
        .map(|s| StyleSuggestion {
            kind: if s.to.is_empty() {
                StyleSuggestionKind::Avoid
            } else {
                StyleSuggestionKind::Replace
            },
            id: s.id,
            from: s.from,
            to: s.to,
            count: s.count,
            confidence: s.confidence,
            section: s.section,
        })
        .collect();
    let in_profile: Vec<String> = active
        .map(|a| a.profile.items.iter().map(|i| normalize(&i.text)).collect())
        .unwrap_or_default();
    out.extend(
        local
            .phrases
            .iter()
            .filter(|p| !learning.dismissed_phrases.contains(&p.norm))
            .filter(|p| !in_profile.iter().any(|t| t.contains(&p.norm)))
            .map(|p| StyleSuggestion {
                id: format!("{PHRASE_ID}{}", p.norm),
                kind: StyleSuggestionKind::Phrase,
                from: p.text.clone(),
                to: String::new(),
                count: p.reports,
                confidence: p.share,
                section: p.section.clone(),
            }),
    );
    out.extend(
        local
            .shapes
            .iter()
            .map(|sh| (format!("{SHAPE_ID}{}", shape_key(sh)), sh))
            .filter(|(id, _)| !learning.dismissed_phrases.contains(id))
            .filter(|(_, sh)| !in_profile.iter().any(|t| t.contains(&sh.norm)))
            .map(|(id, sh)| StyleSuggestion {
                id,
                kind: match sh.place {
                    dv_style::Place::Opening => StyleSuggestionKind::Opening,
                    dv_style::Place::Closing => StyleSuggestionKind::Closing,
                },
                from: sh.text.clone(),
                to: String::new(),
                count: sh.reports,
                confidence: sh.share,
                section: Some(sh.section.clone()),
            }),
    );
    out.extend(
        local
            .formulas
            .iter()
            .map(|f| (format!("{FORMULA_ID}{}", f.norm), f))
            .filter(|(id, _)| !learning.dismissed_phrases.contains(id))
            .filter(|(_, f)| !in_profile.iter().any(|t| *t == normalize(&f.text)))
            .map(|(id, f)| StyleSuggestion {
                id,
                kind: StyleSuggestionKind::Formula,
                from: f.text.clone(),
                to: String::new(),
                count: f.reports,
                confidence: f.share,
                section: f.section.clone(),
            }),
    );
    out
}
