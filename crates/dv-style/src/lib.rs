//! Local learning of Einat's writing style (D-052). Everything here runs on her computer, on
//! text that is already in the vault, and keeps numbers rather than text:
//!
//! * [`stylometry`]: a per-section statistical model of how she writes (sentence length,
//!   connectives, hedging, first person, function words), learned from her past reports, from
//!   her final wording of every paragraph she edits, and from the reports she finishes in the
//!   program. It explains, in words, how a drafted paragraph differs from her usual writing,
//!   and gives one closeness figure (0 to 100) that follows the drafts over time.
//! * [`edits`]: what she changes in the AI's drafts. Replacements and deletions are counted
//!   against how often the phrase appeared and she left it, and become a suggestion only when
//!   the lower bound of a confidence interval says she changes it most of the time.
//! * [`phrases`]: expressions that recur across her past reports, found without any AI.
//! * [`retrieve`]: which of her profile items fit a request best (BM25), within a budget.
//!
//! Nothing here reaches the network, and nothing goes into a request until Einat adds it to her
//! profile, where the usual filter, overlap check and gate apply.

#![forbid(unsafe_code)]

pub mod edits;
pub mod phrases;
pub mod retrieve;
pub mod stylometry;
pub mod text;

pub use edits::{EditMark, EditModel, EditSuggestion, Pattern};
pub use phrases::{mine_phrases, PhraseCandidate, ReportParts};
pub use stylometry::{Measures, StyleModel};

/// The key for what holds across the whole report (same as `dv_ai::style::GENERAL`).
pub const GENERAL: &str = "general";
