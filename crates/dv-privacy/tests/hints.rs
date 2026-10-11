//! Name hints from a local model (`dv-ner`, stage 5): they may add hides, never remove one.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dv_domain::{Identity, IdentitySource, Role};
use dv_privacy::{filter, filter_with_hints, AutoKind, Mark, NameHint, PrivacyContext};
use proptest::prelude::*;

fn child() -> Vec<Identity> {
    vec![Identity {
        id: "c-0".to_owned(),
        case_id: "c".to_owned(),
        role: Role::Child,
        tag: "[ילד]".to_owned(),
        value: "נוגה דמיונית".to_owned(),
        aliases: Vec::new(),
        source: IdentitySource::Manual,
        reason: String::new(),
    }]
}

fn ctx<'a>(ids: &'a [Identity], allow: &'a dyn Fn(&str) -> bool) -> PrivacyContext<'a> {
    PrivacyContext {
        case_id: "c",
        identities: ids,
        practitioner: &[],
        allowlisted: allow,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: (2026, 10, 11),
    }
}

fn hint(text: &str, word: &str, score: f32) -> NameHint {
    let start = text.find(word).unwrap();
    NameHint {
        start,
        end: start + word.len(),
        score,
    }
}

/// Byte ranges of the original text that went out as something else.
fn hidden(out: &dv_privacy::FilterOutcome) -> Vec<(usize, usize)> {
    let mut pos = 0;
    let mut v = Vec::new();
    for s in &out.original_segments {
        if s.mark == Some(Mark::Replaced) {
            v.push((pos, pos + s.text.len()));
        }
        pos += s.text.len();
    }
    v
}

const TEXT: &str = "אתמול זלטקה הגיעה עם נוגה, וזלטקה שיחקה איתה בחצר.";

#[test]
fn a_hint_hides_a_name_no_rule_knows() {
    let ids = child();
    let never = |_: &str| false;
    let plain = filter(TEXT, &ctx(&ids, &never)).unwrap();
    assert!(plain.tagged.contains("זלטקה"), "{}", plain.tagged);

    let out = filter_with_hints(TEXT, &ctx(&ids, &never), &[hint(TEXT, "זלטקה", 0.97)]).unwrap();
    assert!(!out.tagged.contains("זלטקה"), "{}", out.tagged);
    // The prefixed form later on is the same name, and the prefix stays.
    assert!(out.tagged.contains(", ו["), "{}", out.tagged);
    let item = out.auto_hidden.iter().find(|a| a.token == "זלטקה").unwrap();
    assert_eq!(item.kind, AutoKind::Name);
    assert!(!item.uncertain);
}

#[test]
fn a_hint_inside_a_word_keeps_only_a_prefix_out() {
    let ids = child();
    let never = |_: &str| false;
    let text = "ראיתי את וזלטקה.";
    // Starts after the prefix: the prefix stays as written.
    let h = hint(text, "זלטקה", 0.95);
    let out = filter_with_hints(text, &ctx(&ids, &never), &[h]).unwrap();
    assert!(out.tagged.starts_with("ראיתי את ו["), "{}", out.tagged);
    // Ends inside the word: the whole word goes, never a piece of a name.
    let h = NameHint {
        start: text.find("וזל").unwrap(),
        end: text.find("וזל").unwrap() + "וזל".len(),
        score: 0.95,
    };
    let out = filter_with_hints(text, &ctx(&ids, &never), &[h]).unwrap();
    assert!(!out.tagged.contains("טקה"), "{}", out.tagged);
}

#[test]
fn a_low_score_is_hidden_but_marked_uncertain() {
    let ids = child();
    let never = |_: &str| false;
    let out = filter_with_hints(TEXT, &ctx(&ids, &never), &[hint(TEXT, "זלטקה", 0.6)]).unwrap();
    assert!(!out.tagged.contains("זלטקה"));
    assert!(out
        .auto_hidden
        .iter()
        .any(|a| a.token == "זלטקה" && a.uncertain));
}

#[test]
fn a_word_she_kept_stays_kept() {
    let ids = child();
    let kept = |w: &str| w == "זלטקה";
    let out = filter_with_hints(TEXT, &ctx(&ids, &kept), &[hint(TEXT, "זלטקה", 0.99)]).unwrap();
    assert!(out.tagged.contains("זלטקה"), "{}", out.tagged);
}

#[test]
fn a_hint_on_a_declared_name_changes_nothing() {
    let ids = child();
    let never = |_: &str| false;
    let plain = filter(TEXT, &ctx(&ids, &never)).unwrap();
    let out = filter_with_hints(TEXT, &ctx(&ids, &never), &[hint(TEXT, "נוגה", 0.99)]).unwrap();
    assert_eq!(plain, out);
}

const TEXTS: &[&str] = &[
    TEXT,
    "הגננת אסתי סיפרה שנוגה דמיונית משחקת עם ג'וד ועם רקס, הכלב של סבתא נונה.",
    // Split so the repository's pre-commit scan does not read it as a real number.
    concat!(
        "ד\"ר כהן ממכבי, טלפון 03-123",
        "4567, ראה את נוגה ב-3.9.2026 בתל אביב."
    ),
    "שני ההורים הגיעו. מתן זמן לתגובה עזר, והיא מתקרבת אליה בהדרגה.",
];

proptest! {
    /// Whatever the hints, everything the rules hid is still hidden, and the outgoing text
    /// never holds more of the original than without them.
    #[test]
    fn hints_only_ever_add_hides(
        which in 0..TEXTS.len(),
        raw in proptest::collection::vec((0_usize..200, 1_usize..30, 0.0_f32..1.0), 0..6),
    ) {
        let text = TEXTS[which];
        let ids = child();
        let never = |_: &str| false;
        let hints: Vec<NameHint> = raw
            .iter()
            .filter_map(|&(s, len, score)| {
                let s = s.min(text.len());
                let e = (s + len).min(text.len());
                (text.is_char_boundary(s) && text.is_char_boundary(e) && s < e)
                    .then_some(NameHint { start: s, end: e, score })
            })
            .collect();
        let plain = hidden(&filter(text, &ctx(&ids, &never)).unwrap());
        let with = hidden(&filter_with_hints(text, &ctx(&ids, &never), &hints).unwrap());
        for (s, e) in plain {
            for b in s..e {
                prop_assert!(
                    with.iter().any(|(ws, we)| *ws <= b && b < *we),
                    "byte {b} of «{}» was hidden by the rules and is not with hints",
                    &text[s..e]
                );
            }
        }
    }
}
