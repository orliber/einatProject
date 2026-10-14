//! The password-protected Word file, opened again by an independent reader written from
//! [MS-OFFCRYPTO] 2.3.4.10–2.3.4.15 (not from the crate's code): the right password gives
//! back the exact package, a wrong one is refused by the verifier, and a changed byte is
//! caught by the HMAC. Fabricated data only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use aws_lc_rs::cipher::{DecryptingKey, DecryptionContext, UnboundCipherKey, AES_256};
use aws_lc_rs::digest::{Context, SHA512};
use aws_lc_rs::hmac;
use aws_lc_rs::iv::FixedLength;
use dv_export::{encrypt, render, ExportError};
use quick_xml::events::Event;
use quick_xml::Reader;

const PASSWORD: &str = "נהר-ענן-42-שקד";

// [MS-OFFCRYPTO] 2.3.4.11–2.3.4.14 block keys.
const VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
const HMAC_KEY: [u8; 8] = [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
const HMAC_VALUE: [u8; 8] = [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

fn sha512(parts: &[&[u8]]) -> Vec<u8> {
    let mut ctx = Context::new(&SHA512);
    for p in parts {
        ctx.update(p);
    }
    ctx.finish().as_ref().to_vec()
}

fn aes_cbc_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    let key = DecryptingKey::cbc(UnboundCipherKey::new(&AES_256, key).unwrap()).unwrap();
    let iv: [u8; 16] = iv[..16].try_into().unwrap();
    let mut buf = data.to_vec();
    key.decrypt(&mut buf, DecryptionContext::Iv128(FixedLength::from(iv)))
        .unwrap()
        .to_vec()
}

fn b64decode(s: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in s.bytes().filter(|&c| c != b'=') {
        let v = ALPHABET.iter().position(|&a| a == c).unwrap();
        bits = (bits << 6) | u32::try_from(v).unwrap();
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push(u8::try_from((bits >> n) & 0xff).unwrap());
        }
    }
    out
}

fn stream(file: &[u8], path: &str) -> Vec<u8> {
    let mut cfb = cfb::CompoundFile::open(Cursor::new(file)).unwrap();
    let mut out = Vec::new();
    cfb.open_stream(path)
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    out
}

/// `element.attribute` → value, for the elements of the EncryptionInfo descriptor.
fn descriptor(info: &[u8]) -> HashMap<String, String> {
    assert_eq!(
        &info[..8],
        &[4, 0, 4, 0, 0x40, 0, 0, 0],
        "agile, version 4.4"
    );
    let xml = std::str::from_utf8(&info[8..]).unwrap();
    let mut reader = Reader::from_str(xml);
    let mut out = HashMap::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let element = e.local_name().as_ref().to_owned();
                for a in e.attributes() {
                    let a = a.unwrap();
                    out.insert(
                        format!("{element}.{}", a.key.local_name().as_ref()),
                        a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .unwrap()
                            .into_owned(),
                    );
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
enum OpenError {
    WrongPassword,
    Tampered,
}

/// Open an Agile-encrypted file as Word does.
fn open(file: &[u8], password: &str) -> Result<Vec<u8>, OpenError> {
    let d = descriptor(&stream(file, "/EncryptionInfo"));
    let get = |k: &str| b64decode(d.get(k).unwrap_or_else(|| panic!("{k} missing")));
    assert_eq!(d["encryptedKey.cipherAlgorithm"], "AES");
    assert_eq!(d["encryptedKey.hashAlgorithm"], "SHA512");
    assert_eq!(d["keyData.keyBits"], "256");
    let spins: u32 = d["encryptedKey.spinCount"].parse().unwrap();
    assert_eq!(spins, 100_000);

    // The key from the password (2.3.4.11).
    let salt = get("encryptedKey.saltValue");
    let pw: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut h = sha512(&[&salt, &pw]);
    for i in 0..spins {
        h = sha512(&[&i.to_le_bytes(), &h]);
    }
    let key_for = |block: &[u8; 8]| sha512(&[&h, block])[..32].to_vec();

    // The verifier (2.3.4.13).
    let input = aes_cbc_decrypt(
        &key_for(&VERIFIER_INPUT),
        &salt,
        &get("encryptedKey.encryptedVerifierHashInput"),
    );
    let value = aes_cbc_decrypt(
        &key_for(&VERIFIER_VALUE),
        &salt,
        &get("encryptedKey.encryptedVerifierHashValue"),
    );
    if sha512(&[&input[..16]]) != value[..64] {
        return Err(OpenError::WrongPassword);
    }
    let secret = aes_cbc_decrypt(
        &key_for(&KEY_VALUE),
        &salt,
        &get("encryptedKey.encryptedKeyValue"),
    )[..32]
        .to_vec();

    // Data integrity (2.3.4.14).
    let key_salt = get("keyData.saltValue");
    let package = stream(file, "/EncryptedPackage");
    let hmac_key = aes_cbc_decrypt(
        &secret,
        &sha512(&[&key_salt, &HMAC_KEY]),
        &get("dataIntegrity.encryptedHmacKey"),
    );
    let hmac_value = aes_cbc_decrypt(
        &secret,
        &sha512(&[&key_salt, &HMAC_VALUE]),
        &get("dataIntegrity.encryptedHmacValue"),
    );
    let k = hmac::Key::new(hmac::HMAC_SHA512, &hmac_key[..64]);
    if hmac::verify(&k, &package, &hmac_value[..64]).is_err() {
        return Err(OpenError::Tampered);
    }

    // The package (2.3.4.15): the size, then 4096-byte segments with their own IVs.
    let size = usize::try_from(u64::from_le_bytes(package[..8].try_into().unwrap())).unwrap();
    let mut plain = Vec::new();
    for (i, segment) in package[8..].chunks(4096).enumerate() {
        let index = u32::try_from(i).unwrap();
        plain.extend(aes_cbc_decrypt(
            &secret,
            &sha512(&[&key_salt, &index.to_le_bytes()]),
            segment,
        ));
    }
    assert!(plain.len() >= size, "package shorter than its size field");
    plain.truncate(size);
    Ok(plain)
}

#[test]
fn the_right_password_gives_back_the_exact_package() {
    let package = render(&common::sample()).unwrap();
    let file = encrypt(&package, PASSWORD).unwrap();
    assert_eq!(open(&file, PASSWORD).unwrap(), package);
}

/// Sizes around the 4096-byte segment boundary and the 16-byte block boundary.
#[test]
fn packages_of_any_size_round_trip() {
    for len in [0, 1, 15, 16, 17, 4095, 4096, 4097, 8192 + 5] {
        let package: Vec<u8> = (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect();
        let file = encrypt(&package, "סיסמה-ארוכה-בדויה").unwrap();
        assert_eq!(open(&file, "סיסמה-ארוכה-בדויה").unwrap(), package, "{len}");
    }
}

#[test]
fn a_wrong_password_is_refused_by_the_verifier() {
    let file = encrypt(b"PK fabricated", PASSWORD).unwrap();
    assert_eq!(open(&file, "נהר-ענן-42-שקע"), Err(OpenError::WrongPassword));
    // UTF-16, so a password differing only in case or normalization is a different one.
    let latin = encrypt(b"PK fabricated", "Correct-Horse-1").unwrap();
    assert_eq!(
        open(&latin, "correct-horse-1"),
        Err(OpenError::WrongPassword)
    );
}

/// A changed byte anywhere in the encrypted package is caught before anything is decrypted.
#[test]
fn a_changed_byte_in_the_package_is_caught() {
    let package = render(&common::sample()).unwrap();
    let file = encrypt(&package, PASSWORD).unwrap();
    let len = stream(&file, "/EncryptedPackage").len();
    for offset in [0, 9, len / 2, len - 1] {
        // Changed through the compound file, so the change lands in the stream itself.
        let mut cfb = cfb::CompoundFile::open(Cursor::new(file.clone())).unwrap();
        {
            let mut s = cfb.open_stream("/EncryptedPackage").unwrap();
            let mut byte = [0u8];
            s.seek(SeekFrom::Start(offset as u64)).unwrap();
            s.read_exact(&mut byte).unwrap();
            s.seek(SeekFrom::Start(offset as u64)).unwrap();
            s.write_all(&[byte[0] ^ 0x01]).unwrap();
        }
        cfb.flush().unwrap();
        let changed = cfb.into_inner().into_inner();
        assert_eq!(
            open(&changed, PASSWORD),
            Err(OpenError::Tampered),
            "offset {offset}"
        );
    }
}

/// The password rule counts characters, not bytes: ten Hebrew letters are enough, nine are
/// not.
#[test]
fn the_minimum_length_counts_characters() {
    assert_eq!(encrypt(b"x", "אבגדהוזחט"), Err(ExportError::WeakPassword));
    assert!(encrypt(b"x", "אבגדהוזחטי").is_ok());
}
