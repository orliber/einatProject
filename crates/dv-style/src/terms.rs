//! One name for one thing across the report (ST10). A few professional terms have several
//! names or spellings ("זיכרון עבודה" / "זכרון עבודה", "ADHD" / "הפרעת קשב וריכוז"). Her
//! usual name for each is learned from her past reports; a paragraph that uses another one is
//! noted, and so is a report that uses two names for the same thing. Only the chosen variant
//! of each group is kept, never text.

use std::collections::BTreeMap;

/// Groups of names for one thing; within a group the longer names come first, so that
/// "הפרעת קשב וריכוז" is found before "הפרעת קשב".
pub const GROUPS: &[(&str, &[&str])] = &[
    ("adhd", &["הפרעת קשב וריכוז", "הפרעת קשב", "ADHD"]),
    ("asd", &["הספקטרום האוטיסטי", "רצף האוטיזם", "ASD"]),
    ("ld", &["לקות למידה", "LD"]),
    ("fine_motor", &["מוטוריקה עדינה", "מוטוריקה דקה"]),
    ("gross_motor", &["מוטוריקה גסה", "מוטוריקה רחבה"]),
    ("ot", &["ריפוי בעיסוק", "OT"]),
    ("working_memory", &["זיכרון עבודה", "זכרון עבודה"]),
    (
        "executive",
        &["תפקודים ניהוליים", "תפקודי ניהול", "פונקציות ניהוליות"],
    ),
    (
        "speech",
        &["קלינאית תקשורת", "קלינאית דיבור", "קלינאית שפה"],
    ),
];

/// Letters that may stand before a word in Hebrew ("ב", "ו", "ה", "ל", "מ", "ש", "כ").
const PREFIXES: &[char] = &['ב', 'ו', 'ה', 'ל', 'מ', 'ש', 'כ'];

fn letter(c: char) -> bool {
    c.is_alphanumeric()
}

/// Is `text[at..at+len]` a whole word, allowing up to two prefix letters before it?
fn whole(text: &str, at: usize, len: usize) -> bool {
    let after = text[at + len..].chars().next();
    if after.is_some_and(letter) {
        return false;
    }
    let mut before = text[..at].chars().rev();
    for _ in 0..3 {
        match before.next() {
            None => return true,
            Some(c) if !letter(c) => return true,
            Some(c) if PREFIXES.contains(&c) => {}
            Some(_) => return false,
        }
    }
    false
}

/// The names of the groups used in `text`: (group, variant) for each one found, in order.
#[must_use]
pub fn used(text: &str) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for (group, variants) in GROUPS {
        let mut taken: Vec<(usize, usize)> = Vec::new();
        for v in *variants {
            for (at, _) in text.match_indices(v) {
                let end = at + v.len();
                if whole(text, at, v.len()) && !taken.iter().any(|(s, e)| at < *e && *s < end) {
                    taken.push((at, end));
                    out.push((*group, *v));
                }
            }
        }
    }
    out
}

/// The name she uses most for each group, from `texts`, when she used one at least `min` times
/// and more often than any other.
#[must_use]
pub fn usual<'a>(texts: impl IntoIterator<Item = &'a str>, min: u32) -> BTreeMap<String, String> {
    let mut counts: BTreeMap<&str, BTreeMap<&str, u32>> = BTreeMap::new();
    for t in texts {
        for (g, v) in used(t) {
            *counts.entry(g).or_default().entry(v).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(g, vs)| {
            let mut vs: Vec<(&str, u32)> = vs.into_iter().collect();
            vs.sort_by(|a, b| b.1.cmp(&a.1));
            let (best, n) = vs[0];
            let clear = vs.get(1).is_none_or(|(_, m)| *m < n);
            (n >= min && clear).then(|| (g.to_owned(), best.to_owned()))
        })
        .collect()
}

/// Notes on a paragraph that names something otherwise than she usually does (`hers`, from
/// her past reports) or than the rest of the report does (`report`, used where she has no
/// usual name).
#[must_use]
pub fn notes(
    text: &str,
    hers: &BTreeMap<String, String>,
    report: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (g, v) in used(text) {
        let note = if let Some(h) = hers.get(g) {
            (h != v).then(|| format!("כתוב «{v}»; בדוחות שלך בדרך כלל «{h}»"))
        } else {
            report
                .get(g)
                .filter(|r| r.as_str() != v)
                .map(|r| format!("כתוב «{v}»; בשאר הדוח «{r}»"))
        };
        if let Some(n) = note {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn names_are_found_whole_with_prefixes_and_the_longest_first() {
        assert_eq!(
            used("אובחן עם הפרעת קשב וריכוז."),
            vec![("adhd", "הפרעת קשב וריכוז")]
        );
        assert_eq!(
            used("ובזיכרון עבודה נמוך"),
            vec![("working_memory", "זיכרון עבודה")]
        );
        assert!(used("ADHDX").is_empty());
        assert!(
            used("הסבזיכרון עבודה").is_empty(),
            "three letters before: part of a word"
        );
    }

    #[test]
    fn her_usual_name_wins_and_the_report_decides_where_she_has_none() {
        let hers = usual(
            [
                "הפרעת קשב וריכוז אובחנה.",
                "הפרעת קשב וריכוז מתועדת.",
                "ADHD.",
            ],
            2,
        );
        assert_eq!(
            hers.get("adhd").map(String::as_str),
            Some("הפרעת קשב וריכוז")
        );
        let report = usual(["זכרון עבודה טוב.", "זכרון עבודה חזק.", "זיכרון עבודה."], 1);
        let n = notes("עם ADHD וזיכרון עבודה טוב", &hers, &report);
        assert_eq!(
            n,
            vec![
                "כתוב «ADHD»; בדוחות שלך בדרך כלל «הפרעת קשב וריכוז»".to_owned(),
                "כתוב «זיכרון עבודה»; בשאר הדוח «זכרון עבודה»".to_owned(),
            ]
        );
        assert!(notes("הפרעת קשב וריכוז", &hers, &report).is_empty());
        assert!(
            usual(["ADHD", "הפרעת קשב"], 1).is_empty(),
            "a tie decides nothing"
        );
    }
}
