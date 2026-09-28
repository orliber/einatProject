//! Report structure, loaded from data (`templates/report_structure.json`) so new
//! report types can be added without code changes (D-015).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{DomainError, InputKind};

const DEFAULT_STRUCTURE: &str = include_str!("../../../templates/report_structure.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportSection {
    pub key: String,
    pub title: String,
    pub inputs: Vec<InputKind>,
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
    fn duplicate_keys_are_rejected() {
        let json = r#"{"id":"x","title":"t","parts":[{"key":"a","title":"a","sections":[
            {"key":"s","title":"1","inputs":[]},{"key":"s","title":"2","inputs":[]}]}]}"#;
        assert!(ReportStructure::parse(json).is_err());
    }
}
