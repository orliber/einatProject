//! Local OCR: Tesseract, bundled with the app, reads the page images of a scan or photo.
//!
//! Nothing leaves the computer: the engine is a separate program next to the app, started
//! isolated like the document worker ([`crate::worker::spawn_isolated`]: on Windows no network,
//! none of her files, a memory ceiling), once per page, with the image on stdin and a table of
//! words on stdout. It reads Hebrew and English, and turns a sideways or upside-down photo the
//! right way first. Its text then goes through the same filter as any other document (D-048).
//!
//! Pages are read side by side, a few engines at once, each on one thread (D-160). A page the
//! engine could not read leaves out that page only, and a warning names it.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::scan::PageImage;
use crate::worker::{spawn_isolated, Isolation};
use crate::{Extracted, IngestError, Scan};

/// Per page: a full A4 page at 300 dpi takes a few seconds; a slow laptop, a dense page and
/// a few engines side by side take longer, and a page over it is left out, not the scan.
pub const PAGE_TIMEOUT: Duration = Duration::from_secs(120);
/// The most engines at once: each holds a page image and its model (about 200 MB).
const MAX_ENGINES: usize = 4;
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

    /// The engine's program.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.exe
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
        // Relative to its working folder: only ASCII on its command line, whatever the folder
        // the app is installed in is called (P-09).
        let relative = self.tessdata.strip_prefix(&self.root).ok();
        let tessdata = match relative {
            Some(r) if r.as_os_str().is_empty() => Some("."),
            Some(r) => r.to_str(),
            None => self.tessdata.to_str(),
        }
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
            working_dir: relative.is_some().then(|| self.root.clone()),
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
        // level, page, block, paragraph, line, word, left, top, width, height, conf, text.
        let [level, page, blk, par, line, _, _, _, _, _, conf, word, ..] = cols.as_slice() else {
            continue;
        };
        if *level != "5" {
            continue;
        }
        let n = |s: &str| s.parse::<u32>().unwrap_or(0);
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        let this = (n(page), n(blk), n(par), n(line));
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
        if let Ok(conf) = conf.parse::<f64>() {
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

/// How many engines read side by side: the computer's cores less one (the app and her other
/// work keep one), at most [`MAX_ENGINES`].
#[must_use]
pub fn engines() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get().saturating_sub(1))
        .clamp(1, MAX_ENGINES)
}

/// Read every page of a scan and assemble the document, the pages side by side. A page the
/// engine could not read is left out and named in a warning; when no page was read, the
/// scan is refused. `stop` (the vault locked meanwhile) ends it between pages.
pub fn recognize(
    engine: &Engine,
    scan: Scan,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<Extracted, IngestError> {
    recognize_with(scan, engines(), stop, &|image| {
        engine.read_page(image, PAGE_TIMEOUT)
    })
}

/// One page read: its text and its words' average certainty.
type PageRead = Result<(String, Option<f64>), IngestError>;
type ReadPage<'a> = dyn Fn(&PageImage) -> PageRead + Sync + 'a;

pub(crate) fn recognize_with(
    scan: Scan,
    workers: usize,
    stop: &(dyn Fn() -> bool + Sync),
    read: &ReadPage<'_>,
) -> Result<Extracted, IngestError> {
    let images = &scan.images;
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<PageRead>>> =
        Mutex::new((0..images.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers.clamp(1, images.len().max(1)) {
            s.spawn(|| loop {
                if stop() {
                    return;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(image) = images.get(i) else { return };
                let r = read(image);
                if let Ok(mut all) = results.lock() {
                    if let Some(slot) = all.get_mut(i) {
                        *slot = Some(r);
                    }
                }
            });
        }
    });
    if stop() {
        return Err(IngestError::Timeout);
    }
    let results = results
        .into_inner()
        .map_err(|_| IngestError::Ocr("pages".to_owned()))?;

    // Page by page, in order: the text a page carried itself (a mixed PDF), or what was read.
    let mut pages: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (page, text) in &scan.text_pages {
        pages.entry(*page).or_default().push(text.trim().to_owned());
    }
    let mut low_pages: Vec<u32> = Vec::new();
    let mut failed_pages: Vec<u32> = Vec::new();
    let mut first_error = None;
    let mut read_any = !scan.text_pages.is_empty();
    for (image, result) in images.iter().zip(results) {
        match result {
            Some(Ok((text, conf))) => {
                read_any = true;
                pages
                    .entry(image.page)
                    .or_default()
                    .push(text.trim().to_owned());
                if conf.is_some_and(|c| c < LOW_CONFIDENCE) && !low_pages.contains(&image.page) {
                    low_pages.push(image.page);
                }
            }
            Some(Err(e)) => {
                if !failed_pages.contains(&image.page) {
                    failed_pages.push(image.page);
                }
                first_error.get_or_insert(e);
            }
            None => {
                failed_pages.push(image.page);
                first_error.get_or_insert(IngestError::Ocr("page".to_owned()));
            }
        }
    }
    // Fail closed when nothing was read: an empty scan is no material. A mixed PDF's own
    // text is read whatever the engine did.
    if !read_any {
        return Err(first_error.unwrap_or(IngestError::Scanned));
    }
    let body = pages
        .values()
        .map(|parts| {
            parts
                .iter()
                .filter(|t| !t.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut warnings = scan.warnings;
    warnings.insert(
        0,
        "הטקסט זוהה אוטומטית מסריקה (OCR), במחשב הזה. כדאי לעבור עליו מול המקור: שמות ומספרים עלולים להיקרא לא נכון.".to_owned(),
    );
    failed_pages.sort_unstable();
    failed_pages.dedup();
    if !failed_pages.is_empty() {
        let list: Vec<String> = failed_pages.iter().map(u32::to_string).collect();
        warnings.push(format!(
            "עמודים {} לא נקראו (הקריאה נכשלה או לקחה יותר מדי זמן), והם חסרים בטקסט. כדאי להוסיף אותם ידנית, או לסרוק אותם שוב.",
            list.join(", ")
        ));
    }
    low_pages.sort_unstable();
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
        margins: scan.margins,
        metadata: scan.metadata,
        warnings,
        pages: scan.pages,
        ocr: true,
    })
}

/// A mixed PDF where no engine is installed: its digital pages, and a warning naming the
/// scanned pages that were not read.
pub(crate) fn without_engine(scan: Scan) -> Extracted {
    let mut missing: Vec<u32> = scan.images.iter().map(|i| i.page).collect();
    missing.dedup();
    let list: Vec<String> = missing.iter().map(u32::to_string).collect();
    let mut warnings = scan.warnings;
    warnings.push(format!(
        "עמודים {} סרוקים, ולא נקראו: זיהוי טקסט מסריקות לא מותקן במחשב הזה. כדאי להוסיף אותם ידנית.",
        list.join(", ")
    ));
    let body = scan
        .text_pages
        .iter()
        .map(|(_, t)| t.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Extracted {
        format: scan.format,
        body,
        margins: scan.margins,
        metadata: scan.metadata,
        warnings,
        pages: scan.pages,
        ocr: false,
    }
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

    #[test]
    fn rows_cut_short_are_skipped() {
        // A word row missing its text column, an empty line, a lone level: nothing invented.
        let tsv = "5\t1\t1\t1\t1\t1\t0\t0\t1\t1\t90\n\n5\n5\t1\t1\t1\t1\t1\t0\t0\t1\t1\t90\tבדוי\textra\n";
        let (text, conf) = from_tsv(tsv);
        assert_eq!(text, "בדוי");
        assert_eq!(conf, Some(90.0));
        assert_eq!(from_tsv("5\t1\t1"), (String::new(), None));
    }

    fn scan_of(pages: &[u32]) -> Scan {
        Scan {
            format: crate::Format::Pdf,
            images: pages
                .iter()
                .map(|p| PageImage {
                    page: *p,
                    dpi: None,
                    bytes: vec![u8::try_from(*p).unwrap_or(0)],
                })
                .collect(),
            metadata: Vec::new(),
            warnings: Vec::new(),
            pages: u32::try_from(pages.len()).unwrap_or(0),
            text_pages: Vec::new(),
            margins: String::new(),
        }
    }

    fn page_text(image: &PageImage) -> Result<(String, Option<f64>), IngestError> {
        // Later pages answer first: the order must still be the pages'.
        std::thread::sleep(Duration::from_millis(u64::from(10 - image.page.min(9)) * 5));
        Ok((format!("עמוד בדוי {}", image.page), Some(90.0)))
    }

    #[test]
    fn pages_read_side_by_side_come_back_in_order() {
        let out = recognize_with(scan_of(&[1, 2, 3, 4, 5]), 4, &|| false, &page_text).unwrap();
        assert_eq!(
            out.body,
            "עמוד בדוי 1\n\nעמוד בדוי 2\n\nעמוד בדוי 3\n\nעמוד בדוי 4\n\nעמוד בדוי 5"
        );
        assert!(out.ocr);
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    }

    #[test]
    fn a_page_that_fails_leaves_out_that_page_only_and_says_which() {
        let read = |image: &PageImage| {
            if image.page == 2 {
                Err(IngestError::Timeout)
            } else {
                page_text(image)
            }
        };
        let out = recognize_with(scan_of(&[1, 2, 3]), 2, &|| false, &read).unwrap();
        assert_eq!(out.body, "עמוד בדוי 1\n\nעמוד בדוי 3");
        assert!(
            out.warnings
                .iter()
                .any(|w| w.starts_with("עמודים 2 לא נקראו")),
            "{:?}",
            out.warnings
        );
    }

    #[test]
    fn no_page_read_refuses_the_scan() {
        let read = |_: &PageImage| Err(IngestError::Timeout);
        assert_eq!(
            recognize_with(scan_of(&[1, 2]), 2, &|| false, &read),
            Err(IngestError::Timeout)
        );
    }

    #[test]
    fn a_lock_meanwhile_stops_between_pages() {
        let stop = std::sync::atomic::AtomicBool::new(false);
        let read = |image: &PageImage| {
            stop.store(true, Ordering::Release);
            page_text(image)
        };
        let r = recognize_with(
            scan_of(&[1, 2, 3, 4]),
            1,
            &|| stop.load(Ordering::Acquire),
            &read,
        );
        assert_eq!(r, Err(IngestError::Timeout));
    }

    #[test]
    fn a_mixed_pdf_puts_its_own_text_and_the_read_pages_in_page_order() {
        let mut scan = scan_of(&[2, 4]);
        scan.text_pages = vec![(1, "שער דיגיטלי".to_owned()), (3, "עמוד אמצעי".to_owned())];
        scan.pages = 4;
        let out = recognize_with(scan, 2, &|| false, &page_text).unwrap();
        assert_eq!(
            out.body,
            "שער דיגיטלי\n\nעמוד בדוי 2\n\nעמוד אמצעי\n\nעמוד בדוי 4"
        );
        // Every scanned page failed: the digital pages still come through, with a warning.
        let mut scan = scan_of(&[2]);
        scan.text_pages = vec![(1, "שער דיגיטלי".to_owned())];
        let out = recognize_with(scan, 1, &|| false, &|_| Err(IngestError::Timeout)).unwrap();
        assert_eq!(out.body, "שער דיגיטלי");
        assert!(out
            .warnings
            .iter()
            .any(|w| w.starts_with("עמודים 2 לא נקראו")));
    }

    #[test]
    fn the_engines_leave_a_core_free() {
        let n = engines();
        assert!((1..=MAX_ENGINES).contains(&n));
    }
}
