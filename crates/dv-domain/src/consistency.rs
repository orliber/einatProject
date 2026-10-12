//! Checks of a paragraph against the case's own details, in code (no AI): a gentle note,
//! never a block. She decides.

use crate::Age;

/// An age written in the text that is not the child's age in the case details.
///
/// Reads the clinical notation (`5:4`, `5;4`) and "בן/בת 5 שנים ו-4 חודשים". A clock time
/// ("בשעה 8:10", "9:05") is not an age, nor is anything over 18 years or 11 months. Up to a
/// month apart is the same age (the details and the paragraph may be a few weeks apart).
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

fn ages_in(text: &str) -> Vec<(u8, u8)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut found = Vec::new();
    for (i, raw) in words.iter().enumerate() {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != ';');
        let word = word.trim_start_matches(|c: char| !c.is_ascii_digit());
        // `5:4` / `5;4`, not right after "שעה" / "בשעה".
        if let Some((y, m)) = word.split_once([':', ';']) {
            let after_hour = i > 0 && {
                let prev = words[i - 1];
                prev.ends_with("שעה") || prev.ends_with("השעה")
            };
            let clock = m.len() == 2 && m.starts_with('0');
            if !(after_hour || clock) {
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
    fn without_an_age_in_the_details_nothing_is_said() {
        assert!(check_age("בן 6:2", None).is_empty());
    }
}
