//! "בדיקת מערכת" (D-148): one button in Settings → עדכונים ועזרה runs safe self-tests, and
//! each line says ✓ or ✗, why in one short sentence, and what to do.
//!
//! 1. Reading: three files the program makes up itself (a Word file, a PDF with a few lines
//!    of text, a small picture of a page) go through the real isolated reader.
//! 2. OCR: the bundled engine reads the made-up page picture.
//! 3. Folders: a tiny file of fixed bytes is written, renamed and deleted in the folder
//!    reports are saved to and in the folder of the last backup (Controlled Folder Access,
//!    Documents moved to OneDrive, permissions). Never in the vault's folder, never over a file
//!    that exists.
//! 4. Connection: a TLS handshake with the AI service and nothing else (`dv_egress::reach`).
//! 5. Basics: a key is saved (yes or no, never the key), the model is allowed, the version,
//!    the system, the FIPS module, the free space where the vault lives.
//!
//! Nothing here reads a case, writes the vault, or holds the core while it runs. A check that
//! fails only says so: the program works on as before. Every `detail` is ASCII with no path,
//! no content and no key; it is what "העתקה לאור" copies.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use dv_egress::Reach;
use dv_ingest::ocr::Engine;
use dv_ingest::{Format, IngestError, Outcome, PageImage};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Core;

/// The checks, in the order the window runs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SystemCheck {
    Reading,
    Ocr,
    Folders,
    Connection,
    Basics,
}

impl SystemCheck {
    pub const ALL: [Self; 5] = [
        Self::Reading,
        Self::Ocr,
        Self::Folders,
        Self::Connection,
        Self::Basics,
    ];
}

/// ✓, ✗, or in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CheckState {
    /// Works.
    Ok,
    /// A fact, neither good nor bad (the version, the system).
    Info,
    /// Works, with something worth knowing.
    Warn,
    /// Does not work: the next step says what to do.
    Failed,
    /// Not checked, and why.
    Skipped,
}

/// One line of the result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CheckLine {
    /// A stable ASCII name (`docx`, `ocr`, `export_folder`, `connection`…).
    pub key: String,
    /// What was checked, in Hebrew.
    pub label: String,
    pub state: CheckState,
    /// Why, in one short Hebrew sentence.
    pub reason: String,
    /// What to do, when there is something to do.
    pub next_step: Option<String>,
    /// For "העתקה לאור": ASCII, no path, no content, no key.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SystemCheckResult {
    pub check: SystemCheck,
    pub lines: Vec<CheckLine>,
}

/// The usual folders a report is saved to, in the order the shell tries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPlace {
    Downloads,
    Documents,
    Home,
}

impl ExportPlace {
    fn he(self) -> &'static str {
        match self {
            Self::Downloads => "ההורדות",
            Self::Documents => "המסמכים",
            Self::Home => "הבית",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Downloads => "downloads",
            Self::Documents => "documents",
            Self::Home => "home",
        }
    }
}

/// Where the last backup went, as the vault remembers it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BackupFolder {
    Locked,
    Never,
    At(PathBuf),
}

/// What one check needs, taken under the core's lock; [`SystemCheckJob::run`] does the work
/// with the core free.
#[derive(Debug)]
pub struct SystemCheckJob {
    check: SystemCheck,
    inputs: Inputs,
}

#[derive(Debug)]
enum Inputs {
    Reading {
        worker: Option<PathBuf>,
    },
    Ocr {
        engine: Option<Engine>,
    },
    Folders {
        export: Vec<(ExportPlace, PathBuf)>,
        backup: BackupFolder,
        vault_dir: PathBuf,
    },
    Connection {
        /// The model she chose and the AI's name; `None` while the vault is locked.
        model: Option<(String, &'static str)>,
    },
    Basics {
        basics: Basics,
        vault_dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Basics {
    unlocked: bool,
    /// The chosen AI runs on this computer and takes no key.
    local: bool,
    has_key: bool,
    /// The model as saved, before the allow-list is applied.
    stored_model: Option<String>,
    /// The model in use, after it.
    model: String,
}

impl Core {
    /// Take what `check` needs. `export`: the usual folders for a report, from the shell
    /// (only [`SystemCheck::Folders`] uses them). Never fails, and never reads a case.
    #[must_use]
    pub fn begin_system_check(
        &mut self,
        check: SystemCheck,
        export: Vec<(ExportPlace, PathBuf)>,
    ) -> SystemCheckJob {
        let inputs = match check {
            SystemCheck::Reading => Inputs::Reading {
                worker: self.ingest_exe.clone(),
            },
            SystemCheck::Ocr => Inputs::Ocr {
                engine: self.ocr.clone(),
            },
            SystemCheck::Folders => {
                let backup = match &self.vault {
                    None => BackupFolder::Locked,
                    Some(v) => v
                        .setting(crate::backup::LAST_DIR_KEY)
                        .ok()
                        .flatten()
                        .filter(|d| !d.is_empty())
                        .map_or(BackupFolder::Never, |d| BackupFolder::At(PathBuf::from(d))),
                };
                Inputs::Folders {
                    export,
                    backup,
                    vault_dir: self.dir.clone(),
                }
            }
            SystemCheck::Connection => Inputs::Connection {
                model: self.vault.is_some().then(|| {
                    let model = self.chosen_model();
                    let name = crate::provider_of(&model).display_name();
                    (model, name)
                }),
            },
            SystemCheck::Basics => {
                let model = self.chosen_model();
                let provider = crate::provider_of(&model);
                let (has_key, stored_model) = match &self.vault {
                    Some(v) => (
                        crate::key_of(v, provider).ok().flatten().is_some(),
                        v.setting(crate::MODEL_KEY).ok().flatten(),
                    ),
                    None => (false, None),
                };
                Inputs::Basics {
                    basics: Basics {
                        unlocked: self.vault.is_some(),
                        local: provider == dv_ai::Provider::Local,
                        has_key,
                        stored_model,
                        model,
                    },
                    vault_dir: self.dir.clone(),
                }
            }
        };
        SystemCheckJob { check, inputs }
    }
}

/// What the checks touch outside the program: the real things, or stand-ins in the tests
/// (never the network, never a real worker there).
pub(crate) trait Probes {
    fn read(&self, worker: &Path, file_name: &str, bytes: &[u8]) -> Result<Outcome, IngestError>;
    fn ocr(&self, engine: &Engine, page: &PageImage) -> Result<String, IngestError>;
    fn reach(&self, model: &str) -> Reach;
    fn os_version(&self) -> Option<String>;
    fn free_bytes(&self, dir: &Path) -> Option<u64>;
}

struct Real;

impl Probes for Real {
    fn read(&self, worker: &Path, file_name: &str, bytes: &[u8]) -> Result<Outcome, IngestError> {
        // Once, with the reader's own time limit: a reader that cannot start is what this
        // check is for, so it is not started a second time as an import would be.
        dv_ingest::worker::run(worker, file_name, bytes, dv_ingest::worker::DEFAULT_TIMEOUT)
    }

    fn ocr(&self, engine: &Engine, page: &PageImage) -> Result<String, IngestError> {
        engine
            .read_page(page, dv_ingest::ocr::PAGE_TIMEOUT)
            .map(|(text, _)| text)
    }

    fn reach(&self, model: &str) -> Reach {
        dv_egress::reach_ai_service(model)
    }

    fn os_version(&self) -> Option<String> {
        dv_vault::env::os_version()
    }

    fn free_bytes(&self, dir: &Path) -> Option<u64> {
        dv_vault::env::free_bytes(dir)
    }
}

impl SystemCheckJob {
    /// Run the check. Slow (the reader, the engine, the network): never under the core's
    /// lock, never on the window's thread.
    #[must_use]
    pub fn run(self) -> SystemCheckResult {
        self.run_with(&Real)
    }

    pub(crate) fn run_with(self, probes: &dyn Probes) -> SystemCheckResult {
        let lines = match self.inputs {
            Inputs::Reading { worker } => reading(worker.as_deref(), probes),
            Inputs::Ocr { engine } => vec![ocr(engine.as_ref(), probes)],
            Inputs::Folders {
                export,
                backup,
                vault_dir,
            } => vec![
                export_folder(&export, &vault_dir),
                backup_folder(&backup, &vault_dir),
            ],
            Inputs::Connection { model } => vec![connection(model.as_ref(), probes)],
            Inputs::Basics { basics, vault_dir } => basics_lines(&basics, &vault_dir, probes),
        };
        SystemCheckResult {
            check: self.check,
            lines,
        }
    }
}

fn line(
    key: &str,
    label: &str,
    state: CheckState,
    reason: &str,
    next_step: Option<&str>,
    detail: &str,
) -> CheckLine {
    CheckLine {
        key: key.to_owned(),
        label: label.to_owned(),
        state,
        reason: reason.to_owned(),
        next_step: next_step.map(str::to_owned),
        detail: ascii(detail),
    }
}

/// ASCII, no control characters, nothing after a path separator, at most 200 characters.
fn ascii(s: &str) -> String {
    let head = s.split(['/', '\\']).next().unwrap_or("");
    let mut out: String = head
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .take(200)
        .collect();
    if head.len() < s.len() {
        out.push_str("<path left out>");
    }
    out
}

// ------------------------------------------------------------ 1. reading

/// A made-up sample: its key, its label, the file's name, its bytes, and a word the reading
/// must find (compared without spaces, in lower case).
struct Sample {
    key: &'static str,
    label: &'static str,
    file_name: &'static str,
    bytes: Vec<u8>,
    expect: &'static str,
}

/// The fabricated paragraph of the Word sample. Nothing in it is about anyone.
const DOCX_TEXT: &str =
    "זה מסמך בדוי שהתוכנה יצרה בעצמה לבדיקת המערכת. אין בו שם ואין בו שום דבר מתיק.";

fn samples() -> Vec<Sample> {
    vec![
        Sample {
            key: "docx",
            label: "קריאת קובץ Word",
            file_name: "system-check.docx",
            bytes: sample_docx(),
            expect: "מסמךבדוי",
        },
        Sample {
            key: "pdf",
            label: "קריאת קובץ PDF",
            file_name: "system-check.pdf",
            bytes: sample_pdf(),
            expect: "systemcheck",
        },
        Sample {
            key: "image",
            label: "קבלת תמונה של דף",
            file_name: "system-check.png",
            bytes: sample_png(),
            expect: "",
        },
    ]
}

/// A Word file made by the program's own Word writer (the same one that makes her reports).
fn sample_docx() -> Vec<u8> {
    let report = dv_export::Report {
        title: "בדיקת מערכת".to_owned(),
        parts: vec![dv_export::ReportPart {
            title: "דף בדיקה".to_owned(),
            sections: vec![dv_export::ReportSection {
                title: "בדיקה".to_owned(),
                paragraphs: vec![DOCX_TEXT.to_owned()],
            }],
        }],
        font: "David".to_owned(),
        ..dv_export::Report::default()
    };
    dv_export::render(&report).unwrap_or_default()
}

/// The fabricated lines of the PDF sample. Several lines: a single short one reads as a scan.
const PDF_LINES: &[&str] = &[
    "This page was made up by the program for its system check.",
    "It holds no name and nothing from any case.",
    "The reader should find these five lines of plain text.",
    "Line four of the sample page.",
    "The end of the sample page.",
];

/// A one-page PDF with a text layer, in one of the fonts every PDF reader has (Helvetica).
fn sample_pdf() -> Vec<u8> {
    let mut content = String::from("BT /F1 12 Tf 72 700 Td 16 TL\n");
    for l in PDF_LINES {
        content.push_str(&format!("({l}) Tj T*\n"));
    }
    content.push_str("ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

/// The text on the page picture, line by line.
const PNG_LINES: &[&str] = &["SYSTEM CHECK", "1357"];
/// Each dot of the 5×7 letters is this many pixels square.
const PNG_SCALE: usize = 3;

/// A 5×7 dot letter, row by row (the low five bits, the leftmost dot highest).
fn glyph(c: char) -> [u8; 7] {
    match c {
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        _ => [0; 7],
    }
}

/// A small grey picture of a page with [`PNG_LINES`] on it, as a PNG file: black dot letters
/// on white, stored without compression (a few tens of kilobytes).
fn sample_png() -> Vec<u8> {
    let s = PNG_SCALE;
    let margin = 4 * s;
    let (cell, line_height) = (6 * s, 10 * s);
    let widest = PNG_LINES.iter().map(|l| l.len()).max().unwrap_or(1);
    let width = 2 * margin + cell * widest;
    let height = 2 * margin + line_height * PNG_LINES.len();
    let mut pixels = vec![255u8; width * height];
    for (li, text) in PNG_LINES.iter().enumerate() {
        for (ci, c) in text.chars().enumerate() {
            for (r, row) in glyph(c).iter().enumerate() {
                for col in 0..5 {
                    if row & (0x10 >> col) == 0 {
                        continue;
                    }
                    for dy in 0..s {
                        for dx in 0..s {
                            let y = margin + li * line_height + r * s + dy;
                            let x = margin + ci * cell + col * s + dx;
                            if let Some(p) = pixels.get_mut(y * width + x) {
                                *p = 0;
                            }
                        }
                    }
                }
            }
        }
    }
    // Each row: filter 0 (none), then the row.
    let mut raw = Vec::with_capacity((width + 1) * height);
    for row in pixels.chunks(width) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut ihdr = Vec::new();
    ihdr.extend(u32::try_from(width).unwrap_or(0).to_be_bytes());
    ihdr.extend(u32::try_from(height).unwrap_or(0).to_be_bytes());
    // 8-bit greyscale, deflate, filter set 0, no interlace.
    ihdr.extend([8, 0, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    png_chunk(&mut out, b"IEND", &[]);
    out
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend(u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(out.get(start..).unwrap_or_default());
    out.extend(crc.to_be_bytes());
}

/// A zlib stream of stored (uncompressed) deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(0xFFFF).peekable();
    if blocks.peek().is_none() {
        out.extend([1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        let last = u8::from(blocks.peek().is_none());
        let len = u16::try_from(block.len()).unwrap_or(u16::MAX);
        out.push(last);
        out.extend(len.to_le_bytes());
        out.extend((!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend(((b << 16) | a).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Lower case, without spaces: how a sample's word is looked for.
fn squeezed(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

const REINSTALL: &str = "מתקינים מחדש את כספת האבחון (מהקישור של הגרסה האחרונה). אם זה חוזר, מוסיפים את תיקיית התוכנה לחריגים של האנטי-וירוס, ולוחצים \"העתקה לאור\".";
const ANTIVIRUS: &str = "בדרך כלל זה אנטי-וירוס שחוסם את רכיב הקריאה: מוסיפים את תיקיית התוכנה לחריגים שלו ובודקים שוב. אם זה לא עוזר, מתקינים מחדש את כספת האבחון.";
const COPY_FOR_OR: &str = "לוחצים \"העתקה לאור\" ושולחים לו את מה שהועתק.";

fn reading(worker: Option<&Path>, probes: &dyn Probes) -> Vec<CheckLine> {
    let mut lines = Vec::new();
    let mut gave_up = false;
    for s in samples() {
        if gave_up {
            lines.push(line(
                s.key,
                s.label,
                CheckState::Skipped,
                "לא נבדק: הקריאה הקודמת לא הסתיימה בזמן.",
                None,
                &format!("{}: skipped", s.key),
            ));
            continue;
        }
        let Some(exe) = worker else {
            lines.push(line(
                s.key,
                s.label,
                CheckState::Failed,
                "רכיב הקריאה של התוכנה לא נמצא.",
                Some(REINSTALL),
                &format!("{}: no reader", s.key),
            ));
            continue;
        };
        let read = probes.read(exe, s.file_name, &s.bytes);
        gave_up = matches!(read, Err(IngestError::Timeout));
        lines.push(read_line(&s, &read));
    }
    lines
}

fn read_line(s: &Sample, read: &Result<Outcome, IngestError>) -> CheckLine {
    let detail = |what: &str| format!("{}: {what}", s.key);
    match read {
        Ok(Outcome::Text(e)) if s.expect.is_empty() || !squeezed(&e.body).contains(s.expect) => {
            line(
                s.key,
                s.label,
                CheckState::Failed,
                "הקובץ נקרא, אבל הטקסט שחזר אינו הטקסט שנכתב בו.",
                Some(COPY_FOR_OR),
                &detail(&format!("text differs ({})", e.format.name())),
            )
        }
        Ok(Outcome::Text(e)) => line(
            s.key,
            s.label,
            CheckState::Ok,
            "קובץ הבדיקה נקרא כמו שצריך ברכיב הקריאה המבודד.",
            None,
            &detail(&format!("ok ({})", e.format.name())),
        ),
        Ok(Outcome::Scan(scan)) if s.expect.is_empty() && !scan.images.is_empty() => line(
            s.key,
            s.label,
            CheckState::Ok,
            "התמונה התקבלה ועברה לזיהוי הטקסט.",
            None,
            &detail(&format!("ok ({} page image)", scan.images.len())),
        ),
        Ok(Outcome::Scan(scan)) => line(
            s.key,
            s.label,
            CheckState::Failed,
            if scan.format == Format::Pdf {
                "קובץ ה-PDF נקרא כסריקה, למרות שיש בו טקסט."
            } else {
                "הקובץ לא נקרא כמו שצריך."
            },
            Some(COPY_FOR_OR),
            &detail(&format!("read as a scan ({})", scan.format.name())),
        ),
        Err(e) => {
            let (reason, next) = read_failure(e);
            line(
                s.key,
                s.label,
                CheckState::Failed,
                &reason,
                Some(next),
                &detail(&e.detail()),
            )
        }
    }
}

/// Why the reader failed on a file that is known to be good, and what to do.
fn read_failure(e: &IngestError) -> (String, &'static str) {
    match e {
        IngestError::WorkerCrashed(code) => {
            let why = match code {
                0xC000_0022 => "אין לו גישה לקבצים של התוכנה",
                0xC000_0135 | 0xC000_0142 | 0xC000_0139 => {
                    "ספרייה שהוא צריך לא נטענה"
                }
                0xC000_007B => "קובץ ההרצה שלו פגום",
                _ => "הוא נעצר מיד",
            };
            let next = if *code == 0xC000_0022 { ANTIVIRUS } else { REINSTALL };
            (
                format!("רכיב הקריאה לא הצליח לעלות ({why}, קוד {code:#x}). זו לא בעיה בקבצים."),
                next,
            )
        }
        IngestError::Worker(_) => (
            "רכיב הקריאה לא הצליח להתחיל בבידוד שלו.".to_owned(),
            ANTIVIRUS,
        ),
        IngestError::Timeout => (
            "הקריאה של קובץ קטן לקחה יותר מדי זמן.".to_owned(),
            "המחשב עמוס, או שאנטי-וירוס סורק את רכיב הקריאה בכל הפעלה. סוגרים תוכנות כבדות ובודקים שוב; אם זה חוזר, מוסיפים את תיקיית התוכנה לחריגים של האנטי-וירוס.",
        ),
        IngestError::Corrupt(_) => (
            "רכיב הקריאה נעצר באמצע קובץ שידוע שהוא תקין.".to_owned(),
            REINSTALL,
        ),
        _ => (
            "קובץ הבדיקה לא נקרא.".to_owned(),
            COPY_FOR_OR,
        ),
    }
}

// ------------------------------------------------------------ 2. OCR

/// At least this share of the page's letters must come back for the reading to count.
const OCR_ENOUGH: f64 = 0.6;

fn ocr(engine: Option<&Engine>, probes: &dyn Probes) -> CheckLine {
    const LABEL: &str = "זיהוי טקסט בסריקות ובתמונות (OCR)";
    let Some(engine) = engine else {
        return line(
            "ocr",
            LABEL,
            CheckState::Skipped,
            "רכיב זיהוי הטקסט לא מותקן במחשב הזה, ולכן אין מה לבדוק. בלעדיו מסמכים סרוקים ותמונות של דפים לא נקראים; כל השאר עובד.",
            Some("התקנה מחדש של כספת האבחון מוסיפה אותו."),
            "ocr: not installed",
        );
    };
    let page = PageImage {
        page: 1,
        dpi: None,
        bytes: sample_png(),
    };
    match probes.ocr(engine, &page) {
        Ok(text) => {
            let share = ocr_share(&text);
            let percent = (share * 100.0).round();
            if share >= OCR_ENOUGH {
                line(
                    "ocr",
                    LABEL,
                    CheckState::Ok,
                    "רכיב זיהוי הטקסט קרא את דף הבדיקה.",
                    None,
                    &format!("ocr: ok ({percent}% of the sample)"),
                )
            } else {
                line(
                    "ocr",
                    LABEL,
                    CheckState::Warn,
                    "רכיב זיהוי הטקסט פועל, אבל קרא את דף הבדיקה רק בחלקו.",
                    Some("סריקות ייקראו, אבל כדאי לעבור על הטקסט מול הדף. אם גם סריקות ברורות נקראות רע, לוחצים \"העתקה לאור\"."),
                    &format!("ocr: partial ({percent}% of the sample)"),
                )
            }
        }
        Err(e) => {
            let (reason, next) = match e {
                IngestError::Timeout => (
                    "זיהוי הטקסט של דף קטן לקח יותר מדי זמן.",
                    "המחשב עמוס, או שאנטי-וירוס סורק את רכיב זיהוי הטקסט. סוגרים תוכנות כבדות ובודקים שוב; אם זה חוזר, מוסיפים את תיקיית התוכנה לחריגים של האנטי-וירוס.",
                ),
                IngestError::Worker(_) => (
                    "רכיב זיהוי הטקסט לא הצליח להתחיל בבידוד שלו.",
                    ANTIVIRUS,
                ),
                _ => ("רכיב זיהוי הטקסט לא הצליח לפעול.", REINSTALL),
            };
            line(
                "ocr",
                LABEL,
                CheckState::Failed,
                reason,
                Some(next),
                &format!("ocr: {}", e.detail()),
            )
        }
    }
}

/// How much of the page's text the engine read: the longest run of its letters and digits, in
/// order, that the answer holds, over all of them.
fn ocr_share(text: &str) -> f64 {
    let norm = |s: &str| -> Vec<char> {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect()
    };
    let want = norm(&PNG_LINES.concat());
    let got = norm(text);
    if want.is_empty() {
        return 0.0;
    }
    // Longest common subsequence, one row at a time.
    let mut prev = vec![0usize; got.len() + 1];
    for w in &want {
        let mut row = vec![0usize; got.len() + 1];
        for (j, g) in got.iter().enumerate() {
            let diag = prev.get(j).copied().unwrap_or(0);
            let up = prev.get(j + 1).copied().unwrap_or(0);
            let left = row.get(j).copied().unwrap_or(0);
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = if w == g { diag + 1 } else { up.max(left) };
            }
        }
        prev = row;
    }
    let common = prev.last().copied().unwrap_or(0);
    common as f64 / want.len() as f64
}

// ------------------------------------------------------------ 3. folders

/// What the trial file holds: fixed words, nothing else.
const TRIAL_BYTES: &[u8] = b"Diagnostic Vault write check. Holds nothing; deleted at once.\n";

static TRIALS: AtomicU64 = AtomicU64::new(0);

/// A name no file has: the time, the process and a counter.
fn trial_stem() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TRIALS.fetch_add(1, Ordering::SeqCst);
    format!(
        "dv-write-check-{:x}-{:x}-{n}",
        nanos & 0xFFFF_FFFF_FFFF,
        std::process::id()
    )
}

/// Which step of the trial failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Create,
    Write,
    Rename,
    Delete,
}

impl Step {
    fn id(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Write => "write",
            Self::Rename => "rename",
            Self::Delete => "delete",
        }
    }
}

/// Write a tiny file under a new name, sync it, rename it, delete it: what saving a report
/// (a new file) and a backup (a temporary name, then renamed) do. Never over a file that
/// exists. On a failure the trial file is removed as far as the system lets it.
fn trial_write(dir: &Path) -> Result<(), (Step, std::io::Error)> {
    let stem = trial_stem();
    let first = dir.join(format!("{stem}.part"));
    let second = dir.join(format!("{stem}.tmp"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&first)
        .map_err(|e| (Step::Create, e))?;
    let written = file.write_all(TRIAL_BYTES).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&first);
        return Err((Step::Write, e));
    }
    if second.exists() {
        let _ = std::fs::remove_file(&first);
        return Err((
            Step::Rename,
            std::io::Error::from(std::io::ErrorKind::AlreadyExists),
        ));
    }
    if let Err(e) = std::fs::rename(&first, &second) {
        let _ = std::fs::remove_file(&first);
        return Err((Step::Rename, e));
    }
    std::fs::remove_file(&second).map_err(|e| (Step::Delete, e))
}

/// Inside the vault's own folder (or the folder itself): never written to by a check.
fn inside_vault(dir: &Path, vault_dir: &Path) -> bool {
    match (dir.canonicalize(), vault_dir.canonicalize()) {
        (Ok(d), Ok(v)) => d.starts_with(v),
        // Unknown: treated as inside (fail closed).
        (Ok(_), Err(_)) => true,
        (Err(_), _) => false,
    }
}

/// The trial in `dir`, as a line.
fn trial_line(key: &str, label: &str, where_he: &str, dir: &Path, vault_dir: &Path) -> CheckLine {
    if inside_vault(dir, vault_dir) {
        return line(
            key,
            label,
            CheckState::Skipped,
            "התיקייה הזו בתוך תיקיית הכספת, ולכן לא נכתב בה דבר.",
            None,
            &format!("{key}: inside the vault folder, not written"),
        );
    }
    match trial_write(dir) {
        Ok(()) => line(
            key,
            label,
            CheckState::Ok,
            &format!("נכתב ונמחק קובץ בדיקה קטן ב{where_he}."),
            None,
            &format!("{key}: ok"),
        ),
        Err((step, e)) => write_failure(key, label, where_he, step, &e),
    }
}

fn write_failure(
    key: &str,
    label: &str,
    where_he: &str,
    step: Step,
    e: &std::io::Error,
) -> CheckLine {
    use std::io::ErrorKind as K;
    let detail = format!("{key}: {} {}", step.id(), crate::io_detail(e));
    if step == Step::Delete {
        return line(
            key,
            label,
            CheckState::Warn,
            &format!("הכתיבה ב{where_he} עובדת, אבל קובץ הבדיקה לא נמחק. אפשר למחוק בעצמך קובץ ששמו מתחיל ב-dv-write-check."),
            None,
            &detail,
        );
    }
    let (reason, next) = match e.kind() {
        K::PermissionDenied | K::ReadOnlyFilesystem => (
            format!("אין לתוכנה הרשאה לכתוב ב{where_he}."),
            "אם ב\"אבטחת Windows\" מופעלת \"גישה מבוקרת לתיקיות\": הגנה מפני וירוסים ואיומים ← ניהול הגנה מפני תוכנות כופר ← \"התר לאפליקציה גישה דרך גישה מבוקרת לתיקיות\", ומוסיפים את כספת האבחון. אם זה דיסק נייד, בודקים שאינו נעול לכתיבה.",
        ),
        K::StorageFull | K::QuotaExceeded => (
            format!("אין מקום פנוי ב{where_he}."),
            "מפנים מקום בכונן הזה ובודקים שוב.",
        ),
        K::NotFound => (
            format!("{where_he} לא נמצאה."),
            "אם זה דיסק נייד, מחברים אותו ובודקים שוב.",
        ),
        _ => (format!("הכתיבה ב{where_he} נכשלה."), COPY_FOR_OR),
    };
    line(key, label, CheckState::Failed, &reason, Some(next), &detail)
}

fn export_folder(export: &[(ExportPlace, PathBuf)], vault_dir: &Path) -> CheckLine {
    const KEY: &str = "export_folder";
    const LABEL: &str = "שמירת דוחות";
    let synced: Vec<ExportPlace> = export
        .iter()
        .filter(|(_, d)| crate::cloud_synced_folder(d).is_some())
        .map(|(p, _)| *p)
        .collect();
    let chosen = export
        .iter()
        .find(|(_, d)| crate::cloud_synced_folder(d).is_none() && d.is_dir());
    let Some((place, dir)) = chosen else {
        return line(
            KEY,
            LABEL,
            CheckState::Warn,
            "אין תיקיית הורדות או מסמכים שאפשר לשמור בה דוח בלי שיעלה לענן.",
            Some("בכל שמירה של דוח התוכנה תבקש לבחור תיקייה: בוחרים תיקייה מקומית או דיסק נייד."),
            &format!(
                "{KEY}: no usual folder (synced: {})",
                synced.iter().map(|p| p.id()).collect::<Vec<_>>().join(",")
            ),
        );
    };
    let where_he = format!("תיקיית {}", place.he());
    let mut out = trial_line(KEY, LABEL, &where_he, dir, vault_dir);
    out.detail = ascii(&format!("{} (in {})", out.detail, place.id()));
    if !synced.is_empty() && out.state == CheckState::Ok {
        let names: Vec<&str> = synced.iter().map(|p| p.he()).collect();
        out.reason = format!(
            "{} תיקיית {} מסונכרנת לענן, ולכן דוחות נשמרים ב{where_he}.",
            out.reason,
            names.join(" ותיקיית ")
        );
    }
    out
}

fn backup_folder(backup: &BackupFolder, vault_dir: &Path) -> CheckLine {
    const KEY: &str = "backup_folder";
    const LABEL: &str = "שמירת גיבוי";
    match backup {
        BackupFolder::Locked => line(
            KEY,
            LABEL,
            CheckState::Skipped,
            "הכספת נעולה, ולכן לא ידוע איפה הגיבוי האחרון.",
            None,
            &format!("{KEY}: locked"),
        ),
        BackupFolder::Never => line(
            KEY,
            LABEL,
            CheckState::Skipped,
            "עוד לא נשמר גיבוי, ולכן אין תיקיית גיבוי לבדוק.",
            Some("שומרים גיבוי ראשון בהגדרות ← גיבוי, עדיף לדיסק נייד."),
            &format!("{KEY}: no backup yet"),
        ),
        BackupFolder::At(dir) if crate::cloud_synced_folder(dir).is_some() => line(
            KEY,
            LABEL,
            CheckState::Warn,
            "תיקיית הגיבוי האחרון מסונכרנת לענן, ולכן לא נכתב בה דבר.",
            Some("שומרים את הגיבוי הבא בדיסק נייד או בתיקייה מקומית."),
            &format!("{KEY}: synced to the cloud"),
        ),
        BackupFolder::At(dir) if !dir.is_dir() => line(
            KEY,
            LABEL,
            CheckState::Warn,
            "תיקיית הגיבוי האחרון לא נמצאת כרגע.",
            Some("אם הגיבוי נשמר בדיסק נייד, מחברים אותו ובודקים שוב."),
            &format!("{KEY}: folder not found"),
        ),
        BackupFolder::At(dir) => trial_line(KEY, LABEL, "תיקיית הגיבוי", dir, vault_dir),
    }
}

// ------------------------------------------------------------ 4. connection

fn connection(model: Option<&(String, &'static str)>, probes: &dyn Probes) -> CheckLine {
    const KEY: &str = "connection";
    let Some((model, ai)) = model else {
        return line(
            KEY,
            "חיבור לשירות ה-AI",
            CheckState::Skipped,
            "הכספת נעולה, ולכן לא ידוע לאיזה שירות להתחבר.",
            None,
            &format!("{KEY}: locked"),
        );
    };
    let label = format!("חיבור ל-{ai}");
    let reach = probes.reach(model);
    let d = |what: &str| format!("{KEY}: {what}");
    let failed = |reason: &str, next: &str, detail: String| {
        line(KEY, &label, CheckState::Failed, reason, Some(next), &detail)
    };
    match reach {
        Reach::Reached => line(
            KEY,
            &label,
            CheckState::Ok,
            "השירות ענה והוכיח את זהותו בתעודה שהתוכנה סומכת עליה. לא נשלח שום מידע ולא המפתח.",
            None,
            &d("reached (handshake only)"),
        ),
        Reach::NoName(x) => failed(
            "לא נמצאה הכתובת של השירות (DNS): בדרך כלל אין אינטרנט, או שהרשת חוסמת.",
            "בודקים שאתר אחר נפתח בדפדפן. אם כן, מנסים רשת אחרת (למשל נקודה חמה מהטלפון); אם שם זה עובד, הרשת הזו חוסמת.",
            d(&x),
        ),
        Reach::NoNetwork(x) => failed(
            "לא נפתח חיבור לשירות: אין רשת, או שחומת אש חוסמת את התוכנה.",
            "בודקים את החיבור לאינטרנט. אם הוא עובד, מאשרים לכספת האבחון גישה לאינטרנט בחומת האש או באנטי-וירוס, או מנסים רשת אחרת.",
            d(&x),
        ),
        Reach::TimedOut(x) => failed(
            "השירות לא ענה בזמן.",
            "החיבור איטי או נחסם בדרך. בודקים שוב בעוד כמה דקות, או ברשת אחרת.",
            d(&x),
        ),
        Reach::Intercepted(x) => failed(
            "החיבור המאובטח יורט: התעודה שהתקבלה לא שייכת לשירות. כך נראים אנטי-וירוס שסורק תעבורה מוצפנת (HTTPS) או proxy ברשת. התוכנה לא שולחת דבר דרך חיבור כזה.",
            "באנטי-וירוס מכבים \"סריקת HTTPS\" (או \"סריקת חיבורים מוצפנים\"), או מוסיפים את כספת האבחון לחריגים שלה. ברשת של מוסד מבקשים מאיש המחשבים לפטור את השירות מבדיקה.",
            d(&x),
        ),
        Reach::Pinned => failed(
            "השירות מציג תעודה מרשות שהגרסה הזו של התוכנה עוד לא מכירה.",
            "מעדכנים את התוכנה: הגדרות ← עדכונים ועזרה.",
            d("certificate from an authority this version is not pinned to"),
        ),
        Reach::Tls(x) => failed(
            "החיבור המאובטח לא הושלם: משהו בדרך (proxy, חומת אש או אנטי-וירוס) ענה במקום השירות או ניתק.",
            "מנסים ברשת אחרת. אם שם זה עובד, הבעיה ברשת הזו או בתוכנת האבטחה: מוסיפים את כספת האבחון לחריגים שלה.",
            d(&x),
        ),
        Reach::Stopped => line(
            KEY,
            &label,
            CheckState::Skipped,
            "עצירת השליחה מופעלת, ולכן לא נפתח שום חיבור.",
            Some("אחרי שמכבים את העצירה (הגדרות ← סיסמה ואבטחה) אפשר לבדוק שוב."),
            &d("emergency stop on, nothing opened"),
        ),
        Reach::Local => line(
            KEY,
            &label,
            CheckState::Skipped,
            "המודל רץ על המחשב הזה, ואין שירות באינטרנט לבדוק.",
            None,
            &d("local model"),
        ),
        Reach::NotAllowed => failed(
            "הדגם שנבחר לא ברשימה המותרת, ולכן לא נפתח חיבור.",
            "בוחרים דגם בהגדרות ← חיבור ל-AI.",
            d("model not allowed, nothing opened"),
        ),
    }
}

// ------------------------------------------------------------ 5. basics

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

fn basics_lines(b: &Basics, vault_dir: &Path, probes: &dyn Probes) -> Vec<CheckLine> {
    let mut lines = Vec::new();
    let key_line = if !b.unlocked {
        line(
            "api_key",
            "מפתח API",
            CheckState::Skipped,
            "הכספת נעולה.",
            None,
            "api_key: locked",
        )
    } else if b.local {
        line(
            "api_key",
            "מפתח API",
            CheckState::Ok,
            "המודל המקומי לא צריך מפתח.",
            None,
            "api_key: not needed",
        )
    } else if b.has_key {
        line(
            "api_key",
            "מפתח API",
            CheckState::Ok,
            "שמור בכספת (המפתח עצמו לא מוצג ולא נשלח בבדיקה).",
            None,
            "api_key: present",
        )
    } else {
        line(
            "api_key",
            "מפתח API",
            CheckState::Warn,
            "לא שמור מפתח, ולכן כל מה שנכתב כרגע הוא הדגמה בלבד.",
            Some("מדביקים מפתח בהגדרות ← חיבור ל-AI."),
            "api_key: absent (demo mode)",
        )
    };
    lines.push(key_line);
    let in_use_allowed = dv_egress::Provider::of_model(&b.model).is_some();
    let model_line = match &b.stored_model {
        Some(m) if !crate::ALLOWED_MODELS.contains(&m.as_str()) => line(
            "model",
            "הדגם שנבחר",
            CheckState::Warn,
            "הדגם שנשמר כבר לא ברשימה המותרת, ולכן התוכנה משתמשת בדגם ברירת המחדל.",
            Some("בוחרים דגם בהגדרות ← חיבור ל-AI."),
            "model: saved one not allowed, default in use",
        ),
        _ if !in_use_allowed => line(
            "model",
            "הדגם שנבחר",
            CheckState::Failed,
            "הדגם לא ברשימה המותרת של השליחה, ולכן שום בקשה לא תצא.",
            Some(COPY_FOR_OR),
            "model: not on the egress allow-list",
        ),
        stored => line(
            "model",
            "הדגם שנבחר",
            CheckState::Ok,
            "ברשימה המותרת (אפס שמירת מידע).",
            None,
            if stored.is_some() {
                "model: allowed"
            } else {
                "model: default, allowed"
            },
        ),
    };
    lines.push(model_line);
    let version = env!("CARGO_PKG_VERSION");
    let build = option_env!("DV_BUILD_COMMIT").unwrap_or("dev");
    lines.push(line(
        "version",
        "גרסת התוכנה",
        CheckState::Info,
        &format!("{version} ({})", ascii(build)),
        None,
        &format!("version: {version} build {build}"),
    ));
    lines.push(match probes.os_version() {
        Some(os) => {
            let os = ascii(&os);
            let eleven = os
                .strip_prefix("Windows 10.0.")
                .and_then(|rest| rest.split('.').next())
                .and_then(|b| b.parse::<u32>().ok())
                .is_some_and(|b| b >= 22000);
            let shown = if eleven {
                format!("{os} (Windows 11)")
            } else {
                os.clone()
            };
            line(
                "os",
                "מערכת ההפעלה",
                CheckState::Info,
                &shown,
                None,
                &format!("os: {os}"),
            )
        }
        None => line(
            "os",
            "מערכת ההפעלה",
            CheckState::Skipped,
            "לא הצלחתי לברר את הגרסה.",
            None,
            "os: unknown",
        ),
    });
    lines.push(if dv_vault::crypto::fips_active() {
        line(
            "fips",
            "מודול ההצפנה המאושר (FIPS)",
            CheckState::Ok,
            "פעיל: ההצפנה רצה על המודול המאושר (FIPS 140-3).",
            None,
            "fips: active",
        )
    } else {
        line(
            "fips",
            "מודול ההצפנה המאושר (FIPS)",
            CheckState::Info,
            "לא פעיל בגרסה הזו: ההצפנה (AES-256) רגילה, בלי המודול המאושר.",
            None,
            "fips: inactive",
        )
    });
    lines.push(disk_line(probes.free_bytes(vault_dir)));
    lines
}

fn disk_line(free: Option<u64>) -> CheckLine {
    const LABEL: &str = "מקום פנוי בדיסק של הכספת";
    let Some(free) = free else {
        return line(
            "disk",
            LABEL,
            CheckState::Skipped,
            "לא הצלחתי לברר כמה מקום פנוי.",
            None,
            "disk: unknown",
        );
    };
    let detail = format!("disk: {} MiB free", free / MIB);
    let gb = format!("{:.1}", free as f64 / GIB as f64);
    if free < GIB {
        line(
            "disk",
            LABEL,
            CheckState::Failed,
            &format!("נשארו רק {} MB פנויים.", free / MIB),
            Some("מפנים מקום (סל המחזור, הורדות ישנות): שמירה וגיבוי צריכים מקום."),
            &detail,
        )
    } else if free < 5 * GIB {
        line(
            "disk",
            LABEL,
            CheckState::Warn,
            &format!("{gb} GB פנויים: מספיק לעבודה, אבל לא להרבה."),
            Some("כדאי לפנות מקום בהזדמנות."),
            &detail,
        )
    } else {
        line(
            "disk",
            LABEL,
            CheckState::Ok,
            &format!("{gb} GB פנויים."),
            None,
            &detail,
        )
    }
}

#[cfg(test)]
mod tests;
