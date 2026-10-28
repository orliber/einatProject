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
    decode_xml(&buf).map(Some)
}

/// An XML part's text: UTF-8 (with or without a byte-order mark), or UTF-16 with its mark,
/// as some programs save a part (D-135). Anything else is not read.
fn decode_xml(buf: &[u8]) -> Result<String, IngestError> {
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .filter_map(|c| match *c {
                [a, b] if le => Some(u16::from_le_bytes([a, b])),
                [a, b] => Some(u16::from_be_bytes([a, b])),
                _ => None,
            })
            .collect();
        String::from_utf16(&units).map_err(|_| corrupt("utf-16 part does not decode"))
    };
    let text = match buf {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true)?,
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false)?,
        _ => String::from_utf8(buf.to_vec()).map_err(|_| corrupt("part is not utf-8"))?,
    };
    // The declaration may still say UTF-16; the text is already decoded.
    Ok(text.trim_start_matches('\u{FEFF}').to_owned())
}

/// Where the document's body is: the package's own pointer (`_rels/.rels`), which Word 365
/// and Word Online set to names like `word/document2.xml` (D-135); `word/document.xml`
/// when the pointer is missing.
fn main_part(
    zip: &mut Archive<'_>,
    names: &[String],
    budget: &mut u64,
) -> Result<String, IngestError> {
    let fallback = "word/document.xml".to_owned();
    // A pointer that cannot be read is no reason to refuse a document Word reads: the usual
    // name is tried (only the size limit still stops the reading).
    let rels = match read_part(zip, "_rels/.rels", budget) {
        Ok(Some(rels)) => rels,
        Err(IngestError::TooLarge) => return Err(IngestError::TooLarge),
        Ok(None) | Err(_) => return Ok(fallback),
    };
    let mut reader = Reader::from_str(&rels);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => return Ok(fallback),
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "Relationship" => {
                let attr = |k: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == k)
                        .map(|a| a.value.to_string())
                };
                let is_main = attr("Type").is_some_and(|t| t.ends_with("/officeDocument"));
                if let (true, Some(target)) = (is_main, attr("Target")) {
                    let target = target.trim_start_matches('/').to_owned();
                    // Only a part that is really in the archive, never a path out of it.
                    if !target.contains("..") && names.contains(&target) {
                        return Ok(target);
                    }
                }
            }
            Ok(_) => {}
        }
    }
}

pub(crate) fn extract(bytes: &[u8]) -> Result<Extracted, IngestError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(corrupt)?;
    if zip.len() > MAX_ENTRIES {
        return Err(corrupt("too many entries"));
    }
    // A name that does not decode makes the whole file unreadable (fail-closed).
    let names: Vec<String> = zip
        .file_names()
        .map(|n| n.map(std::borrow::Cow::into_owned))
        .collect::<Result<_, _>>()
        .map_err(corrupt)?;
    let mut budget = MAX_TOTAL_BYTES;
    let mut warnings = Vec::new();

    let main = main_part(&mut zip, &names, &mut budget)?;
    let folder = main
        .rsplit_once('/')
        .map_or(String::new(), |(f, _)| format!("{f}/"));
    let document = read_part(&mut zip, &main, &mut budget)?
        .ok_or_else(|| corrupt("not a Word document (no main part)"))?;
    let doc = paragraphs(&document)?;
    let mut body = doc.text;
    for part in ["footnotes.xml", "endnotes.xml"].map(|p| format!("{folder}{p}")) {
        if let Some(xml) = read_part(&mut zip, &part, &mut budget)? {
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
            (n.starts_with(&format!("{folder}header")) || n.starts_with(&format!("{folder}footer")))
                && n.ends_with(".xml")
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
    if names.iter().any(|n| n == &format!("{folder}comments.xml")) {
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
        // File properties only suggest names to hide; a broken one ("A & B") does not make
        // the document unreadable. She is told, and the body is filtered in full (D-135).
        match read_part(&mut zip, part, &mut budget)
            .and_then(|x| x.map(|x| properties(&x, fields)).transpose())
        {
            Ok(Some(found)) => metadata.extend(found),
            Ok(None) => {}
            Err(IngestError::TooLarge) => return Err(IngestError::TooLarge),
            Err(_) => {
                if !warnings.iter().any(|w| w.contains("מאפייני הקובץ")) {
                    warnings.push(
                        "מאפייני הקובץ (שם המחבר וכדומה) לא נקראו. כדאי לבדוק בעצמך שאין בהם שמות."
                            .to_owned(),
                    );
                }
            }
        }
    }

    Ok(Extracted {
        format: Format::Docx,
        body,
        margins,
        metadata,
        warnings,
        pages: 0,
        ocr: false,
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
    let mut in_para_props = false;
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
                    "numPr"
                        if out
                            .get(para_start..)
                            .is_some_and(|para| !para.starts_with("- ")) =>
                    {
                        out.insert_str(para_start, "- ")
                    }
                    "tc" => cell_depth += 1,
                    "rPr" => in_run_props = true,
                    "pPr" => in_para_props = true,
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
                    // A tab character in a run; `w:pPr/w:tabs/w:tab` only defines a tab stop.
                    "tab" if !in_run_props && !in_para_props => out.push('\t'),
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
                    "pPr" => in_para_props = false,
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

    /// A tab in a run becomes a tab (so "קוביות<tab>9" stays two words); a tab stop in the
    /// paragraph's properties is only a definition and emits nothing.
    #[test]
    fn a_tab_in_a_run_is_kept_and_a_tab_stop_is_not() {
        let body = "<w:p><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"2000\"/></w:tabs></w:pPr>\
             <w:r><w:t>קוביות</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>9</w:t></w:r></w:p>";
        let bytes = build(&[("word/document.xml", document(body))]);
        let out = crate::extract(&bytes, "a.docx").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.body, "קוביות\t9");
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

    /// Files the way real programs save them (D-135): each one is read, with its text.
    #[test]
    fn word_files_as_real_programs_save_them_are_read() {
        let body = || document(&para("שורה בדויה מהדוח"));
        let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/word/document2.xml"/></Relationships>"#;
        let mut utf16 = vec![0xFF, 0xFE];
        for u in body().encode_utf16() {
            utf16.extend(u.to_le_bytes());
        }
        let raw = |name: &str, bytes: &[u8]| {
            let mut buf = Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut buf);
                let opts = zip::write::SimpleFileOptions::default();
                zip.start_file(name, opts).unwrap_or_else(|e| panic!("{e}"));
                zip.write_all(bytes).unwrap_or_else(|e| panic!("{e}"));
                zip.finish().unwrap_or_else(|e| panic!("{e}"));
            }
            buf.into_inner()
        };
        let cases: Vec<(&str, Vec<u8>)> = vec![
            (
                "Word 365: document2.xml",
                build(&[
                    ("_rels/.rels", rels.to_owned()),
                    ("word/document2.xml", body()),
                ]),
            ),
            (
                "broken company name",
                build(&[
                    ("word/document.xml", body()),
                    (
                        "docProps/app.xml",
                        "<?xml version=\"1.0\"?><Properties><Company>A & B</Company></Properties>"
                            .to_owned(),
                    ),
                ]),
            ),
            (
                "byte-order mark",
                build(&[("word/document.xml", format!("\u{FEFF}{}", body()))]),
            ),
            ("UTF-16 part", raw("word/document.xml", &utf16)),
        ];
        for (label, bytes) in cases {
            let out = extract(&bytes).unwrap_or_else(|e| panic!("{label}: {e:?}"));
            assert!(
                out.body.contains("שורה בדויה מהדוח"),
                "{label}: {}",
                out.body
            );
        }
        // The broken property is said, not silently skipped.
        let out = extract(&build(&[
            ("word/document.xml", body()),
            (
                "docProps/app.xml",
                "<Properties><Company>A & B</Company></Properties>".to_owned(),
            ),
        ]))
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            out.warnings.iter().any(|w| w.contains("מאפייני הקובץ")),
            "{:?}",
            out.warnings
        );
        // A pointer out of the archive is never followed, even to a part that is there by
        // that name: the usual body is read instead.
        let evil = r#"<Relationships><Relationship Type="x/officeDocument" Target="../secret.xml"/></Relationships>"#;
        let secret = body().replace(
            "<w:body>",
            "<w:body><w:p><w:r><w:t>SECRET</w:t></w:r></w:p>",
        );
        let out = extract(&build(&[
            ("_rels/.rels", evil.to_owned()),
            ("../secret.xml", secret),
            ("word/document.xml", body()),
        ]))
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(!out.body.contains("SECRET"), "{}", out.body);
        assert!(out.body.contains("שורה בדויה מהדוח"), "{}", out.body);
    }
}
