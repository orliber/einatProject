//! Cases, identities, sections and the records that belong to a case.
//!
//! Pure data and rules: no I/O, no crypto. Every type that crosses the IPC boundary
//! derives [`ts_rs::TS`] so the UI gets the same shapes.

pub mod identity;
pub mod records;
pub mod report;
pub mod routing;
pub mod scores;

pub use identity::{assign_tag, Identity, IdentityInput, Role, PRACTITIONER_TAG};
pub use records::{
    Age, Author, CaseInput, CaseMeta, CaseSummary, ChatMessage, ChatRole, Consent, DraftParagraph,
    DraftStatus, GrammaticalGender, InputKind, Transmission,
};
pub use report::{ReportPart, ReportSection, ReportStructure};
pub use routing::{passage_ranges, passages, Feed, Routing, SectionPassages, Suggestion};
pub use scores::{
    format_sheet, instruments, sheet_table, Instrument, ScoreEntry, ScoreRow, ScoreSheet,
};

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("report structure is invalid: {0}")]
    ReportStructure(String),
    #[error("invalid value: {0}")]
    Invalid(String),
}
