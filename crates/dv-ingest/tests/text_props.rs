//! Properties of the text clean-up every document goes through, and of the visual-to-logical
//! reordering of PDF lines, on generated text (Hebrew, Latin, digits, punctuation, line ends,
//! and the invisible characters a hostile file could hide a name with).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dv_ingest::rtl::reverse_line;
use dv_ingest::{extract, IngestError};
use proptest::prelude::*;

/// Characters that may not survive: direction controls, zero-width marks, BOM, soft hyphen,
/// other control characters.
const INVISIBLE: &[char] = &[
    '\u{200B}', '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}',
    '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{FEFF}', '\u{00AD}',
    '\u{061C}', '\u{2060}', '\u{0007}', '\u{001B}',
];

fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => prop::sample::select(vec![
            "הילד", "גן", "ציון", "שָׁלוֹם", "ד\"ר", "ג'ירפה", "WISC-V", "102", "5:4", "(טווח)",
            "–", ".", ",",
        ])
        .prop_map(str::to_owned),
        3 => prop::sample::select(vec![" ", "  ", "\t", "\u{00A0}", "\n", "\r\n", "\r", "\n\n\n"])
            .prop_map(str::to_owned),
        2 => prop::sample::select(INVISIBLE.to_vec()).prop_map(String::from),
    ]
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(piece(), 0..60).prop_map(|v| v.concat())
}

/// The visible characters of a text, in order (what a reader sees, without spacing).
fn visible(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !INVISIBLE.contains(c) && !c.is_control())
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Clean-up removes exactly the invisible characters and normalizes spacing: no visible
    /// character is lost, added or moved, nothing invisible remains, line ends are `\n`, and
    /// cleaning the result again changes nothing.
    #[test]
    fn clean_up_keeps_every_visible_character_and_is_idempotent(t in text()) {
        match extract(t.as_bytes(), "a.txt") {
            Err(IngestError::Empty) => prop_assert!(visible(&t).is_empty(), "{t:?}"),
            Err(e) => prop_assert!(false, "{e} for {t:?}"),
            Ok(out) => {
                prop_assert_eq!(visible(&out.body), visible(&t));
                let leftover = out
                    .body
                    .chars()
                    .any(|c| INVISIBLE.contains(&c) || c == '\r' || c == '\u{00A0}');
                prop_assert!(!leftover, "{:?}", out.body);
                prop_assert!(!out.body.contains("\n\n\n"), "{:?}", out.body);
                prop_assert_eq!(out.body.trim(), out.body.as_str());
                prop_assert!(out.body.lines().all(|l| l == l.trim_end()));
                let again = extract(out.body.as_bytes(), "a.txt").unwrap();
                prop_assert_eq!(&again.body, &out.body);
                prop_assert!(again.warnings.is_empty(), "{:?}", again.warnings);
                let warned = out.warnings.iter().any(|w| w.contains("כיווניות"));
                // A byte-order mark at the very start is the encoding's, not hidden text.
                let rest = t.strip_prefix('\u{FEFF}').unwrap_or(&t);
                let hidden = rest.chars().any(|c| INVISIBLE.contains(&c));
                prop_assert_eq!(warned, hidden);
            }
        }
    }

    /// Reordering a line only moves characters: never loses, adds or changes one.
    #[test]
    fn reordering_a_line_is_a_permutation(line in "[א-ת a-zA-Z0-9.,:()\\-\t]{0,60}") {
        let out = reverse_line(&line);
        let mut a: Vec<char> = line.chars().collect();
        let mut b: Vec<char> = out.chars().collect();
        a.sort_unstable();
        b.sort_unstable();
        prop_assert_eq!(a, b);
    }

    /// A line of words, numbers and Latin names separated by single spaces or tabs comes back
    /// to itself after two reorderings.
    #[test]
    fn reordering_twice_gives_the_line_back(
        words in prop::collection::vec(
            prop::sample::select(vec!["הילד", "קיבל", "ציון", "102", "WISC", "V", "ADHD", "5:4", "שלום"]),
            1..12,
        ),
        tabs in prop::collection::vec(prop::bool::ANY, 12),
    ) {
        let mut line = String::new();
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                line.push(if tabs[i] { '\t' } else { ' ' });
            }
            line.push_str(w);
        }
        prop_assert_eq!(reverse_line(&reverse_line(&line)), line);
    }
}
