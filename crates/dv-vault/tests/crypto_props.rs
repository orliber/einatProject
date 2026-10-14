//! Property tests of the sealing primitives, the recovery-key encoding and the password KDF.
//! Every case value in the vault goes through `seal`/`open`, so these are the properties the
//! rest of the vault stands on. Fabricated data only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashSet;

use dv_vault::crypto::{hkdf, open, seal, Key32};
use dv_vault::password::{check_policy, derive_kek};
use dv_vault::recovery;
use dv_vault::{Argon2Params, PolicyViolation, VaultError, MIN_PASSWORD_CHARS};
use proptest::prelude::*;

/// `version ‖ nonce ‖ ciphertext ‖ tag`
const HEADER_LEN: usize = 1 + 12;
const TAG_LEN: usize = 16;

fn key(seed: u8) -> Key32 {
    Key32::from_bytes([seed; 32])
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn any_payload_round_trips_under_its_own_aad(
        k in any::<[u8; 32]>(),
        aad in proptest::collection::vec(any::<u8>(), 0..64),
        plain in proptest::collection::vec(any::<u8>(), 0..2048),
    ) {
        let key = Key32::from_bytes(k);
        let sealed = seal(&key, &aad, &plain).unwrap();
        prop_assert_eq!(sealed.len(), HEADER_LEN + plain.len() + TAG_LEN);
        prop_assert_eq!(sealed[0], 1, "envelope version");
        let opened = open(&key, &aad, &sealed).unwrap();
        prop_assert_eq!(opened.as_slice(), plain.as_slice());
    }

    /// AES-GCM is authenticated: one flipped bit anywhere (version, nonce, ciphertext, tag)
    /// and the value does not open. Nothing partial is ever returned.
    #[test]
    fn any_single_bit_flip_is_rejected(
        plain in proptest::collection::vec(any::<u8>(), 0..256),
        pick in any::<prop::sample::Index>(),
        bit in 0u8..8,
    ) {
        let key = key(7);
        let sealed = seal(&key, b"dv/v1|inputs|content|row|case", &plain).unwrap();
        let mut tampered = sealed.clone();
        let at = pick.index(tampered.len());
        tampered[at] ^= 1 << bit;
        let result = open(&key, b"dv/v1|inputs|content|row|case", &tampered);
        if at == 0 {
            prop_assert!(matches!(result, Err(VaultError::Corrupt(_))), "version byte");
        } else {
            prop_assert!(matches!(result, Err(VaultError::Authentication)), "byte {at}");
        }
    }

    /// The AAD is what binds a value to its table, column, row and case: any other AAD,
    /// even one bit or one byte longer or shorter, is refused.
    #[test]
    fn any_other_aad_is_rejected(
        aad in proptest::collection::vec(any::<u8>(), 0..48),
        other in proptest::collection::vec(any::<u8>(), 0..48),
        plain in proptest::collection::vec(any::<u8>(), 0..128),
    ) {
        prop_assume!(aad != other);
        let key = key(9);
        let sealed = seal(&key, &aad, &plain).unwrap();
        prop_assert!(matches!(open(&key, &other, &sealed), Err(VaultError::Authentication)));
    }

    #[test]
    fn any_other_key_is_rejected(
        a in any::<[u8; 32]>(),
        b in any::<[u8; 32]>(),
        plain in proptest::collection::vec(any::<u8>(), 0..128),
    ) {
        prop_assume!(a != b);
        let sealed = seal(&Key32::from_bytes(a), b"aad", &plain).unwrap();
        prop_assert!(matches!(
            open(&Key32::from_bytes(b), b"aad", &sealed),
            Err(VaultError::Authentication)
        ));
    }

    /// Arbitrary bytes handed to `open` (a corrupt row) never panic and never open.
    #[test]
    fn arbitrary_bytes_never_open(junk in proptest::collection::vec(any::<u8>(), 0..96)) {
        prop_assert!(open(&key(1), b"aad", &junk).is_err());
    }

    /// The printed recovery key decodes to exactly the key it was printed from, whatever
    /// spacing and letter case she types it with.
    #[test]
    fn recovery_key_round_trips_however_it_is_typed(
        k in any::<[u8; 32]>(),
        lower in any::<bool>(),
        spaced in any::<bool>(),
    ) {
        let key = Key32::from_bytes(k);
        let mut typed = recovery::format(&key).to_string();
        if lower {
            typed = typed.to_lowercase();
        }
        if spaced {
            typed = typed.replace('-', "  ");
        }
        prop_assert_eq!(recovery::parse(&typed).unwrap(), key);
    }

    /// The last group is a checksum of the key: changing any character of it to another
    /// symbol (not a lookalike) is always caught, it never opens a different key.
    #[test]
    fn a_changed_checksum_character_is_always_caught(
        k in any::<[u8; 32]>(),
        pos in 0usize..4,
        sym in 0usize..32,
    ) {
        const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let printed = recovery::format(&Key32::from_bytes(k)).to_string();
        let mut chars: Vec<char> = printed.chars().collect();
        let at = chars.len() - 4 + pos;
        let replacement = char::from(ALPHABET[sym]);
        prop_assume!(chars[at] != replacement);
        chars[at] = replacement;
        let typo: String = chars.into_iter().collect();
        prop_assert!(matches!(recovery::parse(&typo), Err(VaultError::RecoveryKeyInvalid)));
    }

    /// Anything shorter than the minimum, once normalized, is refused as too short.
    #[test]
    fn short_passwords_are_always_too_short(p in "\\PC{0,11}") {
        let normalized = dv_vault::password::normalize(&p);
        prop_assume!(normalized.chars().count() < MIN_PASSWORD_CHARS);
        prop_assert_eq!(check_policy(&p), Err(PolicyViolation::TooShort));
    }
}

/// Nonces are random 96-bit values: across many seals (from several threads at once, as the
/// app seals from the UI and background tasks) none repeats, so GCM's key-nonce pair is never
/// reused even for identical plaintexts.
#[test]
fn nonces_never_repeat_across_many_seals_and_threads() {
    let key = key(3);
    let nonces: Vec<[u8; 12]> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    (0..5_000)
                        .map(|_| {
                            let sealed = seal(&key, b"same aad", b"same plaintext").unwrap();
                            sealed[1..HEADER_LEN].try_into().unwrap()
                        })
                        .collect::<Vec<[u8; 12]>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    let distinct: HashSet<[u8; 12]> = nonces.iter().copied().collect();
    assert_eq!(distinct.len(), nonces.len(), "a nonce repeated");
}

/// Every truncation of a sealed value (a short write, a cut-off row) is refused.
#[test]
fn every_truncation_is_rejected() {
    let key = key(5);
    let sealed = seal(&key, b"aad", "טקסט בדוי לבדיקה".as_bytes()).unwrap();
    for len in 0..sealed.len() {
        assert!(
            open(&key, b"aad", &sealed[..len]).is_err(),
            "opened when cut to {len} bytes"
        );
    }
    let mut longer = sealed.clone();
    longer.push(0);
    assert!(open(&key, b"aad", &longer).is_err(), "trailing byte");
}

/// An empty plaintext is still authenticated (a sealed empty string is not "nothing").
#[test]
fn an_empty_value_is_sealed_and_authenticated() {
    let key = key(6);
    let sealed = seal(&key, b"aad", b"").unwrap();
    assert_eq!(sealed.len(), HEADER_LEN + TAG_LEN);
    assert!(open(&key, b"aad", &sealed).unwrap().is_empty());
    assert!(open(&key, b"aaD", &sealed).is_err());
}

/// The same purpose string never yields the same subkey from different master keys, and
/// purposes that differ only slightly (a version bump) are separated.
#[test]
fn hkdf_separates_keys_and_close_purposes() {
    let a = hkdf(&[1u8; 32], "dv/backup/v1").unwrap();
    assert_ne!(a, hkdf(&[2u8; 32], "dv/backup/v1").unwrap());
    assert_ne!(a, hkdf(&[1u8; 32], "dv/backup/v2").unwrap());
    assert_ne!(a, hkdf(&[1u8; 32], "dv/backup/v1 ").unwrap());
}

/// NFKC: the same password typed with composed or decomposed characters, or with full-width
/// digits from another keyboard, derives the same key; a genuinely different one does not.
#[test]
fn password_kdf_is_unicode_normalized() {
    let salt = [4u8; 32];
    let p = Argon2Params::TEST;
    let composed = derive_kek("caf\u{e9} au lait 2024", &salt, p).unwrap();
    let decomposed = derive_kek("cafe\u{301} au lait 2024", &salt, p).unwrap();
    let fullwidth = derive_kek(
        "caf\u{e9} au lait \u{ff12}\u{ff10}\u{ff12}\u{ff14}",
        &salt,
        p,
    )
    .unwrap();
    assert_eq!(composed, decomposed);
    assert_eq!(composed, fullwidth);
    assert_ne!(
        composed,
        derive_kek("cafe au lait 2024", &salt, p).unwrap(),
        "an accent is part of the password"
    );
    // Inner spaces are kept (only the ends are trimmed).
    assert_ne!(
        composed,
        derive_kek("caf\u{e9}  au lait 2024", &salt, p).unwrap()
    );
}

/// Debug output of a key and of a secret never shows the bytes or the text.
#[test]
fn keys_and_secrets_never_print_their_contents() {
    let printed = format!("{:?}", Key32::from_bytes([0xAB; 32]));
    assert!(!printed.to_lowercase().contains("ab"), "{printed}");
    let pw = "סיסמה בדויה ארוכה";
    let secret = format!(
        "{:?} {:?}",
        dv_vault::Secret::Password(pw),
        dv_vault::Secret::Recovery("AAAA-BBBB")
    );
    assert!(!secret.contains(pw) && !secret.contains("AAAA"), "{secret}");
}
