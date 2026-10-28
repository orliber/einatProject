//! The check of the whole report (G2, stage two; AI-5): once every section is approved, one
//! request with the approved sections only (always through the gate) asks where two sections
//! contradict each other, where the same thing is said twice, and where a finding is in the
//! summary but not in the body. The model only points: nothing here rewrites, approves or
//! removes a word, and the scores are checked on the computer, not by the model.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::prompts::REPORT_CHECK_RULES;
use crate::request::{base_body, data_block, set, set_schema, ModelConfig, LONG_ANSWER_TOKENS};
use crate::response::{text_of, AiError};

/// One approved section as it goes out. Every text is already filtered (tagged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportCheckSection {
    /// The section's heading (fixed text of the template, filtered like the rest).
    pub title: String,
    /// Its approved paragraphs, joined, tagged.
    pub text_tagged: String,
    /// A section that sums up the body (summary, diagnoses, recommendations, DSM).
    pub closing: bool,
}

/// What one whole-report check is made of, in the order of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportCheckInput {
    pub sections: Vec<ReportCheckSection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReportNoteKind {
    /// Two sections say opposite things.
    Contradiction,
    /// The same content twice, without need.
    Repetition,
    /// A finding in a closing section with no ground in the body.
    SummaryOnly,
    /// A kind the answer named that is not one of the three: still shown, never dropped.
    Other,
}

/// One note about the report, as the model gave it (sections as `R#` ids that were sent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportNote {
    pub kind: ReportNoteKind,
    pub sections: Vec<String>,
    pub note: String,
}

/// At most this many notes are kept from one answer, each cut to this many characters: a
/// list to read, not a second report.
pub const MAX_REPORT_NOTES: usize = 40;
const MAX_NOTE_CHARS: usize = 600;

/// The schema is fixed in code and holds nothing from the case (providers cache schemas).
const REPORT_CHECK_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "notes": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "kind": {"type": "string", "enum": ["contradiction", "repetition", "summary_only"]},
          "sections": {"type": "array", "items": {"type": "string"}},
          "note": {"type": "string"}
        },
        "required": ["kind", "sections", "note"],
        "additionalProperties": false
      }
    }
  },
  "required": ["notes"],
  "additionalProperties": false
}"#;

/// The request body and the `R#` ids of the sections, in the order sent.
#[must_use]
pub fn build_report_check_request(
    model: &ModelConfig,
    input: &ReportCheckInput,
    nonce: &str,
) -> (Value, Vec<String>) {
    let mut body = base_body(model, REPORT_CHECK_RULES, LONG_ANSWER_TOKENS);
    let mut parts = vec!["סעיפי הדוח שאושרו, לפי הסדר:".to_owned()];
    let mut ids = Vec::new();
    for (i, s) in input.sections.iter().enumerate() {
        let id = format!("R{}", i + 1);
        let attr = if s.closing {
            "closing_section"
        } else {
            "body_section"
        };
        parts.push(data_block(
            nonce,
            &format!("{attr}=\"{id}\" title=\"{}\"", s.title.replace('"', "'")),
            &s.text_tagged,
        ));
        ids.push(id);
    }
    set(
        &mut body,
        "messages",
        json!([{ "role": "user", "content": parts.join("\n\n") }]),
    );
    if let Ok(schema) = serde_json::from_str::<Value>(REPORT_CHECK_SCHEMA) {
        set_schema(&mut body, schema);
    }
    (body, ids)
}

#[derive(Deserialize)]
struct RawNote {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    sections: Vec<String>,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct RawReportCheck {
    notes: Vec<RawNote>,
}

/// Parse the answer. A section id that was not sent is dropped from a note (a note left with
/// none is still shown, without a place to jump to); an empty note is dropped; an unknown kind
/// reads as "other". A refusal or a cut answer is an error, never an empty list: no notes must
/// mean the report was read.
pub fn parse_report_check(response: &Value, ids: &[String]) -> Result<Vec<ReportNote>, AiError> {
    let text = text_of(response)?;
    let raw: RawReportCheck =
        serde_json::from_str(&text).map_err(|e| AiError::Format(e.to_string()))?;
    Ok(raw
        .notes
        .into_iter()
        .filter_map(|n| {
            let note: String = n.note.trim().chars().take(MAX_NOTE_CHARS).collect();
            if note.is_empty() {
                return None;
            }
            let kind = match n.kind.as_str() {
                "contradiction" => ReportNoteKind::Contradiction,
                "repetition" => ReportNoteKind::Repetition,
                "summary_only" => ReportNoteKind::SummaryOnly,
                _ => ReportNoteKind::Other,
            };
            let mut sections: Vec<String> = Vec::new();
            for s in n.sections {
                if ids.contains(&s) && !sections.contains(&s) {
                    sections.push(s);
                }
            }
            Some(ReportNote {
                kind,
                sections,
                note,
            })
        })
        .take(MAX_REPORT_NOTES)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(notes: &Value) -> Value {
        json!({
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": json!({ "notes": notes }).to_string()}]
        })
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn input() -> ReportCheckInput {
        ReportCheckInput {
            sections: vec![
                ReportCheckSection {
                    title: "רקע".to_owned(),
                    text_tagged: "[ילד] נולד בשבוע 40.".to_owned(),
                    closing: false,
                },
                ReportCheckSection {
                    title: "סיכום".to_owned(),
                    text_tagged: "[ילד] מתקשה במעברים.".to_owned(),
                    closing: true,
                },
            ],
        }
    }

    #[test]
    fn sections_go_out_in_order_and_the_schema_holds_nothing_from_a_case() {
        let (body, sent) = build_report_check_request(&ModelConfig::default(), &input(), "n1");
        assert_eq!(sent, ids(&["R1", "R2"]));
        let text = body["messages"][0]["content"].as_str().unwrap();
        assert!(text.contains("body_section=\"R1\" title=\"רקע\""));
        assert!(text.contains("closing_section=\"R2\" title=\"סיכום\""));
        assert!(text.find("נולד בשבוע").unwrap() < text.find("מתקשה במעברים").unwrap());
        let schema = body["output_config"]["format"]["schema"].to_string();
        assert!(!schema.contains("ילד") && !schema.contains("R1") && !schema.contains("רקע"));
    }

    #[test]
    fn unknown_sections_are_dropped_and_an_unknown_kind_is_still_shown() {
        let r = answer(&json!([
            {"kind": "contradiction", "sections": ["R1", "R9", "R1", "R2"], "note": " סותר "},
            {"kind": "weird", "sections": ["R7"], "note": "משהו"},
            {"kind": "repetition", "sections": ["R1"], "note": "   "}
        ]));
        let out = parse_report_check(&r, &ids(&["R1", "R2"])).unwrap();
        assert_eq!(out.len(), 2, "an empty note is dropped");
        assert_eq!(out[0].kind, ReportNoteKind::Contradiction);
        assert_eq!(out[0].sections, ids(&["R1", "R2"]));
        assert_eq!(out[0].note, "סותר");
        assert_eq!(out[1].kind, ReportNoteKind::Other);
        assert!(out[1].sections.is_empty());
    }

    #[test]
    fn a_long_answer_is_cut_to_a_list_to_read() {
        let many: Vec<Value> = (0..100)
            .map(|_| json!({"kind": "repetition", "sections": ["R1"], "note": "א".repeat(2_000)}))
            .collect();
        let out = parse_report_check(&answer(&json!(many)), &ids(&["R1"])).unwrap();
        assert_eq!(out.len(), MAX_REPORT_NOTES);
        assert_eq!(out[0].note.chars().count(), MAX_NOTE_CHARS);
    }

    #[test]
    fn a_refusal_or_a_cut_answer_is_an_error_not_a_clean_report() {
        let cut = json!({"stop_reason": "max_tokens", "content": []});
        assert_eq!(
            parse_report_check(&cut, &ids(&["R1"])).unwrap_err(),
            AiError::Truncated
        );
        let not_json =
            json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": "הכל תקין"}]});
        assert!(parse_report_check(&not_json, &ids(&["R1"])).is_err());
    }
}
