//! New versions of the program (D-033): the only other place the program reaches the network.
//!
//! What leaves: two `GET` requests to GitHub (the release notice and, when she agrees, the
//! installer), with a fixed user agent and nothing from the vault. What is trusted: only an
//! installer whose SHA-256 is inside a notice signed with Or's Ed25519 key. The public half
//! is pinned in the binary (`update_key.pub`); the private half never enters the repository.
//! Any doubt (no key, bad signature, wrong hash, same or older version, a host that is not
//! on the list) means no update: the program keeps working as it is.

use std::io::Read;
use std::time::Duration;

use aws_lc_rs::{digest, signature};
use serde_json::Value;

/// Where the signed notice of the newest version is published.
pub const RELEASES_REPO: &str = "orliber/einat-vault-releases";
const MANIFEST_URL: &str =
    "https://github.com/orliber/einat-vault-releases/releases/latest/download/latest.json";
const SIGNATURE_URL: &str =
    "https://github.com/orliber/einat-vault-releases/releases/latest/download/latest.json.sig";
/// Every installer lives under this prefix.
const INSTALLER_PREFIX: &str = "https://github.com/orliber/einat-vault-releases/releases/download/";
/// GitHub answers a release download with a redirect to its file storage. Nothing else.
pub const UPDATE_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];
/// The product this notice is for, so a notice for anything else is refused.
pub const PRODUCT: &str = "il.diagnosticvault.desktop";
/// Signed bytes are `DOMAIN || notice`, so a signature over anything else never fits.
const DOMAIN: &[u8] = b"diagnostic-vault-update-v1\n";
const MAX_NOTICE: u64 = 64 * 1024;
const MAX_INSTALLER: u64 = 400 * 1024 * 1024;

/// The pinned public key, hex. Empty (or a comment) until Or creates the key pair.
const PINNED_KEY: &str = include_str!("../update_key.pub");

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UpdateError {
    #[error("updates are not set up in this build (no public key)")]
    NotConfigured,
    #[error("no connection to the update server")]
    Offline,
    #[error("the update server answered {0}")]
    Server(u16),
    #[error("the notice is not signed by the developer's key")]
    BadSignature,
    #[error("the notice is not valid: {0}")]
    BadNotice(String),
    #[error("the downloaded file does not match the signed notice")]
    Tampered,
    #[error("the file is larger than allowed")]
    TooLarge,
    #[error("the address is not on the update list: {0}")]
    HostRefused(String),
}

/// A newer version, verified: the notice was signed by the pinned key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub published: String,
    /// What is new, in Hebrew, one line each.
    pub notes: Vec<String>,
    pub installer_url: String,
    pub sha256: [u8; 32],
    pub size: u64,
}

/// Whatever fetches bytes: GitHub in the program, a table in the tests.
pub trait Fetch {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, UpdateError>;
}

/// The pinned key, when this build has one.
pub fn pinned_key() -> Option<[u8; 32]> {
    parse_key(PINNED_KEY)
}

/// A key file: the first line that is not a comment, 64 hex digits.
pub fn parse_key(text: &str) -> Option<[u8; 32]> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let bytes = from_hex(line)?;
    bytes.try_into().ok()
}

/// The bytes that are signed for a notice.
#[must_use]
pub fn signed_bytes(notice: &[u8]) -> Vec<u8> {
    let mut out = DOMAIN.to_vec();
    out.extend_from_slice(notice);
    out
}

/// Is there a newer version? `Ok(None)` means this one is the newest.
pub fn check(
    fetch: &dyn Fetch,
    current: &str,
    key: &[u8; 32],
) -> Result<Option<UpdateInfo>, UpdateError> {
    let notice = fetch.get(MANIFEST_URL, MAX_NOTICE)?;
    let sig_text = fetch.get(SIGNATURE_URL, 1024)?;
    let sig =
        from_hex(String::from_utf8_lossy(&sig_text).trim()).ok_or(UpdateError::BadSignature)?;
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&signed_bytes(&notice), &sig)
        .map_err(|_| UpdateError::BadSignature)?;
    let info = parse_notice(&notice)?;
    let newer = match (parse_version(&info.version), parse_version(current)) {
        (Some(new), Some(now)) => new > now,
        _ => return Err(UpdateError::BadNotice("version".to_owned())),
    };
    Ok(newer.then_some(info))
}

/// Download the installer of a verified notice and check it against the signed hash.
pub fn download(fetch: &dyn Fetch, info: &UpdateInfo) -> Result<Vec<u8>, UpdateError> {
    if !info.installer_url.starts_with(INSTALLER_PREFIX) {
        return Err(UpdateError::HostRefused(info.installer_url.clone()));
    }
    let bytes = fetch.get(&info.installer_url, info.size.min(MAX_INSTALLER))?;
    if bytes.len() as u64 != info.size || sha256(&bytes) != info.sha256 {
        return Err(UpdateError::Tampered);
    }
    Ok(bytes)
}

fn parse_notice(bytes: &[u8]) -> Result<UpdateInfo, UpdateError> {
    let bad = |what: &str| UpdateError::BadNotice(what.to_owned());
    let v: Value = serde_json::from_slice(bytes).map_err(|_| bad("json"))?;
    if v["product"] != PRODUCT {
        return Err(bad("product"));
    }
    let version = v["version"].as_str().ok_or_else(|| bad("version"))?;
    parse_version(version).ok_or_else(|| bad("version"))?;
    let installer = &v["installer"];
    let url = installer["url"].as_str().ok_or_else(|| bad("url"))?;
    if !url.starts_with(INSTALLER_PREFIX) || url.contains("..") {
        return Err(bad("url"));
    }
    let sha256: [u8; 32] = installer["sha256"]
        .as_str()
        .and_then(from_hex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| bad("sha256"))?;
    let size = installer["size"]
        .as_u64()
        .filter(|s| (1..=MAX_INSTALLER).contains(s))
        .ok_or_else(|| bad("size"))?;
    let notes = v["notes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .take(20)
                .map(|s| s.chars().take(300).collect())
                .collect()
        })
        .unwrap_or_default();
    Ok(UpdateInfo {
        version: version.to_owned(),
        published: v["published"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(10)
            .collect(),
        notes,
        installer_url: url.to_owned(),
        sha256,
        size,
    })
}

/// `major.minor.patch`, digits only.
#[must_use]
pub fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.trim().trim_start_matches('v').split('.');
    let parts = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    it.next().is_none().then_some(parts)
}

#[must_use]
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(digest::digest(&digest::SHA256, bytes).as_ref());
    out
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[must_use]
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// GitHub over TLS 1.3 with Mozilla's roots; redirects only to the hosts on the list.
#[derive(Debug)]
pub struct GitHubFetch {
    client: reqwest::blocking::Client,
}

fn host_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(|h| UPDATE_HOSTS.contains(&h))
}

impl GitHubFetch {
    pub fn new() -> Result<Self, UpdateError> {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !host_allowed(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        let client = reqwest::blocking::Client::builder()
            .use_preconfigured_tls(crate::tls_config())
            .https_only(true)
            .no_proxy()
            .redirect(policy)
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(20))
            .user_agent("diagnostic-vault-update")
            .build()
            .map_err(|_| UpdateError::Offline)?;
        Ok(Self { client })
    }
}

impl Fetch for GitHubFetch {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, UpdateError> {
        let parsed =
            reqwest::Url::parse(url).map_err(|_| UpdateError::HostRefused(url.to_owned()))?;
        if !host_allowed(&parsed) {
            return Err(UpdateError::HostRefused(url.to_owned()));
        }
        let resp = self
            .client
            .get(parsed)
            .send()
            .map_err(|_| UpdateError::Offline)?;
        if !host_allowed(resp.url()) {
            return Err(UpdateError::HostRefused(resp.url().to_string()));
        }
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(UpdateError::Server(status));
        }
        let mut out = Vec::new();
        resp.take(max + 1)
            .read_to_end(&mut out)
            .map_err(|_| UpdateError::Offline)?;
        if out.len() as u64 > max {
            return Err(UpdateError::TooLarge);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
    use serde_json::json;
    use std::collections::HashMap;

    struct Table(HashMap<String, Vec<u8>>);
    impl Fetch for Table {
        fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, UpdateError> {
            let b = self.0.get(url).cloned().ok_or(UpdateError::Server(404))?;
            if b.len() as u64 > max {
                return Err(UpdateError::TooLarge);
            }
            Ok(b)
        }
    }

    fn keypair() -> Ed25519KeyPair {
        let rng = SystemRandom::new();
        let Ok(doc) = Ed25519KeyPair::generate_pkcs8(&rng) else {
            unreachable!()
        };
        let Ok(k) = Ed25519KeyPair::from_pkcs8(doc.as_ref()) else {
            unreachable!()
        };
        k
    }

    fn public(k: &Ed25519KeyPair) -> [u8; 32] {
        let Ok(p) = k.public_key().as_ref().try_into() else {
            unreachable!()
        };
        p
    }

    const INSTALLER: &[u8] = b"MZ fake installer bytes";
    const URL: &str = "https://github.com/orliber/einat-vault-releases/releases/download/v0.2.0/DiagnosticVault-Setup.exe";

    fn notice(version: &str) -> Vec<u8> {
        let hash = digest::digest(&digest::SHA256, INSTALLER);
        serde_json::to_vec(&json!({
            "product": PRODUCT, "version": version, "published": "2026-10-01",
            "notes": ["שיפורים בסינון"],
            "installer": {"url": URL, "sha256": to_hex(hash.as_ref()), "size": INSTALLER.len()}
        }))
        .unwrap_or_default()
    }

    fn server(k: &Ed25519KeyPair, notice: Vec<u8>, installer: &[u8]) -> Table {
        let sig = to_hex(k.sign(&signed_bytes(&notice)).as_ref());
        Table(HashMap::from([
            (MANIFEST_URL.to_owned(), notice),
            (SIGNATURE_URL.to_owned(), sig.into_bytes()),
            (URL.to_owned(), installer.to_vec()),
        ]))
    }

    #[test]
    fn a_signed_newer_version_is_offered_and_its_installer_verified() {
        let k = keypair();
        let t = server(&k, notice("0.2.0"), INSTALLER);
        let Ok(Some(info)) = check(&t, "0.1.0", &public(&k)) else {
            panic!("no update")
        };
        assert_eq!(info.version, "0.2.0");
        assert_eq!(info.notes, vec!["שיפורים בסינון".to_owned()]);
        assert_eq!(download(&t, &info).as_deref(), Ok(INSTALLER));
    }

    #[test]
    fn the_same_or_an_older_version_is_not_offered() {
        let k = keypair();
        let t = server(&k, notice("0.2.0"), INSTALLER);
        assert_eq!(check(&t, "0.2.0", &public(&k)), Ok(None));
        assert_eq!(check(&t, "0.10.0", &public(&k)), Ok(None));
    }

    #[test]
    fn another_key_or_a_changed_notice_is_refused() {
        let k = keypair();
        let other = keypair();
        let t = server(&other, notice("0.2.0"), INSTALLER);
        assert_eq!(
            check(&t, "0.1.0", &public(&k)),
            Err(UpdateError::BadSignature)
        );

        let mut t = server(&k, notice("0.2.0"), INSTALLER);
        t.0.insert(MANIFEST_URL.to_owned(), notice("9.9.9"));
        assert_eq!(
            check(&t, "0.1.0", &public(&k)),
            Err(UpdateError::BadSignature)
        );

        // A signature over the notice alone (no domain prefix) does not fit either.
        let mut t = server(&k, notice("0.2.0"), INSTALLER);
        t.0.insert(
            SIGNATURE_URL.to_owned(),
            to_hex(k.sign(&notice("0.2.0")).as_ref()).into_bytes(),
        );
        assert_eq!(
            check(&t, "0.1.0", &public(&k)),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn a_changed_installer_is_refused() {
        let k = keypair();
        let t = server(&k, notice("0.2.0"), b"MZ evil installer bytes");
        let Ok(Some(info)) = check(&t, "0.1.0", &public(&k)) else {
            panic!("no update")
        };
        assert_eq!(download(&t, &info), Err(UpdateError::Tampered));
    }

    #[test]
    fn a_signed_notice_for_another_product_or_address_is_refused() {
        let k = keypair();
        let mut n: Value = serde_json::from_slice(&notice("0.2.0")).unwrap_or_default();
        n["product"] = json!("something.else");
        let t = server(&k, serde_json::to_vec(&n).unwrap_or_default(), INSTALLER);
        assert!(matches!(
            check(&t, "0.1.0", &public(&k)),
            Err(UpdateError::BadNotice(_))
        ));

        let mut n: Value = serde_json::from_slice(&notice("0.2.0")).unwrap_or_default();
        n["installer"]["url"] = json!("https://evil.example/Setup.exe");
        let t = server(&k, serde_json::to_vec(&n).unwrap_or_default(), INSTALLER);
        assert!(matches!(
            check(&t, "0.1.0", &public(&k)),
            Err(UpdateError::BadNotice(_))
        ));
    }

    #[test]
    fn only_github_hosts_over_https_are_allowed() {
        let ok = |u: &str| reqwest::Url::parse(u).is_ok_and(|u| host_allowed(&u));
        assert!(ok(MANIFEST_URL));
        assert!(ok("https://objects.githubusercontent.com/x"));
        assert!(!ok("http://github.com/x"));
        assert!(!ok("https://github.com.evil.example/x"));
        assert!(!ok("https://api.anthropic.com/v1/messages"));
        assert!(GitHubFetch::new().is_ok());
    }

    #[test]
    fn versions_and_keys_parse_strictly() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2.x"), None);
        assert_eq!(parse_key("# none yet\n"), None);
        assert_eq!(
            parse_key(&format!("# key\n{}\n", "ab".repeat(32))),
            Some([0xab; 32])
        );
        assert_eq!(parse_key("zz"), None);
    }
}
