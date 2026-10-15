//! Properties of the filter and of local restoring: tags round-trip exactly, the same person
//! keeps one tag and different people never share one, filtering is deterministic and stable
//! on its own output, restoring never brings back text that was not there, and nothing panics
//! on any input. All data is invented.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::indexing_slicing
)]

mod common;

use std::collections::HashMap;

use common::*;
use dv_domain::{Identity, PRACTITIONER_TAG};
use dv_privacy::restore::{remaining_tags, restore, scan_model_output};
use dv_privacy::text::{normalize, prefix_splits, tokenize};
use dv_privacy::{
    clear, filter, filter_split, filter_split_with_hints, filter_with_hints, AutoKind, GateRequest,
    NameHint, PrivacyContext,
};
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
        prop_assert_eq!(restore(&out.tagged, &ids, None, &[]), text);
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
        let restored = restore(&text, &ids, practitioner, &[]);
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
        let _ = restore(&text, &ids, Some("x"), &[]);
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
    assert_eq!(restore(&out.tagged, &ids, None, &[]), text);
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
    let restored = restore(&out.tagged, &ids, None, &[]);
    assert!(restored.starts_with("נועם ו"), "{restored}");
    assert!(!restored.contains("ינאי"));
}

#[test]
fn restore_keeps_unknown_tags_and_reports_them() {
    let ids = identities();
    let tagged = "[ילד] ו[גננת_7] ו[מאבחנת] [חסר: תאריך] [ת.ז.]";
    let restored = restore(tagged, &ids, None, &[]);
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
    let with_p = restore(tagged, &ids, Some("ד\"ר נטע לב"), &[]);
    assert!(with_p.contains("וד\"ר נטע לב"), "{with_p}");
    // Another case's identities with the same tag are never used: the first matching
    // identity wins, and the caller passes only this case's (dv-export does).
    let only_other: Vec<Identity> = ids.iter().filter(|i| i.case_id == OTHER).cloned().collect();
    assert!(restore("[ילד]", &only_other, None, &[]).contains("ינאי"));
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
    assert_eq!(restore(&out.tagged, &ids, Some("נטע לבנון"), &[]), text);
}

#[test]
fn a_tag_inside_brackets_written_by_someone_else_is_not_trusted() {
    // Square brackets around ordinary words become round ones, so the words inside are
    // filtered and never pass for a tag; a name inside is hidden.
    let ids = identities();
    let out = filter("[הערה של נועם] ו[מחנכת]", &ctx(&ids, CASE)).unwrap();
    assert_eq!(out.tagged, "(הערה של [ילד]) ו(מחנכת)");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// The model only ever saw tags: a declared name of any case in its answer (any prefix,
    /// niqqud, an invisible character inside), an identifier, or a tag this case does not
    /// have is always pointed out; this case's tags and generic tags never are.
    #[test]
    fn the_answer_scan_points_out_every_name_identifier_and_foreign_tag(
        name in prop::sample::select(vec!["נועם", "רותם", "עידו", "אביטל", "ינאי", "אסנת"]),
        prefix in prop::sample::select(vec!["", "ו", "ל", "ש", "וכש"]),
        inside in prop::sample::select(vec!["", "\u{05B9}", "\u{200B}", "\u{200F}"]),
        foreign in prop::sample::select(vec!["[גננת_4]", "[ילד_גן_2]", "[אדם_9]"]),
        filler in prop::collection::vec(prop::sample::select(FILLER.to_vec()), 0..6),
    ) {
        let ids = identities();
        let tags: Vec<String> = case_tags(&ids, CASE).into_iter().collect();
        let mut spelled: Vec<char> = name.chars().collect();
        spelled.insert(2, inside.chars().next().unwrap_or('\u{05B0}'));
        let spelled: String = spelled.into_iter().collect();
        let answer = format!("[ילד] {} {prefix}{spelled} ו[אם] [ת.ז.] {}", filler.join(" "), filler.join(" "));
        let found = scan_model_output(&answer, &tags, &ids);
        prop_assert!(
            found.iter().any(|s| normalize(&s.token).contains(&normalize(name))),
            "{name} not pointed out in {answer:?}: {found:?}"
        );
        prop_assert!(!found.iter().any(|s| s.token == "[ילד]" || s.token == "[אם]" || s.token == "[ת.ז.]"));
        let with_tag = format!("{} {foreign}", filler.join(" "));
        prop_assert!(scan_model_output(&with_tag, &tags, &ids).iter().any(|s| s.token == foreign));
        let idn = valid_id("55443322");
        let with_id = format!("{} {idn}", filler.join(" "));
        prop_assert!(scan_model_output(&with_id, &tags, &ids).iter().any(|s| s.token.contains(&idn)));
        prop_assert!(scan_model_output(&filler.join(" "), &tags, &ids).is_empty());
    }
}

/// A tag after an institution word and a quote (`לגן " [ילד] "`, a draft stored tagged) was
/// taken for the kindergarten's name: filtering it again cut the tag to `לגן]`. Found by
/// `filtering_is_deterministic_and_idempotent`.
#[test]
fn a_tag_after_an_institution_word_and_a_quote_stays_whole() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    for text in [
        "הגיע לגן \" [ילד] \" שוב",
        "הגיע לגן \"[ילד]\" שוב",
        "הגיע לגן \" [אם] \" שוב",
        "הגיע לגן \" ו[ילד] שוב",
    ] {
        let out = filter(text, &c).unwrap();
        assert_eq!(out.tagged, text);
    }
    // A tag inside the quotes is never part of an institution's name, wherever it sits.
    for text in [
        "הגיע לגן \" מתקשה [ילד] \" שוב",
        "הגיע לגן \"שוב [ילד]\" שוב",
    ] {
        let once = filter(text, &c).unwrap().tagged;
        assert!(once.contains("[ילד]"), "{once}");
        assert_eq!(filter(&once, &c).unwrap().tagged, once);
    }
}

/// Words that send the filter into its context rules: person words, naming phrases, labels,
/// titles, institution and job cues, dates, and the punctuation those rules stop at.
const CONTEXT_WORDS: &[&str] = &[
    "הגננת",
    "סבתא",
    "שלה",
    "בשם",
    "קוראים",
    "לה",
    "משפחת",
    "ד\"ר",
    "גב'",
    "גן",
    "בגן",
    "בית",
    "ספר",
    "מכון",
    "קופ\"ח",
    "מכבי",
    "ע\"ש",
    "עובדת",
    "כמורה",
    "מספרת",
    "שם הילד:",
    "נולד",
    "בחרקוב",
    "עם",
    "ועם",
    "של",
    "יום",
    "ההולדת",
    "באב",
    "ה'",
    "מרץ",
    "March",
    "Noam",
    "-",
    "–",
    "־",
    "(",
    ")",
    "\"",
    "״",
    "'",
    "׳",
    ":",
    ",",
    ".",
    "\n",
    "|",
    "\t",
    "[",
    "]",
    "[ילד]",
    "3",
    "12.3.2020",
    "054",
];

/// Characters that sit on or between letters without showing as one: combining marks,
/// niqqud and cantillation, direction marks, embeddings and isolates, joiners, the BOM, the
/// soft hyphen, emoji (with a skin tone, a ZWJ family, a flag), presentation forms and
/// look-alike letters.
const ODD: &[&str] = &[
    "\u{0301}",
    "\u{0308}",
    "\u{0327}",
    "\u{036F}",
    "\u{05B0}",
    "\u{05B4}",
    "\u{05B9}",
    "\u{05BC}",
    "\u{05BF}",
    "\u{05C1}",
    "\u{05C2}",
    "\u{05C7}",
    "\u{0591}",
    "\u{05A0}",
    "\u{200E}",
    "\u{200F}",
    "\u{202A}",
    "\u{202B}",
    "\u{202E}",
    "\u{2066}",
    "\u{2067}",
    "\u{2069}",
    "\u{200B}",
    "\u{200C}",
    "\u{200D}",
    "\u{2060}",
    "\u{FEFF}",
    "\u{00AD}",
    "\u{034F}",
    "😀",
    "👍🏽",
    "👩‍👧",
    "🇮🇱",
    "\u{FB2A}",
    "\u{FB4B}",
    "\u{FB1D}",
    "Ｎ",
    "о",
    "\u{0640}",
    "ـ",
    "ج",
];

/// One piece of text: a declared name (with a prefix, maybe with an odd character inside), a
/// context word, an odd character, or any character at all; glued to the previous piece or
/// after a space.
fn messy_text(pieces: &[(u8, usize, usize, char, bool)]) -> String {
    let mut out = String::new();
    for &(kind, a, b, any, glued) in pieces {
        if !glued && !out.is_empty() {
            out.push(' ');
        }
        let (name, _) = CASE_NAMES[a % CASE_NAMES.len()];
        match kind % 5 {
            0 => {
                out.push_str(PREFIXES[b % PREFIXES.len()]);
                out.push_str(name);
            }
            1 => {
                // An odd character after the name's `b`-th letter.
                let cut = b % name.chars().count();
                for (k, c) in name.chars().enumerate() {
                    out.push(c);
                    if k == cut {
                        out.push_str(ODD[a % ODD.len()]);
                    }
                }
            }
            2 => out.push_str(CONTEXT_WORDS[b % CONTEXT_WORDS.len()]),
            3 => out.push_str(ODD[b % ODD.len()]),
            _ => out.push(any),
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Declared names among combining marks, direction marks, emoji, niqqud, context words
    /// and arbitrary characters never make a filtering entry point or the gate panic (a
    /// release build aborts on a panic), and the filter never fails on them; split ranges
    /// and name-model hints at any byte offset, char boundary or not, included.
    #[test]
    fn names_among_marks_emoji_and_niqqud_never_panic(
        pieces in prop::collection::vec(
            (any::<u8>(), 0usize..64, 0usize..64, any::<char>(), prop::bool::weighted(0.3)),
            1..28,
        ),
        cuts in prop::collection::vec((0usize..512, 0usize..512), 0..4),
        hint_at in prop::collection::vec((0usize..512, 0usize..512, 0.0f32..1.0), 0..4),
    ) {
        let text = messy_text(&pieces);
        let ids = identities();
        let c = ctx(&ids, CASE);
        let at = |n: usize| n % (text.len() + 1);
        let ranges: Vec<std::ops::Range<usize>> = cuts
            .iter()
            .map(|&(a, b)| at(a).min(at(b))..at(a).max(at(b)))
            .collect();
        let hints: Vec<NameHint> = hint_at
            .iter()
            .map(|&(a, b, score)| NameHint { start: at(a).min(at(b)), end: at(a).max(at(b)), score })
            .collect();

        let out = filter(&text, &c);
        prop_assert!(out.is_ok(), "filter failed on {text:?}");
        let out = out.unwrap();
        assert_segments_consistent(&text, &out);
        prop_assert!(filter_with_hints(&text, &c, &hints).is_ok(), "{text:?}");
        prop_assert!(filter_split(&text, &c, &ranges).is_ok(), "{text:?}");
        prop_assert!(filter_split_with_hints(&text, &c, &ranges, &hints).is_ok(), "{text:?}");
        let again = filter(&out.tagged, &c);
        prop_assert!(again.is_ok(), "filter failed on its own output {:?}", out.tagged);
        let _ = gate_text(&text, &ids, CASE);
        let _ = gate_text(&out.tagged, &ids, CASE);
        let tags: Vec<String> = case_tags(&ids, CASE).into_iter().collect();
        let _ = scan_model_output(&text, &tags, &ids);
        let _ = restore(&out.tagged, &ids, Some("x"), &[]);
    }
}
