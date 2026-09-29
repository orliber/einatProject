//! DOCX: the body (with tables, text boxes, footnotes), not what hides around it.
//!
//! Deleted tracked changes, field codes, hidden text and duplicate fallback content are
//! skipped. Headers and footers go to `margins`; file properties go to `metadata`. Comments
//! are not imported. The archive is read with limits, so a "zip bomb" cannot exhaust memory.

use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::{Extracted, Format, IngestError};

pub(crate) const MAX_ENTRIES: usize = 5_000;
const MAX_PART_BYTES: u64 = 40 * 1024 * 1024;
pub(crate) const MAX_TOTAL_BYTES: u64 = 120 * 1024 * 1024;

pub(crate) fn corrupt(e: impl std::fmt::Display) -> IngestError {
    IngestError::Corrupt(e.to_string())
}

type Archive<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

/// Read one part, bounded by its own limit and by what is left of the total budget.
pub(crate) fn read_part(
    zip: &mut Archive<'_>,
    name: &str,
    budget: &mut u64,
) -> Result<Option<String>, IngestError> {
    let mut file = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(zip::result::ZipError::UnsupportedArchive(_)) => return Err(IngestError::Encrypted),
        Err(e) => return Err(corrupt(e)),
    };
    let limit = MAX_PART_BYTES.min(*budget);
    let mut buf = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(corrupt)?;
    let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
    if len > limit {
        return Err(IngestError::TooLarge);
    }
    *budget -= len;
    String::from_utf8(buf).map(Some).map_err(corrupt)
}

pub(crate) fn extract(bytes: &[u8]) -> Result<Extracted, IngestError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(corrupt)?;
    if zip.len() > MAX_ENTRIES {
        return Err(corrupt("too many entries"));
    }
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    let mut budget = MAX_TOTAL_BYTES;
    let mut warnings = Vec::new();

    let document = read_part(&mut zip, "word/document.xml", &mut budget)?
        .ok_or_else(|| corrupt("not a Word document"))?;
    let doc = paragraphs(&document)?;
    let mut body = doc.text;
    for part in ["word/footnotes.xml", "word/endnotes.xml"] {
        if let Some(xml) = read_part(&mut zip, part, &mut budget)? {
            let notes = paragraphs(&xml)?.text;
            if !notes.trim().is_empty() {
                body.push_str("\n\n");
                body.push_str(notes.trim());
            }
        }
    }
    if doc.deleted {
        warnings.push("במסמך יש שינויים במעקב. יובא הנוסח הסופי, בלי הטקסט שנמחק.".to_owned());
    }
    if doc.hidden {
        warnings.push("במסמך יש טקסט מוסתר. הוא לא יובא.".to_owned());
    }

    let mut margin_parts: Vec<&String> = names
        .iter()
        .filter(|n| {
            (n.starts_with("word/header") || n.starts_with("word/footer")) && n.ends_with(".xml")
        })
        .collect();
    margin_parts.sort();
    let mut margins = String::new();
    for part in margin_parts {
        if let Some(xml) = read_part(&mut zip, part, &mut budget)? {
            let t = paragraphs(&xml)?.text;
            if !t.trim().is_empty() && !margins.contains(t.trim()) {
                margins.push_str(t.trim());
                margins.push('\n');
            }
        }
    }
    if !margins.trim().is_empty() {
        warnings
            .push("הכותרת העליונה/התחתונה לא יובאה (יש בה לרוב שם, ת\"ז ופרטי קשר).".to_owned());
    }
    if names.iter().any(|n| n == "word/comments.xml") {
        warnings.push("ההערות (Comments) במסמך לא יובאו.".to_owned());
    }

    let mut metadata = Vec::new();
    for (part, fields) in [
        (
            "docProps/core.xml",
            &[
                "creator",
                "lastModifiedBy",
                "title",
                "subject",
                "keywords",
                "description",
            ][..],
        ),
        ("docProps/app.xml", &["Company", "Manager"][..]),
    ] {
        if let Some(xml) = read_part(&mut zip, part, &mut budget)? {
            metadata.extend(properties(&xml, fields)?);
        }
    }

    Ok(Extracted {
        format: Format::Docx,
        body,
        margins,
        metadata,
        warnings,
        pages: 0,
    })
}

/// Text extracted from one WordprocessingML part.
struct PartText {
    text: String,
    deleted: bool,
    hidden: bool,
}

/// Walk a WordprocessingML part and keep what a reader of the printed page would see.
fn paragraphs(xml: &str) -> Result<PartText, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut skip_depth = 0u32; // inside mc:Fallback, w:del, w:instrText…
    let mut in_run_props = false;
    let mut run_hidden = false;
    let mut deleted = false;
    let mut hidden = false;
    let mut cell_depth = 0u32;
    // Where the current paragraph starts: a numbered or bulleted item gets "- " there.
    let mut para_start = 0usize;

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
                match name {
                    "Fallback" | "instrText" | "delInstrText" => skip_depth = 1,
                    "del" | "delText" => {
                        deleted = true;
                        skip_depth = 1;
                    }
                    "r" => run_hidden = false,
                    "p" => para_start = out.len(),
                    // List items: keep them apart and recognisable ("- ", like the source).
                    "numPr" if !out[para_start..].starts_with("- ") => {
                        out.insert_str(para_start, "- ")
                    }
                    "tc" => cell_depth += 1,
                    "rPr" => in_run_props = true,
                    "vanish" | "specVanish" if in_run_props => run_hidden = on(&e),
                    "t" => in_text = true,
                    _ => {}
                }
            }
            Event::Empty(e) => {
                if skip_depth > 0 {
                    continue;
                }
                match e.local_name().as_ref() {
                    "vanish" | "specVanish" if in_run_props => run_hidden = on(&e),
                    "ta" if !in_run_props => out.push('\t'),
                    "br" | "cr" => out.push('\n'),
                    "noBreakHyphen" => out.push('-'),
                    _ => {}
                }
            }
            Event::End(e) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                    continue;
                }
                match e.local_name().as_ref() {
                    "t" => in_text = false,
                    "rPr" => in_run_props = false,
                    // Inside a table a row is one line: cells are separated by tabs.
                    "p" if cell_depth > 0 => out.push(' '),
                    "p" => out.push('\n'),
                    "tc" => {
                        cell_depth = cell_depth.saturating_sub(1);
                        out.truncate(out.trim_end_matches(' ').len());
                        out.push('\t');
                    }
                    "tr" => {
                        out.truncate(out.trim_end_matches('\t').len());
                        out.push('\n');
                    }
                    _ => {}
                }
            }
            Event::Text(t) if in_text && skip_depth == 0 => {
                if run_hidden {
                    hidden = true;
                } else {
                    out.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) if in_text && skip_depth == 0 && !run_hidden => {
                if let Some(c) = r.resolve_char_ref().map_err(corrupt)? {
                    out.push(c);
                } else {
                    out.push_str(match r.xml10_content().as_ref() {
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
    Ok(PartText {
        text: out,
        deleted,
        hidden,
    })
}

/// `<w:vanish/>` is on unless `w:val` says otherwise.
fn on(e: &quick_xml::events::BytesStart<'_>) -> bool {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == "val")
        .is_none_or(|a| !matches!(a.value.as_ref(), "0" | "false" | "off"))
}

/// Simple text properties (`<dc:creator>…</dc:creator>`).
pub(crate) fn properties(xml: &str, fields: &[&str]) -> Result<Vec<(String, String)>, IngestError> {
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(corrupt)? {
            Event::Eof => break,
            Event::Start(e) => {
                let local = e.local_name().as_ref().to_owned();
                current = fields.contains(&local.as_str()).then_some(local);
            }
            Event::Text(t) => {
                if let Some(field) = &current {
                    let v = t.xml10_content().trim().to_owned();
                    if !v.is_empty() {
                        out.push((field.clone(), v));
                    }
                }
            }
            Event::End(_) => current = None,
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

    /// Build a DOCX in memory (fixtures are never committed as files).
    pub(crate) fn build(parts: &[(&str, String)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, content) in parts {
                zip.start_file(*name, opts)
                    .unwrap_or_else(|e| panic!("{e}"));
                zip.write_all(content.as_bytes())
                    .unwrap_or_else(|e| panic!("{e}"));
            }
            zip.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        buf.into_inner()
    }

    pub(crate) fn para(text: &str) -> String {
        format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
    }

    pub(crate) fn document(body: &str) -> String {
        format!("<?xml version=\"1.0\"?><w:document {W}><w:body>{body}</w:body></w:document>")
    }

    #[test]
    fn body_only_with_warnings_and_metadata() {
        let body = [
            para("סיכום ביקור במכון להתפתחות הילד"),
            "<w:p><w:r><w:t>נצפה קושי </w:t></w:r><w:del><w:r><w:delText>קשה מאוד </w:delText></w:r></w:del><w:r><w:t>במעברים &amp; ויסות.</w:t></w:r></w:p>".to_owned(),
            "<w:p><w:r><w:rPr><w:vanish/></w:rPr><w:t>טקסט מוסתר</w:t></w:r><w:r><w:t>גלוי</w:t></w:r></w:p>".to_owned(),
            "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> DATE </w:instrText></w:r><w:r><w:t>שדה</w:t></w:r></w:p>".to_owned(),
            "<mc:AlternateContent><mc:Choice><w:p><w:r><w:t>תיבת טקסט</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:t>תיבת טקסט</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>".to_owned(),
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>מבחן</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>ציון</w:t></w:r></w:p></w:tc></w:tr></w:tbl>".to_owned(),
        ]
        .concat();
        let bytes = build(&[
            ("word/document.xml", document(&body)),
            ("word/header1.xml", format!("<w:hdr {W}>{}</w:hdr>", para("מרפאה פרטית · טל' כלשהו"))),
            ("word/comments.xml", format!("<w:comments {W}/>")),
            ("docProps/core.xml", "<cp:coreProperties xmlns:cp=\"x\" xmlns:dc=\"y\"><dc:creator>יוצר בדוי</dc:creator><cp:lastModifiedBy>עורך בדוי</cp:lastModifiedBy></cp:coreProperties>".to_owned()),
        ]);
        let out = crate::extract(&bytes, "a.docx").unwrap_or_else(|e| panic!("{e}"));
        assert!(
            out.body.contains("נצפה קושי במעברים & ויסות."),
            "{}",
            out.body
        );
        assert!(!out.body.contains("קשה מאוד"), "deleted text");
        assert!(!out.body.contains("מוסתר") && out.body.contains("גלוי"));
        assert!(!out.body.contains("DATE"));
        assert_eq!(
            out.body.matches("תיבת טקסט").count(),
            1,
            "fallback duplicate"
        );
        assert!(out.body.contains("מבחן\tציון"), "{:?}", out.body);
        assert!(!out.body.contains("מרפאה"), "header stays out of the body");
        assert!(out.margins.contains("מרפאה פרטית"));
        assert_eq!(
            out.metadata,
            vec![
                ("creator".to_owned(), "יוצר בדוי".to_owned()),
                ("lastModifiedBy".to_owned(), "עורך בדוי".to_owned())
            ]
        );
        assert_eq!(out.warnings.len(), 4, "{:?}", out.warnings);
    }

    #[test]
    fn list_items_stay_separate_and_marked() {
        let item = |t: &str| {
            format!("<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>{t}</w:t></w:r></w:p>")
        };
        let body = [para("המלצות:"), item("טיפול בתקשורת"), item("הדרכת הורים")].concat();
        let bytes = build(&[("word/document.xml", document(&body))]);
        let out = crate::extract(&bytes, "a.docx").unwrap_or_else(|e| panic!("{e}"));
        assert!(
            out.body.contains("המלצות:\n- טיפול בתקשורת\n- הדרכת הורים"),
            "{:?}",
            out.body
        );
    }

    #[test]
    fn not_a_word_document() {
        let bytes = build(&[("hello.txt", "x".to_owned())]);
        assert!(matches!(
            crate::extract(&bytes, "a.docx"),
            Err(IngestError::Corrupt(_))
        ));
    }

    #[test]
    fn zip_bomb_is_refused() {
        // 50 MB of one repeated character compresses to a few kilobytes.
        let huge = "א".repeat(25 * 1024 * 1024);
        let bytes = build(&[("word/document.xml", document(&para(&huge)))]);
        assert!(bytes.len() < crate::MAX_INPUT_BYTES);
        assert_eq!(crate::extract(&bytes, "a.docx"), Err(IngestError::TooLarge));
    }
}
