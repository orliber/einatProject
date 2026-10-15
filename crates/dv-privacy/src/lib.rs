//! De-identification pipeline and the gate that issues [`ClearedPayload`].
//!
//! Layers (docs/PRIVACY_PIPELINE.md): minimization happens in `dv-ai`'s context builder;
//! this crate does declared identities with Hebrew prefixes and spelling variants, near-miss
//! spellings, patterns (Safe Harbor floor + Israeli formats), unknown names from lexicons,
//! indirect identifiers, and the fail-closed gate over the complete request.

pub mod fix;
pub mod gate;
mod lexicon;
pub mod masks;
mod matcher;
mod misread;
pub mod patterns;
mod pipeline;
pub mod restore;
mod self_check;
pub mod text;

pub use fix::{drop_refused_lines, fix_refused, FixedItem, FIXABLE_CODES, LEFT_OUT};
pub use gate::{
    clear, replace_foreign_tags, tag_word, BlockReason, Blocked, ClearedPayload, GateRequest,
    GENERIC_TAGS,
};
pub use masks::{is_generic_tag, is_mask_tag, MaskBook, MaskEntry, MASK_BASES};
pub use misread::{ocr_misreads, Misread};
pub use pipeline::{
    everyday_name, filter, filter_split, filter_split_with_hints, filter_with_hints,
    forget_identity_index, AutoHidden, AutoKind, Checks, FilterOutcome, Mark, NameHint,
    PrivacyContext, Segment, Suspect, SuspectKind, NAME_HINT_SURE,
};
pub use self_check::gate_self_check;

#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
    #[error("internal filter error: {0}")]
    Internal(String),
}
