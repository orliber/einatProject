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
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| corrupt("short"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, IngestError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
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
    let units = characters(&word, clx)?;

    // Split by part, in storage order.
    let mut at = 0usize;
    let mut take = |n: usize| {
        let start = at.min(units.len());
        at = at.saturating_add(n);
        &units[start..at.min(units.len())]
    };
    let text = take(parts.text);
    let footnotes = take(parts.footnotes);
    let headers = take(parts.headers);
    let _macros = take(parts.macros);
    let _comments = take(parts.comments);
    let endnotes = take(parts.endnotes);
    let text_boxes = take(parts.text_boxes);
    let header_boxes = take(parts.header_text_boxes);

    let mut body = render(text);
    for extra in [footnotes, endnotes, text_boxes] {
        let t = render(extra);
        if !t.trim().is_empty() {
            body.push('\n');
            body.push_str(&t);
        }
    }
    let mut margins = render(headers);
    margins.push('\n');
    margins.push_str(&render(header_boxes));

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

/// All characters of the document, in character-position order, as UTF-16 units.
fn characters(word: &[u8], clx: &[u8]) -> Result<Vec<u16>, IngestError> {
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
                    .map(|p| u16::from_le_bytes([p[0], p[1]])),
            );
        }
    }
    Ok(out)
}

/// Word's special characters → text. Field codes are dropped, field results kept.
fn render(units: &[u16]) -> String {
    let mut out = String::new();
    // For each open field: whether its instruction part is still running.
    let mut fields: Vec<bool> = Vec::new();
    let mut last_cell = false;
    for c in char::decode_utf16(units.iter().copied()) {
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
            // A cell ends with 0x07; two in a row end the table row.
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
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A minimal Word 97 file holding `text` (UTF-16) as one piece, with `header` after it.
    pub(crate) fn build(text: &str, header: &str, flags: u16) -> Vec<u8> {
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

    #[test]
    fn encrypted_doc_is_refused() {
        let bytes = build("טקסט", "", 0x0100);
        assert_eq!(crate::extract(&bytes, "x.doc"), Err(IngestError::Encrypted));
    }
}
