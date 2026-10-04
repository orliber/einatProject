//! De-identification pipeline and the gate that issues [`ClearedPayload`].
//!
//! Layers (docs/PRIVACY_PIPELINE.md): minimization happens in `dv-ai`'s context builder;
//! this crate does declared identities with Hebrew prefixes and spelling variants, near-miss
//! spellings, patterns (Safe Harbor floor + Israeli formats), unknown names from lexicons,
//! indirect identifiers, and the fail-closed gate over the complete request.

pub mod gate;
mod lexicon;
mod matcher;
pub mod patterns;
mod pipeline;
pub mod restore;
pub mod text;

pub use gate::{clear, BlockReason, Blocked, ClearedPayload, GateRequest, GENERIC_TAGS};
pub use pipeline::{
    filter, filter_split, AutoHidden, AutoKind, Checks, FilterOutcome, Mark, PrivacyContext,
    Segment, Suspect, SuspectKind,
};

#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
    #[error("internal filter error: {0}")]
    Internal(String),
}
