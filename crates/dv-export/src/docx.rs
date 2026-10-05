//! A minimal, clean WordprocessingML package, right to left.

use std::fmt::Write as _;
use std::io::{Cursor, Write};

use std::collections::HashMap;

use crate::{ExportError, Report, ScoreChart};

pub(crate) const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

/// Text as XML character data: escaped, and without characters XML 1.0 forbids.
pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// One right-to-left paragraph. `bold_prefix` renders a bold label before the text.
pub(crate) fn para(style: &str, text: &str, bold_prefix: Option<&str>) -> String {
    let mut p = format!("<w:p><w:pPr><w:pStyle w:val=\"{style}\"/><w:bidi/></w:pPr>");
    if let Some(label) = bold_prefix {
        let _ = write!(p, "<w:r><w:rPr><w:b/><w:bCs/><w:rtl/></w:rPr><w:t xml:space=\"preserve\">{}: </w:t></w:r>", esc(label));
    }
    let _ = write!(
        p,
        "<w:r><w:rPr><w:rtl/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        esc(text)
    );
    p
}

/// The paragraph styles the body uses: (key, Word's style name, paragraph properties, size in
/// half-points, bold, color). With her template, a style of hers with the same name wins.
/// One row of [`STYLES`].
pub(crate) type StyleRow = (
    &'static str,
    &'static str,
    &'static str,
    u32,
    bool,
    Option<&'static str>,
);

pub(crate) const STYLES: &[StyleRow] = &[
    (
        "Title",
        "Title",
        "<w:jc w:val=\"center\"/><w:spacing w:after=\"360\"/>",
        36,
        true,
        None,
    ),
    (
        "Info",
        "Report Info",
        "<w:spacing w:after=\"60\"/>",
        24,
        false,
        None,
    ),
    (
        "Heading1",
        "heading 1",
        "<w:keepNext/><w:spacing w:before=\"360\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/>",
        30,
        true,
        Some("1F3A5F"),
    ),
    (
        "Heading2",
        "heading 2",
        "<w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"1\"/>",
        26,
        true,
        None,
    ),
    (
        "BodyText",
        "Body Text",
        "<w:jc w:val=\"both\"/>",
        24,
        false,
        None,
    ),
    (
        "Signature",
        "Signature",
        "<w:keepNext/><w:spacing w:after=\"0\"/>",
        24,
        false,
        None,
    ),
    (
        "TableText",
        "Table Text",
        "<w:spacing w:after=\"0\"/>",
        22,
        false,
        None,
    ),
    (
        "TableNote",
        "Table Note",
        "<w:spacing w:before=\"60\" w:after=\"200\"/>",
        18,
        false,
        Some("5B6567"),
    ),
    (
        "ChartText",
        "Chart Text",
        "<w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/>",
        16,
        false,
        None,
    ),
];

/// Text width of an A4 page with the default margins, in twentieths of a point.
pub(crate) const A4_TEXT_WIDTH: usize = 9070;

/// Which style id each key maps to, and how wide the text column is.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub ids: HashMap<&'static str, String>,
    pub width: usize,
}

impl Layout {
    pub fn plain() -> Self {
        Self {
            ids: STYLES.iter().map(|s| (s.0, s.0.to_owned())).collect(),
            width: A4_TEXT_WIDTH,
        }
    }

    fn id(&self, key: &'static str) -> &str {
        self.ids.get(key).map_or(key, String::as_str)
    }
}

/// One style definition. Without a font, the run inherits the document's (her template's).
pub(crate) fn style_def(
    id: &str,
    name: &str,
    ppr: &str,
    size: u32,
    bold: bool,
    color: Option<&str>,
    font: Option<&str>,
) -> String {
    format!(
        "<w:style w:type=\"paragraph\" w:styleId=\"{}\"><w:name w:val=\"{}\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:bidi/>{ppr}</w:pPr>{}</w:style>",
        esc(id),
        esc(name),
        run_props(size, bold, color, font)
    )
}

fn run_props(size: u32, bold: bool, color: Option<&str>, font: Option<&str>) -> String {
    let f = font
        .map(|f| {
            let f = esc(f);
            format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>")
        })
        .unwrap_or_default();
    let b = if bold { "<w:b/><w:bCs/>" } else { "" };
    let c = color
        .map(|c| format!("<w:color w:val=\"{c}\"/>"))
        .unwrap_or_default();
    format!("<w:rPr>{f}{b}{c}<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/><w:lang w:val=\"he-IL\" w:bidi=\"he-IL\"/></w:rPr>")
}

/// Thin table borders; `inside_v` false leaves the bar cells of a chart without lines.
fn borders(inside_v: bool) -> String {
    let b = |side: &str| {
        format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"B8C4C1\"/>")
    };
    let mut out = String::from("<w:tblBorders>");
    for side in ["top", "start", "bottom", "end", "insideH"] {
        out.push_str(&b(side));
    }
    if inside_v {
        out.push_str(&b("insideV"));
    } else {
        out.push_str("<w:insideV w:val=\"nil\"/>");
    }
    out.push_str("</w:tblBorders>");
    out
}

/// One table cell with a single paragraph.
fn cell(
    width: usize,
    span: usize,
    fill: Option<&str>,
    style: &str,
    bold: bool,
    text: &str,
) -> String {
    let span = if span > 1 {
        format!("<w:gridSpan w:val=\"{span}\"/>")
    } else {
        String::new()
    };
    let shade = fill
        .map(|f| format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{f}\"/>"))
        .unwrap_or_default();
    let b = if bold { "<w:b/><w:bCs/>" } else { "" };
    let run = if text.is_empty() {
        String::new()
    } else {
        format!(
            "<w:r><w:rPr>{b}<w:rtl/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
            esc(text)
        )
    };
    format!(
        "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/>{span}{shade}<w:vAlign w:val=\"center\"/></w:tcPr>\
         <w:p><w:pPr><w:pStyle w:val=\"{}\"/><w:bidi/></w:pPr>{run}</w:p></w:tc>",
        esc(style)
    )
}

fn table_start(width: usize, inside_v: bool, grid: &[usize]) -> String {
    let mut t = format!(
        "<w:tbl><w:tblPr><w:bidiVisual/><w:tblW w:w=\"{width}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>{}\
         <w:tblCellMar><w:top w:w=\"60\" w:type=\"dxa\"/><w:start w:w=\"100\" w:type=\"dxa\"/><w:bottom w:w=\"60\" w:type=\"dxa\"/><w:end w:w=\"100\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>",
        borders(inside_v)
    );
    for w in grid {
        let _ = write!(t, "<w:gridCol w:w=\"{w}\"/>");
    }
    t.push_str("</w:tblGrid>");
    t
}

/// A right-to-left table with a bold, shaded header row and thin borders.
fn table(columns: &[String], rows: &[Vec<String>], l: &Layout) -> String {
    let n = columns.len().max(1);
    let first = if n > 1 { l.width * 42 / 100 } else { l.width };
    let rest = if n > 1 {
        (l.width - first) / (n - 1)
    } else {
        0
    };
    let width = |i: usize| if i == 0 { first } else { rest };
    let grid: Vec<usize> = (0..n).map(width).collect();
    let style = l.id("TableText");
    let mut t = table_start(l.width, true, &grid);
    t.push_str("<w:tr><w:trPr><w:tblHeader/></w:trPr>");
    for (i, c) in columns.iter().enumerate() {
        t.push_str(&cell(width(i), 1, Some(HEADER_FILL), style, true, c));
    }
    t.push_str("</w:tr>");
    for row in rows {
        t.push_str("<w:tr><w:trPr><w:cantSplit/></w:trPr>");
        for (i, c) in row.iter().enumerate() {
            t.push_str(&cell(width(i), 1, None, style, false, c));
        }
        t.push_str("</w:tr>");
    }
    t.push_str("</w:tbl>");
    t
}

const HEADER_FILL: &str = "E6EFED";
const BAR_FILL: &str = "1F3A5F";
const BAND_FILL: &str = "EEF3F2";

/// A number without a trailing ".0".
pub(crate) fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Bars of table cells on a fixed axis: a label and the score, then one cell per `step`,
/// filled up to the score. The band within one standard deviation of the mean is tinted, and
/// the header row names the axis in groups of one standard deviation.
fn chart(c: &ScoreChart, l: &Layout) -> String {
    let step = if c.step > 0.0 { c.step } else { 1.0 };
    let span = (c.max - c.min).max(step);
    // At most 60 cells: a chart wider than that is not a profile any more.
    let cells = ((span / step).round() as usize).clamp(1, 60);
    let label_w = l.width * 30 / 100;
    let score_w = l.width * 8 / 100;
    let cell_w = (l.width - label_w - score_w) / cells;
    let mut grid = vec![label_w, score_w];
    grid.extend(std::iter::repeat_n(cell_w, cells));
    let width = label_w + score_w + cell_w * cells;
    let start = |i: usize| c.min + step * i as f64;
    let in_band = |i: usize| start(i) + step > c.mean - c.sd && start(i) < c.mean + c.sd;
    let small = l.id("ChartText");
    let text = l.id("TableText");

    let mut t = table_start(width, false, &grid);
    // Header: the axis in groups of one standard deviation ("85–99").
    t.push_str("<w:tr><w:trPr><w:tblHeader/></w:trPr>");
    t.push_str(&cell(label_w, 1, Some(HEADER_FILL), text, true, "מדד"));
    t.push_str(&cell(score_w, 1, Some(HEADER_FILL), text, true, "ציון"));
    let group = if c.sd > 0.0 {
        ((c.sd / step).round() as usize).clamp(1, cells)
    } else {
        cells
    };
    let mut i = 0;
    while i < cells {
        let n = group.min(cells - i);
        let last = start(i + n) - if step >= 1.0 { 1.0 } else { step / 10.0 };
        let label = if n == 1 {
            num(start(i))
        } else {
            format!("{}–{}", num(start(i)), num(last))
        };
        t.push_str(&cell(
            cell_w * n,
            n,
            Some(HEADER_FILL),
            small,
            false,
            &label,
        ));
        i += n;
    }
    t.push_str("</w:tr>");
    for bar in &c.bars {
        t.push_str(
            "<w:tr><w:trPr><w:cantSplit/><w:trHeight w:val=\"340\" w:hRule=\"atLeast\"/></w:trPr>",
        );
        t.push_str(&cell(label_w, 1, None, text, false, &bar.label));
        t.push_str(&cell(score_w, 1, None, text, true, &num(bar.value)));
        let v = bar.value.clamp(c.min, c.max - step / 2.0);
        for i in 0..cells {
            let fill = if start(i) <= v {
                Some(BAR_FILL)
            } else if in_band(i) {
                Some(BAND_FILL)
            } else {
                None
            };
            t.push_str(&cell(cell_w, 1, fill, small, false, ""));
        }
        t.push_str("</w:tr>");
    }
    t.push_str("</w:tbl>");
    t
}

/// The body's paragraphs and tables, without the section properties.
pub(crate) fn body(report: &Report, l: &Layout) -> String {
    let mut body = String::new();
    let mut p = |key: &'static str, text: &str, label: Option<&str>| {
        if !text.is_empty() || label.is_some() || key == "Signature" {
            body.push_str(&para(l.id(key), text, label));
        }
    };
    p("Title", &report.title, None);
    for line in &report.info {
        p("Info", &line.value, Some(&line.label));
    }
    for part in &report.parts {
        p("Heading1", &part.title, None);
        for section in &part.sections {
            p("Heading2", &section.title, None);
            for text in &section.paragraphs {
                for piece in text.split('\n').filter(|t| !t.trim().is_empty()) {
                    p("BodyText", piece.trim(), None);
                }
            }
        }
    }
    if !report.signature.is_empty() {
        p("Signature", "", None);
        for line in &report.signature {
            p("Signature", line, None);
        }
    }
    if !report.tables.is_empty() {
        body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        body.push_str(&para(l.id("Heading1"), "נספח: טבלאות ציונים", None));
        for t in &report.tables {
            body.push_str(&para(l.id("Heading2"), &t.title, None));
            body.push_str(&table(&t.columns, &t.rows, l));
            if !t.note.is_empty() {
                body.push_str(&para(l.id("TableNote"), &t.note, None));
            }
            for c in &t.charts {
                if c.bars.is_empty() {
                    continue;
                }
                body.push_str(&para(l.id("Heading2"), &c.title, None));
                body.push_str(&chart(c, l));
                if !c.note.is_empty() {
                    body.push_str(&para(l.id("TableNote"), &c.note, None));
                }
            }
        }
    }
    body
}

/// Page size and margins, with the header and footer of the plain document.
const PLAIN_SECTION: &str = "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>\
     <w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/>\
     <w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
     <w:pgMar w:top=\"1418\" w:right=\"1418\" w:bottom=\"1418\" w:left=\"1418\" w:header=\"709\" w:footer=\"709\" w:gutter=\"0\"/>\
     <w:bidi/></w:sectPr>";

pub(crate) fn wrap_document(root_attrs: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document {root_attrs}><w:body>{body}</w:body></w:document>"
    )
}

fn document(report: &Report) -> String {
    let mut b = body(report, &Layout::plain());
    b.push_str(PLAIN_SECTION);
    wrap_document(W_NS, &b)
}

/// The confidentiality line, centered and gray, with direct formatting: it has to look the
/// same inside a header of her template, whatever its styles are.
pub(crate) fn confidentiality_para(text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:jc w:val=\"center\"/><w:bidi/></w:pPr><w:r><w:rPr><w:rtl/><w:color w:val=\"7A7A7A\"/><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr>\
         <w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        esc(text)
    )
}

pub(crate) fn header(report: &Report) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:hdr {W_NS}>{}</w:hdr>",
        confidentiality_para(&report.confidentiality)
    )
}

pub(crate) fn footer() -> String {
    let field = |code: &str| {
        format!(
            "<w:r><w:rPr><w:rtl/></w:rPr><w:fldChar w:fldCharType=\"begin\"/></w:r>\
             <w:r><w:rPr><w:rtl/></w:rPr><w:instrText xml:space=\"preserve\"> {code} </w:instrText></w:r>\
             <w:r><w:rPr><w:rtl/></w:rPr><w:fldChar w:fldCharType=\"separate\"/></w:r>\
             <w:r><w:rPr><w:rtl/></w:rPr><w:t>1</w:t></w:r>\
             <w:r><w:rPr><w:rtl/></w:rPr><w:fldChar w:fldCharType=\"end\"/></w:r>"
        )
    };
    let text = |t: &str| {
        format!("<w:r><w:rPr><w:rtl/><w:color w:val=\"7A7A7A\"/><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr><w:t xml:space=\"preserve\">{t}</w:t></w:r>")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:ftr {W_NS}>\
         <w:p><w:pPr><w:jc w:val=\"center\"/><w:bidi/></w:pPr>{}{}{}{}</w:p></w:ftr>",
        text("עמוד "),
        field("PAGE"),
        text(" מתוך "),
        field("NUMPAGES")
    )
}

pub(crate) fn styles(font: &str) -> String {
    let normal = run_props(24, false, None, Some(font));
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles {W_NS}>\
         <w:docDefaults><w:rPrDefault>{normal}</w:rPrDefault><w:pPrDefault><w:pPr><w:bidi/><w:spacing w:after=\"120\" w:line=\"300\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/><w:pPr><w:bidi/></w:pPr>{normal}</w:style>"
    );
    for (id, name, ppr, size, bold, color) in STYLES {
        out.push_str(&style_def(id, name, ppr, *size, *bold, *color, Some(font)));
    }
    out.push_str("</w:styles>");
    out
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;

pub(crate) const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;

const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rIdSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#;

const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:themeFontLang w:val="he-IL" w:bidi="he-IL"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#;

/// No author, no company, no dates: only a generic title.
pub(crate) const CORE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>דוח אבחון</dc:title></cp:coreProperties>"#;

/// Zip the parts in the order given, with a fixed timestamp: the file does not record when
/// it was made.
pub(crate) fn package(parts: &[(String, Vec<u8>)]) -> Result<Vec<u8>, ExportError> {
    let err = |e: &dyn std::fmt::Display| ExportError::Package(e.to_string());
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for (name, content) in parts {
            zip.start_file(name.as_str(), opts).map_err(|e| err(&e))?;
            zip.write_all(content).map_err(|e| err(&e))?;
        }
        zip.finish().map_err(|e| err(&e))?;
    }
    Ok(buf.into_inner())
}

/// Build the DOCX package: her template when one is given (see `template`), else the plain one.
pub fn render(report: &Report) -> Result<Vec<u8>, ExportError> {
    let parts: Vec<(String, Vec<u8>)> = [
        ("[Content_Types].xml", CONTENT_TYPES.to_owned()),
        ("_rels/.rels", ROOT_RELS.to_owned()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_owned()),
        ("word/document.xml", document(report)),
        ("word/styles.xml", styles(&report.font)),
        ("word/settings.xml", SETTINGS.to_owned()),
        ("word/header1.xml", header(report)),
        ("word/footer1.xml", footer()),
        ("docProps/core.xml", CORE.to_owned()),
    ]
    .into_iter()
    .map(|(n, c)| (n.to_owned(), c.into_bytes()))
    .collect();
    package(&parts)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::tests::sample;

    fn part(docx: &[u8], name: &str) -> String {
        let mut zip = zip::ZipArchive::new(Cursor::new(docx)).unwrap_or_else(|e| panic!("{e}"));
        let mut s = String::new();
        zip.by_name(name)
            .unwrap_or_else(|e| panic!("{e}"))
            .read_to_string(&mut s)
            .unwrap_or_else(|e| panic!("{e}"));
        s
    }

    #[test]
    fn right_to_left_escaped_and_clean() {
        let bytes = render(&sample()).unwrap_or_else(|e| panic!("{e}"));
        let doc = part(&bytes, "word/document.xml");
        assert!(doc.contains("<w:bidi/>") && doc.contains("<w:rtl/>"));
        assert!(doc.contains("סיבת הפניה"));
        assert!(doc.contains("102 (טווח ממוצע) &amp; תצפית"), "escaped");
        assert!(
            doc.contains("נספח: טבלאות ציונים") && doc.contains("<w:bidiVisual/>"),
            "score table"
        );
        assert!(
            doc.contains("<w:tblHeader/>") && doc.contains(">79<"),
            "{doc}"
        );
        let core = part(&bytes, "docProps/core.xml");
        assert!(
            !core.contains("creator") && !core.contains("אלון"),
            "no author, no names in metadata"
        );
        assert!(part(&bytes, "word/header1.xml").contains("חסוי"));
        assert!(part(&bytes, "word/footer1.xml").contains("NUMPAGES"));
    }

    #[test]
    fn deterministic() {
        assert_eq!(render(&sample()).ok(), render(&sample()).ok());
    }

    #[test]
    fn score_chart_is_drawn_with_cells() {
        let mut r = sample();
        r.tables[0].charts.push(crate::ScoreChart {
            title: "פרופיל המדדים".into(),
            min: 40.0,
            max: 160.0,
            step: 5.0,
            mean: 100.0,
            sd: 15.0,
            bars: vec![crate::ChartBar {
                label: "הבנה מילולית (VCI)".into(),
                value: 112.0,
            }],
            note: "הרקע הבהיר: סטיית תקן אחת.".into(),
        });
        let doc = part(&render(&r).unwrap(), "word/document.xml");
        assert!(
            doc.contains("פרופיל המדדים") && doc.contains("85–99"),
            "{doc}"
        );
        // 40..110 filled: 15 bar cells; 115 is tinted (the band ends below 115).
        assert_eq!(doc.matches("w:fill=\"1F3A5F\"").count(), 15);
        assert_eq!(doc.matches("w:fill=\"EEF3F2\"").count(), 0);
        assert!(doc.contains("<w:gridSpan w:val=\"3\"/>"));
    }
}
