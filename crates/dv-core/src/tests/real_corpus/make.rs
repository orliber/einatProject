//! Writes the fabricated documents in the formats they really arrive in. Word, ODT, RTF and
//! text are written here, byte by byte, the way Word 365, LibreOffice and Notepad save them.
//! PDFs, scans and photos need LibreOffice (`soffice`), poppler (`pdftoppm`) and ImageMagick
//! (`convert`): without them those documents are skipped, and the report says so.

// Slices at char boundaries (`char_indices`).
#![allow(clippy::string_slice)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::cases::{Block, Doc, Format};

fn esc(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn zip(parts: &[(&str, String)], stored_first: bool) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(&mut buf);
    for (i, (name, content)) in parts.iter().enumerate() {
        let opts = if stored_first && i == 0 {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
        } else {
            zip::write::SimpleFileOptions::default()
        };
        z.start_file(*name, opts).unwrap();
        z.write_all(content.as_bytes()).unwrap();
    }
    z.finish().unwrap();
    buf.into_inner()
}

// --- Word 365 ---------------------------------------------------------------------------

const W_NS: &str = r#"xmlns:wpc="http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:w10="urn:schemas-microsoft-com:office:word" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" mc:Ignorable="w14 w15 wp14""#;
const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

/// Runs as Word writes them: a Hebrew run (`w:rtl`, `w:hint="cs"`) and a Latin/number run
/// (`w:lang en-US`) wherever the script changes, the first long word split in two runs with a
/// proofing mark between them (an rsid change does that), and a bookmark Word left behind.
fn runs(text: &str, rsid: &mut u32) -> String {
    let latin = |c: char| c.is_ascii_alphanumeric();
    let mut chunks: Vec<(bool, String)> = Vec::new();
    for c in text.chars() {
        let l = latin(c);
        match chunks.last_mut() {
            // Spaces and punctuation stay with the run they are in.
            Some((was, s)) if *was == l || !(c.is_alphanumeric()) => s.push(c),
            _ => chunks.push((l, c.to_string())),
        }
    }
    let mut out = String::new();
    let mut split_done = false;
    for (is_latin, chunk) in chunks {
        *rsid += 1;
        if is_latin {
            out.push_str(&format!(
                r#"<w:r w:rsidR="00{rsid:06X}"><w:rPr><w:lang w:val="en-US"/></w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#,
                esc(&chunk)
            ));
            continue;
        }
        let rpr = r#"<w:rPr><w:rFonts w:hint="cs"/><w:rtl/><w:lang w:bidi="he-IL"/></w:rPr>"#;
        // Split one word of at least five letters mid-word, once per paragraph.
        let cut = (!split_done)
            .then(|| {
                let mut at = 0usize;
                for w in chunk.split(' ') {
                    if w.chars().count() >= 5
                        && w.chars().all(|c| ('\u{5d0}'..='\u{5ea}').contains(&c))
                    {
                        let half: usize = w.chars().take(2).map(char::len_utf8).sum();
                        return Some(at + half);
                    }
                    at += w.len() + 1;
                }
                None
            })
            .flatten();
        match cut {
            Some(at) => {
                split_done = true;
                let (a, b) = chunk.split_at(at);
                out.push_str(&format!(
                    r#"<w:r w:rsidR="00{rsid:06X}">{rpr}<w:t xml:space="preserve">{}</w:t></w:r><w:proofErr w:type="spellStart"/><w:bookmarkStart w:id="{rsid}" w:name="_Hlk{rsid}"/><w:r w:rsidR="00{:06X}">{rpr}<w:t xml:space="preserve">{}</w:t></w:r><w:bookmarkEnd w:id="{rsid}"/><w:proofErr w:type="spellEnd"/>"#,
                    esc(a),
                    *rsid + 7,
                    esc(b)
                ));
            }
            None => out.push_str(&format!(
                r#"<w:r w:rsidR="00{rsid:06X}">{rpr}<w:t xml:space="preserve">{}</w:t></w:r>"#,
                esc(&chunk)
            )),
        }
    }
    out
}

fn wpara(style: Option<&str>, num: bool, inner: &str) -> String {
    let style = style.map_or(String::new(), |s| format!(r#"<w:pStyle w:val="{s}"/>"#));
    let num = if num {
        r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#
    } else {
        ""
    };
    format!(
        r#"<w:p w14:paraId="1A2B3C4D" w14:textId="77777777" w:rsidR="00A12B34" w:rsidRDefault="00A12B34" w:rsidP="00C56D78"><w:pPr>{style}{num}<w:bidi/><w:jc w:val="both"/><w:rPr><w:rtl/></w:rPr></w:pPr>{inner}</w:p>"#
    )
}

fn wtable(rows: &[Vec<String>], rsid: &mut u32) -> String {
    let cols = rows.first().map_or(1, Vec::len);
    let grid: String = (0..cols).map(|_| r#"<w:gridCol w:w="2400"/>"#).collect();
    let mut out = format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:bidiVisual/><w:tblW w:w="0" w:type="auto"/><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid>{grid}</w:tblGrid>"#
    );
    for r in rows {
        out.push_str(r#"<w:tr w:rsidR="00A12B34" w14:paraId="2B3C4D5E" w14:textId="77777777">"#);
        for c in r {
            out.push_str(&format!(
                r#"<w:tc><w:tcPr><w:tcW w:w="2400" w:type="dxa"/></w:tcPr>{}</w:tc>"#,
                wpara(None, false, &runs(c, rsid))
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    out
}

fn textbox(text: &str, rsid: &mut u32) -> String {
    let inner = wpara(None, false, &runs(text, rsid));
    format!(
        r##"<w:r><w:rPr><w:noProof/></w:rPr><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor distT="45720" distB="45720" distL="114300" distR="114300" simplePos="0" relativeHeight="251658240" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx="2360930" cy="1404620"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="217" name="Text Box 2"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:cNvSpPr txBox="1"/><wps:spPr/><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict><v:shape id="Text Box 2" o:spid="_x0000_s1026" type="#_x0000_t202"><v:textbox><w:txbxContent>{inner}</w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r>"##
    )
}

/// A .docx as Word 365 saves it. `main` is `document.xml` or `document2.xml`.
pub(super) fn docx(doc: &Doc, main: &str) -> Vec<u8> {
    let mut rsid = 0x1000u32;
    let mut body = String::new();
    let mut notes = Vec::new();
    let mut first_para = true;
    let blocks = &doc.blocks;
    for (i, b) in blocks.iter().enumerate() {
        let next_note = matches!(blocks.get(i + 1), Some(Block::Note(_)));
        match b {
            Block::H(t) => body.push_str(&wpara(Some("Heading1"), false, &runs(t, &mut rsid))),
            Block::P(t) | Block::Li(t) => {
                let mut inner = String::new();
                if first_para && matches!(b, Block::P(_)) {
                    first_para = false;
                    if let Some(c) = &doc.comment {
                        let _ = c;
                        inner.push_str(r#"<w:commentRangeStart w:id="0"/>"#);
                    }
                    if let Some(t) = &doc.textbox {
                        inner.push_str(&textbox(t, &mut rsid));
                    }
                    inner.push_str(&runs(t, &mut rsid));
                    if doc.comment.is_some() {
                        inner.push_str(r#"<w:commentRangeEnd w:id="0"/><w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="0"/></w:r>"#);
                    }
                    if let Some(d) = &doc.deleted {
                        inner.push_str(&format!(
                            r#"<w:del w:id="91" w:author="מכון בדוי" w:date="2026-08-30T10:12:00Z"><w:r><w:rPr><w:rtl/></w:rPr><w:delText xml:space="preserve"> {}</w:delText></w:r></w:del>"#,
                            esc(d)
                        ));
                    }
                    // Hidden text Word keeps but does not print.
                    inner.push_str(r#"<w:r><w:rPr><w:vanish/><w:rtl/></w:rPr><w:t xml:space="preserve"> טקסט נסתר לבדיקה</w:t></w:r>"#);
                } else {
                    inner.push_str(&runs(t, &mut rsid));
                }
                if next_note {
                    notes.push(format!(
                        r#"<w:footnote w:id="{}"><w:p><w:pPr><w:pStyle w:val="FootnoteText"/><w:bidi/></w:pPr><w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> </w:t></w:r>{}</w:p></w:footnote>"#,
                        notes.len() + 1,
                        runs(
                            match blocks.get(i + 1) {
                                Some(Block::Note(n)) => n,
                                _ => "",
                            },
                            &mut rsid
                        )
                    ));
                    inner.push_str(&format!(
                        r#"<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/><w:rtl/></w:rPr><w:footnoteReference w:id="{}"/></w:r>"#,
                        notes.len()
                    ));
                }
                body.push_str(&wpara(None, matches!(b, Block::Li(_)), &inner));
            }
            Block::Table(rows) => body.push_str(&wtable(rows, &mut rsid)),
            Block::Note(_) => {}
            Block::Break => body.push_str(
                r#"<w:p><w:pPr><w:bidi/></w:pPr><w:r><w:br w:type="page"/></w:r><w:r><w:lastRenderedPageBreak/></w:r></w:p>"#,
            ),
        }
    }
    let sect = r#"<w:sectPr w:rsidR="00A12B34" w:rsidSect="00C56D78"><w:headerReference w:type="default" r:id="rId8"/><w:footerReference w:type="default" r:id="rId9"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:bidi/><w:rtlGutter/><w:docGrid w:linePitch="360"/></w:sectPr>"#;
    let document =
        format!(r#"{XML_DECL}<w:document {W_NS}><w:body>{body}{sect}</w:body></w:document>"#);
    let margin = |tag: &str, t: Option<&String>, page: bool| {
        let field = if page {
            r#"<w:r><w:t xml:space="preserve"> · עמוד </w:t></w:r><w:fldSimple w:instr=" PAGE   \* MERGEFORMAT "><w:r><w:rPr><w:noProof/></w:rPr><w:t>1</w:t></w:r></w:fldSimple>"#
        } else {
            ""
        };
        let text = t.map_or(String::new(), |t| esc(t));
        format!(
            r#"{XML_DECL}<w:{tag} {W_NS}><w:p><w:pPr><w:pStyle w:val="Header"/><w:bidi/></w:pPr><w:r><w:rPr><w:rtl/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>{field}</w:p></w:{tag}>"#
        )
    };
    let footnotes = format!(
        r#"{XML_DECL}<w:footnotes {W_NS}><w:footnote w:type="separator" w:id="-1"><w:p><w:pPr><w:bidi/><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:pPr><w:bidi/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{}</w:footnotes>"#,
        notes.join("")
    );
    let endnotes = format!(
        r#"{XML_DECL}<w:endnotes {W_NS}><w:endnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:endnote><w:endnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:endnote></w:endnotes>"#
    );
    let comments = doc.comment.as_ref().map(|c| {
        format!(
            r#"{XML_DECL}<w:comments {W_NS}><w:comment w:id="0" w:author="{}" w:date="2026-08-30T09:00:00Z" w:initials="מב"><w:p><w:pPr><w:pStyle w:val="CommentText"/><w:bidi/></w:pPr><w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:annotationRef/></w:r><w:r><w:rPr><w:rtl/></w:rPr><w:t>{}</w:t></w:r></w:p></w:comment></w:comments>"#,
            esc(doc.creator.as_deref().unwrap_or("מחבר")),
            esc(c)
        )
    });
    let styles = format!(
        r#"{XML_DECL}<w:styles {W_NS}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:eastAsiaTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi" w:cstheme="minorBidi"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="he-IL"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:bidi/><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val="32"/></w:rPr></w:style><w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr></w:style><w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="footnote text"/></w:style><w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Header"><w:name w:val="header"/></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"{XML_DECL}<w:numbering {W_NS}><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val=""/><w:lvlJc w:val="left"/><w:pPr><w:bidi/><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    let settings = format!(
        r#"{XML_DECL}<w:settings {W_NS}><w:zoom w:percent="100"/><w:proofState w:spelling="clean" w:grammar="clean"/><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat><w:rsids><w:rsidRoot w:val="00A12B34"/></w:rsids><w:themeFontLang w:val="en-US" w:bidi="he-IL"/></w:settings>"#
    );
    let main_name = format!("word/{main}");
    let rels_name = format!("word/_rels/{main}.rels");
    let mut doc_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/><Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>"#,
    );
    if comments.is_some() {
        doc_rels.push_str(r#"<Relationship Id="rId10" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/>"#);
    }
    doc_rels.push_str("</Relationships>");
    let ct = |part: &str, t: &str| {
        format!(
            r#"<Override PartName="/word/{part}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{t}+xml"/>"#
        )
    };
    let content_types = format!(
        r#"{XML_DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{}{}{}{}{}{}{}{}{}<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#,
        ct(main, "document.main"),
        ct("numbering.xml", "numbering"),
        ct("styles.xml", "styles"),
        ct("settings.xml", "settings"),
        ct("footnotes.xml", "footnotes"),
        ct("endnotes.xml", "endnotes"),
        ct("header1.xml", "header"),
        ct("footer1.xml", "footer"),
        if comments.is_some() {
            ct("comments.xml", "comments")
        } else {
            String::new()
        },
    );
    let root_rels = format!(
        r#"{XML_DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/{main}"/></Relationships>"#
    );
    let creator = doc.creator.as_deref().map(esc).unwrap_or_default();
    let core = format!(
        r#"{XML_DECL}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title></dc:title><dc:subject></dc:subject><dc:creator>{creator}</dc:creator><cp:keywords></cp:keywords><dc:description></dc:description><cp:lastModifiedBy>{creator}</cp:lastModifiedBy><cp:revision>4</cp:revision><dcterms:created xsi:type="dcterms:W3CDTF">2026-08-30T08:00:00Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2026-08-30T09:41:00Z</dcterms:modified></cp:coreProperties>"#
    );
    let company = doc.company.as_deref().map(esc).unwrap_or_default();
    let app = format!(
        r#"{XML_DECL}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Template>Normal.dotm</Template><TotalTime>41</TotalTime><Pages>2</Pages><Application>Microsoft Office Word</Application><DocSecurity>0</DocSecurity><Company>{company}</Company><AppVersion>16.0000</AppVersion></Properties>"#
    );
    let mut parts: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", root_rels),
        ("docProps/core.xml", core),
        ("docProps/app.xml", app),
        (main_name.as_str(), document),
        (rels_name.as_str(), doc_rels),
        ("word/styles.xml", styles),
        ("word/numbering.xml", numbering),
        ("word/settings.xml", settings),
        ("word/footnotes.xml", footnotes),
        ("word/endnotes.xml", endnotes),
        (
            "word/header1.xml",
            margin("hdr", doc.header.as_ref(), false),
        ),
        ("word/footer1.xml", margin("ftr", doc.footer.as_ref(), true)),
    ];
    if let Some(c) = comments {
        parts.push(("word/comments.xml", c));
    }
    zip(&parts, false)
}

// --- ODT --------------------------------------------------------------------------------

const ODF_NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3""#;

/// Spaces as LibreOffice writes them: a run of spaces becomes `<text:s text:c="N"/>`.
fn odt_text(t: &str) -> String {
    let mut out = String::new();
    let mut spaces = 0usize;
    let flush = |out: &mut String, spaces: &mut usize| {
        match *spaces {
            0 => {}
            1 => out.push(' '),
            n => {
                out.push(' ');
                out.push_str(&format!(r#"<text:s text:c="{}"/>"#, n - 1));
            }
        }
        *spaces = 0;
    };
    for c in t.chars() {
        if c == ' ' {
            spaces += 1;
        } else {
            flush(&mut out, &mut spaces);
            out.push_str(&esc(&c.to_string()));
        }
    }
    flush(&mut out, &mut spaces);
    // English in its own span, as LibreOffice marks the language change.
    out
}

pub(super) fn odt(doc: &Doc) -> Vec<u8> {
    let mut body = String::new();
    let blocks = &doc.blocks;
    let mut in_list = false;
    let mut first = true;
    for (i, b) in blocks.iter().enumerate() {
        if in_list && !matches!(b, Block::Li(_)) {
            body.push_str("</text:list>");
            in_list = false;
        }
        match b {
            Block::H(t) => body.push_str(&format!(
                r#"<text:h text:style-name="Heading_20_1" text:outline-level="1">{}</text:h>"#,
                odt_text(t)
            )),
            Block::P(t) => {
                let mut inner = String::new();
                if first {
                    first = false;
                    if let Some(c) = &doc.comment {
                        inner.push_str(&format!(
                            r#"<office:annotation office:name="__Annotation__1"><dc:creator>{}</dc:creator><dc:date>2026-08-29T10:00:00</dc:date><text:p>{}</text:p></office:annotation>"#,
                            esc(doc.creator.as_deref().unwrap_or("")),
                            esc(c)
                        ));
                    }
                }
                // The words split over two spans, as an edit leaves them.
                let mid = t
                    .char_indices()
                    .nth(t.chars().count() / 2)
                    .map_or(0, |(i, _)| i);
                inner.push_str(&format!(
                    r#"<text:span text:style-name="T1">{}</text:span>{}"#,
                    odt_text(&t[..mid]),
                    odt_text(&t[mid..])
                ));
                if let Some(Block::Note(n)) = blocks.get(i + 1) {
                    inner.push_str(&format!(
                        r#"<text:note text:id="ftn1" text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p text:style-name="Footnote">{}</text:p></text:note-body></text:note>"#,
                        odt_text(n)
                    ));
                }
                body.push_str(&format!(r#"<text:p text:style-name="P1">{inner}</text:p>"#));
            }
            Block::Li(t) => {
                if !in_list {
                    body.push_str(r#"<text:list text:style-name="L1">"#);
                    in_list = true;
                }
                body.push_str(&format!(
                    r#"<text:list-item><text:p text:style-name="P2">{}</text:p></text:list-item>"#,
                    odt_text(t)
                ));
            }
            Block::Table(rows) => {
                let cols = rows.first().map_or(1, Vec::len);
                body.push_str(&format!(
                    r#"<table:table table:name="טבלה1" table:style-name="Table1"><table:table-column table:style-name="Table1.A" table:number-columns-repeated="{cols}"/>"#
                ));
                for r in rows {
                    body.push_str("<table:table-row>");
                    for c in r {
                        body.push_str(&format!(
                            r#"<table:table-cell table:style-name="Table1.A1" office:value-type="string"><text:p text:style-name="Table_20_Contents">{}</text:p></table:table-cell>"#,
                            odt_text(c)
                        ));
                    }
                    body.push_str("</table:table-row>");
                }
                body.push_str("</table:table>");
            }
            Block::Note(_) => {}
            Block::Break => body.push_str(r#"<text:p text:style-name="PageBreak"/>"#),
        }
    }
    if in_list {
        body.push_str("</text:list>");
    }
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {ODF_NS}><office:automatic-styles><style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="end" style:writing-mode="rl-tb"/></style:style></office:automatic-styles><office:body><office:text><text:sequence-decls/>{body}</office:text></office:body></office:document-content>"#
    );
    let header = doc.header.as_ref().map_or(String::new(), |h| {
        format!("<style:header><text:p>{}</text:p></style:header>", esc(h))
    });
    let footer = doc.footer.as_ref().map_or(String::new(), |h| {
        format!("<style:footer><text:p>{}</text:p></style:footer>", esc(h))
    });
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {ODF_NS}><office:master-styles><style:master-page style:name="Standard" style:page-layout-name="pm1">{header}{footer}</style:master-page></office:master-styles></office:document-styles>"#
    );
    let creator = doc.creator.as_deref().map(esc).unwrap_or_default();
    let meta = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta {ODF_NS}><office:meta><meta:initial-creator>{creator}</meta:initial-creator><dc:creator>{creator}</dc:creator><meta:generator>LibreOffice/24.2$Linux_X86_64</meta:generator></office:meta></office:document-meta>"#
    );
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    zip(
        &[
            (
                "mimetype",
                "application/vnd.oasis.opendocument.text".to_owned(),
            ),
            ("content.xml", content),
            ("styles.xml", styles),
            ("meta.xml", meta),
            ("META-INF/manifest.xml", manifest.to_owned()),
        ],
        true,
    )
}

// --- Windows-1255 and RTF ----------------------------------------------------------------

/// Windows-1255 for the letters, digits and punctuation the corpus uses.
pub(super) fn cp1255(t: &str) -> Vec<u8> {
    t.chars()
        .map(|c| match c {
            '\u{5d0}'..='\u{5ea}' => 0xE0 + (c as u32 - 0x5d0) as u8,
            '–' => 0x96,
            '—' => 0x97,
            '״' => b'"',
            '׳' => b'\'',
            c if c.is_ascii() => c as u8,
            _ => b'?',
        })
        .collect()
}

fn rtf_text(t: &str) -> String {
    let mut out = String::new();
    for (i, c) in t.chars().enumerate() {
        match c {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            // Word writes a few letters as \u with the code page byte as the fallback.
            '\u{5d0}'..='\u{5ea}' if i % 17 == 0 => {
                let b = 0xE0 + (c as u32 - 0x5d0);
                out.push_str(&format!("\\u{}\\'{b:02x}", c as u32));
            }
            '\u{5d0}'..='\u{5ea}' | '–' | '—' => {
                out.push_str(&format!("\\'{:02x}", cp1255(&c.to_string())[0]));
            }
            c if c.is_ascii() => out.push(c),
            c => out.push_str(&format!("\\u{}?", c as u32)),
        }
    }
    out
}

pub(super) fn rtf(doc: &Doc) -> Vec<u8> {
    let par = |t: &str, bold: bool| {
        format!(
            "\\pard\\plain \\rtlpar\\qr \\li0\\ri0\\sa160\\sl259\\slmult1\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\adjustright\\rin0\\lin0\\itap0 \\rtlch\\fcs1 \\af1\\afs24\\alang1037 \\ltrch\\fcs0 \\fs24\\lang1033\\langfe1033\\cgrid\\langnp1033\\langfenp1033 {{\\rtlch\\fcs1 \\af1{} \\ltrch\\fcs0 \\lang1037\\langfe1033\\langnp1037\\insrsid4613823 {}}}\\par\r\n",
            if bold { "\\ab" } else { "" },
            rtf_text(t)
        )
    };
    let mut out = String::from("{\\rtf1\\adeflang1037\\ansi\\ansicpg1255\\uc1\\adeff1\\deff0\\stshfdbch0\\stshfloch0\\stshfhich0\\stshfbi0\\deflang1033\\deflangfe1033\\themelang1033\\themelangfe0\\themelangcs1037{\\fonttbl{\\f0\\fbidi \\froman\\fcharset0\\fprq2{\\*\\panose 02020603050405020304}Times New Roman;}{\\f1\\fbidi \\fswiss\\fcharset177\\fprq2{\\*\\panose 020b0604020202020204}Arial;}}\r\n{\\colortbl;\\red0\\green0\\blue0;}\r\n{\\stylesheet{\\ql \\li0\\ri0\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\adjustright\\rin0\\lin0\\itap0 \\rtlch\\fcs1 \\af1\\afs24\\alang1037 \\ltrch\\fcs0 \\fs24\\lang1033\\langfe1033\\cgrid\\langnp1033\\langfenp1033 \\snext0 \\sqformat \\spriority0 Normal;}}\r\n");
    if let Some(c) = &doc.creator {
        out.push_str(&format!(
            "{{\\info{{\\author {}}}{{\\operator {}}}{{\\creatim\\yr2026\\mo8\\dy20\\hr10\\min5}}}}\r\n",
            rtf_text(c),
            rtf_text(c)
        ));
    }
    out.push_str("\\paperw11906\\paperh16838\\margl1800\\margr1800\\margt1440\\margb1440\\rtlsect\\sectd \\rtlsect\\linex0\\headery708\\footery708\\sectdefaultcl\r\n");
    if let Some(hd) = &doc.header {
        out.push_str(&format!("{{\\headerr {}}}\r\n", par(hd, false)));
    }
    let blocks = &doc.blocks;
    for (i, b) in blocks.iter().enumerate() {
        match b {
            Block::H(t) => out.push_str(&par(t, true)),
            Block::P(t) | Block::Li(t) => {
                if let Some(Block::Note(n)) = blocks.get(i + 1) {
                    out.push_str(&par(t, false).replace(
                        "}\\par\r\n",
                        &format!(
                            "}}{{\\rtlch\\fcs1 \\af1 \\ltrch\\fcs0 \\cs15\\super \\chftn {{\\footnote \\ltrpar \\pard\\plain \\rtlpar\\qr {{\\rtlch\\fcs1 \\af1 \\ltrch\\fcs0 \\cs15\\super \\chftn }}{{\\rtlch\\fcs1 \\af1 \\ltrch\\fcs0  {}}}\\par }}}}\\par\r\n",
                            rtf_text(n)
                        ),
                    ));
                } else {
                    out.push_str(&par(t, false));
                }
            }
            Block::Table(rows) => {
                for (ri, r) in rows.iter().enumerate() {
                    out.push_str(&format!("\\trowd \\irow{ri}\\irowband{ri}\\rtlrow\\ts11\\trgaph108\\trleft-108\\trbrdrt\\brdrs\\brdrw10 "));
                    for ci in 0..r.len() {
                        out.push_str(&format!(
                            "\\clvertalt\\clbrdrt\\brdrs\\brdrw10 \\cltxlrtb\\clftsWidth3\\clwWidth2880\\clshdrawnil \\cellx{}",
                            (ci + 1) * 2880
                        ));
                    }
                    out.push_str("\r\n\\pard\\plain \\rtlpar\\qr \\intbl ");
                    for c in r {
                        out.push_str(&format!(
                            "{{\\rtlch\\fcs1 \\af1 \\ltrch\\fcs0 \\lang1037 {}}}\\cell ",
                            rtf_text(c)
                        ));
                    }
                    out.push_str("\\pard\\plain \\rtlpar\\intbl {\\row }\r\n");
                }
            }
            Block::Note(_) | Block::Break => {}
        }
    }
    out.push_str("}\r\n");
    out.into_bytes()
}

pub(super) fn text(doc: &Doc, legacy: bool) -> Vec<u8> {
    let joined = doc.lines().join("\r\n") + "\r\n";
    if legacy {
        cp1255(&joined)
    } else {
        let mut out = vec![0xEF, 0xBB, 0xBF];
        out.extend_from_slice(joined.as_bytes());
        out
    }
}

// --- PDFs, scans and photos (external tools) ----------------------------------------------

pub(super) fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
        || Command::new(tool)
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
}

fn run(cmd: &mut Command) -> bool {
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// LibreOffice turns the Word files into PDFs, all in one run, in its own profile (another
/// LibreOffice running on the machine does not block it).
pub(super) fn pdfs(work: &Path, docx_files: &[PathBuf]) -> bool {
    if docx_files.is_empty() {
        return true;
    }
    let profile = work.join("lo-profile");
    let mut cmd = Command::new("soffice");
    cmd.arg(format!(
        "-env:UserInstallation=file://{}",
        profile.display()
    ))
    .args(["--headless", "--convert-to", "pdf", "--outdir"])
    .arg(work);
    for f in docx_files {
        cmd.arg(f);
    }
    run(&mut cmd)
}

/// The size of a JPEG, from its frame header.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        let len = usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
        if matches!(marker, 0xC0..=0xC3) {
            let hgt = u32::from(u16::from_be_bytes([b[i + 5], b[i + 6]]));
            let wid = u32::from(u16::from_be_bytes([b[i + 7], b[i + 8]]));
            return Some((wid, hgt));
        }
        i += 2 + len;
    }
    None
}

/// A PDF whose pages are JPEG images (DCTDecode), as an office scanner makes it.
pub(super) fn scan_pdf(pages: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n = pages.len();
    // 1 catalog, 2 pages, then per page: page, content, image.
    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: String = (0..n).map(|i| format!("{} 0 R ", 3 + i * 3)).collect();
    objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").into_bytes());
    for (i, jpg) in pages.iter().enumerate() {
        let (w, h) = jpeg_size(jpg).unwrap_or((2480, 3508));
        let page = 3 + i * 3;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                page + 2,
                page + 1
            )
            .into_bytes(),
        );
        let content = b"q 595 0 0 842 0 0 cm /Im0 Do Q".to_vec();
        let mut c = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        c.extend_from_slice(&content);
        c.extend_from_slice(b"\nendstream");
        objects.push(c);
        let mut im = format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpg.len()
        )
        .into_bytes();
        im.extend_from_slice(jpg);
        im.extend_from_slice(b"\nendstream");
        objects.push(im);
    }
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The pages of a PDF as gray JPEG scans at `dpi`, each turned a little and with scanner
/// noise. Returns the JPEG bytes page by page.
pub(super) fn scan_pages(work: &Path, pdf: &Path, stem: &str, dpi: u32) -> Option<Vec<Vec<u8>>> {
    let prefix = work.join(format!("{stem}-page"));
    if !run(Command::new("pdftoppm")
        .args(["-r", &dpi.to_string(), "-gray", "-png"])
        .arg(pdf)
        .arg(&prefix))
    {
        return None;
    }
    let mut pngs: Vec<PathBuf> = std::fs::read_dir(work)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{stem}-page")) && n.ends_with(".png"))
        })
        .collect();
    pngs.sort();
    let mut out = Vec::new();
    for (i, png) in pngs.iter().enumerate() {
        let jpg = png.with_extension("jpg");
        let angle = if i % 2 == 0 { "0.6" } else { "-0.4" };
        if !run(Command::new("convert")
            .arg(png)
            .args(["-background", "white", "-rotate", angle])
            .args(["-attenuate", "0.25", "+noise", "Gaussian"])
            .args(["-colorspace", "Gray", "-quality", "75"])
            .arg(&jpg))
        {
            return None;
        }
        out.push(std::fs::read(&jpg).ok()?);
    }
    Some(out)
}

/// A phone photo of the first page: 12 MP, turned 3.5°, a desk around the page, soft focus.
pub(super) fn photo(work: &Path, pdf: &Path, stem: &str) -> Option<Vec<u8>> {
    let prefix = work.join(format!("{stem}-photo"));
    if !run(Command::new("pdftoppm")
        .args(["-r", "300", "-f", "1", "-l", "1", "-png"])
        .arg(pdf)
        .arg(&prefix))
    {
        return None;
    }
    let png = std::fs::read_dir(work)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{stem}-photo")) && n.ends_with(".png"))
        })?;
    let jpg = work.join(format!("{stem}.jpg"));
    if !run(Command::new("convert")
        .arg(&png)
        .args([
            "-resize",
            "2700x",
            "-background",
            "#6f6a60",
            "-rotate",
            "3.5",
        ])
        .args(["-gravity", "center", "-extent", "3024x4032"])
        .args(["-fill", "#00000018", "-draw", "rectangle 0,0 3024,1300"])
        .args(["-attenuate", "0.4", "+noise", "Gaussian", "-blur", "0x0.7"])
        .args(["-quality", "85"])
        .arg(&jpg))
    {
        return None;
    }
    std::fs::read(&jpg).ok()
}

/// What could not be made on this machine, by file.
pub(super) struct Made {
    pub files: Vec<(Doc, Vec<u8>)>,
    pub skipped: Vec<(String, &'static str)>,
    pub took: std::time::Duration,
}

/// Every document of `docs`, in its format.
pub(super) fn make_all(work: &Path, docs: &[Doc]) -> Made {
    let started = std::time::Instant::now();
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    // Documents that go through LibreOffice: the PDFs and the page images.
    let mut via_pdf: Vec<(usize, PathBuf)> = Vec::new();
    for (i, d) in docs.iter().enumerate() {
        let bytes = match d.format {
            Format::Docx => docx(d, "document.xml"),
            Format::Docx2 => docx(d, "document2.xml"),
            Format::Odt => odt(d),
            Format::Rtf => rtf(d),
            Format::TextUtf8 => text(d, false),
            Format::Text1255 => text(d, true),
            Format::Pdf | Format::ScanPdf | Format::Photo => {
                let src = work.join(format!("src{i:02}.docx"));
                std::fs::write(&src, docx(d, "document.xml")).unwrap();
                via_pdf.push((i, src));
                continue;
            }
        };
        files.push((d.clone(), bytes));
    }
    let tools = have("soffice") && have("pdftoppm") && have("convert");
    let sources: Vec<PathBuf> = via_pdf.iter().map(|(_, p)| p.clone()).collect();
    let converted = tools && pdfs(work, &sources);
    for (i, src) in via_pdf {
        let d = &docs[i];
        let pdf = src.with_extension("pdf");
        if !converted || !pdf.is_file() {
            skipped.push((d.file.clone(), "LibreOffice/poppler/ImageMagick missing"));
            continue;
        }
        let stem = format!("src{i:02}");
        let bytes = match d.format {
            Format::Pdf => std::fs::read(&pdf).ok(),
            Format::ScanPdf => scan_pages(work, &pdf, &stem, 300).map(|p| scan_pdf(&p)),
            Format::Photo => photo(work, &pdf, &stem),
            _ => None,
        };
        match bytes {
            Some(b) => files.push((d.clone(), b)),
            None => skipped.push((d.file.clone(), "rendering failed")),
        }
    }
    Made {
        files,
        skipped,
        took: started.elapsed(),
    }
}
