//! The parents' version (round 55, D-095): the same approved sections, without the score
//! tables, with a chart of three groups in words instead, and her opening sentence. Nothing is
//! sent to make it: it is built here from text she already approved.
//!
//! Without "show numbers" (the default) no score is left in the text: a parenthesis that holds
//! one goes, and a sentence that still holds one goes. A number counts as a score when it is
//! one of the case's scores (or one the caller found beside a test's name), or when a score
//! word ("ציון", "אחוזון") or "%" stands beside it. Ages ("5:4"), dates ("22.10.2026") and
//! test names ("ADOS-2") are not scores. What is removed is removed whole, so no sentence is
//! left with a hole in it.

use serde::{Deserialize, Serialize};

use crate::{Report, ReportPart, ReportSection};

/// A chart of where each measure falls, in words: one row per measure, one column per group
/// (computed in code from the score tables' ranges, never by a model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandChart {
    pub title: String,
    /// The column headings, right to left ("חזק", "בטווח הגיל", "קשה יותר").
    pub groups: Vec<String>,
    pub rows: Vec<BandRow>,
    /// The sentence printed under the chart.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandRow {
    pub label: String,
    /// Index into [`BandChart::groups`].
    pub group: usize,
    /// The score, only when she chose to show numbers in the parents' version.
    pub value: Option<String>,
}

/// What the parents' version adds to the approved report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParentsVersion {
    /// Her opening sentence, from the report settings.
    pub opening: String,
    pub bands: Vec<BandChart>,
    /// Her setting "show numbers in the parents' version" (off unless she turns it on).
    pub show_numbers: bool,
    /// Every score of the case: none of them may stay in the text while numbers are off.
    pub scores: Vec<f64>,
}

/// The parents' version of a report: no score tables, the chart in words, her opening
/// sentence, and (unless she chose otherwise) no scores in the text. With it, how many
/// sentences were left out because they held a score (the export dialog says so).
#[must_use]
pub fn for_parents(full: &Report, v: ParentsVersion) -> (Report, usize) {
    let mut left_out = 0;
    let mut strip = |t: &str| {
        let (kept, n) = without_scores_counted(t, &v.scores);
        left_out += n;
        kept
    };
    let parts = full
        .parts
        .iter()
        .map(|part| ReportPart {
            title: part.title.clone(),
            sections: part
                .sections
                .iter()
                .map(|s| ReportSection {
                    title: s.title.clone(),
                    paragraphs: s
                        .paragraphs
                        .iter()
                        .map(|p| if v.show_numbers { p.clone() } else { strip(p) })
                        .filter(|p| !p.trim().is_empty())
                        .collect(),
                })
                .filter(|s| !s.paragraphs.is_empty())
                .collect(),
        })
        .filter(|p| !p.sections.is_empty())
        .collect();
    let bands = v
        .bands
        .into_iter()
        .map(|mut c| {
            if !v.show_numbers {
                for r in &mut c.rows {
                    r.value = None;
                }
            }
            c
        })
        .collect();
    let opening = if v.show_numbers {
        v.opening.trim().to_owned()
    } else {
        strip(v.opening.trim())
    };
    let report = Report {
        title: format!("{} – גרסה להורים", full.title.trim()),
        info: full.info.clone(),
        intro: if opening.is_empty() {
            Vec::new()
        } else {
            vec![opening]
        },
        bands,
        parts,
        tables: Vec::new(),
        signature: full.signature.clone(),
        confidentiality: full.confidentiality.clone(),
        font: full.font.clone(),
        logo: full.logo.clone(),
    };
    (report, left_out)
}

/// Hebrew words after which a number is a score (also with a prefix: "בציון", "והאחוזון").
const SCORE_WORDS: &[&str] = &[
    "ציון",
    "ציונו",
    "ציונה",
    "ציוני",
    "ציוניו",
    "ציוניה",
    "ציונים",
    "אחוזון",
    "אחוזונים",
    "משכל",
    "תקן",
    "טווח",
];

/// Score abbreviations, as whole words only ("T 63", "SS 7", "IQ 85"; also "T-63", "T=63").
const SCORE_MARKS: &[&str] = &["IQ", "T", "SS", "ss", "SD", "Z"];

/// How many words before a number are looked at for a score word ("ציון תקן של 85").
const LOOK_BACK: usize = 4;

/// A number in the text: where it is (bytes) and its value.
struct Number {
    start: usize,
    end: usize,
    value: f64,
}

/// Numbers that could be scores: a run of digits, with one decimal point at most. Times and
/// ages ("5:4"), dates ("22.10.2026", "3/4") and a test's edition ("ADOS-2") are left out.
fn numbers(text: &str) -> Vec<Number> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&(start, c)) = chars.get(i) {
        let in_run = |c: char| c.is_ascii_digit() || matches!(c, '.' | ':' | '/' | ',');
        if !c.is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut j = i;
        while chars.get(j).is_some_and(|(_, c)| in_run(*c)) {
            j += 1;
        }
        // A run ends with a digit: "102." at the end of a sentence is 102.
        while j > i && chars.get(j - 1).is_some_and(|(_, c)| !c.is_ascii_digit()) {
            j -= 1;
        }
        let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
        let run = text.get(start..end).unwrap_or_default();
        let before: Vec<char> = text
            .get(..start)
            .unwrap_or_default()
            .chars()
            .rev()
            .take(2)
            .collect();
        // "ADOS-2" is an edition; "T-63" and "SS-7" are scores.
        let edition = before.first() == Some(&'-')
            && before.get(1).is_some_and(char::is_ascii_alphabetic)
            && !SCORE_MARKS.contains(&latin_word_before(text, start.saturating_sub(1)));
        let compound = run.contains([':', '/']) || run.matches('.').count() > 1;
        if !edition && !compound {
            // "85,90" lists two numbers.
            let mut at = start;
            for piece in run.split(',') {
                if let Ok(value) = piece.parse::<f64>() {
                    out.push(Number {
                        start: at,
                        end: at + piece.len(),
                        value,
                    });
                }
                at += piece.len() + 1;
            }
        }
        i = j.max(i + 1);
    }
    out
}

/// The Latin word that ends at byte `end` ("T" in "T-63").
fn latin_word_before(text: &str, end: usize) -> &str {
    let head = text.get(..end).unwrap_or_default();
    let from = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .last()
        .map_or(head.len(), |(i, _)| i);
    head.get(from..).unwrap_or_default()
}

/// Whether the number at `n` is a score: one of `scores`, or beside a score word or "%".
fn is_score(text: &str, n: &Number, scores: &[f64]) -> bool {
    if scores.iter().any(|s| (s - n.value).abs() < 1e-9) {
        return true;
    }
    let after = text.get(n.end..).unwrap_or_default().trim_start();
    if after.starts_with('%') {
        return true;
    }
    // Only this sentence: a score word in the sentence before says nothing about this number.
    let head = text.get(..n.start).unwrap_or_default();
    let from = [". ", "! ", "? ", "; ", "\n"]
        .iter()
        .filter_map(|p| head.rfind(p).map(|i| i + p.len()))
        .max()
        .unwrap_or(0);
    let before: Vec<&str> = head
        .get(from..)
        .unwrap_or_default()
        .split(|c: char| !(c.is_alphanumeric() || c == '"' || c == '\''))
        .filter(|w| !w.is_empty())
        .collect();
    before
        .iter()
        .rev()
        .take(LOOK_BACK)
        .any(|w| SCORE_WORDS.iter().any(|s| w.ends_with(s)) || SCORE_MARKS.contains(w))
}

/// Byte ranges of the sentences of a paragraph: up to and including ".", "!", "?" or ";"
/// followed by a space (or the end), or a line break.
fn sentences(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        let next = iter.peek().map(|(_, c)| *c);
        let ends = c == '\n'
            || (matches!(c, '.' | '!' | '?' | ';') && next.is_none_or(char::is_whitespace));
        if ends {
            out.push((start, i + c.len_utf8()));
            start = i + c.len_utf8();
        }
    }
    if start < text.len() {
        out.push((start, text.len()));
    }
    out
}

/// The parenthesis around byte `at`, if it is inside one: (open, close) inclusive.
fn parenthesis(text: &str, at: usize) -> Option<(usize, usize)> {
    let before = text.get(..at)?;
    let open = before.rfind('(')?;
    if before.get(open..)?.contains(')') {
        return None;
    }
    let close = at + text.get(at..)?.find(')')?;
    if text.get(at..close)?.contains('(') {
        return None;
    }
    Some((open, close + 1))
}

/// The text with every score removed: a parenthesis that holds one goes, then a sentence that
/// still holds one. A text that would still hold one after that is dropped whole (fail-closed).
#[must_use]
pub fn without_scores(text: &str, scores: &[f64]) -> String {
    without_scores_counted(text, scores).0
}

/// [`without_scores`], and how many sentences were left out.
#[must_use]
pub fn without_scores_counted(text: &str, scores: &[f64]) -> (String, usize) {
    // 1. Parentheses with a score in them ("(ציון תקן 102, אחוזון 55)").
    let mut cut: Vec<(usize, usize)> = numbers(text)
        .iter()
        .filter(|n| is_score(text, n, scores))
        .filter_map(|n| parenthesis(text, n.start))
        .collect();
    cut.sort_unstable();
    cut.dedup();
    let mut kept = String::with_capacity(text.len());
    let mut at = 0;
    for (open, close) in cut {
        if open < at {
            continue;
        }
        kept.push_str(text.get(at..open).unwrap_or_default().trim_end_matches(' '));
        at = close;
    }
    kept.push_str(text.get(at..).unwrap_or_default());

    // 2. Sentences that still hold a score.
    let found = numbers(&kept);
    let mut out = String::with_capacity(kept.len());
    let mut left_out = 0;
    for (s, e) in sentences(&kept) {
        let has = found
            .iter()
            .any(|n| n.start >= s && n.end <= e && is_score(&kept, n, scores));
        if !has {
            out.push_str(kept.get(s..e).unwrap_or_default());
        } else {
            left_out += 1;
            if kept.get(s..e).is_some_and(|t| t.ends_with('\n')) {
                out.push('\n');
            }
        }
    }
    let tidy = tidy(&out);
    // 3. Fail-closed: nothing that reads as a score is left.
    if numbers(&tidy).iter().any(|n| is_score(&tidy, n, scores)) {
        let rest = sentences(&tidy)
            .iter()
            .filter(|(s, e)| tidy.get(*s..*e).is_some_and(|t| !t.trim().is_empty()))
            .count();
        return (String::new(), left_out + rest);
    }
    (tidy, left_out)
}

/// Spaces left by what was removed: doubled, at the ends of lines, before punctuation.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (k, line) in text.split('\n').enumerate() {
        if k > 0 {
            out.push('\n');
        }
        let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
        let mut l = words.join(" ");
        for p in [" .", " ,", " ;", " :"] {
            l = l.replace(p, p.trim_start());
        }
        out.push_str(&l);
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_score_in_parentheses_goes_with_its_parentheses() {
        let t = "היכולת הקוגניטיבית הכללית נמצאה בטווח הממוצע (ציון תקן 102, אחוזון 55). הילד שיתף פעולה.";
        assert_eq!(
            without_scores(t, &[102.0]),
            "היכולת הקוגניטיבית הכללית נמצאה בטווח הממוצע. הילד שיתף פעולה."
        );
    }

    #[test]
    fn a_sentence_with_a_score_goes_and_the_rest_stays() {
        let t = "בתת-המבחן קוביות קיבל ציון 7. הוא נהנה מהמשימות; בגיל 5:4 הוא מגיע לגן בשמחה.";
        let out = without_scores(t, &[7.0]);
        assert_eq!(out, "הוא נהנה מהמשימות; בגיל 5:4 הוא מגיע לגן בשמחה.");
    }

    #[test]
    fn dates_ages_and_test_names_are_not_scores() {
        let t = "האבחון נערך ב-22.10.2026 בגיל 5:4, עם ADOS-2 ו-WPPSI-IV.";
        assert_eq!(without_scores(t, &[2.0, 22.0, 5.0, 4.0]), t);
    }

    #[test]
    fn score_phrasings_from_reports_are_caught_without_the_table() {
        for t in [
            "התקבל ציון תקן של 85 במבחן.",
            "הציון נמצא בטווח 90–109 במבחן.",
            "ציוניו היו 7 ו-9 בשני המבחנים.",
            "ציוניה בשני המבחנים היו 7 ו-9.",
            "התקבל ציון T-63 בשאלון.",
            "התקבל T=63 בשאלון.",
            "בקוביות SS 7, נמוך מהצפוי.",
            "בקוביות התקבל SS-7.",
            "מנת משכל IQ 85 בבדיקה הקודמת.",
        ] {
            let (out, n) = without_scores_counted(t, &[]);
            assert_eq!(out, "", "{t}");
            assert_eq!(n, 1, "{t}");
        }
        // An edition is still not a score, and the rest of the text stays.
        let t = "הועבר ADOS-2 ו-WISC-5. התקבל ציון T-63 בשאלון.";
        assert_eq!(
            without_scores_counted(t, &[]),
            ("הועבר ADOS-2 ו-WISC-5.".to_owned(), 1)
        );
    }

    #[test]
    fn the_count_says_how_many_sentences_went() {
        let t = "הוא בן שלוש. ציון 7 בקוביות. ציון 9 במטריצות. הוא אוהב לצייר.";
        assert_eq!(
            without_scores_counted(t, &[]),
            ("הוא בן שלוש. הוא אוהב לצייר.".to_owned(), 2)
        );
        // A score in parentheses goes with them, and no sentence is counted.
        let t = "התפקוד בטווח הממוצע (ציון 102).";
        assert_eq!(
            without_scores_counted(t, &[]),
            ("התפקוד בטווח הממוצע.".to_owned(), 0)
        );
    }

    #[test]
    fn a_number_beside_a_score_word_is_a_score_even_if_not_in_the_table() {
        let t = "מהירות העיבוד באחוזון 9. שאר התחומים תקינים.";
        assert_eq!(without_scores(t, &[]), "שאר התחומים תקינים.");
        assert_eq!(without_scores("שיפור של 30% בקשב.", &[]), "");
        // A score word in the sentence before does not reach into the next one.
        assert_eq!(
            without_scores("הוא בטווח הממוצע. יש לו 2 אחים.", &[]),
            "הוא בטווח הממוצע. יש לו 2 אחים."
        );
        // A number that is no score at all stays.
        assert_eq!(
            without_scores("הוא בן שלוש ויש לו 2 אחים.", &[]),
            "הוא בן שלוש ויש לו 2 אחים."
        );
    }
}
