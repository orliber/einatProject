//! The Word report: a right-to-left DOCX, optionally password-protected.
//!
//! The caller passes the report with real names already restored (locally). This crate
//! adds no identifying metadata: no author, no company, no edit history, fixed timestamps,
//! and a document title without names. With a password, the package is encrypted with
//! ECMA-376 Agile Encryption (AES-256-CBC, SHA-512, 100,000 spins), the format Word itself
//! uses for "Encrypt with Password".

mod agile;
mod docx;
mod template;

use serde::{Deserialize, Serialize};

pub use agile::{encrypt, MIN_PASSWORD_CHARS};
pub use docx::render;
pub use template::{
    check_template, render_with_template, TemplateSummary, MARKER, MAX_TEMPLATE_BYTES,
};

/// One section and its approved paragraphs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSection {
    pub title: String,
    pub paragraphs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportPart {
    pub title: String,
    pub sections: Vec<ReportSection>,
}

/// A labeled line at the top ("שם הילד/ה", "גיל").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfoLine {
    pub label: String,
    pub value: String,
}

/// A score table for the appendix: header cells, then one row per measure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreTable {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// The scale sentence printed under the table.
    pub note: String,
    /// Score profiles drawn under the table (EX-2), computed from the scores in code.
    #[serde(default)]
    pub charts: Vec<ScoreChart>,
}

/// A horizontal bar profile on a fixed scale, drawn with table cells: it looks the same in
/// Word, LibreOffice and on paper, and needs no picture or drawing part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreChart {
    pub title: String,
    /// The axis: `min` up to `max`, one cell per `step`.
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// Cells within one standard deviation of the mean are tinted.
    pub mean: f64,
    pub sd: f64,
    pub bars: Vec<ChartBar>,
    /// The sentence printed under the chart.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartBar {
    pub label: String,
    pub value: f64,
}

/// Everything that goes into the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub title: String,
    pub info: Vec<InfoLine>,
    pub parts: Vec<ReportPart>,
    /// Score tables, printed as an appendix after the sections.
    pub tables: Vec<ScoreTable>,
    /// Signature block lines (name, title, license number).
    pub signature: Vec<String>,
    /// Printed at the top of every page.
    pub confidentiality: String,
    /// Body font (Hebrew and Latin).
    pub font: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("password too short")]
    WeakPassword,
    #[error("crypto")]
    Crypto,
    #[error("package: {0}")]
    Package(String),
    /// Her template cannot be used; the message says what to do, in Hebrew.
    #[error("{0}")]
    Template(String),
}

/// Defense in depth before writing a file: bracketed placeholders that must not reach the
/// family ("[ילד]", "[חסר: ...]"). The caller checks tags too; this runs on the final text.
#[must_use]
pub fn leftover_placeholders(report: &Report) -> Vec<String> {
    let mut texts: Vec<&str> = vec![&report.title];
    texts.extend(report.info.iter().map(|l| l.value.as_str()));
    texts.extend(report.signature.iter().map(String::as_str));
    for part in &report.parts {
        for s in &part.sections {
            texts.extend(s.paragraphs.iter().map(String::as_str));
        }
    }
    for t in &report.tables {
        texts.push(&t.title);
        texts.push(&t.note);
        texts.extend(t.rows.iter().flatten().map(String::as_str));
    }
    let mut found = Vec::new();
    for t in texts {
        let mut rest = t;
        while let Some(start) = rest.find('[') {
            let after = &rest[start + 1..];
            match after.find(']') {
                Some(end) if end <= 60 && !after[..end].contains('\n') => {
                    let item = format!("[{}]", &after[..end]);
                    if !found.contains(&item) {
                        found.push(item);
                    }
                    rest = &after[end + 1..];
                }
                _ => break,
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Report {
        Report {
            title: "דוח אבחון פסיכולוגי".into(),
            info: vec![
                InfoLine {
                    label: "שם הילד".into(),
                    value: "אלון בדוי".into(),
                },
                InfoLine {
                    label: "גיל".into(),
                    value: "5:4".into(),
                },
            ],
            parts: vec![ReportPart {
                title: "חלק א: רקע".into(),
                sections: vec![
                    ReportSection {
                        title: "סיבת הפניה".into(),
                        paragraphs: vec!["ההורים פנו בשל קושי במעברים ובוויסות רגשי.".into()],
                    },
                    ReportSection {
                        title: "כלי האבחון".into(),
                        paragraphs: vec![
                            "WISC-V (גרסה עברית): ציון מלא 102 (טווח ממוצע) & תצפית.".into()
                        ],
                    },
                ],
            }],
            tables: vec![ScoreTable {
                title: "WPPSI-IV".into(),
                columns: vec!["מדד".into(), "ציון".into(), "אחוזון".into(), "טווח".into()],
                rows: vec![vec![
                    "הבנה מילולית (VCI)".into(),
                    "112".into(),
                    "79".into(),
                    "ממוצע גבוה".into(),
                ]],
                note: "ציוני תקן: ממוצע 100, סטיית תקן 15.".into(),
                charts: Vec::new(),
            }],
            signature: vec!["ד\"ר בדויה, פסיכולוגית התפתחותית".into()],
            confidentiality: "חסוי – מידע רפואי".into(),
            font: "David".into(),
        }
    }

    #[test]
    fn finds_leftover_placeholders() {
        let mut r = sample();
        assert!(leftover_placeholders(&r).is_empty());
        r.parts[0].sections[0]
            .paragraphs
            .push("[ילד] מגיב [חסר: גיל ההליכה] ו-[ילד].".into());
        assert_eq!(
            leftover_placeholders(&r),
            vec!["[ילד]".to_owned(), "[חסר: גיל ההליכה]".to_owned()]
        );
    }
}
