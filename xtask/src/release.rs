//! Signed updates, on Or's Mac only (D-033).
//!
//! * `cargo xtask update-keygen` – once: a new Ed25519 key pair. The private key goes to
//!   `~/.diagnostic-vault/update-signing.pk8` (owner-only), never into the repository; the
//!   public key is written to `crates/dv-egress/update_key.pub`, to commit.
//! * `cargo xtask update-notice <version> <installer.exe> <notes.txt> <out-dir>` – the signed
//!   notice (`latest.json` + `latest.json.sig`) for a release, checked with the same code the
//!   program runs before it is written.
//! * `cargo xtask update-keygen --next` – once: a second key pair, pinned beside the first, so
//!   the day the first is lost or retired a release signed with the second is still accepted
//!   (SEC5). Sign with it by pointing `DV_UPDATE_KEY` at its file.
//! * `cargo xtask update-key-backup <out-file>` / `update-key-restore <in-file> <out-file>` –
//!   an encrypted copy of a private key to keep off this computer: a passphrase of at least
//!   12 characters (from `DV_KEY_BACKUP_PASSPHRASE`, or typed), Argon2id at the vault's floor,
//!   AES-256-GCM. The restored key must be one of the pinned keys.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
use dv_egress::update::{self, Fetch, UpdateError, PRODUCT};
use zeroize::Zeroizing;

const PUBLIC_FILE: &str = "crates/dv-egress/update_key.pub";
const BASE: &str = "https://github.com/orliber/einat-vault-releases/releases";

fn default_key_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("DV_UPDATE_KEY") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_owned())?;
    Ok(PathBuf::from(home).join(".diagnostic-vault/update-signing.pk8"))
}

fn next_key_path() -> Result<PathBuf, String> {
    let current = default_key_path()?;
    Ok(current.with_file_name("update-signing-next.pk8"))
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

pub fn keygen(root: &Path, args: &[String]) -> Result<String, String> {
    match args {
        [] => keygen_first(root),
        [flag] if flag == "--next" => keygen_next(root),
        _ => Err("usage: cargo xtask update-keygen [--next]".to_owned()),
    }
}

/// The second key: its private half beside the first, its public half pinned after it.
fn keygen_next(root: &Path) -> Result<String, String> {
    let file = root.join(PUBLIC_FILE);
    let text = fs::read_to_string(&file).map_err(|e| format!("{PUBLIC_FILE}: {e}"))?;
    let pinned = update::parse_keys(&text);
    if pinned.len() != 1 {
        return Err(format!(
            "{PUBLIC_FILE} must hold exactly one key before a next one is added (it holds {})",
            pinned.len()
        ));
    }
    let path = next_key_path()?;
    outside_repo(root, &path)?;
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| "keygen failed")?;
    let pair = Ed25519KeyPair::from_pkcs8(doc.as_ref()).map_err(|_| "keygen failed")?;
    write_private(&path, doc.as_ref())?;
    let public = update::to_hex(pair.public_key().as_ref());
    let joined = format!(
        "{}\n# The next key (SEC5): accepted too, for the day the first is retired.\n{public}\n",
        text.trim_end()
    );
    if update::parse_keys(&joined).len() != 2 {
        return Err("the key file would not read back as two keys".to_owned());
    }
    fs::write(&file, joined).map_err(|e| e.to_string())?;
    Ok(format!(
        "next private key: {} (back it up with update-key-backup, offline)\npublic key added to {PUBLIC_FILE} (commit it)",
        path.display()
    ))
}

fn keygen_first(root: &Path) -> Result<String, String> {
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
    pinned: &[[u8; 32]],
    notice: &[u8],
    installer: &[u8],
) -> Result<String, String> {
    let pair =
        Ed25519KeyPair::from_pkcs8(pair_pkcs8).map_err(|_| "the private key file is not valid")?;
    if !pinned.iter().any(|k| pair.public_key().as_ref() == k) {
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
        .map(|t| update::parse_keys(&t))
        .filter(|k| !k.is_empty())
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

// ------------------------------------------------------------ an encrypted copy of a key

const BACKUP_MAGIC: &[u8] = b"DVUKB1\n";
const BACKUP_AAD: &[u8] = b"dv/update-key-backup/v1";

/// The passphrase: `DV_KEY_BACKUP_PASSPHRASE`, or one line typed on the terminal.
fn passphrase() -> Result<Zeroizing<String>, String> {
    if let Ok(p) = std::env::var("DV_KEY_BACKUP_PASSPHRASE") {
        return Ok(Zeroizing::new(p));
    }
    {
        use std::io::Write;
        let mut err = std::io::stderr().lock();
        let _ = write!(
            err,
            "passphrase (12 characters or more, written down and kept apart from the copy): "
        );
        let _ = err.flush();
    }
    let mut line = Zeroizing::new(String::new());
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(Zeroizing::new(
        line.trim_end_matches(['\r', '\n']).to_owned(),
    ))
}

/// Seal a private key under a passphrase.
pub fn seal_key(
    pkcs8: &[u8],
    passphrase: &str,
    params: dv_vault::Argon2Params,
) -> Result<Vec<u8>, String> {
    dv_vault::password::check_policy(passphrase)
        .map_err(|_| "the passphrase is too weak: 12 characters or more, not a common one")?;
    Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| "the private key file is not valid")?;
    let salt: [u8; 32] = dv_vault::crypto::random_array().map_err(|e| e.to_string())?;
    let kek =
        dv_vault::password::derive_kek(passphrase, &salt, params).map_err(|e| e.to_string())?;
    let sealed = dv_vault::crypto::seal(&kek, BACKUP_AAD, pkcs8).map_err(|e| e.to_string())?;
    let mut out = BACKUP_MAGIC.to_vec();
    out.extend_from_slice(&salt);
    for n in [params.m_kib, params.t, params.p] {
        out.extend_from_slice(&n.to_le_bytes());
    }
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Open a sealed key; it must be one of `pinned`.
pub fn open_key(sealed: &[u8], passphrase: &str, pinned: &[[u8; 32]]) -> Result<Vec<u8>, String> {
    let rest = sealed
        .strip_prefix(BACKUP_MAGIC)
        .ok_or("not an encrypted key copy")?;
    if rest.len() < 32 + 12 {
        return Err("the copy is cut short".to_owned());
    }
    let (salt, rest) = rest.split_at(32);
    let (nums, body) = rest.split_at(12);
    let num = |i: usize| u32::from_le_bytes([nums[i], nums[i + 1], nums[i + 2], nums[i + 3]]);
    let params = dv_vault::Argon2Params {
        m_kib: num(0),
        t: num(4),
        p: num(8),
    };
    let salt: [u8; 32] = salt.try_into().map_err(|_| "bad salt")?;
    let kek =
        dv_vault::password::derive_kek(passphrase, &salt, params).map_err(|e| e.to_string())?;
    let pkcs8 = dv_vault::crypto::open(&kek, BACKUP_AAD, body)
        .map_err(|_| "wrong passphrase, or the copy was changed")?;
    let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| "the key inside is not valid")?;
    if !pinned.iter().any(|k| pair.public_key().as_ref() == k) {
        return Err(format!(
            "the key inside is not one of the keys in {PUBLIC_FILE}"
        ));
    }
    Ok(pkcs8.to_vec())
}

pub fn key_backup(root: &Path, args: &[String]) -> Result<String, String> {
    let [out] = args else {
        return Err("usage: cargo xtask update-key-backup <out-file>".to_owned());
    };
    let out = PathBuf::from(out);
    outside_repo(root, &out)?;
    let key = fs::read(default_key_path()?).map_err(|e| format!("private key: {e}"))?;
    let pass = passphrase()?;
    let sealed = seal_key(&key, &pass, dv_vault::Argon2Params::FLOOR)?;
    let pinned =
        update::parse_keys(&fs::read_to_string(root.join(PUBLIC_FILE)).unwrap_or_default());
    open_key(&sealed, &pass, &pinned)?;
    fs::write(&out, sealed).map_err(|e| e.to_string())?;
    Ok(format!(
        "encrypted copy: {} (checked: it opens and holds a pinned key). Keep it off this computer, and the passphrase apart from it.",
        out.display()
    ))
}

pub fn key_restore(root: &Path, args: &[String]) -> Result<String, String> {
    let [input, out] = args else {
        return Err("usage: cargo xtask update-key-restore <in-file> <out-file>".to_owned());
    };
    let out = PathBuf::from(out);
    outside_repo(root, &out)?;
    if out.exists() {
        return Err(format!("{} already exists", out.display()));
    }
    let sealed = fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let pinned =
        update::parse_keys(&fs::read_to_string(root.join(PUBLIC_FILE)).unwrap_or_default());
    let pass = passphrase()?;
    let key = open_key(&sealed, &pass, &pinned)?;
    write_private(&out, &key)?;
    Ok(format!("private key restored to {}", out.display()))
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
        assert!(sign_and_verify(doc.as_ref(), &[pinned], &body, exe).is_ok());
        assert!(sign_and_verify(doc.as_ref(), &[[7; 32], pinned], &body, exe).is_ok());
        assert!(sign_and_verify(doc.as_ref(), &[[7; 32]], &body, exe).is_err());
    }

    #[test]
    fn a_sealed_key_opens_only_with_its_passphrase_and_only_when_pinned() {
        let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let pinned: [u8; 32] = pair.public_key().as_ref().try_into().unwrap();
        let pass = "סוס כתום רץ בשדה הירוק";
        assert!(seal_key(doc.as_ref(), "קצר", dv_vault::Argon2Params::TEST).is_err());
        let sealed = seal_key(doc.as_ref(), pass, dv_vault::Argon2Params::TEST).unwrap();
        assert!(!sealed
            .windows(16)
            .any(|w| doc.as_ref().windows(16).any(|d| d == w)));
        assert_eq!(open_key(&sealed, pass, &[pinned]).unwrap(), doc.as_ref());
        assert!(open_key(&sealed, "סוס כתום רץ בשדה הכחול", &[pinned]).is_err());
        assert!(
            open_key(&sealed, pass, &[[7; 32]]).is_err(),
            "not a pinned key"
        );
        let mut changed = sealed.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert!(open_key(&changed, pass, &[pinned]).is_err());
        assert!(open_key(&sealed[..40], pass, &[pinned]).is_err());
    }
}
