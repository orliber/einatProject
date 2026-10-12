//! The update check (D-033) against a fake release server: only a notice signed with the
//! pinned key, for this product, for a strictly newer version, with an installer on the
//! release address whose size and SHA-256 match, is ever offered or accepted.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::collections::HashMap;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
use dv_egress::update::{
    check, download, parse_key, parse_version, pinned_key, sha256, signed_bytes, to_hex, Fetch,
    UpdateError, UpdateInfo, PRODUCT, UPDATE_HOSTS,
};
use proptest::prelude::*;
use serde_json::{json, Value};

const INSTALLER: &[u8] = b"MZ invented installer bytes for tests";
const INSTALLER_URL: &str = "https://github.com/orliber/einat-vault-releases/releases/download/v9.0.0/DiagnosticVault-Setup.exe";

/// A fake release server: the notice, its signature and the installer, by address suffix.
/// It records every address asked for, so a test can show nothing was fetched.
struct Server {
    notice: Option<Vec<u8>>,
    signature: Option<Vec<u8>>,
    files: HashMap<String, Vec<u8>>,
    asked: RefCell<Vec<String>>,
}

impl Fetch for Server {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, UpdateError> {
        self.asked.borrow_mut().push(url.to_owned());
        let body = if url.ends_with("/latest.json") {
            self.notice.clone()
        } else if url.ends_with("/latest.json.sig") {
            self.signature.clone()
        } else {
            self.files.get(url).cloned()
        }
        .ok_or(UpdateError::Server(404))?;
        if body.len() as u64 > max {
            return Err(UpdateError::TooLarge);
        }
        Ok(body)
    }
}

fn keypair() -> Ed25519KeyPair {
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap()
}

fn public(k: &Ed25519KeyPair) -> [u8; 32] {
    k.public_key().as_ref().try_into().unwrap()
}

fn notice_value(version: &str) -> Value {
    json!({
        "product": PRODUCT,
        "version": version,
        "published": "2026-10-01T10:00:00Z",
        "notes": ["תיקון קטן"],
        "installer": {"url": INSTALLER_URL, "sha256": to_hex(&sha256(INSTALLER)), "size": INSTALLER.len()}
    })
}

fn signed(k: &Ed25519KeyPair, notice: &Value, installer: &[u8]) -> Server {
    let bytes = serde_json::to_vec(notice).unwrap();
    let sig = to_hex(k.sign(&signed_bytes(&bytes)).as_ref());
    Server {
        notice: Some(bytes),
        signature: Some(sig.into_bytes()),
        files: HashMap::from([(INSTALLER_URL.to_owned(), installer.to_vec())]),
        asked: RefCell::new(Vec::new()),
    }
}

#[test]
fn the_pinned_key_is_present_and_a_notice_from_any_other_key_fails_against_it() {
    let pinned = pinned_key().expect("this build pins an update key");
    let t = signed(&keypair(), &notice_value("9.0.0"), INSTALLER);
    assert_eq!(
        check(&t, "0.1.0", &[pinned]),
        Err(UpdateError::BadSignature)
    );
}

#[test]
fn an_unsigned_notice_is_refused() {
    let k = keypair();
    let mut t = signed(&k, &notice_value("9.0.0"), INSTALLER);
    t.signature = None;
    assert_eq!(
        check(&t, "0.1.0", &[public(&k)]),
        Err(UpdateError::Server(404))
    );
    for sig in [
        "".to_owned(),
        "not hex at all".to_owned(),
        "abc".to_owned(),
        "00".repeat(64),
        "ab".repeat(63),
        "שלום".to_owned(),
    ] {
        t.signature = Some(sig.clone().into_bytes());
        assert_eq!(
            check(&t, "0.1.0", &[public(&k)]),
            Err(UpdateError::BadSignature),
            "{sig:?}"
        );
    }
}

#[test]
fn any_changed_bit_of_the_notice_or_the_signature_is_refused() {
    let k = keypair();
    let good = signed(&k, &notice_value("9.0.0"), INSTALLER);
    let notice = good.notice.clone().unwrap();
    let sig = String::from_utf8(good.signature.clone().unwrap()).unwrap();
    for i in (0..notice.len()).step_by(7) {
        let mut t = signed(&k, &notice_value("9.0.0"), INSTALLER);
        let mut n = notice.clone();
        n[i] ^= 0x01;
        t.notice = Some(n);
        assert_eq!(
            check(&t, "0.1.0", &[public(&k)]),
            Err(UpdateError::BadSignature)
        );
    }
    for i in 0..sig.len() {
        let mut t = signed(&k, &notice_value("9.0.0"), INSTALLER);
        let mut s = sig.clone().into_bytes();
        s[i] = if s[i] == b'0' { b'1' } else { b'0' };
        t.signature = Some(s);
        assert_eq!(
            check(&t, "0.1.0", &[public(&k)]),
            Err(UpdateError::BadSignature)
        );
    }
    // Surrounding whitespace in the signature file is fine (a trailing newline is usual).
    let mut t = signed(&k, &notice_value("9.0.0"), INSTALLER);
    t.signature = Some(format!("  {sig}\n").into_bytes());
    assert!(matches!(check(&t, "0.1.0", &[public(&k)]), Ok(Some(_))));
}

#[test]
fn a_signed_notice_with_a_broken_field_is_refused() {
    let k = keypair();
    type Change = Box<dyn Fn(&mut Value)>;
    let broken: Vec<(&str, Change)> = vec![
        ("version", Box::new(|n| n["version"] = json!("9.0"))),
        ("version", Box::new(|n| n["version"] = json!("9.0.0-beta"))),
        ("version", Box::new(|n| n["version"] = json!(9))),
        ("product", Box::new(|n| n["product"] = Value::Null)),
        (
            "url",
            Box::new(|n| {
                n["installer"]["url"] = json!(
                    "http://github.com/orliber/einat-vault-releases/releases/download/v9/x.exe"
                )
            }),
        ),
        (
            "url",
            Box::new(|n| {
                n["installer"]["url"] = json!("https://github.com/orliber/einat-vault-releases/releases/download/../../../evil/x.exe")
            }),
        ),
        (
            "url",
            Box::new(|n| {
                n["installer"]["url"] =
                    json!("https://github.com/orliber/other-repo/releases/download/v9/x.exe")
            }),
        ),
        (
            "sha256",
            Box::new(|n| n["installer"]["sha256"] = json!("ab".repeat(31))),
        ),
        (
            "sha256",
            Box::new(|n| n["installer"]["sha256"] = json!("zz".repeat(32))),
        ),
        ("size", Box::new(|n| n["installer"]["size"] = json!(0))),
        (
            "size",
            Box::new(|n| n["installer"]["size"] = json!(401_u64 * 1024 * 1024)),
        ),
        ("size", Box::new(|n| n["installer"]["size"] = json!(-1))),
    ];
    for (what, change) in broken {
        let mut n = notice_value("9.0.0");
        change(&mut n);
        let t = signed(&k, &n, INSTALLER);
        assert_eq!(
            check(&t, "0.1.0", &[public(&k)]),
            Err(UpdateError::BadNotice(what.to_owned())),
            "{n}"
        );
    }
    let mut t = signed(&k, &notice_value("9.0.0"), INSTALLER);
    let raw = b"not json".to_vec();
    t.signature = Some(to_hex(k.sign(&signed_bytes(&raw)).as_ref()).into_bytes());
    t.notice = Some(raw);
    assert_eq!(
        check(&t, "0.1.0", &[public(&k)]),
        Err(UpdateError::BadNotice("json".to_owned()))
    );
}

#[test]
fn a_current_version_that_does_not_parse_offers_nothing() {
    let k = keypair();
    let t = signed(&k, &notice_value("9.0.0"), INSTALLER);
    assert!(matches!(
        check(&t, "dev", &[public(&k)]),
        Err(UpdateError::BadNotice(_))
    ));
}

#[test]
fn a_notice_larger_than_the_limit_is_not_read() {
    let k = keypair();
    let mut n = notice_value("9.0.0");
    n["padding"] = json!("x".repeat(64 * 1024));
    let t = signed(&k, &n, INSTALLER);
    assert_eq!(
        check(&t, "0.1.0", &[public(&k)]),
        Err(UpdateError::TooLarge)
    );
}

#[test]
fn notes_are_capped_in_number_and_length() {
    let k = keypair();
    let mut n = notice_value("9.0.0");
    n["notes"] = json!((0..30).map(|_| "א".repeat(400)).collect::<Vec<_>>());
    let t = signed(&k, &n, INSTALLER);
    let info = check(&t, "0.1.0", &[public(&k)]).unwrap().unwrap();
    assert_eq!(info.notes.len(), 20);
    assert!(info.notes.iter().all(|l| l.chars().count() == 300));
    assert_eq!(info.published, "2026-10-01");
}

#[test]
fn an_installer_off_the_release_address_is_refused_before_it_is_fetched() {
    let k = keypair();
    let t = signed(&k, &notice_value("9.0.0"), INSTALLER);
    let info = UpdateInfo {
        version: "9.0.0".to_owned(),
        published: String::new(),
        notes: vec![],
        installer_url: "https://objects.githubusercontent.com/evil/Setup.exe".to_owned(),
        sha256: sha256(INSTALLER),
        size: INSTALLER.len() as u64,
    };
    assert!(matches!(
        download(&t, &info),
        Err(UpdateError::HostRefused(_))
    ));
    assert!(t.asked.borrow().is_empty(), "nothing was fetched");
}

#[test]
fn an_installer_of_another_size_or_hash_is_refused() {
    let k = keypair();
    let t = signed(&k, &notice_value("9.0.0"), INSTALLER);
    let info = check(&t, "0.1.0", &[public(&k)]).unwrap().unwrap();
    // Longer than signed: not even read past the signed size.
    let mut longer = signed(&k, &notice_value("9.0.0"), INSTALLER);
    longer.files.insert(
        INSTALLER_URL.to_owned(),
        [INSTALLER, b"extra".as_slice()].concat(),
    );
    assert_eq!(download(&longer, &info), Err(UpdateError::TooLarge));
    // Shorter than signed.
    let mut shorter = signed(&k, &notice_value("9.0.0"), INSTALLER);
    shorter
        .files
        .insert(INSTALLER_URL.to_owned(), INSTALLER[1..].to_vec());
    assert_eq!(download(&shorter, &info), Err(UpdateError::Tampered));
    // Same size, one byte changed.
    let mut changed = INSTALLER.to_vec();
    changed[5] ^= 0x20;
    let t = signed(&k, &notice_value("9.0.0"), &changed);
    assert_eq!(download(&t, &info), Err(UpdateError::Tampered));
}

#[test]
fn update_hosts_are_github_only() {
    for h in UPDATE_HOSTS {
        assert!(
            *h == "github.com" || h.ends_with(".githubusercontent.com"),
            "{h}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// A validly signed notice is offered exactly when its version is strictly newer:
    /// never a downgrade, never the same version again.
    #[test]
    fn only_a_strictly_newer_version_is_offered(
        new in (0u32..4, 0u32..12, 0u32..12),
        now in (0u32..4, 0u32..12, 0u32..12),
    ) {
        let k = keypair();
        let v = |(a, b, c): (u32, u32, u32)| format!("{a}.{b}.{c}");
        let t = signed(&k, &notice_value(&v(new)), INSTALLER);
        let got = check(&t, &v(now), &[public(&k)]).unwrap();
        prop_assert_eq!(got.is_some(), new > now, "{} vs {}", v(new), v(now));
    }

    /// Version parsing takes any text without panicking and accepts only the strict form.
    #[test]
    fn version_parsing_is_strict_and_total(text in "\\PC{0,40}") {
        if let Some((a, b, c)) = parse_version(&text) {
            prop_assert_eq!(parse_version(&format!("{a}.{b}.{c}")), Some((a, b, c)));
            prop_assert_eq!(text.trim().trim_start_matches('v').matches('.').count(), 2);
        }
    }

    /// A key file holds exactly 64 hex digits on its first line that is not a comment.
    #[test]
    fn key_files_parse_only_with_exactly_32_bytes(hex in "[0-9a-fA-F]{0,70}", junk in "\\PC{0,20}") {
        let key = parse_key(&format!("# public key\n\n  {hex}  \n# {junk}"));
        prop_assert_eq!(key.is_some(), hex.len() == 64);
        if let Some(k) = key {
            prop_assert_eq!(to_hex(&k), hex.to_lowercase());
        }
        // A line that is neither a key nor a comment makes the whole file pin nothing.
        if !junk.trim().is_empty() && !junk.trim().starts_with('#') {
            let file = format!("{hex}\n{junk}");
            prop_assert!(parse_key(&file).is_none());
        }
    }

    #[test]
    fn version_round_trips(a in any::<u32>(), b in any::<u32>(), c in any::<u32>()) {
        prop_assert_eq!(parse_version(&format!("{a}.{b}.{c}")), Some((a, b, c)));
        prop_assert_eq!(parse_version(&format!("v{a}.{b}.{c}")), Some((a, b, c)));
        prop_assert_eq!(parse_version(&format!("{a}.{b}.{c}.0")), None);
    }
}

/// SEC5: two keys are pinned, the one in use and the next. A notice signed with either is
/// accepted; a third key, or a key file that is not all keys, pins nothing.
#[test]
fn a_notice_signed_with_the_next_key_is_accepted_and_a_bad_key_file_pins_nothing() {
    use dv_egress::update::{parse_keys, MAX_KEYS};
    let (now, next, stranger) = (keypair(), keypair(), keypair());
    let pinned = [public(&now), public(&next)];
    for k in [&now, &next] {
        let t = signed(k, &notice_value("9.0.0"), INSTALLER);
        assert!(matches!(check(&t, "0.1.0", &pinned), Ok(Some(_))));
    }
    let t = signed(&stranger, &notice_value("9.0.0"), INSTALLER);
    assert_eq!(check(&t, "0.1.0", &pinned), Err(UpdateError::BadSignature));
    assert_eq!(check(&t, "0.1.0", &[]), Err(UpdateError::BadSignature));

    let line = |k: &[u8; 32]| to_hex(k);
    let two = format!("# keys\n{}\n{}\n", line(&pinned[0]), line(&pinned[1]));
    assert_eq!(parse_keys(&two), pinned.to_vec());
    let three = format!("{two}{}\n", line(&public(&stranger)));
    assert!(parse_keys(&three).is_empty(), "more than {MAX_KEYS} keys");
    assert!(parse_keys(&format!("{two}not a key\n")).is_empty());
    assert_eq!(
        dv_egress::update::pinned_keys().len(),
        1,
        "this build pins one key"
    );
}
