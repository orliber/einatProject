//! Scans and photos through the real OCR engine. Invented letter only (`fixtures/README.md`).
//!
//! Needs Tesseract with Hebrew: the system's (Linux CI installs it), or the bundled one in
//! `DV_TEST_OCR_DIR` (Windows CI, run inside the real sandbox). Without it these tests say so
//! and pass, unless `DV_REQUIRE_OCR=1`, which CI sets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use dv_ingest::ocr::Engine;
use dv_ingest::worker;
use dv_ingest::{import, Format, IngestError, Outcome};
use pdf_extract::{Dictionary, Document, Object, Stream};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn engine() -> Option<Engine> {
    // CI on Windows: the engine as bundled with the app (windows/fetch-ocr.ps1).
    if let Some(dir) = std::env::var_os("DV_TEST_OCR_DIR") {
        let dir = PathBuf::from(dir);
        let exe = dir.join(if cfg!(windows) {
            "tesseract.exe"
        } else {
            "tesseract"
        });
        return Some(Engine::new(exe, dir.join("tessdata")));
    }
    for data in [
        "/usr/share/tesseract-ocr/5/tessdata",
        "/usr/share/tesseract-ocr/4.00/tessdata",
    ] {
        let data = PathBuf::from(data);
        let exe = PathBuf::from("/usr/bin/tesseract");
        if exe.is_file() && data.join("heb.traineddata").is_file() {
            return Some(Engine::new(exe, data));
        }
    }
    assert!(
        std::env::var_os("DV_REQUIRE_OCR").is_none(),
        "DV_REQUIRE_OCR is set but Tesseract with Hebrew is not installed"
    );
    eprintln!("Tesseract with Hebrew is not installed: OCR tests skipped");
    None
}

/// What the letter says, as read back. Every name must come through whole: a misread name
/// would slip past the filter.
fn assert_letter(body: &str) {
    for words in [
        "מכתב הפניה לאבחון פסיכולוגי",
        "נועם",
        "כפר ורדים",
        "רותם ועידו",
        "גן הדקל",
        "הגננת מיכל",
        "אבנר שטרן",
    ] {
        assert!(body.contains(words), "{words:?} not read in:\n{body}");
    }
}

/// A one-page PDF whose page is `image` (an image XObject dictionary and its data).
fn pdf_with_image(image: Dictionary, data: Vec<u8>, width_pt: i64, height_pt: i64) -> Vec<u8> {
    pdf_pages(image, data, width_pt, height_pt, None)
}

/// A digital page of typed text (`cover`) in front of the scanned page, when given.
fn pdf_pages(
    mut image: Dictionary,
    data: Vec<u8>,
    width_pt: i64,
    height_pt: i64,
    cover: Option<&str>,
) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    image.set("Type", Object::Name(b"XObject".to_vec()));
    image.set("Subtype", Object::Name(b"Image".to_vec()));
    let img_id = doc.add_object(Stream::new(image, data));
    let content = format!("q {width_pt} 0 0 {height_pt} 0 0 cm /Im0 Do Q").into_bytes();
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
    let mut xobjects = Dictionary::new();
    xobjects.set("Im0", Object::Reference(img_id));
    let mut resources = Dictionary::new();
    resources.set("XObject", Object::Dictionary(xobjects));
    let pages_id = doc.new_object_id();
    let mut page = Dictionary::new();
    page.set("Type", Object::Name(b"Page".to_vec()));
    page.set("Parent", Object::Reference(pages_id));
    page.set(
        "MediaBox",
        Object::Array(vec![0.into(), 0.into(), width_pt.into(), height_pt.into()]),
    );
    page.set("Resources", Object::Dictionary(resources));
    page.set("Contents", Object::Reference(content_id));
    let page_id = doc.add_object(page);
    let mut kids = vec![Object::Reference(page_id)];
    if let Some(text) = cover {
        let mut font = Dictionary::new();
        font.set("Type", Object::Name(b"Font".to_vec()));
        font.set("Subtype", Object::Name(b"Type1".to_vec()));
        font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
        let font_id = doc.add_object(font);
        let mut fonts = Dictionary::new();
        fonts.set("F1", Object::Reference(font_id));
        let mut resources = Dictionary::new();
        resources.set("Font", Object::Dictionary(fonts));
        let content = format!("BT /F1 14 Tf 72 700 Td ({text}) Tj ET").into_bytes();
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(pages_id));
        page.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), width_pt.into(), height_pt.into()]),
        );
        page.set("Resources", Object::Dictionary(resources));
        page.set("Contents", Object::Reference(content_id));
        kids.insert(0, Object::Reference(doc.add_object(page)));
    }
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Count", i64::try_from(kids.len()).unwrap());
    pages.set("Kids", Object::Array(kids));
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// The scanned letter, after a typed cover page when `cover` is given.
fn gray_pdf_with(cover: Option<&str>) -> Vec<u8> {
    let png = fixture("scan-letter.png");
    let (mut w, mut h, mut idat) = (0i64, 0i64, Vec::new());
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        let kind = &png[i + 4..i + 8];
        let data = &png[i + 8..i + 8 + len];
        match kind {
            b"IHDR" => {
                w = i64::from(u32::from_be_bytes(data[0..4].try_into().unwrap()));
                h = i64::from(u32::from_be_bytes(data[4..8].try_into().unwrap()));
                assert_eq!((data[8], data[9]), (8, 0), "8-bit gray expected");
            }
            b"IDAT" => idat.extend_from_slice(data),
            _ => {}
        }
        i += 12 + len;
    }
    let mut parms = Dictionary::new();
    parms.set("Predictor", 15);
    parms.set("Colors", 1);
    parms.set("BitsPerComponent", 8);
    parms.set("Columns", w);
    let mut image = Dictionary::new();
    image.set("Width", w);
    image.set("Height", h);
    image.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
    image.set("BitsPerComponent", 8);
    image.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    image.set("DecodeParms", Object::Dictionary(parms));
    pdf_pages(image, idat, 595, 842, cover)
}

/// The letter as a scanner's gray PDF: the PNG's compressed rows, as PDF producers store them.
fn gray_pdf() -> Vec<u8> {
    gray_pdf_with(None)
}

/// The letter as an office scanner's black-and-white PDF (fax compression).
fn fax_pdf() -> Vec<u8> {
    let tif = fixture("scan-letter-fax.tif");
    let u16_at = |at: usize| u16::from_le_bytes(tif[at..at + 2].try_into().unwrap());
    let u32_at = |at: usize| u32::from_le_bytes(tif[at..at + 4].try_into().unwrap());
    assert_eq!(&tif[0..4], b"II*\0");
    let ifd = u32_at(4) as usize;
    let mut tags = std::collections::HashMap::new();
    for k in 0..usize::from(u16_at(ifd)) {
        let e = ifd + 2 + k * 12;
        let typ = u16_at(e + 2);
        let val = if typ == 3 {
            u32::from(u16_at(e + 8))
        } else {
            u32_at(e + 8)
        };
        tags.insert(u16_at(e), val);
    }
    let (w, h) = (i64::from(tags[&256]), i64::from(tags[&257]));
    let (off, len) = (tags[&273] as usize, tags[&279] as usize);
    let mut parms = Dictionary::new();
    parms.set("K", -1);
    parms.set("Columns", w);
    parms.set("Rows", h);
    // TIFF "BlackIsZero" fax data shows its coded black runs as white: PDF's BlackIs1.
    parms.set("BlackIs1", tags.get(&262) == Some(&1));
    let mut image = Dictionary::new();
    image.set("Width", w);
    image.set("Height", h);
    image.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
    image.set("BitsPerComponent", 1);
    image.set("Filter", Object::Name(b"CCITTFaxDecode".to_vec()));
    image.set("DecodeParms", Object::Dictionary(parms));
    pdf_with_image(image, tif[off..off + len].to_vec(), 595, 842)
}

#[test]
fn photo_of_a_page_is_read() {
    let Some(engine) = engine() else { return };
    let out = import(
        None,
        Some(&engine),
        "צילום.png",
        &fixture("scan-letter.png"),
    )
    .unwrap();
    assert_eq!(out.format, Format::Image);
    assert!(out.ocr);
    assert_letter(&out.body);
    assert!(out.warnings[0].contains("OCR"), "{:?}", out.warnings);
}

#[test]
fn sideways_photo_is_turned_and_read() {
    let Some(engine) = engine() else { return };
    let out = import(
        None,
        Some(&engine),
        "x.png",
        &fixture("scan-letter-sideways.png"),
    )
    .unwrap();
    assert_letter(&out.body);
}

#[test]
fn scanned_pdf_gray_is_read() {
    let Some(engine) = engine() else { return };
    let out = import(None, Some(&engine), "scan.pdf", &gray_pdf()).unwrap();
    assert_eq!(out.format, Format::Pdf);
    assert_letter(&out.body);
}

#[test]
fn scanned_pdf_fax_is_read() {
    let Some(engine) = engine() else { return };
    let out = import(None, Some(&engine), "scan.pdf", &fax_pdf()).unwrap();
    assert_letter(&out.body);
}

#[test]
fn worker_hands_back_page_images_not_text() {
    let exe = Path::new(env!("CARGO_BIN_EXE_dv-ingest-worker"));
    let r = worker::run(exe, "scan.pdf", &gray_pdf(), worker::DEFAULT_TIMEOUT).unwrap();
    let Outcome::Scan(scan) = r else {
        panic!("a scan expected")
    };
    assert_eq!(scan.images.len(), 1);
    assert_eq!(scan.images[0].dpi, Some(300));
    assert!(scan.images[0].bytes.starts_with(b"P5\n2480 3508\n255\n"));
}

#[test]
fn scan_without_the_engine_says_so() {
    assert_eq!(
        import(None, None, "scan.pdf", &fax_pdf()),
        Err(IngestError::OcrMissing)
    );
    assert_eq!(
        dv_ingest::extract(&fixture("scan-letter.png"), "x.png"),
        Err(IngestError::Scanned)
    );
}

/// Typed on the digital cover page of the mixed PDF (invented, like the letter).
const COVER: &str = "Cover page for the referral of a fictional child, sent by the clinic";

#[test]
fn a_mixed_pdf_keeps_its_digital_page_and_sends_only_the_scanned_one_to_ocr() {
    // D-160: the decision is per page. Before, the cover's text decided for the whole file
    // and the scanned letter behind it disappeared without a word.
    let bytes = gray_pdf_with(Some(COVER));
    let Ok(Outcome::Scan(scan)) = dv_ingest::read(&bytes, "mixed.pdf") else {
        panic!("a scan expected")
    };
    assert_eq!(scan.pages, 2);
    assert_eq!(scan.images.len(), 1);
    assert_eq!(scan.images[0].page, 2);
    assert_eq!(scan.text_pages.len(), 1);
    assert_eq!(scan.text_pages[0].0, 1);
    assert!(
        scan.text_pages[0].1.contains("fictional child"),
        "{:?}",
        scan.text_pages
    );

    let Some(engine) = engine() else { return };
    let out = import(None, Some(&engine), "mixed.pdf", &bytes).unwrap();
    assert!(out.ocr);
    let cover = out.body.find("fictional child").expect("the cover is read");
    let letter = out.body.find("מכתב הפניה").expect("the letter is read");
    assert!(cover < letter, "pages out of order:\n{}", out.body);
    assert_letter(&out.body);
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("חלק מה-PDF דיגיטלי")),
        "{:?}",
        out.warnings
    );
}

#[test]
fn a_mixed_pdf_without_the_engine_keeps_its_digital_page_and_names_the_scanned_one() {
    let out = import(None, None, "mixed.pdf", &gray_pdf_with(Some(COVER))).unwrap();
    assert!(!out.ocr);
    assert!(out.body.contains("fictional child"), "{}", out.body);
    assert!(
        out.warnings.iter().any(|w| w.contains("עמודים 2 סרוקים")),
        "{:?}",
        out.warnings
    );
}

#[test]
fn the_engine_reads_from_a_folder_named_in_hebrew() {
    // P-09: the app may be installed under a Hebrew folder name. Its language data is given
    // to the engine relative to its working folder, so only ASCII is on its command line.
    let Some(engine) = engine() else { return };
    let Some(source) = engine_data_dir() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("כספת האבחון").join("ocr");
    let data = root.join("tessdata");
    std::fs::create_dir_all(&data).unwrap();
    for name in ["heb.traineddata", "eng.traineddata", "osd.traineddata"] {
        if source.join(name).is_file() {
            std::fs::copy(source.join(name), data.join(name)).unwrap();
        }
    }
    let exe = root.join(if cfg!(windows) {
        "tesseract.exe"
    } else {
        "tesseract"
    });
    std::fs::copy(engine.program(), &exe).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // `locate` looks next to the app: the folder above `ocr`.
    let located = Engine::locate(&root.parent().unwrap().join("app")).expect("found");
    let out = import(
        None,
        Some(&located),
        "צילום.png",
        &fixture("scan-letter.png"),
    )
    .unwrap();
    assert_letter(&out.body);
}

/// The folder of the engine's language data used by these tests.
fn engine_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DV_TEST_OCR_DIR") {
        return Some(PathBuf::from(dir).join("tessdata"));
    }
    [
        "/usr/share/tesseract-ocr/5/tessdata",
        "/usr/share/tesseract-ocr/4.00/tessdata",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|d| d.join("heb.traineddata").is_file())
}
