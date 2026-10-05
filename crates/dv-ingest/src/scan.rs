//! Scans and photos: the page images that OCR will read.
//!
//! This runs in the worker, like every other parser. It never reads text from an image: it only
//! finds the images (a scanned PDF's page images, or a photo of a page) and hands them back in a
//! form the OCR engine reads (JPEG, JPEG 2000, PNG, TIFF, or PNM for raw pixels). The app then
//! runs the OCR engine, itself isolated, once per page (D-048).
//!
//! Every size is checked before memory is spent: a "decompression bomb" is refused, not
//! unpacked.

use std::collections::HashSet;
use std::io::Read;

use pdf_extract::Document;
use serde::{Deserialize, Serialize};

use crate::IngestError;

/// More pixels than a 600 dpi A4 page in colour, or a 60-megapixel photo: refused.
pub const MAX_PIXELS: u64 = 60_000_000;
/// All page images of one document together, as handed to OCR.
const MAX_TOTAL_BYTES: usize = 160 * 1024 * 1024;
/// A scanned document longer than this is split by the psychologist.
pub const MAX_PAGES: usize = 60;
/// Smaller images are logos and signatures, not pages.
const MIN_SIDE: i64 = 300;

/// One page image, in a format the OCR engine reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageImage {
    pub page: u32,
    /// Resolution, when the PDF says how large the page is.
    pub dpi: Option<u32>,
    #[serde(with = "base64")]
    pub bytes: Vec<u8>,
}

/// What kind of image file this is, by its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageKind {
    Jpeg,
    Png,
    Tiff,
    /// An iPhone photo (HEIC/HEIF) or AVIF: not readable here.
    Heif,
}

pub(crate) fn image_kind(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageKind::Jpeg)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageKind::Png)
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some(ImageKind::Tiff)
    } else if bytes.get(4..8) == Some(b"ftyp")
        && bytes.get(8..12).is_some_and(|brand| {
            matches!(
                brand,
                b"heic"
                    | b"heix"
                    | b"hevc"
                    | b"hevx"
                    | b"heim"
                    | b"heis"
                    | b"mif1"
                    | b"msf1"
                    | b"avif"
            )
        })
    {
        Some(ImageKind::Heif)
    } else {
        None
    }
}

/// Width and height from a PNG or JPEG header (TIFF is left to the engine's own limits).
fn dimensions(kind: ImageKind, b: &[u8]) -> Option<(u64, u64)> {
    match kind {
        ImageKind::Png => {
            let w = u32::from_be_bytes(b.get(16..20)?.try_into().ok()?);
            let h = u32::from_be_bytes(b.get(20..24)?.try_into().ok()?);
            Some((u64::from(w), u64::from(h)))
        }
        ImageKind::Jpeg => {
            let mut i = 2;
            while i + 9 < b.len() {
                if b[i] != 0xFF {
                    return None;
                }
                let marker = b[i + 1];
                if marker == 0xFF {
                    i += 1;
                    continue;
                }
                let len = usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
                // Start of frame (any kind except DHT, JPG and DAC).
                if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    let h = u16::from_be_bytes([b[i + 5], b[i + 6]]);
                    let w = u16::from_be_bytes([b[i + 7], b[i + 8]]);
                    return Some((u64::from(w), u64::from(h)));
                }
                i += 2 + len;
            }
            None
        }
        ImageKind::Tiff | ImageKind::Heif => None,
    }
}

/// A photo or scan saved as an image file: one page, checked and handed on as it is.
pub(crate) fn from_image_file(bytes: &[u8]) -> Result<Vec<PageImage>, IngestError> {
    let kind = image_kind(bytes).ok_or(IngestError::Unsupported)?;
    if kind == ImageKind::Heif {
        return Err(IngestError::Heif);
    }
    if kind != ImageKind::Tiff {
        let (w, h) = dimensions(kind, bytes).ok_or_else(|| corrupt("image header"))?;
        if w == 0 || h == 0 {
            return Err(corrupt("empty image"));
        }
        if w.saturating_mul(h) > MAX_PIXELS {
            return Err(IngestError::TooLarge);
        }
    }
    Ok(vec![PageImage {
        page: 1,
        dpi: None,
        bytes: bytes.to_vec(),
    }])
}

fn corrupt(why: &str) -> IngestError {
    IngestError::Corrupt(why.to_owned())
}

/// The page images of a scanned PDF, page by page. Pages with no readable image are listed in
/// the warnings, never silently dropped.
pub(crate) fn from_pdf(doc: &Document) -> Result<(Vec<PageImage>, Vec<String>), IngestError> {
    let pages = doc.get_pages();
    if pages.len() > MAX_PAGES {
        return Err(IngestError::TooManyPages);
    }
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut unreadable: Vec<u32> = Vec::new();
    for (num, page_id) in pages {
        let width_pt = page_width(doc, page_id);
        let mut found = Vec::new();
        let mut seen = HashSet::new();
        if let Ok((inline, ids)) = doc.get_page_resources(page_id) {
            let mut dicts: Vec<&pdf_extract::Dictionary> = inline.into_iter().collect();
            dicts.extend(ids.iter().filter_map(|id| doc.get_dictionary(*id).ok()));
            for res in dicts {
                collect(doc, res, 0, &mut seen, &mut found);
            }
        }
        let mut any = false;
        for stream in found {
            if let Some((bytes, w)) = convert(doc, stream)? {
                total = total.saturating_add(bytes.len());
                if total > MAX_TOTAL_BYTES {
                    return Err(IngestError::TooLarge);
                }
                let dpi = width_pt
                    .filter(|pt| *pt > 1.0)
                    .map(|pt| (w as f64) / (pt / 72.0))
                    .filter(|d| (50.0..=1200.0).contains(d))
                    .map(|d| d.round() as u32);
                out.push(PageImage {
                    page: num,
                    dpi,
                    bytes,
                });
                any = true;
            }
        }
        if !any {
            unreadable.push(num);
        }
    }
    let mut warnings = Vec::new();
    if !unreadable.is_empty() && !out.is_empty() {
        let list: Vec<String> = unreadable.iter().map(u32::to_string).collect();
        warnings.push(format!(
            "בעמודים {} לא נמצאה תמונה שאפשר לקרוא, והם לא יובאו.",
            list.join(", ")
        ));
    }
    Ok((out, warnings))
}

fn page_width(doc: &Document, page_id: pdf_extract::ObjectId) -> Option<f64> {
    let mut id = Some(page_id);
    // MediaBox can be inherited from the page tree.
    for _ in 0..16 {
        let dict = doc.get_dictionary(id?).ok()?;
        if let Ok(mb) = dict
            .get(b"MediaBox")
            .and_then(|o| doc.dereference(o).map(|(_, o)| o))
        {
            let a = mb.as_array().ok()?;
            let n = |i: usize| a.get(i).and_then(num);
            return Some((n(2)? - n(0)?).abs());
        }
        id = dict
            .get(b"Parent")
            .and_then(pdf_extract::Object::as_reference)
            .ok();
    }
    None
}

fn num(o: &pdf_extract::Object) -> Option<f64> {
    match o {
        pdf_extract::Object::Integer(i) => Some(*i as f64),
        pdf_extract::Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

/// Image streams under a resources dictionary, following forms a few levels deep.
fn collect<'a>(
    doc: &'a Document,
    res: &'a pdf_extract::Dictionary,
    depth: usize,
    seen: &mut HashSet<pdf_extract::ObjectId>,
    out: &mut Vec<&'a pdf_extract::Stream>,
) {
    if depth > 4 {
        return;
    }
    let Ok(xobjects) = res
        .get(b"XObject")
        .and_then(|o| doc.dereference(o).map(|(_, o)| o))
        .and_then(pdf_extract::Object::as_dict)
    else {
        return;
    };
    for (_, v) in xobjects {
        let Ok(id) = v.as_reference() else { continue };
        if !seen.insert(id) {
            continue;
        }
        let Ok(stream) = doc.get_object(id).and_then(pdf_extract::Object::as_stream) else {
            continue;
        };
        match stream
            .dict
            .get(b"Subtype")
            .and_then(pdf_extract::Object::as_name)
        {
            Ok(b"Image") => {
                let side = |k: &[u8]| {
                    stream
                        .dict
                        .get(k)
                        .and_then(pdf_extract::Object::as_i64)
                        .unwrap_or(0)
                };
                if side(b"Width") >= MIN_SIDE && side(b"Height") >= MIN_SIDE {
                    out.push(stream);
                }
            }
            Ok(b"Form") => {
                if let Ok(inner) = stream
                    .dict
                    .get(b"Resources")
                    .and_then(|o| doc.dereference(o).map(|(_, o)| o))
                    .and_then(pdf_extract::Object::as_dict)
                {
                    collect(doc, inner, depth + 1, seen, out);
                }
            }
            _ => {}
        }
    }
}

fn get_i64(dict: &pdf_extract::Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key).and_then(pdf_extract::Object::as_i64).ok()
}

fn get_bool(dict: &pdf_extract::Dictionary, key: &[u8]) -> bool {
    dict.get(key)
        .and_then(pdf_extract::Object::as_bool)
        .unwrap_or(false)
}

/// One image stream → bytes the OCR engine reads, and its width. `None`: a kind of image
/// this reader does not unpack (JBIG2, unusual colour spaces).
fn convert(
    doc: &Document,
    stream: &pdf_extract::Stream,
) -> Result<Option<(Vec<u8>, u64)>, IngestError> {
    let dict = &stream.dict;
    let w = get_i64(dict, b"Width").unwrap_or(0);
    let h = get_i64(dict, b"Height").unwrap_or(0);
    let (Ok(w), Ok(h)) = (u64::try_from(w), u64::try_from(h)) else {
        return Ok(None);
    };
    if w.saturating_mul(h) > MAX_PIXELS {
        return Err(IngestError::TooLarge);
    }
    let filters: Vec<Vec<u8>> = stream
        .filters()
        .map(|f| f.into_iter().map(<[u8]>::to_vec).collect())
        .unwrap_or_default();
    let params_list: Vec<Option<&pdf_extract::Dictionary>> = match dict.get(b"DecodeParms") {
        Ok(pdf_extract::Object::Array(a)) => a
            .iter()
            .map(|o| doc.dereference(o).ok().and_then(|(_, o)| o.as_dict().ok()))
            .collect(),
        Ok(o) => vec![doc.dereference(o).ok().and_then(|(_, o)| o.as_dict().ok())],
        Err(_) => Vec::new(),
    };
    let params = |i: usize| params_list.get(i).copied().flatten();

    match filters.as_slice() {
        [] => raw(doc, dict, &stream.content, w, h),
        [f] if f == b"DCTDecode" => Ok(Some((stream.content.clone(), w))),
        [f] if f == b"JPXDecode" => Ok(Some((stream.content.clone(), w))),
        [f] if f == b"CCITTFaxDecode" => {
            Ok(Some((ccitt_tiff(&stream.content, params(0), w, h)?, w)))
        }
        [f] if f == b"FlateDecode" => {
            let expected = raw_len(dict, w, h).ok_or_else(|| corrupt("image size"))?;
            let mut data = inflate(&stream.content, expected + h as usize + 1024)?;
            if let Some(p) = params(0) {
                data = unpredict(data, p)?;
            }
            raw(doc, dict, &data, w, h)
        }
        _ => Ok(None),
    }
}

/// Bytes of raw pixel data the image needs.
fn raw_len(dict: &pdf_extract::Dictionary, w: u64, h: u64) -> Option<usize> {
    let bpc = u64::try_from(get_i64(dict, b"BitsPerComponent").unwrap_or(1)).ok()?;
    let comps = if get_bool(dict, b"ImageMask") { 1 } else { 4 };
    let row = (w.checked_mul(bpc)?.checked_mul(comps)?).div_ceil(8);
    usize::try_from(row.checked_mul(h)?).ok()
}

/// zlib, bounded: never more than `cap` bytes come out.
fn inflate(data: &[u8], cap: usize) -> Result<Vec<u8>, IngestError> {
    let mut out = Vec::new();
    let mut z = flate2::read::ZlibDecoder::new(data);
    let cap64 = u64::try_from(cap).unwrap_or(u64::MAX);
    if (&mut z).take(cap64 + 1).read_to_end(&mut out).is_err() && out.is_empty() {
        return Err(corrupt("image stream"));
    }
    if out.len() > cap {
        return Err(IngestError::TooLarge);
    }
    Ok(out)
}

/// Undo the PNG row filters a PDF image may carry (`/Predictor` 10–15).
fn unpredict(data: Vec<u8>, p: &pdf_extract::Dictionary) -> Result<Vec<u8>, IngestError> {
    let predictor = get_i64(p, b"Predictor").unwrap_or(1);
    if predictor < 10 {
        return if predictor <= 1 {
            Ok(data)
        } else {
            Err(corrupt("TIFF predictor"))
        };
    }
    let colors = usize::try_from(get_i64(p, b"Colors").unwrap_or(1).clamp(1, 4)).unwrap_or(1);
    let bpc =
        usize::try_from(get_i64(p, b"BitsPerComponent").unwrap_or(8).clamp(1, 16)).unwrap_or(8);
    let columns = usize::try_from(get_i64(p, b"Columns").unwrap_or(1).max(1)).unwrap_or(1);
    let row = (columns * colors * bpc).div_ceil(8);
    let bpp = (colors * bpc).div_ceil(8).max(1);
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row];
    for chunk in data.chunks(row + 1) {
        if chunk.len() < row + 1 {
            break;
        }
        let kind = chunk[0];
        let mut cur = chunk[1..].to_vec();
        for i in 0..row {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let add = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => {
                    let (pa, pb, pc) = (
                        (i16::from(b) - i16::from(c)).abs(),
                        (i16::from(a) - i16::from(c)).abs(),
                        (i16::from(a) + i16::from(b) - 2 * i16::from(c)).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => return Err(corrupt("row filter")),
            };
            cur[i] = cur[i].wrapping_add(add);
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

/// The colour model of an image: components, and a palette for indexed images.
enum Colors {
    Gray,
    Rgb,
    Cmyk,
    Indexed { base: Box<Colors>, palette: Vec<u8> },
}

impl Colors {
    fn comps(&self) -> usize {
        match self {
            Self::Gray | Self::Indexed { .. } => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }
}

fn colors(doc: &Document, cs: &pdf_extract::Object, depth: usize) -> Option<Colors> {
    if depth > 3 {
        return None;
    }
    let cs = doc.dereference(cs).ok()?.1;
    match cs {
        pdf_extract::Object::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"CalGray" | b"G" => Some(Colors::Gray),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Some(Colors::Rgb),
            b"DeviceCMYK" | b"CMYK" => Some(Colors::Cmyk),
            _ => None,
        },
        pdf_extract::Object::Array(a) => {
            let kind = a.first()?.as_name().ok()?;
            match kind {
                b"ICCBased" => {
                    let s = doc.dereference(a.get(1)?).ok()?.1.as_stream().ok()?;
                    match get_i64(&s.dict, b"N")? {
                        1 => Some(Colors::Gray),
                        3 => Some(Colors::Rgb),
                        4 => Some(Colors::Cmyk),
                        _ => None,
                    }
                }
                b"CalGray" => Some(Colors::Gray),
                b"CalRGB" => Some(Colors::Rgb),
                b"Indexed" | b"I" => {
                    let base = colors(doc, a.get(1)?, depth + 1)?;
                    let hival =
                        usize::try_from(doc.dereference(a.get(2)?).ok()?.1.as_i64().ok()?).ok()?;
                    let lookup = doc.dereference(a.get(3)?).ok()?.1;
                    let palette = match lookup {
                        pdf_extract::Object::String(s, _) => s.clone(),
                        pdf_extract::Object::Stream(s) => s.decompressed_content().ok()?,
                        _ => return None,
                    };
                    if hival > 255 || palette.len() < (hival + 1) * base.comps() {
                        return None;
                    }
                    Some(Colors::Indexed {
                        base: Box::new(base),
                        palette,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Raw pixel data → PNM (PBM for black-and-white, PGM for gray, PPM for colour).
fn raw(
    doc: &Document,
    dict: &pdf_extract::Dictionary,
    data: &[u8],
    w: u64,
    h: u64,
) -> Result<Option<(Vec<u8>, u64)>, IngestError> {
    let mask = get_bool(dict, b"ImageMask");
    let bpc = if mask {
        1
    } else {
        get_i64(dict, b"BitsPerComponent").unwrap_or(8)
    };
    let Ok(bpc) = usize::try_from(bpc) else {
        return Ok(None);
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return Ok(None);
    }
    let cs = if mask {
        Colors::Gray
    } else {
        match dict.get(b"ColorSpace").ok().and_then(|o| colors(doc, o, 0)) {
            Some(c) => c,
            None => return Ok(None),
        }
    };
    // `/Decode [1 0]` swaps black and white (gray and masks only).
    let inverted = dict
        .get(b"Decode")
        .and_then(pdf_extract::Object::as_array)
        .ok()
        .and_then(|a| a.first().and_then(num))
        .is_some_and(|v| v >= 1.0);
    let (wu, hu) = (
        usize::try_from(w).map_err(|_| IngestError::TooLarge)?,
        usize::try_from(h).map_err(|_| IngestError::TooLarge)?,
    );
    let comps = cs.comps();
    let row = (wu * comps * bpc).div_ceil(8);
    if data.len() < row * hu {
        return Err(corrupt("image data cut short"));
    }

    // Black and white: PBM, where 1 is black. In a PDF, 0 is black (or "paint" in a mask).
    if bpc == 1 && matches!(cs, Colors::Gray) {
        let mut out = format!("P4\n{wu} {hu}\n").into_bytes();
        for r in 0..hu {
            let line = &data[r * row..(r + 1) * row];
            out.extend(line.iter().map(|b| if inverted { *b } else { !*b }));
        }
        return Ok(Some((out, w)));
    }

    let sample = |line: &[u8], i: usize| -> u8 {
        match bpc {
            8 => line[i],
            16 => line[i * 2],
            _ => {
                let bit = i * bpc;
                let v = (line[bit / 8] >> (8 - bpc - bit % 8)) & ((1u8 << bpc) - 1);
                if matches!(cs, Colors::Indexed { .. }) {
                    v
                } else {
                    // Scale 1/2/4-bit gray to 0–255.
                    (u16::from(v) * 255 / ((1u16 << bpc) - 1)) as u8
                }
            }
        }
    };
    let gray = matches!(&cs, Colors::Gray)
        || matches!(&cs, Colors::Indexed { base, .. } if matches!(**base, Colors::Gray));
    let mut out = format!("{}\n{wu} {hu}\n255\n", if gray { "P5" } else { "P6" }).into_bytes();
    out.reserve(wu * hu * if gray { 1 } else { 3 });
    for r in 0..hu {
        let line = &data[r * row..(r + 1) * row];
        for x in 0..wu {
            match &cs {
                Colors::Gray => {
                    let v = sample(line, x);
                    out.push(if inverted { 255 - v } else { v });
                }
                Colors::Rgb => {
                    for c in 0..3 {
                        out.push(sample(line, x * 3 + c));
                    }
                }
                Colors::Cmyk => {
                    let k = u16::from(sample(line, x * 4 + 3));
                    for c in 0..3 {
                        let v = u16::from(sample(line, x * 4 + c));
                        out.push((255u16.saturating_sub((v + k).min(255))) as u8);
                    }
                }
                Colors::Indexed { base, palette } => {
                    let idx = usize::from(sample(line, x));
                    let n = base.comps();
                    let entry = palette
                        .get(idx * n..idx * n + n)
                        .unwrap_or(&[0, 0, 0, 0][..n]);
                    match **base {
                        Colors::Gray => out.push(entry[0]),
                        Colors::Rgb => out.extend_from_slice(entry),
                        Colors::Cmyk => {
                            let k = u16::from(entry[3]);
                            for v in &entry[..3] {
                                out.push(
                                    (255u16.saturating_sub((u16::from(*v) + k).min(255))) as u8,
                                );
                            }
                        }
                        Colors::Indexed { .. } => out.extend_from_slice(&[0, 0, 0]),
                    }
                }
            }
        }
    }
    Ok(Some((out, w)))
}

/// Fax-compressed black-and-white page (the most common scanner output) → a one-strip TIFF.
fn ccitt_tiff(
    data: &[u8],
    params: Option<&pdf_extract::Dictionary>,
    w: u64,
    h: u64,
) -> Result<Vec<u8>, IngestError> {
    let k = params.and_then(|p| get_i64(p, b"K")).unwrap_or(0);
    let columns = params
        .and_then(|p| get_i64(p, b"Columns"))
        .and_then(|c| u64::try_from(c).ok())
        .unwrap_or(w);
    let rows = params
        .and_then(|p| get_i64(p, b"Rows"))
        .and_then(|r| u64::try_from(r).ok())
        .filter(|r| *r > 0)
        .unwrap_or(h);
    let black_is_1 = params.is_some_and(|p| get_bool(p, b"BlackIs1"));
    let byte_align = params.is_some_and(|p| get_bool(p, b"EncodedByteAlign"));
    let to32 = |v: u64| u32::try_from(v).map_err(|_| IngestError::TooLarge);
    let len = to32(u64::try_from(data.len()).unwrap_or(u64::MAX))?;

    let mut entries: Vec<(u16, u16, u32)> = vec![
        (256, 4, to32(columns)?),
        (257, 4, to32(rows)?),
        (258, 3, 1),
        (259, 3, if k < 0 { 4 } else { 3 }),
        // PDF fax data with BlackIs1 false has 0 for black: TIFF "WhiteIsZero" reads it right.
        (262, 3, u32::from(black_is_1)),
        (273, 4, 0), // strip offset, filled below
        (277, 3, 1),
        (278, 4, to32(rows)?),
        (279, 4, len),
    ];
    if k >= 0 {
        let mut opts = if k > 0 { 1 } else { 0 };
        if byte_align {
            opts |= 4;
        }
        entries.push((292, 4, opts));
    }
    let count = u16::try_from(entries.len()).unwrap_or(0);
    let ifd_len = 2 + entries.len() * 12 + 4;
    let data_at = to32(8 + u64::try_from(ifd_len).unwrap_or(0))?;
    let mut out = Vec::with_capacity(8 + ifd_len + data.len());
    out.extend_from_slice(b"II*\0");
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    for (tag, typ, val) in entries {
        let val = if tag == 273 { data_at } else { val };
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&typ.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        if typ == 3 {
            out.extend_from_slice(&u16::try_from(val).unwrap_or(0).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
        } else {
            out.extend_from_slice(&val.to_le_bytes());
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    Ok(out)
}

/// Images cross the process boundary inside JSON: base64 keeps them a third larger, not four
/// times (a JSON array of numbers).
mod base64 {
    use serde::{Deserialize, Deserializer, Serializer};

    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(char::from(ABC[((n >> (18 - 6 * i)) & 63) as usize]));
                } else {
                    out.push('=');
                }
            }
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        let mut out = Vec::with_capacity(s.len() / 4 * 3);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in s.bytes() {
            if c == b'=' {
                break;
            }
            let v = ABC
                .iter()
                .position(|a| *a == c)
                .ok_or_else(|| serde::de::Error::custom("base64"))?;
            acc = (acc << 6) | u32::try_from(v).unwrap_or(0);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((acc >> bits) & 0xFF) as u8);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for len in 0..10 {
            let img = PageImage {
                page: 1,
                dpi: None,
                bytes: (0..len).map(|i| (i * 37 + 5) as u8).collect(),
            };
            let json = serde_json::to_string(&img).unwrap_or_default();
            let back: PageImage = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(back, img);
        }
    }

    #[test]
    fn photos_are_recognized_and_heic_refused() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&1000u32.to_be_bytes());
        png.extend_from_slice(&1400u32.to_be_bytes());
        assert_eq!(from_image_file(&png).map(|v| v.len()), Ok(1));
        let mut huge = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        huge.extend_from_slice(&20_000u32.to_be_bytes());
        huge.extend_from_slice(&20_000u32.to_be_bytes());
        assert_eq!(from_image_file(&huge), Err(IngestError::TooLarge));
        let heic = b"\0\0\0\x18ftypheic\0\0\0\0";
        assert_eq!(from_image_file(heic), Err(IngestError::Heif));
        // JPEG: SOI, APP0, then SOF0 with height 1200 and width 900.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0x04, 0xB0, 0x03, 0x84, 1, 0,
            0, 0, 0,
        ];
        assert_eq!(dimensions(ImageKind::Jpeg, &jpeg), Some((900, 1200)));
    }

    #[test]
    fn png_row_filters_are_undone() {
        let mut p = pdf_extract::Dictionary::new();
        p.set("Predictor", 15);
        p.set("Columns", 3);
        // Row 1 "Sub" (+left), row 2 "Up" (+above).
        let data = vec![1, 10, 5, 5, 2, 1, 1, 1];
        assert_eq!(
            unpredict(data, &p).unwrap_or_default(),
            vec![10, 15, 20, 11, 16, 21]
        );
    }

    #[test]
    fn fax_page_becomes_a_tiff() {
        let mut p = pdf_extract::Dictionary::new();
        p.set("K", -1);
        p.set("Columns", 1728);
        let tiff = ccitt_tiff(&[1, 2, 3], Some(&p), 1728, 2200).unwrap_or_default();
        assert!(tiff.starts_with(b"II*\0"));
        assert!(tiff.ends_with(&[1, 2, 3]));
    }
}
