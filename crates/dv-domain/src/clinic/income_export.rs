//! The income diary for the accountant (D-194): the payments of a month or a year as a CSV
//! file that leaves the vault.
//!
//! The strictest of the options: the file holds no name, no phone, no id of a card and no
//! text she wrote. A payment row is its date, its receipt number (only the digits and the
//! separators of one: a word there could be a name), its amount, the method's label and
//! whether it stands or was voided; never who paid, why it was voided, or a note. The
//! functions here are given payments and the anonymous tally of erased cards (D-179) only,
//! never a card, so a name is not there to leak.
//!
//! The file is RFC 4180: UTF-8 with a byte order mark (Excel then reads the Hebrew right),
//! CRLF after every record, every record as wide as the widest, and a field with a comma, a
//! quote or a line break in quotes, its quotes doubled. A cell that starts with `=`, `+`, `-`,
//! `@`, a tab or a carriage return gets a leading apostrophe, so a spreadsheet never reads it
//! as a formula (CSV injection).
//!
//! Pure, like the rest of this crate: the caller reads the vault, writes the file and the log.

use super::logic::{method_label, month_of, parse_month, ERASED_LABEL, PAY_METHODS};
use super::{ErasedTally, PayMethod, Payment};
use crate::dates::{days_in_month, iso};
use crate::DomainError;

/// The first and the last year the clinic keeps (as [`super::validate_year`]).
const FIRST_YEAR: i32 = 2000;
const LAST_YEAR: i32 = 2200;

const MONTH_NAMES: [&str; 12] = [
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

/// Every record of the file is this many fields wide (RFC 4180: the same number in each).
const WIDTH: usize = 5;

/// What a cell must not start with: a spreadsheet would read it as a formula.
const FORMULA_START: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// What may stay of a receipt number: digits, and what separates their groups.
const RECEIPT_SEPARATORS: [char; 4] = ['-', '/', '.', ' '];

/// The stretch of time an income export covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomePeriod {
    /// One month: the year, and the month (1–12).
    Month(i32, u32),
    /// A whole year.
    Year(i32),
}

impl IncomePeriod {
    /// `2026-11` for a month, `2026` for a year: what the window sends, the file is named
    /// by and the log keeps.
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Month(y, m) => format!("{y:04}-{m:02}"),
            Self::Year(y) => format!("{y:04}"),
        }
    }

    /// Its months, `YYYY-MM`, the first first.
    #[must_use]
    pub fn months(self) -> Vec<String> {
        match self {
            Self::Month(y, m) => vec![format!("{y:04}-{m:02}")],
            Self::Year(y) => (1..=12).map(|m| format!("{y:04}-{m:02}")).collect(),
        }
    }

    /// Its first and its last day, ISO.
    #[must_use]
    pub fn bounds(self) -> (String, String) {
        match self {
            Self::Month(y, m) => (iso((y, m, 1)), iso((y, m, days_in_month(y, m)))),
            Self::Year(y) => (iso((y, 1, 1)), iso((y, 12, 31))),
        }
    }

    /// As she reads it: "נובמבר 2026", "שנת 2026".
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::Month(..) => month_title(&self.key()),
            Self::Year(y) => format!("שנת {y:04}"),
        }
    }

    /// The file's name: the period only, never a name (`הכנסות 2026-11.csv`).
    #[must_use]
    pub fn file_name(self) -> String {
        format!("הכנסות {}.csv", self.key())
    }
}

/// A month (`YYYY-MM`) or a year (`YYYY`) of the years the clinic keeps; anything else is
/// refused.
pub fn parse_income_period(text: &str) -> Result<IncomePeriod, DomainError> {
    let year = (text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse::<i32>().ok())
        .flatten();
    if let Some(y) = year.filter(|y| (FIRST_YEAR..=LAST_YEAR).contains(y)) {
        return Ok(IncomePeriod::Year(y));
    }
    match parse_month(text) {
        Some((y, m)) => Ok(IncomePeriod::Month(y, m)),
        None => Err(DomainError::Invalid(
            "צריך לבחור חודש או שנה לייצוא.".to_owned(),
        )),
    }
}

/// `2026-11` → "נובמבר 2026"; anything else as it is.
fn month_title(month: &str) -> String {
    let name = parse_month(month).and_then(|(y, m)| {
        let index = usize::try_from(m).ok()?.checked_sub(1)?;
        MONTH_NAMES.get(index).map(|name| format!("{name} {y:04}"))
    });
    name.unwrap_or_else(|| month.to_owned())
}

/// A file made for someone outside the vault: its name, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomeExport {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

// ------------------------------------------------------------------ CSV

/// One field as RFC 4180 writes it, after the formula guard: a cell that starts with what
/// a spreadsheet takes for a formula gets a leading apostrophe; a cell with a comma, a quote
/// or a line break is quoted, its quotes doubled.
#[must_use]
pub fn csv_cell(text: &str) -> String {
    let guarded = if text.starts_with(FORMULA_START) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if guarded.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// One record: the cells, as wide as every other record, and CRLF.
fn push_row(out: &mut String, cells: &[&str]) {
    let fields: Vec<String> = (0..WIDTH.max(cells.len()))
        .map(|i| cells.get(i).map_or_else(String::new, |c| csv_cell(c)))
        .collect();
    out.push_str(&fields.join(","));
    out.push_str("\r\n");
}

/// Agorot as shekels with two decimals and a dot: `450.00`, `1250.50`. No thousands
/// separator, so a spreadsheet reads it as a number.
#[must_use]
pub fn shekels(agorot: i64) -> String {
    let sign = if agorot < 0 { "-" } else { "" };
    let abs = agorot.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// What the file shows of a receipt number: its digits and the separators between them,
/// nothing else (a letter there could be part of a name). Empty when no digit is left.
#[must_use]
pub fn receipt_number(receipt: &str) -> String {
    let kept: String = receipt
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| c.is_ascii_digit() || RECEIPT_SEPARATORS.contains(c))
        .collect();
    let kept = kept
        .split(' ')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let kept = kept.trim_matches(RECEIPT_SEPARATORS);
    if kept.chars().any(|c| c.is_ascii_digit()) {
        kept.to_owned()
    } else {
        String::new()
    }
}

/// The methods in the order the screens list them, then one of a later version (D-178),
/// so the methods still add up.
fn methods() -> impl Iterator<Item = PayMethod> {
    PAY_METHODS
        .into_iter()
        .chain(std::iter::once(PayMethod::Unknown))
}

fn method_place(method: PayMethod) -> usize {
    methods()
        .position(|m| m == method)
        .unwrap_or(PAY_METHODS.len())
}

/// One payment as the file has it: nothing of who, and nothing she wrote.
struct Row {
    date: String,
    month: String,
    receipt: String,
    amount: i64,
    method: PayMethod,
    voided: bool,
}

/// What a month (or the period) came to by one method: from the listed payments, and from
/// the cards that were erased.
#[derive(Default, Clone, Copy)]
struct Sum {
    listed: i64,
    erased: i64,
}

impl Sum {
    fn add(&mut self, other: Sum) {
        self.listed = self.listed.saturating_add(other.listed);
        self.erased = self.erased.saturating_add(other.erased);
    }

    fn total(self) -> i64 {
        self.listed.saturating_add(self.erased)
    }

    fn is_empty(self) -> bool {
        self.listed == 0 && self.erased == 0
    }
}

/// The sums of one month, by method (in the order of [`methods`]).
fn month_sums(rows: &[Row], tally: &ErasedTally, month: &str) -> Vec<(PayMethod, Sum)> {
    let erased = tally.months.get(month);
    methods()
        .map(|method| {
            let listed = rows
                .iter()
                .filter(|r| !r.voided && r.month == month && r.method == method)
                .fold(0_i64, |sum, r| sum.saturating_add(r.amount));
            let erased = erased.map_or(0, |e| {
                e.received
                    .iter()
                    .filter(|entry| entry.method == method)
                    .fold(0_i64, |sum, entry| sum.saturating_add(entry.amount))
            });
            (method, Sum { listed, erased })
        })
        .collect()
}

/// The lines of one stretch (a month, or the whole year): a line for each method that has
/// anything, then the stretch's total, always.
fn push_sums(out: &mut String, title: &str, total_label: &str, sums: &[(PayMethod, Sum)]) {
    let mut all = Sum::default();
    for (method, sum) in sums.iter().filter(|(_, sum)| !sum.is_empty()) {
        all.add(*sum);
        push_row(
            out,
            &[
                title,
                method_label(*method),
                &shekels(sum.listed),
                &shekels(sum.erased),
                &shekels(sum.total()),
            ],
        );
    }
    push_row(
        out,
        &[
            title,
            total_label,
            &shekels(all.listed),
            &shekels(all.erased),
            &shekels(all.total()),
        ],
    );
}

fn count_text(n: usize) -> String {
    n.to_string()
}

/// The income diary of `period` for the accountant, as the bytes of a CSV file (D-194):
/// every payment of the period (date, receipt number, amount, method, standing or voided),
/// then what each month came to by method, with what the cards that were erased counted for
/// in it as totals only (D-179), then how many payments have no receipt number. A voided
/// payment is listed and counted in no total, and nothing says why it was voided. Payments
/// outside the period are left out.
#[must_use]
pub fn income_csv(period: IncomePeriod, payments: &[Payment], tally: &ErasedTally) -> Vec<u8> {
    let months = period.months();
    let mut rows: Vec<Row> = payments
        .iter()
        .filter_map(|p| {
            let month = month_of(&p.date).filter(|m| months.contains(m))?;
            Some(Row {
                date: p.date.clone(),
                month,
                receipt: receipt_number(&p.receipt),
                amount: i64::from(p.amount),
                method: p.method,
                voided: p.voided,
            })
        })
        .collect();
    // By date, then by receipt number (numbers of one length in order, the ones without at
    // the end); what is left only keeps the order the same every time.
    rows.sort_by_key(|r| {
        (
            r.date.clone(),
            r.receipt.is_empty(),
            r.receipt.len(),
            r.receipt.clone(),
            r.amount,
            method_place(r.method),
            r.voided,
        )
    });
    let erased_anything = months.iter().any(|m| {
        tally.months.get(m).is_some_and(|e| {
            e.received.iter().any(|entry| entry.amount != 0) || e.missing_receipts > 0
        })
    });

    let title = period.title();
    let mut out = String::from('\u{feff}');
    push_row(&mut out, &["יומן הכנסות לרואה החשבון", &title]);
    push_row(
        &mut out,
        &["בקובץ אין שמות, טלפונים, מזהים של מטופלים או הערות."],
    );
    push_row(
        &mut out,
        &["התוכנה אינה רשומה ברשות המסים כתוכנה לניהול 'יומן רופא'. את הקבלות מפיקים בתוכנת החשבוניות."],
    );
    push_row(
        &mut out,
        &["תקבול שבוטל מופיע ברשימה עם הסכום שלו, ולא נספר בסיכומים."],
    );
    if erased_anything {
        push_row(
            &mut out,
            &[&format!(
                "{ERASED_LABEL}: סכומים בלבד, בלי פירוט, ולכן הם רק בסיכום שלמטה."
            )],
        );
    }
    push_row(&mut out, &[]);

    push_row(&mut out, &["תקבולים"]);
    push_row(
        &mut out,
        &["תאריך", "מספר קבלה", "סכום (₪)", "אמצעי תשלום", "מצב"],
    );
    if rows.is_empty() {
        push_row(&mut out, &["אין תקבולים בתקופה הזו."]);
    }
    for row in &rows {
        push_row(
            &mut out,
            &[
                &row.date,
                &row.receipt,
                &shekels(row.amount),
                method_label(row.method),
                if row.voided { "בוטל" } else { "תקין" },
            ],
        );
    }
    push_row(&mut out, &[]);

    push_row(&mut out, &["סיכום לפי חודש ואמצעי תשלום"]);
    let erased_header = format!("{ERASED_LABEL} (₪)");
    push_row(
        &mut out,
        &[
            "חודש",
            "אמצעי תשלום",
            "תקבולים ברשימה (₪)",
            &erased_header,
            "סך הכל (₪)",
        ],
    );
    let mut whole: Vec<(PayMethod, Sum)> = methods().map(|m| (m, Sum::default())).collect();
    for month in &months {
        let sums = month_sums(&rows, tally, month);
        for ((_, all), (_, sum)) in whole.iter_mut().zip(&sums) {
            all.add(*sum);
        }
        push_sums(&mut out, &month_title(month), "סך החודש", &sums);
    }
    if matches!(period, IncomePeriod::Year(_)) {
        push_sums(&mut out, &title, "סך השנה", &whole);
    }
    push_row(&mut out, &[]);

    let missing = rows
        .iter()
        .filter(|r| !r.voided && r.receipt.is_empty())
        .count();
    let erased_missing = months
        .iter()
        .filter_map(|m| tally.months.get(m))
        .fold(0_u32, |sum, e| sum.saturating_add(e.missing_receipts));
    let voided = rows.iter().filter(|r| r.voided).count();
    push_row(
        &mut out,
        &["תקבולים בלי מספר קבלה ברשימה", &count_text(missing)],
    );
    push_row(
        &mut out,
        &[
            &format!("תקבולים בלי מספר קבלה ב{ERASED_LABEL}"),
            &erased_missing.to_string(),
        ],
    );
    push_row(&mut out, &["תקבולים שבוטלו", &count_text(voided)]);
    out.into_bytes()
}

/// The file of `period`: its name, and its bytes ([`income_csv`]).
#[must_use]
pub fn income_export(
    period: IncomePeriod,
    payments: &[Payment],
    tally: &ErasedTally,
) -> IncomeExport {
    IncomeExport {
        file_name: period.file_name(),
        bytes: income_csv(period, payments, tally),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::clinic::{ErasedMethod, ErasedMonth, KindCount, VisitKind};

    /// Reads a CSV the way RFC 4180 does: the records, each a list of fields. Panics on
    /// anything RFC 4180 does not allow (a quote inside an unquoted field, a bare LF, text
    /// after a closing quote, a last record without CRLF).
    fn parse(text: &str) -> Vec<Vec<String>> {
        let mut records = Vec::new();
        let mut record = Vec::new();
        let mut field = String::new();
        let mut chars = text.chars().peekable();
        let mut quoted = false;
        let mut was_quoted = false;
        while let Some(c) = chars.next() {
            if quoted {
                match c {
                    '"' if chars.peek() == Some(&'"') => {
                        chars.next();
                        field.push('"');
                    }
                    '"' => quoted = false,
                    other => field.push(other),
                }
                continue;
            }
            match c {
                '"' => {
                    assert!(field.is_empty() && !was_quoted, "a quote inside a field");
                    quoted = true;
                    was_quoted = true;
                }
                ',' => {
                    record.push(std::mem::take(&mut field));
                    was_quoted = false;
                }
                '\r' => {
                    assert_eq!(chars.next(), Some('\n'), "CR without LF");
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                    was_quoted = false;
                }
                '\n' => panic!("a bare LF"),
                other => {
                    assert!(!was_quoted, "text after a closing quote");
                    field.push(other);
                }
            }
        }
        assert!(!quoted, "an open quote at the end");
        assert!(
            field.is_empty() && record.is_empty(),
            "the last record has no CRLF"
        );
        records
    }

    fn text_of(bytes: &[u8]) -> String {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        text.strip_prefix('\u{feff}').unwrap().to_owned()
    }

    fn records(bytes: &[u8]) -> Vec<Vec<String>> {
        parse(&text_of(bytes))
    }

    /// The records whose first field is `first`.
    fn starting<'a>(records: &'a [Vec<String>], first: &str) -> Vec<&'a Vec<String>> {
        records.iter().filter(|r| r[0] == first).collect()
    }

    fn payment(date: &str, amount: i32, method: PayMethod, receipt: &str) -> Payment {
        Payment {
            id: format!("payment-{date}-{amount}"),
            patient_id: "patient-1".to_owned(),
            date: date.to_owned(),
            amount,
            method,
            receipt: receipt.to_owned(),
            ..Payment::default()
        }
    }

    const NOVEMBER: IncomePeriod = IncomePeriod::Month(2026, 11);

    // -------------------------------------------------------------- the period

    #[test]
    fn a_period_is_a_month_or_a_year_of_the_years_the_clinic_keeps() {
        assert_eq!(parse_income_period("2026-11").unwrap(), NOVEMBER);
        assert_eq!(
            parse_income_period("2026").unwrap(),
            IncomePeriod::Year(2026)
        );
        assert_eq!(
            parse_income_period("2000-01").unwrap(),
            IncomePeriod::Month(2000, 1)
        );
        for wrong in [
            "",
            "2026-13",
            "2026-1",
            "1999",
            "2201",
            "2201-01",
            " 2026",
            "+202",
            "2026-11-01",
            "שנה",
            "20261",
            "2026-00",
        ] {
            let Err(DomainError::Invalid(text)) = parse_income_period(wrong) else {
                panic!("{wrong:?} was taken");
            };
            assert_eq!(text, "צריך לבחור חודש או שנה לייצוא.");
        }
    }

    #[test]
    fn the_file_is_named_by_the_period_only() {
        assert_eq!(NOVEMBER.file_name(), "הכנסות 2026-11.csv");
        assert_eq!(IncomePeriod::Year(2026).file_name(), "הכנסות 2026.csv");
        assert_eq!(NOVEMBER.key(), "2026-11");
        assert_eq!(NOVEMBER.title(), "נובמבר 2026");
        assert_eq!(IncomePeriod::Year(2026).title(), "שנת 2026");
        assert_eq!(
            NOVEMBER.bounds(),
            ("2026-11-01".to_owned(), "2026-11-30".to_owned())
        );
        assert_eq!(
            IncomePeriod::Month(2028, 2).bounds(),
            ("2028-02-01".to_owned(), "2028-02-29".to_owned())
        );
        assert_eq!(
            IncomePeriod::Year(2026).bounds(),
            ("2026-01-01".to_owned(), "2026-12-31".to_owned())
        );
        assert_eq!(IncomePeriod::Year(2026).months().len(), 12);
        assert_eq!(NOVEMBER.months(), ["2026-11"]);
        let export = income_export(NOVEMBER, &[], &ErasedTally::default());
        assert_eq!(export.file_name, "הכנסות 2026-11.csv");
        assert_eq!(
            export.bytes,
            income_csv(NOVEMBER, &[], &ErasedTally::default())
        );
    }

    // -------------------------------------------------------------- the format

    #[test]
    fn utf8_with_a_bom_crlf_and_every_record_as_wide() {
        let bytes = income_csv(
            NOVEMBER,
            &[payment("2026-11-05", 45_000, PayMethod::Cash, "1043")],
            &ErasedTally::default(),
        );
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = text_of(&bytes);
        assert!(text.ends_with("\r\n"));
        // Every LF follows a CR, and the other way round.
        assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
        assert_eq!(text.matches('\r').count(), text.matches("\r\n").count());
        let records = records(&bytes);
        assert!(records.len() > 10);
        assert!(records.iter().all(|r| r.len() == WIDTH), "{records:?}");
        // The Hebrew reads as Hebrew.
        assert_eq!(records[0][0], "יומן הכנסות לרואה החשבון");
        assert_eq!(records[0][1], "נובמבר 2026");
    }

    #[test]
    fn fields_are_quoted_as_rfc_4180_says() {
        assert_eq!(csv_cell("1043"), "1043");
        assert_eq!(csv_cell("מזומן"), "מזומן");
        assert_eq!(csv_cell("א,ב"), "\"א,ב\"");
        assert_eq!(csv_cell("צ'ק"), "צ'ק");
        assert_eq!(csv_cell("אמר \"שלום\""), "\"אמר \"\"שלום\"\"\"");
        assert_eq!(csv_cell("שורה\nשנייה"), "\"שורה\nשנייה\"");
        assert_eq!(csv_cell("שורה\r\nשנייה"), "\"שורה\r\nשנייה\"");
        assert_eq!(csv_cell(""), "");
        let mut out = String::new();
        push_row(&mut out, &["א,ב", "ג\"ד", "ה"]);
        assert_eq!(out, "\"א,ב\",\"ג\"\"ד\",ה,,\r\n");
        assert_eq!(
            parse(&out),
            [["א,ב", "ג\"ד", "ה", "", ""].map(str::to_owned).to_vec()]
        );
        // A sentence of the file's own with a comma is quoted too.
        let text = text_of(&income_csv(NOVEMBER, &[], &ErasedTally::default()));
        assert!(
            text.contains("\"בקובץ אין שמות, טלפונים, מזהים של מטופלים או הערות.\""),
            "{text}"
        );
    }

    #[test]
    fn a_cell_a_spreadsheet_would_run_as_a_formula_gets_an_apostrophe() {
        assert_eq!(csv_cell("=1+1"), "'=1+1");
        assert_eq!(csv_cell("+972"), "'+972");
        assert_eq!(csv_cell("-5"), "'-5");
        assert_eq!(csv_cell("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(csv_cell("\tא"), "'\tא");
        assert_eq!(csv_cell("\rא"), "\"'\rא\"");
        assert_eq!(
            csv_cell("=HYPERLINK(\"x\",\"y\")"),
            "\"'=HYPERLINK(\"\"x\"\",\"\"y\"\")\""
        );
        // Only at the start.
        assert_eq!(csv_cell("1-2"), "1-2");
        assert_eq!(csv_cell("a=b"), "a=b");
        // A receipt that was typed as a formula reaches the file as digits, guarded anyway.
        let bytes = income_csv(
            NOVEMBER,
            &[
                payment("2026-11-02", 100, PayMethod::Cash, "=1+2"),
                payment("2026-11-03", 100, PayMethod::Cash, "-1043"),
                payment("2026-11-04", 100, PayMethod::Cash, "@1044"),
            ],
            &ErasedTally::default(),
        );
        let records = records(&bytes);
        let receipts: Vec<&str> = ["2026-11-02", "2026-11-03", "2026-11-04"]
            .iter()
            .map(|d| starting(&records, d)[0][1].as_str())
            .collect();
        assert_eq!(receipts, ["12", "1043", "1044"]);
        let all: Vec<&String> = records.iter().flatten().collect();
        assert!(
            all.iter().all(|cell| !cell.starts_with(FORMULA_START)),
            "{all:?}"
        );
    }

    #[test]
    fn money_is_shekels_with_two_decimals() {
        assert_eq!(shekels(0), "0.00");
        assert_eq!(shekels(5), "0.05");
        assert_eq!(shekels(45_000), "450.00");
        assert_eq!(shekels(125_050), "1250.50");
        assert_eq!(shekels(10_000_000), "100000.00");
        assert_eq!(shekels(-150), "-1.50");
    }

    #[test]
    fn a_receipt_keeps_only_its_number() {
        assert_eq!(receipt_number("1043"), "1043");
        assert_eq!(receipt_number(" 1043 "), "1043");
        assert_eq!(receipt_number("1043/26"), "1043/26");
        assert_eq!(receipt_number("2026-1043"), "2026-1043");
        assert_eq!(receipt_number("10  43"), "10 43");
        assert_eq!(receipt_number("קבלה 1043"), "1043");
        assert_eq!(receipt_number("ק-1043"), "1043");
        assert_eq!(receipt_number("INV-1043"), "1043");
        assert_eq!(receipt_number("1043 נועם"), "1043");
        // Nothing of a number: as good as none.
        assert_eq!(receipt_number("נועם לוי"), "");
        assert_eq!(receipt_number("--/"), "");
        assert_eq!(receipt_number(""), "");
        // Digits of another script are not taken for a receipt number.
        assert_eq!(receipt_number("١٢٣"), "");
    }

    // -------------------------------------------------------------- what is in it

    /// Every payment of a month, with a name wherever a name can be, and a tally.
    fn named_month() -> (Vec<Payment>, ErasedTally) {
        let named = |p: Payment| Payment {
            patient_id: "נועם לוי".to_owned(),
            payer: "סבתא רבקה".to_owned(),
            note: "שולם על ידי דנה בפגישה".to_owned(),
            ..p
        };
        let payments = vec![
            named(payment("2026-11-05", 45_000, PayMethod::Cash, "")),
            named(payment(
                "2026-11-03",
                35_000,
                PayMethod::Paybox,
                "קבלה 1044 נועם",
            )),
            named(payment("2026-11-03", 20_000, PayMethod::Bit, "1043")),
            named(Payment {
                voided: true,
                void_reason: "דנה ביקשה לבטל, נרשם פעמיים".to_owned(),
                ..payment("2026-11-04", 99_900, PayMethod::Transfer, "1045")
            }),
            // Another month: left out.
            named(payment("2026-10-30", 12_300, PayMethod::Cash, "1001")),
        ];
        let mut tally = ErasedTally::default();
        tally.months.insert(
            "2026-11".to_owned(),
            ErasedMonth {
                held: vec![KindCount {
                    kind: VisitKind::Assessment,
                    count: 2,
                }],
                cancelled: 1,
                no_show: 0,
                charged: 80_000,
                received: vec![
                    ErasedMethod {
                        method: PayMethod::Cash,
                        amount: 40_000,
                    },
                    ErasedMethod {
                        method: PayMethod::Check,
                        amount: 15_000,
                    },
                ],
                missing_receipts: 1,
            },
        );
        tally.months.insert(
            "2026-03".to_owned(),
            ErasedMonth {
                received: vec![ErasedMethod {
                    method: PayMethod::Transfer,
                    amount: 150_000,
                }],
                ..ErasedMonth::default()
            },
        );
        (payments, tally)
    }

    #[test]
    fn no_name_no_note_no_reason_and_no_id_even_when_the_data_has_them() {
        let (payments, tally) = named_month();
        for period in [NOVEMBER, IncomePeriod::Year(2026)] {
            let text = text_of(&income_csv(period, &payments, &tally));
            for word in [
                "נועם",
                "לוי",
                "רבקה",
                "סבתא",
                "דנה",
                "ביקשה",
                "פעמיים",
                "קבלה 1044",
                "patient",
                "payment-",
            ] {
                assert!(!text.contains(word), "{word} in {text}");
            }
        }
    }

    #[test]
    fn every_payment_of_the_month_by_date_and_receipt_with_its_status() {
        let (payments, tally) = named_month();
        let records = records(&income_csv(NOVEMBER, &payments, &tally));
        let header = records
            .iter()
            .position(|r| r[0] == "תאריך")
            .expect("the header of the payments");
        assert_eq!(
            records[header],
            ["תאריך", "מספר קבלה", "סכום (₪)", "אמצעי תשלום", "מצב"]
        );
        let listed: Vec<Vec<&str>> = records[header + 1..header + 5]
            .iter()
            .map(|r| r.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            listed,
            [
                vec!["2026-11-03", "1043", "200.00", "ביט", "תקין"],
                vec!["2026-11-03", "1044", "350.00", "פייבוקס", "תקין"],
                vec!["2026-11-04", "1045", "999.00", "העברה בנקאית", "בוטל"],
                vec!["2026-11-05", "", "450.00", "מזומן", "תקין"],
            ]
        );
        // October's payment is not in November's file.
        assert!(starting(&records, "2026-10-30").is_empty());
        assert_eq!(records[header + 5], ["", "", "", "", ""]);
    }

    #[test]
    fn the_month_by_method_with_the_erased_cards_as_totals_and_no_voided_amount() {
        let (payments, tally) = named_month();
        let records = records(&income_csv(NOVEMBER, &payments, &tally));
        let header = records.iter().position(|r| r[0] == "חודש").unwrap();
        assert_eq!(
            records[header],
            [
                "חודש",
                "אמצעי תשלום",
                "תקבולים ברשימה (₪)",
                "רשומות של כרטיסים שנמחקו (₪)",
                "סך הכל (₪)"
            ]
        );
        let sums: Vec<Vec<&str>> = starting(&records, "נובמבר 2026")
            .into_iter()
            .map(|r| r.iter().skip(1).map(String::as_str).collect())
            .collect();
        assert_eq!(
            sums,
            [
                vec!["מזומן", "450.00", "400.00", "850.00"],
                vec!["ביט", "200.00", "0.00", "200.00"],
                vec!["פייבוקס", "350.00", "0.00", "350.00"],
                vec!["צ'ק", "0.00", "150.00", "150.00"],
                // The voided transfer is in no line.
                vec!["סך החודש", "1000.00", "550.00", "1550.00"],
            ]
        );
        assert!(!starting(&records, "תקבולים בלי מספר קבלה ברשימה").is_empty());
        let count = |first: &str| starting(&records, first)[0][1].clone();
        assert_eq!(count("תקבולים בלי מספר קבלה ברשימה"), "1");
        assert_eq!(
            count("תקבולים בלי מספר קבלה ברשומות של כרטיסים שנמחקו"),
            "1"
        );
        assert_eq!(count("תקבולים שבוטלו"), "1");
        // The erased cards are said to be totals only; their visits and charges are not
        // income, and are not in the file.
        let text = text_of(&income_csv(NOVEMBER, &payments, &tally));
        assert!(text.contains("רשומות של כרטיסים שנמחקו: סכומים בלבד"));
        assert!(!text.contains("800.00"), "the erased charges: {text}");
    }

    #[test]
    fn a_year_has_every_month_its_total_and_the_year_by_method() {
        let (payments, tally) = named_month();
        let records = records(&income_csv(IncomePeriod::Year(2026), &payments, &tally));
        let totals: Vec<(&str, &str)> = records
            .iter()
            .filter(|r| r[1] == "סך החודש")
            .map(|r| (r[0].as_str(), r[4].as_str()))
            .collect();
        assert_eq!(totals.len(), 12);
        assert_eq!(totals[0], ("ינואר 2026", "0.00"));
        assert_eq!(totals[2], ("מרץ 2026", "1500.00"));
        assert_eq!(totals[9], ("אוקטובר 2026", "123.00"));
        assert_eq!(totals[10], ("נובמבר 2026", "1550.00"));
        let year: Vec<Vec<&str>> = starting(&records, "שנת 2026")
            .into_iter()
            .map(|r| r.iter().skip(1).map(String::as_str).collect())
            .collect();
        assert_eq!(
            year,
            [
                vec!["מזומן", "573.00", "400.00", "973.00"],
                vec!["העברה בנקאית", "0.00", "1500.00", "1500.00"],
                vec!["ביט", "200.00", "0.00", "200.00"],
                vec!["פייבוקס", "350.00", "0.00", "350.00"],
                vec!["צ'ק", "0.00", "150.00", "150.00"],
                vec!["סך השנה", "1123.00", "2050.00", "3173.00"],
            ]
        );
        // Five payments, October's among them.
        assert_eq!(starting(&records, "2026-10-30").len(), 1);
        assert_eq!(records[0][1], "שנת 2026");
    }

    #[test]
    fn an_empty_period_says_so_and_still_adds_up() {
        let records = records(&income_csv(NOVEMBER, &[], &ErasedTally::default()));
        assert_eq!(starting(&records, "אין תקבולים בתקופה הזו.").len(), 1);
        let month = starting(&records, "נובמבר 2026");
        assert_eq!(month.len(), 1);
        assert_eq!(month[0][1..], ["סך החודש", "0.00", "0.00", "0.00"]);
        assert_eq!(
            starting(&records, "תקבולים בלי מספר קבלה ברשימה")[0][1],
            "0"
        );
        let text = text_of(&income_csv(NOVEMBER, &[], &ErasedTally::default()));
        assert!(!text.contains("סכומים בלבד"));
    }

    #[test]
    fn a_method_of_a_later_version_is_a_line_of_its_own() {
        let bytes = income_csv(
            NOVEMBER,
            &[payment("2026-11-05", 10_000, PayMethod::Unknown, "7")],
            &ErasedTally::default(),
        );
        let records = records(&bytes);
        assert_eq!(starting(&records, "2026-11-05")[0][3], "אמצעי לא מוכר");
        let month = starting(&records, "נובמבר 2026");
        assert_eq!(month[0][1..], ["אמצעי לא מוכר", "100.00", "0.00", "100.00"]);
        assert_eq!(month[1][1..], ["סך החודש", "100.00", "0.00", "100.00"]);
    }

    proptest! {
        /// Whatever a cell holds, the file reads back to it (behind the apostrophe of the
        /// formula guard), and no cell starts as a formula.
        #[test]
        fn any_cell_reads_back(cells in proptest::collection::vec(".{0,12}", 1..6)) {
            let mut out = String::new();
            let refs: Vec<&str> = cells.iter().map(String::as_str).collect();
            push_row(&mut out, &refs);
            let read = parse(&out);
            prop_assert_eq!(read.len(), 1);
            let row = &read[0];
            prop_assert_eq!(row.len(), WIDTH.max(cells.len()));
            for (cell, back) in cells.iter().zip(row) {
                let guarded = cell.starts_with(FORMULA_START);
                if guarded {
                    prop_assert_eq!(back, &format!("'{cell}"));
                } else {
                    prop_assert_eq!(back, cell);
                }
            }
        }

        /// A receipt reaches the file as digits and separators only.
        #[test]
        fn a_receipt_is_never_more_than_a_number(receipt in ".{0,30}") {
            let kept = receipt_number(&receipt);
            prop_assert!(kept.chars().all(|c| c.is_ascii_digit() || RECEIPT_SEPARATORS.contains(&c)));
            prop_assert!(kept.is_empty() || kept.chars().any(|c| c.is_ascii_digit()));
            prop_assert!(!kept.starts_with(FORMULA_START));
        }
    }
}
