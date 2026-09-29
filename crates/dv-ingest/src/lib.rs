//! Extracts text from untrusted documents (DOCX, PDF with a text layer, plain text).
//!
//! In the app this runs in a separate worker process (the same binary started with
//! [`worker::WORKER_ARG`]): it gets only the file's bytes, has no keys and no network, and is
//! killed after a time limit. A parser bug or a hostile file can at worst crash the worker.
//! Only text comes back, and only the body can become case material: headers, footers,
//! comments and file properties are shown locally and used to suggest names to hide.

mod docx;
mod pdf;
pub mod rtl;
pub mod worker;

use serde::{Deserialize, Serialize};

/// Largest file accepted (a 60-page scanned report is far smaller as text PDF).
pub const MAX_INPUT_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Docx,
    Pdf,
    Text,
}

/// What a document yielded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extracted {
    pub format: Format,
    /// The document body: the only part that can become case material.
    pub body: String,
    /// Headers and footers: shown locally, used to suggest names to hide, never sent.
    pub margins: String,
    /// File properties (author, company, title…): shown locally, never sent.
    pub metadata: Vec<(String, String)>,
    /// Things the psychologist should know ("comments were not imported").
    pub warnings: Vec<String>,
    pub pages: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum IngestError {
    #[error("file too large")]
    TooLarge,
    #[error("password protected")]
    Encrypted,
    #[error("legacy .doc")]
    LegacyDoc,
    #[error("no text layer")]
    Scanned,
    #[error("empty")]
    Empty,
    #[error("unsupported format")]
    Unsupported,
    #[error("corrupt: {0}")]
    Corrupt(String),
    #[error("timed out")]
    Timeout,
    #[error("worker: {0}")]
    Worker(String),
}

impl IngestError {
    /// What the psychologist sees.
    #[must_use]
    pub fn message_he(&self) -> String {
        match self {
            Self::TooLarge => "הקובץ גדול מדי (עד 25MB).".to_owned(),
            Self::Encrypted => "הקובץ מוגן בסיסמה. יש לשמור עותק בלי סיסמה ולייבא אותו.".to_owned(),
            Self::LegacyDoc => "זה קובץ Word ישן (‎.doc). יש לפתוח אותו ב-Word ולשמור בשם כ-‎.docx.".to_owned(),
            Self::Scanned => "לא נמצא טקסט בקובץ – כנראה מסמך סרוק (תמונה). כרגע אפשר לייבא PDF עם טקסט, DOCX או טקסט.".to_owned(),
            Self::Empty => "הקובץ ריק.".to_owned(),
            Self::Unsupported => "סוג הקובץ לא נתמך. אפשר לייבא DOCX, PDF או קובץ טקסט.".to_owned(),
            Self::Corrupt(_) => "לא הצלחתי לקרוא את הקובץ. ייתכן שהוא פגום.".to_owned(),
            Self::Timeout => "קריאת הקובץ לקחה יותר מדי זמן והופסקה.".to_owned(),
            Self::Worker(_) => "קריאת הקובץ נכשלה.".to_owned(),
        }
    }
}

/// Decide the format from the bytes (never from the extension alone).
pub fn detect(bytes: &[u8], file_name: &str) -> Result<Format, IngestError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(IngestError::TooLarge);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Ok(Format::Docx);
    }
    if bytes.starts_with(b"%PDF-") {
        return Ok(Format::Pdf);
    }
    // Compound File: a legacy .doc, or a password-protected .docx (which is stored as CFB).
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Err(if file_name.to_lowercase().ends_with(".doc") {
            IngestError::LegacyDoc
        } else {
            IngestError::Encrypted
        });
    }
    match decode_text(bytes, file_name) {
        Some((s, _)) if !s.contains('\0') => Ok(Format::Text),
        _ => Err(IngestError::Unsupported),
    }
}

/// UTF-8 (with or without BOM), or, for a `.txt` file only, the old Hebrew Windows encoding
/// (Windows-1255) that older Word exports and systems still produce. Any byte that has no
/// meaning in Windows-1255 refuses the file rather than guessing. The flag says it was 1255.
fn decode_text(bytes: &[u8], file_name: &str) -> Option<(String, bool)> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Some((s.to_owned(), false));
    }
    if !file_name.to_lowercase().ends_with(".txt")
        || !bytes.iter().any(|b| (0xE0..=0xFA).contains(b))
    {
        return None;
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let c = match b {
            0x00..=0x7F => char::from(b),
            0xE0..=0xFA => char::from_u32(0x05D0 + u32::from(b - 0xE0))?,
            0xC0..=0xD3 => char::from_u32(0x05B0 + u32::from(b - 0xC0))?,
            0xD4..=0xD8 => char::from_u32(0x05F0 + u32::from(b - 0xD4))?,
            0x80 => '€',
            0x85 => '…',
            0x91 => '‘',
            0x92 => '’',
            0x93 => '“',
            0x94 => '”',
            0x96 => '–',
            0x97 => '—',
            0xA0 => ' ',
            0xA4 => '₪',
            0xAA => '×',
            0xBA => '÷',
            0xA1..=0xBF => char::from(b),
            0xFD | 0xFE => continue, // direction marks
            _ => return None,
        };
        out.push(c);
    }
    Some((out, true))
}

/// Extract in this process. The app calls [`worker::run`] instead, which runs this in the worker.
pub fn extract(bytes: &[u8], file_name: &str) -> Result<Extracted, IngestError> {
    let mut out = match detect(bytes, file_name)? {
        Format::Docx => docx::extract(bytes)?,
        Format::Pdf => pdf::extract(bytes)?,
        Format::Text => {
            let (body, legacy) = decode_text(bytes, file_name).ok_or(IngestError::Unsupported)?;
            Extracted {
                format: Format::Text,
                body,
                margins: String::new(),
                metadata: Vec::new(),
                warnings: if legacy {
                    vec!["הקובץ נקרא בקידוד עברית ישן (Windows-1255). כדאי לעבור על הטקסט לפני השמירה.".to_owned()]
                } else {
                    Vec::new()
                },
                pages: 1,
            }
        }
    };
    let (body, removed) = clean(&out.body);
    out.body = body;
    out.margins = clean(&out.margins).0;
    if removed {
        out.warnings
            .push("הוסרו מהטקסט תווי כיווניות/תווים בלתי נראים.".to_owned());
    }
    if out.body.trim().is_empty() {
        return Err(if out.format == Format::Pdf {
            IngestError::Scanned
        } else {
            IngestError::Empty
        });
    }
    Ok(out)
}

/// Invisible characters that could split a name so the filter misses it ("אל\u{200F}ון"),
/// or reorder what the psychologist sees (bidi overrides). Removed before anything else.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Remove invisible characters, normalize line ends and spaces, collapse blank lines.
fn clean(text: &str) -> (String, bool) {
    let mut removed = false;
    let mut lines: Vec<String> = Vec::new();
    for raw in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let mut line = String::with_capacity(raw.len());
        for c in raw.chars() {
            if is_invisible(c) {
                removed = true;
            } else if c == '\u{00A0}' || c == '\u{2007}' || c == '\u{202F}' {
                line.push(' ');
            } else if c.is_control() && c != '\t' {
                removed = true;
            } else {
                line.push(c);
            }
        }
        lines.push(line.trim_end().to_owned());
    }
    let mut out = String::new();
    let mut blank = 0;
    for l in lines {
        if l.trim().is_empty() {
            blank += 1;
            if blank <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank = 0;
            out.push_str(&l);
            out.push('\n');
        }
    }
    (out.trim().to_owned(), removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_content() {
        assert_eq!(detect(b"PK\x03\x04rest", "x.pdf"), Ok(Format::Docx));
        assert_eq!(detect(b"%PDF-1.7", "a.docx"), Ok(Format::Pdf));
        assert_eq!(detect("שלום".as_bytes(), "a.txt"), Ok(Format::Text));
        // "שלום, נועם" in Windows-1255; a stray undefined byte refuses the file.
        let legacy = [0xF9, 0xEC, 0xE5, 0xED, b',', b' ', 0xF0, 0xE5, 0xF2, 0xED];
        let out = extract(&legacy, "old.txt").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.body, "שלום, נועם");
        assert!(out.warnings.iter().any(|w| w.contains("1255")));
        assert_eq!(detect(&legacy, "old.bin"), Err(IngestError::Unsupported));
        assert_eq!(
            detect(&[0xF9, 0xDB], "old.txt"),
            Err(IngestError::Unsupported)
        );
        let cfb = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0];
        assert_eq!(detect(&cfb, "old.DOC"), Err(IngestError::LegacyDoc));
        assert_eq!(detect(&cfb, "locked.docx"), Err(IngestError::Encrypted));
        assert_eq!(
            detect(&[0, 1, 2, 0xFF], "x.bin"),
            Err(IngestError::Unsupported)
        );
    }

    #[test]
    fn invisible_characters_are_removed() {
        let out = extract(
            "הילד אל\u{200F}ון\u{202E} הגיע\r\n\r\n\r\n\r\nשורה".as_bytes(),
            "a.txt",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.body, "הילד אלון הגיע\n\nשורה");
        assert_eq!(out.warnings.len(), 1);
    }

    #[test]
    fn too_large_is_refused() {
        let big = vec![b'a'; MAX_INPUT_BYTES + 1];
        assert_eq!(detect(&big, "a.txt"), Err(IngestError::TooLarge));
    }
}
