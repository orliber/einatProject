//! Records that belong to a case. All of them are stored encrypted with the case key.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Age {
    pub years: u8,
    pub months: u8,
}

impl Age {
    /// `5:4` – the clinical notation used in reports.
    #[must_use]
    pub fn display(self) -> String {
        format!("{}:{}", self.years, self.months)
    }
}

/// Parents' informed consent to AI-assisted drafting (D-008). No consent, no sending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Consent {
    /// ISO date, e.g. `2026-09-28`.
    pub given_on: String,
    pub form_version: String,
    /// Who signed, by role only (e.g. "שני ההורים").
    pub given_by: String,
}

/// Grammatical gender used for Hebrew agreement in drafts ("מתקשה" vs "מתקשה", "מגיב" vs "מגיבה").
/// Not an identifier; sent so the model writes correct Hebrew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GrammaticalGender {
    Male,
    Female,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
pub struct CaseMeta {
    pub code: String,
    pub age: Option<Age>,
    pub child_gender: Option<GrammaticalGender>,
    pub current_section: Option<String>,
    /// ISO date after which the retention reminder appears.
    pub retention_until: Option<String>,
    pub consent: Option<Consent>,
}

/// One row of the case list. Names appear only locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaseSummary {
    pub id: String,
    pub meta: CaseMeta,
    pub child_name: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
    pub approved_sections: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum InputKind {
    /// Parent intake.
    Intake,
    /// A report by another professional (medical, speech therapy, OT, previous assessment).
    PriorReport,
    /// A conversation with a professional.
    Professional,
    /// Kindergarten / school: conversation or report.
    Kindergarten,
    /// Test results: score sheets, questionnaires, norm tables.
    TestScores,
    /// The psychologist's own observation during testing.
    Observation,
    /// The psychologist's notes from a session.
    SessionNote,
    FreeText,
}

impl InputKind {
    /// How the source is named to the psychologist and in `source_refs`.
    #[must_use]
    pub fn label_he(self) -> &'static str {
        match self {
            InputKind::Intake => "אינטייק הורים",
            InputKind::PriorReport => "דוח קודם",
            InputKind::TestScores => "תוצאות מבחנים",
            InputKind::Professional => "שיחה עם איש מקצוע",
            InputKind::Kindergarten => "מסגרת חינוכית",
            InputKind::Observation => "תצפית",
            InputKind::SessionNote => "תיעוד מפגש",
            InputKind::FreeText => "הערה",
        }
    }
}

impl InputKind {
    /// Guess the kind of an imported document from its first lines and file name.
    /// Only a suggestion: the psychologist confirms it on the import screen.
    #[must_use]
    pub fn guess(text: &str, file_name: &str) -> InputKind {
        let head: String = text.chars().take(800).collect();
        let hay = format!("{file_name} {head}");
        let has = |words: &[&str]| words.iter().any(|w| hay.contains(w));
        let words = head.split_whitespace().count().max(1);
        let numbers = head
            .split_whitespace()
            .filter(|w| w.chars().any(|c| c.is_ascii_digit()))
            .count();
        let score_words = has(&[
            "ציון תקן",
            "ציונים",
            "אחוזון",
            "תוצאות מבחנים",
            "טבלת ציונים",
            "WISC",
            "WPPSI",
            "Vineland",
            "ABAS",
            "Conners",
            "BRIEF",
            "ADOS",
        ]);
        let report_words = has(&[
            "דוח",
            "סיכום ביקור",
            "מכתב שחרור",
            "אבחון",
            "קלינאית",
            "ריפוי בעיסוק",
            "פיזיותרפ",
            "נוירולוג",
            "רופא",
            "ד\"ר",
            "המלצות",
        ]);
        // A score sheet is mostly numbers; a report that quotes scores is a report.
        if score_words && (numbers * 5 > words || !report_words) {
            InputKind::TestScores
        } else if has(&["אינטייק", "שיחת היכרות עם ההורים", "שיחה עם ההורים"])
        {
            InputKind::Intake
        } else if has(&[
            "סיכום מפגש",
            "סיכום פגישה",
            "תיעוד מפגש",
            "מפגש מס",
            "פגישה מס",
        ]) {
            InputKind::SessionNote
        } else if has(&["תצפית"]) && !report_words {
            InputKind::Observation
        } else if has(&[
            "גננת",
            "מחנכת",
            "יועצת בית הספר",
            "דוח גן",
            "מסגרת חינוכית",
            "סייעת",
        ]) && !has(&["קלינאית", "רופא", "ד\"ר"])
        {
            InputKind::Kindergarten
        } else {
            // Most uploaded documents are other professionals' reports.
            InputKind::PriorReport
        }
    }
}

/// Raw material typed or uploaded by the psychologist (contains real names; never sent as is).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaseInput {
    pub id: String,
    pub case_id: String,
    pub kind: InputKind,
    pub title: String,
    pub content: String,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChatRole {
    User,
    Assistant,
}

/// A chat turn, stored with tags only. `hidden` lists what was hidden, by role label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatMessage {
    pub id: String,
    pub case_id: String,
    pub section_key: String,
    pub role: ChatRole,
    pub text_tagged: String,
    pub hidden: Vec<String>,
    pub demo: bool,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DraftStatus {
    Proposed,
    Approved,
    Rejected,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Author {
    Ai,
    User,
}

/// One paragraph of a section draft, stored with tags only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftParagraph {
    pub id: String,
    pub case_id: String,
    pub section_key: String,
    pub position: u32,
    pub text_tagged: String,
    pub status: DraftStatus,
    pub author: Author,
    pub source_refs: Vec<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub approved_at: Option<i64>,
}

/// Exactly what left the computer for one request (tagged text), for accountability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Transmission {
    pub id: String,
    pub case_id: String,
    pub section_key: String,
    pub payload_sha256: String,
    pub model: String,
    pub payload_tagged: String,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_the_kind_of_a_document() {
        assert_eq!(
            InputKind::guess("טבלת ציונים: ציון תקן 85, אחוזון 16", "scores.pdf"),
            InputKind::TestScores
        );
        assert_eq!(
            InputKind::guess("סיכום ביקור במכון. בוצע WISC בעבר.", "a.pdf"),
            InputKind::PriorReport
        );
        assert_eq!(
            InputKind::guess("אינטייק: ההורים מתארים", "a.docx"),
            InputKind::Intake
        );
        assert_eq!(
            InputKind::guess("סיכום מפגש 3 – משחק חופשי", "a.docx"),
            InputKind::SessionNote
        );
        assert_eq!(
            InputKind::guess("שיחה עם הגננת", "a.docx"),
            InputKind::Kindergarten
        );
        assert_eq!(
            InputKind::guess("דוח ריפוי בעיסוק", "a.pdf"),
            InputKind::PriorReport
        );
    }
}
