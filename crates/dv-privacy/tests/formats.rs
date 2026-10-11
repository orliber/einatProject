//! Identifier formats and spellings: the Israeli ID check digit at its edges, phones, e-mails,
//! dates of birth and addresses in their usual variants, Hebrew prefixes, niqqud and
//! direction marks on names, gershayim in titles, OCR misreads and lexicon edge cases.
//! Every value that could look real is generated at run time (see `common`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

mod common;

use common::*;
use dv_domain::Role;
use dv_privacy::patterns::{find, is_valid_israeli_id, PatternKind};
use dv_privacy::text::{edit_distance, normalize};
use dv_privacy::{filter, ocr_misreads, PrivacyContext};
use proptest::prelude::*;

fn kinds(text: &str) -> Vec<PatternKind> {
    find(text, TODAY)
        .unwrap()
        .into_iter()
        .map(|h| h.kind)
        .collect()
}

/// `value` inside `text` leaves nothing of itself once filtered, the raw text is refused,
/// and what the filter let out clears.
fn assert_hidden(text: &str, value: &str) {
    let ids = identities();
    assert!(
        gate_text(text, &ids, CASE).is_err(),
        "raw cleared: «{text}»"
    );
    let out = filter(text, &ctx(&ids, CASE)).unwrap();
    assert!(
        !out.tagged.contains(value),
        "«{value}» kept: {}",
        out.tagged
    );
    let d = digits(value);
    if d.len() >= 5 {
        // No run of four or more of its digits survives.
        let kept = digits(&out.tagged);
        for w in d.as_bytes().windows(5) {
            let w = std::str::from_utf8(w).unwrap();
            assert!(
                !kept.contains(w),
                "digits {w} of {value} kept: {}",
                out.tagged
            );
        }
    }
    assert!(
        gate_text(&out.tagged, &ids, CASE).is_ok(),
        "dead end: {}",
        out.tagged
    );
}

// ---------------------------------------------------------------- Israeli ID numbers

#[test]
fn the_id_check_digit_at_its_boundaries() {
    // Leading zeros are part of the number: the smallest valid one is all zeros but the
    // check digit, and nine zeros pass the sum test too.
    assert!(is_valid_israeli_id("000000018"));
    assert!(is_valid_israeli_id("000000000"));
    assert!(!is_valid_israeli_id("000000019"));
    // Exactly nine ASCII digits: eight, ten, signs, spaces and other scripts are refused.
    for bad in [
        "",
        "00000018",
        "0000000018",
        "-00000018",
        "+00000018",
        "00000001 8",
        "0000000١8",
        "00000001８",
        "00000001a",
    ] {
        assert!(!is_valid_israeli_id(bad), "{bad:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Exactly one check digit fits any eight digits; changing any one digit, or swapping two
    /// different adjacent digits (but 0/9), breaks it: the checksum catches every typing
    /// slip the rule is meant to catch.
    #[test]
    fn the_check_digit_catches_one_wrong_digit_and_most_swaps(first in 0u32..100_000_000, pos in 0usize..9, delta in 1u8..10, swap in 0usize..8) {
        let first8 = format!("{first:08}");
        let fits = (0..10).filter(|d| is_valid_israeli_id(&format!("{first8}{d}"))).count();
        prop_assert_eq!(fits, 1);
        let id = valid_id(&first8);
        let mut wrong = id.clone().into_bytes();
        wrong[pos] = b'0' + (wrong[pos] - b'0' + delta) % 10;
        prop_assert!(!is_valid_israeli_id(std::str::from_utf8(&wrong).unwrap()));
        let mut swapped = id.clone().into_bytes();
        swapped.swap(swap, swap + 1);
        let (a, b) = (id.as_bytes()[swap], id.as_bytes()[swap + 1]);
        if a != b && !matches!((a, b), (b'0', b'9') | (b'9', b'0')) {
            prop_assert!(!is_valid_israeli_id(std::str::from_utf8(&swapped).unwrap()));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// Nine digits are an ID whether or not the check digit fits (fail-closed), typed whole,
    /// with dots, or in groups after a label; an ID written without its leading zero (eight
    /// digits) is still hidden as a number.
    #[test]
    fn id_numbers_are_hidden_in_every_usual_spelling(first in 0u32..100_000_000, check in 0u8..10) {
        let idn = format!("{first:08}{check}");
        let (a, b, c) = (&idn[..3], &idn[3..6], &idn[6..]);
        for spelled in [
            idn.clone(),
            format!("{a}.{b}.{c}"),
            format!("{a}-{b}-{c}"),
            format!("{a} {b} {c}"),
            format!("{a}\u{00A0}{b}\u{00A0}{c}"),
            format!("{}-{}", &idn[..8], &idn[8..]),
        ] {
            let text = format!("ת.ז. של הילד: {spelled}.");
            assert_hidden(&text, &spelled);
        }
        if let Some(short) = idn.strip_prefix('0') {
            assert_hidden(&format!("ת\"ז {short}"), short);
        }
    }
}

#[test]
fn grouped_scores_are_not_taken_for_an_id() {
    // Nine digits in groups of three, with no label and a wrong check digit, are a row of
    // scores; with the check digit or a label they are an ID.
    let mut scores = String::from("112104106");
    while is_valid_israeli_id(&scores) {
        scores = (scores.parse::<u64>().unwrap() + 1).to_string();
    }
    let spaced = format!("{} {} {}", &scores[..3], &scores[3..6], &scores[6..]);
    assert!(!kinds(&format!("ציונים: {spaced}")).contains(&PatternKind::IdNumber));
    assert!(kinds(&format!("ת.ז. {spaced}")).contains(&PatternKind::IdNumber));
    let v = valid_id("11210410");
    let spaced = format!("{} {} {}", &v[..3], &v[3..6], &v[6..]);
    assert!(kinds(&format!("ציונים: {spaced}")).contains(&PatternKind::IdNumber));
}

// ---------------------------------------------------------------- phones

#[test]
fn phones_are_hidden_in_every_usual_spelling() {
    let m = mobile(2, "7654321");
    let (p, rest) = (&m[..3], &m[3..]);
    let intl = &m[1..];
    for spelled in [
        m.clone(),
        format!("{p}-{rest}"),
        format!("{p}-{}-{}", &rest[..3], &rest[3..]),
        format!("{p} {} {}", &rest[..3], &rest[3..]),
        format!("{p}.{rest}"),
        format!("{p}\u{00A0}{rest}"),
        format!("+972{intl}"),
        format!("+972-{}-{rest}", &intl[..2]),
        format!("+972 {} {rest}", &intl[..2]),
        format!("972{intl}"),
        format!("00972{intl}"),
    ] {
        assert_hidden(&format!("טלפון האם: {spelled}"), &spelled);
    }
    // Landlines: area code and seven digits.
    let land = ["03-", "6", "543210"].concat();
    assert_hidden(&format!("בבית {land}"), &land);
}

// ---------------------------------------------------------------- e-mails and links

#[test]
fn emails_are_hidden_in_every_usual_spelling() {
    for e in [
        email("ruth.levi", "example.co.il"),
        email("RUTH.LEVI", "EXAMPLE.CO.IL"),
        email("r_levi+gan", "mail.example.org"),
        email("r-levi99", "sub.example-domain.net"),
        email("a", "b.io"),
    ] {
        for text in [
            format!("מייל: {e}"),
            format!("מייל:{e}."),
            format!("({e})"),
            format!("לכתובת {e}, תודה"),
            format!("<{e}>"),
        ] {
            assert_hidden(&text, &e);
        }
    }
}

#[test]
fn links_and_ip_addresses_are_hidden() {
    for (text, value) in [
        ("ראו https://example.org/kids?id=7", "example.org"),
        ("באתר www.gan-example.co.il", "gan-example"),
        ("בדף gan-example.org.il/about", "gan-example"),
        ("מהכתובת 10.20.30.40", "10.20.30.40"),
    ] {
        let ids = identities();
        assert!(gate_text(text, &ids, CASE).is_err(), "{text}");
        let out = filter(text, &ctx(&ids, CASE)).unwrap();
        assert!(!out.tagged.contains(value), "{}", out.tagged);
    }
}

// ---------------------------------------------------------------- dates of birth

#[test]
fn dates_of_birth_never_go_out_not_even_the_year() {
    for text in [
        "נולד ב-1.1.2020",
        "נולד ב1/1/20",
        "תאריך לידה: 01.01.2020",
        "תאריך לידה: 1.1.20.",
        "נולד ב-01-01-2020",
        "נולד ב2020-01-01",
        "DOB 2020-01-01",
        "נולד ב-1 בינואר 2020",
        "נולד בינואר 2020",
        "born on January 1, 2020",
        "born 1 Jan 2020",
        "נולד בשנת 2020",
        "ילידת 2020",
        "נולד בכ\"ג בטבת תש\"פ",
        "נולד בתש\"פ",
    ] {
        let ids = identities();
        assert!(
            gate_text(text, &ids, CASE).is_err(),
            "raw cleared: «{text}»"
        );
        let out = filter(text, &ctx(&ids, CASE)).unwrap();
        for kept in ["2020", "תש\"פ", "טבת", "20"] {
            assert!(
                !out.tagged.contains(kept),
                "«{text}» kept {kept}: {}",
                out.tagged
            );
        }
        assert!(gate_text(&out.tagged, &ids, CASE).is_ok(), "{}", out.tagged);
    }
}

#[test]
fn ages_and_scores_are_not_dates() {
    for text in [
        "בגיל 2.5 החל לדבר",
        "בן 4.5 שנים",
        "אפגר 9/10",
        "ציון 12/19 במבחן",
        "גיל 5:4",
    ] {
        assert!(
            !kinds(text).contains(&PatternKind::Date),
            "{text}: {:?}",
            kinds(text)
        );
    }
}

// ---------------------------------------------------------------- addresses and places

#[test]
fn addresses_are_hidden_with_and_without_the_word_street() {
    // (text, what must go, the gate refuses it raw too: the "כתובת:" field rule lives in
    // the filter only, the gate checks the address patterns).
    for (text, value, gate_too) in [
        ("גרים ברחוב הרצל 12", "הרצל", true),
        ("גרים ברח' הרצל 12", "הרצל", true),
        ("גרים בשדרות ירושלים 4", "ירושלים 4", true),
        ("עברו לרחוב התשבי", "התשבי", true),
        ("כתובת: מסדה 33, חיפה", "מסדה", false),
        ("גרים בשכונת נווה שאנן", "שאנן", true),
        ("עברו לקיבוץ גבעת ברנר", "ברנר", true),
        ("גרים במשק 41", "41", true),
    ] {
        let ids = identities();
        if gate_too {
            assert!(
                gate_text(text, &ids, CASE).is_err(),
                "raw cleared: «{text}»"
            );
        }
        let out = filter(text, &ctx(&ids, CASE)).unwrap();
        assert!(
            !out.tagged.contains(value),
            "«{text}» kept {value}: {}",
            out.tagged
        );
    }
}

// ---------------------------------------------------------------- spellings of names

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// A declared name with any valid stack of prefixes, niqqud on any letter, direction
    /// marks, a joiner or a BOM inside it, final or non-final letters, is hidden and the raw
    /// text is refused.
    #[test]
    fn names_are_hidden_under_marks_and_prefixes(
        name in prop::sample::select(vec!["נועם", "רותם", "עידו", "אביטל"]),
        prefix in prop::sample::select(vec!["", "ו", "ה", "ב", "ל", "מ", "ש", "כ", "וב", "ול", "של", "שב", "כש", "וכש", "לכש", "מש", "ו־", "ב-"]),
        marks in prop::collection::vec(prop::sample::select(vec![None, None, Some('\u{05B0}'), Some('\u{05B4}'), Some('\u{05B8}'), Some('\u{05BC}'), Some('\u{05B9}'), Some('\u{200F}'), Some('\u{200E}'), Some('\u{200D}'), Some('\u{200C}'), Some('\u{FEFF}')]), 8),
        nonfinal in any::<bool>(),
        slot in 0usize..3,
    ) {
        let mut spelled = String::new();
        let letters: Vec<char> = name.chars().collect();
        for (i, c) in letters.iter().enumerate() {
            let c = match (nonfinal && i + 1 == letters.len(), c) {
                (true, 'ם') => 'מ',
                _ => *c,
            };
            spelled.push(c);
            if let Some(m) = marks[i] {
                // A mark after the last letter is fine; a mark is never the first character.
                spelled.push(m);
            }
        }
        let word = format!("{prefix}{spelled}");
        let text = match slot {
            0 => format!("{word} הגיע לגן."),
            1 => format!("הבוקר שוחחתי עם {word}, והיה נעים."),
            _ => format!("(«{word}»)"),
        };
        let ids = identities();
        prop_assert!(gate_text(&text, &ids, CASE).is_err(), "raw cleared «{text:?}»");
        let out = filter(&text, &ctx(&ids, CASE)).unwrap();
        prop_assert!(!contains_norm(&out.tagged, name), "{name} kept in {:?}", out.tagged);
        prop_assert!(gate_text(&out.tagged, &ids, CASE).is_ok(), "dead end on {:?}", out.tagged);
    }
}

#[test]
fn titles_with_any_gershayim_and_a_surname_are_hidden() {
    let ids = identities();
    for title in ["ד\"ר", "ד״ר", "ד\u{201D}ר", "דר'", "דוקטור"] {
        for text in [
            format!("{title} שטרן המליץ."),
            format!("לפי {title} אבנר שטרן"),
            format!("אצל {title} שטרן."),
        ] {
            let out = filter(&text, &ctx(&ids, CASE)).unwrap();
            assert!(
                !out.tagged.contains("שטרן") && !out.tagged.contains("אבנר"),
                "{text} → {}",
                out.tagged
            );
            assert!(gate_text(&text, &ids, CASE).is_err());
        }
    }
    // An undeclared surname after a title is hidden too (D-034).
    let out = filter("ד״ר קורנבליט בדק אותו.", &ctx(&ids, CASE)).unwrap();
    assert!(!out.tagged.contains("קורנבליט"), "{}", out.tagged);
}

#[test]
fn latin_spellings_of_a_declared_name_are_hidden_in_any_case() {
    let ids = identities();
    for text in [
        "Noam came.",
        "NOAM came.",
        "noam came.",
        "with Noam, his mother",
    ] {
        let out = filter(text, &ctx(&ids, CASE)).unwrap();
        assert!(
            !contains_norm(&out.tagged, "noam"),
            "{text} → {}",
            out.tagged
        );
        assert!(gate_text(text, &ids, CASE).is_err(), "{text}");
    }
}

#[test]
fn a_prefix_letter_is_never_split_off_inside_a_longer_word() {
    // "בשאלון" is not ב + ש + אלון; "מטלה" holds no "טל"; a name is matched only as a word.
    let ids = vec![
        id("c", Role::Child, "[ילד]", "אלון", &[]),
        id("c", Role::Mother, "[אם]", "טל", &[]),
    ];
    let c = ctx(&ids, "c");
    let out = filter("ממלאת מטלה בטלפון ובשאלון.", &c).unwrap();
    assert_eq!(out.tagged, "ממלאת מטלה בטלפון ובשאלון.");
}

// ---------------------------------------------------------------- OCR misreads

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// In scanned text, a word one letter (replaced, added, dropped) from a case name of
    /// three letters or more is reported, if at all, as exactly that word under that name's
    /// tag; once kept as a spelling of the name (as dv-core does), it is hidden and the gate
    /// refuses it. Two letters away is never a misread, and another case's names never are.
    #[test]
    fn ocr_misreads_are_one_edit_from_a_name_of_this_case(
        name in prop::sample::select(vec![("אביטל", "[גננת]"), ("נועם", "[ילד]"), ("רותם", "[אם]")]),
        op in 0u8..3,
        pos in 0usize..6,
        letter in prop::sample::select("אבגדהוזחטיכלמנסעפצקרשת".chars().collect::<Vec<_>>()),
    ) {
        let ids = identities();
        let c = ctx(&ids, CASE);
        let mut chars: Vec<char> = name.0.chars().collect();
        match op {
            0 => { let i = pos % chars.len(); chars[i] = letter; }
            1 => chars.insert(pos % (chars.len() + 1), letter),
            _ => { chars.remove(pos % chars.len()); }
        }
        let misread: String = chars.iter().collect();
        prop_assume!(normalize(&misread) != normalize(name.0));
        let found = ocr_misreads(&format!("בסריקה: {misread} הגיע."), &c);
        prop_assert!(found.len() <= 1, "{:?}", found);
        if let Some(m) = found.first() {
            // That name's tag, or another case name's that is one edit away too ("נותם" is
            // one letter from both "נועם" and "רותם").
            let near = CASE_NAMES
                .iter()
                .find(|(n, t)| *t == m.tag && edit_distance(&normalize(n), &normalize(&misread), 1) == 1);
            prop_assert!(near.is_some(), "{misread} reported under {}", m.tag);
            prop_assert_eq!(&m.written, &misread);
            let mut kept = identities();
            let i = kept.iter_mut().find(|i| i.case_id == CASE && i.tag == m.tag).unwrap();
            i.aliases.push(misread.clone());
            let text = format!("{misread} הגיע לגן.");
            let out = filter(&text, &ctx(&kept, CASE)).unwrap();
            prop_assert!(!out.tagged.contains(&misread), "{}", out.tagged);
            prop_assert!(gate_text(&text, &kept, CASE).is_err());
        }
        let mut far: Vec<char> = name.0.chars().collect();
        far[0] = 'ק';
        far[1] = 'ק';
        let far: String = far.iter().collect();
        prop_assert!(ocr_misreads(&far, &c).iter().all(|m| m.tag != name.1));
    }
}

#[test]
fn typical_ocr_misreads_are_found() {
    // Letters OCR confuses: ו/ן/ז, ר/ד, ה/ח, ם/ס.
    let ids = identities();
    let c = ctx(&ids, CASE);
    for (scan, tag) in [
        ("אביטו", "[גננת]"),
        ("אביטז", "[גננת]"),
        ("עירו", "[אב]"),
        ("נועס", "[ילד]"),
        ("רותס", "[אם]"),
    ] {
        let found = ocr_misreads(&format!("{scan} הגיע."), &c);
        assert_eq!(found.len(), 1, "{scan}: {found:?}");
        assert_eq!(found[0].tag, tag);
    }
}

#[test]
fn ocr_misreads_ignore_other_cases_allowlisted_and_short_words() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    // "ינאי" is the other case's child: a misread of it is not this case's business here.
    assert!(ocr_misreads("ינאו הגיע", &c).is_empty());
    // A misread the psychologist marked "not a name" is left alone.
    let allow = |t: &str| t == normalize("אביטה");
    let c2 = PrivacyContext {
        allowlisted: &allow,
        ..ctx(&ids, CASE)
    };
    assert_eq!(ocr_misreads("אביטה הגיעה", &ctx(&ids, CASE)).len(), 1);
    assert!(ocr_misreads("אביטה הגיעה", &c2).is_empty());
    // A prefix is kept out of what is reported as written.
    let m = ocr_misreads("ולאביטה", &c);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].written, "אביטה");
    // Nothing on empty or digit-only text.
    assert!(ocr_misreads("", &c).is_empty());
    assert!(ocr_misreads("12345 678", &c).is_empty());
}

// ---------------------------------------------------------------- lexicon edge cases

#[test]
fn lexicon_names_are_hidden_but_everyday_words_that_contain_them_stay() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    // Lexicon names after a person word or "עם" go; the raw text is refused.
    for (text, name) in [
        ("שיחק עם יובל בחצר.", "יובל"),
        ("הגננת רינת סיפרה.", "רינת"),
        ("ולסבתא שולה יש גינה.", "שולה"),
    ] {
        let out = filter(text, &c).unwrap();
        assert!(!out.tagged.contains(name), "{text} → {}", out.tagged);
        assert!(gate_text(text, &ids, CASE).is_err(), "{text}");
        assert!(gate_text(&out.tagged, &ids, CASE).is_err() || !out.tagged.contains(name));
    }
    // Everyday words that are a prefix + a lexicon name stay words ("כרים", "מלאה").
    for text in ["הביא כרים לגן.", "הכוס מלאה במים.", "בגיל 3 הלך."] {
        let out = filter(text, &c).unwrap();
        assert_eq!(out.tagged, text);
        assert!(gate_text(text, &ids, CASE).is_ok(), "{text}");
    }
}

// ---------------------------------------------------------------- bank and card numbers

#[test]
fn bank_accounts_and_card_numbers_are_hidden() {
    // Built at run time: an IBAN-shaped account and a card number in groups of four.
    let iban = ["IL62", " 0108 0000 ", "0009 9999 999"].concat();
    let card = ["4580", " 1111 2222 ", "3333"].concat();
    for (text, value) in [
        (format!("חשבון: {iban}"), iban.clone()),
        (
            format!("חשבון:{}", iban.replace(' ', "")),
            iban.replace(' ', ""),
        ),
        (format!("כרטיס {card}"), card.clone()),
        (
            format!("כרטיס {}", card.replace(' ', "-")),
            card.replace(' ', "-"),
        ),
    ] {
        assert_hidden(&text, &value);
    }
}
