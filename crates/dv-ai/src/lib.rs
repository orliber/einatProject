//! Builds requests for the AI (Claude by default; D-040) and parses its answers. No network access (that is `dv-egress`).
//!
//! Every text that goes into a request must already be filtered (tagged) by `dv-privacy`;
//! the gate then scans the finished request body as a whole.

pub mod demo;
mod interpret;
pub mod prompts;
mod report_check;
mod request;
mod response;
mod shrink;
mod sort;
pub mod style;
mod verify;

pub use interpret::{
    build_interpretation_request, parse_interpretations, render_interpretations, Interpretation,
    InterpretationGroup, RawInterpretation, INTERPRETATION_FIXED_TEXTS, MAX_INTERPRETATION_CHARS,
};
pub use report_check::{
    build_report_check_request, parse_report_check, ReportCheckInput, ReportCheckSection,
    ReportNote, ReportNoteKind, MAX_REPORT_NOTES,
};
pub use request::{
    build_consult_request, build_research_request, build_section_request, content_text,
    guide_lines, nonce_from, ConsultInput, ModelConfig, Provider, ResearchInput, SectionInput,
    TaggedInput, TaggedTurn, ALLOWED_MODELS, ANTHROPIC_MODELS, DEFAULT_MODEL, GEMINI_MODELS,
    LOCAL_MODELS, LONG_ANSWER_TOKENS, MISTRAL_MODELS, OPENAI_MODELS, ORDER_LINE, POINTS_LINE,
    RESEARCH_ALLOWED_DOMAINS, SHORT_ANSWER_TOKENS,
};
pub use response::{
    numbers_not_in, parse_consult, parse_derived_section, parse_points_section, parse_section,
    worth_another_try, AiError, ProposedParagraph, SectionReply,
};
pub use shrink::{shrink_to_fit, Cut};
pub use sort::{
    build_sort_request, parse_sort, passage_id, Reading, SortInput, SortMaterial, SortReply,
    SortSection,
};
pub use style::{
    build_style_analysis_request, build_style_synthesis_request, parse_style_analysis,
    parse_style_synthesis, render_for_section, RawStyleItem, StyleExcerpt, StyleItem, StyleKind,
    StyleOrigin, StyleProfile,
};
pub use verify::{
    build_verify_request, parse_verify, ParagraphCheck, Verdict, VerifyInput, VerifyNote,
};
