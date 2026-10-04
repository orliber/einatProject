//! RTF: the body, not what hides around it.
//!
//! Word and LibreOffice write Hebrew either as `\uN` (Unicode, with a fallback that is
//! skipped) or as `\'hh` bytes in the code page of the run's font (`\fcharset177` is
//! Windows-1255). A byte in a code page this reader does not know refuses the file rather than
//! guessing letters. Headers and footers go to `margins`, `{\info}` to `metadata`; comments,
//! pictures, embedded objects, field codes and hidden text are skipped.

use std::collections::HashMap;

use crate::{codepage, Extracted, Format, IngestError};

/// Groups nested deeper than this are a hostile file, not a document.
const MAX_DEPTH: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dest {
    Body,
    Margins,
    /// A file property (`{\info{\author …}}`), collected under its name.
    Meta(&'static str),
    FontTable,
    /// `{\info}`: only its property groups are read.
    Info,
    Skip,
}

#[derive(Debug, Clone, Copy)]
struct State {
    dest: Dest,
    /// Characters to skip after `\uN` (`\ucN`).
    uc: usize,
    font: Option<i32>,
    /// The associated font of right-to-left runs (`\afN`), used inside `\rtlch`.
    afont: Option<i32>,
    rtl_run: bool,
    hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Cp1255,
    Cp1252,
    /// The Symbol font: its bytes are pictures (bullets, arrows), not letters.
    Symbol,
    Unknown,
}

fn page_of_charset(charset: i32) -> Page {
    match charset {
        177 => Page::Cp1255,
        2 => Page::Symbol,
        0 | 1 => Page::Cp1252,
        _ => Page::Unknown,
    }
}

fn page_of_ansicpg(cp: i32) -> Page {
    match cp {
        1255 => Page::Cp1255,
        1252 => Page::Cp1252,
        _ => Page::Unknown,
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// After a backslash: a control word with its optional number, or a control symbol.
    fn control(&mut self) -> (String, Option<i32>) {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        if self.pos == start {
            let sym = self
                .peek()
                .map_or(String::new(), |b| char::from(b).to_string());
            self.pos += 1;
            return (sym, None);
        }
        let word = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
        let num_start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while self.peek().is_some_and(|b| b.is_ascii_digit()) && self.pos - num_start < 11 {
            self.pos += 1;
        }
        let num = std::str::from_utf8(&self.bytes[num_start..self.pos])
            .ok()
            .and_then(|s| s.parse::<i32>().ok());
        if self.peek() == Some(b' ') {
            self.pos += 1;
        }
        (word, num)
    }

    fn hex_byte(&mut self) -> Option<u8> {
        let hex = self.bytes.get(self.pos..self.pos + 2)?;
        let v = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
        self.pos += 2;
        Some(v)
    }
}

fn corrupt(why: &str) -> IngestError {
    IngestError::Corrupt(why.to_owned())
}

/// Destinations whose text is never part of the document's visible body.
fn skipped(word: &str) -> bool {
    matches!(
        word,
        "colortbl"
            | "stylesheet"
            | "pict"
            | "object"
            | "objdata"
            | "themedata"
            | "colorschememapping"
            | "datastore"
            | "latentstyles"
            | "listtable"
            | "listoverridetable"
            | "rsidtbl"
            | "generator"
            | "xmlnstbl"
            | "fldinst"
            | "bkmkstart"
            | "bkmkend"
            | "annotation"
            | "atnid"
            | "atnauthor"
            | "atndate"
            | "atnref"
            | "atrfstart"
            | "atrfend"
            | "revtbl"
            | "pgdsctbl"
            | "filetbl"
            | "nonshppict"
            | "shpinst"
            | "userprops"
            | "docvar"
            | "passwordhash"
            | "template"
            | "mmathPr"
            | "wgrffmtfilter"
            | "pnseclvl"
            | "listtext"
            | "pntext"
    )
}

pub(crate) fn extract(bytes: &[u8]) -> Result<Extracted, IngestError> {
    if !bytes.starts_with(b"{\\rtf") {
        return Err(corrupt("not RTF"));
    }
    let mut r = Reader { bytes, pos: 0 };
    let mut stack: Vec<State> = Vec::new();
    let mut st = State {
        dest: Dest::Body,
        uc: 1,
        font: None,
        afont: None,
        rtl_run: false,
        hidden: false,
    };
    let mut ansi = Page::Cp1252;
    let mut fonts: HashMap<i32, Page> = HashMap::new();
    let mut table_font: Option<i32> = None;
    let mut body = String::new();
    let mut margins = String::new();
    let mut meta: Vec<(String, String)> = Vec::new();
    let mut had_comments = false;
    let mut had_hidden = false;
    // `\*`: the next control word starts a destination that may be skipped if unknown.
    let mut starred = false;
    // A new group's first control word decides its destination.
    let mut group_start = false;
    // Fallback characters still to skip after `\uN`.
    let mut skip = 0usize;
    let mut depth_seen = false;

    // Word marks a run with both its left-to-right font (`\f`) and its right-to-left one
    // (`\af`), and the letters decide which applies. Hebrew wins whenever either font (or the
    // document) says Hebrew: Hebrew misread as Western letters would hide a name from the
    // filter, while the reverse only garbles a rare accented letter.
    let page_now = |st: &State, fonts: &HashMap<i32, Page>, ansi: Page| {
        let of = |f: Option<i32>| f.and_then(|f| fonts.get(&f).copied()).unwrap_or(ansi);
        let (ltr, rtl) = (of(st.font), of(st.afont.or(st.font)));
        if ltr == Page::Cp1255 || rtl == Page::Cp1255 || ansi == Page::Cp1255 {
            Page::Cp1255
        } else if st.rtl_run {
            rtl
        } else {
            ltr
        }
    };

    while let Some(b) = r.peek() {
        r.pos += 1;
        match b {
            b'{' => {
                if stack.len() >= MAX_DEPTH {
                    return Err(corrupt("nesting"));
                }
                stack.push(st);
                group_start = true;
                depth_seen = true;
                skip = 0;
            }
            b'}' => {
                if st.dest == Dest::FontTable {
                    table_font = None;
                }
                st = stack.pop().ok_or_else(|| corrupt("unbalanced"))?;
                if st.dest == Dest::FontTable {
                    table_font = None;
                }
                starred = false;
                group_start = false;
                skip = 0;
                if stack.is_empty() && depth_seen {
                    break;
                }
            }
            b'\\' => {
                let (word, num) = r.control();
                let first = std::mem::take(&mut group_start);
                let was_starred = std::mem::take(&mut starred);
                match word.as_str() {
                    "*" => {
                        starred = true;
                        group_start = first;
                        continue;
                    }
                    "'" => {
                        let byte = r.hex_byte().ok_or_else(|| corrupt("hex"))?;
                        if skip > 0 {
                            skip -= 1;
                            continue;
                        }
                        let c = match page_now(&st, &fonts, ansi) {
                            Page::Cp1255 => codepage::cp1255(byte),
                            Page::Cp1252 => codepage::cp1252(byte),
                            Page::Symbol => continue,
                            Page::Unknown => None,
                        };
                        let c = match c {
                            Some(c) => c,
                            // Inside skipped parts an unknown byte harms nothing.
                            None if matches!(
                                st.dest,
                                Dest::Skip | Dest::FontTable | Dest::Info
                            ) =>
                            {
                                continue
                            }
                            None => return Err(IngestError::Unsupported),
                        };
                        push(&st, c, &mut body, &mut margins, &mut meta, &mut had_hidden);
                        continue;
                    }
                    "u" => {
                        let n = num.unwrap_or(0);
                        let n = if n < 0 { n + 65_536 } else { n };
                        let c = u32::try_from(n)
                            .ok()
                            .and_then(char::from_u32)
                            .unwrap_or('\u{FFFD}');
                        push(&st, c, &mut body, &mut margins, &mut meta, &mut had_hidden);
                        skip = st.uc;
                        continue;
                    }
                    "bin" => {
                        let n = usize::try_from(num.unwrap_or(0)).unwrap_or(0);
                        r.pos = r.pos.saturating_add(n).min(bytes.len());
                        continue;
                    }
                    _ => {}
                }
                // Destinations: only as a group's first word.
                if first {
                    let dest = match word.as_str() {
                        "fonttbl" => Some(Dest::FontTable),
                        "header" | "headerl" | "headerr" | "headerf" | "footer" | "footerl"
                        | "footerr" | "footerf" => Some(Dest::Margins),
                        "info" => Some(Dest::Info),
                        "title" => Some(Dest::Meta("title")),
                        "subject" => Some(Dest::Meta("subject")),
                        "author" => Some(Dest::Meta("creator")),
                        "operator" => Some(Dest::Meta("lastModifiedBy")),
                        "manager" => Some(Dest::Meta("Manager")),
                        "company" => Some(Dest::Meta("Company")),
                        "keywords" => Some(Dest::Meta("keywords")),
                        "doccomm" => Some(Dest::Meta("description")),
                        "annotation" => {
                            had_comments = true;
                            Some(Dest::Skip)
                        }
                        // Footnotes are part of the text, as in a Word file.
                        "footnote" | "fldrslt" | "shptxt" => None,
                        w if skipped(w) || was_starred => Some(Dest::Skip),
                        _ => None,
                    };
                    if let Some(d) = dest {
                        // Properties only inside `{\info}`; inside any part that is not body
                        // text, nothing becomes body again.
                        st.dest = match (st.dest, d) {
                            (Dest::Info, Dest::Meta(_)) => d,
                            (Dest::Body | Dest::Margins, Dest::Meta(_)) => Dest::Skip,
                            (Dest::Body | Dest::Margins, _) => d,
                            _ => Dest::Skip,
                        };
                        if let Dest::Meta(name) = st.dest {
                            meta.push((name.to_owned(), String::new()));
                        }
                        continue;
                    }
                }
                match word.as_str() {
                    "ansicpg" => ansi = page_of_ansicpg(num.unwrap_or(1252)),
                    "f" if st.dest == Dest::FontTable => table_font = num,
                    "f" => st.font = num,
                    "af" => st.afont = num,
                    "rtlch" => st.rtl_run = true,
                    "ltrch" => st.rtl_run = false,
                    "fcharset" if st.dest == Dest::FontTable => {
                        if let (Some(f), Some(cs)) = (table_font, num) {
                            fonts.insert(f, page_of_charset(cs));
                        }
                    }
                    "uc" => st.uc = usize::try_from(num.unwrap_or(1)).unwrap_or(1).min(8),
                    "v" => st.hidden = num != Some(0),
                    "plain" => {
                        st.hidden = false;
                        st.rtl_run = false;
                    }
                    "par" | "line" | "sect" | "page" | "row" | "column" => {
                        push(
                            &st,
                            '\n',
                            &mut body,
                            &mut margins,
                            &mut meta,
                            &mut had_hidden,
                        );
                    }
                    "tab" | "cell" => {
                        push(
                            &st,
                            '\t',
                            &mut body,
                            &mut margins,
                            &mut meta,
                            &mut had_hidden,
                        );
                    }
                    "~" => push(
                        &st,
                        ' ',
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    ),
                    "_" => push(
                        &st,
                        '-',
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    ),
                    "\\" | "{" | "}" => {
                        if skip > 0 {
                            skip -= 1;
                        } else {
                            let c = word.chars().next().unwrap_or(' ');
                            push(&st, c, &mut body, &mut margins, &mut meta, &mut had_hidden);
                        }
                    }
                    "emdash" => push(
                        &st,
                        '—',
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    ),
                    "endash" => push(
                        &st,
                        '–',
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    ),
                    "lquote" => push(
                        &st,
                        '‘',
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    ),
                    "rquote" => push(
                        &st,
                        '’',
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    ),
                    "ldblquote" => {
                        push(
                            &st,
                            '“',
                            &mut body,
                            &mut margins,
                            &mut meta,
                            &mut had_hidden,
                        );
                    }
                    "rdblquote" => {
                        push(
                            &st,
                            '”',
                            &mut body,
                            &mut margins,
                            &mut meta,
                            &mut had_hidden,
                        );
                    }
                    "bullet" => push(
                        &st,
                        '•',
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    ),
                    _ => {}
                }
            }
            b'\r' | b'\n' => {}
            _ => {
                group_start = false;
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                if b >= 0x80 {
                    // Raw 8-bit text: allowed by the format, read in the current code page.
                    let c = match page_now(&st, &fonts, ansi) {
                        Page::Cp1255 => codepage::cp1255(b),
                        Page::Cp1252 => codepage::cp1252(b),
                        Page::Symbol => continue,
                        Page::Unknown => None,
                    };
                    match c {
                        Some(c) => {
                            push(&st, c, &mut body, &mut margins, &mut meta, &mut had_hidden);
                        }
                        None if matches!(st.dest, Dest::Skip | Dest::FontTable | Dest::Info) => {}
                        None => return Err(IngestError::Unsupported),
                    }
                } else {
                    push(
                        &st,
                        char::from(b),
                        &mut body,
                        &mut margins,
                        &mut meta,
                        &mut had_hidden,
                    );
                }
            }
        }
    }
    if !stack.is_empty() && depth_seen && body.trim().is_empty() {
        return Err(corrupt("cut short"));
    }

    let mut warnings = Vec::new();
    if had_comments {
        warnings.push("הערות שוליים של עורכי המסמך (comments) לא יובאו.".to_owned());
    }
    if had_hidden {
        warnings.push("טקסט מוסתר במסמך לא יובא.".to_owned());
    }
    if !margins.trim().is_empty() {
        warnings.push("כותרת עליונה/תחתונה לא יובאה.".to_owned());
    }
    let metadata = meta
        .into_iter()
        .map(|(k, v)| (k, v.trim().to_owned()))
        .filter(|(_, v)| !v.is_empty())
        .collect();
    Ok(Extracted {
        format: Format::Rtf,
        body,
        margins,
        metadata,
        warnings,
        pages: 1,
    })
}

fn push(
    st: &State,
    c: char,
    body: &mut String,
    margins: &mut String,
    meta: &mut [(String, String)],
    had_hidden: &mut bool,
) {
    if st.hidden && !matches!(st.dest, Dest::Skip | Dest::FontTable | Dest::Info) {
        *had_hidden = true;
        return;
    }
    match st.dest {
        Dest::Body => body.push(c),
        Dest::Margins => margins.push(c),
        Dest::Meta(_) => {
            if let Some((_, v)) = meta.last_mut() {
                if v.len() < 1_000 {
                    v.push(c);
                }
            }
        }
        Dest::FontTable | Dest::Info | Dest::Skip => {}
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// What Word writes for a short Hebrew note (fabricated text).
    pub(crate) const WORD_STYLE: &str = r"{\rtf1\adeflang1037\ansi\ansicpg1255\uc1\adeff0\deff0{\fonttbl{\f0\fbidi \froman\fcharset177\fprq2 Times New Roman;}{\f1\fbidi \fswiss\fcharset0\fprq2 Arial;}}{\colortbl;\red0\green0\blue0;}{\*\generator Microsoft Word 16;}{\info{\title \'e3\'e5\'e7 \'e2\'ef}{\author \'e4\'e9\'ec\'e4 \'e1\'e3\'e5\'e9\'e4}{\operator Test User}}{\header \pard\plain \rtlch\af0 \'ee\'f8\'f4\'e0\'e4 \'e1\'e3\'e5\'e9\'e4\par}
\pard\plain \rtlch\fcs1 \af0 \ltrch\fcs0 \f1 {\rtlch\fcs1 \af0 \ltrch\fcs0 \'f0\'e5\'f2\'ed \'ee\'f9\'e7\'f7 \'ec\'e1\'e3}{\ltrch\fcs0 \f1  WPPSI-IV\par}
{\*\atnid AB}{\*\annotation \'e4\'f2\'f8\'e4 \'f4\'f0\'e9\'ee\'e9\'fa}{\v \'e8\'f7\'f1\'e8 \'ee\'e5\'f1\'fa\'f8}\u1513?\u1500?\u1493?\u1501?\par}";

    #[test]
    fn word_rtf_body_margins_and_properties() {
        let out = extract(WORD_STYLE.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.format, Format::Rtf);
        assert!(out.body.contains("נועם משחק לבד"), "{}", out.body);
        assert!(out.body.contains("WPPSI-IV"));
        assert!(out.body.contains("שלום"), "{}", out.body);
        assert!(!out.body.contains('?'), "fallback characters are skipped");
        assert!(!out.body.contains("הערה"), "comments stay out");
        assert!(!out.body.contains("טקסט"), "hidden text stays out");
        assert!(!out.body.contains("Times"), "font names stay out");
        assert!(out.margins.contains("מרפאה בדויה"));
        assert!(out
            .metadata
            .contains(&("creator".to_owned(), "הילה בדויה".to_owned())));
        assert!(out
            .metadata
            .contains(&("title".to_owned(), "דוח גן".to_owned())));
        assert!(out.warnings.iter().any(|w| w.contains("comments")));
        assert!(out.warnings.iter().any(|w| w.contains("מוסתר")));
    }

    #[test]
    fn unknown_code_page_is_refused_not_guessed() {
        // Arabic code page (1256): not read here, so not guessed.
        let rtf = r"{\rtf1\ansi\ansicpg1256{\fonttbl{\f0\fcharset178 Arial;}}\f0 \'c7\'e1\par}";
        assert_eq!(extract(rtf.as_bytes()), Err(IngestError::Unsupported));
    }

    #[test]
    fn field_codes_are_skipped_results_kept() {
        let rtf = r#"{\rtf1\ansi{\field{\*\fldinst HYPERLINK "http://example.invalid"}{\fldrslt link}}\par}"#;
        let out = extract(rtf.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.body.trim(), "link");
    }
}
