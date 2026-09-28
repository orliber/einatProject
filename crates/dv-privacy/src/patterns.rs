//! Pattern-based identifiers (HIPAA Safe Harbor floor + Israeli formats, STANDARDS.md §3).
//!
//! Clinical text is full of numbers that must survive (ages "בגיל 2.5", scores "(96)",
//! score lists "12 13 9"), so every rule is written to hide identifiers without eating them.

use std::sync::LazyLock;

use regex::Regex;

use crate::text::{normalize, prefix_splits, tokenize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatternKind {
    IdNumber,
    Phone,
    Email,
    Url,
    IpAddress,
    Number,
    Date,
    Year,
    Address,
    Settlement,
}

impl PatternKind {
    #[must_use]
    pub fn label_he(self) -> &'static str {
        match self {
            PatternKind::IdNumber => "מספר זהות",
            PatternKind::Phone => "מספר טלפון",
            PatternKind::Email => "כתובת דוא\"ל",
            PatternKind::Url => "כתובת אתר",
            PatternKind::IpAddress => "כתובת IP",
            PatternKind::Number => "מספר מזהה",
            PatternKind::Date => "תאריך",
            PatternKind::Year => "שנה",
            PatternKind::Address => "כתובת מגורים",
            PatternKind::Settlement => "יישוב",
        }
    }

    fn fixed_replacement(self) -> &'static str {
        match self {
            PatternKind::IdNumber => "[ת.ז.]",
            PatternKind::Phone => "[טלפון]",
            PatternKind::Email => "[דוא\"ל]",
            PatternKind::Url => "[קישור]",
            PatternKind::IpAddress | PatternKind::Number => "[מספר]",
            PatternKind::Date => "[תאריך]",
            PatternKind::Year => "[שנה]",
            PatternKind::Address => "[כתובת]",
            PatternKind::Settlement => "[יישוב_אחר]",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternHit {
    pub start: usize,
    pub end: usize,
    pub kind: PatternKind,
    pub replacement: String,
}

/// A calendar date `(year, month, day)`.
pub type Ymd = (i32, u32, u32);

/// Gregorian months in Hebrew; `true` = also an ordinary word or name, so a number must be adjacent.
const HE_MONTHS: &[(&str, u32, bool)] = &[
    ("ינואר", 1, false),
    ("פברואר", 2, false),
    ("מרץ", 3, true),
    ("מרס", 3, false),
    ("אפריל", 4, false),
    ("מאי", 5, true),
    ("יוני", 6, true),
    ("יולי", 7, true),
    ("אוגוסט", 8, false),
    ("ספטמבר", 9, false),
    ("אוקטובר", 10, false),
    ("נובמבר", 11, false),
    ("דצמבר", 12, false),
];
const EN_MONTHS: &[(&str, u32)] = &[
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("sept", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
];
/// Hebrew-calendar months – always need a day or a year next to them ("אב" is also "father").
const HEBREW_CAL_MONTHS: &[&str] = &[
    "תשרי",
    "חשוון",
    "חשון",
    "מרחשוון",
    "כסלו",
    "כסליו",
    "טבת",
    "שבט",
    "אדר",
    "ניסן",
    "אייר",
    "סיוון",
    "סיון",
    "תמוז",
    "אב",
    "אלול",
];

struct Compiled {
    email: Regex,
    url: Regex,
    ip: Regex,
    digit_run: Regex,
    spaced_phone: Regex,
    spaced_id: Regex,
    num_date: Regex,
    year: Regex,
    address: Regex,
    settlement: Regex,
    hebrew_numeral: Regex,
    hebrew_year: Regex,
}

static COMPILED: LazyLock<Result<Compiled, regex::Error>> = LazyLock::new(|| {
    Ok(Compiled {
        email: Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")?,
        url: Regex::new(
            r"(?i)(?:https?://|www\.)[^\s<>]+|[a-z0-9\-]+\.(?:co\.il|org\.il|gov\.il|ac\.il|muni\.il|com|net|org|il)(?:/[^\s<>]*)?",
        )?,
        ip: Regex::new(r"\d{1,3}(?:\.\d{1,3}){3}")?,
        // Digits joined by at most one - / . between them (IDs, phones, file numbers). No spaces,
        // so score lists such as "12 13 9" are never merged.
        digit_run: Regex::new(r"\+?\d(?:[\-./]?\d)+")?,
        spaced_phone: Regex::new(
            r"(?:\+972[\s\-]?|0)(?:5\d|[2-4]|[89]|7\d)[\s\-]\d{3}[\s\-]?\d{4}",
        )?,
        // ID numbers typed in groups ("000 000 018", "0000-0001-8").
        spaced_id: Regex::new(r"\d{3,4}[\s\-]\d{3,4}[\s\-]\d{1,3}")?,
        num_date: Regex::new(r"(\d{1,2})[./](\d{1,2})(?:[./](\d{4}|\d{2}))?")?,
        year: Regex::new(r"(19[5-9]\d|20[0-4]\d)")?,
        address: Regex::new(
            r#"(?:רחוב|רח'|שדרות|שד'|דרך|סמטת|כיכר|ככר)\s+[א-ת"'׳״\-]+(?:\s+[א-ת"'׳״\-]+){0,2}\s*\d{1,4}"#,
        )?,
        settlement: Regex::new(r"(?:קיבוץ|מושב|מושבה)\s+[א-ת\-]+(?:\s+[א-ת\-]+)?")?,
        hebrew_numeral: Regex::new(r#"^[א-ת]{1,2}['"׳״]?[א-ת]?$"#)?,
        hebrew_year: Regex::new(r#"^ה?תש[א-ת"'׳״]{1,4}$"#)?,
    })
});

#[must_use]
pub fn is_valid_israeli_id(digits: &str) -> bool {
    if digits.len() != 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let sum: u32 = digits
        .bytes()
        .enumerate()
        .map(|(i, b)| {
            let d = u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 2 };
            if d > 9 {
                d - 9
            } else {
                d
            }
        })
        .sum();
    sum % 10 == 0
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(if m <= 2 { y - 1 } else { y });
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "לפני כשבועיים", "לפני כ-3 חודשים", "בעוד כחודש"… relative to `today`.
#[must_use]
pub fn relative_he(date: Ymd, today: Ymd) -> String {
    let diff = days_from_civil(today.0, today.1, today.2) - days_from_civil(date.0, date.1, date.2);
    let (past, days) = (diff >= 0, diff.unsigned_abs());
    let amount = match days {
        0..=3 => {
            return if past {
                "לאחרונה".to_owned()
            } else {
                "בימים הקרובים".to_owned()
            }
        }
        4..=10 => "כשבוע".to_owned(),
        11..=17 => "כשבועיים".to_owned(),
        18..=25 => "כשלושה שבועות".to_owned(),
        26..=45 => "כחודש".to_owned(),
        46..=75 => "כחודשיים".to_owned(),
        76..=334 => format!("כ-{} חודשים", (days + 15) / 30),
        335..=547 => "כשנה".to_owned(),
        548..=912 => "כשנתיים".to_owned(),
        _ => format!("כ-{} שנים", (days + 182) / 365),
    };
    if past {
        format!("לפני {amount}")
    } else {
        format!("בעוד {amount}")
    }
}

fn valid_ymd(y: i32, m: u32, d: u32) -> bool {
    (1..=12).contains(&m) && (1..=31).contains(&d) && (1900..=2100).contains(&y)
}

fn expand_year(y: &str, today: Ymd) -> Option<i32> {
    let v: i32 = y.parse().ok()?;
    Some(if y.len() == 2 {
        if 2000 + v <= today.0 + 1 {
            2000 + v
        } else {
            1900 + v
        }
    } else {
        v
    })
}

/// A day-month without a year is the most recent such date that is not in the future.
fn infer_year(m: u32, d: u32, today: Ymd) -> i32 {
    if (m, d) > (today.1, today.2) {
        today.0 - 1
    } else {
        today.0
    }
}

fn char_before(text: &str, i: usize) -> Option<char> {
    text[..i].chars().next_back()
}

fn char_after(text: &str, i: usize) -> Option<char> {
    text[i..].chars().next()
}

/// Not glued to other digits (so "2021" inside "120215" is not a year).
fn digit_bounded(text: &str, start: usize, end: usize) -> bool {
    !char_before(text, start).is_some_and(|c| c.is_ascii_digit() || c == '.')
        && !char_after(text, end).is_some_and(|c| c.is_ascii_digit())
}

/// The word right before a number makes it an age or a score, not a date ("בגיל 2.5").
fn preceded_by_age_or_score(text: &str, start: usize) -> bool {
    let last = text[..start].split_whitespace().last().unwrap_or("");
    let last = last.trim_end_matches(['-', '־']);
    [
        "גיל",
        "בן",
        "בת",
        "כבן",
        "כבת",
        "ציון",
        "ממוצע",
        "סטיית",
        "T",
        "(",
        "=",
    ]
    .iter()
    .any(|w| last.ends_with(w))
}

fn push(
    hits: &mut Vec<PatternHit>,
    start: usize,
    end: usize,
    kind: PatternKind,
    replacement: String,
) {
    if start < end && !hits.iter().any(|h| start < h.end && h.start < end) {
        hits.push(PatternHit {
            start,
            end,
            kind,
            replacement,
        });
    }
}

fn month_dates(text: &str, today: Ymd, c: &Compiled, hits: &mut Vec<PatternHit>) {
    let tokens = tokenize(text);
    let num = |i: usize| tokens.get(i).and_then(|t| t.norm.parse::<u32>().ok());
    for (i, tok) in tokens.iter().enumerate() {
        let splits = prefix_splits(&tok.norm);
        let day_before = i
            .checked_sub(1)
            .and_then(num)
            .filter(|d| (1..=31).contains(d));
        let year_after = num(i + 1)
            .filter(|y| (1900..=2100).contains(y))
            .and_then(|y| i32::try_from(y).ok());

        let gregorian = splits.iter().find_map(|(p, w)| {
            HE_MONTHS
                .iter()
                .find(|(m, _, _)| normalize(m) == *w)
                .map(|(_, n, amb)| (*p, *n, *amb))
        });
        let english = EN_MONTHS
            .iter()
            .find(|(m, _)| *m == tok.norm)
            .map(|(_, n)| *n);
        let hebrew_cal = splits
            .iter()
            .any(|(_, w)| HEBREW_CAL_MONTHS.iter().any(|m| normalize(m) == *w));

        let (month, ambiguous) = match (gregorian, english) {
            (Some((_, m, amb)), _) => (Some(m), amb),
            (None, Some(m)) => (Some(m), m == 5),
            (None, None) => (None, true),
        };
        let start_tok = if day_before.is_some() { i - 1 } else { i };
        let mut end_tok = if year_after.is_some() { i + 1 } else { i };
        if let Some(m) = month {
            if ambiguous && day_before.is_none() && year_after.is_none() {
                continue;
            }
            if english.is_some()
                && day_before.is_none()
                && num(i + 1).is_some_and(|d| (1..=31).contains(&d))
            {
                end_tok = i + 1; // "March 3"
            }
            let replacement = match (day_before, year_after) {
                (Some(d), Some(y)) if valid_ymd(y, m, d) => relative_he((y, m, d), today),
                (Some(d), None) if valid_ymd(today.0, m, d) => {
                    relative_he((infer_year(m, d, today), m, d), today)
                }
                (None, Some(y)) => relative_he((y, m, 15), today),
                _ => PatternKind::Date.fixed_replacement().to_owned(),
            };
            push(
                hits,
                tokens[start_tok].start,
                tokens[end_tok].end,
                PatternKind::Date,
                replacement,
            );
        } else if hebrew_cal {
            let numeral_before = i
                .checked_sub(1)
                .and_then(|j| tokens.get(j))
                .filter(|t| c.hebrew_numeral.is_match(&text[t.start..t.end]));
            let year_tok = tokens
                .get(i + 1)
                .filter(|t| c.hebrew_year.is_match(&text[t.start..t.end]));
            if numeral_before.is_none() && year_tok.is_none() {
                continue;
            }
            let start = numeral_before.map_or(tok.start, |t| t.start);
            let end = year_tok.map_or(tok.end, |t| t.end);
            push(
                hits,
                start,
                end,
                PatternKind::Date,
                PatternKind::Date.fixed_replacement().to_owned(),
            );
        }
    }
}

/// All pattern identifiers in `text`, non-overlapping, sorted by position.
pub fn find(text: &str, today: Ymd) -> Result<Vec<PatternHit>, regex::Error> {
    let c = COMPILED.as_ref().map_err(Clone::clone)?;
    let mut hits = Vec::new();
    let fixed = |k: PatternKind| k.fixed_replacement().to_owned();

    for m in c.email.find_iter(text) {
        push(
            &mut hits,
            m.start(),
            m.end(),
            PatternKind::Email,
            fixed(PatternKind::Email),
        );
    }
    for m in c.url.find_iter(text) {
        push(
            &mut hits,
            m.start(),
            m.end(),
            PatternKind::Url,
            fixed(PatternKind::Url),
        );
    }
    for m in c.ip.find_iter(text) {
        if digit_bounded(text, m.start(), m.end()) {
            push(
                &mut hits,
                m.start(),
                m.end(),
                PatternKind::IpAddress,
                fixed(PatternKind::IpAddress),
            );
        }
    }
    for m in c.address.find_iter(text) {
        push(
            &mut hits,
            m.start(),
            m.end(),
            PatternKind::Address,
            fixed(PatternKind::Address),
        );
    }
    for m in c.settlement.find_iter(text) {
        push(
            &mut hits,
            m.start(),
            m.end(),
            PatternKind::Settlement,
            fixed(PatternKind::Settlement),
        );
    }
    for m in c.spaced_phone.find_iter(text) {
        if digit_bounded(text, m.start(), m.end()) {
            push(
                &mut hits,
                m.start(),
                m.end(),
                PatternKind::Phone,
                fixed(PatternKind::Phone),
            );
        }
    }
    for m in c.spaced_id.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        if digits.len() == 9 && digit_bounded(text, m.start(), m.end()) {
            push(
                &mut hits,
                m.start(),
                m.end(),
                PatternKind::IdNumber,
                fixed(PatternKind::IdNumber),
            );
        }
    }
    // Numeric dates first, so "14.9.2025" becomes a date and not a generic number.
    for caps in c.num_date.captures_iter(text) {
        let Some(m) = caps.get(0) else { continue };
        if !digit_bounded(text, m.start(), m.end()) || preceded_by_age_or_score(text, m.start()) {
            continue;
        }
        let (Some(d), Some(mo)) = (caps.get(1), caps.get(2)) else {
            continue;
        };
        let (Ok(d), Ok(mo)) = (d.as_str().parse::<u32>(), mo.as_str().parse::<u32>()) else {
            continue;
        };
        let year = match caps.get(3) {
            Some(y) => expand_year(y.as_str(), today),
            None => Some(infer_year(mo, d, today)),
        };
        if let Some(y) = year.filter(|y| valid_ymd(*y, mo, d)) {
            push(
                &mut hits,
                m.start(),
                m.end(),
                PatternKind::Date,
                relative_he((y, mo, d), today),
            );
        }
    }
    for m in c.digit_run.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        if digits.len() < 5 || !digit_bounded(text, m.start(), m.end()) {
            continue;
        }
        let kind = if is_valid_israeli_id(&digits) || digits.len() == 9 {
            PatternKind::IdNumber
        } else if (digits.starts_with('0') && (9..=10).contains(&digits.len()))
            || digits.starts_with("972")
        {
            PatternKind::Phone
        } else {
            PatternKind::Number
        };
        push(&mut hits, m.start(), m.end(), kind, fixed(kind));
    }
    month_dates(text, today, c, &mut hits);
    for m in c.year.find_iter(text) {
        if !digit_bounded(text, m.start(), m.end()) {
            continue;
        }
        let Ok(y) = m.as_str().parse::<i32>() else {
            continue;
        };
        let replacement = if y <= today.0 {
            relative_he((y, 7, 1), today)
        } else {
            fixed(PatternKind::Year)
        };
        push(
            &mut hits,
            m.start(),
            m.end(),
            PatternKind::Year,
            replacement,
        );
    }
    hits.sort_by_key(|h| h.start);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: Ymd = (2026, 9, 28);

    fn kinds(text: &str) -> Vec<(String, PatternKind, String)> {
        find(text, TODAY)
            .unwrap()
            .into_iter()
            .map(|h| (text[h.start..h.end].to_owned(), h.kind, h.replacement))
            .collect()
    }

    #[test]
    fn ids_phones_and_long_numbers() {
        assert_eq!(kinds("ת.ז. 000 000 018")[0].1, PatternKind::IdNumber);
        let text = [
            "ת.ז. 000000018, טלפון 054-",
            "7654321, נייח 03 123 ",
            "4567, מספר תיק 45-778812",
        ]
        .concat();
        let hits = kinds(&text);
        let k: Vec<PatternKind> = hits.iter().map(|h| h.1).collect();
        assert_eq!(
            k,
            vec![
                PatternKind::IdNumber,
                PatternKind::Phone,
                PatternKind::Phone,
                PatternKind::Number
            ]
        );
    }

    #[test]
    fn dates_become_relative_but_ages_and_scores_stay() {
        let hits = kinds("ב-14.9 לא רצה לצאת לחצר");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].2, "לפני כשבועיים");
        assert_eq!(kinds("ב14.9.2026 נבדק")[0].1, PatternKind::Date);
        assert!(kinds("בגיל 2.5 החל טיפול").is_empty());
        assert!(kinds("תפקוד ממוצע (9) ומדד 111, מטריצות 13 12 9").is_empty());
        assert_eq!(kinds("ב-3 במרץ 2024 נבדק")[0].1, PatternKind::Date);
        assert_eq!(kinds("נולד בשנת 2021")[0].2, "לפני כ-5 שנים");
        assert_eq!(kinds("ב2021 עברו דירה")[0].1, PatternKind::Year);
    }

    #[test]
    fn ambiguous_month_words_need_a_number() {
        assert!(kinds("ילד מלא מרץ ושמחה").is_empty());
        assert!(kinds("האב סיפר שהילד בוכה").is_empty());
        assert_eq!(kinds("בספטמבר האחרון התחיל גן")[0].1, PatternKind::Date);
        assert_eq!(kinds("ה' באב תשפ\"ה")[0].1, PatternKind::Date);
        assert_eq!(kinds("seen on March 3, 2025")[0].1, PatternKind::Date);
    }

    #[test]
    fn emails_urls_addresses_settlements() {
        let text = [
            "כתבו ל-someone",
            "@",
            "example.com או www.clinic.co.il, גרים ברחוב הרצל 12 ועברו לקיבוץ גבעת ברנר",
        ]
        .concat();
        let hits = kinds(&text);
        let found: Vec<PatternKind> = hits.iter().map(|h| h.1).collect();
        for k in [
            PatternKind::Email,
            PatternKind::Url,
            PatternKind::Address,
            PatternKind::Settlement,
        ] {
            assert!(found.contains(&k), "{k:?} in {found:?}");
        }
        assert!(
            kinds("מסתובב ברחוב עם אמא").is_empty(),
            "no house number – not an address"
        );
    }

    #[test]
    fn relative_phrases() {
        assert_eq!(relative_he((2026, 9, 14), TODAY), "לפני כשבועיים");
        assert_eq!(relative_he((2025, 9, 1), TODAY), "לפני כשנה");
        assert_eq!(relative_he((2026, 10, 28), TODAY), "בעוד כחודש");
    }
}
