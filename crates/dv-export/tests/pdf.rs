//! The PDF report, opened by an independent reader (Poppler's `pdfinfo` and `pdftotext`):
//! it opens only with its password, Hebrew reads back in logical order, report text is only
//! ever text (it can never become PDF operators), and the file carries no metadata.
//!
//! lopdf cannot read the pages of an encrypted file with object streams, so these tests use
//! Poppler when it is installed. Like the crate's own PDF tests, they are skipped when the
//! computer has no Hebrew font, or no Poppler. Fabricated data only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{report_with, sample, system_font};
use dv_export::{render_pdf, ExportError, PdfFonts, Report};

const PASSWORD: &str = "נהר-ענן-42-שקד-אורן";

/// Text that would be PDF syntax if it were written as a literal string.
const HOSTILE: &str = "x) Tj ET BT /F1 72 Tf (y) Tj ET q 0 0 m 600 800 l S Q BT (\\) << /JS (z) >>";

/// Write `pdf` to a scratch file and run a Poppler tool on it with the password.
fn poppler(tool: &str, pdf: &[u8], password: &str, extra: &[&str]) -> Option<(bool, String)> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "dv-export-{}-{}.pdf",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, pdf).unwrap();
    let out = Command::new(tool)
        .arg("-upw")
        .arg(password)
        .args(extra)
        .arg(&path)
        .args(if tool == "pdftotext" {
            vec!["-"]
        } else {
            vec![]
        })
        .output();
    let _ = std::fs::remove_file(&path);
    let out = out.ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// The font and Poppler, or `None` (the test is then skipped).
fn tools() -> Option<Vec<u8>> {
    let Some(font) = system_font() else {
        eprintln!("no Hebrew font on this computer; skipped");
        return None;
    };
    if Command::new("pdfinfo").arg("-v").output().is_err()
        || Command::new("pdftotext").arg("-v").output().is_err()
    {
        eprintln!("Poppler (pdfinfo, pdftotext) is not installed; skipped");
        return None;
    }
    Some(font)
}

fn pdf_of(report: &Report, font: &[u8]) -> Vec<u8> {
    render_pdf(
        report,
        PdfFonts {
            regular: font,
            bold: None,
        },
        PASSWORD,
    )
    .unwrap()
}

/// `pdfinfo`'s fields, by name.
fn info(pdf: &[u8]) -> std::collections::HashMap<String, String> {
    let (ok, text) = poppler("pdfinfo", pdf, PASSWORD, &[]).unwrap();
    assert!(ok, "pdfinfo could not open the file");
    text.lines()
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// The text as a reader extracts it, without the direction-embedding marks it adds.
fn text(pdf: &[u8]) -> String {
    let (ok, text) = poppler("pdftotext", pdf, PASSWORD, &["-enc", "UTF-8"]).unwrap();
    assert!(ok, "pdftotext could not open the file");
    text.chars()
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .collect()
}

#[test]
fn it_opens_only_with_its_password_and_reads_back_in_hebrew() {
    let Some(font) = tools() else { return };
    let mut r = sample();
    r.confidentiality = "חסוי – מידע רפואי".into();
    let pdf = pdf_of(&r, &font);
    let (ok, _) = poppler("pdfinfo", &pdf, "סיסמה-שגויה-99", &[]).unwrap();
    assert!(!ok, "opened with a wrong password");
    let fields = info(&pdf);
    assert!(fields["Encrypted"].starts_with("yes"), "{fields:?}");
    assert!(fields["Encrypted"].contains("AES-256"), "{fields:?}");
    assert!(fields["Encrypted"].contains("change:no"), "{fields:?}");
    let t = text(&pdf);
    // Logical (reading) order: drawn right to left, read back as it was written.
    for line in [
        "דוח אבחון פסיכולוגי",
        "ההורים פנו בשל קושי במעברים ובוויסות רגשי",
        "אלון בדוי",
        "חסוי",
    ] {
        assert!(t.contains(line), "{line:?} not read back in:\n{t}");
    }
}

/// Text that looks like PDF operators is drawn as text: the page count does not change and
/// the operators read back as words on the page.
#[test]
fn report_text_never_becomes_pdf_operators() {
    let Some(font) = tools() else { return };
    let plain = pdf_of(&report_with(|_| "x".to_owned()), &font);
    let hostile = pdf_of(&report_with(|_| HOSTILE.to_owned()), &font);
    let (a, b) = (info(&plain), info(&hostile));
    assert_eq!(a["Pages"], b["Pages"]);
    assert_eq!(b["JavaScript"], "no");
    assert_eq!(b["Form"], "none");
    let t = text(&hostile);
    assert!(
        t.contains("Tj ET BT /F1 72 Tf"),
        "operators not drawn as text:\n{t}"
    );
    assert!(t.contains("/JS"), "{t}");
}

/// A long report flows over several pages, each numbered "page n of N" with the right N.
#[test]
fn a_long_report_flows_over_numbered_pages() {
    let Some(font) = tools() else { return };
    let mut r = sample();
    r.parts[0].sections[0].paragraphs = (0..120)
        .map(|i| format!("פסקה בדויה מספר {i}: הילד שיתף פעולה לאורך כל המפגש והפגין סקרנות."))
        .collect();
    let pdf = pdf_of(&r, &font);
    let pages: usize = info(&pdf)["Pages"].parse().unwrap();
    assert!(pages > 3, "{pages} pages");
    let t = text(&pdf);
    // The reader moves spaces around the numbers (they are left-to-right runs).
    let squeezed: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    for n in [1, pages] {
        assert!(
            squeezed.contains(&format!("עמוד{n}מתוך{pages}")),
            "page {n} of {pages} not numbered"
        );
    }
    // Every paragraph is there, the last one included.
    assert_eq!(t.matches("הילד שיתף פעולה לאורך כל המפגש").count(), 120);
    assert!(t.contains("119"), "the last paragraph is missing");
}

/// No author, producer, creator or dates in the file.
#[test]
fn the_pdf_carries_no_metadata() {
    let Some(font) = tools() else { return };
    let pdf = pdf_of(&sample(), &font);
    let fields = info(&pdf);
    for key in [
        "Title",
        "Author",
        "Creator",
        "Producer",
        "CreationDate",
        "ModDate",
    ] {
        assert!(!fields.contains_key(key), "{key}: {:?}", fields.get(key));
    }
    assert_eq!(fields["Metadata Stream"], "no");
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    assert!(doc.trailer.get(b"Info").is_err(), "an Info dictionary");
}

/// A file that is not a font, or a font without Hebrew letters, is refused with a sentence
/// she can act on.
#[test]
fn an_unusable_font_is_refused_clearly() {
    let not_a_font = PdfFonts {
        regular: b"not a font",
        bold: None,
    };
    assert!(matches!(
        render_pdf(&sample(), not_a_font, PASSWORD),
        Err(ExportError::Pdf(m)) if m.contains("גופן")
    ));
    let latin_only = Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf");
    let Ok(bytes) = std::fs::read(latin_only) else {
        return;
    };
    let fonts = PdfFonts {
        regular: &bytes,
        bold: None,
    };
    if let Err(ExportError::Pdf(m)) = render_pdf(&sample(), fonts, PASSWORD) {
        assert!(m.contains("עבריות"), "{m}");
    }
}
