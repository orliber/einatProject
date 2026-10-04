//! Local OCR: Tesseract, bundled with the app, reads the page images of a scan or photo.
//!
//! Nothing leaves the computer: the engine is a separate program next to the app, started
//! isolated like the document worker ([`crate::worker::spawn_isolated`]: on Windows no network,
//! none of her files, a memory ceiling), once per page, with the image on stdin and a table of
//! words on stdout. It reads Hebrew and English, and turns a sideways or upside-down photo the
//! right way first. Its text then goes through the same filter as any other document (D-048).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::scan::PageImage;
use crate::worker::{spawn_isolated, Isolation};
use crate::{Extracted, IngestError, Scan};

/// Per page: a full A4 page at 300 dpi takes a few seconds.
pub const PAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// Far above what one 300 dpi page needs (about 200 MB).
const MEMORY_MB: u32 = 1024;
const MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Below this average certainty the psychologist is told to compare with the paper.
const LOW_CONFIDENCE: f64 = 75.0;

/// The bundled OCR engine: the program and its language data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    exe: PathBuf,
    tessdata: PathBuf,
    /// The folder the engine may read (its program files and language data).
    root: PathBuf,
}

impl Engine {
    /// An engine at explicit paths (tests, development).
    #[must_use]
    pub fn new(exe: PathBuf, tessdata: PathBuf) -> Self {
        let root = tessdata.clone();
        Self {
            exe,
            tessdata,
            root,
        }
    }

    /// The engine shipped next to the app: `<app folder>/ocr/tesseract(.exe)` with
    /// `<app folder>/ocr/tessdata/heb.traineddata`. `None` when it is not there.
    #[must_use]
    pub fn locate(app_exe: &Path) -> Option<Self> {
        let root = app_exe.parent()?.join("ocr");
        let exe = root.join(if cfg!(windows) {
            "tesseract.exe"
        } else {
            "tesseract"
        });
        let tessdata = root.join("tessdata");
        if exe.is_file() && tessdata.join("heb.traineddata").is_file() {
            return Some(Self {
                exe,
                tessdata,
                root,
            });
        }
        // Development builds on Linux use the system's Tesseract when it has Hebrew.
        if cfg!(debug_assertions) {
            for data in [
                "/usr/share/tesseract-ocr/5/tessdata",
                "/usr/share/tesseract-ocr/4.00/tessdata",
                "/opt/homebrew/share/tessdata",
                "/usr/local/share/tessdata",
            ] {
                let data = PathBuf::from(data);
                for exe in [
                    "/usr/bin/tesseract",
                    "/opt/homebrew/bin/tesseract",
                    "/usr/local/bin/tesseract",
                ] {
                    let exe = PathBuf::from(exe);
                    if exe.is_file() && data.join("heb.traineddata").is_file() {
                        return Some(Self::new(exe, data));
                    }
                }
            }
        }
        None
    }

    fn languages(&self) -> &'static str {
        if self.tessdata.join("eng.traineddata").is_file() {
            "heb+eng"
        } else {
            "heb"
        }
    }

    /// Read one page image. Returns the page's text and its words' average certainty (0–100).
    pub fn read_page(
        &self,
        image: &PageImage,
        timeout: Duration,
    ) -> Result<(String, Option<f64>), IngestError> {
        let tessdata = self
            .tessdata
            .to_str()
            .ok_or_else(|| IngestError::Ocr("path".to_owned()))?;
        // Page segmentation 1 turns a rotated page first; it needs the orientation data.
        let psm = if self.tessdata.join("osd.traineddata").is_file() {
            "1"
        } else {
            "3"
        };
        let dpi = image.dpi.map(|d| d.to_string());
        let mut args: Vec<&str> = vec![
            "stdin",
            "stdout",
            "-l",
            self.languages(),
            "--tessdata-dir",
            tessdata,
            "--psm",
            psm,
        ];
        if let Some(d) = &dpi {
            args.extend(["--dpi", d]);
        }
        args.extend(["-c", "tessedit_create_tsv=1", "-c", "tessedit_create_txt=0"]);
        let policy = Isolation {
            memory_mb: MEMORY_MB,
            read_dirs: vec![self.root.clone()],
            single_thread: true,
        };
        let mut child = spawn_isolated(&self.exe, &args, &policy)?;
        let (Some(mut stdin), Some(mut stdout)) = (child.take_stdin(), child.take_stdout()) else {
            child.kill();
            return Err(IngestError::Ocr("pipes".to_owned()));
        };
        let bytes = image.bytes.clone();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            (&mut stdout)
                .take(MAX_OUTPUT_BYTES)
                .read_to_end(&mut buf)
                .map(|_| buf)
        });
        let status = match child.wait_timeout(timeout) {
            Ok(Some(status)) => status,
            Ok(None) => {
                child.kill();
                return Err(IngestError::Timeout);
            }
            Err(e) => {
                child.kill();
                return Err(IngestError::Ocr(e.to_string()));
            }
        };
        let _ = writer.join();
        let output = reader
            .join()
            .map_err(|_| IngestError::Ocr("reader".to_owned()))?
            .map_err(|e| IngestError::Ocr(e.to_string()))?;
        if !status.success() {
            return Err(IngestError::Ocr(format!("engine exited: {status}")));
        }
        Ok(from_tsv(&String::from_utf8_lossy(&output)))
    }
}

/// Tesseract's word table → text (a line per line, a blank line between blocks) and the
/// average certainty of its words, weighted by their length.
pub(crate) fn from_tsv(tsv: &str) -> (String, Option<f64>) {
    let mut text = String::new();
    let mut key: Option<(u32, u32, u32, u32)> = None;
    let mut block: Option<(u32, u32)> = None;
    let (mut weighted, mut letters) = (0.0f64, 0usize);
    for row in tsv.lines() {
        let cols: Vec<&str> = row.split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let n = |i: usize| cols[i].parse::<u32>().unwrap_or(0);
        let word = cols[11].trim();
        if word.is_empty() {
            continue;
        }
        let this = (n(1), n(2), n(3), n(4));
        if key != Some(this) {
            if key.is_some() {
                text.push('\n');
                if block != Some((this.0, this.1)) {
                    text.push('\n');
                }
            }
            key = Some(this);
            block = Some((this.0, this.1));
        } else {
            text.push(' ');
        }
        text.push_str(word);
        if let Ok(conf) = cols[10].parse::<f64>() {
            if conf >= 0.0 {
                let len = word.chars().count();
                weighted += conf * len as f64;
                letters += len;
            }
        }
    }
    let conf = (letters > 0).then(|| weighted / letters as f64);
    (text, conf)
}

/// Read every page of a scan and assemble the document. Fails closed: a page the engine could
/// not read stops the import (a missing page could hold what the psychologist relies on).
pub fn recognize(engine: &Engine, scan: Scan) -> Result<Extracted, IngestError> {
    let started = Instant::now();
    let mut body = String::new();
    let mut low_pages: Vec<u32> = Vec::new();
    let mut current_page = 0;
    for image in &scan.images {
        // The whole document gets at most a few minutes.
        if started.elapsed() > PAGE_TIMEOUT * 5 {
            return Err(IngestError::Timeout);
        }
        let (text, conf) = engine.read_page(image, PAGE_TIMEOUT)?;
        if image.page != current_page && !body.is_empty() {
            body.push_str("\n\n");
        }
        current_page = image.page;
        body.push_str(text.trim());
        if let Some(c) = conf {
            if c < LOW_CONFIDENCE && !low_pages.contains(&image.page) {
                low_pages.push(image.page);
            }
        }
    }
    let mut warnings = scan.warnings;
    warnings.insert(
        0,
        "הטקסט זוהה אוטומטית מסריקה (OCR), במחשב הזה. כדאי לעבור עליו מול המקור: שמות ומספרים עלולים להיקרא לא נכון.".to_owned(),
    );
    if !low_pages.is_empty() {
        let list: Vec<String> = low_pages.iter().map(u32::to_string).collect();
        warnings.push(format!(
            "האיכות בעמודים {} נמוכה (סריקה מטושטשת או צילום עקום). מומלץ לבדוק את הטקסט מול הדף, או לסרוק שוב.",
            list.join(", ")
        ));
    }
    Ok(Extracted {
        format: scan.format,
        body,
        margins: String::new(),
        metadata: scan.metadata,
        warnings,
        pages: scan.pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_table_becomes_lines_and_blocks() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
            1\t1\t0\t0\t0\t0\t0\t0\t10\t10\t-1\t\n\
            5\t1\t1\t1\t1\t1\t0\t0\t1\t1\t90\tנועם\n\
            5\t1\t1\t1\t1\t2\t0\t0\t1\t1\t80\tמשחק\n\
            5\t1\t1\t1\t2\t1\t0\t0\t1\t1\t70\tבחצר\n\
            5\t1\t2\t1\t1\t1\t0\t0\t1\t1\t60\tסוף\n";
        let (text, conf) = from_tsv(tsv);
        assert_eq!(text, "נועם משחק\nבחצר\n\nסוף");
        let c = conf.unwrap_or(0.0);
        assert!((c - 76.0).abs() < 0.01, "{c}");
    }
}
