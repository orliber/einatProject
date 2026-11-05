//! Checks of a paragraph against the case's own details, in code (no AI): a gentle note,
//! never a block. She decides.

use crate::{Age, GrammaticalGender};

/// An age written in the text that is not the child's age in the case details.
///
/// Reads the clinical notation (`5:4`, `5;4`) right after a word for an age ("בן 5:4", "בגיל
/// 5;4") and "בן/בת 5 שנים ו-4 חודשים". A clock time ("בשעה 8:10", "הגיע ב-8:10", "9:05") is
/// not an age, nor is anything over 18 years or 11 months. Up to a month apart is the same
/// age (the details and the paragraph may be a few weeks apart).
#[must_use]
pub fn check_age(text: &str, age: Option<Age>) -> Vec<String> {
    let Some(age) = age else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (years, months) in ages_in(text) {
        let written = i32::from(years) * 12 + i32::from(months);
        let entered = i32::from(age.years) * 12 + i32::from(age.months);
        if (written - entered).abs() > 1 {
            let line = format!("כתוב גיל {years}:{months}, ובפרטי התיק {}", age.display());
            if !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out
}

/// The words an age in clinical notation follows ("בן 5:4", "בגיל 5;4", "גיל כרונולוגי 6:2").
/// "8:10" on its own is as often a clock time, so without one of them it is not read as an age.
const AGE_WORDS: &[&str] = &[
    "בן",
    "בת",
    "כבן",
    "כבת",
    "גיל",
    "בגיל",
    "לגיל",
    "מגיל",
    "כגיל",
    "כרונולוגי",
];

/// The clitic prefixes a word for an age may carry ("ובן", "שבגיל", "הגיל", "כשבת").
const CLITICS: &[&str] = &["כש", "וש", "ו", "ש", "ה"];

/// How far back a label is looked for before a colon ("גיל בעת המבחן: 4:11").
const LABEL_WORDS: usize = 3;

/// Whether `raw` is a word for an age, with or without a clitic prefix.
fn age_word(raw: &str) -> bool {
    let w = raw.trim_matches(|c: char| !c.is_alphanumeric());
    AGE_WORDS.contains(&w)
        || CLITICS.iter().any(|p| {
            w.strip_prefix(p)
                .is_some_and(|rest| AGE_WORDS.contains(&rest))
        })
}

/// Whether the figure at `i` follows a word for an age: right before it ("בן 6:2", "ובת 6;2",
/// "גיל כרונולוגי 6:2"), or, after a label's colon ("גיל האבחון: 6:2", "גיל בעת המבחן: 4:11"),
/// "גיל"/"בגיל" within the last few words.
fn after_age_word(words: &[&str], i: usize) -> bool {
    let Some(prev) = i.checked_sub(1).and_then(|j| words.get(j)) else {
        return false;
    };
    if age_word(prev) {
        return true;
    }
    if !prev.ends_with(':') {
        return false;
    }
    words
        .get(..i)
        .unwrap_or_default()
        .iter()
        .rev()
        .take(LABEL_WORDS)
        .any(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric());
            let w = CLITICS.iter().find_map(|p| w.strip_prefix(p)).unwrap_or(w);
            matches!(w, "גיל" | "בגיל")
        })
}

fn ages_in(text: &str) -> Vec<(u8, u8)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut found = Vec::new();
    for (i, raw) in words.iter().enumerate() {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != ';');
        let word = word.trim_start_matches(|c: char| !c.is_ascii_digit());
        // `5:4` / `5;4`, only right after a word for an age (so never a clock time).
        if let Some((y, m)) = word.split_once([':', ';']) {
            let clock = m.len() == 2 && m.starts_with('0');
            if after_age_word(&words, i) && !clock {
                if let (Some(y), Some(m)) = (number(y), number(m)) {
                    if y <= 18 && m <= 11 {
                        found.push((y, m));
                    }
                }
            }
            continue;
        }
        // "בן 5 שנים ו-4 חודשים" / "בת 5 ו-4 חודשים": the months must be said.
        if matches!(*raw, "בן" | "בת" | "כבן" | "כבת") {
            let Some(y) = words.get(i + 1).and_then(|w| number(w)) else {
                continue;
            };
            let mut j = i + 2;
            if words.get(j).is_some_and(|w| matches!(*w, "שנים" | "שנה")) {
                j += 1;
            }
            let Some(m) = words.get(j).and_then(|w| {
                let w = w.trim_start_matches('ו').trim_start_matches('-');
                number(w)
            }) else {
                continue;
            };
            if words
                .get(j + 1)
                .is_some_and(|w| w.trim_matches(|c: char| !c.is_alphanumeric()) == "חודשים")
                && y <= 18
                && m <= 11
            {
                found.push((y, m));
            }
        }
    }
    found
}

fn number(s: &str) -> Option<u8> {
    let s = s.trim_matches(|c: char| !c.is_ascii_digit());
    if s.is_empty() || s.len() > 2 {
        return None;
    }
    s.parse().ok()
}

/// A word for the child in the other grammatical gender than the case details ("בן 5" for a
/// girl, "הילדה" for a boy). Only the plain cases: an age phrase and the word for the child.
#[must_use]
pub fn check_gender(text: &str, gender: Option<GrammaticalGender>) -> Vec<String> {
    let Some(gender) = gender else {
        return Vec::new();
    };
    let (wrong_age, wrong_child, right) = match gender {
        GrammaticalGender::Female => (["בן", "כבן"], "הילד", "בת"),
        GrammaticalGender::Male => (["בת", "כבת"], "הילדה", "בן"),
    };
    let words: Vec<&str> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let age_after = words
            .get(i + 1)
            .is_some_and(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit()));
        let note = if wrong_age.contains(w) && age_after {
            Some(format!(
                "כתוב «{w}», ובפרטי התיק הילד{} {right}",
                if right == "בת" { "ה" } else { "" }
            ))
        } else if *w == wrong_child && !names_no_child(i.checked_sub(1).and_then(|p| words.get(p)))
        {
            Some(format!(
                "כתוב «{w}», ובפרטי התיק {}",
                if right == "בת" {
                    "זו ילדה"
                } else {
                    "זה ילד"
                }
            ))
        } else {
            None
        };
        if let Some(n) = note {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// "המכון להתפתחות הילד", "שם הילד:": the words name a place or a form's field, not her.
fn names_no_child(prev: Option<&&str>) -> bool {
    prev.is_some_and(|p| {
        let p = p.trim_start_matches(['ו', 'ל', 'ב']);
        p == "התפתחות" || p == "שם"
    })
}

/// The letters of the grades, in order.
const GRADES: &[&str] = &["א", "ב", "ג", "ד", "ה", "ו", "ז", "ח", "ט"];

/// The grade a paragraph says the child is in now: "לומד בכיתה ב'", "תלמידת כיתה ג".
fn current_grade(text: &str) -> Option<&'static str> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        let class = matches!(w, "כיתה" | "בכיתה");
        if !class {
            continue;
        }
        let Some(before) = i.checked_sub(1).and_then(|j| words.get(j)) else {
            continue;
        };
        let before = before.trim_matches(|c: char| !c.is_alphanumeric());
        let now = matches!(
            before,
            "לומד"
                | "לומדת"
                | "תלמיד"
                | "תלמידת"
                | "משובץ"
                | "משובצת"
                | "כעת"
                | "היום"
        );
        if !now {
            continue;
        }
        let letter = words
            .get(i + 1)?
            .trim_matches(|c: char| !c.is_alphanumeric());
        if let Some(g) = GRADES.iter().find(|g| **g == letter) {
            return Some(g);
        }
    }
    None
}

/// A grade the paragraph gives that another paragraph of the report gives otherwise.
#[must_use]
pub fn check_grade(text: &str, others: &[&str]) -> Vec<String> {
    let Some(mine) = current_grade(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for o in others {
        if let Some(g) = current_grade(o).filter(|g| *g != mine) {
            let n = format!("כתוב כיתה {mine}', ובמקום אחר בדוח כיתה {g}'");
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// What a recommendation is about, and words in the body of the report that are a basis
/// for it. A recommendation with none of them in the body is pointed out.
const RECOMMENDATIONS: &[(&str, &[&str], &[&str])] = &[
    (
        "ריפוי בעיסוק",
        &["ריפוי בעיסוק", "מרפאה בעיסוק"],
        &[
            "מוטורי",
            "גרפו",
            "כתיבה",
            "ויסות חושי",
            "תחושתי",
            "אחיזה",
            "ריפוי בעיסוק",
        ],
    ),
    (
        "טיפול בתקשורת",
        &["קלינאית תקשורת", "טיפול בתקשורת", "קלינאות תקשורת"],
        &[
            "שפה",
            "שפתי",
            "דיבור",
            "הגייה",
            "היגוי",
            "אוצר מילים",
            "תקשורת",
        ],
    ),
    (
        "טיפול רגשי",
        &["טיפול רגשי", "טיפול במשחק", "פסיכותרפיה", "הדרכת הורים"],
        &[
            "רגשי",
            "רגשית",
            "חרדה",
            "ויסות",
            "התנהגות",
            "תסכול",
            "הורים",
        ],
    ),
    (
        "הערכה נוירולוגית",
        &["נוירולוג", "נוירולוגית", "הערכה רפואית"],
        &["קשב", "ריכוז", "נוירולוג", "רפואי", "התפתחות"],
    ),
    (
        "הערכה פסיכיאטרית",
        &["פסיכיאטר", "פסיכיאטרית"],
        &["קשב", "חרדה", "רגשי", "התנהגות", "פסיכיאטר"],
    ),
    (
        "פיזיותרפיה",
        &["פיזיותרפיה", "פיזיותרפיסט"],
        &["מוטוריקה גסה", "מוטורי", "יציבה", "שיווי משקל", "טונוס"],
    ),
];

/// A recommendation in `text` whose subject the body of the report (`body`) never comes near.
#[must_use]
pub fn check_recommendations(text: &str, body: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for (what, asks, basis) in RECOMMENDATIONS {
        if !asks.iter().any(|a| text.contains(a)) {
            continue;
        }
        if !body.iter().any(|b| basis.iter().any(|w| b.contains(w))) {
            out.push(format!("המלצה על {what}, ואין לה בסיס בגוף הדוח"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGE: Option<Age> = Some(Age {
        years: 5,
        months: 4,
    });

    #[test]
    fn the_childs_age_as_written_passes() {
        for text in [
            "גלעד, בן 5:4, הופנה לאבחון.",
            "בגיל 5;4 הוא מתקשה במעברים.",
            "הילד, בן 5 שנים ו-4 חודשים, שיתף פעולה.",
            "בת 5 ו-5 חודשים.",
            "אינו מגיע בשעה 8:10, ולעיתים ב-9:05.",
            "בגיל 3 החל לדבר במשפטים.",
        ] {
            assert!(check_age(text, AGE).is_empty(), "{text}");
        }
    }

    #[test]
    fn a_clock_time_is_not_an_age() {
        for text in [
            "הגיע ב-8:10 לגן.",
            "המפגש החל ב-8:10 והסתיים ב-9:30.",
            "בשעה 8:10 נכנס לחדר.",
            "משעה 7:45 בבוקר.",
            "(8:10) הגיע.",
            "בגיל הגן הגיע ב-8:10.",
            "ובשעה 8:10 נכנס לחדר.",
            "השעה 8:10.",
        ] {
            assert!(check_age(text, AGE).is_empty(), "{text}");
        }
        // The same figures after a word for an age are one.
        assert_eq!(
            check_age("הילד, בן 8:10, הגיע.", AGE),
            vec!["כתוב גיל 8:10, ובפרטי התיק 5:4".to_owned()]
        );
        assert_eq!(check_age("(בגיל 8:10) נבדק.", AGE).len(), 1);
        assert_eq!(check_age("כבת 6;2.", AGE).len(), 1);
    }

    #[test]
    fn another_age_is_pointed_out_once() {
        assert_eq!(
            check_age("גלעד, בן 6:2, הופנה. בגיל 6:2 הוא...", AGE),
            vec!["כתוב גיל 6:2, ובפרטי התיק 5:4".to_owned()]
        );
        assert_eq!(
            check_age("הילד, בן 4 שנים ו-11 חודשים, שיתף פעולה.", AGE).len(),
            1
        );
    }

    #[test]
    fn an_age_word_with_a_prefix_or_before_a_label_is_still_one() {
        for text in [
            "ובן 6:2 הוא מתקשה במעברים.",
            "ובת 6;2 היא מתקשה במעברים.",
            "שבגיל 6:2 החל לדבר.",
            "הגיל 6:2 תואם.",
            "גיל כרונולוגי 6:2.",
            "גיל כרונולוגי: 6:2",
            "גיל האבחון: 6:2",
            "כשבן 6:2 עבר גן.",
        ] {
            assert_eq!(
                check_age(text, AGE),
                vec!["כתוב גיל 6:2, ובפרטי התיק 5:4".to_owned()],
                "{text}"
            );
        }
        assert_eq!(
            check_age("גיל בעת המבחן: 4:11", AGE),
            vec!["כתוב גיל 4:11, ובפרטי התיק 5:4".to_owned()]
        );
    }

    #[test]
    fn without_an_age_in_the_details_nothing_is_said() {
        assert!(check_age("בן 6:2", None).is_empty());
    }

    #[test]
    fn a_word_for_the_child_in_the_other_gender_is_pointed_out() {
        let girl = Some(GrammaticalGender::Female);
        assert!(check_gender("הילדה, בת 5:4, שיתפה פעולה.", girl).is_empty());
        assert_eq!(
            check_gender("הילד, בן 5:4, שיתף פעולה.", girl),
            vec![
                "כתוב «הילד», ובפרטי התיק זו ילדה".to_owned(),
                "כתוב «בן», ובפרטי התיק הילדה בת".to_owned(),
            ]
        );
        assert!(
            check_gender("אחיה בן הדוד שיחק", girl).is_empty(),
            "only before an age"
        );
        assert!(check_gender("הילדה", None).is_empty());
        // An institution's name and a form's field are no word for her (the real-corpus bench).
        assert!(
            check_gender("הוערכה במכון להתפתחות הילד. שם הילד: הדר.", girl).is_empty(),
            "a name, not the child"
        );
    }

    #[test]
    fn two_grades_now_are_pointed_out_and_a_past_grade_is_not() {
        let others = [
            "בכיתה א' התקשה בקריאה.",
            "היא לומדת בכיתה ג' בבית ספר יסודי.",
        ];
        assert_eq!(
            check_grade("כיום היא תלמידת כיתה ב'.", &others),
            vec!["כתוב כיתה ב', ובמקום אחר בדוח כיתה ג'".to_owned()]
        );
        assert!(check_grade("היא לומדת בכיתה ג'.", &others).is_empty());
        assert!(check_grade("בכיתה ב' עברה דירה.", &others).is_empty());
    }

    #[test]
    fn a_recommendation_without_a_basis_in_the_body_is_pointed_out() {
        let body = ["נצפו קשיים במוטוריקה עדינה ובאחיזת עיפרון."];
        assert!(check_recommendations("מומלץ ריפוי בעיסוק.", &body).is_empty());
        assert_eq!(
            check_recommendations("מומלץ טיפול רגשי ופנייה לקלינאית תקשורת.", &body),
            vec![
                "המלצה על טיפול בתקשורת, ואין לה בסיס בגוף הדוח".to_owned(),
                "המלצה על טיפול רגשי, ואין לה בסיס בגוף הדוח".to_owned(),
            ]
        );
    }
}
