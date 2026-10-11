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

/// The words of `text`, in order. A word keeps inner geresh and gershayim (ד"ר, ג'ירפה).
#[must_use]
pub fn words(text: &str) -> Vec<Word> {
    text.split_whitespace()
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

/// Sentences of `text`: split at `. ! ? ;` and line breaks; empty ones dropped.
#[must_use]
pub fn sentences(text: &str) -> Vec<&str> {
    text.split(['.', '!', '?', ';', '\n'])
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
