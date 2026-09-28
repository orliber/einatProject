//! A minimal, clean WordprocessingML package, right to left.

use std::fmt::Write as _;
use std::io::{Cursor, Write};

use crate::{ExportError, Report};

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

/// Text as XML character data: escaped, and without characters XML 1.0 forbids.
fn esc(s: &str) -> String {
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
fn para(style: &str, text: &str, bold_prefix: Option<&str>) -> String {
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

fn document(report: &Report) -> String {
    let mut body = String::new();
    body.push_str(&para("Title", &report.title, None));
    for line in &report.info {
        body.push_str(&para("Info", &line.value, Some(&line.label)));
    }
    for part in &report.parts {
        body.push_str(&para("Heading1", &part.title, None));
        for section in &part.sections {
            body.push_str(&para("Heading2", &section.title, None));
            for text in &section.paragraphs {
                for piece in text.split('\n').filter(|t| !t.trim().is_empty()) {
                    body.push_str(&para("BodyText", piece.trim(), None));
                }
            }
        }
    }
    if !report.signature.is_empty() {
        body.push_str(&para("Signature", "", None));
        for line in &report.signature {
            body.push_str(&para("Signature", line, None));
        }
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document {W_NS}><w:body>{body}\
         <w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>\
         <w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/>\
         <w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
         <w:pgMar w:top=\"1418\" w:right=\"1418\" w:bottom=\"1418\" w:left=\"1418\" w:header=\"709\" w:footer=\"709\" w:gutter=\"0\"/>\
         <w:bidi/></w:sectPr></w:body></w:document>"
    )
}

fn header(report: &Report) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:hdr {W_NS}>{}</w:hdr>",
        para("Header", &report.confidentiality, None)
    )
}

fn footer() -> String {
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
        format!("<w:r><w:rPr><w:rtl/></w:rPr><w:t xml:space=\"preserve\">{t}</w:t></w:r>")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:ftr {W_NS}>\
         <w:p><w:pPr><w:pStyle w:val=\"Footer\"/><w:bidi/></w:pPr>{}{}{}{}</w:p></w:ftr>",
        text("עמוד "),
        field("PAGE"),
        text(" מתוך "),
        field("NUMPAGES")
    )
}

fn styles(font: &str) -> String {
    let f = esc(font);
    let run = |size: u32, bold: bool, color: Option<&str>| {
        let b = if bold { "<w:b/><w:bCs/>" } else { "" };
        let c = color
            .map(|c| format!("<w:color w:val=\"{c}\"/>"))
            .unwrap_or_default();
        format!("<w:rPr><w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>{b}{c}<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/><w:lang w:val=\"he-IL\" w:bidi=\"he-IL\"/></w:rPr>")
    };
    let style = |id: &str, name: &str, ppr: &str, rpr: String| {
        format!("<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:bidi/>{ppr}</w:pPr>{rpr}</w:style>")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles {W_NS}>\
         <w:docDefaults><w:rPrDefault>{}</w:rPrDefault><w:pPrDefault><w:pPr><w:bidi/><w:spacing w:after=\"120\" w:line=\"300\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/><w:pPr><w:bidi/></w:pPr>{}</w:style>\
         {}{}{}{}{}{}{}{}</w:styles>",
        run(24, false, None),
        run(24, false, None),
        style("Title", "Title", "<w:jc w:val=\"center\"/><w:spacing w:after=\"360\"/>", run(36, true, None)),
        style("Info", "Report Info", "<w:spacing w:after=\"60\"/>", run(24, false, None)),
        style("Heading1", "heading 1", "<w:keepNext/><w:spacing w:before=\"360\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/>", run(30, true, Some("1F3A5F"))),
        style("Heading2", "heading 2", "<w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"1\"/>", run(26, true, None)),
        style("BodyText", "Body Text", "<w:jc w:val=\"both\"/>", run(24, false, None)),
        style("Signature", "Signature", "<w:keepNext/><w:spacing w:after=\"0\"/>", run(24, false, None)),
        style("Header", "header", "<w:jc w:val=\"center\"/>", run(18, false, Some("7A7A7A"))),
        style("Footer", "footer", "<w:jc w:val=\"center\"/>", run(18, false, Some("7A7A7A"))),
    )
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;

const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rIdSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#;

const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:themeFontLang w:val="he-IL" w:bidi="he-IL"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#;

/// No author, no company, no dates: only a generic title.
const CORE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>דוח אבחון</dc:title></cp:coreProperties>"#;

/// Build the DOCX package.
pub fn render(report: &Report) -> Result<Vec<u8>, ExportError> {
    let parts: [(&str, String); 8] = [
        ("[Content_Types].xml", CONTENT_TYPES.to_owned()),
        ("_rels/.rels", ROOT_RELS.to_owned()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_owned()),
        ("word/document.xml", document(report)),
        ("word/styles.xml", styles(&report.font)),
        ("word/settings.xml", SETTINGS.to_owned()),
        ("word/header1.xml", header(report)),
        ("word/footer1.xml", footer()),
    ];
    let err = |e: &dyn std::fmt::Display| ExportError::Package(e.to_string());
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        // Fixed timestamp: the file does not record when it was made.
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for (name, content) in parts
            .iter()
            .chain(std::iter::once(&("docProps/core.xml", CORE.to_owned())))
        {
            zip.start_file(*name, opts).map_err(|e| err(&e))?;
            zip.write_all(content.as_bytes()).map_err(|e| err(&e))?;
        }
        zip.finish().map_err(|e| err(&e))?;
    }
    Ok(buf.into_inner())
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
}
