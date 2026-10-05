//! Extracts text from untrusted documents: DOCX, ODT, RTF, old Word (.doc), PDF, plain text,
//! and, through local OCR, scanned PDFs and photos of a page (JPEG, PNG, TIFF).
//!
//! In the app this runs in a separate worker process (the same binary started with
//! [`worker::WORKER_ARG`]): it gets only the file's bytes, has no keys and no network, and is
//! killed after a time limit. A parser bug or a hostile file can at worst crash the worker.
//! Only text comes back, and only the body can become case material: headers, footers,
//! comments and file properties are shown locally and used to suggest names to hide.
//!
//! A scan or photo comes back from the worker as page images ([`Outcome::Scan`]); the app then
//! reads them with the bundled OCR engine ([`ocr`]), itself isolated, and the text joins the
//! same path as any other document ([`import`] does both steps).

mod codepage;
mod doc;
mod docx;
pub mod ocr;
mod odt;
mod pdf;
mod rtf;
pub mod rtl;
pub mod scan;
pub mod worker;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use scan::PageImage;

/// Largest file accepted (a 60-page scanned report is far smaller as text PDF).
pub const MAX_INPUT_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Docx,
    Odt,
    Pdf,
    Text,
    Rtf,
    /// Word 97–2003.
    Doc,
    /// A photo or scan saved as an image (JPEG, PNG, TIFF).
    Image,
}

impl Format {
    /// The name the app shows and stores (`docx`, `pdf`, `image`…).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Odt => "odt",
            Self::Pdf => "pdf",
            Self::Text => "text",
            Self::Rtf => "rtf",
            Self::Doc => "doc",
            Self::Image => "image",
        }
    }
}

/// What the worker found: text, or page images for OCR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Text(Extracted),
    Scan(Scan),
}

/// A scan or photo: its page images, for the OCR engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scan {
    pub format: Format,
    pub images: Vec<PageImage>,
    pub metadata: Vec<(String, String)>,
    pub warnings: Vec<String>,
    pub pages: u32,
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
    /// The text was read from a scan or photo by OCR: a name may be misread by a letter.
    #[serde(default)]
    pub ocr: bool,
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
    #[error("HEIC/HEIF photo")]
    Heif,
    #[error("too many scanned pages")]
    TooManyPages,
    #[error("OCR engine not installed")]
    OcrMissing,
    #[error("OCR: {0}")]
    Ocr(String),
}

impl IngestError {
    /// What the psychologist sees.
    #[must_use]
    pub fn message_he(&self) -> String {
        match self {
            Self::TooLarge => "הקובץ גדול מדי (עד 25MB).".to_owned(),
            Self::Encrypted => "הקובץ מוגן בסיסמה. יש לשמור עותק בלי סיסמה ולייבא אותו.".to_owned(),
            Self::LegacyDoc => "זה קובץ Word ישן מאוד (Word 95 או לפני). יש לפתוח אותו ב-Word ולשמור בשם כ-‎.docx.".to_owned(),
            Self::Scanned => "לא נמצא בקובץ טקסט שאפשר לקרוא, גם לא בזיהוי טקסט מסריקה. אם זו סריקה, כדאי לסרוק שוב בבירור (300 dpi, דף ישר).".to_owned(),
            Self::Empty => "הקובץ ריק.".to_owned(),
            Self::Unsupported => "סוג הקובץ לא נתמך. אפשר לייבא Word (DOCX או DOC), ODT, RTF, PDF, קובץ טקסט, או תמונה של דף (JPG, PNG, TIFF).".to_owned(),
            Self::Corrupt(_) => "לא הצלחתי לקרוא את הקובץ. ייתכן שהוא פגום.".to_owned(),
            Self::Timeout => "קריאת הקובץ לקחה יותר מדי זמן והופסקה.".to_owned(),
            Self::Worker(_) => "קריאת הקובץ נכשלה.".to_owned(),
            Self::Heif => "תמונת HEIC (מהאייפון) לא נתמכת. אפשר לשלוח אותה לעצמך כ-JPG, או לשנות במצלמה: הגדרות ← מצלמה ← תבניות ← \"הכי תואם\".".to_owned(),
            Self::TooManyPages => format!("הסריקה ארוכה מדי (עד {} עמודים). אפשר לפצל אותה לכמה קבצים.", scan::MAX_PAGES),
            Self::OcrMissing => "זה מסמך סרוק, ורכיב זיהוי הטקסט לא מותקן במחשב הזה. התקנה מחדש של כספת האבחון תוסיף אותו.".to_owned(),
            Self::Ocr(_) => "זיהוי הטקסט בסריקה נכשל.".to_owned(),
        }
    }
}

/// Decide the format from the bytes (never from the extension alone).
pub fn detect(bytes: &[u8], file_name: &str) -> Result<Format, IngestError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(IngestError::TooLarge);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Ok(if odt::is_odt(bytes) {
            Format::Odt
        } else {
            Format::Docx
        });
    }
    if bytes.starts_with(b"%PDF-") {
        return Ok(Format::Pdf);
    }
    // Compound File: an old Word file, or a password-protected .docx (stored as one).
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return match doc::kind(bytes)? {
            doc::Kind::Word => Ok(Format::Doc),
            doc::Kind::EncryptedPackage => Err(IngestError::Encrypted),
            doc::Kind::Other => Err(IngestError::Unsupported),
        };
    }
    if bytes.starts_with(b"{\\rtf") {
        return Ok(Format::Rtf);
    }
    if let Some(kind) = scan::image_kind(bytes) {
        return if kind == scan::ImageKind::Heif {
            Err(IngestError::Heif)
        } else {
            Ok(Format::Image)
        };
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
    let out: Option<String> = bytes.iter().map(|b| codepage::cp1255(*b)).collect();
    Some((out?, true))
}

/// Read a document and finish it: in this process, or (the app) through the worker, and with
/// OCR when it is a scan or photo. `ocr` is `None` when the engine is not installed: a scan is
/// then refused with a message that says so.
pub fn import(
    worker_exe: Option<&Path>,
    ocr: Option<&ocr::Engine>,
    file_name: &str,
    bytes: &[u8],
) -> Result<Extracted, IngestError> {
    let outcome = match worker_exe {
        Some(exe) => worker::run(exe, file_name, bytes, worker::DEFAULT_TIMEOUT)?,
        None => read(bytes, file_name)?,
    };
    match outcome {
        Outcome::Text(out) => Ok(out),
        Outcome::Scan(scan) => {
            let engine = ocr.ok_or(IngestError::OcrMissing)?;
            finish(ocr::recognize(engine, scan)?)
        }
    }
}

/// Text documents only, in this process: a scan or photo is refused (no OCR here).
pub fn extract(bytes: &[u8], file_name: &str) -> Result<Extracted, IngestError> {
    match read(bytes, file_name)? {
        Outcome::Text(out) => Ok(out),
        Outcome::Scan(_) => Err(IngestError::Scanned),
    }
}

/// What the worker runs: the text, or a scan's page images.
pub fn read(bytes: &[u8], file_name: &str) -> Result<Outcome, IngestError> {
    let out = match detect(bytes, file_name)? {
        Format::Docx => docx::extract(bytes)?,
        Format::Odt => odt::extract(bytes)?,
        Format::Rtf => rtf::extract(bytes)?,
        Format::Doc => doc::extract(bytes)?,
        Format::Image => {
            return Ok(Outcome::Scan(Scan {
                format: Format::Image,
                images: scan::from_image_file(bytes)?,
                metadata: Vec::new(),
                warnings: Vec::new(),
                pages: 1,
            }))
        }
        Format::Pdf => match pdf::extract(bytes) {
            Err(IngestError::Scanned) => return pdf::scan(bytes).map(Outcome::Scan),
            other => other?,
        },
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
                ocr: false,
            }
        }
    };
    finish(out).map(Outcome::Text)
}

/// Clean the text (invisible characters, line ends) and refuse an empty result.
fn finish(mut out: Extracted) -> Result<Extracted, IngestError> {
    let (body, removed) = clean(&out.body);
    out.body = body;
    out.margins = clean(&out.margins).0;
    if removed {
        out.warnings
            .push("הוסרו מהטקסט תווי כיווניות/תווים בלתי נראים.".to_owned());
    }
    if out.body.trim().is_empty() {
        return Err(if matches!(out.format, Format::Pdf | Format::Image) {
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
        // Compound Files: decided by what they hold, not by the name.
        let doc = crate::doc::tests::build("טקסט", "", 0);
        assert_eq!(detect(&doc, "renamed.docx"), Ok(Format::Doc));
        let mut cfb = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new()))
            .unwrap_or_else(|e| panic!("{e}"));
        drop(cfb.create_stream("/EncryptedPackage"));
        let locked = cfb.into_inner().into_inner();
        assert_eq!(detect(&locked, "locked.docx"), Err(IngestError::Encrypted));
        assert!(matches!(
            detect(
                &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0],
                "x.doc"
            ),
            Err(IngestError::Corrupt(_))
        ));
        assert_eq!(detect(br"{\rtf1 x}", "a.txt"), Ok(Format::Rtf));
        assert_eq!(detect(b"\xFF\xD8\xFF\xE0", "a"), Ok(Format::Image));
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

/// A hostile or damaged file must end in an error, never a crash: thousands of mutated
/// documents (flipped bytes, cut short, garbage) through the same `extract` the worker runs.
/// Deterministic, so a failure reproduces. PDF is left out: its parser is a dependency whose
/// crashes the isolated worker contains (`tests/worker.rs`).
#[cfg(test)]
mod robustness {
    use super::extract;

    /// xorshift64*: enough randomness for mutations, no dependency, same run every time.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % (n.max(1) as u64)).unwrap_or(0)
        }
    }

    fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
        let mut b = seed.to_vec();
        match rng.below(4) {
            0 => {
                for _ in 0..=rng.below(8) {
                    let i = rng.below(b.len());
                    b[i] ^= u8::try_from(rng.next() & 0xFF).unwrap_or(1) | 1;
                }
            }
            1 => b.truncate(rng.below(b.len())),
            2 => {
                let i = rng.below(b.len());
                let junk: Vec<u8> = (0..rng.below(64))
                    .map(|_| u8::try_from(rng.next() & 0xFF).unwrap_or(0))
                    .collect();
                b.splice(i..i, junk);
            }
            _ => {
                let (i, j) = (rng.below(b.len()), rng.below(b.len()));
                b.swap(i, j);
            }
        }
        b
    }

    fn never_panics(name: &str, seed: &[u8], rounds: usize) {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed.len() as u64);
        for round in 0..rounds {
            let bytes = mutate(&mut rng, seed);
            let r = std::panic::catch_unwind(|| extract(&bytes, name));
            assert!(r.is_ok(), "{name}: round {round} crashed the parser");
        }
    }

    #[test]
    fn damaged_word_files_are_refused_not_crashed_on() {
        use crate::docx::tests::{build, document, para};
        let seed = build(&[(
            "word/document.xml",
            document(&format!(
                "{}{}",
                para("שורה ראשונה"),
                para("שורה שנייה עם <b>")
            )),
        )]);
        never_panics("a.docx", &seed, 3_000);
    }

    #[test]
    fn damaged_odt_files_are_refused_not_crashed_on() {
        let ns = crate::odt::tests::NS;
        let content = format!(
            "<office:document-content {ns}><office:body><office:text><text:p>שורה</text:p></office:text></office:body></office:document-content>"
        );
        let seed = crate::odt::tests::build(&content, "<x/>", "<x/>");
        never_panics("a.odt", &seed, 3_000);
    }

    #[test]
    fn garbage_and_odd_text_are_refused_not_crashed_on() {
        let mut rng = Rng(42);
        for round in 0..3_000 {
            let len = rng.below(512);
            let bytes: Vec<u8> = (0..len)
                .map(|_| u8::try_from(rng.next() & 0xFF).unwrap_or(0))
                .collect();
            let name = ["a.txt", "a.docx", "a.odt", "a.doc", "x"][round % 5];
            let r = std::panic::catch_unwind(|| extract(&bytes, name));
            assert!(r.is_ok(), "{name}: round {round} crashed");
        }
        never_panics(
            "a.txt",
            "שורה\r\nשורה ‏עם כיווניות\u{200f}".as_bytes(),
            2_000,
        );
    }
}
