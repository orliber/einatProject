//! A locked PDF of the report (EX-3, D-049), made here, without LibreOffice or Word.
//!
//! The text is laid out right to left with the font she chose (read from the computer's own
//! fonts by the caller), embedded whole. The file is encrypted with the PDF 2.0 standard
//! handler (AES-256, revision 6, written here: the library's own handler wrote keys that
//! readers could not use): it opens with the password she gives the parents, and
//! allows printing only; editing needs an owner password that is random and never kept. The
//! caller records the file's SHA-256 in the audit log, so a copy that reaches her later can
//! be checked against what she sent.
//!
//! Bidirectional text: a paragraph is right to left; runs of Latin letters and digits
//! ("WISC-V", "FSIQ = 102", "5:4") keep their own order inside it. Hebrew needs no shaping
//! (final letters are their own characters); marks such as niqqud are drawn unpositioned.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use aws_lc_rs::cipher::{EncryptingKey, EncryptionContext, UnboundCipherKey, AES_128, AES_256};
use aws_lc_rs::digest::{digest, SHA256, SHA384, SHA512};
use aws_lc_rs::iv::FixedLength;
use lopdf::{dictionary, Document, Object, Stream, StringFormat};

use crate::{ExportError, Report, ScoreChart, MIN_PASSWORD_CHARS};

const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN: f32 = 56.7;
const TOP: f32 = PAGE_H - MARGIN - 6.0;
const BOTTOM: f32 = MARGIN + 6.0;
const LEFT: f32 = MARGIN;
const RIGHT: f32 = PAGE_W - MARGIN;

type Rgb = (f32, f32, f32);
const INK: Rgb = (0.0, 0.0, 0.0);
const NAVY: Rgb = (0.122, 0.227, 0.373);
const GRAY: Rgb = (0.36, 0.40, 0.40);
const LIGHT_GRAY: Rgb = (0.48, 0.48, 0.48);
const HEADER_FILL: Rgb = (0.902, 0.937, 0.929);
const BAND_FILL: Rgb = (0.933, 0.953, 0.949);
const LINE: Rgb = (0.722, 0.769, 0.757);

/// The fonts the report is set in: the bytes of TrueType files. Without a bold file,
/// headings are drawn with a thin outline instead.
#[derive(Debug, Clone, Copy)]
pub struct PdfFonts<'a> {
    pub regular: &'a [u8],
    pub bold: Option<&'a [u8]>,
}

struct Font<'a> {
    face: ttf_parser::Face<'a>,
    data: &'a [u8],
    upem: f32,
    /// Glyphs drawn, with the character each stands for (widths and copy-and-paste).
    used: RefCell<BTreeMap<u16, char>>,
}

impl<'a> Font<'a> {
    fn load(data: &'a [u8]) -> Result<Self, ExportError> {
        let face = ttf_parser::Face::parse(data, 0)
            .map_err(|_| ExportError::Pdf("קובץ הגופן לא נקרא.".to_owned()))?;
        if matches!(
            face.permissions(),
            Some(ttf_parser::Permissions::Restricted)
        ) {
            return Err(ExportError::Pdf(
                "הגופן שנבחר לא מרשה להטמיע אותו בקובץ. בוחרים גופן אחר בהגדרות הדוח.".to_owned(),
            ));
        }
        if face.glyph_index('א').is_none() {
            return Err(ExportError::Pdf(
                "בגופן שנבחר אין אותיות עבריות.".to_owned(),
            ));
        }
        Ok(Self {
            upem: f32::from(face.units_per_em().max(1)),
            face,
            data,
            used: RefCell::new(BTreeMap::new()),
        })
    }

    fn glyph(&self, c: char) -> u16 {
        let id = self
            .face
            .glyph_index(c)
            .or_else(|| self.face.glyph_index('?'))
            .map_or(0, |g| g.0);
        self.used.borrow_mut().entry(id).or_insert(c);
        id
    }

    fn advance(&self, c: char) -> f32 {
        let g = self
            .face
            .glyph_index(c)
            .or_else(|| self.face.glyph_index('?'));
        g.and_then(|g| self.face.glyph_hor_advance(g))
            .map_or(0.0, |a| f32::from(a) * 1000.0 / self.upem)
    }

    fn width(&self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.advance(c)).sum::<f32>() * size / 1000.0
    }

    /// The glyph ids of a visual-order string, as a hex string for `Tj`.
    fn hex(&self, visual: &str) -> String {
        let mut out = String::with_capacity(visual.len() * 4 + 2);
        out.push('<');
        for c in visual.chars() {
            let _ = write!(out, "{:04X}", self.glyph(c));
        }
        out.push('>');
        out
    }
}

fn is_rtl(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{05FF}' | '\u{FB1D}'..='\u{FB4F}' | '\u{0600}'..='\u{06FF}')
}

fn is_ltr(c: char) -> bool {
    c.is_ascii_alphanumeric() || (c.is_alphabetic() && !is_rtl(c)) || c.is_numeric()
}

fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        c => c,
    }
}

/// One line of a right-to-left paragraph in the order it is drawn, left to right.
fn visual(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    // Spans that keep their own (left-to-right) order: from a Latin letter or digit to the
    // last one before the next Hebrew letter.
    let mut ltr = vec![false; chars.len()];
    let mut i = 0;
    while i < chars.len() {
        if is_ltr(chars[i]) {
            let mut end = i;
            let mut j = i;
            while j < chars.len() && !is_rtl(chars[j]) {
                if is_ltr(chars[j]) {
                    end = j;
                }
                j += 1;
            }
            for flag in &mut ltr[i..=end] {
                *flag = true;
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut k = chars.len();
    while k > 0 {
        k -= 1;
        if ltr[k] {
            let mut start = k;
            while start > 0 && ltr[start - 1] {
                start -= 1;
            }
            out.extend_from_slice(&chars[start..=k]);
            k = start;
        } else {
            out.push(mirror(chars[k]));
        }
    }
    out.into_iter().collect()
}

/// Logical lines that fit in `max` points.
fn wrap(font: &Font<'_>, text: &str, size: f32, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_owned()
        } else {
            format!("{cur} {word}")
        };
        if !cur.is_empty() && font.width(&candidate, size) > max {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_owned();
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[derive(Clone, Copy)]
enum Align {
    Right,
    Center,
}

struct Pages<'a> {
    regular: &'a Font<'a>,
    bold: Option<&'a Font<'a>>,
    done: Vec<String>,
    cur: String,
    y: f32,
}

impl<'a> Pages<'a> {
    fn new(regular: &'a Font<'a>, bold: Option<&'a Font<'a>>) -> Self {
        Self {
            regular,
            bold,
            done: Vec::new(),
            cur: String::new(),
            y: TOP,
        }
    }

    fn new_page(&mut self) {
        self.done.push(std::mem::take(&mut self.cur));
        self.y = TOP;
    }

    /// Room for `h` points, or a new page.
    fn ensure(&mut self, h: f32) {
        if self.y - h < BOTTOM && self.y < TOP {
            self.new_page();
        }
    }

    fn font(&self, bold: bool) -> &'a Font<'a> {
        if bold {
            self.bold.unwrap_or(self.regular)
        } else {
            self.regular
        }
    }

    /// One line of text whose right edge (or center) is at `x`, baseline at `y`.
    #[allow(clippy::too_many_arguments)]
    fn text_at(
        &mut self,
        logical: &str,
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        color: Rgb,
        align: Align,
    ) {
        let font = self.font(bold);
        let vis = visual(logical);
        let w = font.width(&vis, size);
        let left = match align {
            Align::Right => x - w,
            Align::Center => x - w / 2.0,
        };
        let name = if bold && self.bold.is_some() {
            "F2"
        } else {
            "F1"
        };
        // Without a bold file: fill and a thin stroke.
        let fake = if bold && self.bold.is_none() {
            format!(
                "2 Tr {:.2} w {:.3} {:.3} {:.3} RG ",
                size / 40.0,
                color.0,
                color.1,
                color.2
            )
        } else {
            "0 Tr ".to_owned()
        };
        let hex = font.hex(&vis);
        let _ = writeln!(
            self.cur,
            "BT {fake}{:.3} {:.3} {:.3} rg /{name} {size:.2} Tf {left:.2} {y:.2} Td {hex} Tj ET",
            color.0, color.1, color.2
        );
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgb) {
        let _ = writeln!(
            self.cur,
            "{:.3} {:.3} {:.3} rg {x:.2} {y:.2} {w:.2} {h:.2} re f",
            fill.0, fill.1, fill.2
        );
    }

    fn frame(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let _ = writeln!(
            self.cur,
            "{:.3} {:.3} {:.3} RG 0.5 w {x:.2} {y:.2} {w:.2} {h:.2} re S",
            LINE.0, LINE.1, LINE.2
        );
    }

    /// A wrapped paragraph across the text width.
    #[allow(clippy::too_many_arguments)]
    fn paragraph(
        &mut self,
        text: &str,
        size: f32,
        bold: bool,
        color: Rgb,
        align: Align,
        before: f32,
        after: f32,
        keep: f32,
    ) {
        let font = self.font(bold);
        let lines = wrap(font, text, size, RIGHT - LEFT);
        let lead = size * 1.45;
        self.y -= before;
        self.ensure(lead.min(lead * lines.len() as f32) + keep);
        for l in lines {
            self.ensure(lead);
            self.y -= lead;
            let x = match align {
                Align::Right => RIGHT,
                Align::Center => (LEFT + RIGHT) / 2.0,
            };
            self.text_at(&l, x, self.y + size * 0.3, size, bold, color, align);
        }
        self.y -= after;
    }

    /// A table, first column on the right, with a shaded header row.
    fn table(&mut self, columns: &[String], rows: &[Vec<String>]) {
        let size = 10.5;
        let lead = size * 1.35;
        let total = RIGHT - LEFT;
        let n = columns.len().max(1);
        let first = if n > 1 { total * 0.42 } else { total };
        let rest = if n > 1 {
            (total - first) / (n - 1) as f32
        } else {
            0.0
        };
        let width = |i: usize| if i == 0 { first } else { rest };
        let mut all = vec![(columns.to_vec(), true)];
        all.extend(rows.iter().map(|r| (r.clone(), false)));
        for (cells, header) in all {
            let font = self.font(header);
            let wrapped: Vec<Vec<String>> = cells
                .iter()
                .enumerate()
                .map(|(i, c)| wrap(font, c, size, width(i) - 8.0))
                .collect();
            let h = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1) as f32 * lead + 6.0;
            self.ensure(h);
            let top = self.y;
            let mut right = RIGHT;
            for (i, lines) in wrapped.iter().enumerate() {
                let w = width(i);
                if header {
                    self.rect(right - w, top - h, w, h, HEADER_FILL);
                }
                self.frame(right - w, top - h, w, h);
                let mut y = top - 3.0;
                for l in lines {
                    y -= lead;
                    self.text_at(
                        l,
                        right - 4.0,
                        y + size * 0.3,
                        size,
                        header,
                        INK,
                        Align::Right,
                    );
                }
                right -= w;
            }
            self.y = top - h;
        }
    }

    /// Bars on a fixed axis, low values on the right as in the Word file.
    fn chart(&mut self, c: &ScoreChart) {
        let step = if c.step > 0.0 { c.step } else { 1.0 };
        let cells = (((c.max - c.min).max(step) / step).round() as usize).clamp(1, 60);
        let total = RIGHT - LEFT;
        let label_w = total * 0.30;
        let score_w = total * 0.08;
        let area_right = RIGHT - label_w - score_w;
        let cw = (area_right - LEFT) / cells as f32;
        let start = |i: usize| c.min + step * i as f64;
        let cell_x = |i: usize| area_right - (i + 1) as f32 * cw;
        let row_h = 16.0;
        // Header: the axis in groups of one standard deviation.
        self.ensure(row_h * 2.0);
        let top = self.y;
        self.rect(LEFT, top - row_h, total, row_h, HEADER_FILL);
        self.frame(LEFT, top - row_h, total, row_h);
        self.text_at(
            "מדד",
            RIGHT - 4.0,
            top - row_h + 5.0,
            9.0,
            true,
            INK,
            Align::Right,
        );
        self.text_at(
            "ציון",
            RIGHT - label_w - score_w / 2.0,
            top - row_h + 5.0,
            9.0,
            true,
            INK,
            Align::Center,
        );
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
                crate::docx::num(start(i))
            } else {
                format!("{}–{}", crate::docx::num(start(i)), crate::docx::num(last))
            };
            let mid = (cell_x(i + n - 1) + cell_x(i) + cw) / 2.0;
            self.text_at(
                &label,
                mid,
                top - row_h + 5.0,
                7.0,
                false,
                INK,
                Align::Center,
            );
            i += n;
        }
        self.y = top - row_h;
        for bar in &c.bars {
            self.ensure(row_h);
            let top = self.y;
            for i in 0..cells {
                if start(i) + step > c.mean - c.sd && start(i) < c.mean + c.sd {
                    self.rect(cell_x(i), top - row_h, cw, row_h, BAND_FILL);
                }
            }
            let v = bar.value.clamp(c.min, c.max - step / 2.0);
            let filled = (0..cells).filter(|&i| start(i) <= v).count();
            if filled > 0 {
                self.rect(
                    cell_x(filled - 1),
                    top - row_h + 3.0,
                    cw * filled as f32,
                    row_h - 6.0,
                    NAVY,
                );
            }
            self.frame(LEFT, top - row_h, total, row_h);
            self.text_at(
                &bar.label,
                RIGHT - 4.0,
                top - row_h + 5.0,
                9.0,
                false,
                INK,
                Align::Right,
            );
            self.text_at(
                &crate::docx::num(bar.value),
                RIGHT - label_w - score_w / 2.0,
                top - row_h + 5.0,
                9.0,
                true,
                INK,
                Align::Center,
            );
            self.y = top - row_h;
        }
    }

    fn finish(mut self) -> Vec<String> {
        if !self.cur.is_empty() || self.done.is_empty() {
            self.done.push(self.cur);
        }
        self.done
    }
}

fn lay_out(report: &Report, p: &mut Pages<'_>) {
    p.paragraph(
        &report.title,
        18.0,
        true,
        INK,
        Align::Center,
        0.0,
        14.0,
        0.0,
    );
    for line in &report.info {
        p.paragraph(
            &format!("{}: {}", line.label, line.value),
            12.0,
            false,
            INK,
            Align::Right,
            0.0,
            2.0,
            0.0,
        );
    }
    for part in &report.parts {
        if !part.title.is_empty() {
            p.paragraph(&part.title, 15.0, true, NAVY, Align::Right, 16.0, 4.0, 40.0);
        }
        for s in &part.sections {
            if !s.title.is_empty() {
                p.paragraph(&s.title, 13.0, true, INK, Align::Right, 10.0, 2.0, 30.0);
            }
            for text in &s.paragraphs {
                for piece in text.split('\n').filter(|t| !t.trim().is_empty()) {
                    p.paragraph(piece.trim(), 12.0, false, INK, Align::Right, 0.0, 6.0, 0.0);
                }
            }
        }
    }
    if !report.signature.is_empty() {
        p.y -= 18.0;
        p.ensure(report.signature.len() as f32 * 17.0);
        for line in &report.signature {
            p.paragraph(line, 12.0, false, INK, Align::Right, 0.0, 0.0, 0.0);
        }
    }
    if !report.tables.is_empty() {
        p.new_page();
        p.paragraph(
            "נספח: טבלאות ציונים",
            15.0,
            true,
            NAVY,
            Align::Right,
            0.0,
            4.0,
            40.0,
        );
        for t in &report.tables {
            p.paragraph(&t.title, 13.0, true, INK, Align::Right, 10.0, 4.0, 40.0);
            p.table(&t.columns, &t.rows);
            if !t.note.is_empty() {
                p.paragraph(&t.note, 9.0, false, GRAY, Align::Right, 3.0, 6.0, 0.0);
            }
            for c in t.charts.iter().filter(|c| !c.bars.is_empty()) {
                p.paragraph(&c.title, 13.0, true, INK, Align::Right, 10.0, 4.0, 50.0);
                p.chart(c);
                if !c.note.is_empty() {
                    p.paragraph(&c.note, 9.0, false, GRAY, Align::Right, 3.0, 6.0, 0.0);
                }
            }
        }
    }
}

fn to_unicode(font: &Font<'_>) -> Vec<u8> {
    let used = font.used.borrow();
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = used.iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (gid, c) in chunk {
            let mut buf = [0u16; 2];
            let units: String = c
                .encode_utf16(&mut buf)
                .iter()
                .map(|u| format!("{u:04X}"))
                .collect();
            let _ = writeln!(s, "<{gid:04X}> <{units}>");
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

fn embed(doc: &mut Document, font: &Font<'_>, name: &str) -> lopdf::ObjectId {
    let file = doc.add_object(Stream::new(
        dictionary! { "Length1" => i64::try_from(font.data.len()).unwrap_or(i64::MAX) },
        font.data.to_vec(),
    ));
    let scale = |v: i16| (f32::from(v) * 1000.0 / font.upem).round() as i64;
    let bbox = font.face.global_bounding_box();
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(name.as_bytes().to_vec()),
        "Flags" => 32,
        "FontBBox" => vec![scale(bbox.x_min).into(), scale(bbox.y_min).into(), scale(bbox.x_max).into(), scale(bbox.y_max).into()],
        "ItalicAngle" => 0,
        "Ascent" => scale(font.face.ascender()),
        "Descent" => scale(font.face.descender()),
        "CapHeight" => scale(font.face.capital_height().unwrap_or(font.face.ascender())),
        "StemV" => 80,
        "FontFile2" => file,
    });
    let mut widths: Vec<Object> = Vec::new();
    for gid in font.used.borrow().keys() {
        let w = font
            .face
            .glyph_hor_advance(ttf_parser::GlyphId(*gid))
            .map_or(0, |a| (f32::from(a) * 1000.0 / font.upem).round() as i64);
        widths.push(i64::from(*gid).into());
        widths.push(vec![w.into()].into());
    }
    let cid = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => Object::Name(name.as_bytes().to_vec()),
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::String(b"Adobe".to_vec(), StringFormat::Literal),
            "Ordering" => Object::String(b"Identity".to_vec(), StringFormat::Literal),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor,
        "DW" => 1000,
        "W" => widths,
        "CIDToGIDMap" => "Identity",
    });
    let cmap = doc.add_object(Stream::new(dictionary! {}, to_unicode(font)));
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => Object::Name(name.as_bytes().to_vec()),
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid.into()],
        "ToUnicode" => cmap,
    })
}

fn random<const N: usize>() -> Result<[u8; N], ExportError> {
    let mut b = [0u8; N];
    aws_lc_rs::rand::fill(&mut b).map_err(|_| ExportError::Crypto)?;
    Ok(b)
}

/// The report as a PDF that opens with `password` and can be printed but not changed.
pub fn render_pdf(
    report: &Report,
    fonts: PdfFonts<'_>,
    password: &str,
) -> Result<Vec<u8>, ExportError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ExportError::WeakPassword);
    }
    let mut doc = build(report, fonts)?;
    lock(&mut doc, password)?;
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| ExportError::Package(e.to_string()))?;
    Ok(out)
}

/// The document, not yet encrypted.
fn build(report: &Report, fonts: PdfFonts<'_>) -> Result<Document, ExportError> {
    let regular = Font::load(fonts.regular)?;
    let bold = fonts.bold.map(Font::load).transpose()?;
    let mut pages = Pages::new(&regular, bold.as_ref());
    lay_out(report, &mut pages);
    let bodies = pages.finish();
    let count = bodies.len();

    // The confidentiality line and "page n of N" on every page.
    let mut contents = Vec::with_capacity(count);
    for (i, body) in bodies.into_iter().enumerate() {
        let mut margin = Pages::new(&regular, bold.as_ref());
        margin.text_at(
            &report.confidentiality,
            PAGE_W / 2.0,
            PAGE_H - MARGIN / 2.0 - 4.0,
            8.5,
            false,
            LIGHT_GRAY,
            Align::Center,
        );
        margin.text_at(
            &format!("עמוד {} מתוך {count}", i + 1),
            PAGE_W / 2.0,
            MARGIN / 2.0,
            8.5,
            false,
            LIGHT_GRAY,
            Align::Center,
        );
        contents.push(format!("{}{body}", margin.cur));
    }

    let mut doc = Document::with_version("2.0");
    let pages_id = doc.new_object_id();
    let f1 = embed(&mut doc, &regular, "DVReport-Regular");
    let mut fonts_dict = dictionary! { "F1" => f1 };
    if let Some(b) = &bold {
        fonts_dict.set("F2", embed(&mut doc, b, "DVReport-Bold"));
    }
    let resources = doc.add_object(dictionary! { "Font" => fonts_dict });
    let mut kids = Vec::with_capacity(count);
    for c in contents {
        let content = doc.add_object(Stream::new(dictionary! {}, c.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), PAGE_W.into(), PAGE_H.into()],
            "Contents" => content,
            "Resources" => resources,
        });
        kids.push(page.into());
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => i64::try_from(count).unwrap_or(i64::MAX),
        }),
    );
    // No author, producer or dates: only the language, for screen readers.
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Lang" => Object::String(b"he-IL".to_vec(), StringFormat::Literal),
        "ViewerPreferences" => dictionary! { "Direction" => "R2L" },
    });
    doc.trailer.set("Root", catalog);
    let id = random::<16>()?;
    doc.trailer.set(
        "ID",
        vec![
            Object::String(id.to_vec(), StringFormat::Hexadecimal),
            Object::String(id.to_vec(), StringFormat::Hexadecimal),
        ],
    );
    doc.compress();
    Ok(doc)
}

/// AES-CBC without padding (the data is a whole number of blocks).
fn cbc(key: &[u8], iv: [u8; 16], data: &[u8]) -> Result<Vec<u8>, ExportError> {
    let alg = if key.len() == 16 { &AES_128 } else { &AES_256 };
    let unbound = UnboundCipherKey::new(alg, key).map_err(|_| ExportError::Crypto)?;
    let k = EncryptingKey::cbc(unbound).map_err(|_| ExportError::Crypto)?;
    let mut buf = data.to_vec();
    k.less_safe_encrypt(&mut buf, EncryptionContext::Iv128(FixedLength::from(iv)))
        .map_err(|_| ExportError::Crypto)?;
    Ok(buf)
}

/// ISO 32000-2 algorithm 2.B: the hash of a password with a salt (and, for the owner, U).
fn hash_2b(password: &[u8], salt: &[u8], udata: &[u8]) -> Result<[u8; 32], ExportError> {
    let mut k: Vec<u8> = digest(&SHA256, &[password, salt, udata].concat())
        .as_ref()
        .to_vec();
    let mut round = 0usize;
    loop {
        let unit = [password, &k, udata].concat();
        let k1 = unit.repeat(64);
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&k[16..32]);
        let e = cbc(&k[..16], iv, &k1)?;
        let m = e[..16].iter().map(|b| u32::from(*b)).sum::<u32>() % 3;
        let alg = match m {
            0 => &SHA256,
            1 => &SHA384,
            _ => &SHA512,
        };
        k = digest(alg, &e).as_ref().to_vec();
        round += 1;
        let last = usize::from(*e.last().unwrap_or(&0));
        if round >= 64 && last + 32 <= round {
            break;
        }
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&k[..32]);
    Ok(out)
}

/// Passwords the PDF standard prepares unchanged (SASLprep is the identity on them):
/// letters, digits, hyphens and spaces; Hebrew ones start and end with a Hebrew letter and
/// have no Latin letters (the bidirectional rule).
fn plain_password(password: &str) -> bool {
    let ok_chars = password.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '-' || c == ' ' || ('\u{05D0}'..='\u{05EA}').contains(&c)
    });
    let hebrew = password.chars().any(is_rtl);
    let bidi = !hebrew
        || (password.chars().next().is_some_and(is_rtl)
            && password.chars().last().is_some_and(is_rtl)
            && !password.chars().any(|c| c.is_ascii_alphabetic()));
    ok_chars && bidi && password.len() <= 127
}

fn encrypt_strings(obj: &mut Object, key: &[u8; 32]) -> Result<(), ExportError> {
    match obj {
        Object::String(bytes, format) => {
            *bytes = seal(key, bytes)?;
            *format = StringFormat::Hexadecimal;
        }
        Object::Array(items) => {
            for i in items {
                encrypt_strings(i, key)?;
            }
        }
        Object::Dictionary(d) => {
            for (_, v) in d.iter_mut() {
                encrypt_strings(v, key)?;
            }
        }
        Object::Stream(st) => {
            for (_, v) in st.dict.iter_mut() {
                encrypt_strings(v, key)?;
            }
            let sealed = seal(key, &st.content)?;
            st.set_content(sealed);
        }
        _ => {}
    }
    Ok(())
}

/// AESV3: a random IV, then the data with PKCS#7 padding, under the file key.
fn seal(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, ExportError> {
    let iv = random::<16>()?;
    let pad = 16 - data.len() % 16;
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat_n(u8::try_from(pad).unwrap_or(16), pad));
    let mut out = iv.to_vec();
    out.extend(cbc(key, iv, &padded)?);
    Ok(out)
}

/// The standard security handler, revision 6 (AES-256): opens with `password`, allows
/// printing only. The owner password is random and forgotten at once.
fn lock(doc: &mut Document, password: &str) -> Result<(), ExportError> {
    if !plain_password(password) {
        return Err(ExportError::Pdf("סיסמה לקובץ PDF: אותיות, ספרות ומקפים בלבד, בלי לערבב עברית ואנגלית. אפשר ללחוץ \"סיסמה חדשה\".".to_owned()));
    }
    let key = random::<32>()?;
    let user = password.as_bytes();
    let owner_pw = random::<32>()?;
    let owner: Vec<u8> = owner_pw.iter().map(|b| b % 26 + b'a').collect();

    let (uv, uk) = (random::<8>()?, random::<8>()?);
    let mut u = hash_2b(user, &uv, &[])?.to_vec();
    u.extend(uv);
    u.extend(uk);
    let ue = cbc(&hash_2b(user, &uk, &[])?, [0; 16], &key)?;
    let (ov, ok) = (random::<8>()?, random::<8>()?);
    let mut o = hash_2b(&owner, &ov, &u)?.to_vec();
    o.extend(ov);
    o.extend(ok);
    let oe = cbc(&hash_2b(&owner, &ok, &u)?, [0; 16], &key)?;
    // Print (bit 3) and high-quality print (12), accessibility extraction (10); the reserved
    // high bits set.
    let p: i32 = -4096 | (1 << 2) | (1 << 9) | (1 << 11);
    let mut perms = [0u8; 16];
    perms[..4].copy_from_slice(&p.to_le_bytes());
    perms[4..8].copy_from_slice(&[0xFF; 4]);
    perms[8] = b'T';
    perms[9..12].copy_from_slice(b"adb");
    perms[12..].copy_from_slice(&random::<4>()?);
    let perms = cbc(&key, [0; 16], &perms)?;

    for obj in doc.objects.values_mut() {
        encrypt_strings(obj, &key)?;
    }
    let hex = |b: Vec<u8>| Object::String(b, StringFormat::Hexadecimal);
    let encrypt = doc.add_object(dictionary! {
        "Filter" => "Standard",
        "V" => 5,
        "R" => 6,
        "Length" => 256,
        "CF" => dictionary! { "StdCF" => dictionary! { "Type" => "CryptFilter", "CFM" => "AESV3", "AuthEvent" => "DocOpen", "Length" => 32 } },
        "StmF" => "StdCF",
        "StrF" => "StdCF",
        "O" => hex(o),
        "U" => hex(u),
        "OE" => hex(oe),
        "UE" => hex(ue),
        "P" => i64::from(p),
        "Perms" => hex(perms),
        "EncryptMetadata" => true,
    });
    doc.trailer.set("Encrypt", encrypt);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::sample;

    /// A font with Hebrew letters from this computer, or `None` (the test is then skipped).
    pub(crate) fn system_font() -> Option<Vec<u8>> {
        [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
        ]
        .iter()
        .find_map(|p| std::fs::read(p).ok())
    }

    #[test]
    fn right_to_left_with_latin_runs_kept() {
        assert_eq!(visual("שלום"), "םולש");
        assert_eq!(visual("ציון 102 (ממוצע)"), "(עצוממ) 102 ןויצ");
        assert_eq!(visual("WISC-V מלא"), "אלמ WISC-V");
        assert_eq!(visual("הבנה מילולית (VCI)"), "(VCI) תילולימ הנבה");
        assert_eq!(visual("FSIQ = 102 בטווח"), "חווטב FSIQ = 102");
    }

    #[test]
    fn locked_pdf_opens_only_with_the_password() {
        let Some(font) = system_font() else {
            eprintln!("no Hebrew font on this computer; skipped");
            return;
        };
        let mut r = sample();
        r.tables[0].charts.push(ScoreChart {
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
            note: String::new(),
        });
        let fonts = PdfFonts {
            regular: &font,
            bold: None,
        };
        assert!(matches!(
            render_pdf(&r, fonts, "קצר"),
            Err(ExportError::WeakPassword)
        ));
        assert!(matches!(
            render_pdf(&r, fonts, "סיסמה-לקובץ-1"),
            Err(ExportError::Pdf(_))
        ));
        let pdf = render_pdf(&r, fonts, "נהר-ענן-42-שקד-אורן").unwrap();
        assert!(pdf.starts_with(b"%PDF-2.0"));
        // Encrypted: no readable text, no name, no metadata.
        let raw = String::from_utf8_lossy(&pdf);
        assert!(raw.contains("/Encrypt") && !raw.contains("Producer") && !raw.contains("Author"));
        if let Ok(dir) = std::env::var("DV_PDF_SAMPLE") {
            std::fs::write(std::path::Path::new(&dir).join("sample.pdf"), &pdf).unwrap();
        }
        let mut doc = Document::load_mem(&pdf).unwrap();
        assert!(doc.is_encrypted());
        assert!(doc.decrypt("סיסמה-שגויה-99").is_err());
        let mut doc = Document::load_mem(&pdf).unwrap();
        doc.decrypt("נהר-ענן-42-שקד-אורן").unwrap();
        // lopdf checks the password against U independently of the code above; that the
        // file opens in a reader (pdfinfo, pdftoppm with -upw) was checked by hand.
        // Each export is its own file: the fingerprint identifies this one.
        assert_ne!(render_pdf(&r, fonts, "נהר-ענן-42-שקד-אורן").unwrap(), pdf);
    }
}
