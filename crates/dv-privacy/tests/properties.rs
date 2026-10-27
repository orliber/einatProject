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
use dv_privacy::restore::{
    her_text_to_send, remaining_tags, restore, restore_hers, scan_model_output,
};
use dv_privacy::text::{normalize, prefix_splits, tokenize};
use dv_privacy::{
    clear, filter, filter_split, filter_split_with_hints, filter_to_keep, filter_with_hints,
    AutoKind, GateRequest, Mark, MaskBook, NameHint, PrivacyContext,
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
    // Quotes, gershayim, geresh, maqaf, dashes and other marks a report has around words.
    "״",
    "׳",
    "'",
    "-",
    "־",
    "–",
    ";",
    "…",
    "\"מתקשה\"",
    "״שוב״",
    "(בשקט)",
    "רב-תחומי",
    "דו־לשוני",
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

/// The filter's outcome generalized something (left a word out, or a date became relative
/// time): what goes out is meant to differ from what she wrote there, by design.
fn generalized(out: &dv_privacy::FilterOutcome) -> bool {
    out.tagged_segments
        .iter()
        .any(|s| s.mark == Some(Mark::Relative))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Declared names with any prefix in any surrounding text: every name becomes its tag
    /// with the prefix kept outside it, and restoring the tags gives back exactly the
    /// original text, byte for byte, unless the outgoing filter generalized something (a
    /// quoted word after "לגן" may be the kindergarten's name, and is left out); kept as her
    /// own words (D-126), the text always comes back whole.
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
        if !generalized(&out) {
            prop_assert_eq!(restore(&out.tagged, &ids, None, &[]), text.clone());
        }
        prop_assert!(gate_text(&out.tagged, &ids, CASE).is_ok(), "dead end on {}", out.tagged);
        let book = MaskBook::default();
        let c = PrivacyContext { masks: Some(&book), ..ctx(&ids, CASE) };
        let kept = filter_to_keep(&text, &c, &[]).unwrap();
        prop_assert_eq!(restore_hers(&kept.tagged, &ids, None, &book.entries()), text);
        // Sent again, the kept text goes out as the text itself would.
        let sent = her_text_to_send(&kept.tagged, &ids, None, &book.entries());
        prop_assert_eq!(filter(&sent, &c).unwrap().tagged, filter(&sentence(&p), &c).unwrap().tagged);
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
        if generalized(&a) {
            // A name left out brings the next word up to the institution word (`לגן ״שוב״
            // "מתקשה"` goes out `לגן "מתקשה"`), which the second time reads as the name: more
            // is hidden then, never less.
            prop_assert!(gate_text(&again.tagged, &ids, CASE).is_ok(), "{}", again.tagged);
        } else {
            prop_assert_eq!(&again.tagged, &a.tagged);
            prop_assert!(again.auto_hidden.is_empty(), "{:?}", again.auto_hidden);
        }
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
    // After the closing quote a tag leaves the quoted name as on the first filtering, and
    // "המוסד" (what a recurring name became) is not a name.
    for (text, out) in [
        ("הגיע לגן \"ניצנים\" [ילד] שוב", "הגיע לגן [ילד] שוב"),
        ("הגיע לגן \"המוסד\" שוב", "הגיע לגן \"המוסד\" שוב"),
    ] {
        assert_eq!(filter(text, &c).unwrap().tagged, out);
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

/// Found by `her_kept_words_come_back_exactly` at 20,000 cases: the mask book gives one tag to
/// the same number however it is written (a point inside it is dropped when it is read), so
/// her second copy, with the point, came back as the first. A numbered tag is kept in her
/// paragraph only for the very words it stands for; anything else keeps her words.
#[test]
fn the_same_number_written_twice_ways_comes_back_as_she_wrote_each() {
    let ids = identities();
    let text = "מספר 000000018 ושוב 0000\u{5B0}00018 בתיק";
    let book = MaskBook::default();
    let c = PrivacyContext {
        masks: Some(&book),
        ..ctx(&ids, CASE)
    };
    let kept = filter_to_keep(text, &c, &[]).unwrap();
    assert!(!kept.tagged.contains("00018"), "{}", kept.tagged);
    assert_eq!(
        restore_hers(&kept.tagged, &ids, None, &book.entries()),
        text
    );
}

/// Found by `filter_then_restore_is_the_identity` on main (dc51787): `לגן " מתקשה "` came back
/// as `לגן "`. Not the restore step: after an institution word a quoted word may be the
/// kindergarten's name ("לגן \"ניצנים\""), and the outgoing filter leaves it out, as decided
/// (round 30: nothing to put back). The closing quote after a space stayed behind, and her
/// own paragraphs were stored as that outgoing text, so the report lost the name too.
#[test]
fn a_quoted_word_after_an_institution_word_is_left_out_but_kept_for_her() {
    let ids = identities();
    let text = sentence(&[(7, 0, false), (27, 0, false), (0, 0, false), (27, 0, false)]);
    assert_eq!(text, "לגן \" מתקשה \"");
    // Out: left out with both quotes, nothing dangling.
    for (text, out) in [
        (text.as_str(), "לגן"),
        ("לגן \" מתקשה \" שוב", "לגן שוב"),
        ("לגן \"מתקשה\" שוב", "לגן שוב"),
        ("בגן ״ניצנים״ שוב", "בגן שוב"),
    ] {
        assert_eq!(
            filter(text, &ctx(&ids, CASE)).unwrap().tagged,
            out,
            "{text}"
        );
    }
    // Kept: exactly as she wrote it, and sent again it goes out the same way.
    let book = MaskBook::default();
    let c = PrivacyContext {
        masks: Some(&book),
        ..ctx(&ids, CASE)
    };
    let kept = filter_to_keep(&text, &c, &[]).unwrap();
    assert_eq!(kept.tagged, "לגן[מילים_1]");
    assert_eq!(
        restore_hers(&kept.tagged, &ids, None, &book.entries()),
        text
    );
    // Shown as anyone else's text, the tag stays a tag (second review of D-126).
    assert_eq!(
        restore(&kept.tagged, &ids, None, &book.entries()),
        kept.tagged
    );
    let sent = her_text_to_send(&kept.tagged, &ids, None, &book.entries());
    assert_eq!(filter(&sent, &c).unwrap().tagged, "לגן");
    // Anywhere else (an answer, a material) the tag is plain words, and the gate refuses it.
    assert_eq!(filter(&kept.tagged, &c).unwrap().tagged, "לגן(מילים_1)");
    assert!(gate_text(&kept.tagged, &ids, CASE).is_err());
    // Without the case's book there is nowhere to keep the words: refused, not lost.
    assert!(filter_to_keep(&text, &ctx(&ids, CASE), &[]).is_err());
}

/// `name` as a word of `text`, with or without a prefix (two pieces glued into one word,
/// "גן" and "ועם", are not "נועם").
fn has_word(text: &str, name: &str) -> bool {
    let name = normalize(name);
    tokenize(text)
        .iter()
        .any(|t| prefix_splits(&t.norm).iter().any(|(_, rest)| *rest == name))
}

/// Pieces of her own writing: declared names with a prefix, ordinary words and marks, the
/// context words that send the filter into its rules (institutions, dates, titles, quotes,
/// gershayim, maqaf), identifiers, and odd characters; after a space or glued.
fn her_text(pieces: &[(u8, usize, usize, bool)], idn: &str, phone: &str) -> String {
    let mut out = String::new();
    for &(kind, a, b, glued) in pieces {
        let name = kind % 6 == 0;
        // A name stands as a word of its own: glued to a mark it is still hidden, and
        // restored as it is declared, not as it was spelled.
        if !out.is_empty() && (name || !glued || out.ends_with(' ')) {
            out.push(' ');
        }
        match kind % 6 {
            0 => {
                out.push_str(PREFIXES[b % PREFIXES.len()]);
                out.push_str(CASE_NAMES[a % CASE_NAMES.len()].0);
                out.push(' ');
            }
            1 => out.push_str(FILLER[b % FILLER.len()]),
            2 | 3 => {
                let w = CONTEXT_WORDS[b % CONTEXT_WORDS.len()];
                // Square brackets around words that are not a tag become round ones, by
                // design (a forged tag never passes): not part of this property.
                if !w.contains(['[', ']']) {
                    out.push_str(w);
                }
            }
            4 => out.push_str(if a % 2 == 0 { idn } else { phone }),
            _ => out.push_str(ODD[b % ODD.len()]),
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// What she writes and keeps (an edited or her own paragraph, D-126) comes back exactly
    /// as she wrote it, whatever is around the words; no declared name and no identifier is
    /// in the stored text; and sent again, the stored text goes out exactly as the text
    /// itself would, with no kept-words tag in it.
    #[test]
    fn her_kept_words_come_back_exactly(
        pieces in prop::collection::vec((any::<u8>(), 0usize..64, 0usize..64, prop::bool::weighted(0.3)), 1..24),
        idn in 10_000_000u32..99_999_999,
    ) {
        let ids = identities();
        let idn = valid_id(&idn.to_string());
        let phone = mobile(4, "1234567");
        let text = her_text(&pieces, &idn, &phone);
        let book = MaskBook::default();
        let c = PrivacyContext { masks: Some(&book), ..ctx(&ids, CASE) };
        let kept = filter_to_keep(&text, &c, &[]).unwrap();
        prop_assert_eq!(restore_hers(&kept.tagged, &ids, None, &book.entries()), text.clone(), "stored {:?}", kept.tagged);
        for (name, _) in CASE_NAMES {
            prop_assert!(!has_word(&kept.tagged, name), "{name} stored in {:?}", kept.tagged);
        }
        prop_assert!(!digits(&kept.tagged).contains(&digits(&idn)), "{:?}", kept.tagged);
        prop_assert!(!digits(&kept.tagged).contains(&digits(&phone)), "{:?}", kept.tagged);
        let case_ids: Vec<Identity> = ids.iter().filter(|i| i.case_id == CASE).cloned().collect();
        let sent = her_text_to_send(&kept.tagged, &case_ids, None, &book.entries());
        let again = filter(&sent, &c).unwrap();
        prop_assert!(!again.tagged.contains("[מילים_"), "{:?}", again.tagged);
        for (name, _) in CASE_NAMES {
            prop_assert!(!has_word(&again.tagged, name), "{name} sent in {:?}", again.tagged);
        }
        prop_assert!(!digits(&again.tagged).contains(&digits(&idn)), "{:?}", again.tagged);
        // With words kept, it goes out exactly as the text itself would: restored whole
        // first. (With only tags in it, it is read as any tagged draft sent again, where a tag
        // can count differently from the name it stands for; the checks above still hold.)
        if kept.tagged.contains("[מילים_") {
            prop_assert_eq!(&again.tagged, &filter(&text, &c).unwrap().tagged);
        }
        // Storing never makes a dead end: what the gate clears sent directly, it clears
        // stored and sent again. (The other way a stored text can hide more: "March" before a
        // tag reads as a date. A name the filter found has a tag the case keeps only in
        // dv-core.)
        if again.auto_hidden.is_empty() {
            let direct = filter(&text, &c).unwrap().tagged;
            if gate_text(&direct, &ids, CASE).is_ok() {
                prop_assert!(
                    gate_text(&again.tagged, &ids, CASE).is_ok(),
                    "{:?} / {:?}", again.tagged, direct
                );
            }
        }
        // The stored text read as any other text (an answer, a material): a kept-words tag
        // is plain words, never her words put back, and the gate refuses the tag itself.
        let other = filter(&kept.tagged, &c).unwrap();
        prop_assert!(!other.tagged.contains("[מילים_"), "{:?}", other.tagged);
        for e in book.entries().iter().filter(|e| e.kind == dv_privacy::KEPT_WORDS) {
            prop_assert!(gate_text(&e.tag, &ids, CASE).is_err());
        }
    }
}

/// A stored draft sent again: a tag after "משפחת" was read as the family's name and cut
/// (`משפחת [ילד]` went out as `משפחת [[משפחה_1]]`). Found by `her_kept_words_come_back_exactly`.
/// A "[חסר: …]" note holds words of its own and is still read.
#[test]
fn a_tag_after_a_family_word_stays_whole() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    for text in [
        "משפחת [ילד] הגיעה",
        "למשפחת [אם] יש",
        "משפחת [תאריך_2]",
        "משפחת ו[ילד] הגיעה",
    ] {
        assert_eq!(filter(text, &c).unwrap().tagged, text);
    }
    for text in ["[חסר: משפחת ברקוביץ] הגיעה", "משפחת ברקוביץ[ת.ז.] הגיעה"]
    {
        let out = filter(text, &c).unwrap().tagged;
        assert!(!out.contains("ברקוביץ"), "{out}");
    }
}

/// A spaced dash after two declared names is a pause, not a double surname: "מתקשה" in
/// `נועם אביטל - מתקשה` was hidden as one. Found by `filter_then_restore_is_the_identity`
/// with the dashes added to its words.
#[test]
fn a_spaced_dash_after_declared_names_is_a_pause() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    let out = filter("נועם אביטל - מתקשה במעברים", &c).unwrap();
    assert_eq!(out.tagged, "[ילד] [גננת] - מתקשה במעברים");
    assert!(out.auto_hidden.is_empty(), "{:?}", out.auto_hidden);
    // A name with a tight hyphen is still hidden, and so is a found surname's second part.
    let out = filter("דנה כהן-לוי ודנה כהן - טגסט", &c).unwrap();
    for word in ["כהן", "לוי", "טגסט"] {
        assert!(!out.tagged.contains(word), "{}", out.tagged);
    }
}

/// The number inside a numbered tag was read as a day before a month: a stored answer with
/// `[מספר_זהות_1] מרץ` went out as `[מספר_זהות_לפני כ-7 חודשים`, and the gate refused it as a
/// date. Found by `her_kept_words_come_back_exactly`. A date next to a tag is still a date, and
/// a "[חסר: …]" note is still read.
#[test]
fn the_number_inside_a_tag_is_never_part_of_a_date() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    for text in ["[מספר_זהות_1] מרץ [ילד]", "[תאריך_2] במאי", "[ילד] 3 במרץ"]
    {
        let out = filter(text, &c).unwrap().tagged;
        assert!(gate_text(&out, &ids, CASE).is_ok(), "{out}");
        assert!(out.starts_with('['), "{out}");
    }
    assert_eq!(
        filter("[מספר_זהות_1] מרץ [ילד]", &c).unwrap().tagged,
        "[מספר_זהות_1] מרץ [ילד]"
    );
    assert!(!filter("[ילד] 3 במרץ", &c).unwrap().tagged.contains('3'));
    assert!(gate_text("[ילד] 12.3.2020", &ids, CASE).is_err());
    assert!(gate_text("[חסר: 12.3.2020]", &ids, CASE).is_err());
}
