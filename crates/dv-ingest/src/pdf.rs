//! PDF with a text layer. Scanned PDFs (images only) are refused with a clear message.
//!
//! Producers store Hebrew in different orders (LibreOffice, Word and browsers all differ), so
//! the text order in the file cannot be trusted. Every character's position on the page is
//! collected instead; lines are rebuilt from the positions (true visual order) and then
//! turned into logical order. Lines that repeat at the top or bottom of most pages
//! (letterhead, patient name, page numbers) are moved out of the body into `margins`.

use std::collections::HashMap;

use pdf_extract::{Document, MediaBox, OutputDev, OutputError, Transform};

use crate::{rtl, Extracted, Format, IngestError};

#[derive(Debug, Clone)]
struct Glyph {
    x: f64,
    end: f64,
    y: f64,
    size: f64,
    text: String,
}

/// One page's characters and its vertical extent.
#[derive(Debug, Default)]
struct Page {
    glyphs: Vec<Glyph>,
    bottom: f64,
    top: f64,
}

/// Collects positioned characters per page.
#[derive(Debug, Default)]
struct Positions {
    pages: Vec<Page>,
    current: Page,
}

impl OutputDev for Positions {
    fn begin_page(
        &mut self,
        _: u32,
        media: &MediaBox,
        _: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.current = Page {
            glyphs: Vec::new(),
            bottom: media.lly,
            top: media.ury,
        };
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), OutputError> {
        self.pages.push(std::mem::take(&mut self.current));
        Ok(())
    }
    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _: f64,
        font_size: f64,
        text: &str,
    ) -> Result<(), OutputError> {
        let vx = font_size * (trm.m11 + trm.m21);
        let vy = font_size * (trm.m12 + trm.m22);
        let size = (vx * vy).abs().sqrt().max(0.1);
        self.current.glyphs.push(Glyph {
            x: trm.m31,
            end: trm.m31 + width * size,
            y: trm.m32,
            size,
            text: text.to_owned(),
        });
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

use rtl::is_hebrew;

/// A rebuilt line and whether it sits in the page's top or bottom margin zone.
struct Line {
    text: String,
    in_margin_zone: bool,
    /// Height on the page and font size, to tell a header apart from the text below it.
    y: f64,
    size: f64,
}

/// Share of the page height, at the top and at the bottom, where headers and footers sit.
const MARGIN_ZONE: f64 = 0.1;

/// Rebuild the page's lines from positions, top to bottom, in logical order.
fn page_lines(page: Page) -> Vec<Line> {
    let Page {
        mut glyphs,
        bottom,
        top,
    } = page;
    let height = (top - bottom).max(1.0);
    glyphs.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut lines: Vec<Vec<Glyph>> = Vec::new();
    for g in glyphs {
        match lines.last_mut() {
            Some(line)
                if line
                    .first()
                    .is_some_and(|f| (f.y - g.y).abs() <= f.size.max(g.size) * 0.5) =>
            {
                line.push(g)
            }
            _ => lines.push(vec![g]),
        }
    }
    lines
        .into_iter()
        .map(|mut line| {
            line.sort_by(|a, b| a.x.total_cmp(&b.x));
            let mut visual = String::new();
            let mut last: Option<&Glyph> = None;
            for g in &line {
                if let Some(prev) = last {
                    let gap = g.x - prev.end;
                    let spaced = prev.text.ends_with(char::is_whitespace)
                        || g.text.starts_with(char::is_whitespace);
                    if !spaced && gap > g.size * 2.5 {
                        visual.push('\t');
                    } else if !spaced && gap > g.size * 0.1 {
                        visual.push(' ');
                    }
                }
                visual.push_str(&g.text);
                last = Some(g);
            }
            let visual = visual
                .split(' ')
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let rel = line.first().map_or(0.5, |g| (g.y - bottom) / height);
            Line {
                text: logical(&visual),
                in_margin_zone: !(MARGIN_ZONE..=1.0 - MARGIN_ZONE).contains(&rel),
                y: line.first().map_or(0.0, |g| g.y),
                size: line.iter().map(|g| g.size).fold(0.0, f64::max),
            }
        })
        .collect()
}

/// Visual (left-to-right) order → logical order. Mostly-Hebrew lines are right-to-left
/// paragraphs; in a mostly-Latin line only the Hebrew runs are reversed.
fn logical(visual: &str) -> String {
    let hebrew = visual.chars().filter(|c| is_hebrew(*c)).count();
    let latin = visual.chars().filter(char::is_ascii_alphabetic).count();
    if hebrew == 0 {
        return visual.to_owned();
    }
    if hebrew >= latin {
        return rtl::reverse_line(visual);
    }
    // Left-to-right paragraph: reverse each Hebrew run (with the spaces inside it).
    let chars: Vec<char> = visual.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if is_hebrew(chars[i]) {
            let mut j = i;
            let mut end = i;
            while j < chars.len() && (is_hebrew(chars[j]) || chars[j] == ' ') {
                if is_hebrew(chars[j]) {
                    end = j;
                }
                j += 1;
            }
            out.extend(chars[i..=end].iter().rev());
            i = end + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn load(bytes: &[u8]) -> Result<Document, IngestError> {
    let mut doc = Document::load_mem(bytes).map_err(|e| IngestError::Corrupt(e.to_string()))?;
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return Err(IngestError::Encrypted);
    }
    Ok(doc)
}

fn edge_key(line: &str) -> String {
    // Page numbers differ per page: "עמוד 2 מתוך 5" and "עמוד 3 מתוך 5" are the same line.
    line.trim()
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

/// On a single page: the lines in the margin zone at the top and at the bottom that a gap
/// (more than 1.8 times their font size) separates from the text next to them.
fn set_apart(page: &[Line]) -> Vec<(usize, usize)> {
    let lines: Vec<usize> = (0..page.len())
        .filter(|i| !page[*i].text.trim().is_empty())
        .collect();
    let mut out = Vec::new();
    for order in [lines.clone(), lines.into_iter().rev().collect::<Vec<_>>()] {
        let mut edge = Vec::new();
        for (k, &i) in order.iter().enumerate() {
            if !page[i].in_margin_zone {
                break;
            }
            edge.push(i);
            let Some(&next) = order.get(k + 1) else { break };
            if (page[i].y - page[next].y).abs() > page[i].size * 1.8 {
                out.extend(edge.iter().map(|i| (0, *i)));
                break;
            }
        }
    }
    out
}

pub(crate) fn extract(bytes: &[u8]) -> Result<Extracted, IngestError> {
    let doc = load(bytes)?;
    let mut positions = Positions::default();
    pdf_extract::output_doc(&doc, &mut positions)
        .map_err(|e| IngestError::Corrupt(e.to_string()))?;
    let pages: Vec<Vec<Line>> = positions.pages.into_iter().map(page_lines).collect();
    let count = pages.len().max(1);
    let letters = pages
        .iter()
        .flatten()
        .flat_map(|l| l.text.chars())
        .filter(|c| c.is_alphabetic())
        .count();
    if letters < 40 || letters / count < 15 {
        return Err(IngestError::Scanned);
    }
    let mut warnings =
        vec!["נקרא מ-PDF: כדאי לעבור על הטקסט (טבלאות ומספרים בתוך משפטים).".to_owned()];

    // Headers and footers: in the margin zone, and repeated on most pages.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for page in &pages {
        let mut keys: Vec<String> = page
            .iter()
            .filter(|l| l.in_margin_zone)
            .map(|l| edge_key(&l.text))
            .collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            *seen.entry(k).or_default() += 1;
        }
    }
    let threshold = (pages.len() * 3).div_ceil(5).max(2);
    // A one-page letter has nothing to repeat: its letterhead and footer are the lines at the
    // page's edge, set apart from the text by a wider gap than the text's own line spacing.
    let lone: Vec<(usize, usize)> = match pages.as_slice() {
        [page] => set_apart(page),
        _ => Vec::new(),
    };
    let is_margin = |p: usize, i: usize, l: &Line| {
        l.in_margin_zone
            && (lone.contains(&(p, i))
                || seen
                    .get(&edge_key(&l.text))
                    .is_some_and(|n| *n >= threshold))
    };

    let mut body = String::new();
    let mut margins: Vec<String> = Vec::new();
    for (p, page) in pages.iter().enumerate() {
        for (i, line) in page.iter().enumerate() {
            if line.text.trim().is_empty() {
                continue;
            }
            if is_margin(p, i, line) {
                let t = line.text.trim().to_owned();
                if !margins.contains(&t) {
                    margins.push(t);
                }
            } else {
                body.push_str(&line.text);
                body.push('\n');
            }
        }
        body.push('\n');
    }
    if !margins.is_empty() {
        warnings.push("שורות בשולי העמוד (כותרת עליונה/תחתונה, מספרי עמודים) לא יובאו.".to_owned());
    }
    let (metadata, extra) = hidden_parts(&doc);
    warnings.extend(extra);
    Ok(Extracted {
        format: Format::Pdf,
        body,
        margins: margins.join("\n"),
        metadata,
        warnings,
        pages: u32::try_from(pages.len()).unwrap_or(u32::MAX),
    })
}

/// What a PDF carries besides its pages: file properties (shown locally, and used to suggest
/// names to hide, like a Word file's), and comments or form fields, which are not imported.
fn hidden_parts(doc: &Document) -> (Vec<(String, String)>, Vec<String>) {
    let mut metadata = Vec::new();
    let info = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| o.as_reference().ok())
        .and_then(|id| doc.get_dictionary(id).ok());
    if let Some(info) = info {
        for (key, label) in [
            (&b"Author"[..], "יוצר"),
            (b"Title", "כותרת"),
            (b"Subject", "נושא"),
            (b"Keywords", "מילות מפתח"),
            (b"Creator", "נוצר בתוכנה"),
        ] {
            if let Some(v) = info
                .get(key)
                .ok()
                .and_then(|o| o.as_str().ok())
                .map(pdf_string)
            {
                let v = v.trim().to_owned();
                if !v.is_empty() && v.chars().count() <= 200 {
                    metadata.push((label.to_owned(), v));
                }
            }
        }
    }
    let mut warnings = Vec::new();
    let annotated = doc.get_pages().values().any(|id| {
        doc.get_dictionary(*id)
            .ok()
            .and_then(|p| p.get(b"Annots").ok())
            .is_some()
    });
    if annotated {
        warnings.push("הערות או סימונים שנוספו ל-PDF לא יובאו.".to_owned());
    }
    if doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .is_some()
    {
        warnings.push(
            "שדות טופס שמולאו ב-PDF לא יובאו. אם צריך אותם, מעתיקים את התשובות ידנית.".to_owned(),
        );
    }
    (metadata, warnings)
}

/// A PDF text string: UTF-16 with a byte-order mark, or single bytes (PDFDocEncoding, close
/// enough to Latin-1 for names).
fn pdf_string(bytes: &[u8]) -> String {
    match bytes {
        [0xFE, 0xFF, rest @ ..] => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        [0xFF, 0xFE, rest @ ..] => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PDF built in memory: each character is placed separately, left to right,
    /// the way a producer that stores display order would place it.
    fn tiny_pdf(pages: &[Vec<(f64, &str)>]) -> Vec<u8> {
        let mut chars: Vec<char> = pages
            .iter()
            .flatten()
            .flat_map(|(_, t)| t.chars())
            .collect();
        chars.sort_unstable();
        chars.dedup();
        let cid = |c: char| chars.iter().position(|x| *x == c).map_or(0, |i| i + 1);
        let mut objs: Vec<String> = Vec::new();
        let n_pages = pages.len();
        // 1 catalog, 2 pages, 3 font, 4 cid font, 5 descriptor, 6 tounicode, then page+content pairs.
        objs.push("<< /Type /Catalog /Pages 2 0 R >>".to_owned());
        let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 7 + i * 2)).collect();
        objs.push(format!(
            "<< /Type /Pages /Kids [{}] /Count {n_pages} >>",
            kids.join(" ")
        ));
        objs.push("<< /Type /Font /Subtype /Type0 /BaseFont /T /Encoding /Identity-H /DescendantFonts [4 0 R] /ToUnicode 6 0 R >>".to_owned());
        objs.push("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /T /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 500 /FontDescriptor 5 0 R >>".to_owned());
        objs.push("<< /Type /FontDescriptor /FontName /T /Flags 4 /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>".to_owned());
        let mut cmap = String::from("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName /T def 1 begincodespacerange <0000> <FFFF> endcodespacerange\n");
        cmap.push_str(&format!("{} beginbfchar\n", chars.len()));
        for c in &chars {
            cmap.push_str(&format!("<{:04X}> <{:04X}>\n", cid(*c), u32::from(*c)));
        }
        cmap.push_str("endbfchar endcmap CMapName currentdict /CMap defineresource pop end end");
        objs.push(format!(
            "<< /Length {} >>\nstream\n{cmap}\nendstream",
            cmap.len() + 1
        ));
        for (i, lines) in pages.iter().enumerate() {
            let mut content = String::new();
            for (y, text) in lines {
                for (k, c) in text.chars().enumerate() {
                    if c != ' ' {
                        let x = 60.0 + 6.0 * f64::from(u32::try_from(k).unwrap_or(0));
                        content.push_str(&format!(
                            "BT /F1 12 Tf 1 0 0 1 {x} {y} Tm <{:04X}> Tj ET\n",
                            cid(c)
                        ));
                    }
                }
            }
            objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 8 + i * 2));
            objs.push(format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ));
        }
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .bytes(),
        );
        out
    }

    #[test]
    fn display_order_pdf_reads_in_logical_order_without_margins() {
        // Visual strings, as they sit on the page left to right.
        let header = (800.0, "יודב זכרמ");
        let page1 = vec![
            header,
            (700.0, "הנפוה .5:4 ליג"),
            (680.0, "85\tהנבה"),
            (40.0, "1 דומע"),
        ];
        let page2 = vec![header, (700.0, "ישגר תוסיווב ישוק לשב"), (40.0, "2 דומע")];
        let bytes = tiny_pdf(&[page1, page2]);
        let out = crate::extract(&bytes, "r.pdf").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.format, Format::Pdf);
        assert!(out.body.contains("גיל 5:4. הופנה"), "{}", out.body);
        assert!(out.body.contains("בשל קושי בוויסות רגשי"), "{}", out.body);
        assert!(!out.body.contains("מרכז"), "header stays out: {}", out.body);
        assert!(
            !out.body.contains("עמוד"),
            "page numbers stay out: {}",
            out.body
        );
        assert!(out.margins.contains("מרכז בדוי"), "{}", out.margins);
        assert_eq!(out.pages, 2);
    }

    #[test]
    fn file_properties_are_shown_and_comments_and_form_fields_are_pointed_out() {
        use pdf_extract::{Dictionary, Object, StringFormat};
        let bytes = tiny_pdf(&[vec![
            (720.0, "תוליעפ ןיב םירבעמב השק"),
            (706.0, "הנטק הצובקב רתוי ףתתשמ"),
            (692.0, "תוליעפ ןיב םירבעמב השק"),
        ]]);
        let mut doc = Document::load_mem(&bytes).unwrap_or_else(|e| panic!("{e}"));
        let mut author = vec![0xFE, 0xFF];
        for u in "רותם בדויה".encode_utf16() {
            author.extend(u.to_be_bytes());
        }
        let mut info = Dictionary::new();
        info.set("Author", Object::String(author, StringFormat::Hexadecimal));
        info.set("Title", Object::string_literal("Report"));
        let info_id = doc.add_object(info);
        doc.trailer.set("Info", Object::Reference(info_id));
        let page = *doc
            .get_pages()
            .values()
            .next()
            .unwrap_or_else(|| panic!("no page"));
        if let Ok(p) = doc.get_dictionary_mut(page) {
            p.set("Annots", Object::Array(vec![]));
        }
        if let Ok(c) = doc.catalog_mut() {
            c.set("AcroForm", Object::Dictionary(Dictionary::new()));
        }
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap_or_else(|e| panic!("{e}"));
        let got = crate::extract(&out, "p.pdf").unwrap_or_else(|e| panic!("{e}"));
        assert!(
            got.metadata
                .contains(&("יוצר".to_owned(), "רותם בדויה".to_owned())),
            "{:?}",
            got.metadata
        );
        assert!(
            got.warnings.iter().any(|w| w.contains("הערות")),
            "{:?}",
            got.warnings
        );
        assert!(
            got.warnings.iter().any(|w| w.contains("שדות טופס")),
            "{:?}",
            got.warnings
        );
        assert!(!got.body.contains("רותם"));
    }

    #[test]
    fn a_one_page_letter_keeps_its_letterhead_and_footer_out() {
        let page = vec![
            (800.0, "יודב ןוכמ תנייפל ,הנד"),
            (720.0, "תוליעפ ןיב םירבעמב השק"),
            (706.0, "הנטק הצובקב רתוי ףתתשמ"),
            (50.0, "000000018 .ז.ת"),
        ];
        let out = crate::extract(&tiny_pdf(&[page]), "l.pdf").unwrap_or_else(|e| panic!("{e}"));
        assert!(out.body.contains("קשה במעברים בין פעילות"), "{}", out.body);
        for gone in ["מכון", "000000018"] {
            assert!(!out.body.contains(gone), "{gone} in {}", out.body);
            assert!(out.margins.contains(gone), "{gone} not in {}", out.margins);
        }
    }

    #[test]
    fn a_one_page_text_that_runs_into_the_margin_zone_stays_in_the_body() {
        // At the top, evenly spaced text runs into the margin zone: it is the body. At the
        // bottom, two lines set far apart from the text are a footer.
        let lines: Vec<(f64, &str)> = [812.0, 798.0, 784.0, 770.0, 756.0, 44.0, 30.0]
            .iter()
            .map(|y| (*y, "ףוגה ןמ הרוש איה וז"))
            .chain([(400.0, "עצמאב הרוש")])
            .collect();
        let out = crate::extract(&tiny_pdf(&[lines]), "f.pdf").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            out.body.matches("זו היא שורה מן הגוף").count(),
            5,
            "{}",
            out.body
        );
        assert!(
            out.margins.contains("זו היא שורה מן הגוף"),
            "{}",
            out.margins
        );
    }

    #[test]
    fn image_only_pdf_is_scanned() {
        let bytes = tiny_pdf(&[vec![(700.0, "א")]]);
        assert_eq!(crate::extract(&bytes, "s.pdf"), Err(IngestError::Scanned));
    }
}
