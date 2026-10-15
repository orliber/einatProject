//! What the reading of the materials says about each one (round 31): a short summary, and for
//! a report by another professional a record (what, by whom, where, findings, recommendations).
//!
//! Claude writes the text, in the same request that sorts the materials. Everything that
//! decides an order is local: the date is read here from the document itself (dates are
//! hidden in the request), and the order of the records (the institute first, then the
//! community, newest first) is fixed in code, never by the AI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a previous assessment or treatment took place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Setting {
    /// At the institute itself.
    Institute,
    /// Outside it: a clinic, a child development unit, a private practitioner.
    Community,
    Unknown,
}

impl Setting {
    /// The value the request's schema uses; anything else is `Unknown`.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        match key {
            "institute" => Setting::Institute,
            "community" => Setting::Community,
            _ => Setting::Unknown,
        }
    }

    #[must_use]
    pub fn label_he(self) -> &'static str {
        match self {
            Setting::Institute => "במכון",
            Setting::Community => "בקהילה",
            Setting::Unknown => "לא צוין איפה",
        }
    }
}

/// A previous assessment or treatment, as the reading found it. Text is tagged (names are
/// tags); it is shown restored, as plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PriorRecord {
    /// What kind of assessment or treatment ("אבחון פסיכו-דידקטי", "ריפוי בעיסוק").
    pub kind: String,
    /// Who did it, by role.
    pub by: String,
    pub setting: Setting,
    pub findings: String,
    pub recommendations: String,
}

/// The reading of one material, stored with its routing. `passage_count` ties it to the text
/// it was made from, like the sorting: once the text changes, it no longer applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MaterialSummary {
    pub passage_count: u32,
    /// Two to five sentences, tagged. Empty when the reading said nothing about it.
    pub text: String,
    #[serde(default)]
    pub record: Option<PriorRecord>,
    /// What the check of the answer found in it (a name the AI should not know), shown beside.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Year and month of a document, as written in it.
pub type DocDate = (u16, u8);

const MONTHS: &[&str] = &[
    "ינואר",
    "פברואר",
    "מרץ",
    "אפריל",
    "מאי",
    "יוני",
    "יולי",
    "אוגוסט",
    "ספטמבר",
    "אוקטובר",
    "נובמבר",
    "דצמבר",
];

/// The date of a document: the latest date written in it, leaving out lines about a birth
/// (a report states the child's date of birth; the report itself is the latest). Reads
/// `14.3.2024`, `14/03/24`, `3/2024` and `מרץ 2024`. `None` when there is none.
#[must_use]
pub fn document_date(text: &str) -> Option<DocDate> {
    text.lines()
        .filter(|l| !l.contains("לידה") && !l.contains("נולד"))
        .flat_map(dates_in_line)
        .max()
}

fn dates_in_line(line: &str) -> Vec<DocDate> {
    let mut out = Vec::new();
    // Numeric: runs of digits joined by . / or -.
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars.get(i).is_some_and(char::is_ascii_digit)
            || (i > 0 && chars.get(i - 1).is_some_and(char::is_ascii_digit))
        {
            i += 1;
            continue;
        }
        let mut parts: Vec<String> = vec![String::new()];
        let mut j = i;
        while let Some(&c) = chars.get(j) {
            if c.is_ascii_digit() {
                if let Some(last) = parts.last_mut() {
                    last.push(c);
                }
            } else if matches!(c, '.' | '/' | '-')
                && chars.get(j + 1).is_some_and(char::is_ascii_digit)
                && parts.len() < 3
            {
                parts.push(String::new());
            } else {
                break;
            }
            j += 1;
        }
        if let Some(d) = numeric_date(&parts) {
            out.push(d);
        }
        i = j.max(i + 1);
    }
    // A month by name, then a year.
    for (m, name) in MONTHS.iter().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find(name) {
            let after = rest.get(at + name.len()..).unwrap_or_default();
            let year: String = after
                .trim_start_matches([' ', ','])
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let (Some(y), Ok(month)) = (full_year(&year, false), u8::try_from(m + 1)) {
                out.push((y, month));
            }
            rest = after;
        }
    }
    out
}

fn numeric_date(parts: &[String]) -> Option<DocDate> {
    let num = |s: &String| s.parse::<u32>().ok();
    let (month, year) = match parts {
        [d, m, y] => {
            let d = num(d)?;
            if !(1..=31).contains(&d) {
                return None;
            }
            (num(m)?, full_year(y, true)?)
        }
        [m, y] if y.len() == 4 => (num(m)?, full_year(y, false)?),
        _ => return None,
    };
    let month = u8::try_from(month).ok().filter(|m| (1..=12).contains(m))?;
    Some((year, month))
}

/// A year as written: four digits from 1990 to 2099, or (in a full date) two digits as 20xx.
fn full_year(s: &str, short_ok: bool) -> Option<u16> {
    let n: u16 = s.parse().ok()?;
    match s.len() {
        4 if (1990..=2099).contains(&n) => Some(n),
        2 if short_ok => Some(2000 + n),
        _ => None,
    }
}

/// The order of the records in "אבחונים וטיפולים קודמים": the institute first, then the
/// community, then where it is not said; within each, newest first, undated last. Returns the
/// indices of `records` in that order; equal ones keep their order.
#[must_use]
pub fn record_order(records: &[(Setting, Option<DocDate>)]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..records.len()).collect();
    idx.sort_by(|&a, &b| {
        let (sa, da) = records.get(a).copied().unwrap_or((Setting::Unknown, None));
        let (sb, db) = records.get(b).copied().unwrap_or((Setting::Unknown, None));
        sa.cmp(&sb).then_with(|| match (da, db) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        })
    });
    idx
}

/// "3/2024" for a date, for the screen.
#[must_use]
pub fn date_text(date: DocDate) -> String {
    format!("{}/{}", date.1, date.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_of_a_report_is_its_latest_date_and_never_the_birth_date() {
        let report = "דוח אבחון\nתאריך לידה: 03.05.2019\nתאריך הבדיקה: 14.3.2024\n\
                      נבדק לראשונה ב-2/2023. סיכום מרץ 2024.";
        assert_eq!(document_date(report), Some((2024, 3)));
        assert_eq!(document_date("נכתב 14/11/23"), Some((2023, 11)));
        assert_eq!(document_date("בדצמבר 2022 החל טיפול"), Some((2022, 12)));
        assert_eq!(document_date("ציון 12.5 ו-3/4 מהמשימות"), None);
        assert_eq!(document_date("נולד ב-1.1.2020"), None);
        assert_eq!(document_date("בלי תאריך"), None);
        assert_eq!(document_date("קוד 7-1234"), None);
    }

    #[test]
    fn the_institute_comes_first_then_the_community_newest_first() {
        let order = record_order(&[
            (Setting::Community, Some((2022, 5))),
            (Setting::Unknown, Some((2024, 1))),
            (Setting::Institute, Some((2021, 2))),
            (Setting::Community, None),
            (Setting::Community, Some((2023, 9))),
            (Setting::Institute, Some((2023, 1))),
        ]);
        assert_eq!(order, vec![5, 2, 4, 0, 3, 1]);
        assert_eq!(Setting::from_key("community"), Setting::Community);
        assert_eq!(Setting::from_key("anything"), Setting::Unknown);
    }
}
