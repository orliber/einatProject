//! Word 97–2003 (`.doc`): the text, read from the document's piece table.
//!
//! A `.doc` is a Compound File holding a `WordDocument` stream (the File Information Block and
//! the characters) and a table stream (`0Table`/`1Table`) with the piece table that says where
//! each run of characters lives. Only the text is read: the main body, footnotes, endnotes and
//! text boxes become the body; headers and footers go to `margins`; comments are not imported.
//! Field codes (the hidden instruction of a link or a page number) are skipped and their
//! results kept. An encrypted or Word 95 (and older) file is refused with a clear message.

use std::io::{Cursor, Read};

use crate::{codepage, Extracted, Format, IngestError};

/// The streams of a `.doc` are small; a larger one is a hostile file.
const MAX_STREAM_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIECES: usize = 200_000;

fn corrupt(why: &str) -> IngestError {
    IngestError::Corrupt(why.to_owned())
}

/// What a Compound File holds, decided without reading its text.
pub(crate) enum Kind {
    Word,
    /// A password-protected .docx (its package is stored inside a Compound File).
    EncryptedPackage,
    Other,
}

pub(crate) fn kind(bytes: &[u8]) -> Result<Kind, IngestError> {
    let cfb = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|e| corrupt(&e.to_string()))?;
    Ok(if cfb.is_stream("/WordDocument") {
        Kind::Word
    } else if cfb.is_stream("/EncryptedPackage") {
        Kind::EncryptedPackage
    } else {
        Kind::Other
    })
}

fn read_stream(
    cfb: &mut cfb::CompoundFile<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, IngestError> {
    let mut s = cfb.open_stream(name).map_err(|e| corrupt(&e.to_string()))?;
    let mut buf = Vec::new();
    (&mut s)
        .take(MAX_STREAM_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| corrupt(&e.to_string()))?;
    if u64::try_from(buf.len()).unwrap_or(u64::MAX) > MAX_STREAM_BYTES {
        return Err(IngestError::TooLarge);
    }
    Ok(buf)
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, IngestError> {
    b.get(at..)
        .and_then(<[u8]>::first_chunk::<2>)
        .map(|s| u16::from_le_bytes(*s))
        .ok_or_else(|| corrupt("short"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, IngestError> {
    b.get(at..)
        .and_then(<[u8]>::first_chunk::<4>)
        .map(|s| u32::from_le_bytes(*s))
        .ok_or_else(|| corrupt("short"))
}

fn usize_at(b: &[u8], at: usize) -> Result<usize, IngestError> {
    usize::try_from(u32_at(b, at)?).map_err(|_| corrupt("size"))
}

/// The counts of characters in each part of the document, in the order they are stored.
struct Parts {
    text: usize,
    footnotes: usize,
    headers: usize,
    macros: usize,
    comments: usize,
    endnotes: usize,
    text_boxes: usize,
    header_text_boxes: usize,
}

pub(crate) fn extract(bytes: &[u8]) -> Result<Extracted, IngestError> {
    let mut cfb =
        cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|e| corrupt(&e.to_string()))?;
    let word = read_stream(&mut cfb, "/WordDocument")?;

    // File Information Block.
    if u16_at(&word, 0)? != 0xA5EC {
        return Err(corrupt("not a Word document"));
    }
    let n_fib = u16_at(&word, 2)?;
    if n_fib < 0x00C1 {
        // Word 95 and older keep text differently.
        return Err(IngestError::LegacyDoc);
    }
    let flags = u16_at(&word, 0x0A)?;
    if flags & 0x0100 != 0 {
        return Err(IngestError::Encrypted);
    }
    let table_name = if flags & 0x0200 != 0 {
        "/1Table"
    } else {
        "/0Table"
    };
    let csw = usize::from(u16_at(&word, 32)?);
    let lw = 32 + 2 + csw * 2;
    let cslw = usize::from(u16_at(&word, lw)?);
    let rg_lw = lw + 2;
    if cslw < 11 {
        return Err(corrupt("FIB"));
    }
    let parts = Parts {
        text: usize_at(&word, rg_lw + 3 * 4)?,
        footnotes: usize_at(&word, rg_lw + 4 * 4)?,
        headers: usize_at(&word, rg_lw + 5 * 4)?,
        macros: usize_at(&word, rg_lw + 6 * 4)?,
        comments: usize_at(&word, rg_lw + 7 * 4)?,
        endnotes: usize_at(&word, rg_lw + 8 * 4)?,
        text_boxes: usize_at(&word, rg_lw + 9 * 4)?,
        header_text_boxes: usize_at(&word, rg_lw + 10 * 4)?,
    };
    let fc_lcb = rg_lw + cslw * 4 + 2;
    // fcClx / lcbClx: the 34th pair in FibRgFcLcb97.
    let fc_clx = usize_at(&word, fc_lcb + 33 * 8)?;
    let lcb_clx = usize_at(&word, fc_lcb + 33 * 8 + 4)?;

    let table = read_stream(&mut cfb, table_name)?;
    let clx = table
        .get(fc_clx..fc_clx.saturating_add(lcb_clx))
        .ok_or_else(|| corrupt("piece table"))?;
    // fcPlcfBtePapx / lcbPlcfBtePapx: the 14th pair. Where the paragraph properties are.
    let ttp = row_mark_ends(&word, &table, fc_lcb + 13 * 8);
    let (units, rows) = characters(&word, clx, &ttp)?;
    // Believed only when every mark they point at is a cell mark; otherwise (and for a file
    // without them) the rows are guessed, as before.
    let rows =
        (!rows.is_empty() && rows.iter().all(|cp| units.get(*cp) == Some(&7))).then_some(rows);

    // Split by part, in storage order. A part longer than what is left takes the rest.
    let mut rest: &[u16] = &units;
    let mut at = 0usize;
    let mut take = |n: usize| {
        let (part, after) = rest.split_at_checked(n).unwrap_or((rest, &[]));
        rest = after;
        let part = Part {
            units: part,
            start: at,
            rows: rows.as_deref(),
        };
        at += part.units.len();
        part
    };
    let text = take(parts.text);
    let footnotes = take(parts.footnotes);
    let headers = take(parts.headers);
    let _macros = take(parts.macros);
    let _comments = take(parts.comments);
    let endnotes = take(parts.endnotes);
    let text_boxes = take(parts.text_boxes);
    let header_boxes = take(parts.header_text_boxes);

    let mut body = render(&text);
    for extra in [footnotes, endnotes, text_boxes] {
        let t = render(&extra);
        if !t.trim().is_empty() {
            body.push('\n');
            body.push_str(&t);
        }
    }
    let mut margins = render(&headers);
    margins.push('\n');
    margins.push_str(&render(&header_boxes));

    let mut warnings = vec!["נקרא מקובץ Word ישן (‎.doc): כדאי לעבור על הטקסט.".to_owned()];
    if parts.comments > 0 {
        warnings.push("הערות שוליים של עורכי המסמך (comments) לא יובאו.".to_owned());
    }
    if !margins.trim().is_empty() {
        warnings.push("כותרת עליונה/תחתונה לא יובאה.".to_owned());
    }
    Ok(Extracted {
        format: Format::Doc,
        body,
        margins,
        metadata: Vec::new(),
        warnings,
        pages: 1,
        ocr: false,
    })
}

/// The size of a page of paragraph properties (PapxFkp) in the `WordDocument` stream.
const FKP: usize = 512;

/// sprmPFTtp: the paragraph is the mark that ends a table row.
const SPRM_P_F_TTP: u16 = 0x2417;

/// Where each end-of-row paragraph of a table ends, as offsets in the `WordDocument` stream,
/// sorted. A cell mark and a row mark are the same character (0x07), so an empty cell looks
/// exactly like the end of a row; only the paragraph properties (sprmPFTtp) tell them apart.
///
/// Read leniently: properties that are missing or do not parse give no row ends, and the
/// rows are then guessed from the text. They never make the document unreadable.
fn row_mark_ends(word: &[u8], table: &[u8], pair_at: usize) -> Vec<u32> {
    let mut ends = Vec::new();
    let (Ok(fc), Ok(lcb)) = (usize_at(word, pair_at), usize_at(word, pair_at + 4)) else {
        return ends;
    };
    // PlcBtePapx: (n + 1) offsets, then n page numbers.
    let Some(plc) = table.get(fc..fc.saturating_add(lcb)) else {
        return ends;
    };
    if plc.len() < 12 || (plc.len() - 4) % 8 != 0 {
        return ends;
    }
    let n = (plc.len() - 4) / 8;
    // Each page is listed once: more entries than pages is a hostile file.
    if n > word.len() / FKP + 1 {
        return ends;
    }
    for k in 0..n {
        let Ok(pn) = usize_at(plc, (n + 1) * 4 + k * 4) else {
            return Vec::new();
        };
        let start = (pn & 0x003F_FFFF).saturating_mul(FKP);
        let Some(page) = word.get(start..start.saturating_add(FKP)) else {
            return Vec::new();
        };
        let paras = usize::from(page.last().copied().unwrap_or(0));
        for i in 0..paras {
            // BxPap: where this paragraph's properties are in the page (in words), then 12
            // bytes of layout.
            let Some(&offset) = page.get((paras + 1) * 4 + i * 13) else {
                return Vec::new();
            };
            if offset == 0 {
                continue;
            }
            let at = usize::from(offset) * 2;
            // PapxInFkp: a size (in words, in one of two forms), a style index, the sprms.
            let size = match (page.get(at), page.get(at + 1)) {
                (Some(0), Some(&cb)) => Some((at + 2, usize::from(cb) * 2)),
                (Some(&cb), _) => Some((at + 1, usize::from(cb) * 2 - 1)),
                _ => None,
            };
            let grpprl = size.and_then(|(from, len)| page.get(from..from + len));
            let Some(grpprl) = grpprl.and_then(|g| g.get(2..)) else {
                return Vec::new();
            };
            if ends_table_row(grpprl) {
                if let Ok(end) = u32_at(page, (i + 1) * 4) {
                    ends.push(end);
                }
            }
        }
    }
    ends.sort_unstable();
    ends.dedup();
    ends
}

/// Whether a paragraph's list of properties (sprms) says it is an end-of-row mark. The list
/// is walked sprm by sprm; one that does not fit ends the walk.
fn ends_table_row(grpprl: &[u8]) -> bool {
    let mut rest = grpprl;
    let mut ttp = false;
    while let Ok(sprm) = u16_at(rest, 0) {
        let operand = rest.get(2..).unwrap_or_default();
        // The top three bits give the operand's size; 6 is "the size is in the operand".
        let len = match sprm >> 13 {
            0 | 1 => 1,
            2 | 4 | 5 => 2,
            3 => 4,
            7 => 3,
            _ => match (sprm, operand) {
                // sprmTDefTable: a two-byte size that counts one more than what follows it.
                (0xD608, _) => match u16_at(operand, 0) {
                    Ok(cb) => 1 + usize::from(cb),
                    Err(_) => break,
                },
                // sprmPChgTabs, long form: tabs to delete (4 bytes each), tabs to add (3).
                (0xC615, [255, del, ..]) => {
                    let del = usize::from(*del);
                    match operand.get(2 + del * 4) {
                        Some(add) => 3 + del * 4 + usize::from(*add) * 3,
                        None => break,
                    }
                }
                (_, [cb, ..]) => 1 + usize::from(*cb),
                _ => break,
            },
        };
        let Some(value) = operand.get(..len) else {
            break;
        };
        if sprm == SPRM_P_F_TTP {
            ttp = value.first().is_some_and(|v| *v != 0);
        }
        rest = operand.get(len..).unwrap_or_default();
    }
    ttp
}

/// All characters of the document, in character-position order, as UTF-16 units; and the
/// positions of the characters that end a table row (those that end at one of `ttp`, the
/// sorted stream offsets from [`row_mark_ends`]).
fn characters(word: &[u8], clx: &[u8], ttp: &[u32]) -> Result<(Vec<u16>, Vec<usize>), IngestError> {
    // Skip Prc entries (formatting), find the Pcdt (the piece table).
    let mut i = 0usize;
    let plc = loop {
        match clx.get(i) {
            Some(1) => {
                let cb = usize::from(u16_at(clx, i + 1)?);
                i = i + 3 + cb;
            }
            Some(2) => {
                let lcb = usize_at(clx, i + 1)?;
                break clx
                    .get(i + 5..i.saturating_add(5).saturating_add(lcb))
                    .ok_or_else(|| corrupt("piece table"))?;
            }
            _ => return Err(corrupt("piece table")),
        }
    };
    // PlcPcd: (n + 1) character positions, then n 8-byte piece descriptors.
    if plc.len() < 4 || (plc.len() - 4) % 12 != 0 {
        return Err(corrupt("piece table"));
    }
    let n = (plc.len() - 4) / 12;
    if n > MAX_PIECES {
        return Err(corrupt("piece table"));
    }
    let mut out: Vec<u16> = Vec::new();
    let mut rows: Vec<usize> = Vec::new();
    let budget = word.len();
    for k in 0..n {
        let cp_start = usize_at(plc, k * 4)?;
        let cp_end = usize_at(plc, (k + 1) * 4)?;
        let count = cp_end
            .checked_sub(cp_start)
            .ok_or_else(|| corrupt("piece order"))?;
        if out.len() + count > budget {
            return Err(corrupt("piece size"));
        }
        let pcd = (n + 1) * 4 + k * 8;
        let fc = u32_at(plc, pcd + 2)?;
        let compressed = fc & 0x4000_0000 != 0;
        let fc = usize::try_from(fc & 0x3FFF_FFFF).map_err(|_| corrupt("size"))?;
        // The row marks in this piece: a character that ends at one of the offsets.
        let (start, unit) = if compressed { (fc / 2, 1) } else { (fc, 2) };
        let from = ttp.partition_point(|end| usize::try_from(*end).is_ok_and(|e| e <= start));
        for end in ttp.iter().skip(from) {
            let Some(offset) = usize::try_from(*end)
                .ok()
                .and_then(|e| e.checked_sub(start))
            else {
                break;
            };
            if offset > count.saturating_mul(unit) {
                break;
            }
            if offset % unit == 0 {
                rows.push(out.len() + offset / unit - 1);
            }
        }
        if compressed {
            let start = fc / 2;
            let bytes = word
                .get(start..start.saturating_add(count))
                .ok_or_else(|| corrupt("piece range"))?;
            out.extend(bytes.iter().map(|b| {
                codepage::cp1252(*b)
                    .map_or(0xFFFD, |c| u16::try_from(u32::from(c)).unwrap_or(0xFFFD))
            }));
        } else {
            let bytes = word
                .get(fc..fc.saturating_add(count.saturating_mul(2)))
                .ok_or_else(|| corrupt("piece range"))?;
            out.extend(
                bytes
                    .chunks_exact(2)
                    .filter_map(|p| <[u8; 2]>::try_from(p).ok())
                    .map(u16::from_le_bytes),
            );
        }
    }
    rows.sort_unstable();
    Ok((out, rows))
}

/// One part of the document (body, footnotes, headers…): its characters, where it starts in
/// the document, and the positions of the end-of-row marks when the file's paragraph
/// properties gave them.
struct Part<'a> {
    units: &'a [u16],
    start: usize,
    rows: Option<&'a [usize]>,
}

/// Word's special characters → text. Field codes are dropped, field results kept.
fn render(part: &Part<'_>) -> String {
    let mut out = String::new();
    // For each open field: whether its instruction part is still running.
    let mut fields: Vec<bool> = Vec::new();
    let mut last_cell = false;
    let mut at = part.start;
    for c in char::decode_utf16(part.units.iter().copied()) {
        let cp = at;
        at += c.as_ref().map_or(1, |c| c.len_utf16());
        let c = c.unwrap_or('\u{FFFD}');
        match c {
            '\u{13}' => {
                fields.push(true);
                continue;
            }
            '\u{14}' => {
                if let Some(f) = fields.last_mut() {
                    *f = false;
                }
                continue;
            }
            '\u{15}' => {
                fields.pop();
                continue;
            }
            _ => {}
        }
        if fields.iter().any(|in_code| *in_code) {
            continue;
        }
        let cell = c == '\u{07}';
        match c {
            '\r' | '\u{0B}' | '\u{0C}' | '\u{0E}' => out.push('\n'),
            // A cell ends with 0x07, and so does the row, after its last cell. The file's
            // paragraph properties say which one is the row's; an empty cell is not.
            '\u{07}' if part.rows.is_some_and(|r| r.binary_search(&cp).is_ok()) => {
                out.truncate(out.trim_end_matches('\t').len());
                out.push('\n');
            }
            '\u{07}' if part.rows.is_some() => out.push('\t'),
            // Without them: two in a row end the table row (a guess; an empty cell breaks it).
            '\u{07}' if last_cell => {
                if out.ends_with('\t') {
                    out.pop();
                }
                out.push('\n');
            }
            '\u{07}' => out.push('\t'),
            '\u{1E}' => out.push('-'),
            '\t' => out.push('\t'),
            // Pictures, drawn objects, footnote marks, optional hyphens: no text.
            c if c.is_control() || c == '\u{1F}' => {}
            c => out.push(c),
        }
        last_cell = cell && !last_cell;
    }
    out
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A minimal Word 97 file holding `text` (UTF-16) as one piece, with `header` after it.
    pub(crate) fn build(text: &str, header: &str, flags: u16) -> Vec<u8> {
        build_rows(text, header, flags, &[])
    }

    /// The same, with paragraph properties that mark the characters at `row_ends` (positions
    /// in `text`) as the end-of-row marks of a table, the way Word stores them.
    fn build_rows(text: &str, header: &str, flags: u16, row_ends: &[usize]) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().chain(header.encode_utf16()).collect();
        let text_len = text.encode_utf16().count();
        let header_len = header.encode_utf16().count();

        // FIB: base (32 bytes), csw = 14, cslw = 22, cbRgFcLcb = 93.
        let mut fib = vec![0u8; 32];
        fib[0..2].copy_from_slice(&0xA5ECu16.to_le_bytes());
        fib[2..4].copy_from_slice(&0x00C1u16.to_le_bytes());
        fib[0x0A..0x0C].copy_from_slice(&(flags | 0x0200).to_le_bytes());
        fib.extend_from_slice(&14u16.to_le_bytes());
        fib.extend_from_slice(&[0u8; 28]);
        fib.extend_from_slice(&22u16.to_le_bytes());
        let mut lw = [0u32; 22];
        lw[3] = u32::try_from(text_len).unwrap_or(0);
        lw[5] = u32::try_from(header_len).unwrap_or(0);
        for v in lw {
            fib.extend_from_slice(&v.to_le_bytes());
        }
        fib.extend_from_slice(&93u16.to_le_bytes());
        let fc_lcb_at = fib.len();
        fib.extend_from_slice(&vec![0u8; 93 * 8]);
        let fc_text = fib.len();
        let mut word = fib;
        for u in &units {
            word.extend_from_slice(&u.to_le_bytes());
        }
        // One page of paragraph properties (PapxFkp): for each row mark, a plain paragraph
        // up to it and a one-character paragraph with sprmPFInTable and sprmPFTtp.
        let mut bte = Vec::new();
        if !row_ends.is_empty() {
            word.resize(word.len().next_multiple_of(512), 0);
            let page = u32::try_from(word.len() / 512).unwrap_or(0);
            let fc = |cp: usize| u32::try_from(fc_text + cp * 2).unwrap_or(0);
            let mut fkp = vec![0u8; 512];
            let mut bounds = vec![fc(0)];
            for e in row_ends {
                bounds.extend([fc(*e), fc(e + 1)]);
            }
            let paras = bounds.len() - 1;
            for (i, b) in bounds.iter().enumerate() {
                fkp[i * 4..i * 4 + 4].copy_from_slice(&b.to_le_bytes());
            }
            let papx = [0u8, 4, 0, 0, 0x16, 0x24, 1, 0x17, 0x24, 1];
            fkp[500..510].copy_from_slice(&papx);
            for i in (1..paras).step_by(2) {
                fkp[(paras + 1) * 4 + i * 13] = 250;
            }
            fkp[511] = u8::try_from(paras).unwrap_or(0);
            word.extend_from_slice(&fkp);
            bte.extend_from_slice(&fc(0).to_le_bytes());
            bte.extend_from_slice(&fc(units.len()).to_le_bytes());
            bte.extend_from_slice(&page.to_le_bytes());
        }

        // Table stream: a Clx with one piece covering all characters.
        let total = u32::try_from(units.len()).unwrap_or(0);
        let mut plc = Vec::new();
        plc.extend_from_slice(&0u32.to_le_bytes());
        plc.extend_from_slice(&total.to_le_bytes());
        plc.extend_from_slice(&0u16.to_le_bytes());
        plc.extend_from_slice(&u32::try_from(fc_text).unwrap_or(0).to_le_bytes());
        plc.extend_from_slice(&0u16.to_le_bytes());
        let mut clx = vec![2u8];
        clx.extend_from_slice(&u32::try_from(plc.len()).unwrap_or(0).to_le_bytes());
        clx.extend_from_slice(&plc);
        let at = fc_lcb_at + 33 * 8;
        word[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
        word[at + 4..at + 8].copy_from_slice(&u32::try_from(clx.len()).unwrap_or(0).to_le_bytes());
        // PlcfBtePapx, after the Clx in the table stream.
        let at = fc_lcb_at + 13 * 8;
        word[at..at + 4].copy_from_slice(&u32::try_from(clx.len()).unwrap_or(0).to_le_bytes());
        word[at + 4..at + 8].copy_from_slice(&u32::try_from(bte.len()).unwrap_or(0).to_le_bytes());
        clx.extend_from_slice(&bte);

        let mut cfb =
            cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap_or_else(|e| panic!("{e}"));
        for (name, data) in [("/WordDocument", &word), ("/1Table", &clx)] {
            let mut s = cfb.create_stream(name).unwrap_or_else(|e| panic!("{e}"));
            s.write_all(data).unwrap_or_else(|e| panic!("{e}"));
        }
        cfb.flush().unwrap_or_else(|e| panic!("{e}"));
        cfb.into_inner().into_inner()
    }

    #[test]
    fn body_fields_tables_and_headers() {
        let text = "נועם משחק לבד בחצר.\r\u{13} HYPERLINK \"http://example.invalid\" \u{14}קישור\u{15} בגן\rא\u{07}ב\u{07}\u{07}סוף\r";
        let bytes = build(text, "מרפאה בדויה\r", 0);
        let out = crate::extract(&bytes, "old.doc").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.format, Format::Doc);
        assert_eq!(out.body, "נועם משחק לבד בחצר.\nקישור בגן\nא\tב\nסוף");
        assert_eq!(out.margins, "מרפאה בדויה");
        assert!(!out.body.contains("HYPERLINK"));
    }

    /// An empty cell is two cell marks in a row, exactly like the end of a row: only the
    /// paragraph properties tell them apart.
    #[test]
    fn an_empty_table_cell_does_not_end_the_row() {
        // קוביות | (empty) | 9, then אוצר | טוב | (empty), then a line after the table.
        let text = "מבחנים\rקוביות\u{07}\u{07}9\u{07}\u{07}אוצר\u{07}טוב\u{07}\u{07}\u{07}סוף\r";
        let marks: Vec<usize> = text
            .encode_utf16()
            .enumerate()
            .filter(|(_, u)| *u == 7)
            .map(|(i, _)| i)
            .collect();
        let row_ends = [marks[3], marks[7]];
        let bytes = build_rows(text, "", 0, &row_ends);
        let out = crate::extract(&bytes, "old.doc").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.body, "מבחנים\nקוביות\t\t9\nאוצר\tטוב\nסוף");
        // Properties that point at something that is not a cell mark are not believed.
        let bytes = build_rows(text, "", 0, &[marks[3], 2]);
        let out = crate::extract(&bytes, "old.doc").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.body, "מבחנים\nקוביות\n9\nאוצר\tטוב\n\tסוף");
    }

    #[test]
    fn encrypted_doc_is_refused() {
        let bytes = build("טקסט", "", 0x0100);
        assert_eq!(crate::extract(&bytes, "x.doc"), Err(IngestError::Encrypted));
    }
}
