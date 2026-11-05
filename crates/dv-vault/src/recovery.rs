//! Printed recovery key (D-016): 256 bits in Crockford Base32, 13 groups of 4 characters
//! plus one checksum group. No ambiguous characters (0/O, 1/I/L are read as the same).

use zeroize::Zeroizing;

use crate::crypto::{hkdf, sha256_hex, Key32};
use crate::VaultError;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const KEY_CHARS: usize = 52; // ceil(256 / 5)
const CHECK_CHARS: usize = 4;

fn encode_bits(bytes: &[u8], chars: usize) -> String {
    let mut out = String::with_capacity(chars);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut iter = bytes.iter();
    while out.len() < chars {
        if bits < 5 {
            acc = (acc << 8) | u32::from(*iter.next().unwrap_or(&0));
            bits += 8;
        }
        let idx = (acc >> (bits - 5)) & 31;
        bits -= 5;
        // Five bits always name one of the 32 letters; '?' would make the key fail to parse
        // back, never print a key that opens nothing.
        out.push(ALPHABET.get(idx as usize).map_or('?', |&c| char::from(c)));
    }
    out
}

fn decode_char(c: char) -> Option<u32> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|&a| char::from(a) == c)
        .and_then(|p| u32::try_from(p).ok())
}

fn checksum(key: &[u8; 32]) -> String {
    let digest = sha256_hex(key);
    let bytes: Vec<u8> = (0..4)
        .filter_map(|i| u8::from_str_radix(digest.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    encode_bits(&bytes, CHECK_CHARS)
}

/// The recovery key as it is printed: `XXXX-XXXX-…` (14 groups).
#[must_use]
pub fn format(key: &Key32) -> Zeroizing<String> {
    let mut chars = encode_bits(key.as_bytes(), KEY_CHARS);
    chars.push_str(&checksum(key.as_bytes()));
    let grouped: Vec<String> = chars
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    Zeroizing::new(grouped.join("-"))
}

/// A separator between groups: whitespace, or any kind of dash. The printed key has a plain
/// hyphen-minus; a Hebrew keyboard, autocorrect or a word processor turns it into a maqaf,
/// an en or em dash, a minus sign or a non-breaking hyphen, and the key is still hers.
fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '-' | '\u{00AD}' | '\u{05BE}' | '\u{2010}'
                ..='\u{2015}' | '\u{2212}' | '\u{FE63}' | '\u{FF0D}'
        )
}

/// Parse what the user typed. Spaces, dashes (of any kind) and case do not matter.
pub fn parse(typed: &str) -> Result<Key32, VaultError> {
    let symbols: Vec<u32> = typed
        .chars()
        .filter(|c| !is_separator(*c))
        .map(|c| decode_char(c).ok_or(VaultError::RecoveryKeyInvalid))
        .collect::<Result<_, _>>()?;
    if symbols.len() != KEY_CHARS + CHECK_CHARS {
        return Err(VaultError::RecoveryKeyInvalid);
    }
    let (key_symbols, check_symbols) = symbols
        .split_at_checked(KEY_CHARS)
        .ok_or(VaultError::RecoveryKeyInvalid)?;
    let mut out = [0u8; 32];
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut pos = 0usize;
    for &s in key_symbols {
        acc = (acc << 5) | s;
        bits += 5;
        if bits >= 8 {
            if let Some(byte) = out.get_mut(pos) {
                *byte = u8::try_from((acc >> (bits - 8)) & 0xff).unwrap_or(0);
                pos += 1;
                bits -= 8;
            }
        }
        acc &= (1 << bits) - 1;
    }
    let key = Key32::from_bytes(out);
    let expected: Vec<u32> = checksum(key.as_bytes())
        .chars()
        .filter_map(decode_char)
        .collect();
    if expected != check_symbols {
        return Err(VaultError::RecoveryKeyInvalid);
    }
    Ok(key)
}

/// Key-encryption key for the recovery slot.
pub fn derive_kek(recovery: &Key32) -> Result<Key32, VaultError> {
    hkdf(recovery.as_bytes(), "dv/slot/recovery/v1")
}

/// Key-encryption key for the Google slot (D-041): the key kept in her Google Drive AND the
/// key sealed to her Windows account on this computer. Neither alone opens the slot, so a
/// broken-into Google account with a stolen backup reads nothing.
pub fn derive_google_kek(drive_key: &Key32, computer_key: &Key32) -> Result<Key32, VaultError> {
    let both = Zeroizing::new([drive_key.as_bytes().as_slice(), computer_key.as_bytes()].concat());
    hkdf(&both, "dv/slot/google/v1")
}

/// Who the Google slot belongs to: the account id, hashed with this vault's id so the header
/// never carries the id itself and two vaults never share a value.
#[must_use]
pub fn google_account_tag(vault_id: &str, google_account_id: &str) -> String {
    sha256_hex(format!("dv/google-account/v1|{vault_id}|{google_account_id}").as_bytes())
}

/// The key as it is kept in the Drive file: 64 hex characters, nothing else.
#[must_use]
pub fn drive_key_text(drive_key: &Key32) -> Zeroizing<String> {
    Zeroizing::new(crate::crypto::hex(drive_key.as_bytes()))
}

pub fn parse_drive_key(text: &str) -> Result<Key32, VaultError> {
    let bytes =
        Zeroizing::new(crate::crypto::unhex(text.trim()).map_err(|_| VaultError::WrongSecret)?);
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::WrongSecret)?;
    Ok(Key32::from_bytes(arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_parse_round_trip() {
        for _ in 0..50 {
            let key = Key32::random().unwrap();
            let printed = format(&key);
            assert_eq!(printed.split('-').count(), 14);
            assert_eq!(parse(&printed).unwrap(), key);
        }
    }

    #[test]
    fn parsing_forgives_case_spacing_and_lookalikes() {
        let key = Key32::from_bytes([0u8; 32]);
        let printed = format(&key)
            .to_lowercase()
            .replace('0', "o")
            .replace('-', " ");
        assert_eq!(parse(&printed).unwrap(), key);
    }

    #[test]
    fn parsing_forgives_the_dashes_a_keyboard_or_a_word_processor_makes() {
        let key = Key32::random().unwrap();
        let printed = format(&key);
        // En dash, em dash, Hebrew maqaf, minus sign, non-breaking hyphen, figure dash,
        // fullwidth and small hyphen-minus: what a Hebrew keyboard, autocorrect or a PDF
        // reader turns the printed dash into.
        for dash in [
            '\u{2013}', '\u{2014}', '\u{05BE}', '\u{2212}', '\u{2011}', '\u{2012}', '\u{FF0D}',
            '\u{FE63}', '\u{2010}', '\u{2015}',
        ] {
            let typed = printed.replace('-', &dash.to_string());
            assert_eq!(parse(&typed).unwrap(), key, "{dash:?}");
        }
        // Mixed, with spaces around some dashes and a trailing newline.
        let mixed = printed.replacen('-', " \u{2013} ", 3) + "\n";
        assert_eq!(parse(&mixed).unwrap(), key);
        // A dash is a separator and nothing more: one in place of a letter is still a typo.
        let mut chars: Vec<char> = printed.chars().collect();
        chars[1] = '\u{2013}';
        let short: String = chars.into_iter().collect();
        assert!(matches!(parse(&short), Err(VaultError::RecoveryKeyInvalid)));
    }

    #[test]
    fn typos_are_caught_by_the_checksum() {
        let key = Key32::random().unwrap();
        let mut printed: Vec<char> = format(&key).chars().collect();
        printed[0] = if printed[0] == 'A' { 'B' } else { 'A' };
        let typo: String = printed.into_iter().collect();
        assert!(matches!(parse(&typo), Err(VaultError::RecoveryKeyInvalid)));
        assert!(parse("too-short").is_err());
    }
}
