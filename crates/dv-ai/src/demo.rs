//! Demo mode: realistic answers built locally from the (tagged) sources, with no network and
//! no API key. Every answer is marked as a demo in the UI. Used until an API key with ZDR is
//! configured, and in tests.

use serde_json::{json, Value};

/// Pull `(id, body)` pairs out of the `<data nonce=… id="S#" …>` blocks of a request.
fn sources_in(text: &str) -> Vec<(String, String)> {
    blocks_with(text, "id")
}

/// `(title, body)` of the `<data nonce=… approved_section="…">` blocks: what a summary,
/// diagnoses or recommendations section is written from.
fn approved_in(text: &str) -> Vec<(String, String)> {
    blocks_with(text, "approved_section")
}

/// `(value of attr, body)` for every data block that has the attribute `attr`.
fn blocks_with(text: &str, attr: &str) -> Vec<(String, String)> {
    let needle = format!(" {attr}=\"");
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
        if let Some(id_at) = head.find(&needle) {
            let id: String = head[id_at + needle.len()..]
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

/// Words that point a passage to a section, for sorting in demo mode (D-022). Deliberately
/// simple: the real sorting reads the text; this only shows the flow without sending anything.
const SECTION_WORDS: &[(&str, &[&str])] = &[
    (
        "referral",
        &["הפני", "סיבת", "לברר", "פנו לאבחון", "פנו בשל"],
    ),
    (
        "background",
        &[
            "הריון",
            "לידה",
            "נולד",
            "אבני דרך",
            "הלך בגיל",
            "מילים ראשונות",
            "אלרגי",
            "בריאות",
        ],
    ),
    (
        "parents_view",
        &[
            "בבית",
            "לדברי ההורים",
            "לדברי האם",
            "לדברי האב",
            "ההורים מתארים",
            "ההורים מספרים",
        ],
    ),
    (
        "kindergarten",
        &[
            "בגן",
            "גננת",
            "הסייעת",
            "בכיתה",
            "המורה",
            "המסגרת",
            "בחצר",
            "מפגש בוקר",
        ],
    ),
    (
        "prior_assessments",
        &[
            "אבחון קודם",
            "טיפול",
            "קלינאית",
            "ריפוי בעיסוק",
            "פיזיותרפ",
            "נוירולוג",
            "התפתחות הילד",
        ],
    ),
    (
        "tools",
        &[
            "WPPSI",
            "WISC",
            "ABAS",
            "ADOS",
            "CBCL",
            "ASRS",
            "CARS",
            "Bayley",
            "הועבר",
        ],
    ),
    (
        "appearance",
        &["הגיע", "נכנס לחדר", "נפרד", "שיתף פעולה", "במפגש", "בחדר"],
    ),
    (
        "cognitive",
        &[
            "ציון",
            "אחוזון",
            "הבנה מילולית",
            "זיכרון",
            "מהירות עיבוד",
            "חשיבה",
            "VCI",
            "FSIQ",
            "WPPSI",
            "WISC",
        ],
    ),
    ("adaptive", &["ABAS", "הסתגלות", "תפקוד יומיומי", "עצמאות"]),
    (
        "communication",
        &[
            "שפה",
            "דיבור",
            "תקשורת",
            "משפטים",
            "קשר עין",
            "הצבעה",
            "ADOS",
            "תחומי עניין",
            "חזרתי",
            "הדדי",
        ],
    ),
    (
        "emotional",
        &[
            "משחק",
            "רגש",
            "תסכול",
            "חרדה",
            "פחד",
            "ויסות",
            "בובות",
            "כעס",
        ],
    ),
];

/// Demo-mode sorting: each passage goes to every section whose words it contains.
fn sort_locally(request: &str, keys: &[&str]) -> Value {
    let passages = sources_in(request);
    let sections: Vec<Value> = SECTION_WORDS
        .iter()
        .filter(|(key, _)| keys.contains(key))
        .filter_map(|(key, words)| {
            let ids: Vec<&str> = passages
                .iter()
                .filter(|(_, text)| words.iter().any(|w| text.contains(w)))
                .map(|(id, _)| id.as_str())
                .collect();
            (!ids.is_empty()).then(|| json!({ "section": key, "passages": ids }))
        })
        .collect();
    json!({ "sections": sections })
}

/// Style markers demo mode looks for in a past report, with what each one says about the style.
/// Deliberately simple: the real analysis reads the text; this only shows the flow.
const STYLE_MARKERS: &[(&str, &str, &str)] = &[
    ("עם זאת", "rule", "מבנה של חוזקה ואחריה הקושי, עם מילת מעבר"),
    ("כך לדוגמא", "phrase", "כך לדוגמא"),
    ("כך לדוגמה", "phrase", "כך לדוגמה"),
    ("ניכר", "phrase", "ניכר כי"),
    ("בשיח עימי", "rule", "גוף ראשון לתצפית של המאבחנת"),
    ("מגיב", "rule", "גוף נסתר וזמן הווה בתיאור הילד"),
    ("מגיבה", "rule", "גוף נסתר וזמן הווה בתיאור הילדה"),
    ("מומלץ", "phrase", "מומלץ על"),
    ("לסיכום", "phrase", "לסיכום,"),
];

/// Demo-mode style analysis (step 1) or synthesis (step 2), from the request only.
fn style_locally(request: &str, synthesis: bool) -> Value {
    if !synthesis {
        let mut items: Vec<Value> = Vec::new();
        for (section, text) in style_excerpts_in(request) {
            for (marker, kind, item) in STYLE_MARKERS {
                if text.contains(marker)
                    && !items
                        .iter()
                        .any(|i| i["text"] == *item && i["kind"] == *kind)
                {
                    let key = if *kind == "phrase" {
                        "general"
                    } else {
                        section.as_str()
                    };
                    items.push(json!({ "section": key, "kind": kind, "text": item }));
                }
            }
        }
        items.push(json!({
            "section": "general",
            "kind": "template",
            "text": "הציג תפקוד {רמה} ({ציון}), עם זאת ניכר קושי ב{תחום}"
        }));
        return json!({ "items": items });
    }
    // Merge the analyses: an item seen in several reports counts once, with its support.
    let mut merged: Vec<(String, String, String, u32)> = Vec::new();
    let mut sections: Vec<String> = Vec::new();
    for line in request.lines() {
        let parts: Vec<&str> = line.splitn(3, " | ").collect();
        let [section, kind, text] = parts[..] else {
            continue;
        };
        if section != "general" && !sections.iter().any(|s| s == section) {
            sections.push(section.to_owned());
        }
        match merged
            .iter_mut()
            .find(|(s, k, t, _)| s == section && k == kind && t == text)
        {
            Some(m) => m.3 += 1,
            None => merged.push((section.to_owned(), kind.to_owned(), text.to_owned(), 1)),
        }
    }
    let mut items: Vec<Value> = merged
        .into_iter()
        .map(|(s, k, t, n)| json!({ "section": s, "kind": k, "text": t, "support": n }))
        .collect();
    for section in sections {
        items.push(json!({
            "section": section,
            "kind": "example",
            "text": "מצב הדגמה: הילד מגיע למפגש בסקרנות ונענה לבקשות בשיתוף פעולה. עם זאת, ניכר כי במשימות ממושכות הוא זקוק לתיווך כדי להשלים אותן. כך לדוגמא, הציג תפקוד ממוצע ({מספר}) כשהמשימה חולקה לשלבים.",
            "support": 1
        }));
    }
    json!({ "items": items })
}

/// `(section, text)` of every excerpt in a style-analysis request.
fn style_excerpts_in(request: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = request;
    while let Some(start) = rest.find("section=\"") {
        let after = &rest[start + 9..];
        let Some(q) = after.find('"') else { break };
        let section = after[..q].to_owned();
        let body_start = after.find(">\n").map_or(q, |i| i + 2);
        let body = &after[body_start..];
        let end = body.find("\n</data").unwrap_or(body.len());
        out.push((section, body[..end].to_owned()));
        rest = &body[end..];
    }
    out
}

/// Answer a request body the way the real API would, from local material only.
#[must_use]
pub fn respond(body: &Value) -> Value {
    let last_user = body["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|x| x["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");
    let ai = body["model"]
        .as_str()
        .and_then(crate::Provider::of_model)
        .unwrap_or(crate::Provider::Anthropic)
        .display_name();

    if body.get("tools").is_some() {
        return api_text(
            "מצב הדגמה: כאן יופיע סיכום של מקורות מקצועיים שנמצאו בחיפוש (למשל APA ו-PubMed), \
             עם ציטוט לכל טענה. חיפוש אמיתי יתאפשר אחרי הגדרת מפתח API עם ZDR.",
        );
    }
    let schema = &body["output_config"]["format"]["schema"];
    if let Some(kinds) =
        schema["properties"]["items"]["items"]["properties"]["kind"]["enum"].as_array()
    {
        let synthesis = kinds.iter().any(|k| k == "example");
        return api_text(&style_locally(last_user, synthesis).to_string());
    }
    if let Some(keys) =
        schema["properties"]["sections"]["items"]["properties"]["section"]["enum"].as_array()
    {
        let keys: Vec<&str> = keys.iter().filter_map(Value::as_str).collect();
        return api_text(&sort_locally(last_user, &keys).to_string());
    }
    if body["output_config"]["format"].is_null() {
        return api_text(&format!(
            "מצב הדגמה: אני עונה כאן תשובה לדוגמה. בהתייעצות אמיתית {ai} יענה על השאלה המקצועית, \
             יבחין בין ידע מבוסס לדעה, ויציין כשיש אי-ודאות. ההחלטה המקצועית נשארת שלך."
        ));
    }

    let sources = sources_in(last_user);
    let approved = approved_in(last_user);
    if sources.is_empty() && !approved.is_empty() {
        // A section written from the approved ones (summary, diagnoses, recommendations).
        let paragraphs: Vec<Value> = approved
            .iter()
            .filter(|(_, body)| !body.trim().is_empty())
            .take(3)
            .map(|(_, body)| json!({ "text": first_sentences(body, 1), "source_refs": [] }))
            .collect();
        let answer = json!({
            "reply": format!("מצב הדגמה: ניסחתי {} פסקאות לדוגמה מתוך הסעיפים שאישרת. במצב אמיתי {ai} מסכם ומקשר בין הסעיפים בסגנון שלך.", paragraphs.len()),
            "paragraphs": paragraphs,
            "questions": [],
            "missing": [],
            "contradictions": []
        });
        return api_text(&answer.to_string());
    }
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
        format!("מצב הדגמה: ניסחתי {} פסקאות לדוגמה מתוך המקורות. במצב אמיתי {ai} מנסח בסגנון שלך ומצליב בין המקורות.", paragraphs.len())
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
    use crate::response::{parse_derived_section, parse_section};

    #[test]
    fn demo_sorting_parses_and_places_passages_by_their_words() {
        use crate::sort::{build_sort_request, parse_sort, SortInput, SortMaterial, SortSection};
        let section = |k: &str| SortSection {
            key: k.into(),
            about: String::new(),
        };
        let input = SortInput {
            sections: vec![
                section("background"),
                section("kindergarten"),
                section("cognitive"),
            ],
            materials: vec![SortMaterial {
                input_id: "i1".into(),
                kind_label: "אינטייק".into(),
                title_tagged: String::new(),
                passages_tagged: vec![
                    "ההריון והלידה עברו ללא סיבוכים.".into(),
                    "בגן [גננת] מתארת קושי במעברים.".into(),
                    "תודה רבה.".into(),
                ],
            }],
        };
        let body = build_sort_request(&ModelConfig::default(), &input, "n");
        let keys: Vec<String> = input.sections.iter().map(|s| s.key.clone()).collect();
        let reply = parse_sort(&respond(&body), &[3], &keys).unwrap();
        assert_eq!(reply.ignored, 0);
        let got: Vec<(&str, &[u32])> = reply.materials[0]
            .iter()
            .map(|s| (s.section.as_str(), s.passages.as_slice()))
            .collect();
        assert_eq!(
            got,
            vec![("background", &[1][..]), ("kindergarten", &[2][..])]
        );
    }

    #[test]
    fn demo_style_analysis_and_synthesis_parse_and_merge() {
        use crate::style::{
            build_style_analysis_request, build_style_synthesis_request, parse_style_analysis,
            parse_style_synthesis, StyleExcerpt, StyleKind,
        };
        let keys: Vec<String> = vec!["cognitive".into(), "summary".into()];
        let excerpt = |s: &str, t: &str| StyleExcerpt {
            section: s.into(),
            text: t.into(),
        };
        let body = build_style_analysis_request(
            &ModelConfig::default(),
            &keys,
            &[
                excerpt("cognitive", "[ילד] הציג תפקוד ממוצע. עם זאת, ניכר קושי."),
                excerpt("summary", "כך לדוגמא, [ילד] מגיב לתיווך."),
            ],
            "n",
        );
        let (one, dropped) = parse_style_analysis(&respond(&body), &keys).unwrap();
        assert_eq!(dropped, 0);
        assert!(one.iter().any(|i| i.text == "כך לדוגמא"));
        assert!(one
            .iter()
            .any(|i| i.section.as_deref() == Some("cognitive")));
        let body =
            build_style_synthesis_request(&ModelConfig::default(), &keys, &[one.clone(), one], "n");
        let (profile, dropped) = parse_style_synthesis(&respond(&body), &keys).unwrap();
        assert_eq!(dropped, 0);
        let phrase = profile.iter().find(|i| i.text == "כך לדוגמא").unwrap();
        assert_eq!(phrase.support, 2, "seen in both reports");
        assert!(profile.iter().any(|i| i.kind == StyleKind::Example));
    }

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

    #[test]
    fn demo_summary_is_written_from_the_approved_sections() {
        let input = SectionInput {
            section_key: "summary".into(),
            section_title: "סיכום".into(),
            age: None,
            gender: None,
            sources: vec![],
            approved_context: vec![
                (
                    "מסגרת חינוכית".into(),
                    "[ילד] מתקשה במעברים בגן. בקבוצה קטנה משתתף יותר.".into(),
                ),
                (
                    "תפקוד קוגניטיבי".into(),
                    "אוצר מילים: 9. מטריצות: 13.".into(),
                ),
            ],
            current_draft: vec![],
            history: vec![],
            instruction_tagged: "כתבי טיוטה לסעיף מתוך הסעיפים שאושרו.".into(),
            style_profile: None,
        };
        let (body, _) = build_section_request(&ModelConfig::default(), &input, "n");
        let approved: Vec<String> = input
            .approved_context
            .iter()
            .map(|(_, t)| t.clone())
            .collect();
        let reply = parse_derived_section(&respond(&body), &approved).unwrap();
        assert_eq!(reply.paragraphs.len(), 2);
        assert!(reply.paragraphs.iter().all(|p| p.warnings.is_empty()));
        assert!(reply.paragraphs[0].text.contains("מעברים"));
    }
}
