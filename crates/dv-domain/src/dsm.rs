//! The severity of a DSM-5 autism spectrum diagnosis (round 43), as she sets it: one level for
//! each of the two domains. It is her decision, never the AI's: the drafting request carries
//! it as fixed words written here, and a paragraph that names another level gets a note.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Levels 1–3 for social communication and for restricted, repetitive behaviour; `None` when
/// she has not set one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DsmSeverity {
    pub social: Option<u8>,
    pub restricted: Option<u8>,
}

/// The two domains, as the report names them.
pub const SOCIAL_DOMAIN: &str = "תקשורת חברתית ואינטראקציה חברתית";
pub const RESTRICTED_DOMAIN: &str = "דפוסי התנהגות, תחומי עניין ופעילויות מוגבלים וחוזרים";

/// The DSM-5 wording of a level, in Hebrew.
#[must_use]
pub fn level_words(level: u8) -> Option<&'static str> {
    match level {
        1 => Some("נדרשת תמיכה"),
        2 => Some("נדרשת תמיכה משמעותית"),
        3 => Some("נדרשת תמיכה משמעותית מאוד"),
        _ => None,
    }
}

impl DsmSeverity {
    /// A level outside 1–3 is not kept.
    #[must_use]
    pub fn new(social: Option<u8>, restricted: Option<u8>) -> Self {
        let ok = |l: Option<u8>| l.filter(|n| level_words(*n).is_some());
        Self {
            social: ok(social),
            restricted: ok(restricted),
        }
    }

    #[must_use]
    pub fn is_set(&self) -> bool {
        self.social.is_some() || self.restricted.is_some()
    }

    fn lines(&self) -> Vec<String> {
        [
            (SOCIAL_DOMAIN, self.social),
            (RESTRICTED_DOMAIN, self.restricted),
        ]
        .into_iter()
        .filter_map(|(domain, level)| {
            let n = level?;
            Some(format!("{domain}: דרגה {n}, \"{}\"", level_words(n)?))
        })
        .collect()
    }

    /// The lines that go with the request for the DSM section: her levels, to be written as
    /// they are.
    #[must_use]
    pub fn request_text(&self) -> Option<String> {
        let lines = self.lines();
        if lines.is_empty() {
            return None;
        }
        Some(format!(
            "רמת החומרה שקבעה המאבחנת (DSM-5). לכתוב אותה בדיוק כך, בלי לשנות ובלי להוסיף רמה:\n{}",
            lines.join("\n")
        ))
    }
}

/// The fixed line for the DSM section when she has set no level: the model writes none
/// (review of stage 6, R3; the level is her decision).
pub const UNSET_REQUEST: &str =
    "רמת החומרה (DSM-5) עוד לא נקבעה בידי המאבחנת. לא לכתוב דרגה או רמת חומרה.";

/// The fixed words for the DSM section's request: her levels, or that there are none yet.
#[must_use]
pub fn severity_request(severity: Option<&DsmSeverity>) -> String {
    severity
        .and_then(DsmSeverity::request_text)
        .unwrap_or_else(|| UNSET_REQUEST.to_owned())
}

/// A level written as a number or in words ("2", "שנייה", "שתיים").
fn level_of(word: &str) -> Option<u8> {
    let n = match word {
        "1" | "אחת" | "ראשונה" | "אחד" => 1,
        "2" | "שתיים" | "שנייה" | "שניה" | "שני" => 2,
        "3" | "שלוש" | "שלישית" | "שלישי" => 3,
        _ => return None,
    };
    Some(n)
}

/// The levels a text names ("דרגה 2", "רמה 3", "דרגה שנייה", "Level 1").
fn levels_in(text: &str) -> Vec<u8> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | ':' | ';' | '(' | ')'))
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    for pair in words.windows(2) {
        let [word, next] = pair else { continue };
        let word = word.trim_start_matches(['ב', 'ו', 'ה', 'ש', 'ל']);
        let named =
            matches!(word, "דרגה" | "רמה" | "דרגת" | "רמת") || word.eq_ignore_ascii_case("level");
        if let (true, Some(n)) = (named, level_of(next)) {
            out.push(n);
        }
    }
    out
}

/// Which domain a sentence is about, when it names only one of them.
fn domain_of(sentence: &str) -> Option<bool> {
    let social = ["חברתי", "תקשורת"].iter().any(|w| sentence.contains(w));
    let restricted = ["מוגבל", "חוזר", "תחומי עניין", "סטריאוטיפ"]
        .iter()
        .any(|w| sentence.contains(w));
    match (social, restricted) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// A note when the DSM paragraph names a level she did not set: another level, a level of the
/// other domain in a sentence about one domain, or any level while she has set none yet
/// (review of stage 6, R3).
#[must_use]
pub fn check_dsm_severity(text: &str, severity: Option<&DsmSeverity>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(s) = severity.filter(|s| s.is_set()) else {
        for n in levels_in(text) {
            out.push(format!(
                "בפסקה כתובה דרגה {n}, ועוד לא קבעת רמת חומרה. כדאי לבדוק."
            ));
        }
        out.dedup();
        return out;
    };
    let yours = |set: &[u8]| {
        set.iter()
            .map(|l| format!("דרגה {l}"))
            .collect::<Vec<_>>()
            .join(" ו")
    };
    for sentence in text.split(['.', ';', '\n']) {
        let set: Vec<u8> = match domain_of(sentence) {
            Some(true) => s.social.into_iter().collect(),
            Some(false) => s.restricted.into_iter().collect(),
            None => [s.social, s.restricted].into_iter().flatten().collect(),
        };
        for n in levels_in(sentence) {
            if set.is_empty() {
                out.push(format!(
                    "בפסקה כתובה דרגה {n} בתחום שלא קבעת לו רמת חומרה. כדאי לבדוק."
                ));
            } else if !set.contains(&n) {
                out.push(format!(
                    "בפסקה כתובה דרגה {n}, ואת קבעת {}. כדאי לבדוק.",
                    yours(&set)
                ));
            }
        }
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn her_levels_go_out_in_the_dsm_words_and_nothing_else() {
        let s = DsmSeverity::new(Some(2), Some(1));
        let t = s.request_text().unwrap();
        assert!(t.contains("תקשורת חברתית ואינטראקציה חברתית: דרגה 2, \"נדרשת תמיכה משמעותית\""));
        assert!(t.contains("מוגבלים וחוזרים: דרגה 1, \"נדרשת תמיכה\""));
        assert_eq!(DsmSeverity::new(Some(4), None), DsmSeverity::default());
        assert!(DsmSeverity::default().request_text().is_none());
    }

    #[test]
    fn a_paragraph_with_another_level_gets_a_note() {
        let s = DsmSeverity::new(Some(2), Some(1));
        assert!(check_dsm_severity("בתחום התקשורת החברתית: דרגה 2.", Some(&s)).is_empty());
        let notes = check_dsm_severity(
            "עומד בקריטריונים ברמה 3 (נדרשת תמיכה משמעותית מאוד).",
            Some(&s),
        );
        assert_eq!(
            notes,
            vec!["בפסקה כתובה דרגה 3, ואת קבעת דרגה 2 ודרגה 1. כדאי לבדוק.".to_owned()]
        );
        // Not set: any level written is a note (review of stage 6, R3).
        assert_eq!(
            check_dsm_severity("דרגה 3", None),
            vec!["בפסקה כתובה דרגה 3, ועוד לא קבעת רמת חומרה. כדאי לבדוק.".to_owned()]
        );
        assert!(check_dsm_severity("עומד בקריטריונים לאבחנה.", None).is_empty());
        // In words, and a level of the other domain.
        assert_eq!(
            check_dsm_severity("בתחום התקשורת החברתית: דרגה שלישית.", Some(&s)),
            vec!["בפסקה כתובה דרגה 3, ואת קבעת דרגה 2. כדאי לבדוק.".to_owned()]
        );
        assert_eq!(
            check_dsm_severity(
                "בתחום התקשורת החברתית: דרגה 1. בדפוסים המוגבלים והחוזרים: דרגה 2.",
                Some(&s)
            )
            .len(),
            2,
            "swapped levels"
        );
        assert!(check_dsm_severity(
            "בתחום התקשורת החברתית: דרגה שנייה. בדפוסים המוגבלים והחוזרים: דרגה 1.",
            Some(&s)
        )
        .is_empty());
        assert_eq!(severity_request(None), UNSET_REQUEST);
        // A number that is not a level is not one.
        assert!(check_dsm_severity("בגיל 5 נצפו 3 התנהגויות.", Some(&s)).is_empty());
    }
}
