//! Properties of reading the model's answers: the structured output is taken as data and as
//! plain text (never markup to act on), a broken answer is an error rather than an empty
//! result, the local checks flag numbers and sources that were not sent, and the sorting and
//! verification answers are reduced to what the request actually contained, deterministically.
//!
//! All text here is invented.

#![allow(clippy::indexing_slicing)]

use dv_ai::{
    parse_consult, parse_derived_section, parse_section, parse_sort, parse_style_analysis,
    parse_style_synthesis, parse_verify, passage_id, AiError, StyleKind, Verdict,
};
use proptest::prelude::*;
use serde_json::{json, Value};

fn api(text: &str) -> Value {
    json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": text}]})
}

fn reply_with(paragraphs: &Value) -> Value {
    api(&json!({
        "reply": "ניסחתי",
        "paragraphs": paragraphs,
        "questions": [], "missing": [], "contradictions": []
    })
    .to_string())
}

fn sources(text: &str) -> Vec<(String, String)> {
    vec![("S1".to_owned(), text.to_owned())]
}

#[test]
fn a_section_answer_of_the_wrong_shape_is_a_format_error() {
    let broken = [
        "",
        "not json",
        "{\"reply\": \"x\"",
        // A required field missing.
        r#"{"reply": "x", "paragraphs": [], "questions": []}"#,
        // A paragraph without its sources.
        r#"{"reply": "x", "paragraphs": [{"text": "t"}], "questions": [], "missing": []}"#,
        // Wrong types.
        r#"{"reply": 1, "paragraphs": [], "questions": [], "missing": []}"#,
        r#"{"reply": "x", "paragraphs": "t", "questions": [], "missing": []}"#,
        r#"{"reply": "x", "paragraphs": [{"text": "t", "source_refs": "S1"}], "questions": [], "missing": []}"#,
        r#"[]"#,
        "null",
    ];
    for text in broken {
        assert!(
            matches!(
                parse_section(&api(text), &sources("x")),
                Err(AiError::Format(_))
            ),
            "{text}"
        );
    }
    // No content array at all.
    assert!(matches!(
        parse_section(&json!({"stop_reason": "end_turn"}), &[]),
        Err(AiError::Format(_))
    ));
}

#[test]
fn fields_outside_the_contract_never_reach_the_reply() {
    let text = json!({
        "reply": "ניסחתי",
        "paragraphs": [{"text": "פסקה", "source_refs": ["S1"], "html": "<b>x</b>", "approved": true}],
        "questions": [], "missing": [], "contradictions": [],
        "draft_update": {"section_key": "diagnoses"},
        "script": "<script>alert(1)</script>"
    })
    .to_string();
    let out = parse_section(&api(&text), &sources("פסקה")).unwrap();
    let back = serde_json::to_string(&out).unwrap();
    for word in [
        "html",
        "approved",
        "draft_update",
        "diagnoses",
        "script",
        "alert",
    ] {
        assert!(!back.contains(word), "{word} leaked into the reply");
    }
    assert_eq!(out.paragraphs[0].text, "פסקה");
}

#[test]
fn markup_in_an_answer_stays_the_literal_text_it_was() {
    // The parser neither strips nor interprets markup: the UI shows it as text, so what
    // Einat reads is exactly what the model wrote, and nothing in it can run.
    let markup = "<img src=x onerror=alert(1)> &amp; <b>מודגש</b>";
    let out = parse_section(
        &reply_with(&json!([{"text": markup, "source_refs": ["S1"]}])),
        &sources("x 1"),
    )
    .unwrap();
    assert_eq!(out.paragraphs[0].text, markup);
    assert_eq!(parse_consult(&api(markup)).unwrap(), markup);
}

#[test]
fn only_text_blocks_are_read_never_thinking_or_tool_blocks() {
    let response = json!({
        "stop_reason": "end_turn",
        "content": [
            {"type": "thinking", "thinking": "SECRET-THOUGHT", "signature": "s"},
            {"type": "text", "text": "חלק א "},
            {"type": "tool_use", "id": "t", "name": "x", "input": {"text": "TOOL-INPUT"}},
            {"type": "server_tool_use", "id": "u", "name": "web_search", "input": {}},
            {"type": "web_search_tool_result", "tool_use_id": "u", "content": [{"text": "RESULT"}]},
            {"type": "text", "text": "חלק ב"},
            {"type": "html", "text": "<script>HTML-BLOCK</script>"}
        ]
    });
    assert_eq!(parse_consult(&response).unwrap(), "חלק א חלק ב");
}

#[test]
fn a_refusal_or_a_cut_answer_is_never_read_as_content() {
    // Even a complete, valid JSON answer is not used when the model stopped for these reasons.
    let good = json!({
        "reply": "x", "paragraphs": [], "questions": [], "missing": [], "contradictions": []
    })
    .to_string();
    let mut refused = api(&good);
    refused["stop_reason"] = json!("refusal");
    assert_eq!(
        parse_section(&refused, &[]),
        Err(AiError::Refused("unspecified".to_owned()))
    );
    let mut cut = api(&good);
    cut["stop_reason"] = json!("max_tokens");
    assert_eq!(parse_section(&cut, &[]), Err(AiError::Truncated));
    assert_eq!(parse_derived_section(&cut, &[]), Err(AiError::Truncated));
}

#[test]
fn a_number_is_matched_whole_not_as_part_of_a_longer_one() {
    let out = parse_section(
        &reply_with(&json!([
            {"text": "ציון 11 במטלה.", "source_refs": ["S1"]},
            {"text": "ציון 111 במטלה.", "source_refs": ["S1"]},
            {"text": "בגיל 5:4.", "source_refs": ["S1"]},
            {"text": "בגיל 5.", "source_refs": ["S1"]}
        ])),
        &sources("ציון 111. גיל 5:4."),
    )
    .unwrap();
    assert!(out.paragraphs[0].warnings.iter().any(|w| w.contains("11")));
    assert!(out.paragraphs[1].warnings.is_empty());
    assert!(out.paragraphs[2].warnings.is_empty());
    assert!(
        out.paragraphs[3].warnings.iter().any(|w| w.contains('5')),
        "the 5 of 5:4 is not the number 5"
    );
}

#[test]
fn a_number_found_only_in_a_source_the_paragraph_did_not_cite_is_still_flagged() {
    let out = parse_section(
        &reply_with(&json!([{"text": "ציון 83 בשאלון.", "source_refs": ["S1"]}])),
        &[
            ("S1".to_owned(), "שאלון הורים.".to_owned()),
            ("S2".to_owned(), "ציון 83.".to_owned()),
        ],
    )
    .unwrap();
    assert!(out.paragraphs[0].warnings.iter().any(|w| w.contains("83")));
}

#[test]
fn a_verification_answer_of_the_wrong_shape_is_an_error_not_a_pass() {
    let ids = vec!["P1".to_owned()];
    for text in [
        "",
        "{}",
        r#"{"results": "supported"}"#,
        r#"{"results": [{"verdict": "supported"}]}"#,
    ] {
        assert!(
            matches!(parse_verify(&api(text), &ids, &[]), Err(AiError::Format(_))),
            "{text}"
        );
    }
}

#[test]
fn sorting_is_deterministic_whatever_order_the_answer_lists_things_in() {
    let sections = vec!["background".to_owned(), "cognitive".to_owned()];
    let a = json!({"sections": [
        {"section": "background", "passages": ["S1P3", "S1P1", "S2P2"]},
        {"section": "cognitive", "passages": ["S1P2"]}
    ]});
    let b = json!({"sections": [
        {"section": "background", "passages": ["S2P2", "S1P1"]},
        {"section": "cognitive", "passages": ["S1P2", "S1P2"]},
        {"section": "background", "passages": ["S1P3", "S1P1"]}
    ]});
    let ra = parse_sort(&api(&a.to_string()), &[3, 2], &sections).unwrap();
    let rb = parse_sort(&api(&b.to_string()), &[3, 2], &sections).unwrap();
    assert_eq!(ra.materials, rb.materials);
    assert_eq!(ra.materials[0][0].passages, vec![1, 3]);
}

#[test]
fn style_items_with_markup_tags_or_runaway_length_are_dropped() {
    let keys = vec!["cognitive".to_owned()];
    let items = json!({"items": [
        {"section": "general", "kind": "rule", "text": "<b>כלל</b> מודגש"},
        {"section": "general", "kind": "rule", "text": "כלל עם [תגית]"},
        {"section": "general", "kind": "rule", "text": "א".repeat(301)},
        {"section": "cognitive", "kind": "example", "text": "ב".repeat(1_201)},
        {"section": "cognitive", "kind": "example", "text": "ג".repeat(1_200)},
        {"section": "general", "kind": "rule", "text": "   "},
        {"section": "general", "kind": "unknown", "text": "סוג לא קיים"},
        {"section": "general", "kind": "rule", "text": "כלל תקין", "support": -3},
        {"section": "general", "kind": "rule", "text": "כלל נתמך", "support": 4}
    ]});
    let (kept, dropped) = parse_style_synthesis(&api(&items.to_string()), &keys).unwrap();
    assert_eq!(dropped, 6);
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[0].kind, StyleKind::Example);
    assert_eq!(kept[1].support, 1, "a negative support reads as one report");
    assert_eq!(kept[2].support, 4);
    // Analysis takes no examples at all.
    let (kept, _) = parse_style_analysis(&api(&items.to_string()), &keys).unwrap();
    assert!(kept.iter().all(|i| i.kind != StyleKind::Example));
}

#[test]
fn style_answers_hold_at_most_the_item_limit() {
    let many: Vec<Value> = (0..200)
        .map(|i| json!({"section": "general", "kind": "rule", "text": format!("כלל מספר {i}")}))
        .collect();
    let text = json!({ "items": many }).to_string();
    let (kept, dropped) = parse_style_analysis(&api(&text), &[]).unwrap();
    assert_eq!((kept.len(), dropped), (60, 140));
    let (kept, dropped) = parse_style_synthesis(&api(&text), &[]).unwrap();
    assert_eq!((kept.len(), dropped), (160, 40));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// A number the paragraph states that no cited source holds is always flagged; one that
    /// a cited source holds never is.
    #[test]
    fn invented_numbers_are_always_flagged(
        known in 1u32..500,
        invented in 1u32..500,
    ) {
        prop_assume!(known != invented);
        let src = sources(&format!("ציון {known} במטלה."));
        let out = parse_section(
            &reply_with(&json!([
                {"text": format!("הציג תפקוד ממוצע ({known})."), "source_refs": ["S1"]},
                {"text": format!("הציג תפקוד ממוצע ({invented})."), "source_refs": ["S1"]}
            ])),
            &src,
        ).unwrap();
        prop_assert!(out.paragraphs[0].warnings.is_empty());
        let flag = format!("המספר {invented} ");
        prop_assert!(out.paragraphs[1].warnings.iter().any(|w| w.contains(&flag)));

        // The same holds for sections written from the approved sections.
        let out = parse_derived_section(
            &reply_with(&json!([{"text": format!("ציון {invented}."), "source_refs": []}])),
            &[format!("ציון {known}.")],
        ).unwrap();
        prop_assert!(out.paragraphs[0].warnings.iter().any(|w| w.contains(&flag)));
    }

    /// Every cited source that was not sent is named in a warning.
    #[test]
    fn every_unknown_source_is_flagged(n in 2u32..40) {
        let id = format!("S{n}");
        let out = parse_section(
            &reply_with(&json!([{"text": "פסקה", "source_refs": ["S1", id.clone()]}])),
            &sources("פסקה"),
        ).unwrap();
        let w = &out.paragraphs[0].warnings;
        prop_assert_eq!(w.len(), 1);
        prop_assert!(w[0].contains(&id));
    }

    /// Any answer text at all: an error or a reply, never a panic.
    #[test]
    fn arbitrary_answer_text_never_panics(text in "\\PC{0,200}") {
        let _ = parse_section(&api(&text), &sources("1 2 3"));
        let _ = parse_derived_section(&api(&text), &["1".to_owned()]);
        let _ = parse_verify(&api(&text), &["P1".to_owned()], &["S1".to_owned()]);
        let _ = parse_sort(&api(&text), &[2], &["background".to_owned()]);
    }

    /// One check per paragraph sent, in the order sent; only sources that were sent; a
    /// paragraph the answer skipped or gave an unknown verdict reads as not checked.
    #[test]
    fn verification_is_reduced_to_what_was_sent(
        results in prop::collection::vec(
            (0u32..6, prop::sample::select(vec!["supported", "partial", "unsupported", "contradicted", "fine", ""]),
             prop::collection::vec(0u32..6, 0..4)),
            0..8,
        ),
        sent_paragraphs in 1usize..5,
        sent_sources in 0usize..4,
    ) {
        let raw: Vec<Value> = results.iter().map(|(p, v, refs)| json!({
            "paragraph": format!("P{p}"),
            "verdict": v,
            "notes": [{"note": "הערה", "source_refs": refs.iter().map(|r| format!("S{r}")).collect::<Vec<_>>()}]
        })).collect();
        let pids: Vec<String> = (1..=sent_paragraphs).map(|i| format!("P{i}")).collect();
        let sids: Vec<String> = (1..=sent_sources).map(|i| format!("S{i}")).collect();
        let out = parse_verify(&api(&json!({"results": raw}).to_string()), &pids, &sids).unwrap();
        prop_assert_eq!(out.len(), pids.len());
        for (check, pid) in out.iter().zip(&pids) {
            prop_assert_eq!(&check.paragraph, pid);
            for n in &check.notes {
                prop_assert!(n.source_refs.iter().all(|r| sids.contains(r)));
            }
            let first = results.iter().find(|(p, _, _)| &format!("P{p}") == pid);
            let expected = match first.map(|(_, v, _)| *v) {
                Some("supported") => Verdict::Supported,
                Some("partial") => Verdict::Partial,
                Some("unsupported") => Verdict::Unsupported,
                Some("contradicted") => Verdict::Contradicted,
                _ => Verdict::Unchecked,
            };
            prop_assert_eq!(check.verdict, expected);
        }
    }

    /// Whatever ids the answer holds, the sorting keeps only real passages of real materials
    /// in the listed sections, each once, in ascending order, and counts the rest.
    #[test]
    fn sorting_keeps_only_passages_that_were_sent(
        counts in prop::collection::vec(0usize..5, 1..4),
        items in prop::collection::vec(
            (prop::sample::select(vec!["background", "cognitive", "invented"]),
             prop::collection::vec(
                 prop_oneof![
                     (0usize..6, 0usize..7).prop_map(|(m, p)| passage_id(m, p)),
                     "\\PC{0,8}",
                 ],
                 0..6,
             )),
            0..6,
        ),
    ) {
        let sections = vec!["background".to_owned(), "cognitive".to_owned()];
        let answer = json!({"sections": items.iter().map(|(s, p)| json!({"section": s, "passages": p})).collect::<Vec<_>>()});
        let r = parse_sort(&api(&answer.to_string()), &counts, &sections).unwrap();
        prop_assert_eq!(r.materials.len(), counts.len());
        let mut kept = 0usize;
        for (list, count) in r.materials.iter().zip(&counts) {
            for s in list {
                prop_assert!(sections.contains(&s.section));
                prop_assert!(s.passages.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats");
                prop_assert!(s.passages.iter().all(|p| (1..=*count).contains(&(*p as usize))));
                kept += s.passages.len();
            }
            let mut names: Vec<&str> = list.iter().map(|s| s.section.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            prop_assert_eq!(names.len(), list.len(), "one entry per section");
        }
        let total: usize = items.iter().map(|(s, p)| if *s == "invented" { 1 } else { p.len() }).sum();
        prop_assert!(kept + r.ignored as usize <= total);
        // The same answer again gives exactly the same sorting.
        let again = parse_sort(&api(&answer.to_string()), &counts, &sections).unwrap();
        prop_assert_eq!(again, r);
    }
}
