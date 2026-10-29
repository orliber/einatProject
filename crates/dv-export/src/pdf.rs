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

use crate::{
    BandChart, ExportError, Logo, LogoFormat, Notice, Report, ScoreChart, MIN_PASSWORD_CHARS,
};

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
const MARK_FILL: Rgb = (0.788, 0.851, 0.918);

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
    /// Codes drawn (a glyph id, or an alias of one), with the character each stands for
    /// (widths and copy-and-paste).
    used: RefCell<BTreeMap<u16, char>>,
    /// A bracket drawn mirrored in right-to-left text: its own code past the font's glyphs,
    /// drawn with the mirror's glyph and copied as the bracket that was written. With the
    /// mirror's own code, "(עד 42 חודשים)" was copied out of the file as ")עד 42 חודשים(".
    aliases: RefCell<BTreeMap<char, (u16, u16)>>,
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
            aliases: RefCell::new(BTreeMap::new()),
        })
    }

    /// The code of `drawn` standing for `meant`: the glyph id itself, or for a mirrored
    /// bracket an alias of it that copies as `meant` (see `aliases`).
    fn code(&self, drawn: char, meant: char) -> u16 {
        let gid = self.glyph(drawn);
        if drawn == meant {
            return gid;
        }
        let mut aliases = self.aliases.borrow_mut();
        if let Some((code, _)) = aliases.get(&meant) {
            return *code;
        }
        let glyphs = self.face.number_of_glyphs();
        let Some(code) = u16::try_from(aliases.len())
            .ok()
            .and_then(|n| glyphs.checked_add(n))
        else {
            return gid;
        };
        aliases.insert(meant, (code, gid));
        self.used.borrow_mut().insert(code, meant);
        code
    }

    /// The glyph a code draws.
    fn gid_of(&self, code: u16) -> u16 {
        self.aliases
            .borrow()
            .values()
            .find(|(c, _)| *c == code)
            .map_or(code, |(_, g)| *g)
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

    /// The codes of a visual-order line (each character drawn, with the one it stands for),
    /// as a hex string for `Tj`.
    fn hex(&self, visual: &[(char, char)]) -> String {
        let mut out = String::with_capacity(visual.len() * 4 + 2);
        out.push('<');
        for (drawn, meant) in visual {
            let _ = write!(out, "{:04X}", self.code(*drawn, *meant));
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

/// A bracket ends a left-to-right span, so "(FSIQ) 102" keeps each parenthesis on its own
/// side (the Unicode algorithm gives such neutrals the paragraph's direction).
fn is_bracket(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '«' | '»')
}

/// A sign or unit that stays with the digit beside it ("79%", "-3", "₪40").
fn is_terminator(c: char) -> bool {
    matches!(c, '%' | '°' | '₪' | '$' | '+' | '−' | '-')
}

/// A Hebrew point or accent: drawn on the letter before it, so it stays after that letter.
fn is_mark(c: char) -> bool {
    matches!(c, '\u{0591}'..='\u{05C7}')
        && !matches!(c, '\u{05BE}' | '\u{05C0}' | '\u{05C3}' | '\u{05C6}')
}

/// One line of a right-to-left paragraph in the order it is drawn, left to right.
#[cfg(test)]
fn visual(line: &str) -> String {
    visual_pairs(line).into_iter().map(|(c, _)| c).collect()
}

/// [`visual`], each character drawn with the one it stands for (a mirrored bracket).
fn visual_pairs(line: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = line.chars().collect();
    // Spans that keep their own (left-to-right) order: from a Latin letter or digit to the
    // last one before the next Hebrew letter or bracket, with a sign before the first digit
    // and a unit after the last one.
    let mut ltr = vec![false; chars.len()];
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if is_ltr(c) {
            let len = chars
                .iter()
                .skip(i)
                .take_while(|c| !is_rtl(**c) && !is_bracket(**c))
                .enumerate()
                .filter(|(_, c)| is_ltr(**c))
                .last()
                .map_or(1, |(j, _)| j + 1);
            let mut end = i + len;
            let at = |k: Option<usize>| k.and_then(|k| chars.get(k)).copied();
            // "-3" at the start, after a space or after a bracket: the sign is the number's
            // (after a letter, "ו-5", the hyphen is the word's).
            let signed = c.is_ascii_digit()
                && at(i.checked_sub(1)).is_some_and(is_terminator)
                && at(i.checked_sub(2)).is_none_or(|b| b.is_whitespace() || is_bracket(b));
            let start = if signed { i.saturating_sub(1) } else { i };
            // "79%": the unit after the last digit.
            if at(end.checked_sub(1)).is_some_and(|d| d.is_ascii_digit())
                && at(Some(end)).is_some_and(|u| is_terminator(u) && u != '-')
            {
                end += 1;
            }
            for flag in ltr.iter_mut().take(end).skip(start) {
                *flag = true;
            }
            i = end;
        } else {
            i += 1;
        }
    }
    // Drawn from the end: a left-to-right span keeps its order, everything else is reversed
    // one letter at a time (its points stay after it) and mirrored.
    let marked: Vec<(char, bool)> = chars.iter().copied().zip(ltr).collect();
    let mut out: Vec<(char, char)> = Vec::with_capacity(chars.len());
    for run in marked.chunk_by(|a, b| a.1 == b.1).rev() {
        if run.first().is_some_and(|(_, keep)| *keep) {
            out.extend(run.iter().map(|(c, _)| (*c, *c)));
        } else {
            for letter in run.chunk_by(|_, b| is_mark(b.0)).rev() {
                out.extend(letter.iter().map(|(c, _)| (mirror(*c), *c)));
            }
        }
    }
    out
}

/// A word wider than `max` cut into pieces that fit (a letter keeps its points); a word
/// that fits is one piece. Nothing is drawn past the margin.
fn pieces(font: &Font<'_>, word: &str, size: f32, max: f32) -> Vec<String> {
    if font.width(word, size) <= max {
        return vec![word.to_owned()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in word.chars() {
        let next = font.width(&cur, size) + font.advance(c) * size / 1000.0;
        if !cur.is_empty() && !is_mark(c) && next > max {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Logical lines that fit in `max` points.
fn wrap(font: &Font<'_>, text: &str, size: f32, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text
        .split_whitespace()
        .flat_map(|w| pieces(font, w, size, max))
    {
        let candidate = if cur.is_empty() {
            word.clone()
        } else {
            format!("{cur} {word}")
        };
        if !cur.is_empty() && font.width(&candidate, size) > max {
            lines.push(std::mem::take(&mut cur));
            cur = word;
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
        let vis = visual_pairs(logical);
        let w = font.width(&vis.iter().map(|(c, _)| *c).collect::<String>(), size);
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
            let v = crate::docx::bar_end(bar.value, c.min, c.max - step / 2.0);
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

    /// The parents' chart (round 55): one row per measure, one column per group, as in the
    /// Word file. The measure's group is shaded and marked, with its score when shown.
    fn bands(&mut self, c: &BandChart) {
        let size = 10.5;
        let lead = size * 1.35;
        let total = RIGHT - LEFT;
        let n = c.groups.len().max(1);
        let label_w = total * 0.40;
        let group_w = (total - label_w) / n as f32;
        // The right edge of group `i`'s column.
        let col_right = |i: usize| RIGHT - label_w - i as f32 * group_w;
        let header: Vec<String> = c.groups.clone();
        let head_h = lead + 6.0;
        self.ensure(head_h * 2.0);
        let top = self.y;
        self.rect(LEFT, top - head_h, total, head_h, HEADER_FILL);
        self.frame(RIGHT - label_w, top - head_h, label_w, head_h);
        self.text_at(
            "תחום",
            RIGHT - 4.0,
            top - head_h + 6.0,
            size,
            true,
            INK,
            Align::Right,
        );
        for (i, g) in header.iter().enumerate() {
            self.frame(col_right(i) - group_w, top - head_h, group_w, head_h);
            self.text_at(
                g,
                col_right(i) - group_w / 2.0,
                top - head_h + 6.0,
                size,
                true,
                INK,
                Align::Center,
            );
        }
        self.y = top - head_h;
        for r in &c.rows {
            let lines = wrap(self.font(false), &r.label, size, label_w - 8.0);
            let h = lines.len().max(1) as f32 * lead + 6.0;
            self.ensure(h);
            let top = self.y;
            self.frame(RIGHT - label_w, top - h, label_w, h);
            let mut y = top - 3.0;
            for l in &lines {
                y -= lead;
                self.text_at(
                    l,
                    RIGHT - 4.0,
                    y + size * 0.3,
                    size,
                    false,
                    INK,
                    Align::Right,
                );
            }
            for i in 0..n {
                let x = col_right(i) - group_w;
                if i == r.group {
                    self.rect(x, top - h, group_w, h, MARK_FILL);
                    let mid = x + group_w / 2.0;
                    match &r.value {
                        Some(v) => self.text_at(
                            v,
                            mid,
                            top - h / 2.0 - size * 0.35,
                            size,
                            true,
                            INK,
                            Align::Center,
                        ),
                        None => self.rect(mid - 3.5, top - h / 2.0 - 3.5, 7.0, 7.0, NAVY),
                    }
                }
                self.frame(x, top - h, group_w, h);
            }
            self.y = top - h;
        }
    }

    /// Her logo, centered at the top of the page (the image `/Logo`).
    fn logo(&mut self, logo: &Logo) {
        let (w, h) = logo.fit(crate::docx::LOGO_MAX_W, crate::docx::LOGO_MAX_H);
        let x = (PAGE_W - w) / 2.0;
        let y = self.y - h;
        let _ = writeln!(self.cur, "q {w:.2} 0 0 {h:.2} {x:.2} {y:.2} cm /Logo Do Q");
        self.y = y - 12.0;
    }

    fn finish(mut self) -> Vec<String> {
        if !self.cur.is_empty() || self.done.is_empty() {
            self.done.push(self.cur);
        }
        self.done
    }
}

fn lay_out(report: &Report, p: &mut Pages<'_>) {
    if let Some(logo) = &report.logo {
        p.logo(logo);
    }
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
    for text in &report.intro {
        for piece in text.split('\n').filter(|t| !t.trim().is_empty()) {
            p.paragraph(piece.trim(), 12.0, false, INK, Align::Right, 8.0, 4.0, 0.0);
        }
    }
    for c in report.bands.iter().filter(|c| !c.rows.is_empty()) {
        p.paragraph(&c.title, 13.0, true, INK, Align::Right, 10.0, 4.0, 50.0);
        p.bands(c);
        if !c.note.is_empty() {
            p.paragraph(&c.note, 9.0, false, GRAY, Align::Right, 3.0, 6.0, 0.0);
        }
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
    for code in font.used.borrow().keys() {
        let w = font
            .face
            .glyph_hor_advance(ttf_parser::GlyphId(font.gid_of(*code)))
            .map_or(0, |a| (f32::from(a) * 1000.0 / font.upem).round() as i64);
        widths.push(i64::from(*code).into());
        widths.push(vec![w.into()].into());
    }
    // Codes are glyph ids, except a mirrored bracket's alias past the last glyph: then a map
    // from every code to its glyph.
    let to_gid: Object = match font.aliases.borrow().values().map(|(code, _)| *code).max() {
        None => "Identity".into(),
        Some(last) => {
            let map: Vec<u8> = (0..=last)
                .flat_map(|code| font.gid_of(code).to_be_bytes())
                .collect();
            doc.add_object(Stream::new(dictionary! {}, map)).into()
        }
    };
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
        "CIDToGIDMap" => to_gid,
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

/// The report as a PDF that can be printed but not changed. With `password` it opens only
/// with it; without one (her choice, D-081) it opens for anyone and stays print-only.
pub fn render_pdf(
    report: &Report,
    fonts: PdfFonts<'_>,
    password: Option<&str>,
) -> Result<Vec<u8>, ExportError> {
    if password.is_some_and(|p| p.chars().count() < MIN_PASSWORD_CHARS) {
        return Err(ExportError::WeakPassword);
    }
    let mut doc = build(report, fonts)?;
    lock(&mut doc, password.unwrap_or_default())?;
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| ExportError::Package(e.to_string()))?;
    Ok(out)
}

/// A notice that holds nothing about a child (the protection layers page, L6): it opens
/// without a password, and is print-only like the report, so the copy the parents or the
/// institute keep is the one she made. It takes a [`Notice`], never a [`Report`], so a
/// child's report cannot reach this path.
pub fn render_notice_pdf(notice: &Notice, fonts: PdfFonts<'_>) -> Result<Vec<u8>, ExportError> {
    let report = Report {
        title: notice.title.clone(),
        info: Vec::new(),
        parts: vec![crate::ReportPart {
            title: String::new(),
            sections: notice.sections.clone(),
        }],
        tables: Vec::new(),
        signature: Vec::new(),
        confidentiality: notice.footer.clone(),
        font: String::new(),
        ..Report::default()
    };
    let mut doc = build(&report, fonts)?;
    lock(&mut doc, "")?;
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| ExportError::Package(e.to_string()))?;
    Ok(out)
}

/// Her logo as an image: a JPEG as it is (DCT), a PNG as its pixels, with its transparency
/// as a soft mask. Compressed with the rest of the file.
fn logo_image(doc: &mut Document, logo: &Logo) -> Result<lopdf::ObjectId, ExportError> {
    let (w, h) = (i64::from(logo.width), i64::from(logo.height));
    match logo.format {
        LogoFormat::Jpeg => {
            let space = if crate::image::jpeg_gray(logo) {
                "DeviceGray"
            } else {
                "DeviceRGB"
            };
            Ok(doc.add_object(
                Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
                        "ColorSpace" => space, "BitsPerComponent" => 8, "Filter" => "DCTDecode",
                    },
                    logo.bytes.clone(),
                )
                .with_compression(false),
            ))
        }
        LogoFormat::Png => {
            let px = crate::image::png_pixels(logo)?;
            let space = if px.gray { "DeviceGray" } else { "DeviceRGB" };
            let mut dict = dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
                "ColorSpace" => space, "BitsPerComponent" => 8,
            };
            if let Some(alpha) = px.alpha {
                let mask = doc.add_object(Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
                        "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                    },
                    alpha,
                ));
                dict.set("SMask", mask);
            }
            Ok(doc.add_object(Stream::new(dict, px.color)))
        }
    }
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
    let mut res = dictionary! { "Font" => fonts_dict };
    if let Some(logo) = &report.logo {
        let image = logo_image(&mut doc, logo)?;
        res.set("XObject", dictionary! { "Logo" => image });
    }
    let resources = doc.add_object(res);
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
        let (Some(key), Some(iv)) = (k.get(..16), k.get(16..32)) else {
            return Err(ExportError::Crypto);
        };
        let iv: [u8; 16] = iv.try_into().map_err(|_| ExportError::Crypto)?;
        let e = cbc(key, iv, &k1)?;
        let m = e.iter().take(16).map(|b| u32::from(*b)).sum::<u32>() % 3;
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
    k.get(..32)
        .and_then(|head| <[u8; 32]>::try_from(head).ok())
        .ok_or(ExportError::Crypto)
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
    fn a_mirrored_bracket_is_drawn_mirrored_and_copied_as_written() {
        // Copied out of the file, "(עד 42 חודשים)" came out ")עד 42 חודשים(" (0.9.9).
        let Some(data) = system_font() else {
            eprintln!("no Hebrew font on this computer; skipped");
            return;
        };
        let font = Font::load(&data).unwrap();
        let vis = visual_pairs("ציון (עד 42) ו-(VCI)");
        assert_eq!(vis.first(), Some(&('(', ')')), "{vis:?}");
        let hex = font.hex(&vis);
        let codes: Vec<u16> = hex
            .trim_matches(['<', '>'])
            .as_bytes()
            .chunks(4)
            .map(|c| u16::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            .collect();
        let gid = |c: char| font.face.glyph_index(c).unwrap().0;
        let used = font.used.borrow().clone();
        // Drawn as "(", copied as ")": a code of its own, past the font's glyphs.
        let first = codes[0];
        assert!(first >= font.face.number_of_glyphs());
        assert_eq!((font.gid_of(first), used[&first]), (gid('('), ')'));
        // The same bracket again takes the same code; a Latin letter is itself.
        let mirrored: Vec<u16> = codes
            .iter()
            .copied()
            .filter(|c| *c >= font.face.number_of_glyphs())
            .collect();
        // Four brackets in right-to-left text, two codes.
        assert_eq!(mirrored.len(), 4, "{vis:?}");
        let mut distinct = mirrored.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 2);
        assert!(codes.contains(&gid('V')));
        assert_eq!(used[&gid('(')], '(');
        // The file maps every code to its glyph once an alias is drawn.
        let mut doc = Document::with_version("2.0");
        let id = embed(&mut doc, &font, "F");
        let descendant = doc
            .get_dictionary(id)
            .unwrap()
            .get(b"DescendantFonts")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let map = doc
            .get_dictionary(descendant)
            .unwrap()
            .get(b"CIDToGIDMap")
            .unwrap();
        let map = doc
            .get_object(map.as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        let at = |code: u16| {
            let i = usize::from(code) * 2;
            u16::from_be_bytes([map.content[i], map.content[i + 1]])
        };
        assert_eq!(at(first), gid('('));
        assert_eq!(at(gid('V')), gid('V'));
    }

    /// A bracket ends a Latin span, so "(FSIQ) 102" keeps both parentheses; a sign or unit
    /// stays with its number; a hyphen after a Hebrew letter stays with the word.
    #[test]
    fn brackets_signs_and_units_sit_where_a_reader_expects() {
        assert_eq!(visual("ציון מלא (FSIQ) 102"), "102 (FSIQ) אלמ ןויצ");
        assert_eq!(
            visual("הבנה מילולית (VCI) 112, אחוזון 79"),
            "79 ןוזוחא ,112 (VCI) תילולימ הנבה"
        );
        assert_eq!(visual("FSIQ (102)"), "(102) FSIQ");
        assert_eq!(visual("אחוזון 79%"), "79% ןוזוחא");
        assert_eq!(visual("הפרש -3 נקודות"), "תודוקנ -3 שרפה");
        assert_eq!(visual("(-3)"), "(-3)");
        assert_eq!(visual("ו-5 ילדים"), "םידלי 5-ו");
        assert_eq!(visual("טווח 85–99"), "85–99 חווט");
    }

    /// Points and accents are drawn on the letter before them: they stay after it when the
    /// line is reversed, while a maqaf is a letter of its own.
    #[test]
    fn nikud_stays_on_its_letter() {
        assert_eq!(visual("שָׁלוֹם"), "םוֹלשָׁ");
        assert_eq!(visual("בֵּית־סֵפֶר"), "רפֶסֵ־תיבֵּ");
        assert_eq!(visual("בָּ 5"), "5 בָּ");
    }

    /// A word wider than the line is cut into pieces that fit, with its points kept on their
    /// letters, so nothing is drawn past the margin (also inside a table cell).
    #[test]
    fn an_over_long_word_is_cut_to_the_width() {
        let Some(data) = system_font() else {
            eprintln!("no Hebrew font on this computer; skipped");
            return;
        };
        let font = Font::load(&data).unwrap_or_else(|e| panic!("{e}"));
        let long = "א".repeat(200);
        let text = format!("מילה {long}ָ סוף");
        let lines = wrap(&font, &text, 12.0, 100.0);
        assert!(lines.len() > 3, "{lines:?}");
        for l in &lines {
            assert!(font.width(l, 12.0) <= 100.0, "{l}");
        }
        let joined: String = lines.join("").replace(' ', "");
        assert_eq!(joined, text.replace(' ', ""), "nothing lost");
        assert!(
            lines.iter().any(|l| l.contains("אָ"))
                && !lines.iter().any(|l| l.starts_with('\u{05B8}')),
            "the point stays on its letter: {lines:?}"
        );
        // A short word is one line, and so is a word that just fits.
        assert_eq!(wrap(&font, "שלום", 12.0, 100.0), vec!["שלום".to_owned()]);
    }

    #[test]
    fn a_chart_narrower_than_one_step_does_not_panic() {
        let Some(font) = system_font() else {
            eprintln!("no Hebrew font on this computer; skipped");
            return;
        };
        let mut r = sample();
        r.tables[0].charts.push(ScoreChart {
            title: "פרופיל".into(),
            min: 0.0,
            max: 0.0,
            step: 0.0,
            mean: 0.0,
            sd: 0.0,
            bars: vec![crate::ChartBar {
                label: "מדד".into(),
                value: 0.0,
            }],
            note: String::new(),
        });
        let fonts = PdfFonts {
            regular: &font,
            bold: None,
        };
        assert!(build(&r, fonts).is_ok());
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
            render_pdf(&r, fonts, Some("קצר")),
            Err(ExportError::WeakPassword)
        ));
        assert!(matches!(
            render_pdf(&r, fonts, Some("סיסמה-לקובץ-1")),
            Err(ExportError::Pdf(_))
        ));
        let pdf = render_pdf(&r, fonts, Some("נהר-ענן-42-שקד-אורן")).unwrap();
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
        assert_ne!(
            render_pdf(&r, fonts, Some("נהר-ענן-42-שקד-אורן")).unwrap(),
            pdf
        );

        // Without a password (D-081): it opens for anyone, and is still print-only.
        let open = render_pdf(&r, fonts, None).unwrap();
        assert!(String::from_utf8_lossy(&open).contains("/Encrypt"));
        let doc = Document::load_mem(&open).unwrap();
        assert!(!doc.get_pages().is_empty());
    }
    #[test]
    fn a_notice_opens_without_a_password_and_cannot_be_changed() {
        let Some(font) = system_font() else {
            eprintln!("no Hebrew font on this computer; skipped");
            return;
        };
        let n = Notice {
            title: "שכבות ההגנה".into(),
            sections: vec![crate::ReportSection {
                title: "הצפנה".into(),
                paragraphs: vec!["כל התיקים שמורים מוצפנים (AES-256).".into()],
            }],
            footer: "נוצר בכספת האבחון".into(),
        };
        let fonts = PdfFonts {
            regular: &font,
            bold: None,
        };
        let pdf = render_notice_pdf(&n, fonts).unwrap();
        assert!(pdf.starts_with(b"%PDF-2.0"));
        // Print-only, like the report; a reader opens it with the empty password by itself
        // (lopdf does so when it loads it).
        let raw = String::from_utf8_lossy(&pdf);
        assert!(raw.contains("/Encrypt") && !raw.contains("Producer") && !raw.contains("Author"));
        let doc = Document::load_mem(&pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
    }

    #[test]
    fn parents_pdf_carries_the_logo_and_the_chart() {
        let Some(font) = system_font() else {
            eprintln!("no Hebrew font on this computer; skipped");
            return;
        };
        let fonts = PdfFonts {
            regular: &font,
            bold: None,
        };
        for logo in [
            crate::image::tests::png(40, 20, 6, 2),
            crate::image::tests::png(10, 10, 2, 0),
            crate::image::tests::jpeg(30, 10),
        ] {
            let mut r = crate::docx::tests::parents(false);
            r.logo = Some(crate::check_logo(&logo).unwrap());
            let doc = build(&r, fonts).unwrap();
            let page = doc.page_iter().next().unwrap();
            let content = String::from_utf8(doc.get_page_content(page).unwrap()).unwrap();
            assert!(content.contains("/Logo Do"), "the logo on the first page");
            let images = doc
                .objects
                .values()
                .filter_map(|o| o.as_stream().ok())
                .filter(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image"))
                .count();
            assert!(images >= 1);
            let pdf = render_pdf(&r, fonts, None).unwrap();
            if let Ok(dir) = std::env::var("DV_PDF_SAMPLE") {
                let dir = std::path::Path::new(&dir);
                std::fs::write(
                    dir.join(format!(
                        "parents-{}.pdf",
                        r.logo.as_ref().map_or("", |l| l.extension())
                    )),
                    &pdf,
                )
                .unwrap();
                std::fs::write(dir.join("parents.docx"), crate::render(&r).unwrap()).unwrap();
            }
        }
    }
}
