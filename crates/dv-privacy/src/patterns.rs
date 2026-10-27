//! Pattern-based identifiers (HIPAA Safe Harbor floor + Israeli formats, STANDARDS.md §3).
//!
//! Clinical text is full of numbers that must survive (ages "בגיל 2.5", scores "(96)",
//! score lists "12 13 9"), so every rule is written to hide identifiers without eating them.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use crate::text::{norm_eq, prefix_splits, tokenize};

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

    /// The kind's base in a case's mask book ([`crate::MASK_BASES`]).
    #[must_use]
    pub fn mask_base(self) -> &'static str {
        crate::masks::mask_base(self.fixed_replacement()).unwrap_or("מספר")
    }

    pub(crate) fn fixed_replacement(self) -> &'static str {
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

use dv_domain::dates::days_from_civil;
/// A calendar date `(year, month, day)`.
pub use dv_domain::dates::Ymd;

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
    // Short forms, with the geresh, and the other spelling of March.
    ("ינו'", 1, false),
    ("פבר'", 2, false),
    ("מארס", 3, false),
    ("אפר'", 4, false),
    ("אוג'", 8, false),
    ("ספט'", 9, false),
    ("אוק'", 10, false),
    ("אוקט'", 10, false),
    ("נוב'", 11, false),
    ("דצמ'", 12, false),
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
/// Words that open a two-word place name ("גבעת ברנר", "עין ורד", "בית שקמה").
const PLACE_OPENERS: &[&str] = &[
    "גבעת",
    "כפר",
    "נווה",
    "נוה",
    "בית",
    "בני",
    "עין",
    "רמת",
    "שדה",
    "שדות",
    "נאות",
    "גני",
    "גן",
    "מעלה",
    "קריית",
    "קרית",
    "ניר",
    "תל",
    "מבוא",
    "אבן",
    "אור",
    "באר",
    "רמות",
    "מצפה",
    "יד",
    "גבע",
    "הר",
    "אלוני",
    "משמר",
    "מעגן",
    "עמק",
    "נחלת",
    "שער",
    "שערי",
    "צור",
    "גינות",
    "נחל",
];

/// How much of "קיבוץ X Y" / "מושב X Y" is a place: the keyword and one name word, two after
/// a word that opens a place name. "מושב כמעט לא" (sitting, in a motor report) is no place:
/// an everyday word after the keyword is not a name unless it opens one.
fn settlement_end(found: &str) -> Option<usize> {
    let words: Vec<(usize, &str)> = found
        .split_whitespace()
        .map(|w| (w.as_ptr() as usize - found.as_ptr() as usize, w))
        .collect();
    let (start1, first) = *words.get(1)?;
    let opener = PLACE_OPENERS.contains(&first);
    if !opener && crate::lexicon::word_bucket(&crate::text::normalize(first)) >= 9 {
        return None;
    }
    let end1 = start1 + first.len();
    match words.get(2) {
        Some((start2, second))
            if opener || crate::lexicon::word_bucket(&crate::text::normalize(second)) < 9 =>
        {
            Some(start2 + second.len())
        }
        _ => Some(end1),
    }
}

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

/// What may stand between the groups of an ID, a phone, a card or an account number typed in
/// parts, for every rule that reads one (one class, so no rule is left with fewer): any space
/// (also a non-breaking, thin or ideographic one), a hyphen, the maqaf, every dash and minus,
/// the other scripts' hyphens, an apostrophe, a geresh, a comma, a colon, a semicolon and an
/// underscore. Any of them went out whole between the groups of an ID (the second and third
/// reviews of D-126). A plain number with a
/// dash ("100–115", a score range) is read by `digit_run`, which keeps to hyphens.
const GROUP_SEP: &str = r"\s\-\x{05BE}\x{2010}-\x{2015}\x{2212}\x{FE58}\x{FE63}\x{FF0D}\x{058A}\x{1806}\x{2043}\x{2E17}\x{2E3A}\x{2E3B}\x{30FC}'\x{05F3}\x{2019},:;_";

/// One character of [`GROUP_SEP`].
fn group_sep(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '-' | '\u{05BE}' | '\u{2010}'
                ..='\u{2015}'
                    | '\u{2212}'
                    | '\u{FE58}'
                    | '\u{FE63}'
                    | '\u{FF0D}'
                    | '\u{058A}'
                    | '\u{1806}'
                    | '\u{2043}'
                    | '\u{2E17}'
                    | '\u{2E3A}'
                    | '\u{2E3B}'
                    | '\u{30FC}'
                    | '\''
                    | '\u{05F3}'
                    | '\u{2019}'
                    | ','
                    | ':'
                    | ';'
                    | '_'
        )
}

/// A hyphen of any script, as opposed to a dash or a space: groups joined by one are one
/// number. Between groups joined by anything else ("100–115–120", a row of scores) nine
/// digits are an ID only with the check digit or a label.
fn hyphen(c: char) -> bool {
    matches!(
        c,
        '-' | '\u{05BE}'
            | '\u{2010}'
            | '\u{2011}'
            | '\u{FE63}'
            | '\u{FF0D}'
            | '\u{058A}'
            | '\u{1806}'
            | '\u{2043}'
            | '\u{2E17}'
    )
}

struct Compiled {
    email: Regex,
    url: Regex,
    ip: Regex,
    digit_run: Regex,
    spaced_phone: Regex,
    spaced_id: Regex,
    single_digits: Regex,
    labelled_digits: Regex,
    num_date: Regex,
    year: Regex,
    address: Regex,
    settlement: Regex,
    quarter: Regex,
    street_bare: Regex,
    labeled_number: Regex,
    iban: Regex,
    card: Regex,
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
        // so score lists such as "12 13 9" are never merged. A hyphen is also the maqaf and the
        // Unicode hyphens ("000־000־018" went out whole): a hyphen, never a dash, so a score
        // range ("100–115") is read as before.
        digit_run: Regex::new(r"\+?\d(?:[\-./\x{05BE}\x{2010}\x{2011}]?\d)+")?,
        // A phone typed in groups. Between them a space (also a non-breaking one), a hyphen or
        // any dash ("054‑1234567" with a non-breaking hyphen, "054–1234567"); the area code
        // may be in parentheses ("(054) 123-4567", "(03)1234567"), and then it is part of it.
        spaced_phone: Regex::new(&format!(
            r"(?:(?:\+972[{s}]?|0)(?:5\d|[2-4]|[89]|7\d)[{s}]|(?:\+972\s?)?\(\s?0?(?:5\d|[2-4]|[89]|7\d)\s?\)[{s}]?)\d{{3}}[{s}]?\d{{4}}",
            s = GROUP_SEP
        ))?,
        // ID numbers typed in groups ("000 000 018", "0000-0001-8"), with up to three
        // separators between them ("000 - 000 - 018", "000, 000, 018" went out whole, fourth
        // review of D-128).
        spaced_id: Regex::new(&format!(
            r"\d{{3,4}}[{s}]{{1,3}}\d{{3,4}}[{s}]{{1,3}}\d{{1,3}}",
            s = GROUP_SEP
        ))?,
        // Nine or ten digits one at a time, each followed by one separator.
        single_digits: Regex::new(&format!(r"\d(?:[{GROUP_SEP}]\d){{8,9}}"))?,
        // Nine digits in groups of any size, each at most three separators (or periods) from
        // the next: next to an ID label, an ID however it was typed ("ת.ז 00 00 00 01 8",
        // "ת.ז 00  00  00  01  8").
        labelled_digits: Regex::new(&format!(r"\d(?:[{GROUP_SEP}.]{{0,3}}\d){{8}}"))?,
        // Bank accounts (IBAN) and card numbers typed in groups of four.
        iban: Regex::new(&format!(
            r"(?i)\bIL\d{{2}}(?:[{GROUP_SEP}]?\d{{1,4}}){{3,6}}"
        ))?,
        card: Regex::new(&format!(r"\d{{4}}(?:[{GROUP_SEP}]\d{{4}}){{2,3}}"))?,
        num_date: Regex::new(r"(\d{1,2})[./](\d{1,2})(?:[./](\d{4}|\d{2}))?")?,
        year: Regex::new(r"(19[5-9]\d|20[0-4]\d)")?,
        address: Regex::new(
            r#"(?:רחוב|רח'|שדרות|שד'|דרך|סמטת|כיכר|ככר)\s+[א-ת"'׳״\-]+(?:\s+[א-ת"'׳״\-]+){0,2}\s*\d{1,4}"#,
        )?,
        settlement: Regex::new(
            r"(?:קיבוץ|מושב|מושבה|שכונת|בשכונת|לשכונת|משכונת)\s+[א-ת\-]+(?:\s+[א-ת\-]+)?",
        )?,
        // "רובע ז'" (a quarter of Ashdod), "משק 41" (a farm in a moshav).
        quarter: Regex::new(
            r#"(?:רובע|ברובע|לרובע|מרובע)\s+[א-ת]['׳](?:[^א-ת]|$)|(?:משק|במשק|למשק)\s+\d{1,4}"#,
        )?,
        // A street named without a house number ("ברחוב התשבי"); kept only when the name is
        // not an everyday word ("ברחוב הראשי").
        street_bare: Regex::new(r#"(?:רחוב|ברחוב|לרחוב|מרחוב|ברח'|רח')\s+([א-ת"'׳״\-]+)"#)?,
        // Short numbers that a label marks as an identifier: a medical license ("מ.ר. 4471"),
        // a file, a membership or a passport number.
        labeled_number: Regex::new(
            r#"(?:מ\.ר\.?|מ"ר|מ״ר|(?:מס'|מס׳|מספר)\s*(?:רישיון|רשיון|רישום|תיק|חבר|תלמיד|פנייה|הפניה|דרכון)|(?:רישיון|רשיון|תיק|דרכון)\s*(?:מס'|מס׳|מספר))\s*:?\s*(\d(?:[\d\-/]{0,12}\d)?)"#,
        )?,
        // A day in Hebrew letters: "ט\"ו", "כ'", "יג" – two letters, or a geresh / gershayim
        // ("כמו האב" is not a date).
        hebrew_numeral: Regex::new(r#"^(?:[א-ת]{1,2}|[א-ת]{1,2}['"׳״][א-ת]?)$"#)?,
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

/// "לפני כשבועיים", "לפני כ-3 חודשים", "בעוד כחודש"… relative to `today`.
#[must_use]
pub fn relative_he(date: Ymd, today: Ymd) -> String {
    let diff = days_from_civil(today) - days_from_civil(date);
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

/// Labels that make nine digits near them an ID number.
const ID_LABELS: [&str; 6] = ["ת.ז", "ת\"ז", "ת״ז", "זהות", "ID", "מ.ז"];

/// Labels written as a word of their own, so a longer word that holds them is not one:
/// "תז" and not "תזונה", "id" and not "idea".
const ID_LABEL_WORDS: [&str; 3] = ["תז", "מז", "id"];

/// Where the first ID label in this text ends.
fn label_end(near: &str) -> Option<usize> {
    let mut words = Vec::new();
    let mut from = None;
    for (i, c) in near
        .char_indices()
        .chain(std::iter::once((near.len(), ' ')))
    {
        match (c.is_alphanumeric(), from) {
            (true, None) => from = Some(i),
            (false, Some(f)) => {
                words.push((f, i));
                from = None;
            }
            _ => {}
        }
    }
    let labels = ID_LABELS
        .iter()
        .filter_map(|l| near.find(l).map(|i| i + l.len()));
    let label_words = words.into_iter().filter_map(|(f, t)| {
        near.get(f..t)
            .filter(|w| ID_LABEL_WORDS.iter().any(|l| w.eq_ignore_ascii_case(l)))
            .map(|_| t)
    });
    labels.chain(label_words).min()
}

/// An ID label within twenty characters before the number, or after it ("00 00 00 01 8
/// ת.ז", "000 000 01 8 (מספר זהות)" went out whole, fourth review of D-128). After it, only
/// when no number of its own follows the label: in "ציונים 100 112 125, ת.ז 000000018" the
/// label is the next number's, also once that number is a tag ("ת.ז [ת.ז.]", blanked to
/// `x` for reading). Text around that cannot be read (never expected) counts as a label.
fn id_label_near(text: &str, start: usize, end: usize) -> bool {
    let before = text.get(..start).and_then(|b| {
        let from = b.char_indices().rev().nth(20).map_or(0, |(i, _)| i);
        b.get(from..)
    });
    let after = text.get(end..).and_then(|a| {
        let to = a.char_indices().nth(20).map_or(a.len(), |(i, _)| i);
        a.get(..to)
    });
    let labelled_after = |a: &str| {
        label_end(a).is_some_and(|i| {
            text.get(end + i..).is_none_or(|rest| {
                !rest
                    .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '[')
                    .starts_with(|c: char| c.is_ascii_digit() || matches!(c, 'x' | '['))
            })
        })
    };
    before.is_none_or(|b| label_end(b).is_some()) || after.is_none_or(labelled_after)
}

// Both read "no character" where `i` is not a boundary (never expected: it ends or starts a
// match), and `digit_bounded`, their only reader, then takes the number as standing alone.
fn char_before(text: &str, i: usize) -> Option<char> {
    text.get(..i)?.chars().next_back()
}

fn char_after(text: &str, i: usize) -> Option<char> {
    text.get(i..)?.chars().next()
}

/// The text with each tag `blank` picks (one with no word of its own: a person's tag or a
/// generic one) written over with `x`, byte for byte, so every offset still holds and the
/// number inside a tag ("[מספר_זהות_1] מרץ") is never read as part of a date or a number
/// next to it. A letter, not a mark, so a prefix glued to a tag stays one word with it
/// ("ו[ילד] באב" is not "ו' באב"). A tag left as it is ("[חסר: …]") is read like any text.
pub(crate) fn blank_tags<'t>(text: &'t str, blank: impl Fn(&str) -> bool) -> Cow<'t, str> {
    let Some(re) = crate::masks::TAG_RE.as_ref() else {
        return Cow::Borrowed(text);
    };
    let mut out: Option<String> = None;
    for m in re.find_iter(text).filter(|m| blank(m.as_str())) {
        out.get_or_insert_with(|| text.to_owned())
            .replace_range(m.range(), &"x".repeat(m.len()));
    }
    out.map_or(Cow::Borrowed(text), Cow::Owned)
}

/// Not glued to other digits (so "2021" inside "120215" is not a year).
fn digit_bounded(text: &str, start: usize, end: usize) -> bool {
    // A period counts only as a decimal point, after a digit ("3.14159"): after a word or
    // a label ("ת.ז.000000018", "שוב.000000018") the number stands on its own, and was
    // let through whole.
    let decimal = || {
        text.get(..start)
            .and_then(|b| b.strip_suffix('.'))
            .is_some_and(|b| b.ends_with(|c: char| c.is_ascii_digit()))
    };
    !char_before(text, start).is_some_and(|c| c.is_ascii_digit() || (c == '.' && decimal()))
        && !char_after(text, end).is_some_and(|c| c.is_ascii_digit())
}

/// The word right before a number makes it an age or a score, not a date ("בגיל 2.5",
/// "אפגר 9/10"). The second end of a range follows the first ("בני 4.5–6.5").
fn preceded_by_age_or_score(text: &str, start: usize) -> bool {
    // Text that cannot be read (never expected) is not shown to be an age: the date stays.
    let Some(head) = text.get(..start) else {
        return false;
    };
    let before = head.trim_end_matches(['-', '־', '–', '—', ' ']);
    let last = before.split_whitespace().last().unwrap_or("");
    // A dash among what was trimmed off.
    let dashed = head
        .chars()
        .rev()
        .take_while(|c| matches!(c, '-' | '־' | '–' | '—' | ' '))
        .any(|c| c != ' ');
    if dashed
        && last.chars().next().is_some_and(|c| c.is_ascii_digit())
        && last.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return preceded_by_age_or_score(text, before.len() - last.len());
    }
    [
        "גיל",
        "גילאי",
        "בן",
        "בת",
        "בני",
        "בנות",
        "כבן",
        "כבת",
        "ציון",
        "ממוצע",
        "סטיית",
        "אפגר",
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

/// A day of a Hebrew month in letters, 1–30, largest letter first ("ה", "טו", "כ\"ט"):
/// "שם" and "של" are words.
fn hebrew_day(raw: &str) -> bool {
    hebrew_day_value(raw).is_some()
}

/// The day a Hebrew numeral stands for ("י\"ב" → 12), as [`hebrew_day`] reads it.
fn hebrew_day_value(raw: &str) -> Option<u32> {
    let mut total = 0;
    let mut last = u32::MAX;
    for ch in raw
        .chars()
        .filter(|ch| !matches!(ch, '\'' | '"' | '׳' | '״'))
    {
        let value = match ch {
            'א'..='ט' => u32::from(ch) - u32::from('א') + 1,
            'י' => 10,
            'כ' | 'ך' => 20,
            'ל' => 30,
            _ => return None,
        };
        if value > last {
            return None;
        }
        last = value;
        total += value;
    }
    Some(total).filter(|t| (1..=30).contains(t))
}

/// The month a word names: a Gregorian month in Hebrew, with a prefix or without ("במרץ",
/// "לינואר"), or in English, in full or short ("March", "Mar"). Also the ambiguous ones
/// ("מרץ", "מאי"): next to a date, they are read as its month.
pub(crate) fn month_word(norm: &str) -> Option<u32> {
    prefix_splits(norm)
        .iter()
        .find_map(|(_, w)| {
            HE_MONTHS
                .iter()
                .find(|(m, _, _)| norm_eq(m, w))
                .map(|(_, n, _)| *n)
        })
        .or_else(|| EN_MONTHS.iter().find(|(m, _)| *m == norm).map(|(_, n)| *n))
        .or_else(|| hebrew_calendar_month(norm).then_some(0))
}

/// A month of the Hebrew calendar ("בתשרי", "כסלו"), bare or after "ב" only: never with the
/// article ("שם האב" is the father). "אב" alone is the father far more often than the month,
/// so next to a date it is not taken into it ("3.9.2026, אב ואם נכחו").
fn hebrew_calendar_month(norm: &str) -> bool {
    prefix_splits(norm).iter().any(|(p, w)| {
        (*p == 0 || (*p == 1 && norm.starts_with('ב')))
            && !norm_eq("אב", w)
            && HEBREW_CAL_MONTHS.iter().any(|m| norm_eq(m, w))
    })
}

/// A day of the month as a word: "12", or in English with its ending ("12th", "3rd").
fn day_word(norm: &str) -> Option<u32> {
    let digits = ["st", "nd", "rd", "th"]
        .iter()
        .find_map(|end| norm.strip_suffix(end))
        .unwrap_or(norm);
    digits.parse::<u32>().ok().filter(|d| (1..=31).contains(d))
}

fn month_dates(text: &str, today: Ymd, c: &Compiled, hits: &mut Vec<PatternHit>) {
    let tokens = tokenize(text);
    let raw = |i: usize| tokens.get(i).and_then(|t| text.get(t.start..t.end));
    let norm = |i: usize| tokens.get(i).map_or("", |t| t.norm.as_str());
    let num = |i: usize| norm(i).parse::<u32>().ok();
    let is_month = |i: usize| month_word(norm(i)).is_some();
    // A day as a word: "12", "12th", or a Hebrew numeral with its geresh or gershayim
    // ("י\"ב", "ה'"); a bare pair of letters is a word ("גב").
    let day = |i: usize| {
        day_word(norm(i)).or_else(|| {
            raw(i)
                .filter(|r| r.contains(['\'', '"', '׳', '״']) && c.hebrew_numeral.is_match(r))
                .and_then(hebrew_day_value)
        })
    };
    let full_year = |i: usize| {
        num(i)
            .filter(|y| (1900..=2100).contains(y))
            .and_then(|y| i32::try_from(y).ok())
    };
    // "Jan '20", "מרץ ’20": a year with its century cut off.
    let cut_year = |i: usize| {
        tokens.get(i).is_some_and(|t| {
            text.get(..t.start)
                .is_some_and(|b| b.ends_with(['\'', '\u{2019}', '\u{05F3}']))
        })
    };
    // "מרץ 20", "12 במרץ 20": two digits after a month are the year, unless a month or
    // another number comes right after them ("12 במרץ – 15 באפריל 2020" is the 15th of
    // April, "ינואר 10 15" the 10th of January 2015).
    let short_year = |i: usize| {
        tokens
            .get(i)
            .filter(|t| t.norm.len() == 2 && t.norm.bytes().all(|b| b.is_ascii_digit()))
            .filter(|_| !is_month(i + 1) && num(i + 1).is_none())
            .and_then(|t| expand_year(&t.norm, today))
    };
    for (i, tok) in tokens.iter().enumerate() {
        let splits = prefix_splits(&tok.norm);
        // The word before the month that may be its day: past a prefix written apart with a
        // hyphen ("15 ב-ינואר") and past "of" ("12th of March").
        let prev = i.checked_sub(1).map(|j| {
            let apart = tokens.get(j).is_some_and(|p| {
                crate::text::is_valid_prefix(&p.norm)
                    && text
                        .get(p.end..tok.start)
                        .is_some_and(|g| matches!(g, "-" | "\u{05BE}"))
            });
            if apart || norm(j) == "of" {
                j.saturating_sub(1)
            } else {
                j
            }
        });
        let day_before = prev.filter(|&j| j < i).and_then(day);
        // "2020-Mar-12", "2020 אפריל": the year before the month.
        let year_before = i
            .checked_sub(1)
            .filter(|_| day_before.is_none())
            .and_then(full_year);

        let gregorian = splits.iter().find_map(|(p, w)| {
            HE_MONTHS
                .iter()
                .find(|(m, _, _)| norm_eq(m, w))
                .map(|(_, n, amb)| (*p, *n, *amb))
        });
        let english = EN_MONTHS
            .iter()
            .find(|(m, _)| *m == tok.norm)
            .map(|(_, n)| *n);
        // "ה' באב", "ה' אב" – never with the article: "שם האב" is the father.
        let hebrew_cal = splits.iter().any(|(p, w)| {
            (*p == 0 || (*p == 1 && tok.norm.starts_with('ב')))
                && HEBREW_CAL_MONTHS.iter().any(|m| norm_eq(m, w))
        });
        let (month, ambiguous) = match (gregorian, english) {
            (Some((_, m, amb)), _) => (Some(m), amb),
            (None, Some(m)) => (Some(m), m == 5),
            (None, None) => (None, true),
        };
        // "ביוני", "ממאי": May–July with "in" or "from" glued on are months; were the word
        // the name יוני or יולי, hiding it is right too. Not "במרץ" ("energetically").
        let glued_month = gregorian.is_some_and(|(p, m, _)| {
            p == 1 && (5..=7).contains(&m) && tok.norm.starts_with(['ב', 'מ'])
        });
        if let Some(m) = month {
            let mut start_tok = match (day_before, prev) {
                (Some(_), Some(j)) => j,
                _ => i,
            };
            let mut end_tok = i;
            let (mut day_of, mut year_of) = (day_before, None);
            if let Some(y) = full_year(i + 1).or_else(|| {
                (cut_year(i + 1)
                    || (year_before.is_none()
                        && (gregorian.is_some() || (english.is_some() && day_before.is_some()))))
                .then(|| short_year(i + 1))
                .flatten()
            }) {
                year_of = Some(y);
                end_tok = i + 1;
            } else if day_before.is_none() && !cut_year(i + 1) {
                // "March 3", "March the 12th, 2020", "March 3, 25"; in Hebrew only with the
                // year after it ("ינואר 1 2015"), since "מרץ 20" is March 2020.
                let n = if english.is_some() && norm(i + 1) == "the" {
                    i + 2
                } else {
                    i + 1
                };
                if let Some(d) = day(n) {
                    let year = full_year(n + 1).or_else(|| short_year(n + 1));
                    if english.is_some() || year.is_some() || year_before.is_some() {
                        day_of = Some(d);
                        year_of = year;
                        end_tok = if year.is_some() { n + 1 } else { n };
                    }
                }
            }
            if year_of.is_none() {
                if let Some(y) = year_before {
                    year_of = Some(y);
                    start_tok = i.saturating_sub(1);
                }
            }
            if ambiguous && day_of.is_none() && year_of.is_none() && !glued_month {
                continue;
            }
            // "may" is a verb too ("She may 3 times refuse"): as a month, with a year, or
            // written "May" with a day.
            let may_verb = english == Some(5)
                && year_of.is_none()
                && !raw(i).is_some_and(|r| r.starts_with('M'));
            if may_verb {
                continue;
            }
            let replacement = match (day_of, year_of) {
                (Some(d), Some(y)) if valid_ymd(y, m, d) => relative_he((y, m, d), today),
                (Some(d), None) if valid_ymd(today.0, m, d) => {
                    relative_he((infer_year(m, d, today), m, d), today)
                }
                (None, Some(y)) => relative_he((y, m, 15), today),
                _ => PatternKind::Date.fixed_replacement().to_owned(),
            };
            // The day and the year were read from these tokens; were one missing (never
            // expected), everything up to the text's edge on that side is hidden with it.
            let start = tokens
                .get(start_tok)
                .or(tokens.first())
                .map_or(tok.start, |t| t.start.min(tok.start));
            let end = tokens
                .get(end_tok)
                .or(tokens.last())
                .map_or(tok.end, |t| t.end.max(tok.end));
            push(hits, start, end, PatternKind::Date, replacement);
        } else if hebrew_cal {
            // "[אב]" is the father's tag, not the month: "[אם] ו[אב]" is not the 6th of Av,
            // nor "ו[אב_2]" (a second father) the 2nd.
            if inside_tag(text, tok.start, tok.end) {
                continue;
            }
            // A neighbour that cannot be read (never expected) is taken into the date.
            // Past "in" written apart ("3 ב-אדר 2024"), as for the other months.
            let day_at = prev.filter(|&j| j < i);
            let numeral_before = day_at.and_then(|j| tokens.get(j)).filter(|t| {
                text.get(t.start..t.end)
                    .is_none_or(|raw| c.hebrew_numeral.is_match(raw) && hebrew_day(raw))
            });
            // "בתשרי", "ב-תשרי": with "in", the month, not a name ("סיון").
            let in_month = splits.iter().any(|(p, w)| {
                *p == 1
                    && tok.norm.starts_with('ב')
                    && HEBREW_CAL_MONTHS.iter().any(|m| norm_eq(m, w))
            }) || i.checked_sub(1).is_some_and(|j| {
                norm(j) == "ב"
                    && tokens.get(j).is_some_and(|p| {
                        text.get(p.end..tok.start)
                            .is_some_and(|g| matches!(g, "-" | "\u{05BE}"))
                    })
            });
            // A day in digits: with the year after it ("15 תשרי 2020"), or before "in" the
            // month ("15 בתשרי" went out unchanged, fourth review of D-128).
            let digits_before = day_at
                .filter(|&j| num(j).is_some_and(|d| (1..=30).contains(&d)))
                .and_then(|j| tokens.get(j));
            // "אדר ב' 2024": the second Adar.
            let after = if norm_eq("אדר", tok.norm.trim_start_matches('ב'))
                && raw(i + 1).is_some_and(|r| matches!(r, "א'" | "ב'" | "א׳" | "ב׳"))
            {
                i + 2
            } else {
                i + 1
            };
            // A Hebrew year ("תשפ\"ה") or a Gregorian one ("בכסלו 2021"), also cut short after
            // "in" the month or a day ("בתשרי 20", "15 בתשרי 20").
            let with_day = numeral_before.is_some() || digits_before.is_some();
            let year_tok = tokens.get(after).filter(|t| {
                full_year(after).is_some()
                    || ((in_month || with_day) && short_year(after).is_some())
                    || text
                        .get(t.start..t.end)
                        .is_none_or(|raw| c.hebrew_year.is_match(raw))
            });
            let before =
                numeral_before.or(digits_before.filter(|_| year_tok.is_some() || in_month));
            if before.is_none() && year_tok.is_none() {
                continue;
            }
            let start = before.map_or(tok.start, |t| t.start);
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

/// The word at `start..end` is part of a tag the filter wrote ("[אב]", "[אב_2]", "[בית_ספר_1]").
/// Text that cannot be read (never expected) is not shown to be a tag.
fn inside_tag(text: &str, start: usize, end: usize) -> bool {
    let (Some(head), Some(tail)) = (text.get(..start), text.get(end..)) else {
        return false;
    };
    let (Some(open), Some(close)) = (head.rfind('['), tail.find(']')) else {
        return false;
    };
    let Some(inner) = text.get(open + 1..end + close) else {
        return false;
    };
    inner.chars().count() <= 40 && inner.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// The ASCII digit a decimal digit of another script stands for ("٣", "３", "𝟑" → '3').
fn ascii_digit(c: char) -> Option<char> {
    // The zero of each run of ten decimal digits a document may hold: Arabic-Indic, Persian,
    // N'Ko, the Indic scripts, Thai, Lao, Tibetan, Myanmar, Khmer, Mongolian, full-width,
    // and the mathematical digits (bold, double-struck, sans-serif, monospace).
    const ZEROS: &[u32] = &[
        0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6,
        0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0xFF10, 0x1D7CE,
        0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D7F6,
    ];
    let code = u32::from(c);
    ZEROS
        .iter()
        .find(|z| (**z..*z + 10).contains(&code))
        .and_then(|z| char::from_digit(code - z, 10))
}

/// `text` as the rules read it, and for each byte of it the span of the original character
/// it came from. Digits of other scripts become ASCII digits, full-width forms ASCII and
/// Hebrew presentation forms bare letters ("０５４", "ת.ז. ١٢٣…" are a phone and an ID on
/// screen); invisible characters and points are dropped, so a zero-width space or a direction
/// mark inside a number or a street name does not hide it from every rule.
fn fold(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut out = String::with_capacity(text.len());
    let mut spans = Vec::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        if crate::text::is_mark(c) {
            continue;
        }
        let folded = ascii_digit(c).unwrap_or_else(|| crate::text::compat_char(c));
        out.push(folded);
        spans.resize(out.len(), (i, i + c.len_utf8()));
    }
    (out, spans)
}

/// All pattern identifiers in `text`, non-overlapping, sorted by position.
pub fn find(text: &str, today: Ymd) -> Result<Vec<PatternHit>, regex::Error> {
    let (folded, spans) = fold(text);
    if folded == text {
        return find_plain(text, today);
    }
    Ok(find_plain(&folded, today)?
        .into_iter()
        .filter_map(|h| {
            let start = spans.get(h.start)?.0;
            let end = spans.get(h.end.checked_sub(1)?)?.1;
            Some(PatternHit { start, end, ..h })
        })
        .collect())
}

fn find_plain(text: &str, today: Ymd) -> Result<Vec<PatternHit>, regex::Error> {
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
    for caps in c.street_bare.captures_iter(text) {
        let (Some(m), Some(name)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        let word = crate::text::normalize(name.as_str());
        if crate::lexicon::word_bucket(&word) < 9 && word.chars().count() >= 2 {
            push(
                &mut hits,
                m.start(),
                name.end(),
                PatternKind::Address,
                fixed(PatternKind::Address),
            );
        }
    }
    for caps in c.labeled_number.captures_iter(text) {
        let (Some(m), Some(num)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        // "80 מ\"ר" is square meters: the label follows a number there (not shown when the
        // text before cannot be read, never expected).
        let after_number = text.get(..m.start()).is_some_and(|before| {
            before
                .trim_end()
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_digit())
        });
        if !after_number && digit_bounded(text, num.start(), num.end()) {
            push(
                &mut hits,
                num.start(),
                num.end(),
                PatternKind::Number,
                fixed(PatternKind::Number),
            );
        }
    }
    for m in c.quarter.find_iter(text) {
        let found = m.as_str();
        let end = m.start()
            + found
                .trim_end_matches(|ch: char| !ch.is_alphanumeric() && ch != '\'' && ch != '׳')
                .len();
        push(
            &mut hits,
            m.start(),
            end,
            PatternKind::Address,
            fixed(PatternKind::Address),
        );
    }
    for m in c.settlement.find_iter(text) {
        if let Some(end) = settlement_end(m.as_str()) {
            push(
                &mut hits,
                m.start(),
                m.start() + end,
                PatternKind::Settlement,
                fixed(PatternKind::Settlement),
            );
        }
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
    for m in c.iban.find_iter(text).chain(c.card.find_iter(text)) {
        if digit_bounded(text, m.start(), m.end()) {
            push(
                &mut hits,
                m.start(),
                m.end(),
                PatternKind::Number,
                fixed(PatternKind::Number),
            );
        }
    }
    for m in c.spaced_id.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        // Grouped with spaces, nine digits are an ID only when they pass the check digit or
        // an ID label is next to them: "112 104 106" is a row of scores.
        let labelled = id_label_near(text, m.start(), m.end());
        // A row of scores is typed with spaces, commas or dashes ("100–115–120").
        let spaced = m
            .as_str()
            .chars()
            .any(|ch| !ch.is_ascii_digit() && !hyphen(ch));
        if digits.len() == 9
            && (!spaced || is_valid_israeli_id(&digits) || labelled)
            && digit_bounded(text, m.start(), m.end())
        {
            push(
                &mut hits,
                m.start(),
                m.end(),
                PatternKind::IdNumber,
                fixed(PatternKind::IdNumber),
            );
        }
    }
    // One digit at a time ("1 2 3 4 5 6 7 8 2"): a form with a box per digit, as OCR reads
    // it. Nine digits are an ID when the check digit fits or a label is near; ten digits
    // from 0 are a phone. Longer runs and rows of scores stay.
    for m in c.single_digits.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        // Text around that cannot be read (never expected) is taken as no digit next to the
        // run and as a label before it: the run is hidden.
        let before = text.get(..m.start()).map(|b| b.trim_end_matches(group_sep));
        let after = text.get(m.end()..).map(|a| a.trim_start_matches(group_sep));
        let alone = !before.is_some_and(|b| b.ends_with(|ch: char| ch.is_ascii_digit()))
            && !after.is_some_and(|a| a.starts_with(|ch: char| ch.is_ascii_digit()));
        let labelled = id_label_near(text, m.start(), m.end());
        let kind = match digits.len() {
            9 if is_valid_israeli_id(&digits) || labelled => PatternKind::IdNumber,
            10 if digits.starts_with('0') && !digits.starts_with("00") => PatternKind::Phone,
            _ => continue,
        };
        if alone {
            push(&mut hits, m.start(), m.end(), kind, fixed(kind));
        }
    }
    // Nine digits next to an ID label are the ID, in groups of any size: "ת.ז 00 00 00 01
    // 8" and "ת.ז 000 000 01 8" went out whole (third review of D-126).
    for m in c.labelled_digits.find_iter(text) {
        // A full date in them is a date ("ת.ז בתיק. נבדק 12.3.2020 10:15" lost the date and
        // left ":15"): the date rule hides its digits whole, so no ID is left either.
        let dated = c
            .num_date
            .captures_iter(m.as_str())
            .any(|caps| caps.get(3).is_some_and(|y| y.len() == 4));
        if !dated
            && id_label_near(text, m.start(), m.end())
            && digit_bounded(text, m.start(), m.end())
        {
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
        // An age or a score never carries a year: "בגיל 2.5" is not a date, "בגיל 2.5.2020" is.
        if !digit_bounded(text, m.start(), m.end())
            || (caps.get(3).is_none() && preceded_by_age_or_score(text, m.start()))
        {
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
    // Before plain runs of digits, so "January-1-2015" is one date, not "January-[מספר]"
    // (D-128). Whatever a date takes is hidden whole, so an ID it reads as a day goes too.
    month_dates(text, today, c, &mut hits);
    let run_kind = |digits: &str| {
        if is_valid_israeli_id(digits) || digits.len() == 9 {
            PatternKind::IdNumber
        } else if (digits.starts_with('0') && (9..=10).contains(&digits.len()))
            || digits.starts_with("972")
        {
            PatternKind::Phone
        } else {
            PatternKind::Number
        }
    };
    for m in c.digit_run.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        if digits.len() < 5 || !digit_bounded(text, m.start(), m.end()) {
            continue;
        }
        let kind = run_kind(&digits);
        push(&mut hits, m.start(), m.end(), kind, fixed(kind));
        // A date already took the end of the run ("000000018-3.9.2026") or its start
        // ("3.9.2026/000000018"): an ID or a phone in a part of it between joiners is still
        // hidden (it went out whole).
        let mut at = m.start();
        for part in m
            .as_str()
            .split(['-', '.', '/', '\u{05BE}', '\u{2010}', '\u{2011}'])
        {
            let (start, end) = (at, at + part.len());
            // The joiner after this part, in bytes.
            at = end
                + text
                    .get(end..)
                    .and_then(|r| r.chars().next())
                    .map_or(0, char::len_utf8);
            let digits: String = part.chars().filter(char::is_ascii_digit).collect();
            if digits.len() >= 9 && part.bytes().all(|b| b.is_ascii_digit()) {
                let kind = run_kind(&digits);
                push(&mut hits, start, end, kind, fixed(kind));
            }
        }
    }
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
    // A Hebrew year on its own: "תשפ\"ה", "בשנת הלימודים התשפ״ו" (not "תשע" = nine).
    for t in crate::text::tokenize(text) {
        // A word that cannot be read (never expected) is hidden as a year.
        let year_like = text.get(t.start..t.end).is_none_or(|raw| {
            let bare = raw.trim_start_matches(['ב', 'ל', 'מ', 'ו', 'ה']);
            bare.starts_with("תש")
                && bare.chars().filter(|c| matches!(c, '"' | '״')).count() == 1
                && bare.chars().count() <= 6
        });
        if year_like && !hits.iter().any(|h| h.start < t.end && t.start < h.end) {
            push(
                &mut hits,
                t.start,
                t.end,
                PatternKind::Year,
                fixed(PatternKind::Year),
            );
        }
    }
    join_month_names(text, &mut hits);
    hits.sort_by_key(|h| h.start);
    Ok(hits)
}

/// What may stand between a month name and the date it belongs to: up to four spaces,
/// commas, hyphens or dashes, slashes, periods and parentheses ("Jan. - 1.1.2015").
fn date_gap(gap: &str) -> bool {
    gap.chars().count() <= 4
        && gap.chars().all(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    ',' | '-' | '\u{05BE}' | '\u{2010}'..='\u{2015}' | '/' | '.' | '(' | ')'
                )
        })
}

/// A date phrase with a month name comes out as one generalized date (D-128), with nothing
/// of it left beside it: "March 12.3.2020" went out as "March לפני כ-7 שנים" (the numeric
/// date took the day, and the month was left), and the gate refused it; "2020 March" as two
/// generalizations; "12.3.2020 (מרץ)" with the month in parentheses. A month name right
/// next to a date or a year is taken into it, and a date or a year next to one that carries
/// a month name joins it. Two dates that both carry a month ("מרץ 2020 – מאי 2021", a range)
/// stay two. A month name alone, with nothing next to it ("במרץ"), is read as before.
fn join_month_names(text: &str, hits: &mut Vec<PatternHit>) {
    let tokens = tokenize(text);
    // A month of the Hebrew calendar only with "in" glued on or written apart ("בסיון",
    // "ב-סיון"): bare, it may be a name ("פגישה עם סיון 12.3.2020" hid סיון in the date, and
    // it was never asked about, fourth review of D-128).
    let month_at = |t: &crate::text::Token, apart_in: bool| {
        let month = match month_word(&t.norm) {
            Some(0) => apart_in || t.norm.starts_with('ב'),
            Some(_) => true,
            None => false,
        };
        month && !inside_tag(text, t.start, t.end)
    };
    // The words next to a date, as spans: each word, and a prefix written apart with a
    // hyphen together with the month after it ("2015 ב-מרץ"), as one.
    let mut words: Vec<(usize, usize, bool)> = Vec::with_capacity(tokens.len());
    let mut idx = 0;
    while let Some(t) = tokens.get(idx) {
        let next = tokens.get(idx + 1);
        let glued = next.filter(|n| {
            crate::text::is_valid_prefix(&t.norm)
                && month_at(n, t.norm == "ב")
                && text
                    .get(t.end..n.start)
                    .is_some_and(|g| matches!(g, "-" | "\u{05BE}"))
        });
        if let Some(n) = glued {
            words.push((t.start, n.end, true));
            idx += 2;
        } else {
            words.push((t.start, t.end, month_at(t, false)));
            idx += 1;
        }
    }
    let dated = |k: PatternKind| matches!(k, PatternKind::Date | PatternKind::Year);
    let has_month = |h: &PatternHit| {
        words
            .iter()
            .any(|&(s, e, m)| m && h.start <= s && e <= h.end)
    };
    let fixed = |r: &str| {
        r == PatternKind::Date.fixed_replacement() || r == PatternKind::Year.fixed_replacement()
    };
    // A year written out in the hit ("2020"): its generalization knows the year.
    let has_year = |h: &PatternHit| {
        h.kind == PatternKind::Year
            || tokens.iter().any(|t| {
                h.start <= t.start
                    && t.end <= h.end
                    && t.norm.len() == 4
                    && t.norm
                        .parse::<u32>()
                        .is_ok_and(|y| (1900..=2100).contains(&y))
            })
    };
    // Each pass takes one word or joins two hits; a text has fewer passes than words.
    for _ in 0..=words.len() {
        let mut changed = false;
        'hits: for k in 0..hits.len() {
            let Some(h) = hits.get(k).filter(|h| dated(h.kind)).cloned() else {
                continue;
            };
            let before = words.iter().rev().find(|w| w.1 <= h.start);
            let after = words.iter().find(|w| w.0 >= h.end);
            let near = [
                before.filter(|w| text.get(w.1..h.start).is_some_and(date_gap)),
                after.filter(|w| text.get(h.end..w.0).is_some_and(date_gap)),
            ];
            for &(ws, we, is_month) in near.into_iter().flatten() {
                let other = hits.iter().position(|o| o.start < we && ws < o.end);
                match other {
                    // A month name that nothing took yet.
                    None if is_month => {
                        let Some(this) = hits.get_mut(k) else {
                            break 'hits;
                        };
                        this.kind = PatternKind::Date;
                        if ws < this.start {
                            this.start = ws;
                        } else {
                            this.end = we;
                            // "12.3.2020 (מרץ)": the closing parenthesis with it.
                            let opened = text.get(h.end..ws).is_some_and(|g| g.contains('('));
                            if opened && text.get(we..).is_some_and(|r| r.starts_with(')')) {
                                this.end += 1;
                            }
                        }
                        changed = true;
                    }
                    Some(j) if j != k => {
                        let Some(o) = hits.get(j).filter(|o| dated(o.kind)).cloned() else {
                            continue;
                        };
                        // Two dates, each with its own month or its own year, stay two.
                        if has_month(&h) == has_month(&o) || (has_year(&h) && has_year(&o)) {
                            continue;
                        }
                        // The more exact generalization stays: one that knows the year
                        // ("ב-12.3 במרץ 2020" is 2020, not this year), a relative time over a
                        // fixed tag, a date over a year.
                        let rank = |x: &PatternHit| {
                            (
                                has_year(x),
                                !fixed(&x.replacement),
                                x.kind == PatternKind::Date,
                            )
                        };
                        let replacement = if rank(&o) > rank(&h) {
                            o.replacement.clone()
                        } else {
                            h.replacement.clone()
                        };
                        let joined = PatternHit {
                            start: h.start.min(o.start),
                            end: h.end.max(o.end),
                            kind: PatternKind::Date,
                            replacement,
                        };
                        hits.retain(|x| *x != h && *x != o);
                        hits.push(joined);
                        changed = true;
                    }
                    _ => {}
                }
                if changed {
                    break 'hits;
                }
            }
        }
        if !changed {
            break;
        }
    }
    // A parenthesis the date opened or closed goes with it: "12 במרץ (2020)" left ")" behind,
    // "(12 במרץ) 2020" left "(".
    for k in 0..hits.len() {
        let Some(h) = hits.get(k).filter(|h| h.kind == PatternKind::Date) else {
            continue;
        };
        let Some(inside) = text.get(h.start..h.end) else {
            continue;
        };
        let (opens, closes) = (inside.matches('(').count(), inside.matches(')').count());
        let (start, end) =
            if opens > closes && text.get(h.end..).is_some_and(|r| r.starts_with(')')) {
                (h.start, h.end.saturating_add(1))
            } else if closes > opens && text.get(..h.start).is_some_and(|b| b.ends_with('(')) {
                (h.start.saturating_sub(1), h.end)
            } else {
                continue;
            };
        let free = hits
            .iter()
            .enumerate()
            .all(|(j, o)| j == k || o.end <= start || end <= o.start);
        if let Some(h) = hits.get_mut(k).filter(|_| free) {
            (h.start, h.end) = (start, end);
        }
    }
}

#[cfg(test)]
#[allow(clippy::string_slice)]
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
    fn offsets_inside_a_character_read_the_strict_way() {
        // "א" is two bytes: byte 1 is inside it.
        assert!(digit_bounded("א12", 1, 4));
        assert!(!digit_bounded("12345", 1, 3));
        // Not shown to be a tag, so "אב" there is still read as the month.
        assert!(!inside_tag("[אב]", 2, 5));
        assert!(inside_tag("[אב]", 1, 5));
        // Not shown to be an age, so the date is still hidden.
        assert!(!preceded_by_age_or_score("בגיל 3", 1));
        assert!(preceded_by_age_or_score("בגיל 3", "בגיל ".len()));
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
    fn phones_with_the_area_code_in_parentheses_or_other_dashes_are_hidden_whole() {
        for text in [
            "טלפון (054) 123-4567 בערב",
            "טלפון (054)1234567 בערב",
            "טלפון (03) 1234567 בערב",
            "טלפון 054\u{2011}1234567 בערב",
            "טלפון 054\u{2013}123\u{2013}4567 בערב",
            "טלפון 054\u{00A0}123\u{00A0}4567 בערב",
            "טלפון +972 (54) 123-4567 בערב",
        ] {
            let hits = kinds(text);
            assert_eq!(hits.len(), 1, "{text}: {hits:?}");
            assert_eq!(hits[0].1, PatternKind::Phone, "{text}");
            let start = text
                .find(|c: char| c == '(' || c == '+' || c.is_ascii_digit())
                .unwrap();
            let end = text.rfind(|c: char| c.is_ascii_digit()).unwrap() + 1;
            assert_eq!(hits[0].0, &text[start..end], "{text}: the whole number");
        }
        // Score ranges with a dash are not phones.
        assert!(kinds("טווח ממוצע 85–115, ציון 92")
            .iter()
            .all(|h| h.1 != PatternKind::Phone));
    }

    #[test]
    fn dates_become_relative_but_ages_and_scores_stay() {
        let hits = kinds("ב-14.9 לא רצה לצאת לחצר");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].2, "לפני כשבועיים");
        assert_eq!(kinds("ב14.9.2026 נבדק")[0].1, PatternKind::Date);
        assert!(kinds("בגיל 2.5 החל טיפול").is_empty());
        assert!(kinds("אפגר 9/10, ללא סיבוכים").is_empty());
        assert_eq!(
            kinds("מומחית ברפואת ילדים, מ.ר. 4471")[0].1,
            PatternKind::Number
        );
        assert_eq!(kinds("תיק מס' 31")[0].1, PatternKind::Number);
        assert!(kinds("דירה של 80 מ\"ר 3 חדרים").is_empty());
        assert!(kinds("ארבעה ילדים בני 4.5–6.5 השתתפו").is_empty());
        // A year makes it a date whatever comes before; a range of dates stays dates.
        assert_eq!(kinds("בגיל 2.5.2020 נבדק")[0].1, PatternKind::Date);
        assert_eq!(kinds("בין 14.9–20.9 נבדק").len(), 2);
        assert!(kinds("תפקוד ממוצע (9) ומדד 111, מטריצות 13 12 9").is_empty());
        assert_eq!(kinds("ב-3 במרץ 2024 נבדק")[0].1, PatternKind::Date);
        assert_eq!(kinds("נולד בשנת 2021")[0].2, "לפני כ-5 שנים");
        assert_eq!(kinds("ב2021 עברו דירה")[0].1, PatternKind::Year);
    }

    #[test]
    fn ambiguous_month_words_need_a_number() {
        assert!(kinds("ילד מלא מרץ ושמחה").is_empty());
        assert!(kinds("עובד במרץ על המשימה").is_empty());
        assert_eq!(kinds("צפתה בו ביוני והמליצה")[0].1, PatternKind::Date);
        assert_eq!(kinds("את התצפית ממאי.")[0].1, PatternKind::Date);
        assert!(kinds("האב סיפר שהילד בוכה").is_empty());
        assert_eq!(kinds("בספטמבר האחרון התחיל גן")[0].1, PatternKind::Date);
        assert_eq!(kinds("ה' באב תשפ\"ה")[0].1, PatternKind::Date);
        // The father's tag after "and" is not "the 6th of Av".
        assert!(kinds("[אם] ו[אב] מתארים ילד סקרן").is_empty());
        assert!(kinds("אבא ו[אב_2] [ילד]").is_empty());
        assert_eq!(kinds("ו' אב תשפ\"ה")[0].1, PatternKind::Date);
        assert_eq!(kinds("ט\"ו באב")[0].1, PatternKind::Date);
        assert!(kinds("שם האב: שמעון").is_empty());
        assert!(kinds("אחיו של האב אובחן").is_empty());
        assert_eq!(kinds("seen on March 3, 2025")[0].1, PatternKind::Date);
    }

    /// Every tag the filter can write, glued to any prefix and after a Hebrew numeral, is
    /// never read as a date, a number or anything else: the gate would refuse the filter's
    /// own output, and she could do nothing about it (D-050).
    #[test]
    fn the_filters_own_tags_are_never_patterns() {
        let mut tags: Vec<String> = crate::gate::GENERIC_TAGS
            .iter()
            .map(|t| (*t).to_owned())
            .collect();
        for role in dv_domain::Role::ALL {
            let base = role.tag_base();
            tags.push(format!("[{base}]"));
            tags.extend((1..=12).map(|n| format!("[{base}_{n}]")));
        }
        for tag in &tags {
            for prefix in ["", "ו", "ה", "ב", "ל", "מ", "ש", "וב", "ו-"] {
                for before in ["", "ה' ", "ו ", "ט\"ו ", "[אם] ", "אבא "] {
                    for after in ["", " תשפ\"ה", " 2", " ו-3"] {
                        let text = format!("{before}{prefix}{tag}{after}");
                        let at = before.len() + prefix.len();
                        let own = find(&text, TODAY)
                            .unwrap()
                            .into_iter()
                            .any(|h| h.start < at + tag.len() && at < h.end);
                        assert!(!own, "{text}: {:?}", kinds(&text));
                    }
                }
            }
        }
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
        assert_eq!(kinds("השריפה ברחוב התשבי")[0].1, PatternKind::Address);
        assert!(kinds("גרים ברחוב הראשי של העיר").is_empty());
    }

    #[test]
    fn relative_phrases() {
        assert_eq!(relative_he((2026, 9, 14), TODAY), "לפני כשבועיים");
        assert_eq!(relative_he((2025, 9, 1), TODAY), "לפני כשנה");
        assert_eq!(relative_he((2026, 10, 28), TODAY), "בעוד כחודש");
    }
}
