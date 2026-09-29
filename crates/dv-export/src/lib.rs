//! The Word report: a right-to-left DOCX, optionally password-protected.
//!
//! The caller passes the report with real names already restored (locally). This crate
//! adds no identifying metadata: no author, no company, no edit history, fixed timestamps,
//! and a document title without names. With a password, the package is encrypted with
//! ECMA-376 Agile Encryption (AES-256-CBC, SHA-512, 100,000 spins), the format Word itself
//! uses for "Encrypt with Password".

mod agile;
mod docx;

use serde::{Deserialize, Serialize};

pub use agile::{encrypt, MIN_PASSWORD_CHARS};
pub use docx::render;

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

/// Everything that goes into the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub title: String,
    pub info: Vec<InfoLine>,
    pub parts: Vec<ReportPart>,
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
