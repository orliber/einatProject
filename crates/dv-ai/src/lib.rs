//! Builds requests for the AI (Claude by default; D-040) and parses its answers. No network access (that is `dv-egress`).
//!
//! Every text that goes into a request must already be filtered (tagged) by `dv-privacy`;
//! the gate then scans the finished request body as a whole.

pub mod demo;
pub mod prompts;
mod request;
mod response;
mod sort;
pub mod style;

pub use request::{
    build_consult_request, build_research_request, build_section_request, nonce_from, ConsultInput,
    ModelConfig, Provider, ResearchInput, SectionInput, TaggedInput, TaggedTurn, ALLOWED_MODELS,
    ANTHROPIC_MODELS, DEFAULT_MODEL, GEMINI_MODELS, LOCAL_MODELS, MISTRAL_MODELS, OPENAI_MODELS,
    RESEARCH_ALLOWED_DOMAINS,
};
pub use response::{
    parse_consult, parse_derived_section, parse_section, AiError, ProposedParagraph, SectionReply,
};
pub use sort::{
    build_sort_request, parse_sort, passage_id, SortInput, SortMaterial, SortReply, SortSection,
};
pub use style::{
    build_style_analysis_request, build_style_synthesis_request, parse_style_analysis,
    parse_style_synthesis, render_for_section, RawStyleItem, StyleExcerpt, StyleItem, StyleKind,
    StyleOrigin, StyleProfile,
};
