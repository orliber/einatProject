//! Cases, identities, sections and the records that belong to a case.
//!
//! Pure data and rules: no I/O, no crypto. Every type that crosses the IPC boundary
//! derives [`ts_rs::TS`] so the UI gets the same shapes.

pub mod compare;
pub mod consistency;
pub mod dates;
pub mod identity;
pub mod prior;
pub mod records;
pub mod report;
pub mod routing;
pub mod scores;

pub use compare::{compare, comparison_text, ComparisonRow};
pub use consistency::{check_age, check_gender, check_grade, check_recommendations};
pub use identity::{
    assign_tag, FoundName, Identity, IdentityInput, IdentitySource, Role, PRACTITIONER_TAG,
};
pub use prior::{
    date_text, document_date, record_order, DocDate, MaterialSummary, PriorRecord, Setting,
};
pub use records::{
    Age, Author, CaseInput, CaseMeta, CaseSummary, ChatMessage, ChatRole, Consent, DraftParagraph,
    DraftStatus, Folder, GrammaticalGender, InputKind, MaskEntry, Transmission,
};
pub use report::{
    ReportPart, ReportSection, ReportStructure, SectionMode, DEFAULT_TEMPLATE, INSTITUTE_TEMPLATE,
};
pub use routing::{passage_ranges, passages, Feed, Routing, SectionPassages, Suggestion};
pub use scores::{
    check_scores, find_score_mentions, format_sheet, instruments, is_score_tag, score_tag,
    sheet_profiles, sheet_table, Instrument, Relative, ScoreEntry, ScoreMention, ScoreProfile,
    ScoreRow, ScoreSheet,
};

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("report structure is invalid: {0}")]
    ReportStructure(String),
    #[error("invalid value: {0}")]
    Invalid(String),
}
