//! Mutation fuzzing of every document reader (S3). Valid invented documents are damaged in
//! many ways (flipped bytes, cut short, repeated chunks, wild sizes) and read; the readers
//! may refuse, but must never panic and never run away. Deterministic, so a failure repeats:
//! the seed is printed. The nightly job raises the number of rounds with `DV_FUZZ_ROUNDS`.
//! (The real isolation is the worker process; this finds the bugs before they reach it.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

/// xorshift64*: small, fixed, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let deflated = zip::write::SimpleFileOptions::default();
        // The ODF "mimetype" entry is stored uncompressed: that is how a file is recognized.
        let stored = deflated.compression_method(zip::CompressionMethod::Stored);
        for (name, content) in parts {
            let opts = if *name == "mimetype" {
                stored
            } else {
                deflated
            };
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

/// (file name, bytes): one valid invented document of each kind.
fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    let docx = zip_of(&[
        (
            "word/document.xml",
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>הילד בדוי משחק בגן</w:t></w:r></w:p></w:body></w:document>"#,
        ),
        ("docProps/core.xml", "<cp:coreProperties/>"),
    ]);
    let odt = zip_of(&[
        ("mimetype", "application/vnd.oasis.opendocument.text"),
        (
            "content.xml",
            r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text><text:p>טקסט בדוי לבדיקה</text:p></office:text></office:body></office:document-content>"#,
        ),
    ]);
    let rtf = b"{\\rtf1\\ansi\\ansicpg1255{\\fonttbl{\\f0 Arial;}}\\f0 \\u1514?\\u1510? \\'e0\\'e1\\par second line}".to_vec();
    let pdf = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n4 0 obj<</Length 44>>stream\nBT /F1 12 Tf 20 100 Td (Invented text) Tj ET\nendstream endobj\n5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R/Size 6>>\n%%EOF\n".to_vec();
    let mut doc = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    doc.extend((0..2048u32).map(|i| (i * 7 % 251) as u8));
    let png = std::fs::read("tests/fixtures/scan-letter.png").unwrap_or_default();
    vec![
        ("a.docx", docx),
        ("a.odt", odt),
        ("a.rtf", rtf),
        ("a.pdf", pdf),
        ("a.doc", doc),
        ("a.txt", "תצפית בדויה: הילד משחק לבד.".as_bytes().to_vec()),
        ("a.png", png),
        ("b.docx", rich_docx()),
        ("b.odt", rich_odt()),
        ("b.rtf", WORD_RTF.as_bytes().to_vec()),
        ("b.txt", CP1255_TXT.to_vec()),
    ]
}

/// A Word document with what hides around the body: headers, footnotes, comments, properties,
/// tracked changes, hidden text, fields, a list and a table.
fn rich_docx() -> Vec<u8> {
    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let body = concat!(
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>פריט ברשימה</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>נצפה </w:t></w:r><w:del><w:r><w:delText>נמחק</w:delText></w:r></w:del><w:r><w:rPr><w:vanish/></w:rPr><w:t>מוסתר</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> DATE </w:instrText></w:r><w:r><w:t>שדה</w:t></w:r></w:p>"#,
        r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>מבחן</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>102</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let document =
        format!(r#"<?xml version="1.0"?><w:document {W}><w:body>{body}</w:body></w:document>"#);
    let header = format!("<w:hdr {W}><w:p><w:r><w:t>מכון בדוי</w:t></w:r></w:p></w:hdr>");
    let notes = format!(
        "<w:footnotes {W}><w:footnote><w:p><w:r><w:t>הערת שוליים בדויה</w:t></w:r></w:p></w:footnote></w:footnotes>"
    );
    let comments = format!("<w:comments {W}/>");
    zip_of(&[
        ("word/document.xml", document.as_str()),
        ("word/header1.xml", header.as_str()),
        ("word/footer1.xml", header.as_str()),
        ("word/footnotes.xml", notes.as_str()),
        ("word/comments.xml", comments.as_str()),
        (
            "docProps/core.xml",
            r#"<cp:coreProperties xmlns:cp="x" xmlns:dc="y"><dc:creator>יוצר בדוי</dc:creator></cp:coreProperties>"#,
        ),
        (
            "docProps/app.xml",
            "<Properties><Company>חברה בדויה</Company></Properties>",
        ),
    ])
}

/// An ODT with a header in its styles, properties, a comment, a note, tracked changes and a
/// hidden section.
fn rich_odt() -> Vec<u8> {
    const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#;
    let content = format!(
        r#"<?xml version="1.0"?><office:document-content {NS}><office:body><office:text><text:tracked-changes><text:changed-region><text:deletion><text:p>נמחק</text:p></text:deletion></text:changed-region></text:tracked-changes><text:p>גוף<text:s text:c="3"/>בדוי<office:annotation><text:p>הערה</text:p></office:annotation><text:note><text:note-body><text:p>הערה בסוף</text:p></text:note-body></text:note></text:p><text:section text:display="none"><text:p>מוסתר</text:p></text:section><table:table><table:table-row><table:table-cell><text:p>תא</text:p></table:table-cell></table:table-row></table:table></office:text></office:body></office:document-content>"#
    );
    let styles = format!(
        r#"<?xml version="1.0"?><office:document-styles {NS}><office:master-styles><style:master-page><style:header><text:p>כותרת בדויה</text:p></style:header></style:master-page></office:master-styles></office:document-styles>"#
    );
    let meta = format!(
        r#"<?xml version="1.0"?><office:document-meta {NS}><office:meta><meta:initial-creator>יוצרת בדויה</meta:initial-creator></office:meta></office:document-meta>"#
    );
    zip_of(&[
        ("mimetype", "application/vnd.oasis.opendocument.text"),
        ("content.xml", content.as_str()),
        ("styles.xml", styles.as_str()),
        ("meta.xml", meta.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest/>"),
    ])
}

/// RTF as Word writes it: a font table with charsets, an info group, a header, text as `\uN`
/// and as `\'xx`, a field, a table row, a skipped destination and a Western-font run.
const WORD_RTF: &str = r"{\rtf1\adeflang1037\ansi\ansicpg1255\uc1{\fonttbl{\f0\fbidi\froman\fcharset177 Times;}{\f1\fswiss\fcharset0 Arial;}}{\info{\title \'e3\'e5\'e7}{\author \'e1\'e3\'e5\'e9}}{\header \pard \'ee\'f8\'f4\'e0\'e4\par}{\*\generator Word;}\pard\rtlch\f0 \'e4\'e9\'ec\'e3 \u1513?\u1500?\u1493?\u1501? {\field{\*\fldinst DATE}{\fldrslt 1.1.2000}}\par\trowd\cellx1000 \'e0\cell 85\cell\row{\*\annotation \'e4\'f2\'f8\'e4}\par}";

/// "שלום, נועם – ₪ 12" in Windows-1255, as an old system would save a note.
const CP1255_TXT: &[u8] = &[
    0xF9, 0xEC, 0xE5, 0xED, b',', b' ', 0xF0, 0xE5, 0xF2, 0xED, b' ', 0x96, b' ', 0xA4, b' ', b'1',
    b'2',
];

fn mutate(rng: &mut Rng, original: &[u8]) -> Vec<u8> {
    let mut b = original.to_vec();
    if b.is_empty() {
        return b;
    }
    for _ in 0..=rng.below(4) {
        match rng.below(6) {
            0 => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(b.len());
                b[i] = rng.next() as u8;
            }
            2 => b.truncate(rng.below(b.len()) + 1),
            3 => {
                let start = rng.below(b.len());
                let end = (start + rng.below(64) + 1).min(b.len());
                let chunk = b[start..end].to_vec();
                for _ in 0..=rng.below(50) {
                    b.splice(start..start, chunk.clone());
                }
            }
            4 => {
                let i = rng.below(b.len());
                b.drain(i..(i + rng.below(32) + 1).min(b.len()));
                if b.is_empty() {
                    b.push(0);
                }
            }
            _ => {
                // A wild size field: four bytes of 0xFF or 0x00 somewhere.
                let i = rng.below(b.len());
                let v = if rng.below(2) == 0 { 0xFF } else { 0x00 };
                for k in i..(i + 4).min(b.len()) {
                    b[k] = v;
                }
            }
        }
    }
    b
}

#[test]
fn damaged_documents_never_panic_or_hang() {
    let rounds: u64 = std::env::var("DV_FUZZ_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let seed0: u64 = std::env::var("DV_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5eed_d0c5);
    for (name, original) in seeds() {
        // The undamaged seed must read (or be refused) without a panic as well.
        let clean = dv_ingest::read(&original, name);
        if [
            "a.docx", "a.odt", "a.rtf", "a.txt", "b.docx", "b.odt", "b.rtf", "b.txt",
        ]
        .contains(&name)
        {
            assert!(
                matches!(clean, Ok(dv_ingest::Outcome::Text(_))),
                "the undamaged {name} seed must read: {clean:?}"
            );
        }
        for round in 0..rounds {
            let seed = seed0 ^ round.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ name.len() as u64;
            let mut rng = Rng(seed | 1);
            let bytes = mutate(&mut rng, &original);
            let started = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| dv_ingest::read(&bytes, name)));
            assert!(
                result.is_ok(),
                "panic reading a damaged {name}: round {round}, seed {seed}, {} bytes",
                bytes.len()
            );
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "{name} took {:?} (round {round}, seed {seed})",
                started.elapsed()
            );
        }
    }
}
