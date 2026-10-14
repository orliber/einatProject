//! Hostile, damaged and unusual documents through the same entry points the worker runs.
//! Every fixture is built here, in memory (no binary file is committed), from invented text.
//!
//! What must hold: a damaged or hostile file ends in a clean error, with every size bounded
//! before memory is spent; Hebrew, right-to-left and mixed-direction text comes out exactly as
//! written; the old Hebrew code page round-trips and anything else is refused, not guessed; and
//! a PDF with no text layer is never imported as if it had one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use dv_ingest::{detect, extract, import, read, Format, IngestError, Outcome};

// ---------------------------------------------------------------- builders

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
const ODF_NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#;

/// A zip of `(name, bytes)` parts. ODF's `mimetype` is stored, as a reader expects.
fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let deflated = zip::write::SimpleFileOptions::default();
        let stored = deflated.compression_method(zip::CompressionMethod::Stored);
        for (name, content) in parts {
            zip.start_file(
                *name,
                if *name == "mimetype" {
                    stored
                } else {
                    deflated
                },
            )
            .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn docx(paragraphs: &[&str]) -> Vec<u8> {
    let body: String = paragraphs
        .iter()
        .map(|p| {
            format!(
                "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                xml_escape(p)
            )
        })
        .collect();
    let doc =
        format!("<?xml version=\"1.0\"?><w:document {W_NS}><w:body>{body}</w:body></w:document>");
    zip_of(&[("word/document.xml", doc.as_bytes())])
}

fn odt(paragraphs: &[&str]) -> Vec<u8> {
    let body: String = paragraphs
        .iter()
        .map(|p| format!("<text:p>{}</text:p>", xml_escape(p)))
        .collect();
    let content = format!(
        "<?xml version=\"1.0\"?><office:document-content {ODF_NS}><office:body><office:text>{body}</office:text></office:body></office:document-content>"
    );
    zip_of(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", content.as_bytes()),
    ])
}

/// RTF the way Word writes Unicode text: `\uN?` for every character outside ASCII.
fn rtf(paragraphs: &[&str]) -> Vec<u8> {
    let mut out = String::from(r"{\rtf1\ansi\ansicpg1255\uc1{\fonttbl{\f0\fbidi Arial;}}\f0 ");
    for (i, p) in paragraphs.iter().enumerate() {
        if i > 0 {
            out.push_str("\\par ");
        }
        for c in p.chars() {
            match c {
                '\\' | '{' | '}' => {
                    out.push('\\');
                    out.push(c);
                }
                ' '..='~' => out.push(c),
                _ => {
                    let n = u32::from(c);
                    assert!(n < 0x8000, "test text stays in the BMP's lower half");
                    out.push_str(&format!("\\u{n}?"));
                }
            }
        }
    }
    out.push_str("\\par}");
    out.into_bytes()
}

/// Windows-1255, written out independently of the reader's table.
fn to_cp1255(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| {
            let n = u32::from(c);
            let b = match c {
                '\0'..='\x7f' => n,
                '\u{05D0}'..='\u{05EA}' => 0xE0 + n - 0x05D0,
                '\u{05B0}'..='\u{05C3}' => 0xC0 + n - 0x05B0,
                '\u{05F0}'..='\u{05F4}' => 0xD4 + n - 0x05F0,
                '₪' => 0xA4,
                '–' => 0x96,
                '…' => 0x85,
                _ => panic!("{c:?} has no Windows-1255 byte"),
            };
            u8::try_from(b).unwrap()
        })
        .collect()
}

/// A PDF of the given objects (object 1 is the catalog), with a correct cross-reference table.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
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

fn stream(dict: &str, content: &[u8]) -> Vec<u8> {
    let mut o = format!("<< {dict} /Length {} >>\nstream\n", content.len()).into_bytes();
    o.extend(content);
    o.extend(b"\nendstream");
    o
}

/// One A4 page that draws a single image XObject (object 4) and has no text at all.
fn image_pdf(image_dict: &str, image: &[u8]) -> Vec<u8> {
    let content = b"q 595 0 0 842 0 0 cm /Im1 Do Q";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
        stream(&format!("/Type /XObject /Subtype /Image {image_dict}"), image),
        stream("", content),
    ])
}

/// `pages` A4 pages, each with the given content stream, Helvetica available.
fn text_pdf(pages: usize, content: &str) -> Vec<u8> {
    let mut objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        Vec::new(), // the page tree, filled in below
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    let mut kids = Vec::new();
    for _ in 0..pages {
        let page = objs.len() + 1;
        kids.push(format!("{page} 0 R"));
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", page + 1).into_bytes());
        objs.push(stream("", content.as_bytes()));
    }
    objs[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    )
    .into_bytes();
    pdf(&objs)
}

/// A JPEG that is a header and nothing else: enough for the size checks, never decoded here.
fn jpeg_header(w: u16, h: u16) -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8];
    j.extend(h.to_be_bytes());
    j.extend(w.to_be_bytes());
    j.extend([1, 1, 0x11, 0, 0xFF, 0xD9]);
    j
}

/// Runs `f`, which must neither panic nor take long.
fn calmly<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let r = catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| panic!("{what}: panicked"));
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{what}: took {:?}",
        started.elapsed()
    );
    r
}

// ---------------------------------------------------------------- archives

#[test]
fn every_truncation_of_a_word_or_odt_file_is_refused_cleanly() {
    let p = ["תצפית בדויה: הילד משחק בחצר.", "שורה שנייה."];
    for (name, full) in [("a.docx", docx(&p)), ("a.odt", odt(&p))] {
        assert!(extract(&full, name).is_ok(), "the whole {name} reads");
        // From the 4-byte zip signature on, every prefix lacks the central directory.
        for len in 4..full.len() {
            let r = calmly(name, || extract(&full[..len], name));
            assert!(
                r.is_err(),
                "{name} cut at {len} of {} was accepted",
                full.len()
            );
        }
    }
}

#[test]
fn an_odt_zip_bomb_is_refused() {
    // 50 MB of one repeated letter compresses to a few kilobytes.
    let huge = "א".repeat(25 * 1024 * 1024);
    let bytes = odt(&[huge.as_str()]);
    assert!(bytes.len() < dv_ingest::MAX_INPUT_BYTES);
    assert_eq!(extract(&bytes, "a.odt"), Err(IngestError::TooLarge));
}

#[test]
fn many_parts_under_their_own_limit_still_hit_the_total_budget() {
    // Each part is 30 MB (under the 40 MB per-part limit); five of them pass the 120 MB budget
    // that the whole archive gets. Compressed once and copied raw, so the test is quick.
    let part = format!(
        "<?xml version=\"1.0\"?><w:hdr {W_NS}><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:hdr>",
        "א".repeat(15 * 1024 * 1024)
    );
    let one = zip_of(&[("word/header1.xml", part.as_bytes())]);
    let mut source = zip::ZipArchive::new(Cursor::new(&one)).unwrap();
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let doc = format!(
            "<?xml version=\"1.0\"?><w:document {W_NS}><w:body><w:p><w:r><w:t>גוף</w:t></w:r></w:p></w:body></w:document>"
        );
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(doc.as_bytes()).unwrap();
        for i in 1..=5 {
            let f = source.by_index_raw(0).unwrap();
            zip.raw_copy_file_rename(f, format!("word/header{i}.xml"))
                .unwrap();
        }
        zip.finish().unwrap();
    }
    let bytes = buf.into_inner();
    assert!(bytes.len() < dv_ingest::MAX_INPUT_BYTES);
    let r = calmly("budget", || extract(&bytes, "a.docx"));
    assert_eq!(r, Err(IngestError::TooLarge));
}

#[test]
fn an_archive_with_too_many_entries_is_refused() {
    let doc = format!(
        "<?xml version=\"1.0\"?><w:document {W_NS}><w:body><w:p><w:r><w:t>גוף</w:t></w:r></w:p></w:body></w:document>"
    );
    let names: Vec<String> = (0..5_001).map(|i| format!("x/{i}")).collect();
    let mut parts: Vec<(&str, &[u8])> = vec![("word/document.xml", doc.as_bytes())];
    parts.extend(names.iter().map(|n| (n.as_str(), &b""[..])));
    let bytes = zip_of(&parts);
    assert!(matches!(
        extract(&bytes, "a.docx"),
        Err(IngestError::Corrupt(_))
    ));
}

#[test]
fn a_password_protected_zip_entry_says_so() {
    use zip::unstable::write::FileOptionsExt;
    let doc = format!(
        "<?xml version=\"1.0\"?><w:document {W_NS}><w:body><w:p><w:r><w:t>סודי</w:t></w:r></w:p></w:body></w:document>"
    );
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .with_deprecated_encryption(b"invented")
            .unwrap();
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(doc.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    assert_eq!(
        extract(&buf.into_inner(), "a.docx"),
        Err(IngestError::Encrypted)
    );
}

// ---------------------------------------------------------------- RTF

#[test]
fn deeply_nested_rtf_is_refused_without_recursion() {
    let depth = 100_000;
    let mut b = br"{\rtf1 ".to_vec();
    b.extend(std::iter::repeat_n(b'{', depth));
    b.extend(b"x");
    b.extend(std::iter::repeat_n(b'}', depth + 1));
    let r = calmly("nesting", || extract(&b, "a.rtf"));
    assert!(matches!(r, Err(IngestError::Corrupt(_))), "{r:?}");
}

#[test]
fn odd_rtf_control_words_end_cleanly() {
    for (what, src) in [
        (
            "huge \\bin",
            r"{\rtf1 before\bin2147483647 after}".to_owned(),
        ),
        ("negative \\bin", r"{\rtf1 a\bin-5 b}".to_owned()),
        (
            "lone surrogate",
            r"{\rtf1 \u-10240?\u-9216? text}".to_owned(),
        ),
        ("huge \\uc", r"{\rtf1\uc99999 \u1488 abc}".to_owned()),
        (
            "number overflow",
            r"{\rtf1\fs99999999999999999999 text}".to_owned(),
        ),
        ("cut in a hex byte", r"{\rtf1\ansicpg1255 \'e".to_owned()),
        ("cut after a backslash", r"{\rtf1 text\".to_owned()),
        ("only opening", "{\\rtf1".to_owned()),
        ("unbalanced close", r"{\rtf1 a}}}}".to_owned()),
    ] {
        let _ = calmly(what, || extract(src.as_bytes(), "a.rtf"));
    }
    // Every prefix of a valid file, too.
    let full = rtf(&["שלום [בדוי]", "שורה ב"]);
    for len in 5..full.len() {
        let _ = calmly("rtf prefix", || extract(&full[..len], "a.rtf"));
    }
}

// ---------------------------------------------------------------- Hebrew and direction

/// Hebrew with points, geresh and gershayim, maqaf, numbers, an age, Latin test names and
/// brackets: everything a report mixes into one right-to-left line.
const MIXED: [&str; 3] = [
    "הילד (בן 5:4) קיבל ציון 102 במבחן WISC-V; אמר \"לא רוצה\" – וחזר לשחק.",
    "ד\"ר בדויה המליצה על ריפוי בעיסוק ו-ADHD נשלל. ג'ירפה, שָׁלוֹם, בית־ספר.",
    "סך הכול 3/4 מהמשימות [50%] הושלמו; 1.5 שעות, ₪ 0.",
];

#[test]
fn hebrew_and_mixed_direction_text_comes_out_exactly_as_written() {
    let want = MIXED.join("\n");
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("a.docx", docx(&MIXED)),
        ("a.odt", odt(&MIXED)),
        ("a.rtf", rtf(&MIXED)),
        ("a.txt", want.as_bytes().to_vec()),
        (
            "a.txt",
            [b"\xEF\xBB\xBF".as_slice(), want.as_bytes()].concat(),
        ),
        ("a.txt", want.replace('\n', "\r\n").into_bytes()),
    ];
    for (name, bytes) in cases {
        let out = extract(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(out.body, want, "{name}");
        assert!(
            !out.warnings.iter().any(|w| w.contains("כיווניות")),
            "{name}: nothing was invisible: {:?}",
            out.warnings
        );
    }
}

#[test]
fn direction_controls_are_removed_and_the_letters_around_them_kept() {
    // Isolates, embeddings, overrides and marks around a Latin name and inside a Hebrew word:
    // gone, with a warning, and the visible text untouched.
    let with = "ציון \u{2066}WISC-V\u{2069} של \u{202B}נו\u{200F}עם\u{202C} \u{202E}102\u{202C}.";
    let without = "ציון WISC-V של נועם 102.";
    for (name, bytes) in [
        ("a.docx", docx(&[with])),
        ("a.odt", odt(&[with])),
        ("a.rtf", rtf(&[with])),
        ("a.txt", with.as_bytes().to_vec()),
    ] {
        let out = extract(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(out.body, without, "{name}");
        assert!(
            out.warnings.iter().any(|w| w.contains("כיווניות")),
            "{name}"
        );
    }
}

#[test]
fn visual_order_lines_turn_logical_and_back() {
    use dv_ingest::rtl::reverse_line;
    for logical in [
        "גיל 5:4. הופנה",
        "ציון 102 (טווח ממוצע).",
        "במבחן WISC V התקבל",
        "הבנה\t85\t92",
        "ADHD",
        "שלום",
        "",
    ] {
        assert_eq!(reverse_line(&reverse_line(logical)), logical);
    }
}

// ---------------------------------------------------------------- code pages

#[test]
fn windows_1255_round_trips_every_hebrew_letter_and_point() {
    let letters: String = ('\u{05D0}'..='\u{05EA}').collect();
    let text = format!("{letters}\nשָׁלוֹם בְּגַן ג׳ירפה ד״ר – ₪ 120…\nWISC 5:4");
    let out = extract(&to_cp1255(&text), "old.txt").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out.body, text);
    assert_eq!(out.format, Format::Text);
    assert!(out.warnings.iter().any(|w| w.contains("1255")));
    // The same bytes under another name are not guessed at.
    assert_eq!(
        detect(&to_cp1255(&text), "old.doc"),
        Err(IngestError::Unsupported)
    );
    assert_eq!(
        detect(&to_cp1255(&text), "old"),
        Err(IngestError::Unsupported)
    );
}

#[test]
fn utf8_wins_over_the_old_code_page_and_its_mark_is_dropped() {
    let text = "תצפית בדויה: הילד משחק לבד.";
    // UTF-8 Hebrew is all 0xD7 0x90–0xAA: it must never be read as Windows-1255.
    let plain = extract(text.as_bytes(), "a.txt").unwrap();
    assert_eq!(plain.body, text);
    assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);
    let marked = extract(
        &[b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat(),
        "a.txt",
    )
    .unwrap();
    assert_eq!(marked.body, text);
    assert!(!marked.body.starts_with('\u{FEFF}'));
}

#[test]
fn utf16_is_refused_rather_than_misread() {
    let text = "תצפית בדויה: הילד משחק לבד.";
    let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    for (what, bytes) in [
        ("UTF-16LE with mark", [&[0xFF, 0xFE][..], &le].concat()),
        ("UTF-16BE with mark", [&[0xFE, 0xFF][..], &be].concat()),
        ("UTF-16LE without mark", le.clone()),
        ("UTF-16BE without mark", be.clone()),
    ] {
        for name in ["a.txt", "a.TXT", "a"] {
            let r = extract(&bytes, name);
            assert_eq!(r, Err(IngestError::Unsupported), "{what} as {name}: {r:?}");
        }
    }
}

#[test]
fn a_byte_with_no_meaning_in_windows_1255_refuses_the_file() {
    let mut bytes = to_cp1255("שלום לכולם");
    for bad in [0x81u8, 0x8A, 0x9F, 0xD9, 0xDF, 0xFB, 0xFC, 0xFF] {
        bytes.push(bad);
        let r = extract(&bytes, "a.txt");
        assert_eq!(r, Err(IngestError::Unsupported), "byte {bad:#04x}");
        bytes.pop();
    }
}

// ---------------------------------------------------------------- PDF without text

#[test]
fn a_pdf_with_no_text_layer_is_never_imported_as_text() {
    // An image of a page and nothing else: the worker hands it back for OCR.
    let jpeg = jpeg_header(1000, 1400);
    let bytes = image_pdf(
        "/Width 1000 /Height 1400 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode",
        &jpeg,
    );
    match read(&bytes, "scan.pdf") {
        Ok(Outcome::Scan(s)) => {
            assert_eq!(s.format, Format::Pdf);
            assert_eq!(s.images.len(), 1);
            assert_eq!(s.images[0].bytes, jpeg, "the image is handed on untouched");
            assert_eq!(s.images[0].page, 1);
        }
        other => panic!("expected page images, got {other:?}"),
    }
    // Without OCR it is refused, and with no engine installed the message says so.
    assert_eq!(extract(&bytes, "scan.pdf"), Err(IngestError::Scanned));
    assert_eq!(
        import(None, None, "scan.pdf", &bytes),
        Err(IngestError::OcrMissing)
    );
}

#[test]
fn a_pdf_with_only_a_few_letters_or_blank_pages_is_refused() {
    // A stray line (a stamp, a page number) is not a text layer: it is treated as a scan,
    // and with no image to read the file is refused rather than imported half-empty.
    for (what, bytes) in [
        (
            "a short line",
            text_pdf(1, "BT /F1 12 Tf 72 700 Td (Invented stamp 12) Tj ET"),
        ),
        (
            "white space",
            text_pdf(2, "BT /F1 12 Tf 72 700 Td (     ) Tj ET"),
        ),
        ("empty pages", text_pdf(3, "")),
    ] {
        assert_eq!(
            extract(&bytes, "a.pdf"),
            Err(IngestError::Scanned),
            "{what}"
        );
        assert_eq!(
            import(None, None, "a.pdf", &bytes),
            Err(IngestError::Scanned),
            "{what}"
        );
    }
}

#[test]
fn a_scan_longer_than_the_page_limit_is_refused() {
    let bytes = text_pdf(dv_ingest::scan::MAX_PAGES + 1, "");
    assert_eq!(extract(&bytes, "a.pdf"), Err(IngestError::TooManyPages));
}

#[test]
fn image_size_bombs_in_a_pdf_are_refused_before_unpacking() {
    // Declared far past the pixel limit.
    let jpeg = jpeg_header(1000, 1000);
    let big = image_pdf(
        "/Width 20000 /Height 20000 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode",
        &jpeg,
    );
    assert_eq!(
        calmly("declared", || read(&big, "a.pdf")),
        Err(IngestError::TooLarge)
    );
    // Declared small, but the compressed stream unpacks to 64 MB: inflation stops at the size
    // the image can have and the file is refused.
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let zeros = vec![0u8; 1024 * 1024];
    for _ in 0..64 {
        z.write_all(&zeros).unwrap();
    }
    let bomb = z.finish().unwrap();
    assert!(bomb.len() < 1024 * 1024);
    let bytes = image_pdf(
        "/Width 1000 /Height 1000 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode",
        &bomb,
    );
    assert_eq!(
        calmly("inflated", || read(&bytes, "a.pdf")),
        Err(IngestError::TooLarge)
    );
}

#[test]
fn raw_pixels_in_every_colour_model_unpack_exactly_or_are_refused() {
    // A page-sized image (smaller ones are not looked at) whose rows repeat `pattern`, and
    // what the OCR engine is handed for it.
    const SIDE: usize = 300;
    let image = |dict: &str, pattern: &[u8], bytes: usize| {
        let data: Vec<u8> = pattern.iter().copied().cycle().take(bytes).collect();
        let dict = format!("/Width {SIDE} /Height {SIDE} {dict}");
        match read(&image_pdf(&dict, &data), "a.pdf") {
            Ok(Outcome::Scan(s)) => s.images.into_iter().next().map(|i| i.bytes),
            other => panic!("{dict}: expected page images, got {other:?}"),
        }
    };
    let pnm = |kind: &str, pattern: &[u8], bytes: usize| {
        let mut out = format!("{kind}\n{SIDE} {SIDE}\n255\n").into_bytes();
        out.extend(pattern.iter().copied().cycle().take(bytes));
        Some(out)
    };
    let px = SIDE * SIDE;
    // A palette of two colours; index 5 is past its end and is drawn black.
    assert_eq!(
        image(
            "/BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 1 <FF000000FF00>]",
            &[1, 5],
            px
        ),
        pnm("P6", &[0, 255, 0, 0, 0, 0], px * 3)
    );
    assert_eq!(
        image(
            "/BitsPerComponent 8 /ColorSpace [/Indexed /DeviceGray 1 <1020>]",
            &[1, 9],
            px
        ),
        pnm("P5", &[0x20, 0], px)
    );
    assert_eq!(
        image(
            "/BitsPerComponent 8 /ColorSpace [/Indexed /DeviceCMYK 0 <0A141E05>]",
            &[0],
            px
        ),
        pnm("P6", &[240, 230, 220], px * 3)
    );
    assert_eq!(
        image(
            "/BitsPerComponent 8 /ColorSpace /DeviceCMYK",
            &[10, 20, 30, 5],
            px * 4
        ),
        pnm("P6", &[240, 230, 220], px * 3)
    );
    // 2-bit gray, four pixels a byte: 3, 1, 0, 0 scaled to 0–255.
    assert_eq!(
        image(
            "/BitsPerComponent 2 /ColorSpace /DeviceGray",
            &[0b1101_0000],
            px / 4
        ),
        pnm("P5", &[255, 85, 0, 0], px)
    );
    // An indexed palette over an indexed palette: drawn black, never read past its end.
    assert_eq!(
        image(
            "/BitsPerComponent 4 /ColorSpace [/Indexed [/Indexed /DeviceRGB 0 <000000>] 0 <00>]",
            &[0x12],
            px / 2
        ),
        pnm("P6", &[0], px * 3)
    );
    // Rows cut short, with or without PNG row filters: refused, nothing guessed.
    let gray = format!("/Width {SIDE} /Height {SIDE} /BitsPerComponent 8 /ColorSpace /DeviceGray");
    assert!(matches!(
        calmly("cut short", || read(&image_pdf(&gray, &[1, 2, 3]), "a.pdf")),
        Err(IngestError::Corrupt(_))
    ));
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&vec![2u8; SIDE + 1 + SIDE / 2]).unwrap();
    let filtered = z.finish().unwrap();
    let dict =
        format!("{gray} /Filter /FlateDecode /DecodeParms << /Predictor 15 /Columns {SIDE} >>");
    assert!(matches!(
        calmly("filtered and cut short", || read(
            &image_pdf(&dict, &filtered),
            "a.pdf"
        )),
        Err(IngestError::Corrupt(_))
    ));
}

#[test]
fn photo_headers_with_impossible_sizes_are_refused() {
    let png = |w: u32, h: u32| {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b
    };
    assert_eq!(
        read(&png(u32::MAX, u32::MAX), "a.png"),
        Err(IngestError::TooLarge)
    );
    assert!(matches!(
        read(&png(0, 1000), "a.png"),
        Err(IngestError::Corrupt(_))
    ));
    assert!(matches!(
        read(&png(1000, 0), "a.png"),
        Err(IngestError::Corrupt(_))
    ));
    // Cut before the size: refused, not guessed.
    assert!(matches!(
        read(b"\x89PNG\r\n\x1a\n\0\0", "a.png"),
        Err(IngestError::Corrupt(_))
    ));
    assert_eq!(
        read(&jpeg_header(u16::MAX, u16::MAX), "a.jpg"),
        Err(IngestError::TooLarge)
    );
    // A JPEG whose segments never reach a frame header.
    assert!(matches!(
        read(
            &[0xFF, 0xD8, 0xFF, 0xE0, 0, 2, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            "a.jpg"
        ),
        Err(IngestError::Corrupt(_))
    ));
    // A photo is never text: without OCR it is refused.
    assert_eq!(
        extract(&png(1000, 1400), "a.png"),
        Err(IngestError::Scanned)
    );
    assert_eq!(
        import(None, None, "a.png", &png(1000, 1400)),
        Err(IngestError::OcrMissing)
    );
}
