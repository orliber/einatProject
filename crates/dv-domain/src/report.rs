//! Report structure, loaded from data (`templates/*.json`) so new report types can be added
//! without code changes (D-015). Each case keeps the template it was opened with: a new case
//! is in the institute's (D-066), and a case opened before keeps the one it had.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{DomainError, InputKind};

const DEFAULT_STRUCTURE: &str = include_str!("../../../templates/report_structure.json");
const INSTITUTE_STRUCTURE: &str = include_str!("../../../templates/institute-v1.json");

/// The template of the cases opened before the institute's (`report_structure.json`).
pub const DEFAULT_TEMPLATE: &str = "developmental-assessment-v1";

/// The institute's report (D-066): the template of every new case.
pub const INSTITUTE_TEMPLATE: &str = "institute-v1";

/// Who writes a section's first draft.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SectionMode {
    /// The AI drafts it from the materials, she approves.
    #[default]
    Ai,
    /// Hers to write (the impression, the emotional section): nothing is sent for it.
    #[serde(rename = "self")]
    Own,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportSection {
    pub key: String,
    pub title: String,
    pub inputs: Vec<InputKind>,
    /// What belongs in the section, in a few words. Sent when materials are sorted into
    /// sections (D-022), so it must never contain case data or a first name.
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub mode: SectionMode,
    /// How the section is written, from the institute's document: general, never about a
    /// child. One line per paragraph when it has a fixed order of paragraphs.
    #[serde(default)]
    pub guide: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportPart {
    pub key: String,
    pub title: String,
    pub sections: Vec<ReportSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportStructure {
    pub id: String,
    pub title: String,
    pub parts: Vec<ReportPart>,
}

impl ReportStructure {
    /// The psychologist's standard report (REPORT_STRUCTURE.md).
    pub fn load_default() -> Result<Self, DomainError> {
        Self::parse(DEFAULT_STRUCTURE)
    }

    /// A case's template: `None` for a case opened before templates were kept with it.
    /// A name the program does not know is an error, never a silent fallback.
    pub fn for_template(template: Option<&str>) -> Result<Self, DomainError> {
        match template {
            None => Self::load_default(),
            Some(INSTITUTE_TEMPLATE) => Self::parse(INSTITUTE_STRUCTURE),
            Some(t) if t == DEFAULT_TEMPLATE => Self::load_default(),
            Some(t) => Err(DomainError::ReportStructure(format!(
                "unknown template {t}"
            ))),
        }
    }

    pub fn parse(json: &str) -> Result<Self, DomainError> {
        let structure: Self =
            serde_json::from_str(json).map_err(|e| DomainError::ReportStructure(e.to_string()))?;
        let mut keys: Vec<&str> = structure.sections().map(|s| s.key.as_str()).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        if keys.len() != count {
            return Err(DomainError::ReportStructure(
                "duplicate section key".to_owned(),
            ));
        }
        if count == 0 {
            return Err(DomainError::ReportStructure("no sections".to_owned()));
        }
        Ok(structure)
    }

    pub fn sections(&self) -> impl Iterator<Item = &ReportSection> {
        self.parts.iter().flat_map(|p| p.sections.iter())
    }

    #[must_use]
    pub fn section(&self, key: &str) -> Option<&ReportSection> {
        self.sections().find(|s| s.key == key)
    }

    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.section(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_structure_has_the_sixteen_sections_of_the_report() {
        let s = ReportStructure::load_default().unwrap();
        assert_eq!(s.sections().count(), 16);
        assert!(s.contains("kindergarten"));
        assert!(s.contains("dsm"));
        assert!(!s.contains("nonexistent"));
    }

    #[test]
    fn the_institute_report_keeps_every_section_and_adds_behavior_and_attention() {
        let old = ReportStructure::load_default().unwrap();
        let s = ReportStructure::for_template(Some(INSTITUTE_TEMPLATE)).unwrap();
        assert_eq!(s.id, INSTITUTE_TEMPLATE);
        for o in old.sections() {
            assert!(s.contains(&o.key), "{} is kept", o.key);
        }
        assert!(s.contains("behavior_attention"));
        assert_eq!(s.sections().count(), 17);
        // The institute's order: communication before adaptive, DSM right after communication.
        let keys: Vec<&str> = s.sections().map(|x| x.key.as_str()).collect();
        let at = |k: &str| keys.iter().position(|x| *x == k).unwrap();
        assert!(at("cognitive") < at("communication"));
        assert_eq!(at("dsm"), at("communication") + 1);
        assert!(at("adaptive") < at("behavior_attention"));
        assert!(at("behavior_attention") < at("emotional"));
        // Hers to write by default: the impression and the emotional section.
        let own: Vec<&str> = s
            .sections()
            .filter(|x| x.mode == SectionMode::Own)
            .map(|x| x.key.as_str())
            .collect();
        assert_eq!(own, ["appearance", "emotional"]);
        assert!(s.section("appearance").unwrap().guide.lines().count() == 4);
        // The old template stays as it was, every section drafted by the AI.
        assert!(old
            .sections()
            .all(|x| x.mode == SectionMode::Ai && x.guide.is_empty()));
    }

    #[test]
    fn a_case_without_a_template_keeps_the_old_one_and_an_unknown_one_is_an_error() {
        assert_eq!(
            ReportStructure::for_template(None).unwrap(),
            ReportStructure::load_default().unwrap()
        );
        assert_eq!(
            ReportStructure::for_template(Some(DEFAULT_TEMPLATE))
                .unwrap()
                .id,
            DEFAULT_TEMPLATE
        );
        assert!(ReportStructure::for_template(Some("other-v9")).is_err());
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let json = r#"{"id":"x","title":"t","parts":[{"key":"a","title":"a","sections":[
            {"key":"s","title":"1","inputs":[]},{"key":"s","title":"2","inputs":[]}]}]}"#;
        assert!(ReportStructure::parse(json).is_err());
    }
}
