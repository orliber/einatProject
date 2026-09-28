//! Shapes the UI receives. Names appear here only for local display.

use dv_ai::ProposedParagraph;
use dv_domain::{CaseInput, CaseMeta, DraftStatus, Identity, InputKind, Role};
use dv_privacy::{BlockReason, Checks, Segment, Suspect};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The error the UI shows: a stable `code` and a Hebrew `message`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UiError {
    pub code: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppStatus {
    pub vault_exists: bool,
    pub unlocked: bool,
    /// `on` | `off` | `unknown` (D-013: reminder only).
    pub disk_encryption: String,
    /// Set when the vault folder is inside a cloud-synced folder (the app refuses to run).
    pub cloud_synced_folder: Option<String>,
    pub fips_active: bool,
    /// No API key configured: answers come from local demo mode.
    pub demo_mode: bool,
    pub model: String,
    pub integrity_warning: Option<String>,
    pub lock_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreatedVault {
    /// Shown once, to print. The program does not keep it.
    pub recovery_key: String,
}

/// A draft paragraph with names restored for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ParagraphView {
    pub id: String,
    pub text: String,
    pub status: DraftStatus,
    pub by_ai: bool,
    /// Human-readable sources ("S2 · דוח קודם · אבחון נוירו-התפתחותי").
    pub sources: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionView {
    pub key: String,
    pub title: String,
    pub part: String,
    /// Input kinds that feed this section and how many inputs of those kinds exist.
    pub source_count: u32,
    pub paragraphs: Vec<ParagraphView>,
    pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatView {
    pub role: String,
    pub text: String,
    pub hidden: Vec<String>,
    pub demo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaseDetail {
    pub id: String,
    pub meta: CaseMeta,
    pub identities: Vec<Identity>,
    pub inputs: Vec<CaseInput>,
    pub sections: Vec<SectionView>,
}

/// One piece of outgoing text as the review screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewPart {
    pub label: String,
    pub original: Vec<Segment>,
    pub outgoing: Vec<Segment>,
}

/// "מה יוצא מהמחשב" – everything needed to approve (or understand a block).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Prepared {
    /// Bound to the approval; `send` accepts only this exact payload.
    pub approval_id: Option<String>,
    pub parts: Vec<ReviewPart>,
    pub suspects: Vec<Suspect>,
    pub hidden: Vec<String>,
    pub checks: Checks,
    pub blocked: Vec<BlockReason>,
    pub demo_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionResult {
    pub reply: String,
    pub paragraphs: Vec<ProposedParagraph>,
    pub questions: Vec<String>,
    pub missing: Vec<String>,
    pub contradictions: Vec<String>,
    pub demo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConsultResult {
    pub answer: String,
    pub demo: bool,
}

/// What the psychologist decided about a suspect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "decision", rename_all = "snake_case")]
#[ts(export)]
pub enum SuspectDecision {
    /// Add it to the case's identities (it will be hidden from now on).
    Hide { role: dv_domain::Role },
    /// An ordinary word that here hides a declared name ("סיפרה שאלון" = that Alon).
    IsName,
    /// An ordinary word / keep as is; remembered for this case.
    NotAName,
}

/// A name found where it will not be imported (header, footer, file properties): the
/// psychologist can add it to the case's names to hide in one click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NameSuggestion {
    pub value: String,
    /// Where it was found ("כותרת עליונה/תחתונה", "מאפייני הקובץ · יוצר המסמך").
    pub source: String,
    pub role: Role,
}

/// The import screen: what was read, what will be hidden, and what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportPreview {
    pub file_name: String,
    /// `docx` | `pdf` | `text`
    pub format: String,
    pub pages: u32,
    pub title: String,
    pub suggested_kind: InputKind,
    /// The body as it will be stored (editable before saving).
    pub body: String,
    /// The body with what the filter would hide marked.
    pub preview: Vec<Segment>,
    pub suspects: Vec<Suspect>,
    pub hidden: Vec<String>,
    /// Lines that were not imported (headers, footers, page numbers).
    pub left_out: Vec<String>,
    pub name_suggestions: Vec<NameSuggestion>,
    pub warnings: Vec<String>,
}
