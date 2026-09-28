//! Password slot: policy (D-012) and Argon2id key derivation (RFC 9106).

use std::time::Instant;

use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::crypto::{hkdf, Key32};
use crate::VaultError;

pub const MIN_PASSWORD_CHARS: usize = 12;

/// Argon2id cost parameters, stored in the vault header next to the salt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon2Params {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl Argon2Params {
    /// Floor for production vaults: 256 MiB, 3 passes, 4 lanes.
    pub const FLOOR: Self = Self {
        m_kib: 256 * 1024,
        t: 3,
        p: 4,
    };
    /// Cheap parameters for unit tests only.
    pub const TEST: Self = Self {
        m_kib: 64,
        t: 1,
        p: 1,
    };

    /// Pick the strongest memory cost (1 GiB → 512 MiB → 256 MiB) this machine handles in
    /// about a second, never going below [`Self::FLOOR`].
    #[must_use]
    pub fn calibrate() -> Self {
        let probe = Self::FLOOR;
        let start = Instant::now();
        let ok = derive_raw(b"calibration", &[0u8; 32], probe).is_ok();
        let secs = start.elapsed().as_secs_f64();
        if !ok || secs > 0.5 {
            return probe;
        }
        if secs < 0.25 {
            Self {
                m_kib: 1024 * 1024,
                ..probe
            }
        } else {
            Self {
                m_kib: 512 * 1024,
                ..probe
            }
        }
    }
}

fn derive_raw(
    password: &[u8],
    salt: &[u8; 32],
    params: Argon2Params,
) -> Result<Zeroizing<[u8; 32]>, VaultError> {
    let p = Params::new(params.m_kib, params.t, params.p, Some(32))
        .map_err(|_| VaultError::Crypto("argon2 params"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password, salt, out.as_mut())
        .map_err(|_| VaultError::Crypto("argon2"))?;
    Ok(out)
}

/// NFKC-normalize (NIST 800-63B §3.1.1.2) and trim surrounding whitespace.
#[must_use]
pub fn normalize(password: &str) -> Zeroizing<String> {
    Zeroizing::new(password.trim().nfkc().collect())
}

/// Derive the password slot's key-encryption key.
pub fn derive_kek(
    password: &str,
    salt: &[u8; 32],
    params: Argon2Params,
) -> Result<Key32, VaultError> {
    let normalized = normalize(password);
    let stretched = derive_raw(normalized.as_bytes(), salt, params)?;
    hkdf(stretched.as_ref(), "dv/slot/password/v1")
}

/// Very common passwords and keyboard walks (Latin and Hebrew layouts).
const BLOCKLIST: &[&str] = &[
    "123456789012",
    "1234567890123",
    "123456123456",
    "qwertyuiop12",
    "qwertyuiopas",
    "password1234",
    "passwordpassword",
    "1q2w3e4r5t6y",
    "asdfghjkl123",
    "iloveyou1234",
    "123123123123",
    "000000000000",
    "111111111111",
    "abcdefghijkl",
    "abc123abc123",
    "zxcvbnm12345",
    "qazwsxedcrfv",
    "welcome12345",
    "adminadmin12",
    "letmein12345",
    "/'קראטוןםפ",
    "שדגכעיחלךף",
    "זסבהנמצתץ",
    "אבגדהוזחטיכל",
];

/// Why a new password was refused (shown to the user in Hebrew by the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyViolation {
    TooShort,
    Common,
    Repetitive,
}

/// NIST-style checks: length and blocklist, no composition rules.
pub fn check_policy(password: &str) -> Result<(), PolicyViolation> {
    let normalized = normalize(password);
    let chars: Vec<char> = normalized.chars().collect();
    if chars.len() < MIN_PASSWORD_CHARS {
        return Err(PolicyViolation::TooShort);
    }
    let lower = normalized.to_lowercase();
    if BLOCKLIST.iter().any(|b| lower.contains(b)) {
        return Err(PolicyViolation::Common);
    }
    let mut distinct = chars.clone();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() < 4 {
        return Err(PolicyViolation::Repetitive);
    }
    let sequential = chars
        .windows(2)
        .filter(|w| (w[1] as i64 - w[0] as i64).abs() == 1)
        .count();
    if sequential + 1 == chars.len() {
        return Err(PolicyViolation::Repetitive);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_accepts_passphrases_and_rejects_weak_passwords() {
        assert!(check_policy("כלב ירוק רץ מהר בגינה").is_ok());
        assert!(check_policy("correct horse battery").is_ok());
        assert_eq!(check_policy("short1"), Err(PolicyViolation::TooShort));
        assert_eq!(
            check_policy("my123456789012x"),
            Err(PolicyViolation::Common)
        );
        assert_eq!(
            check_policy("aaaaaaaaaaaaaa"),
            Err(PolicyViolation::Repetitive)
        );
        assert_eq!(check_policy("abcdefghijklmn"), Err(PolicyViolation::Common));
        assert_eq!(
            check_policy("bcdefghijklmno"),
            Err(PolicyViolation::Repetitive)
        );
    }

    #[test]
    fn derivation_is_deterministic_salted_and_normalized() {
        let salt = [1u8; 32];
        let a = derive_kek("סיסמה ארוכה מאוד", &salt, Argon2Params::TEST).unwrap();
        let b = derive_kek("  סיסמה ארוכה מאוד ", &salt, Argon2Params::TEST).unwrap();
        let c = derive_kek("סיסמה ארוכה מאוד", &[2u8; 32], Argon2Params::TEST).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
