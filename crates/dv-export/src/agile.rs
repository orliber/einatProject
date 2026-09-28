//! ECMA-376 Agile Encryption with a password key encryptor ([MS-OFFCRYPTO] 2.3.4.10–2.3.4.15):
//! the format Word writes for "Encrypt with Password" and reads without extra software.
//!
//! A random 256-bit key encrypts the package in 4096-byte segments (AES-256-CBC, per-segment
//! IV); the key itself is encrypted with a key derived from the password (SHA-512, 100,000
//! iterations, random salt). An HMAC-SHA512 over the encrypted package detects tampering.
//! The result is a Compound File with the streams Word expects.

use std::io::{Cursor, Write};

use aws_lc_rs::cipher::{EncryptingKey, EncryptionContext, UnboundCipherKey, AES_256};
use aws_lc_rs::digest::{Context, SHA512};
use aws_lc_rs::hmac;
use aws_lc_rs::iv::FixedLength;
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

use crate::ExportError;

/// Shortest password accepted for an exported file (it travels by e-mail).
pub const MIN_PASSWORD_CHARS: usize = 10;
const SPIN_COUNT: u32 = 100_000;
const SEGMENT: usize = 4096;

// Block keys from [MS-OFFCRYPTO] 2.3.4.11–2.3.4.14.
const BK_VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const BK_VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const BK_KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
const BK_HMAC_KEY: [u8; 8] = [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
const BK_HMAC_VALUE: [u8; 8] = [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut ctx = Context::new(&SHA512);
    for p in parts {
        ctx.update(p);
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

fn random<const N: usize>() -> Result<[u8; N], ExportError> {
    let mut out = [0u8; N];
    SystemRandom::new()
        .fill(&mut out)
        .map_err(|_| ExportError::Crypto)?;
    Ok(out)
}

fn iv16(bytes: &[u8]) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&bytes[..16]);
    iv
}

/// AES-256-CBC without padding: `data` is zero-padded to the block size first.
fn aes_cbc(key: &[u8], iv: [u8; 16], data: &[u8]) -> Result<Vec<u8>, ExportError> {
    let mut buf = data.to_vec();
    buf.resize(data.len().div_ceil(16) * 16, 0);
    let unbound = UnboundCipherKey::new(&AES_256, key).map_err(|_| ExportError::Crypto)?;
    let key = EncryptingKey::cbc(unbound).map_err(|_| ExportError::Crypto)?;
    key.less_safe_encrypt(&mut buf, EncryptionContext::Iv128(FixedLength::from(iv)))
        .map_err(|_| ExportError::Crypto)?;
    Ok(buf)
}

/// H(n) from the password: SHA-512 over salt + UTF-16LE password, then 100,000 iterations.
fn password_hash(password: &str, salt: &[u8; 16]) -> Zeroizing<[u8; 64]> {
    let pw: Zeroizing<Vec<u8>> =
        Zeroizing::new(password.encode_utf16().flat_map(u16::to_le_bytes).collect());
    let mut h = Zeroizing::new(sha512(&[salt, &pw]));
    for i in 0..SPIN_COUNT {
        *h = sha512(&[&i.to_le_bytes(), &h[..]]);
    }
    h
}

/// The key for one purpose: SHA-512(H(n) + block key), truncated to 256 bits.
fn derive(h: &[u8; 64], block_key: &[u8; 8]) -> Zeroizing<[u8; 32]> {
    let full = Zeroizing::new(sha512(&[h, block_key]));
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&full[..32]);
    key
}

fn b64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(
                    ALPHABET[usize::try_from((n >> (18 - 6 * i)) & 63).unwrap_or(0)],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Encrypt a DOCX package with a password. Returns the bytes of the protected file.
pub fn encrypt(package: &[u8], password: &str) -> Result<Vec<u8>, ExportError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ExportError::WeakPassword);
    }
    let key_salt: [u8; 16] = random()?;
    let password_salt: [u8; 16] = random()?;
    let secret = Zeroizing::new(random::<32>()?);
    let verifier_input: [u8; 16] = random()?;
    let hmac_key = Zeroizing::new(random::<64>()?);

    // EncryptedPackage: the plain size, then each 4096-byte segment with its own IV.
    let mut encrypted = Vec::with_capacity(package.len() + 32);
    encrypted.extend_from_slice(
        &u64::try_from(package.len())
            .map_err(|_| ExportError::Crypto)?
            .to_le_bytes(),
    );
    for (i, segment) in package.chunks(SEGMENT).enumerate() {
        let index = u32::try_from(i).map_err(|_| ExportError::Crypto)?;
        let iv = iv16(&sha512(&[&key_salt, &index.to_le_bytes()]));
        encrypted.extend(aes_cbc(&secret[..], iv, segment)?);
    }

    // Data integrity.
    let tag = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA512, &hmac_key[..]),
        &encrypted,
    );
    let enc_hmac_key = aes_cbc(
        &secret[..],
        iv16(&sha512(&[&key_salt, &BK_HMAC_KEY])),
        &hmac_key[..],
    )?;
    let enc_hmac_value = aes_cbc(
        &secret[..],
        iv16(&sha512(&[&key_salt, &BK_HMAC_VALUE])),
        tag.as_ref(),
    )?;

    // Password key encryptor.
    let h = password_hash(password, &password_salt);
    let enc_verifier_input = aes_cbc(
        &derive(&h, &BK_VERIFIER_INPUT)[..],
        password_salt,
        &verifier_input,
    )?;
    let enc_verifier_hash = aes_cbc(
        &derive(&h, &BK_VERIFIER_VALUE)[..],
        password_salt,
        &sha512(&[&verifier_input]),
    )?;
    let enc_key_value = aes_cbc(&derive(&h, &BK_KEY_VALUE)[..], password_salt, &secret[..])?;

    let params = r#"saltSize="16" blockSize="16" keyBits="256" hashSize="64" cipherAlgorithm="AES" cipherChaining="ChainingModeCBC" hashAlgorithm="SHA512""#;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n\
         <encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\" \
         xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\" \
         xmlns:c=\"http://schemas.microsoft.com/office/2006/keyEncryptor/certificate\">\
         <keyData {params} saltValue=\"{}\"/>\
         <dataIntegrity encryptedHmacKey=\"{}\" encryptedHmacValue=\"{}\"/>\
         <keyEncryptors><keyEncryptor uri=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\">\
         <p:encryptedKey spinCount=\"{SPIN_COUNT}\" {params} saltValue=\"{}\" \
         encryptedVerifierHashInput=\"{}\" encryptedVerifierHashValue=\"{}\" encryptedKeyValue=\"{}\"/>\
         </keyEncryptor></keyEncryptors></encryption>",
        b64(&key_salt),
        b64(&enc_hmac_key),
        b64(&enc_hmac_value),
        b64(&password_salt),
        b64(&enc_verifier_input),
        b64(&enc_verifier_hash),
        b64(&enc_key_value),
    );
    // EncryptionInfo: version 4.4, flags 0x40 (agile), then the XML descriptor.
    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(xml.as_bytes());

    container(&info, &encrypted)
}

/// UNICODE-LP-P4: byte length, UTF-16LE, padded to 4 bytes.
fn lp_p4(s: &str) -> Vec<u8> {
    let utf16: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = u32::try_from(utf16.len())
        .unwrap_or(0)
        .to_le_bytes()
        .to_vec();
    out.extend(&utf16);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

fn u32le(n: u32) -> [u8; 4] {
    n.to_le_bytes()
}

/// The \x06DataSpaces structures ([MS-OFFCRYPTO] 2.2) that mark the package as encrypted.
fn data_spaces() -> Vec<(&'static str, Vec<u8>)> {
    let versions = [1u8, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]; // reader, updater, writer 1.0
    let mut version = lp_p4("Microsoft.Container.DataSpaces");
    version.extend(versions);

    let mut entry = Vec::new();
    entry.extend(u32le(1)); // reference component count
    entry.extend(u32le(0)); // stream
    entry.extend(lp_p4("EncryptedPackage"));
    entry.extend(lp_p4("StrongEncryptionDataSpace"));
    let mut map = Vec::new();
    map.extend(u32le(8)); // header length
    map.extend(u32le(1)); // entry count
    map.extend(u32le(u32::try_from(entry.len() + 4).unwrap_or(0)));
    map.extend(entry);

    let mut info = Vec::new();
    info.extend(u32le(8));
    info.extend(u32le(1));
    info.extend(lp_p4("StrongEncryptionTransform"));

    let id = lp_p4("{FF9A3F03-56EF-4613-BDD5-5A41C1D07246}");
    let mut primary = Vec::new();
    primary.extend(u32le(u32::try_from(8 + id.len()).unwrap_or(0))); // bytes before the name
    primary.extend(u32le(1)); // transform type
    primary.extend(id);
    primary.extend(lp_p4("Microsoft.Container.EncryptionTransform"));
    primary.extend(versions);
    primary.extend(u32le(0)); // encryption name (empty UTF-8-LP-P4)
    primary.extend(u32le(0)); // block size
    primary.extend(u32le(0)); // cipher mode
    primary.extend(u32le(4)); // reserved

    vec![
        ("/\u{6}DataSpaces/Version", version),
        ("/\u{6}DataSpaces/DataSpaceMap", map),
        (
            "/\u{6}DataSpaces/DataSpaceInfo/StrongEncryptionDataSpace",
            info,
        ),
        (
            "/\u{6}DataSpaces/TransformInfo/StrongEncryptionTransform/\u{6}Primary",
            primary,
        ),
    ]
}

fn container(info: &[u8], package: &[u8]) -> Result<Vec<u8>, ExportError> {
    let io = |e: std::io::Error| ExportError::Package(e.to_string());
    let mut cfb = cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))
        .map_err(io)?;
    for storage in [
        "/\u{6}DataSpaces",
        "/\u{6}DataSpaces/DataSpaceInfo",
        "/\u{6}DataSpaces/TransformInfo",
        "/\u{6}DataSpaces/TransformInfo/StrongEncryptionTransform",
    ] {
        cfb.create_storage(storage).map_err(io)?;
    }
    let mut streams = data_spaces();
    streams.push(("/EncryptionInfo", info.to_vec()));
    streams.push(("/EncryptedPackage", package.to_vec()));
    for (path, data) in streams {
        let mut s = cfb.create_stream(path).map_err(io)?;
        s.write_all(&data).map_err(io)?;
    }
    cfb.flush().map_err(io)?;
    Ok(cfb.into_inner().into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn short_password_is_refused() {
        assert_eq!(encrypt(b"x", "short"), Err(ExportError::WeakPassword));
    }

    #[test]
    fn produces_a_compound_file_without_plain_text() {
        let package = crate::render(&crate::tests::sample()).unwrap_or_else(|e| panic!("{e}"));
        let out = encrypt(&package, "סיסמה ארוכה מספיק").unwrap_or_else(|e| panic!("{e}"));
        assert!(out.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]));
        let hay = String::from_utf8_lossy(&out);
        assert!(hay.contains("keyEncryptor/password"));
        assert!(
            !out.windows(4).any(|w| w == b"PK\x03\x04"),
            "no zip inside in the clear"
        );
        // Two exports never look alike (random key and salts).
        let again = encrypt(&package, "סיסמה ארוכה מספיק").unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(out, again);
    }
}
