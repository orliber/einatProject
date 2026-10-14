//! The "check against the sources" stage: drafted paragraphs and the sources they were
//! written from go out as one more request (always through the gate), and the answer says
//! per paragraph whether its claims are supported. The model only judges: nothing here
//! rewrites a paragraph, approves it or removes it.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::prompts::VERIFY_RULES;
use crate::request::{base_body, data_block, set, set_schema, ModelConfig, TaggedInput};
use crate::response::{text_of, AiError};

/// What one verification request is made of. Every text is already filtered (tagged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyInput {
    pub section_title: String,
    pub sources: Vec<TaggedInput>,
    /// Tagged text of each drafted paragraph, in order: they become `P1`, `P2`…
    pub paragraphs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Verdict {
    Supported,
    Partial,
    Unsupported,
    Contradicted,
    /// Set locally for a paragraph the answer did not mention: it was not checked.
    Unchecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VerifyNote {
    pub note: String,
    /// `S1`, `S2`… that exist in the request.
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ParagraphCheck {
    /// `P1`, `P2`… as sent.
    pub paragraph: String,
    pub verdict: Verdict,
    pub notes: Vec<VerifyNote>,
}

/// The schema is fixed in code and holds nothing from the case (providers cache schemas).
const VERIFY_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "paragraph": {"type": "string"},
          "verdict": {"type": "string", "enum": ["supported", "partial", "unsupported", "contradicted"]},
          "notes": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "note": {"type": "string"},
                "source_refs": {"type": "array", "items": {"type": "string"}}
              },
              "required": ["note", "source_refs"],
              "additionalProperties": false
            }
          }
        },
        "required": ["paragraph", "verdict", "notes"],
        "additionalProperties": false
      }
    }
  },
  "required": ["results"],
  "additionalProperties": false
}"#;

/// The request body, the `P#` ids of the paragraphs and the `S#` ids of the sources.
#[must_use]
pub fn build_verify_request(
    model: &ModelConfig,
    input: &VerifyInput,
    nonce: &str,
) -> (Value, Vec<String>, Vec<String>) {
    let mut body = base_body(model, VERIFY_RULES, 8_000);
    let mut parts = vec![format!("סעיף: {}", input.section_title)];
    let mut source_ids = Vec::new();
    parts.push("מקורות:".to_owned());
    for (i, s) in input.sources.iter().enumerate() {
        let sid = format!("S{}", i + 1);
        parts.push(data_block(
            nonce,
            &format!(
                "id=\"{sid}\" kind=\"{}\" title=\"{}\"",
                s.kind_label,
                s.title_tagged.replace('"', "'")
            ),
            &s.content_tagged,
        ));
        source_ids.push(sid);
    }
    parts.push("פסקאות לבדיקה:".to_owned());
    let mut paragraph_ids = Vec::new();
    for (i, text) in input.paragraphs.iter().enumerate() {
        let pid = format!("P{}", i + 1);
        parts.push(data_block(nonce, &format!("paragraph=\"{pid}\""), text));
        paragraph_ids.push(pid);
    }
    set(
        &mut body,
        "messages",
        json!([{ "role": "user", "content": parts.join("\n\n") }]),
    );
    if let Ok(schema) = serde_json::from_str::<Value>(VERIFY_SCHEMA) {
        set_schema(&mut body, schema);
    }
    (body, paragraph_ids, source_ids)
}

#[derive(Deserialize)]
struct RawNote {
    note: String,
    #[serde(default)]
    source_refs: Vec<String>,
}

#[derive(Deserialize)]
struct RawCheck {
    paragraph: String,
    verdict: String,
    #[serde(default)]
    notes: Vec<RawNote>,
}

#[derive(Deserialize)]
struct RawVerify {
    results: Vec<RawCheck>,
}

/// Parse the answer. Anything about a paragraph or a source that was not in the request is
/// dropped, an unknown verdict reads as "not checked", and a paragraph the answer skipped
/// is "not checked" too: a missing check never reads as a pass. One result per paragraph,
/// in the order sent.
pub fn parse_verify(
    response: &Value,
    paragraph_ids: &[String],
    source_ids: &[String],
) -> Result<Vec<ParagraphCheck>, AiError> {
    let text = text_of(response)?;
    let raw: RawVerify = serde_json::from_str(&text).map_err(|e| AiError::Format(e.to_string()))?;
    Ok(paragraph_ids
        .iter()
        .map(|pid| {
            let Some(found) = raw.results.iter().find(|r| &r.paragraph == pid) else {
                return ParagraphCheck {
                    paragraph: pid.clone(),
                    verdict: Verdict::Unchecked,
                    notes: Vec::new(),
                };
            };
            let verdict = match found.verdict.as_str() {
                "supported" => Verdict::Supported,
                "partial" => Verdict::Partial,
                "unsupported" => Verdict::Unsupported,
                "contradicted" => Verdict::Contradicted,
                _ => Verdict::Unchecked,
            };
            let notes = found
                .notes
                .iter()
                .map(|n| VerifyNote {
                    note: n.note.clone(),
                    source_refs: n
                        .source_refs
                        .iter()
                        .filter(|r| source_ids.contains(r))
                        .cloned()
                        .collect(),
                })
                .collect();
            ParagraphCheck {
                paragraph: pid.clone(),
                verdict,
                notes,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(results: &Value) -> Value {
        json!({
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": json!({ "results": results }).to_string()}]
        })
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_paragraph_the_answer_skips_is_not_checked() {
        let r = answer(&json!([{"paragraph": "P1", "verdict": "supported", "notes": []}]));
        let out = parse_verify(&r, &ids(&["P1", "P2"]), &ids(&["S1"])).unwrap();
        assert_eq!(out[0].verdict, Verdict::Supported);
        assert_eq!(out[1].verdict, Verdict::Unchecked);
    }

    #[test]
    fn unknown_sources_paragraphs_and_verdicts_are_dropped() {
        let r = answer(&json!([
            {"paragraph": "P9", "verdict": "supported", "notes": []},
            {"paragraph": "P1", "verdict": "fine", "notes": [
                {"note": "x", "source_refs": ["S1", "S7"]}
            ]}
        ]));
        let out = parse_verify(&r, &ids(&["P1"]), &ids(&["S1"])).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].verdict, Verdict::Unchecked);
        assert_eq!(out[0].notes[0].source_refs, ids(&["S1"]));
    }

    #[test]
    fn a_refusal_or_a_cut_answer_is_an_error_not_a_pass() {
        let cut = json!({"stop_reason": "max_tokens", "content": []});
        assert_eq!(
            parse_verify(&cut, &ids(&["P1"]), &[]).unwrap_err(),
            AiError::Truncated
        );
    }

    #[test]
    fn the_schema_holds_nothing_from_a_case() {
        let (body, p, s) = build_verify_request(
            &ModelConfig::default(),
            &VerifyInput {
                section_title: "t".to_owned(),
                sources: vec![TaggedInput {
                    input_id: "i1".to_owned(),
                    kind_label: "k".to_owned(),
                    title_tagged: "a".to_owned(),
                    content_tagged: "c".to_owned(),
                }],
                paragraphs: vec!["p".to_owned()],
            },
            "abc",
        );
        assert_eq!((p, s), (ids(&["P1"]), ids(&["S1"])));
        let schema = body["output_config"]["format"]["schema"].to_string();
        assert!(!schema.contains("i1") && !schema.contains("\"c\""));
        assert!(!body.to_string().contains("i1"), "input ids never leave");
    }
}
