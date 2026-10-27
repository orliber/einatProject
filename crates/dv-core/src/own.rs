//! Who writes each section (round 30): the AI from the materials, or she herself. The template
//! says which (the impression and the emotional section are hers); she can change any section
//! in settings, for every case. Nothing is sent for a section she writes, except her own points
//! when she asks for them to be worded (`prepare_from_points`).

use super::*;
use dv_domain::SectionMode;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Settings key prefix of who writes a section: `mode/appearance` = `self` | `ai`.
const MODE_PREFIX: &str = "mode/";

/// Her choice for a section, if she made one.
pub(crate) fn mode_setting(v: &Vault, key: &str) -> Result<Option<SectionMode>, CoreError> {
    Ok(
        match v.setting(&format!("{MODE_PREFIX}{key}"))?.as_deref() {
            Some("self") => Some(SectionMode::Own),
            Some("ai") => Some(SectionMode::Ai),
            _ => None,
        },
    )
}

/// One section of the institute's report, for the settings list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionModeView {
    pub key: String,
    pub title: String,
    pub mode: SectionMode,
    /// As in the institute's template.
    pub template_mode: SectionMode,
}

impl Core {
    /// Who writes each section of the institute's report, as it applies now. The sections
    /// written from the approved ones (summary, diagnoses, recommendations, DSM) are not listed:
    /// they take no material anyway.
    pub fn section_modes(&mut self) -> Result<Vec<SectionModeView>, CoreError> {
        let v = self.vault_ref()?;
        let institute = ReportStructure::for_template(Some(dv_domain::INSTITUTE_TEMPLATE))
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        institute
            .sections()
            .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()) && !s.inputs.is_empty())
            .map(|s| {
                Ok(SectionModeView {
                    key: s.key.clone(),
                    title: s.title.clone(),
                    mode: mode_setting(v, &s.key)?.unwrap_or(s.mode),
                    template_mode: s.mode,
                })
            })
            .collect()
    }

    /// She writes the section herself (`self`), or the AI drafts it (`ai`), in every case.
    pub fn set_section_mode(
        &mut self,
        section_key: &str,
        mode: SectionMode,
    ) -> Result<(), CoreError> {
        if !self.section_modes()?.iter().any(|s| s.key == section_key) {
            return Err(CoreError::NotFound("הסעיף".to_owned()));
        }
        let value = match mode {
            SectionMode::Own => "self",
            SectionMode::Ai => "ai",
        };
        self.vault_mut()?
            .set_setting(&format!("{MODE_PREFIX}{section_key}"), value)?;
        // Nothing of a case is sent for a section she writes herself: a request for it,
        // waiting or still being filtered, in any case, does not go.
        if mode == SectionMode::Own {
            self.drop_pending();
        }
        Ok(())
    }
}
