//! Words, sentences and paragraphs, and the small word lists the models use.

use std::collections::HashSet;
use std::sync::LazyLock;

use dv_privacy::text::normalize;

/// A word as written (punctuation trimmed) and its normalized form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub raw: String,
    pub norm: String,
}

impl Word {
    /// A tag (`[ילד]`) or a number: never part of anything learned as text.
    #[must_use]
    pub fn is_masked(&self) -> bool {
        self.raw.contains('[')
            || self.raw.contains(']')
            || self.raw.chars().any(|c| c.is_ascii_digit())
    }
}

/// A bracketed tag runs over this many whitespace-separated pieces at most
/// (`[תוצאה 7 · ממוצע נמוך · חולשה יחסית]` is seven).
const MAX_TAG_PIECES: usize = 12;

/// The whitespace-separated pieces of `text`, a bracketed tag (`[תוצאה 7 · ממוצע נמוך]`) kept
/// as one piece: its words are the software's, not hers, and must neither lengthen a sentence
/// nor turn into a phrase of hers (C6). A `[` never closed leaves the pieces as they are.
fn pieces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (piece, after) = rest.split_at(end);
        // An opening bracket not closed in this piece: the tag goes on to the piece that
        // closes it, when one does within a few pieces.
        let open = piece.rfind('[');
        let unclosed = open.is_some_and(|o| !piece.get(o..).unwrap_or_default().contains(']'));
        if unclosed {
            let mut closed_at = None;
            let mut seen = 0;
            for (i, c) in after.char_indices() {
                if c == ']' {
                    closed_at = Some(i);
                    break;
                }
                if c == '[' || c == '\n' {
                    break;
                }
                if c.is_whitespace() {
                    seen += 1;
                    if seen > MAX_TAG_PIECES {
                        break;
                    }
                }
            }
            if let Some(i) = closed_at {
                // The piece that holds the `]`, to its end.
                let tail = after.get(i..).unwrap_or_default();
                let tail_end = tail.find(char::is_whitespace).unwrap_or(tail.len());
                let whole = end + i + tail_end;
                out.push(rest.get(..whole).unwrap_or(piece));
                rest = rest.get(whole..).unwrap_or_default().trim_start();
                continue;
            }
        }
        out.push(piece);
        rest = after.trim_start();
    }
    out
}

/// The words of `text`, in order. A word keeps inner geresh and gershayim (ד"ר, ג'ירפה). A
/// bracketed tag is one word, and a masked one.
#[must_use]
pub fn words(text: &str) -> Vec<Word> {
    pieces(text)
        .into_iter()
        .filter_map(|w| {
            let raw = w
                .trim_matches(|c: char| !(c.is_alphanumeric() || c == '[' || c == ']' || c == '_'));
            if raw.is_empty() {
                return None;
            }
            Some(Word {
                raw: raw.to_owned(),
                norm: normalize(raw),
            })
        })
        .collect()
}

/// Sentences of `text`: split at `. ! ? ;` and line breaks; empty ones dropped. A mark inside a
/// bracketed tag (`[תוצאה 7.5]`) does not end a sentence.
#[must_use]
pub fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '\n' => {
                depth = 0;
                out.push(text.get(start..i).unwrap_or_default());
                start = i + c.len_utf8();
            }
            '.' | '!' | '?' | ';' if depth == 0 => {
                out.push(text.get(start..i).unwrap_or_default());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(text.get(start..).unwrap_or_default());
    out.into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Paragraphs worth measuring: lines of at least 12 words (headings and list stubs say little
/// about how she writes).
#[must_use]
pub fn paragraphs(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(str::trim)
        .filter(|p| p.split_whitespace().count() >= 12)
        .collect()
}

fn set(list: &[&str]) -> HashSet<String> {
    list.iter().map(|w| normalize(w)).collect()
}

/// Short words that carry style rather than content (Burrows' Delta uses their rates).
pub const FUNCTION_WORDS: &[&str] = &[
    "של",
    "את",
    "על",
    "עם",
    "אל",
    "כי",
    "גם",
    "אך",
    "אולם",
    "אבל",
    "לא",
    "הוא",
    "היא",
    "הם",
    "זה",
    "זו",
    "זאת",
    "כאשר",
    "אשר",
    "כמו",
    "יותר",
    "מאוד",
    "בין",
    "לפי",
    "כן",
    "אף",
    "רק",
    "עוד",
    "כל",
    "או",
    "אם",
    "בנוסף",
    "לכן",
    "כך",
    "בעיקר",
    "אלא",
    "למרות",
    "כדי",
    "ללא",
    "תוך",
];

pub(crate) static FUNCTION: LazyLock<Vec<String>> =
    LazyLock::new(|| FUNCTION_WORDS.iter().map(|w| normalize(w)).collect());

pub(crate) static FUNCTION_SET: LazyLock<HashSet<String>> = LazyLock::new(|| set(FUNCTION_WORDS));

/// Words of contrast ("עם זאת" is counted by its "זאת" after "עם").
pub(crate) static CONTRAST: LazyLock<HashSet<String>> = LazyLock::new(|| {
    set(&[
        "אך",
        "אולם",
        "אבל",
        "לעומת",
        "למרות",
        "אמנם",
        "ואילו",
        "מנגד",
    ])
});

/// Hedging and evaluation.
pub(crate) static HEDGE: LazyLock<HashSet<String>> = LazyLock::new(|| {
    set(&[
        "נראה",
        "נראית",
        "ניכר",
        "ניכרת",
        "ייתכן",
        "יתכן",
        "כנראה",
        "עשוי",
        "עשויה",
        "ככל",
        "מתרשם",
        "מתרשמת",
        "משתמע",
        "מרשים",
    ])
});

/// First person.
pub(crate) static FIRST_PERSON: LazyLock<HashSet<String>> = LazyLock::new(|| {
    set(&[
        "התרשמתי",
        "ראיתי",
        "הערכתי",
        "מצאתי",
        "לדעתי",
        "להערכתי",
        "אני",
        "בדקתי",
        "נפגשתי",
        "המלצתי",
        "שוחחתי",
        "אבחנתי",
        "התרשמותי",
        "ממליצה",
        "אמליץ",
    ])
});

/// Linking words whose use is counted one by one (C3), every one of them from the lists above
/// (contrast, and the function words). Fixed text: the rhythm card names them, never a word of
/// a report. "עם זאת" is two words, counted as in the contrast feature.
pub const CONNECTORS: &[&str] = &[
    "עם זאת",
    "אך",
    "אולם",
    "אבל",
    "אמנם",
    "ואילו",
    "מנגד",
    "למרות",
    "לעומת",
    "אלא",
    "בנוסף",
    "לכן",
];

/// How often each of [`CONNECTORS`] is used in `ws`, in their order.
pub(crate) fn connector_counts(ws: &[&Word]) -> Vec<f64> {
    CONNECTORS
        .iter()
        .map(|c| {
            let parts: Vec<String> = c.split(' ').map(normalize).collect();
            match parts.as_slice() {
                [one] => {
                    let list: HashSet<String> = HashSet::from([one.clone()]);
                    ws.iter().filter(|w| has(&list, &w.norm)).count() as f64
                }
                [first, second] => ws
                    .windows(2)
                    .filter(|p| {
                        matches!(p, [a, b] if (a.norm == *first || a.norm.ends_with(first.as_str()))
                            && b.norm == *second)
                    })
                    .count() as f64,
                _ => 0.0,
            }
        })
        .collect()
}

/// Report words too common to make a phrase hers ("הילד", "ההורים").
pub(crate) static GENERIC: LazyLock<HashSet<String>> = LazyLock::new(|| {
    set(&[
        "הילד",
        "הילדה",
        "ילד",
        "ילדה",
        "ההורים",
        "הורים",
        "האם",
        "האב",
        "הגן",
        "הגננת",
        "המבחן",
        "האבחון",
        "בגן",
        "בבית",
        "הבית",
        "היה",
        "הייתה",
        "היו",
        "יש",
        "אין",
        "ב",
        "ו",
        "ה",
        "ל",
        "מ",
        "ש",
    ])
});

/// `norm` or `norm` without one leading prefix letter is in `list` ("ואולם", "שנראה").
pub(crate) fn has(list: &HashSet<String>, norm: &str) -> bool {
    if list.contains(norm) {
        return true;
    }
    let mut chars = norm.chars();
    match chars.next() {
        Some('ו' | 'ה' | 'ש' | 'ב' | 'ל' | 'כ' | 'מ') => {
            let rest = chars.as_str();
            rest.chars().count() >= 2 && list.contains(rest)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn words_trim_punctuation_and_keep_tags() {
        let w = words("הילד, [ילד] הציג (ד\"ר) 12.");
        let raw: Vec<&str> = w.iter().map(|w| w.raw.as_str()).collect();
        assert_eq!(raw, ["הילד", "[ילד]", "הציג", "ד\"ר", "12"]);
        assert!(w[1].is_masked() && w[4].is_masked() && !w[0].is_masked());
    }

    #[test]
    fn a_multi_word_score_tag_is_one_masked_word() {
        // Before C6 the tag was four words, one of them ("ממוצע") a plain word of hers.
        let w = words("בקוביות ([תוצאה 7 · ממוצע נמוך · חולשה יחסית]), עם זאת [אח_1] עזר.");
        let raw: Vec<&str> = w.iter().map(|w| w.raw.as_str()).collect();
        assert_eq!(
            raw,
            [
                "בקוביות",
                "[תוצאה 7 · ממוצע נמוך · חולשה יחסית]",
                "עם",
                "זאת",
                "[אח_1]",
                "עזר"
            ]
        );
        assert!(w[1].is_masked());
        // A bracket never closed leaves the words as they are.
        let open = words("סוגר [פתוח בלי סוף והמשך");
        assert_eq!(open.len(), 5);
    }

    #[test]
    fn a_mark_inside_a_tag_does_not_end_the_sentence() {
        let s = sentences("הציג [תוצאה 7.5 · ממוצע] בקוביות. ואחר כך המשיך!\nשורה");
        assert_eq!(
            s,
            ["הציג [תוצאה 7.5 · ממוצע] בקוביות", "ואחר כך המשיך", "שורה"]
        );
    }

    #[test]
    fn connectors_are_counted_one_by_one_from_the_fixed_lists() {
        let ws = words("ההורים מתארים שינוי, עם זאת הגננת מתארת קושי ואולם גם בנוסף ולכן");
        let refs: Vec<&Word> = ws.iter().collect();
        let counts = connector_counts(&refs);
        let of = |c: &str| counts[CONNECTORS.iter().position(|x| *x == c).unwrap()];
        assert_eq!(of("עם זאת"), 1.0);
        assert_eq!(of("אולם"), 1.0, "with its prefix letter");
        assert_eq!(of("בנוסף"), 1.0);
        assert_eq!(of("לכן"), 1.0);
        assert_eq!(of("אך"), 0.0);
        // Every linking word is from the lists the model already counted.
        for c in CONNECTORS {
            let known = c
                .split(' ')
                .all(|w| FUNCTION_SET.contains(&normalize(w)) || CONTRAST.contains(&normalize(w)))
                || *c == "עם זאת";
            assert!(known, "{c}");
        }
    }

    #[test]
    fn prefixes_are_seen_through() {
        assert!(has(&CONTRAST, &normalize("ואולם")));
        assert!(has(&HEDGE, &normalize("שנראה")));
        assert!(!has(&HEDGE, &normalize("ראה")));
    }

    #[test]
    fn short_lines_are_not_paragraphs() {
        let t = "כותרת\nזו פסקה ארוכה מספיק כדי שיהיה בה מה למדוד בסגנון הכתיבה של הכותבת.";
        assert_eq!(paragraphs(t).len(), 1);
    }
}
