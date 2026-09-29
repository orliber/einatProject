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
    /// Names always hidden as the practitioner (shown in settings).
    pub practitioner: Vec<String>,
    /// Show the review screen only when something is suspicious (D-020).
    pub review_only_suspect: bool,
    /// The choice above opens after the first 14 days of use (D-020).
    pub review_choice_available: bool,
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
    /// How many materials feed this section (table, sorting and Einat's choice, D-022).
    pub source_count: u32,
    /// Written from materials (not from other sections): materials can be sorted into it.
    pub sortable: bool,
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
    /// Where each material goes in the report, in the order of `inputs`.
    pub routing: Vec<MaterialRouting>,
    pub sections: Vec<SectionView>,
}

/// Where one material goes in the report (D-022).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MaterialRouting {
    pub input_id: String,
    /// Sections it feeds now, in report order.
    pub feeds: Vec<String>,
    /// Sections the sorting chose (empty when not sorted, or the text changed since).
    pub suggested: Vec<String>,
    /// Sections of the fixed table for its kind.
    pub table: Vec<String>,
    pub sorted: bool,
    /// Not read by a sorting since its text last changed: offer "מיון החומרים".
    pub needs_sorting: bool,
    /// Sorted by Claude (false: locally, in demo mode).
    pub by_ai: bool,
    /// Einat's changes.
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub passages: u32,
    /// Passages that reach at least one section.
    pub used_passages: u32,
}

/// What a sorting changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SortResult {
    /// Materials now sorted.
    pub sorted: u32,
    /// Materials left as they were (nothing placed, or edited meanwhile).
    pub unchanged: u32,
    /// Material-to-section links made.
    pub links: u32,
    /// Ids or sections in the answer that were not in the request (dropped).
    pub ignored: u32,
    pub demo: bool,
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

/// Before the Word file is made: what stops it and what the psychologist should know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExportCheck {
    /// Must be fixed first (leftover placeholders, missing-information markers).
    pub blocking: Vec<String>,
    /// Sections without approved paragraphs (left out of the file).
    pub empty_sections: Vec<String>,
    pub included_sections: u32,
    /// Score tables entered in the case, printed as an appendix.
    pub score_tables: u32,
    /// The file name, without the child's name (file names travel in e-mails).
    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportSettings {
    pub title: String,
    pub font: String,
    pub confidentiality: String,
    pub signature: Vec<String>,
}

/// A name typed for a case that already appears in another case (D-023).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NameMatch {
    /// What was typed.
    pub typed: String,
    pub case_id: String,
    pub case_code: String,
    pub child_name: Option<String>,
    /// Who the name is in that case.
    pub role: Role,
    pub value: String,
    /// That case is in the recycle bin.
    pub trashed: bool,
}

/// When the vault was last backed up, and whether it is time again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BackupStatus {
    #[ts(type = "number | null")]
    pub last_at: Option<i64>,
    #[ts(type = "number | null")]
    pub days_since: Option<i64>,
    /// No backup yet, or the last one is older than a week.
    pub due: bool,
    /// The last successful restore drill.
    #[ts(type = "number | null")]
    pub last_check_at: Option<i64>,
    /// An empty vault has nothing to lose yet: no reminder.
    pub has_cases: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BackupDone {
    pub path: String,
    #[ts(type = "number")]
    pub bytes: u64,
    #[ts(type = "number")]
    pub created_at: i64,
}

/// A backup file that was chosen, before its password is typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StagedBackup {
    pub file_name: String,
    #[ts(type = "number")]
    pub created_at: i64,
    /// Whether it belongs to the open vault (`None` when no vault is open).
    pub same_vault: Option<bool>,
}

/// The result of a restore drill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BackupCheckView {
    #[ts(type = "number")]
    pub created_at: i64,
    pub cases: u32,
    pub integrity_ok: bool,
}
