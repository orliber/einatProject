//! Parse Claude's answers and check them locally before the psychologist sees them.
//!
//! Accuracy safeguards (the psychologist's first concern): every paragraph must cite sources
//! that exist, and every number it states (a score, an age) must appear in those sources.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AiError {
    #[error("Claude declined the request ({0})")]
    Refused(String),
    #[error("the answer was cut off (max_tokens); try a shorter request")]
    Truncated,
    #[error("the answer was not in the expected format: {0}")]
    Format(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProposedParagraph {
    pub text: String,
    /// `S1`, `S2`… as cited by Claude.
    pub source_refs: Vec<String>,
    /// Problems found locally (unknown source, a number not found in the sources…).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionReply {
    pub reply: String,
    pub paragraphs: Vec<ProposedParagraph>,
    pub questions: Vec<String>,
    pub missing: Vec<String>,
    pub contradictions: Vec<String>,
}

#[derive(Deserialize)]
struct RawParagraph {
    text: String,
    source_refs: Vec<String>,
}

#[derive(Deserialize)]
struct RawReply {
    reply: String,
    paragraphs: Vec<RawParagraph>,
    questions: Vec<String>,
    missing: Vec<String>,
    #[serde(default)]
    contradictions: Vec<String>,
}

/// Concatenated text blocks, after checking why the model stopped.
pub(crate) fn text_of(response: &Value) -> Result<String, AiError> {
    match response["stop_reason"].as_str() {
        Some("refusal") => {
            let category = response["stop_details"]["category"]
                .as_str()
                .unwrap_or("unspecified");
            return Err(AiError::Refused(category.to_owned()));
        }
        Some("max_tokens") => return Err(AiError::Truncated),
        _ => {}
    }
    let blocks = response["content"]
        .as_array()
        .ok_or_else(|| AiError::Format("no content".to_owned()))?;
    Ok(blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join(""))
}

/// Numbers written in `text` (scores, ages such as 5:4, T-scores).
fn numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || ((c == '.' || c == ':') && !cur.is_empty()) {
            cur.push(c);
        } else if !cur.is_empty() {
            let n = cur.trim_end_matches(['.', ':']).to_owned();
            if !n.is_empty() && !out.contains(&n) {
                out.push(n);
            }
            cur.clear();
        }
    }
    out
}

/// Parse a section answer and verify it against the sources that were sent.
///
/// `sources` maps `S#` to the tagged text of that source.
pub fn parse_section(
    response: &Value,
    sources: &[(String, String)],
) -> Result<SectionReply, AiError> {
    let text = text_of(response)?;
    let raw: RawReply = serde_json::from_str(&text).map_err(|e| AiError::Format(e.to_string()))?;
    let paragraphs = raw
        .paragraphs
        .into_iter()
        .map(|p| {
            let mut warnings = Vec::new();
            let cited: Vec<&str> = p
                .source_refs
                .iter()
                .filter_map(|r| {
                    sources
                        .iter()
                        .find(|(id, _)| id == r)
                        .map(|(_, t)| t.as_str())
                })
                .collect();
            if p.source_refs.is_empty() {
                warnings.push("הפסקה לא מציינת מקור".to_owned());
            }
            for r in &p.source_refs {
                if !sources.iter().any(|(id, _)| id == r) {
                    warnings.push(format!("מקור לא קיים: {r}"));
                }
            }
            for n in numbers(&p.text) {
                if !cited.iter().any(|t| numbers(t).contains(&n)) {
                    warnings.push(format!("המספר {n} לא מופיע במקורות שצוינו – לבדוק"));
                }
            }
            ProposedParagraph {
                text: p.text,
                source_refs: p.source_refs,
                warnings,
            }
        })
        .collect();
    Ok(SectionReply {
        reply: raw.reply,
        paragraphs,
        questions: raw.questions,
        missing: raw.missing,
        contradictions: raw.contradictions,
    })
}

/// Parse an answer for a section written from the approved sections (summary, diagnoses,
/// recommendations, DSM). Those are sent without `S#` sources, so a paragraph citing none is
/// expected; every number it states must still appear in an approved section that was sent.
pub fn parse_derived_section(
    response: &Value,
    approved: &[String],
) -> Result<SectionReply, AiError> {
    let mut reply = parse_section(response, &[])?;
    for p in &mut reply.paragraphs {
        p.warnings = p
            .source_refs
            .iter()
            .map(|r| format!("מקור לא קיים: {r}"))
            .collect();
        for n in numbers(&p.text) {
            if !approved.iter().any(|t| numbers(t).contains(&n)) {
                p.warnings
                    .push(format!("המספר {n} לא מופיע בסעיפים שאושרו – לבדוק"));
            }
        }
    }
    Ok(reply)
}

/// Plain-text answer (consultation, research).
pub fn parse_consult(response: &Value) -> Result<String, AiError> {
    text_of(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn api(text: &str) -> Value {
        json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": text}]})
    }

    #[test]
    fn derived_sections_check_numbers_against_the_approved_sections() {
        let reply = json!({
            "reply": "סיכמתי",
            "paragraphs": [
                {"text": "[ילד] הציג תפקוד ממוצע (9) באוצר מילים.", "source_refs": []},
                {"text": "ציון כולל של 83.", "source_refs": []},
                {"text": "לפי S1.", "source_refs": ["S1"]}
            ],
            "questions": [], "missing": [], "contradictions": []
        });
        let approved = vec!["אוצר מילים: 9.".to_owned()];
        let out = parse_derived_section(&api(&reply.to_string()), &approved).unwrap();
        assert!(out.paragraphs[0].warnings.is_empty());
        assert!(out.paragraphs[1].warnings.iter().any(|w| w.contains("83")));
        assert!(out.paragraphs[2].warnings.iter().any(|w| w.contains("S1")));
    }

    #[test]
    fn invented_numbers_and_unknown_sources_are_flagged() {
        let reply = json!({
            "reply": "ניסחתי",
            "paragraphs": [
                {"text": "[ילד] הציג תפקוד ממוצע (9) במטלת אוצר מילים.", "source_refs": ["S1"]},
                {"text": "בשאלון הושג ציון T של 83.", "source_refs": ["S1"]},
                {"text": "משפט בלי מקור.", "source_refs": []},
                {"text": "לפי S9.", "source_refs": ["S9"]}
            ],
            "questions": [], "missing": [], "contradictions": []
        });
        let sources = vec![("S1".to_owned(), "אוצר מילים: 9. מטריצות: 13.".to_owned())];
        let out = parse_section(&api(&reply.to_string()), &sources).unwrap();
        assert!(out.paragraphs[0].warnings.is_empty());
        assert!(out.paragraphs[1].warnings.iter().any(|w| w.contains("83")));
        assert!(!out.paragraphs[2].warnings.is_empty());
        assert!(out.paragraphs[3].warnings.iter().any(|w| w.contains("S9")));
    }

    #[test]
    fn refusals_and_truncation_are_errors() {
        let refused =
            json!({"stop_reason": "refusal", "stop_details": {"category": "bio"}, "content": []});
        assert_eq!(
            parse_consult(&refused),
            Err(AiError::Refused("bio".to_owned()))
        );
        let cut = json!({"stop_reason": "max_tokens", "content": [{"type": "text", "text": "{"}]});
        assert_eq!(parse_consult(&cut), Err(AiError::Truncated));
    }

    #[test]
    fn ages_and_decimals_count_as_numbers() {
        assert_eq!(
            numbers("בן 5:4, בגיל 2.5, ציון 111."),
            vec!["5:4", "2.5", "111"]
        );
    }
}
