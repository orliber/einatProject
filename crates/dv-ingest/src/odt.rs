//! ODT (LibreOffice, Google Docs "OpenDocument"): the body, with the same rules as DOCX.
//!
//! Comments (`office:annotation`) and the text of deleted tracked changes are skipped; headers
//! and footers (in `styles.xml`) go to `margins`; file properties (`meta.xml`) go to
//! `metadata`. Footnotes are kept, at the end. The archive is read with the same limits.

use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::docx::{corrupt, properties, read_part, MAX_ENTRIES, MAX_TOTAL_BYTES};
use crate::{Extracted, Format, IngestError};

/// The `mimetype` entry an ODF text document starts with (ODF 1.2 §3.3).
pub(crate) const MIMETYPE: &[u8] = b"mimetypeapplication/vnd.oasis.opendocument.text";

/// True when the zip's first entry is the ODT `mimetype` (stored uncompressed, at offset 30).
pub(crate) fn is_odt(bytes: &[u8]) -> bool {
    bytes
        .get(30..)
        .is_some_and(|rest| rest.starts_with(MIMETYPE))
}

pub(crate) fn extract(bytes: &[u8]) -> Result<Extracted, IngestError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(corrupt)?;
    if zip.len() > MAX_ENTRIES {
        return Err(corrupt("too many entries"));
    }
    let mut budget = MAX_TOTAL_BYTES;
    let mut warnings = Vec::new();

    // A password-protected ODT keeps its parts encrypted and says so in the manifest.
    if read_part(&mut zip, "META-INF/manifest.xml", &mut budget)?
        .is_some_and(|m| m.contains("encryption-data"))
    {
        return Err(IngestError::Encrypted);
    }
    let content = read_part(&mut zip, "content.xml", &mut budget)?
        .ok_or_else(|| corrupt("not an OpenDocument text"))?;
    let doc = walk(&content, Part::Body)?;
    let mut body = doc.text;
    if !doc.notes.trim().is_empty() {
        body.push_str("\n\n");
        body.push_str(doc.notes.trim());
    }
    if doc.deleted {
        warnings.push("במסמך יש שינויים במעקב. יובא הנוסח הסופי, בלי הטקסט שנמחק.".to_owned());
    }
    if doc.comments {
        warnings.push("ההערות במסמך לא יובאו.".to_owned());
    }
    if doc.hidden {
        warnings.push("במסמך יש טקסט מוסתר. הוא לא יובא.".to_owned());
    }

    let mut margins = String::new();
    if let Some(styles) = read_part(&mut zip, "styles.xml", &mut budget)? {
        margins = walk(&styles, Part::Margins)?.text.trim().to_owned();
        if !margins.is_empty() {
            margins.push('\n');
            warnings.push(
                "הכותרת העליונה/התחתונה לא יובאה (יש בה לרוב שם, ת\"ז ופרטי קשר).".to_owned(),
            );
        }
    }

    let metadata = match read_part(&mut zip, "meta.xml", &mut budget)? {
        Some(xml) => properties(
            &xml,
            &[
                "creator",
                "initial-creator",
                "title",
                "subject",
                "keyword",
                "description",
                "printed-by",
                "user-defined",
            ],
        )?,
        None => Vec::new(),
    };

    Ok(Extracted {
        format: Format::Odt,
        body,
        margins,
        metadata,
        warnings,
        pages: 0,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// `content.xml`: the text body.
    Body,
    /// `styles.xml`: only what is inside headers and footers.
    Margins,
}

struct Walked {
    text: String,
    notes: String,
    deleted: bool,
    comments: bool,
    hidden: bool,
}

/// `<text:section text:display="none">` (or shown only on a condition).
fn is_hidden_section(e: &quick_xml::events::BytesStart<'_>) -> bool {
    e.attributes().flatten().any(|a| {
        a.key.local_name().as_ref() == "display" && matches!(a.value.as_ref(), "none" | "condition")
    })
}

/// Text of an ODF part. Paragraphs and headings are lines, list items get "- ", a table row
/// is one line with cells separated by tabs (like the DOCX reader).
fn walk(xml: &str, part: Part) -> Result<Walked, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Walked {
        text: String::new(),
        notes: String::new(),
        deleted: false,
        comments: false,
        hidden: false,
    };
    // Inside a paragraph or heading: only there is text content (ODF 1.2 §6.1.2).
    let mut para_depth = 0u32;
    // Inside something that is never imported (a comment, deleted text, a note's citation).
    let mut skip_depth = 0u32;
    // Inside a header or footer (Margins) or inside the office:text body (Body).
    let mut keep_depth = 0u32;
    let mut note_depth = 0u32;
    let mut cell_depth = 0u32;

    loop {
        match reader.read_event().map_err(corrupt)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name = e.local_name();
                let name = name.as_ref();
                if skip_depth > 0 {
                    skip_depth += 1;
                    continue;
                }
                let keeps = match part {
                    Part::Body => name == "text" && keep_depth == 0,
                    Part::Margins => matches!(
                        name,
                        "header"
                            | "footer"
                            | "header-left"
                            | "footer-left"
                            | "header-first"
                            | "footer-first"
                    ),
                };
                if keeps {
                    keep_depth += 1;
                    continue;
                }
                if keep_depth == 0 {
                    continue;
                }
                match name {
                    "annotation" => {
                        out.comments = true;
                        skip_depth = 1;
                    }
                    "tracked-changes" => {
                        out.deleted = true;
                        skip_depth = 1;
                    }
                    // Hidden text, a hidden paragraph, or a section set not to display: never
                    // imported, like hidden text in Word.
                    "hidden-text" | "hidden-paragraph" => {
                        out.hidden = true;
                        skip_depth = 1;
                    }
                    "section" if is_hidden_section(&e) => {
                        out.hidden = true;
                        skip_depth = 1;
                    }
                    "p" | "h" => para_depth += 1,
                    // The note's number in the text; its body is kept apart.
                    "note-citation" => skip_depth = 1,
                    "note-body" => note_depth += 1,
                    "list-item" => {
                        let buf = if note_depth > 0 {
                            &mut out.notes
                        } else {
                            &mut out.text
                        };
                        buf.push_str("- ");
                    }
                    "table-cell" => cell_depth += 1,
                    _ => {}
                }
            }
            Event::Empty(e) => {
                if skip_depth > 0 || keep_depth == 0 {
                    continue;
                }
                let buf = if note_depth > 0 {
                    &mut out.notes
                } else {
                    &mut out.text
                };
                match e.local_name().as_ref() {
                    "s" => {
                        let n = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.local_name().as_ref() == "c")
                            .and_then(|a| a.value.as_ref().parse().ok())
                            .unwrap_or(1usize)
                            .min(200);
                        buf.push_str(&" ".repeat(n));
                    }
                    "tab" => buf.push('\t'),
                    "line-break" => buf.push('\n'),
                    "soft-page-break" => {}
                    // An empty paragraph: a blank line, as in the document.
                    "p" | "h" if cell_depth == 0 => buf.push('\n'),
                    _ => {}
                }
            }
            Event::End(e) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                    continue;
                }
                if keep_depth == 0 {
                    continue;
                }
                let name = e.local_name();
                let name = name.as_ref();
                let closes_keep = match part {
                    Part::Body => name == "text",
                    Part::Margins => matches!(
                        name,
                        "header"
                            | "footer"
                            | "header-left"
                            | "footer-left"
                            | "header-first"
                            | "footer-first"
                    ),
                };
                if closes_keep && note_depth == 0 && cell_depth == 0 {
                    keep_depth -= 1;
                    if part == Part::Margins {
                        out.text.push('\n');
                    }
                    continue;
                }
                if matches!(name, "p" | "h") {
                    para_depth = para_depth.saturating_sub(1);
                }
                let in_note = note_depth > 0;
                let buf = if in_note {
                    &mut out.notes
                } else {
                    &mut out.text
                };
                match name {
                    "p" | "h" if cell_depth > 0 => buf.push(' '),
                    "p" | "h" => buf.push('\n'),
                    "note-body" => note_depth = note_depth.saturating_sub(1),
                    "table-cell" => {
                        cell_depth = cell_depth.saturating_sub(1);
                        buf.truncate(buf.trim_end_matches(' ').len());
                        buf.push('\t');
                    }
                    "table-row" => {
                        buf.truncate(buf.trim_end_matches('\t').len());
                        buf.push('\n');
                    }
                    _ => {}
                }
            }
            Event::Text(t) if skip_depth == 0 && keep_depth > 0 && para_depth > 0 => {
                let buf = if note_depth > 0 {
                    &mut out.notes
                } else {
                    &mut out.text
                };
                // Runs of white space are one space; line breaks come only from elements.
                let mut last_space = buf.ends_with(' ');
                for c in t.xml10_content().chars() {
                    if c.is_whitespace() {
                        if !last_space {
                            buf.push(' ');
                        }
                        last_space = true;
                    } else {
                        buf.push(c);
                        last_space = false;
                    }
                }
            }
            Event::GeneralRef(r) if skip_depth == 0 && keep_depth > 0 => {
                let buf = if note_depth > 0 {
                    &mut out.notes
                } else {
                    &mut out.text
                };
                if let Some(c) = r.resolve_char_ref().map_err(corrupt)? {
                    buf.push(c);
                } else {
                    buf.push_str(match r.xml10_content().as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::{detect, extract as extract_any};

    const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#;

    /// An ODT in memory: `mimetype` first and stored, as ODF requires.
    fn build(content: &str, styles: &str, meta: &str) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated = zip::write::SimpleFileOptions::default();
            zip.start_file("mimetype", stored)
                .unwrap_or_else(|e| panic!("{e}"));
            zip.write_all(b"application/vnd.oasis.opendocument.text")
                .unwrap_or_else(|e| panic!("{e}"));
            for (name, body) in [
                ("content.xml", content),
                ("styles.xml", styles),
                ("meta.xml", meta),
            ] {
                zip.start_file(name, deflated)
                    .unwrap_or_else(|e| panic!("{e}"));
                zip.write_all(body.as_bytes())
                    .unwrap_or_else(|e| panic!("{e}"));
            }
            zip.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        buf.into_inner()
    }

    fn fixture() -> Vec<u8> {
        let content = format!(
            r#"<?xml version="1.0"?><office:document-content {NS}><office:body><office:text>
<text:tracked-changes><text:changed-region><text:deletion><text:p>טקסט שנמחק</text:p></text:deletion></text:changed-region></text:tracked-changes>
<text:h>סיכום ביקור</text:h>
<text:p>שורה
    שנשברה בקובץ<text:hidden-text>שם מוסתר</text:hidden-text></text:p>
<text:section text:name="פנימי" text:display="none"><text:p>הערה פנימית מוסתרת</text:p></text:section>
<text:p>נצפה קושי<text:s text:c="2"/>במעברים &amp; ויסות.<office:annotation><text:p>הערה פנימית</text:p></office:annotation><text:note><text:note-citation>1</text:note-citation><text:note-body><text:p>לפי דיווח הגננת.</text:p></text:note-body></text:note></text:p>
<text:list><text:list-item><text:p>משחק סימבולי</text:p></text:list-item></text:list>
<table:table><table:table-row><table:table-cell><text:p>מבחן</text:p></table:table-cell><table:table-cell><text:p>ציון</text:p></table:table-cell></table:table-row></table:table>
</office:text></office:body></office:document-content>"#
        );
        let styles = format!(
            r#"<?xml version="1.0"?><office:document-styles {NS}><office:master-styles><style:master-page><style:header><text:p>מכון בדוי · ד"ר ישראלה בדויה</text:p></style:header><style:footer><text:p>עמוד 1</text:p></style:footer></style:master-page></office:master-styles></office:document-styles>"#
        );
        let meta = format!(
            r#"<?xml version="1.0"?><office:document-meta {NS}><office:meta><meta:initial-creator>ישראלה בדויה</meta:initial-creator><dc:title>סיכום</dc:title></office:meta></office:document-meta>"#
        );
        build(&content, &styles, &meta)
    }

    #[test]
    fn an_odt_is_recognised_by_its_bytes_not_its_name() {
        let bytes = fixture();
        assert!(is_odt(&bytes));
        assert_eq!(detect(&bytes, "report.docx"), Ok(Format::Odt));
    }

    #[test]
    fn body_only_with_notes_at_the_end_and_everything_else_aside() {
        let out = extract_any(&fixture(), "סיכום.odt").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(out.format, Format::Odt);
        let body = &out.body;
        assert!(body.contains("סיכום ביקור\n"), "{body}");
        // White space from the file's own line breaks is one space (ODF 1.2 §6.1.2).
        assert!(body.contains("שורה שנשברה בקובץ\n"), "{body}");
        assert!(body.contains("נצפה קושי  במעברים & ויסות."), "{body}");
        assert!(body.contains("- משחק סימבולי"), "{body}");
        assert!(body.contains("מבחן\tציון"), "{body}");
        assert!(body.trim_end().ends_with("לפי דיווח הגננת."), "{body}");
        for gone in [
            "טקסט שנמחק",
            "הערה פנימית",
            "שם מוסתר",
            "מכון בדוי",
            "עמוד 1",
            "ישראלה",
        ] {
            assert!(!body.contains(gone), "{gone} in the body");
        }
        assert!(out.margins.contains("מכון בדוי") && out.margins.contains("עמוד 1"));
        assert!(out
            .metadata
            .iter()
            .any(|(k, v)| k == "initial-creator" && v == "ישראלה בדויה"));
        assert_eq!(out.warnings.len(), 4, "{:?}", out.warnings);
        assert!(out.warnings.iter().any(|w| w.contains("מוסתר")));
    }

    #[test]
    fn a_password_protected_odt_says_so() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, body) in [
                ("mimetype", &b"application/vnd.oasis.opendocument.text"[..]),
                ("META-INF/manifest.xml", &b"<manifest:encryption-data/>"[..]),
                ("content.xml", &b"\x01\x02 not xml"[..]),
            ] {
                zip.start_file(name, stored)
                    .unwrap_or_else(|e| panic!("{e}"));
                zip.write_all(body).unwrap_or_else(|e| panic!("{e}"));
            }
            zip.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(extract(&buf.into_inner()), Err(IngestError::Encrypted));
    }

    #[test]
    fn an_empty_document_is_refused() {
        let bytes = build("", "", "");
        assert_eq!(extract_any(&bytes, "x.odt"), Err(crate::IngestError::Empty));
    }
}
