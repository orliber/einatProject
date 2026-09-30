//! Signed updates, on Or's Mac only (D-033).
//!
//! * `cargo xtask update-keygen` – once: a new Ed25519 key pair. The private key goes to
//!   `~/.diagnostic-vault/update-signing.pk8` (owner-only), never into the repository; the
//!   public key is written to `crates/dv-egress/update_key.pub`, to commit.
//! * `cargo xtask update-notice <version> <installer.exe> <notes.txt> <out-dir>` – the signed
//!   notice (`latest.json` + `latest.json.sig`) for a release, checked with the same code the
//!   program runs before it is written.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
use dv_egress::update::{self, Fetch, UpdateError, PRODUCT};

const PUBLIC_FILE: &str = "crates/dv-egress/update_key.pub";
const BASE: &str = "https://github.com/orliber/einat-vault-releases/releases";

fn default_key_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("DV_UPDATE_KEY") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_owned())?;
    Ok(PathBuf::from(home).join(".diagnostic-vault/update-signing.pk8"))
}

fn outside_repo(root: &Path, path: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let parent = path.parent().unwrap_or(path);
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if parent.starts_with(&root) {
        return Err("the private key must live outside the repository".to_owned());
    }
    Ok(())
}

pub fn keygen(root: &Path) -> Result<String, String> {
    let path = default_key_path()?;
    outside_repo(root, &path)?;
    if path.exists() {
        return Err(format!(
            "{} already exists. A new key would make every installed copy refuse updates; \
             move the old one away only if you mean it.",
            path.display()
        ));
    }
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| "keygen failed")?;
    let pair = Ed25519KeyPair::from_pkcs8(doc.as_ref()).map_err(|_| "keygen failed")?;
    write_private(&path, doc.as_ref())?;
    let public = update::to_hex(pair.public_key().as_ref());
    fs::write(
        root.join(PUBLIC_FILE),
        format!(
            "# Public key for signed updates (D-033). Written by `cargo xtask update-keygen`.\n\
             # The private half is only on the developer's computer.\n{public}\n"
        ),
    )
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "private key: {} (back it up somewhere safe, offline)\npublic key: {PUBLIC_FILE} (commit it)",
        path.display()
    ))
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| e.to_string())
}

/// The notice for one release, as bytes.
pub fn notice_json(version: &str, installer: &[u8], notes: &[String], published: &str) -> Vec<u8> {
    let v = version.trim_start_matches('v');
    let body = serde_json::json!({
        "product": PRODUCT,
        "version": v,
        "published": published,
        "notes": notes,
        "installer": {
            "url": format!("{BASE}/download/v{v}/DiagnosticVault-Setup.exe"),
            "sha256": update::to_hex(&update::sha256(installer)),
            "size": installer.len(),
        }
    });
    serde_json::to_vec_pretty(&body).unwrap_or_default()
}

struct Local(HashMap<String, Vec<u8>>);
impl Fetch for Local {
    fn get(&self, url: &str, _max: u64) -> Result<Vec<u8>, UpdateError> {
        self.0.get(url).cloned().ok_or(UpdateError::Server(404))
    }
}

/// Sign, then check with the program's own code and the committed public key.
pub fn sign_and_verify(
    pair_pkcs8: &[u8],
    pinned: &[u8; 32],
    notice: &[u8],
    installer: &[u8],
) -> Result<String, String> {
    let pair =
        Ed25519KeyPair::from_pkcs8(pair_pkcs8).map_err(|_| "the private key file is not valid")?;
    if pair.public_key().as_ref() != pinned {
        return Err(format!(
            "the private key does not match {PUBLIC_FILE}: installed copies would refuse this update"
        ));
    }
    let sig = update::to_hex(pair.sign(&update::signed_bytes(notice)).as_ref());
    let v: serde_json::Value = serde_json::from_slice(notice).map_err(|e| e.to_string())?;
    let url = v["installer"]["url"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let server = Local(HashMap::from([
        (
            format!("{BASE}/latest/download/latest.json"),
            notice.to_vec(),
        ),
        (
            format!("{BASE}/latest/download/latest.json.sig"),
            sig.clone().into_bytes(),
        ),
        (url, installer.to_vec()),
    ]));
    let info = update::check(&server, "0.0.0", pinned)
        .map_err(|e| format!("self-check failed: {e}"))?
        .ok_or("self-check failed: not newer than 0.0.0")?;
    update::download(&server, &info).map_err(|e| format!("self-check failed: {e}"))?;
    Ok(sig)
}

pub fn notice(root: &Path, args: &[String]) -> Result<String, String> {
    let [version, installer, notes, out] = args else {
        return Err(
            "usage: cargo xtask update-notice <version> <installer.exe> <notes.txt> <out-dir>"
                .to_owned(),
        );
    };
    update::parse_version(version).ok_or("the version must look like 1.2.3")?;
    let pinned = fs::read_to_string(root.join(PUBLIC_FILE))
        .ok()
        .and_then(|t| update::parse_key(&t))
        .ok_or("no public key yet: run `cargo xtask update-keygen` first")?;
    let key = fs::read(default_key_path()?).map_err(|e| format!("private key: {e}"))?;
    let exe = fs::read(installer).map_err(|e| format!("{installer}: {e}"))?;
    if !exe.starts_with(b"MZ") {
        return Err(format!("{installer} is not a Windows program"));
    }
    let lines: Vec<String> = fs::read_to_string(notes)
        .map_err(|e| format!("{notes}: {e}"))?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let today = std::process::Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let body = notice_json(version, &exe, &lines, &today);
    let sig = sign_and_verify(&key, &pinned, &body, &exe)?;
    let out = PathBuf::from(out);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    fs::write(out.join("latest.json"), &body).map_err(|e| e.to_string())?;
    fs::write(out.join("latest.json.sig"), sig).map_err(|e| e.to_string())?;
    Ok(format!(
        "signed and checked: {}",
        out.join("latest.json").display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_signed_here_is_accepted_by_the_program_and_a_wrong_key_is_caught() {
        let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let pinned: [u8; 32] = pair.public_key().as_ref().try_into().unwrap();
        let exe = b"MZ installer";
        let body = notice_json("v0.2.0", exe, &["שיפור".to_owned()], "2026-10-01");
        assert!(sign_and_verify(doc.as_ref(), &pinned, &body, exe).is_ok());
        assert!(sign_and_verify(doc.as_ref(), &[7; 32], &body, exe).is_err());
    }
}
