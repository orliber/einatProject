//! Properties of the filter and of local restoring: tags round-trip exactly, the same person
//! keeps one tag and different people never share one, filtering is deterministic and stable
//! on its own output, restoring never brings back text that was not there, and nothing panics
//! on any input. All data is invented.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

mod common;

use std::collections::HashMap;

use common::*;
use dv_domain::{Identity, PRACTITIONER_TAG};
use dv_privacy::restore::{remaining_tags, restore};
use dv_privacy::text::{normalize, prefix_splits, tokenize};
use dv_privacy::{clear, filter, AutoKind, GateRequest, PrivacyContext};
use proptest::prelude::*;

/// Words the filter leaves exactly as they are, alone and next to a name.
const FILLER: &[&str] = &[
    "מתקשה",
    "במעברים",
    "בין",
    "פעילויות",
    "היום",
    "בבוקר",
    "הגיע",
    "לגן",
    "שוב",
    "מאוד",
    "שיחק",
    "בחצר",
    "וצייר",
    "ציור",
    "גדול",
    "בשקט",
    "אמרה",
    "שהוא",
    "רוצה",
    "לשחק",
    ",",
    ".",
    "!",
    "?",
    ":",
    "(",
    ")",
    "\"",
    "\n",
];

const PREFIXES: &[&str] = &[
    "", "ו", "ה", "ב", "ל", "מ", "ש", "כ", "וב", "ול", "ומ", "וש", "של", "שב", "כש", "וכש", "ו-",
];

fn sentence(parts: &[(usize, usize, bool)]) -> String {
    // (filler index, name index, put a name with prefix here?)
    let mut out = Vec::new();
    for (f, n, name) in parts {
        if *name {
            let (word, _) = CASE_NAMES[n % CASE_NAMES.len()];
            out.push(format!("{}{word}", PREFIXES[f % PREFIXES.len()]));
        } else {
            out.push(FILLER[f % FILLER.len()].to_owned());
        }
    }
    out.join(" ")
}

fn parts() -> impl Strategy<Value = Vec<(usize, usize, bool)>> {
    prop::collection::vec((0usize..64, 0usize..64, prop::bool::weighted(0.35)), 1..14)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Declared names with any prefix in any surrounding text: every name becomes its tag
    /// with the prefix kept outside it, nothing else changes, and restoring the tags gives
    /// back exactly the original text, byte for byte.
    #[test]
    fn filter_then_restore_is_the_identity(p in parts()) {
        let ids = identities();
        let text = sentence(&p);
        let out = filter(&text, &ctx(&ids, CASE)).unwrap();
        for (name, _) in CASE_NAMES {
            prop_assert!(!contains_norm(&out.tagged, name), "{name} kept in {}", out.tagged);
        }
        prop_assert!(out.auto_hidden.is_empty(), "{:?}", out.auto_hidden);
        assert_segments_consistent(&text, &out);
        prop_assert_eq!(restore(&out.tagged, &ids, None), text);
        prop_assert!(gate_text(&out.tagged, &ids, CASE).is_ok(), "dead end on {}", out.tagged);
    }

    /// The same text filtered twice gives the same outcome, and filtering the filter's own
    /// output (a draft stored tagged and sent again) changes nothing.
    #[test]
    fn filtering_is_deterministic_and_idempotent(p in parts(), idn in 10_000_000u32..99_999_999) {
        let ids = identities();
        let c = ctx(&ids, CASE);
        let text = format!("{} ת.ז. {} נבדק ב-3.9.2026", sentence(&p), valid_id(&idn.to_string()));
        let a = filter(&text, &c).unwrap();
        let b = filter(&text, &c).unwrap();
        prop_assert_eq!(&a, &b);
        let again = filter(&a.tagged, &c).unwrap();
        prop_assert_eq!(&again.tagged, &a.tagged);
        prop_assert!(again.auto_hidden.is_empty(), "{:?}", again.auto_hidden);
    }

    /// One person, one tag; two people, two tags. Every occurrence of a declared name (any
    /// prefix, plene or defective spelling, niqqud) goes out as that person's tag, and the
    /// count of tags equals the count of occurrences.
    #[test]
    fn each_person_keeps_one_tag_and_people_never_share_one(p in parts()) {
        let ids = identities();
        let text = sentence(&p);
        let out = filter(&text, &ctx(&ids, CASE)).unwrap();
        let mut expected: HashMap<&str, usize> = HashMap::new();
        for (_, n, name) in &p {
            if *name {
                *expected.entry(CASE_NAMES[n % CASE_NAMES.len()].1).or_default() += 1;
            }
        }
        for (_, tag) in CASE_NAMES {
            let found = out.tagged.matches(tag).count();
            prop_assert_eq!(found, expected.get(tag).copied().unwrap_or(0), "{} in {}", tag, out.tagged);
        }
    }

    /// Restoring only ever puts back the value of a tag that was in the text: on any text
    /// without a known tag it changes nothing; each known tag becomes exactly its value; an
    /// unknown tag stays as it was.
    #[test]
    fn restore_brings_back_only_what_a_tag_stood_for(
        pieces in prop::collection::vec(prop_oneof![
            "[^\\[\\]]{0,12}",
            prop::sample::select(vec!["[ילד]", "[אם]", "[גננת]", "[גננת_9]", "[ילד_גן_1]", "[חסר: גיל]", "[מאבחנת]", "[", "]", "[]"]).prop_map(str::to_owned),
        ], 0..12),
        practitioner in prop::option::of(prop::sample::select(vec!["ד\"ר נטע לב", "מכון בדוי"])),
    ) {
        let ids = identities();
        let text: String = pieces.concat();
        let restored = restore(&text, &ids, practitioner);
        // What restoring adds is made only of the values of known tags it found.
        let mut expected = text.clone();
        for i in ids.iter().filter(|i| i.case_id == CASE && text.contains(&i.tag)) {
            expected = expected.replace(&i.tag, &i.value);
        }
        if let Some(p) = practitioner {
            expected = expected.replace(PRACTITIONER_TAG, p);
        }
        // `restore` matches tags left to right; on text built from whole pieces it agrees with
        // plain replacement whenever no tag was glued into another bracketed span.
        let glued = text.contains("[[") || text.contains("]]") || text.matches('[').count() != text.matches(']').count();
        if !glued {
            prop_assert_eq!(&restored, &expected);
        }
        // Never a name that had no tag.
        for i in &ids {
            let before = text.matches(&i.value).count();
            let tags = text.matches(&i.tag).count();
            let after = restored.matches(&i.value).count();
            prop_assert!(after <= before + tags, "{} appeared from nowhere: {}", i.value, restored);
        }
        if !text.contains('[') {
            prop_assert_eq!(&restored, &text);
        }
    }

    /// Nothing panics on any text: the filter, the gate, the tokenizer and the restorer
    /// return, tokens are well-formed spans of the input, and normalizing is idempotent.
    #[test]
    fn no_input_makes_anything_panic(text in "\\PC{0,80}|[\\u0591-\\u05F4\\u200B-\\u200F\\u202A-\\u202E\\u2066-\\u2069\\[\\]\"'׳״ 0-9a-z.,-]{0,80}") {
        let ids = identities();
        let c = ctx(&ids, CASE);
        let out = filter(&text, &c).unwrap();
        assert_segments_consistent(&text, &out);
        let _ = gate_text(&text, &ids, CASE);
        let _ = gate_text(&out.tagged, &ids, CASE);
        let _ = restore(&text, &ids, Some("x"));
        let _ = remaining_tags(&text);
        let mut last_end = 0;
        for t in tokenize(&text) {
            prop_assert!(t.start >= last_end && t.start < t.end && t.end <= text.len());
            prop_assert!(text.is_char_boundary(t.start) && text.is_char_boundary(t.end));
            prop_assert!(!t.norm.is_empty());
            prop_assert_eq!(normalize(&t.norm), t.norm.clone());
            for (p, rest) in prefix_splits(&t.norm) {
                prop_assert!(rest.chars().count() + p == t.norm.chars().count());
            }
            last_end = t.end;
        }
        let n = normalize(&text);
        prop_assert_eq!(normalize(&n), n.clone());
        prop_assert!(!n.starts_with(' ') && !n.ends_with(' ') && !n.contains("  "));
    }
}

#[test]
fn empty_blank_and_mark_only_inputs_pass_through_unchanged() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    for text in [
        "",
        " ",
        "\n\n\t",
        "\u{05B0}\u{05B4}\u{05BC}",
        &"\u{200F}\u{200E}\u{FEFF}".repeat(40),
        "[]",
        "[[[]]]",
        "[",
        "]",
        "...",
        "״״׳׳",
    ] {
        let out = filter(text, &c).unwrap();
        assert_eq!(out.tagged, text, "«{text:?}»");
        assert!(out.auto_hidden.is_empty());
        assert_segments_consistent(text, &out);
    }
}

#[test]
fn very_long_input_is_filtered_completely() {
    // About 60 KB: a name near the start, in the middle and at the very end must all go.
    let ids = identities();
    let c = ctx(&ids, CASE);
    let filler = "הילד מתקשה במעברים בין פעילויות ושיחק בחצר. ".repeat(700);
    let text = format!("נועם {filler} ולרותם {filler} עם אביטל");
    let out = filter(&text, &c).unwrap();
    for name in ["נועם", "רותם", "אביטל"] {
        assert!(!out.tagged.contains(name), "{name} kept");
    }
    assert!(out.tagged.starts_with("[ילד] ") && out.tagged.ends_with("עם [גננת]"));
    assert_eq!(restore(&out.tagged, &ids, None), text);
    // One long word, and a long run of one letter.
    let word = "א".repeat(20_000);
    assert_eq!(filter(&word, &c).unwrap().tagged, word);
    let digits = "7".repeat(5_000);
    let out = filter(&digits, &c).unwrap();
    assert!(!out.tagged.contains("77777"), "a long digit run is hidden");
}

#[test]
fn found_names_get_distinct_tags_and_repeats_share_one() {
    // Two teachers the case never listed, each named twice: two tags, each used twice.
    let ids = identities();
    let text = "הגננת רינת והגננת סיגלית שוחחו. אחר כך רינת הלכה, וסיגלית נשארה.";
    let out = filter(text, &ctx(&ids, CASE)).unwrap();
    let tag_of = |token: &str| {
        out.auto_hidden
            .iter()
            .find(|a| a.token == token && a.kind == AutoKind::Name)
            .map(|a| a.tag.clone())
            .unwrap_or_else(|| panic!("{token} not hidden: {:?}", out.auto_hidden))
    };
    let (a, b) = (tag_of("רינת"), tag_of("סיגלית"));
    assert_ne!(a, b);
    for tag in [&a, &b] {
        // Not one of the case's declared tags.
        assert!(
            !ids.iter().any(|i| i.case_id == CASE && &i.tag == tag),
            "{tag}"
        );
        assert_eq!(
            out.tagged.matches(tag.as_str()).count(),
            2,
            "{}",
            out.tagged
        );
    }
    assert!(!out.tagged.contains("רינת") && !out.tagged.contains("סיגלית"));
}

#[test]
fn another_cases_name_never_gets_this_cases_tag() {
    // "ינאי" is the other case's child. In this case it must not become "[ילד]" (which
    // would restore as this child's name), only a tag of its own.
    let ids = identities();
    let out = filter("נועם וינאי שיחקו יחד.", &ctx(&ids, CASE)).unwrap();
    assert!(out.tagged.starts_with("[ילד] ו"), "{}", out.tagged);
    assert_eq!(out.tagged.matches("[ילד]").count(), 1, "{}", out.tagged);
    assert!(!out.tagged.contains("ינאי"));
    let other = out
        .auto_hidden
        .iter()
        .find(|a| a.token == "ינאי")
        .expect("listed on the card");
    assert_eq!(other.kind, AutoKind::OtherCase);
    // Restoring this case's tags never writes the other child's name.
    let restored = restore(&out.tagged, &ids, None);
    assert!(restored.starts_with("נועם ו"), "{restored}");
    assert!(!restored.contains("ינאי"));
}

#[test]
fn restore_keeps_unknown_tags_and_reports_them() {
    let ids = identities();
    let tagged = "[ילד] ו[גננת_7] ו[מאבחנת] [חסר: תאריך] [ת.ז.]";
    let restored = restore(tagged, &ids, None);
    assert_eq!(restored, "נועם ו[גננת_7] ו[מאבחנת] [חסר: תאריך] [ת.ז.]");
    // An export must notice every tag left, but never a "[חסר …]" note.
    assert_eq!(
        remaining_tags(&restored),
        vec![
            "[גננת_7]".to_owned(),
            "[מאבחנת]".to_owned(),
            "[ת.ז.]".to_owned()
        ]
    );
    let with_p = restore(tagged, &ids, Some("ד\"ר נטע לב"));
    assert!(with_p.contains("וד\"ר נטע לב"), "{with_p}");
    // Another case's identities with the same tag are never used: the first matching
    // identity wins, and the caller passes only this case's (dv-export does).
    let only_other: Vec<Identity> = ids.iter().filter(|i| i.case_id == OTHER).cloned().collect();
    assert!(restore("[ילד]", &only_other, None).contains("ינאי"));
}

#[test]
fn the_practitioner_is_hidden_and_restored_with_her_own_tag() {
    let ids = identities();
    let practitioner = vec!["נטע לבנון".to_owned()];
    let c = PrivacyContext {
        practitioner: &practitioner,
        ..ctx(&ids, CASE)
    };
    let text = "לנטע לבנון נמסר שנועם הגיע.";
    let out = filter(text, &c).unwrap();
    assert_eq!(out.tagged, format!("ל{PRACTITIONER_TAG} נמסר ש[ילד] הגיע."));
    let tags = case_tags(&ids, CASE);
    let body = serde_json::json!({"messages": [{"role": "user", "content": out.tagged}]});
    let req = GateRequest {
        body: &body,
        ctx: &c,
        case_tags: &tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 10_000,
    };
    assert!(clear(&req).is_ok());
    assert_eq!(restore(&out.tagged, &ids, Some("נטע לבנון")), text);
}

#[test]
fn a_tag_inside_brackets_written_by_someone_else_is_not_trusted() {
    // Square brackets around ordinary words become round ones, so the words inside are
    // filtered and never pass for a tag; a name inside is hidden.
    let ids = identities();
    let out = filter("[הערה של נועם] ו[מחנכת]", &ctx(&ids, CASE)).unwrap();
    assert_eq!(out.tagged, "(הערה של [ילד]) ו(מחנכת)");
}
