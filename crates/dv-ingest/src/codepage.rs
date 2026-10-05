//! The old one-byte Windows code pages that Word, RTF and older systems still produce:
//! Windows-1255 (Hebrew) and Windows-1252 (Western). A byte with no meaning in the page gives
//! `None`, so the caller refuses the file rather than guessing a letter (a guessed letter can
//! turn a name into something the filter does not recognize).

/// Windows-1255 (Hebrew). Direction marks (0xFD, 0xFE) give `Some('\u{200E}')`/`'\u{200F}'`
/// and are removed later with the other invisible characters.
#[must_use]
pub fn cp1255(b: u8) -> Option<char> {
    Some(match b {
        0x00..=0x7F => char::from(b),
        0xE0..=0xFA => char::from_u32(0x05D0 + u32::from(b - 0xE0))?,
        0xC0..=0xD3 => char::from_u32(0x05B0 + u32::from(b - 0xC0))?,
        0xD4..=0xD8 => char::from_u32(0x05F0 + u32::from(b - 0xD4))?,
        0x80 => '€',
        0x82 => '‚',
        0x84 => '„',
        0x85 => '…',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0xA0 => ' ',
        0xA4 => '₪',
        0xAA => '×',
        0xBA => '÷',
        0xA1..=0xBF => char::from(b),
        0xFD => '\u{200E}',
        0xFE => '\u{200F}',
        _ => return None,
    })
}

/// Windows-1252 (Western): Latin-1 plus the printer's quotes and dashes in 0x80–0x9F.
#[must_use]
pub fn cp1252(b: u8) -> Option<char> {
    const HIGH: [Option<char>; 32] = [
        Some('€'),
        None,
        Some('‚'),
        Some('ƒ'),
        Some('„'),
        Some('…'),
        Some('†'),
        Some('‡'),
        Some('ˆ'),
        Some('‰'),
        Some('Š'),
        Some('‹'),
        Some('Œ'),
        None,
        Some('Ž'),
        None,
        None,
        Some('‘'),
        Some('’'),
        Some('“'),
        Some('”'),
        Some('•'),
        Some('–'),
        Some('—'),
        Some('˜'),
        Some('™'),
        Some('š'),
        Some('›'),
        Some('œ'),
        None,
        Some('ž'),
        Some('Ÿ'),
    ];
    match b {
        0x80..=0x9F => HIGH[usize::from(b - 0x80)],
        _ => Some(char::from(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hebrew_and_western() {
        let shalom: String = [0xF9, 0xEC, 0xE5, 0xED]
            .iter()
            .filter_map(|b| cp1255(*b))
            .collect();
        assert_eq!(shalom, "שלום");
        assert_eq!(cp1255(0xDB), None);
        assert_eq!(cp1252(0x93), Some('“'));
        assert_eq!(cp1252(0xE9), Some('é'));
        assert_eq!(cp1252(0x81), None);
    }
}
