//! Her own Word template (EX-1): the report is written into a file she made once, with her
//! letterhead, fonts, header, footer and logo.
//!
//! Only what makes the page look like hers is kept: styles, theme, fonts, numbering, headers,
//! footers and their pictures, and a filtered `settings.xml`. Everything that can carry
//! identifying or active content is dropped (file properties, comments, custom XML, glossary,
//! edit-session ids, the link to the template on her disk) or refused (macros, embedded
//! objects, tracked changes with author names, external links that Word would fetch).
//!
//! Without a marker the template's body is replaced by the report. A paragraph that reads
//! only `{{הדוח}}` marks where the report goes; what is around it (an opening line, a
//! signature picture) stays.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};

use crate::docx::{
    body, confidentiality_para, footer, header, package, style_def, wrap_document, Layout, CORE,
    ROOT_RELS, STYLES,
};
use crate::{ExportError, Report};

/// The largest template accepted, packed.
pub const MAX_TEMPLATE_BYTES: usize = 5 * 1024 * 1024;
const MAX_UNPACKED: u64 = 40 * 1024 * 1024;
const MAX_ENTRIES: usize = 400;
/// The paragraph that marks where the report goes.
pub const MARKER: &str = "{{הדוח}}";

const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const DOC_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

/// What was found in a template, for the settings screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateSummary {
    /// The report goes where `{{הדוח}}` is; otherwise it replaces the body.
    pub has_marker: bool,
    pub headers: u32,
    pub footers: u32,
    pub images: u32,
    /// Report styles taken from hers (headings, body text…), out of all the report uses.
    pub styles_matched: u32,
    pub styles_total: u32,
}

fn refuse(msg: &str) -> ExportError {
    ExportError::Template(msg.to_owned())
}

fn bad(e: impl std::fmt::Display) -> ExportError {
    ExportError::Template(format!(
        "קובץ התבנית פגום ({e}). פותחים אותו בוורד ושומרים מחדש."
    ))
}

type Parts = Vec<(String, Vec<u8>)>;

/// Read the archive with limits: a "zip bomb" cannot exhaust memory.
fn unpack(bytes: &[u8]) -> Result<Parts, ExportError> {
    if bytes.len() > MAX_TEMPLATE_BYTES {
        return Err(refuse(
            "התבנית גדולה מדי (עד 5MB). בדרך כלל זו תמונה גדולה בכותרת: מקטינים אותה ושומרים מחדש.",
        ));
    }
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Err(refuse("התבנית מוגנת בסיסמה או שמורה בפורמט הישן (.doc). שומרים אותה בוורד כ-.docx, בלי סיסמה."));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| refuse("זה לא קובץ Word (.docx או .dotx)."))?;
    if zip.len() > MAX_ENTRIES {
        return Err(bad("too many parts"));
    }
    let mut budget = MAX_UNPACKED;
    let mut parts = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(bad)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_owned();
        if name.starts_with('/') || name.contains("..") || name.contains('\\') {
            return Err(bad("part name"));
        }
        let mut buf = Vec::new();
        (&mut f)
            .take(budget + 1)
            .read_to_end(&mut buf)
            .map_err(bad)?;
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if len > budget {
            return Err(refuse("התבנית גדולה מדי כשהיא פתוחה."));
        }
        budget -= len;
        parts.push((name, buf));
    }
    Ok(parts)
}

/// Parts of hers that are kept. Everything else is dropped.
fn kept(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("word/") else {
        return false;
    };
    let top_xml = |p: &str| rest.starts_with(p) && rest.ends_with(".xml") && !rest.contains('/');
    matches!(
        rest,
        "styles.xml"
            | "stylesWithEffects.xml"
            | "fontTable.xml"
            | "numbering.xml"
            | "settings.xml"
            | "webSettings.xml"
            | "footnotes.xml"
            | "endnotes.xml"
            | "_rels/fontTable.xml.rels"
            | "_rels/numbering.xml.rels"
            | "_rels/footnotes.xml.rels"
            | "_rels/endnotes.xml.rels"
    ) || top_xml("header")
        || top_xml("footer")
        || rest.starts_with("theme/")
        || rest.starts_with("media/")
        || rest.starts_with("fonts/")
        || (rest.starts_with("_rels/header") || rest.starts_with("_rels/footer"))
            && rest.ends_with(".xml.rels")
}

/// Relationship types the document keeps (the last segment of the type URI).
const KEPT_RELS: &[&str] = &[
    "styles",
    "stylesWithEffects",
    "theme",
    "fontTable",
    "numbering",
    "settings",
    "webSettings",
    "header",
    "footer",
    "footnotes",
    "endnotes",
    "image",
    "hyperlink",
];

#[derive(Debug, Clone)]
struct Rel {
    id: String,
    kind: String,
    target: String,
    external: bool,
}

fn attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.try_get_attribute(name).ok().flatten().and_then(|a| {
        a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
            .ok()
            .map(std::borrow::Cow::into_owned)
    })
}

fn rels(xml: &str) -> Result<Vec<Rel>, ExportError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(bad)? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "Relationship" => {
                let kind = attr(&e, "Type").unwrap_or_default();
                out.push(Rel {
                    id: attr(&e, "Id").unwrap_or_default(),
                    kind: kind.rsplit('/').next().unwrap_or_default().to_owned(),
                    target: attr(&e, "Target").unwrap_or_default(),
                    external: attr(&e, "TargetMode").is_some_and(|m| m == "External"),
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn rels_xml(rels: &[Rel]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for r in rels {
        let mode = if r.external {
            " TargetMode=\"External\""
        } else {
            ""
        };
        out.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{REL_NS}{}\" Target=\"{}\"{mode}/>",
            crate::docx::esc(&r.id),
            crate::docx::esc(&r.kind),
            crate::docx::esc(&r.target),
        ));
    }
    out.push_str("</Relationships>");
    out
}

fn text(bytes: &[u8]) -> Result<&str, ExportError> {
    std::str::from_utf8(bytes).map_err(bad)
}

/// Refuse what cannot be cleaned without changing what she sees.
fn check_safe(parts: &Parts) -> Result<(), ExportError> {
    for (name, data) in parts {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with("vbaproject.bin")
            || lower.contains("activex")
            || lower.contains("/embeddings/")
        {
            return Err(refuse("יש בתבנית מאקרו או אובייקט מוטבע. שומרים אותה בוורד כ\"מסמך Word\" (.docx) רגיל, בלי אובייקטים."));
        }
        if name == "[Content_Types].xml" && text(data)?.contains("macroEnabled") {
            return Err(refuse(
                "התבנית שמורה עם מאקרו. שומרים אותה בוורד כ\"מסמך Word\" (.docx).",
            ));
        }
        let looked_at = name == "word/document.xml" || (kept(name) && lower.ends_with(".xml"));
        if looked_at {
            let t = text(data)?;
            if t.contains("w:author=")
                || t.contains("commentReference")
                || t.contains("commentRangeStart")
            {
                return Err(refuse("יש בתבנית שינויים במעקב או הערות, ואיתם שמות. בוורד: סקירה ← קבלת כל השינויים, מחיקת כל ההערות, ושמירה מחדש."));
            }
        }
        if lower.ends_with(".rels") && (name == "word/_rels/document.xml.rels" || kept(name)) {
            for r in rels(text(data)?)? {
                if r.external && r.kind != "hyperlink" {
                    return Err(refuse("יש בתבנית תמונה או קובץ מקושרים מבחוץ, שוורד היה מוריד בכל פתיחה. מכניסים את התמונה לקובץ עצמו (הוספה ← תמונה) ושומרים מחדש."));
                }
            }
        }
    }
    Ok(())
}

/// A top-level element of the body: its byte range and the text it holds.
#[derive(Debug)]
struct Child {
    start: usize,
    end: usize,
    name: String,
    text: String,
}

fn children(body: &str) -> Result<Vec<Child>, ExportError> {
    let mut reader = Reader::from_str(body);
    let mut out: Vec<Child> = Vec::new();
    let mut depth = 0usize;
    let mut in_t = false;
    loop {
        let before = usize::try_from(reader.buffer_position()).map_err(bad)?;
        let ev = reader.read_event().map_err(bad)?;
        let after = usize::try_from(reader.buffer_position()).map_err(bad)?;
        match ev {
            Event::Start(e) => {
                if depth == 0 {
                    out.push(Child {
                        start: before,
                        end: after,
                        name: e.name().as_ref().to_owned(),
                        text: String::new(),
                    });
                }
                in_t = e.local_name().as_ref() == "t";
                depth += 1;
            }
            Event::Empty(e) => {
                if depth == 0 {
                    out.push(Child {
                        start: before,
                        end: after,
                        name: e.name().as_ref().to_owned(),
                        text: String::new(),
                    });
                }
            }
            Event::End(_) => {
                in_t = false;
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(c) = out.last_mut() {
                        c.end = after;
                    }
                }
            }
            Event::Text(t) if in_t => {
                if let Some(c) = out.last_mut() {
                    c.text.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) if in_t => {
                if let (Some(c), Ok(Some(ch))) = (out.last_mut(), r.resolve_char_ref()) {
                    c.text.push(ch);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// `settings.xml` with only layout settings kept: no edit-session ids, no document id, no
/// link to the template on her disk, no protection or mail merge; and Word is told to strip
/// personal information when the file is saved again.
fn clean_settings(xml: &str) -> Result<String, ExportError> {
    const KEEP: &[&str] = &[
        "mirrorMargins",
        "gutterAtTop",
        "bordersDoNotSurroundHeader",
        "bordersDoNotSurroundFooter",
        "defaultTabStop",
        "autoHyphenation",
        "consecutiveHyphenLimit",
        "hyphenationZone",
        "doNotHyphenateCaps",
        "evenAndOddHeaders",
        "characterSpacingControl",
        "footnotePr",
        "endnotePr",
        "compat",
        "themeFontLang",
        "clrSchemeMapping",
        "decimalSymbol",
        "listSeparator",
        "mathPr",
        "defaultTableStyle",
        "embedTrueTypeFonts",
        "embedSystemFonts",
        "saveSubsetFonts",
    ];
    let mut reader = Reader::from_str(xml);
    let mut w = Writer::new(Vec::new());
    let mut depth = 0usize;
    let mut skip: Option<usize> = None;
    let keep = |e: &BytesStart<'_>| KEEP.iter().any(|k| e.local_name().as_ref() == *k);
    loop {
        match reader.read_event().map_err(bad)? {
            Event::Start(e) => {
                depth += 1;
                if skip.is_none() && depth == 2 && !keep(&e) {
                    skip = Some(depth);
                }
                if skip.is_none() {
                    w.write_event(Event::Start(e)).map_err(bad)?;
                    if depth == 1 {
                        w.get_mut().extend_from_slice(
                            b"<w:removePersonalInformation/><w:removeDateAndTime/>",
                        );
                    }
                }
            }
            Event::Empty(e) => {
                if skip.is_none() && (depth != 1 || keep(&e)) {
                    w.write_event(Event::Empty(e)).map_err(bad)?;
                }
            }
            Event::End(e) => {
                if skip.is_none() {
                    w.write_event(Event::End(e)).map_err(bad)?;
                }
                if skip == Some(depth) {
                    skip = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            other => {
                if skip.is_none() {
                    w.write_event(other).map_err(bad)?;
                }
            }
        }
    }
    String::from_utf8(w.into_inner()).map_err(bad)
}

/// Name (lower case) → id, every id, and the default paragraph style's id.
type HerStyles = (HashMap<String, String>, HashSet<String>, String);

/// Her paragraph styles: Word's style name (lower case) → style id, and the default one's id.
/// Hebrew Word names its ids in Hebrew ("1" for heading 1), so the name is what matches.
fn her_styles(
    xml: &str,
) -> Result<HerStyles, ExportError> {
    let mut reader = Reader::from_str(xml);
    let mut by_name = HashMap::new();
    let mut ids = HashSet::new();
    let mut normal = String::from("Normal");
    let mut current: Option<(String, bool, bool)> = None;
    loop {
        match reader.read_event().map_err(bad)? {
            Event::Start(e) if e.local_name().as_ref() == "style" => {
                let id = attr(&e, "w:styleId").unwrap_or_default();
                ids.insert(id.clone());
                let para = attr(&e, "w:type").is_some_and(|t| t == "paragraph");
                let default = attr(&e, "w:default").is_some_and(|d| d == "1" || d == "true");
                current = Some((id, para, default));
            }
            Event::Empty(e) if e.local_name().as_ref() == "name" => {
                if let (Some((id, true, default)), Some(name)) = (&current, attr(&e, "w:val")) {
                    by_name
                        .entry(name.to_lowercase())
                        .or_insert_with(|| id.clone());
                    if *default {
                        normal.clone_from(id);
                    }
                }
            }
            Event::End(e) if e.local_name().as_ref() == "style" => current = None,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((by_name, ids, normal))
}

/// The section properties' header and footer references: (header?, type, rel id).
fn references(sect: &str) -> Result<Vec<(bool, String, String)>, ExportError> {
    let mut reader = Reader::from_str(sect);
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(bad)? {
            Event::Start(e) | Event::Empty(e) => {
                let is_header = e.local_name().as_ref() == "headerReference";
                if is_header || e.local_name().as_ref() == "footerReference" {
                    out.push((
                        is_header,
                        attr(&e, "w:type").unwrap_or_else(|| "default".to_owned()),
                        attr(&e, "r:id").unwrap_or_default(),
                    ));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// The section properties without the header and footer references to `ids`.
fn drop_references(sect: &str, ids: &[&String]) -> String {
    let mut out = String::with_capacity(sect.len());
    let mut rest = sect;
    while let Some(at) = rest
        .find("<w:headerReference")
        .into_iter()
        .chain(rest.find("<w:footerReference"))
        .min()
    {
        let Some(len) = rest[at..].find("/>").map(|e| e + 2) else {
            break;
        };
        out.push_str(&rest[..at]);
        let tag = &rest[at..at + len];
        if !ids.iter().any(|id| tag.contains(&format!("r:id=\"{id}\""))) {
            out.push_str(tag);
        }
        rest = &rest[at + len..];
    }
    out.push_str(rest);
    out
}

/// The text width of her page (page width less the margins), in twentieths of a point.
fn text_width(sect: &str) -> usize {
    let num = |el: &str, name: &str| -> Option<usize> {
        let start = sect.find(&format!("<w:{el} "))?;
        let tag = &sect[start..start + sect[start..].find('>')?];
        let at = tag.find(&format!(" w:{name}=\""))? + name.len() + 5;
        let end = at + tag[at..].find('"')?;
        tag[at..end].parse().ok()
    };
    let page = num("pgSz", "w").unwrap_or(11906);
    let left = num("pgMar", "left")
        .or_else(|| num("pgMar", "start"))
        .unwrap_or(1418);
    let right = num("pgMar", "right")
        .or_else(|| num("pgMar", "end"))
        .unwrap_or(1418);
    let gutter = num("pgMar", "gutter").unwrap_or(0);
    page.saturating_sub(left + right + gutter)
        .clamp(3000, 20000)
}

/// Content types: hers for kept parts, the document as a document (not a template), ours
/// for the parts added.
fn content_types(
    xml: &str,
    names: &HashSet<String>,
    added: &[(&str, &str)],
) -> Result<String, ExportError> {
    let mut reader = Reader::from_str(xml);
    let mut defaults: Vec<(String, String)> = Vec::new();
    let mut overrides: Vec<(String, String)> = Vec::new();
    loop {
        match reader.read_event().map_err(bad)? {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "Default" => {
                    if let (Some(x), Some(t)) = (attr(&e, "Extension"), attr(&e, "ContentType")) {
                        defaults.push((x.to_lowercase(), t));
                    }
                }
                "Override" => {
                    if let (Some(p), Some(t)) = (attr(&e, "PartName"), attr(&e, "ContentType")) {
                        if names.contains(p.trim_start_matches('/')) {
                            overrides.push((p, t));
                        }
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    for (x, t) in [
        (
            "rels",
            "application/vnd.openxmlformats-package.relationships+xml",
        ),
        ("xml", "application/xml"),
    ] {
        if !defaults.iter().any(|(e, _)| e == x) {
            defaults.push((x.to_owned(), t.to_owned()));
        }
    }
    overrides.retain(|(p, _)| p != "/word/document.xml");
    overrides.push(("/word/document.xml".to_owned(), DOC_MAIN.to_owned()));
    for (p, t) in added {
        overrides.retain(|(q, _)| q != p);
        overrides.push(((*p).to_owned(), (*t).to_owned()));
    }
    let esc = crate::docx::esc;
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    for (x, t) in &defaults {
        out.push_str(&format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            esc(x),
            esc(t)
        ));
    }
    for (p, t) in &overrides {
        out.push_str(&format!(
            "<Override PartName=\"{}\" ContentType=\"{}\"/>",
            esc(p),
            esc(t)
        ));
    }
    out.push_str("</Types>");
    Ok(out)
}

const HDR_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const FTR_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const CORE_TYPE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const STYLES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";

struct Built {
    parts: Parts,
    summary: TemplateSummary,
}

#[allow(clippy::too_many_lines)]
fn build(report: &Report, template: &[u8]) -> Result<Built, ExportError> {
    let parts = unpack(template)?;
    check_safe(&parts)?;
    let get = |n: &str| {
        parts
            .iter()
            .find(|(name, _)| name == n)
            .map(|(_, d)| d.as_slice())
    };
    let doc =
        text(get("word/document.xml").ok_or_else(|| refuse("זה לא קובץ Word: אין בו גוף מסמך."))?)?;
    let doc_rels = match get("word/_rels/document.xml.rels") {
        Some(d) => rels(text(d)?)?,
        None => Vec::new(),
    };

    // The body, and the section properties at its end.
    let root_start = doc.find("<w:document").ok_or_else(|| bad("document"))?;
    let root_end = root_start + doc[root_start..].find('>').ok_or_else(|| bad("document"))?;
    let root_attrs = doc[root_start + "<w:document".len()..root_end]
        .trim()
        .trim_end_matches('/');
    let body_open = doc.find("<w:body>").ok_or_else(|| bad("body"))? + "<w:body>".len();
    let body_close = doc.rfind("</w:body>").ok_or_else(|| bad("body"))?;
    let inner = &doc[body_open..body_close];
    let kids = children(inner)?;
    let (content, sect_child) = match kids.split_last() {
        Some((last, rest)) if last.name == "w:sectPr" => (rest, Some(last)),
        _ => (kids.as_slice(), None),
    };
    let mut sect = sect_child.map_or_else(String::new, |c| inner[c.start..c.end].to_owned());
    if sect.is_empty() || sect.ends_with("/>") {
        sect = "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1418\" w:right=\"1418\" w:bottom=\"1418\" w:left=\"1418\" w:header=\"709\" w:footer=\"709\" w:gutter=\"0\"/><w:bidi/></w:sectPr>".to_owned();
    }
    let marker = content
        .iter()
        .position(|c| c.name == "w:p" && matches!(c.text.trim(), MARKER | "{{report}}"));

    // Which of her parts stay, and the relationships that point at them.
    let mut out: Parts = parts.iter().filter(|(n, _)| kept(n)).cloned().collect();
    let names: HashSet<String> = out.iter().map(|(n, _)| n.clone()).collect();
    let mut keep_rels: Vec<Rel> = doc_rels
        .into_iter()
        .filter(|r| KEPT_RELS.contains(&r.kind.as_str()))
        .filter(|r| {
            if r.external {
                r.kind == "hyperlink"
            } else {
                names.contains(&format!(
                    "word/{}",
                    r.target.trim_start_matches('/').trim_start_matches("word/")
                ))
            }
        })
        .collect();
    let targets: HashMap<String, String> = keep_rels
        .iter()
        .filter(|r| !r.external)
        .map(|r| {
            let t = r.target.trim_start_matches('/').trim_start_matches("word/");
            (r.id.clone(), format!("word/{t}"))
        })
        .collect();
    let target_of = |id: &str| targets.get(id).cloned();

    // The confidentiality line goes into each of her headers; a missing default header or
    // footer is ours.
    let refs = references(&sect)?;
    let mut added: Vec<(&str, &str)> = vec![("/docProps/core.xml", CORE_TYPE)];
    let mut headers = 0u32;
    let mut footers = 0u32;
    let line = confidentiality_para(&report.confidentiality);
    for (is_header, _, id) in &refs {
        let Some(target) = target_of(id) else {
            continue;
        };
        if *is_header {
            headers += 1;
            if let Some((_, data)) = out.iter_mut().find(|(n, _)| *n == target) {
                let xml = text(data)?.to_owned();
                let at = xml.rfind("</w:hdr>").ok_or_else(|| bad("header"))?;
                *data = format!("{}{line}{}", &xml[..at], &xml[at..]).into_bytes();
            }
        } else {
            footers += 1;
        }
    }
    let has = |header: bool| {
        refs.iter()
            .any(|(h, t, id)| *h == header && t == "default" && target_of(id).is_some())
    };
    let mut inserted = String::new();
    if !has(true) {
        out.push(("word/header_dv.xml".to_owned(), header(report).into_bytes()));
        keep_rels.push(Rel {
            id: "rIdDvHeader".into(),
            kind: "header".into(),
            target: "header_dv.xml".into(),
            external: false,
        });
        inserted.push_str("<w:headerReference w:type=\"default\" r:id=\"rIdDvHeader\"/>");
        added.push(("/word/header_dv.xml", HDR_TYPE));
    }
    if !has(false) {
        out.push(("word/footer_dv.xml".to_owned(), footer().into_bytes()));
        keep_rels.push(Rel {
            id: "rIdDvFooter".into(),
            kind: "footer".into(),
            target: "footer_dv.xml".into(),
            external: false,
        });
        inserted.push_str("<w:footerReference w:type=\"default\" r:id=\"rIdDvFooter\"/>");
        added.push(("/word/footer_dv.xml", FTR_TYPE));
    }
    if !inserted.is_empty() {
        // References come first in the section properties (schema order); a dangling one of
        // hers (to a dropped part) is removed so Word does not complain.
        let open_end = sect.find('>').ok_or_else(|| bad("sectPr"))? + 1;
        sect.insert_str(open_end, &inserted);
    }
    let dangling: Vec<&String> = refs
        .iter()
        .filter(|(_, _, id)| target_of(id).is_none())
        .map(|(_, _, id)| id)
        .collect();
    sect = drop_references(&sect, &dangling);

    // Styles: hers where the name matches, ours (without a font: hers is used) where not.
    let mut layout = Layout {
        ids: HashMap::new(),
        width: text_width(&sect),
    };
    let mut matched = 0u32;
    let styles_at = out.iter().position(|(n, _)| n == "word/styles.xml");
    match styles_at {
        Some(i) => {
            let xml = text(&out[i].1)?.to_owned();
            let (by_name, ids, normal) = her_styles(&xml)?;
            let mut extra = String::new();
            for (key, name, ppr, size, bold, color) in STYLES {
                if let Some(id) = by_name.get(&name.to_lowercase()) {
                    matched += 1;
                    layout.ids.insert(key, id.clone());
                } else {
                    let id = if ids.contains(*key) {
                        format!("Dv{key}")
                    } else {
                        (*key).to_owned()
                    };
                    extra.push_str(
                        &style_def(&id, name, ppr, *size, *bold, *color, None).replace(
                            "<w:basedOn w:val=\"Normal\"/>",
                            &format!("<w:basedOn w:val=\"{}\"/>", crate::docx::esc(&normal)),
                        ),
                    );
                    layout.ids.insert(key, id);
                }
            }
            let at = xml.rfind("</w:styles>").ok_or_else(|| bad("styles"))?;
            out[i].1 = format!("{}{extra}{}", &xml[..at], &xml[at..]).into_bytes();
        }
        None => {
            layout = Layout {
                width: layout.width,
                ..Layout::plain()
            };
            out.push((
                "word/styles.xml".to_owned(),
                crate::docx::styles(&report.font).into_bytes(),
            ));
            keep_rels.push(Rel {
                id: "rIdDvStyles".into(),
                kind: "styles".into(),
                target: "styles.xml".into(),
                external: false,
            });
            added.push(("/word/styles.xml", STYLES_TYPE));
        }
    }
    if let Some((_, data)) = out.iter_mut().find(|(n, _)| n == "word/settings.xml") {
        *data = clean_settings(text(data)?)?.into_bytes();
    }

    // The body: around the marker, hers; at the marker (or instead of her body), the report.
    let report_xml = body(report, &layout);
    let mut new_body = String::new();
    match marker {
        Some(m) => {
            for (i, c) in content.iter().enumerate() {
                if i == m {
                    new_body.push_str(&report_xml);
                } else {
                    new_body.push_str(&inner[c.start..c.end]);
                }
            }
        }
        None => new_body.push_str(&report_xml),
    }
    new_body.push_str(&sect);
    let images = u32::try_from(
        out.iter()
            .filter(|(n, _)| n.starts_with("word/media/"))
            .count(),
    )
    .unwrap_or(u32::MAX);

    let names: HashSet<String> = out.iter().map(|(n, _)| n.clone()).collect();
    let types = content_types(
        text(get("[Content_Types].xml").ok_or_else(|| bad("content types"))?)?,
        &names,
        &added,
    )?;
    let mut final_parts: Parts = vec![
        ("[Content_Types].xml".to_owned(), types.into_bytes()),
        ("_rels/.rels".to_owned(), ROOT_RELS.as_bytes().to_vec()),
        (
            "word/_rels/document.xml.rels".to_owned(),
            rels_xml(&keep_rels).into_bytes(),
        ),
        (
            "word/document.xml".to_owned(),
            wrap_document(root_attrs, &new_body).into_bytes(),
        ),
    ];
    final_parts.extend(out);
    final_parts.push(("docProps/core.xml".to_owned(), CORE.as_bytes().to_vec()));
    Ok(Built {
        parts: final_parts,
        summary: TemplateSummary {
            has_marker: marker.is_some(),
            headers,
            footers,
            images,
            styles_matched: matched,
            styles_total: u32::try_from(STYLES.len()).unwrap_or(u32::MAX),
        },
    })
}

/// Check a template once, when she chooses it: refused with a sentence she can act on, or a
/// summary of what will be used.
pub fn check_template(template: &[u8]) -> Result<TemplateSummary, ExportError> {
    let sample = Report {
        title: String::new(),
        info: Vec::new(),
        parts: Vec::new(),
        tables: Vec::new(),
        signature: Vec::new(),
        confidentiality: String::new(),
        font: String::new(),
    };
    Ok(build(&sample, template)?.summary)
}

/// The report written into her template.
pub fn render_with_template(report: &Report, template: &[u8]) -> Result<Vec<u8>, ExportError> {
    package(&build(report, template)?.parts)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::tests::sample;

    /// The fictional template in tests/fixtures/report_template (unpacked XML: a binary .docx
    /// outside templates/ is refused by the pre-commit scan), zipped here.
    fn fixture(edit: impl Fn(&str, String) -> Option<String>) -> Vec<u8> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/report_template");
        let mut files = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    files.push(p);
                }
            }
        }
        files.sort();
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for p in files {
                let name = p
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let content = std::fs::read_to_string(&p).unwrap();
                if let Some(c) = edit(&name, content) {
                    zip.start_file(name, opts).unwrap();
                    zip.write_all(c.as_bytes()).unwrap();
                }
            }
            // A tiny logo in her header (bytes, not a file: the scan refuses binaries).
            if edit("word/media/logo.png", String::new()).is_some() {
                zip.start_file("word/media/logo.png", opts).unwrap();
                zip.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    fn part(docx: &[u8], name: &str) -> Option<String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
        let mut s = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut s).unwrap();
        Some(s)
    }

    #[test]
    fn the_report_goes_into_her_template() {
        let tpl = fixture(|_, c| Some(c));
        let summary = check_template(&tpl).unwrap();
        assert!(summary.has_marker);
        assert_eq!((summary.headers, summary.images), (1, 1));
        let out = render_with_template(&sample(), &tpl).unwrap();
        let doc = part(&out, "word/document.xml").unwrap();
        // Hebrew Word's style ids ("1" is heading 1) are used, found by name.
        assert!(
            doc.contains("<w:pStyle w:val=\"1\"/>") && doc.contains("סיבת הפניה"),
            "{doc}"
        );
        // Her opening line and signature line stay around the report; the marker is gone.
        let open = doc.find("לכבוד ההורים").unwrap();
        let report = doc.find("סיבת הפניה").unwrap();
        let sign = doc.find("בברכה, ד\"ר בדויה").unwrap();
        assert!(open < report && report < sign);
        assert!(!doc.contains("הדוח}}"));
        // Her letterhead stays, with the confidentiality line added.
        let hdr = part(&out, "word/header1.xml").unwrap();
        assert!(hdr.contains("מרפאה בדויה להתפתחות הילד") && hdr.contains("חסוי"));
        assert!(zip::ZipArchive::new(Cursor::new(&out))
            .unwrap()
            .by_name("word/media/logo.png")
            .is_ok());
        // Nothing about her or her computer: no author, company, template path or edit ids.
        assert!(part(&out, "docProps/app.xml").is_none());
        assert!(part(&out, "customXml/item1.xml").is_none());
        assert!(!part(&out, "docProps/core.xml").unwrap().contains("creator"));
        let settings = part(&out, "word/settings.xml").unwrap();
        assert!(
            !settings.contains("attachedTemplate") && !settings.contains("rsid"),
            "{settings}"
        );
        assert!(
            settings.contains("removePersonalInformation")
                && settings.contains("evenAndOddHeaders")
        );
        assert!(part(&out, "word/_rels/settings.xml.rels").is_none());
        let types = part(&out, "[Content_Types].xml").unwrap();
        assert!(types.contains(DOC_MAIN) && !types.contains("template.main"));
        assert!(!types.contains("app.xml"));
        // A default footer she does not have is ours (page numbers).
        assert!(part(&out, "word/footer_dv.xml")
            .unwrap()
            .contains("NUMPAGES"));
        assert!(doc.contains("rIdDvFooter"));
        assert_eq!(render_with_template(&sample(), &tpl).ok(), Some(out));
    }

    #[test]
    fn without_a_marker_the_body_is_replaced() {
        let tpl = fixture(|name, c| {
            Some(if name == "word/document.xml" {
                c.replace("{{", "").replace("}}", "")
            } else {
                c
            })
        });
        assert!(!check_template(&tpl).unwrap().has_marker);
        let doc = part(
            &render_with_template(&sample(), &tpl).unwrap(),
            "word/document.xml",
        )
        .unwrap();
        assert!(!doc.contains("לכבוד ההורים") && doc.contains("סיבת הפניה"));
    }

    #[test]
    fn refuses_what_cannot_be_cleaned() {
        let tracked = fixture(|name, c| {
            Some(if name == "word/header1.xml" {
                c.replace("<w:r>", "<w:ins w:id=\"1\" w:author=\"Someone\"><w:r>")
                    .replace("</w:r>", "</w:r></w:ins>")
            } else {
                c
            })
        });
        assert!(
            matches!(check_template(&tracked), Err(ExportError::Template(m)) if m.contains("שינויים במעקב"))
        );
        let linked = fixture(|name, c| {
            Some(if name == "word/_rels/header1.xml.rels" {
                c.replace(
                    "Target=\"media/logo.png\"",
                    "Target=\"https://example.invalid/logo.png\" TargetMode=\"External\"",
                )
            } else {
                c
            })
        });
        assert!(
            matches!(check_template(&linked), Err(ExportError::Template(m)) if m.contains("מקושרים"))
        );
        let macro_ct = fixture(|name, c| {
            Some(if name == "[Content_Types].xml" {
                c.replace(
                    "template.main+xml",
                    "template.macroEnabledTemplate.main+xml",
                )
            } else {
                c
            })
        });
        assert!(check_template(&macro_ct).is_err());
        assert!(check_template(b"not a zip").is_err());
        assert!(check_template(&[0xD0, 0xCF, 0x11, 0xE0, 0, 0]).is_err());
    }
}
