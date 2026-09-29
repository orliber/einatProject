//! Cryptographic primitives. Everything here runs on AWS-LC (`aws-lc-rs`), which is the
//! FIPS 140-3 validated module when the `fips` feature is on.

use std::fmt;

use aws_lc_rs::{aead, digest, hkdf, hmac, rand};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::VaultError;

/// Envelope format version for sealed values.
const SEAL_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// A 256-bit secret that is wiped from memory when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct Key32([u8; 32]);

impl Key32 {
    pub fn random() -> Result<Self, VaultError> {
        Ok(Self(random_array()?))
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Key32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key32(<redacted>)")
    }
}

/// True when the crypto module is running in FIPS mode.
#[must_use]
pub fn fips_active() -> bool {
    aws_lc_rs::try_fips_mode().is_ok()
}

pub fn random_array<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut out = [0u8; N];
    rand::fill(&mut out).map_err(|_| VaultError::Crypto("random generator failed"))?;
    Ok(out)
}

/// 128-bit random identifier, hex encoded.
pub fn random_id() -> Result<String, VaultError> {
    Ok(hex(&random_array::<16>()?))
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    s
}

pub fn unhex(s: &str) -> Result<Vec<u8>, VaultError> {
    if s.len() % 2 != 0 {
        return Err(VaultError::Corrupt("odd-length hex".to_owned()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            s.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| VaultError::Corrupt("invalid hex".to_owned()))
        })
        .collect()
}

struct Len(usize);

impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

/// HKDF-SHA256 (SP 800-56C) with an empty salt and a purpose string as `info`.
pub fn hkdf(ikm: &[u8], info: &str) -> Result<Key32, VaultError> {
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, &[]).extract(ikm);
    let info = [info.as_bytes()];
    let okm = prk
        .expand(&info, Len(32))
        .map_err(|_| VaultError::Crypto("hkdf expand"))?;
    let mut out = [0u8; 32];
    okm.fill(&mut out)
        .map_err(|_| VaultError::Crypto("hkdf fill"))?;
    let key = Key32::from_bytes(out);
    out.zeroize();
    Ok(key)
}

fn aead_key(key: &Key32) -> Result<aead::LessSafeKey, VaultError> {
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key.as_bytes())
        .map_err(|_| VaultError::Crypto("aead key"))?;
    Ok(aead::LessSafeKey::new(unbound))
}

/// AES-256-GCM with a random 96-bit nonce. Output: `version ‖ nonce ‖ ciphertext ‖ tag`.
///
/// `aad` binds the value to where it is stored, so it cannot be moved to another row or case.
pub fn seal(key: &Key32, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let nonce_bytes = random_array::<NONCE_LEN>()?;
    let mut buf = Vec::with_capacity(1 + NONCE_LEN + plaintext.len() + TAG_LEN);
    buf.push(SEAL_VERSION);
    buf.extend_from_slice(&nonce_bytes);
    let mut body = plaintext.to_vec();
    aead_key(key)?
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce_bytes),
            aead::Aad::from(aad),
            &mut body,
        )
        .map_err(|_| VaultError::Crypto("seal"))?;
    buf.extend_from_slice(&body);
    body.zeroize();
    Ok(buf)
}

/// Inverse of [`seal`]. Fails if the key, the AAD or any byte is wrong.
pub fn open(key: &Key32, aad: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    if sealed.len() < 1 + NONCE_LEN + TAG_LEN || sealed[0] != SEAL_VERSION {
        return Err(VaultError::Corrupt(
            "sealed value has an unknown format".to_owned(),
        ));
    }
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&sealed[1..=NONCE_LEN]);
    let mut body = Zeroizing::new(sealed[1 + NONCE_LEN..].to_vec());
    let len = aead_key(key)?
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(aad),
            &mut body,
        )
        .map_err(|_| VaultError::Authentication)?
        .len();
    body.truncate(len);
    Ok(body)
}

pub fn seal_str(key: &Key32, aad: &[u8], text: &str) -> Result<Vec<u8>, VaultError> {
    seal(key, aad, text.as_bytes())
}

pub fn open_string(key: &Key32, aad: &[u8], sealed: &[u8]) -> Result<String, VaultError> {
    let bytes = open(key, aad, sealed)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| VaultError::Corrupt("not UTF-8".to_owned()))
}

#[must_use]
pub fn hmac_sha256(key: &Key32, data: &[u8]) -> [u8; 32] {
    let k = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
    let tag = hmac::sign(&k, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(tag.as_ref());
    out
}

/// Constant-time HMAC verification.
#[must_use]
pub fn hmac_verify(key: &Key32, data: &[u8], tag: &[u8]) -> bool {
    let k = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
    hmac::verify(&k, data, tag).is_ok()
}

#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, data).as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_round_trips() {
        let key = Key32::random().unwrap();
        let sealed = seal(&key, b"cases|meta|1", "תוכן רגיש".as_bytes()).unwrap();
        assert_eq!(
            open_string(&key, b"cases|meta|1", &sealed).unwrap(),
            "תוכן רגיש"
        );
    }

    #[test]
    fn wrong_aad_wrong_key_or_tampering_fail() {
        let key = Key32::random().unwrap();
        let sealed = seal(&key, b"row-1", b"secret").unwrap();
        assert!(matches!(
            open(&key, b"row-2", &sealed),
            Err(VaultError::Authentication)
        ));
        assert!(open(&Key32::random().unwrap(), b"row-1", &sealed).is_err());
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open(&key, b"row-1", &tampered).is_err());
    }

    #[test]
    fn nonces_are_unique_per_seal() {
        let key = Key32::random().unwrap();
        let a = seal(&key, b"", b"x").unwrap();
        let b = seal(&key, b"", b"x").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn hkdf_separates_purposes() {
        let ikm = [7u8; 32];
        assert_ne!(hkdf(&ikm, "a").unwrap(), hkdf(&ikm, "b").unwrap());
        assert_eq!(hkdf(&ikm, "a").unwrap(), hkdf(&ikm, "a").unwrap());
    }

    #[test]
    fn hmac_verifies_in_constant_time() {
        let key = Key32::random().unwrap();
        let tag = hmac_sha256(&key, b"data");
        assert!(hmac_verify(&key, b"data", &tag));
        assert!(!hmac_verify(&key, b"datA", &tag));
    }

    #[cfg(feature = "fips")]
    #[test]
    fn fips_build_runs_in_fips_mode() {
        assert!(
            fips_active(),
            "built with `fips` but the module is not in FIPS mode"
        );
    }

    #[test]
    fn hex_round_trips_and_debug_is_redacted() {
        assert_eq!(unhex(&hex(&[0, 255, 16])).unwrap(), vec![0, 255, 16]);
        assert_eq!(
            format!("{:?}", Key32::from_bytes([1; 32])),
            "Key32(<redacted>)"
        );
    }
}
