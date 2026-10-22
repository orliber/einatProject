//! Her logo (round 55, D-095): one PNG or JPEG of at most 1 MB, at the top of the first page.
//!
//! The kind is read from the file's first bytes, never from its name. SVG is refused by name:
//! it is a program (scripts, links to other files), not a picture. What is kept is rebuilt
//! from the parts a picture needs, so nothing else in the file reaches the report: a PNG keeps
//! its header, palette, transparency and pixel data; a JPEG loses its EXIF, XMP, ICC and
//! comment segments (camera, place, author). A PNG is also decoded once here, so a file that
//! would not draw is refused when she chooses it, not when she exports.

use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::ExportError;

/// The largest file taken: 1 MB.
pub const MAX_LOGO_BYTES: usize = 1024 * 1024;
/// The largest side, and the most pixels, of a picture taken (a logo is far smaller).
const MAX_SIDE: u32 = 8000;
const MAX_PIXELS: u64 = 16_000_000;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogoFormat {
    Png,
    Jpeg,
}

/// A checked logo: its kind, its size in pixels, and the rebuilt file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logo {
    pub format: LogoFormat,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

impl Logo {
    #[must_use]
    pub fn extension(&self) -> &'static str {
        match self.format {
            LogoFormat::Png => "png",
            LogoFormat::Jpeg => "jpeg",
        }
    }

    #[must_use]
    pub fn content_type(&self) -> &'static str {
        match self.format {
            LogoFormat::Png => "image/png",
            LogoFormat::Jpeg => "image/jpeg",
        }
    }

    /// Width and height on the page, in points: within `max_w` × `max_h`, in proportion.
    #[must_use]
    pub fn fit(&self, max_w: f32, max_h: f32) -> (f32, f32) {
        let (w, h) = (self.width.max(1) as f32, self.height.max(1) as f32);
        let scale = (max_w / w).min(max_h / h);
        (w * scale, h * scale)
    }
}

fn refuse(msg: &str) -> ExportError {
    ExportError::Image(msg.to_owned())
}

fn unreadable() -> ExportError {
    refuse("התמונה לא נקראה. שומרים אותה מחדש כ-PNG או JPEG (למשל בתוכנת הצייר) ובוחרים שוב.")
}

/// Check a logo once, when she chooses it: refused with a sentence she can act on, or rebuilt
/// from its picture alone.
pub fn check_logo(bytes: &[u8]) -> Result<Logo, ExportError> {
    if bytes.is_empty() {
        return Err(refuse("הקובץ ריק."));
    }
    if bytes.len() > MAX_LOGO_BYTES {
        return Err(refuse(
            "התמונה גדולה מ-1MB. שומרים אותה בגודל קטן יותר ובוחרים שוב.",
        ));
    }
    if looks_like_markup(bytes) {
        return Err(refuse(
            "קובץ SVG (או כל קובץ טקסט) לא מתקבל: הוא קוד ולא תמונה. שומרים את הלוגו כ-PNG או JPEG.",
        ));
    }
    if bytes.starts_with(PNG_SIGNATURE) {
        let logo = clean_png(bytes)?;
        png_pixels(&logo)?;
        Ok(logo)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        clean_jpeg(bytes)
    } else {
        Err(refuse("רק תמונת PNG או JPEG. הקובץ הזה אינו אחד מהם."))
    }
}

/// Text that opens with `<` (after a byte-order mark and spaces): SVG, XML, HTML.
fn looks_like_markup(bytes: &[u8]) -> bool {
    let rest = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let rest = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .and_then(|i| rest.get(i..))
        .unwrap_or_default();
    rest.starts_with(b"<")
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// CRC-32 (ISO 3309), as PNG chunks carry it.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for part in parts {
        for &byte in *part {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
    }
    !crc
}

fn size_ok(width: u32, height: u32) -> Result<(), ExportError> {
    if width == 0 || height == 0 {
        return Err(unreadable());
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(refuse(
            "התמונה גדולה מדי (יותר מ-8000 פיקסלים בצד). שומרים אותה קטנה יותר ובוחרים שוב.",
        ));
    }
    Ok(())
}

/// One PNG chunk: its type, its data, and the whole chunk as it is in the file.
type Chunk<'a> = ([u8; 4], &'a [u8], &'a [u8]);

fn png_chunks(bytes: &[u8]) -> Result<Vec<Chunk<'_>>, ExportError> {
    let mut out = Vec::new();
    let mut at = PNG_SIGNATURE.len();
    loop {
        let len = be32(bytes, at).ok_or_else(unreadable)? as usize;
        let kind: [u8; 4] = bytes
            .get(at + 4..at + 8)
            .and_then(|k| k.try_into().ok())
            .ok_or_else(unreadable)?;
        let data = bytes.get(at + 8..at + 8 + len).ok_or_else(unreadable)?;
        let crc = be32(bytes, at + 8 + len).ok_or_else(unreadable)?;
        if crc32(&[&kind, data]) != crc {
            return Err(unreadable());
        }
        let whole = bytes.get(at..at + 12 + len).ok_or_else(unreadable)?;
        out.push((kind, data, whole));
        at += 12 + len;
        if &kind == b"IEND" {
            return Ok(out);
        }
    }
}

/// The PNG with only its header, palette, transparency and pixel data.
fn clean_png(bytes: &[u8]) -> Result<Logo, ExportError> {
    let chunks = png_chunks(bytes)?;
    let (kind, ihdr, _) = chunks.first().ok_or_else(unreadable)?;
    if kind != b"IHDR" || ihdr.len() != 13 {
        return Err(unreadable());
    }
    let width = be32(ihdr, 0).ok_or_else(unreadable)?;
    let height = be32(ihdr, 4).ok_or_else(unreadable)?;
    size_ok(width, height)?;
    let mut out = PNG_SIGNATURE.to_vec();
    for (kind, _, whole) in &chunks {
        match kind {
            b"IHDR" | b"PLTE" | b"tRNS" | b"IDAT" | b"IEND" => out.extend_from_slice(whole),
            // Another chunk a reader must understand: not a picture this code draws.
            k if k.first().is_some_and(u8::is_ascii_uppercase) => return Err(unreadable()),
            // Text, dates, colour profiles and the rest: left out.
            _ => {}
        }
    }
    Ok(Logo {
        format: LogoFormat::Png,
        width,
        height,
        bytes: out,
    })
}

/// A JPEG's markers that carry the picture (the frame, tables, scans); the application
/// segments, comments and anything after the end are left out. APP14 (Adobe) stays: it only
/// says how the colours are coded.
fn clean_jpeg(bytes: &[u8]) -> Result<Logo, ExportError> {
    let mut out = vec![0xFF, 0xD8];
    let mut at = 2;
    let mut size: Option<(u32, u32)> = None;
    let mut scanned = false;
    loop {
        // Fill bytes before a marker are allowed.
        while bytes.get(at) == Some(&0xFF) && bytes.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if bytes.get(at) != Some(&0xFF) {
            return Err(unreadable());
        }
        let marker = *bytes.get(at + 1).ok_or_else(unreadable)?;
        if marker == 0xD9 {
            out.extend_from_slice(&[0xFF, 0xD9]);
            break;
        }
        let len = be16(bytes, at + 2).ok_or_else(unreadable)? as usize;
        if len < 2 {
            return Err(unreadable());
        }
        let segment = bytes.get(at..at + 2 + len).ok_or_else(unreadable)?;
        match marker {
            // Frames: baseline, extended, progressive (Huffman). Lossless and arithmetic
            // coding are rare, and not every reader draws them.
            0xC0..=0xC2 => {
                let h = u32::from(be16(segment, 5).ok_or_else(unreadable)?);
                let w = u32::from(be16(segment, 7).ok_or_else(unreadable)?);
                let components = *segment.get(9).ok_or_else(unreadable)?;
                if segment.get(4) != Some(&8) || !matches!(components, 1 | 3) {
                    return Err(refuse(
                        "תמונת JPEG בצבעי הדפסה (CMYK) או ברזולוציית צבע מיוחדת לא נתמכת. שומרים אותה מחדש כ-JPEG רגיל או כ-PNG.",
                    ));
                }
                size_ok(w, h)?;
                size = Some((w, h));
                out.extend_from_slice(segment);
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err(unreadable()),
            // Tables, restart interval, Adobe colour transform.
            0xC4 | 0xDB | 0xDD | 0xEE => out.extend_from_slice(segment),
            0xDA => {
                if size.is_none() {
                    return Err(unreadable());
                }
                out.extend_from_slice(segment);
                // The entropy-coded data runs to the next marker that is not a stuffed 0xFF00
                // or a restart marker.
                let mut end = at + 2 + len;
                loop {
                    match (bytes.get(end), bytes.get(end + 1)) {
                        (Some(0xFF), Some(&next))
                            if next != 0 && !(0xD0..=0xD7).contains(&next) =>
                        {
                            break
                        }
                        (Some(_), Some(_)) => end += 1,
                        _ => return Err(unreadable()),
                    }
                }
                out.extend_from_slice(bytes.get(at + 2 + len..end).ok_or_else(unreadable)?);
                scanned = true;
                at = end;
                continue;
            }
            // APP0–APP13, APP15 (JFIF, EXIF, XMP, ICC, Photoshop…) and comments: left out.
            0xE0..=0xED | 0xEF | 0xFE => {}
            _ => return Err(unreadable()),
        }
        at += 2 + len;
    }
    let (width, height) = size.filter(|_| scanned).ok_or_else(unreadable)?;
    Ok(Logo {
        format: LogoFormat::Jpeg,
        width,
        height,
        bytes: out,
    })
}

/// Whether a checked JPEG is grey (one component) rather than colour (three).
pub(crate) fn jpeg_gray(logo: &Logo) -> bool {
    let b = &logo.bytes;
    let mut at = 2;
    while let (Some(0xFF), Some(&marker)) = (b.get(at), b.get(at + 1)) {
        if matches!(marker, 0xC0..=0xC2) {
            return b.get(at + 9) == Some(&1);
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let Some(len) = be16(b, at + 2) else { break };
        at += 2 + usize::from(len);
    }
    false
}

/// A PNG's pixels, 8 bits a sample: grey or RGB, and the transparency when it has any.
pub(crate) struct Pixels {
    pub gray: bool,
    pub color: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Undo the PNG row filters: each row is a filter byte, then `row` bytes.
fn unfilter(data: &[u8], row: usize, bpp: usize) -> Result<Vec<u8>, ExportError> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row];
    for line in data.chunks_exact(row + 1) {
        let (filter, src) = line.split_first().ok_or_else(unreadable)?;
        let mut cur = vec![0u8; row];
        for (x, raw) in src.iter().enumerate() {
            let left = |r: &[u8]| x.checked_sub(bpp).and_then(|i| r.get(i)).copied();
            let a = left(&cur).unwrap_or(0);
            let b = prev.get(x).copied().unwrap_or(0);
            let c = left(&prev).unwrap_or(0);
            let v = match filter {
                0 => *raw,
                1 => raw.wrapping_add(a),
                2 => raw.wrapping_add(b),
                3 => raw.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => raw.wrapping_add(paeth(a, b, c)),
                _ => return Err(unreadable()),
            };
            if let Some(slot) = cur.get_mut(x) {
                *slot = v;
            }
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

/// Decode a cleaned PNG (8-bit samples, palettes and low-bit grey, no interlacing).
pub(crate) fn png_pixels(logo: &Logo) -> Result<Pixels, ExportError> {
    let chunks = png_chunks(&logo.bytes)?;
    let ihdr = chunks
        .iter()
        .find(|(k, _, _)| k == b"IHDR")
        .map(|(_, d, _)| *d)
        .ok_or_else(unreadable)?;
    let [depth, ctype, method, filtering, interlace] =
        <[u8; 5]>::try_from(ihdr.get(8..13).ok_or_else(unreadable)?).map_err(|_| unreadable())?;
    if method != 0 || filtering != 0 || interlace != 0 {
        // Interlaced, or an unknown method.
        return Err(unreadable());
    }
    let channels: usize = match (ctype, depth) {
        (0 | 3, 1 | 2 | 4 | 8) => 1,
        (2, 8) => 3,
        (4, 8) => 2,
        (6, 8) => 4,
        _ => return Err(unreadable()),
    };
    let palette = chunks
        .iter()
        .find(|(k, _, _)| k == b"PLTE")
        .map(|(_, d, _)| *d);
    let trns = chunks
        .iter()
        .find(|(k, _, _)| k == b"tRNS")
        .map(|(_, d, _)| *d);
    if ctype == 3 && palette.is_none() {
        return Err(unreadable());
    }
    let (w, h) = (logo.width as usize, logo.height as usize);
    let bits = channels * usize::from(depth);
    let row = (w * bits).div_ceil(8);
    let bpp = (bits / 8).max(1);
    let expected = h * (row + 1);

    let compressed: Vec<u8> = chunks
        .iter()
        .filter(|(k, _, _)| k == b"IDAT")
        .flat_map(|(_, d, _)| d.iter().copied())
        .collect();
    // Never more than the picture needs: a small file cannot unpack into a huge one.
    let mut raw = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(compressed.as_slice())
        .take(expected as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| unreadable())?;
    if raw.len() != expected {
        return Err(unreadable());
    }
    let data = unfilter(&raw, row, bpp)?;

    let max = (1u16 << depth) - 1;
    // One sample of a palette or grey row, 1 to 8 bits wide.
    let sample = |line: &[u8], x: usize| -> Result<u8, ExportError> {
        let bit = x * usize::from(depth);
        let byte = *line.get(bit / 8).ok_or_else(unreadable)?;
        let shift = 8 - usize::from(depth) - bit % 8;
        Ok((byte >> shift) & (max as u8))
    };
    let key = |n: usize| -> Option<Vec<u16>> {
        trns.and_then(|t| (0..n).map(|i| be16(t, i * 2)).collect())
    };
    let gray = matches!(ctype, 0 | 4);
    let mut color = Vec::with_capacity(w * h * if gray { 1 } else { 3 });
    let mut alpha = Vec::with_capacity(w * h);
    for line in data.chunks_exact(row) {
        match ctype {
            2 => {
                let key = key(3);
                for px in line.chunks_exact(3) {
                    color.extend_from_slice(px);
                    let same = key
                        .as_ref()
                        .is_some_and(|k| k.iter().zip(px).all(|(k, p)| *k == u16::from(*p)));
                    alpha.push(if same { 0 } else { 255 });
                }
            }
            4 => {
                for px in line.chunks_exact(2) {
                    color.extend(px.first());
                    alpha.extend(px.get(1));
                }
            }
            6 => {
                for px in line.chunks_exact(4) {
                    color.extend(px.iter().take(3));
                    alpha.extend(px.get(3));
                }
            }
            0 => {
                let key = key(1).and_then(|k| k.first().copied());
                for x in 0..w {
                    let v = sample(line, x)?;
                    color.push(((u16::from(v) * 255) / max) as u8);
                    alpha.push(if key == Some(u16::from(v)) { 0 } else { 255 });
                }
            }
            _ => {
                for x in 0..w {
                    let i = usize::from(sample(line, x)?);
                    let rgb = palette
                        .and_then(|p| p.get(i * 3..i * 3 + 3))
                        .ok_or_else(unreadable)?;
                    color.extend_from_slice(rgb);
                    alpha.push(trns.and_then(|t| t.get(i).copied()).unwrap_or(255));
                }
            }
        }
    }
    let alpha = alpha.iter().any(|a| *a != 255).then_some(alpha);
    Ok(Pixels { gray, color, alpha })
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use std::io::Write;

    use super::*;

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(data);
        c.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
        c
    }

    /// A small PNG: `ctype` 2 (RGB) or 6 (RGBA), 8 bits, every row with filter `filter`,
    /// with a text chunk that must not survive.
    pub(crate) fn png(width: u32, height: u32, ctype: u8, filter: u8) -> Vec<u8> {
        let channels = if ctype == 6 { 4 } else { 3 };
        let mut ihdr = width.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, ctype, 0, 0, 0]);
        let mut raw = Vec::new();
        for y in 0..height {
            raw.push(filter);
            for x in 0..width * channels {
                raw.push(u8::try_from((x * 7 + y * 3) % 251).unwrap());
            }
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let mut out = PNG_SIGNATURE.to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"tEXt", b"Author\0Einat Bduya"));
        out.extend(chunk(b"IDAT", &z.finish().unwrap()));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    /// A minimal baseline JPEG header (frame, a table, one scan) with an EXIF segment and a
    /// comment that must not survive. Readers would draw it as noise; the tests only read it.
    pub(crate) fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        let exif = b"Exif\0\0GPS 32.08N 34.78E Einat";
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(exif);
        out.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x07]);
        out.extend_from_slice(b"hello");
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        out.extend(std::iter::repeat_n(1u8, 64));
        let mut sof = vec![0xFF, 0xC0, 0x00, 0x11, 8];
        sof.extend_from_slice(&height.to_be_bytes());
        sof.extend_from_slice(&width.to_be_bytes());
        sof.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 0, 3, 0x11, 0]);
        out.extend(sof);
        out.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0,
        ]);
        out.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD0, 0x78]);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out.extend_from_slice(b"trailing data after the end");
        out
    }

    #[test]
    fn png_rows_are_unfiltered_whatever_the_filter() {
        let plain = png_pixels(&check_logo(&png(5, 4, 2, 0)).unwrap()).unwrap();
        assert_eq!(plain.color.len(), 5 * 4 * 3);
        assert!(!plain.gray && plain.alpha.is_none());
        // With filters, the same raw bytes mean other pixels; each must decode without error.
        for f in 1..=4 {
            let p = png_pixels(&check_logo(&png(5, 4, 6, f)).unwrap()).unwrap();
            assert_eq!(p.color.len(), 5 * 4 * 3);
            assert_eq!(p.alpha.map(|a| a.len()), Some(5 * 4));
        }
        assert!(check_logo(&png(5, 4, 2, 9)).is_err(), "an unknown filter");
    }

    #[test]
    fn metadata_is_left_out() {
        let logo = check_logo(&png(3, 3, 2, 0)).unwrap();
        assert!(!logo.bytes.windows(5).any(|w| w == b"Einat"));
        assert!(!logo.bytes.windows(4).any(|w| w == b"tEXt"));
        let logo = check_logo(&jpeg(40, 20)).unwrap();
        assert_eq!((logo.width, logo.height), (40, 20));
        assert!(!logo.bytes.windows(5).any(|w| w == b"Einat"));
        assert!(!logo.bytes.windows(5).any(|w| w == b"hello"));
        assert!(!logo.bytes.windows(8).any(|w| w == b"trailing"));
        assert!(logo.bytes.ends_with(&[0xFF, 0xD9]));
        // The scan data, stuffed bytes and restart marker included, is kept whole.
        assert!(logo
            .bytes
            .windows(8)
            .any(|w| w == [0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD0, 0x78]));
    }

    #[test]
    fn a_damaged_png_is_refused() {
        let mut bad = png(4, 4, 2, 0);
        let n = bad.len();
        bad[n - 20] ^= 0x55; // inside the pixel data: the chunk's CRC no longer matches
        assert!(matches!(check_logo(&bad), Err(ExportError::Image(_))));
        assert!(check_logo(&bad[..30]).is_err(), "cut short");
    }

    #[test]
    fn anything_but_png_or_jpeg_is_refused_in_hebrew() {
        let refused = |bytes: &[u8]| match check_logo(bytes) {
            Err(ExportError::Image(m)) => m,
            other => panic!("not refused: {other:?}"),
        };
        assert!(refused(b"").contains("ריק"));
        let svg =
            refused(b"\xEF\xBB\xBF  <svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>");
        assert!(svg.contains("SVG"), "{svg}");
        assert!(refused(b"<?xml version=\"1.0\"?><svg/>").contains("SVG"));
        assert!(refused(b"GIF89a\x01\x00\x01\x00").contains("PNG או JPEG"));
        assert!(refused(b"BM\x00\x00\x00\x00").contains("PNG או JPEG"));
        // A logo named .png is still read by its bytes.
        assert!(refused(b"%PDF-1.7 not a picture").contains("PNG או JPEG"));
        let mut big = png(4, 4, 2, 0);
        big.resize(MAX_LOGO_BYTES + 1, 0);
        assert!(refused(&big).contains("1MB"));
        // Over the size in pixels.
        assert!(refused(&png(9000, 1, 2, 0)).contains("8000"));
        // A JPEG that is cut before its picture.
        assert!(check_logo(&[0xFF, 0xD8, 0xFF, 0xD9]).is_err());
    }

    #[test]
    fn checking_twice_gives_the_same_logo() {
        for bytes in [png(6, 3, 6, 4), jpeg(30, 10)] {
            let once = check_logo(&bytes).unwrap();
            assert_eq!(check_logo(&once.bytes).unwrap(), once);
        }
        assert!(!jpeg_gray(&check_logo(&jpeg(30, 10)).unwrap()));
    }
}
