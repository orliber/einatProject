//! Demo mode: realistic answers built locally from the (tagged) sources, with no network and
//! no API key. Every answer is marked as a demo in the UI. Used until an API key with ZDR is
//! configured, and in tests.

use serde_json::{json, Value};

/// Pull `(id, body)` pairs out of the `<data nonce=… id="S#" …>` blocks of a request.
fn sources_in(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("<data nonce=") {
        let after = &rest[open..];
        let Some(head_end) = after.find(">\n") else {
            break;
        };
        let head = &after[..head_end];
        let body_start = head_end + 2;
        let Some(close) = after[body_start..].find("\n</data nonce=") else {
            break;
        };
        let body = &after[body_start..body_start + close];
        if let Some(id_at) = head.find("id=\"") {
            let id: String = head[id_at + 4..]
                .chars()
                .take_while(|c| *c != '"')
                .collect();
            out.push((id, body.to_owned()));
        }
        rest = &after[body_start + close + 1..];
    }
    out
}

fn first_sentences(text: &str, n: usize) -> String {
    let mut out = String::new();
    for (count, part) in text.split_inclusive(['.', '!', '?']).enumerate() {
        if count >= n {
            break;
        }
        out.push_str(part);
    }
    out.trim().to_owned()
}

fn api_text(text: &str) -> Value {
    json!({
        "id": "demo",
        "type": "message",
        "model": "demo",
        "stop_reason": "end_turn",
        "content": [{"type": "text", "text": text}],
        "usage": {"input_tokens": 0, "output_tokens": 0}
    })
}

/// Answer a request body the way the real API would, from local material only.
#[must_use]
pub fn respond(body: &Value) -> Value {
    let last_user = body["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|x| x["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");

    if body.get("tools").is_some() {
        return api_text(
            "מצב הדגמה: כאן יופיע סיכום של מקורות מקצועיים שנמצאו בחיפוש (למשל APA ו-PubMed), \
             עם ציטוט לכל טענה. חיפוש אמיתי יתאפשר אחרי הגדרת מפתח API עם ZDR.",
        );
    }
    if body["output_config"]["format"].is_null() {
        return api_text(
            "מצב הדגמה: אני עונה כאן תשובה לדוגמה. בהתייעצות אמיתית Claude יענה על השאלה המקצועית, \
             יבחין בין ידע מבוסס לדעה, ויציין כשיש אי-ודאות. ההחלטה המקצועית נשארת שלך.",
        );
    }

    let sources = sources_in(last_user);
    let paragraphs: Vec<Value> = sources
        .iter()
        .take(3)
        .filter(|(_, body)| !body.trim().is_empty())
        .map(|(id, body)| json!({ "text": first_sentences(body, 2), "source_refs": [id] }))
        .collect();
    let reply = if paragraphs.is_empty() {
        "מצב הדגמה: אין עדיין מקורות לסעיף הזה. הוסיפי אינטייק, שיחה או מסמך, ואנסח טיוטה על סמכם."
            .to_owned()
    } else {
        format!("מצב הדגמה: ניסחתי {} פסקאות לדוגמה מתוך המקורות. במצב אמיתי Claude מנסח בסגנון שלך ומצליב בין המקורות.", paragraphs.len())
    };
    let answer = json!({
        "reply": reply,
        "paragraphs": paragraphs,
        "questions": ["האם יש מידע נוסף מהמסגרת החינוכית על ההשתתפות במפגשי הבוקר?"],
        "missing": if sources.is_empty() { vec!["מקורות לסעיף"] } else { Vec::new() },
        "contradictions": []
    });
    api_text(&answer.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{build_section_request, ModelConfig, SectionInput, TaggedInput};
    use crate::response::parse_section;

    #[test]
    fn demo_answers_parse_and_cite_real_sources() {
        let input = SectionInput {
            section_key: "kindergarten".into(),
            section_title: "מסגרת חינוכית".into(),
            age: None,
            gender: None,
            sources: vec![TaggedInput {
                input_id: "i1".into(),
                kind_label: "מסגרת חינוכית".into(),
                title_tagged: "שיחה".into(),
                content_tagged:
                    "[גננת] סיפרה ש[ילד] מתקשה במעברים. בקבוצה קטנה משתתף יותר. בחצר פחות.".into(),
            }],
            approved_context: vec![],
            current_draft: vec![],
            history: vec![],
            instruction_tagged: "נסחי".into(),
            style_profile: None,
        };
        let (body, refs) = build_section_request(&ModelConfig::default(), &input, "n");
        let sources: Vec<(String, String)> = refs
            .iter()
            .map(|(s, _)| (s.clone(), input.sources[0].content_tagged.clone()))
            .collect();
        let reply = parse_section(&respond(&body), &sources).unwrap();
        assert_eq!(reply.paragraphs.len(), 1);
        assert_eq!(reply.paragraphs[0].source_refs, vec!["S1"]);
        assert!(reply.paragraphs[0].warnings.is_empty());
        assert!(reply.reply.starts_with("מצב הדגמה"));
    }
}
