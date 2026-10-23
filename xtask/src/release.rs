//! Signed updates (D-033, D-091).
//!
//! * `cargo xtask update-keygen` – once: a new Ed25519 key pair. The private key goes to
//!   `~/.diagnostic-vault/update-signing.pk8` (owner-only), never into the repository; the
//!   public key is written to `crates/dv-egress/update_key.pub`, to commit.
//! * `cargo xtask update-notice [--test-key] <version> <installer.exe> <notes.txt> <out-dir>
//!   [<mac.dmg> <app.cdhash>]` – the signed notice (`latest.json` + `latest.json.sig`) for a
//!   release, with the Mac disk image and its app's CDHash (as CI recorded it with
//!   `codesign`) when there is one (D-101), checked with the same
//!   code the program runs before it is written. The key is read from `DV_UPDATE_KEY` (on
//!   Or's Mac, or in the release workflow from a repository secret, D-091); `--test-key` signs
//!   with a key made for the run, for a rehearsal.
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

/// The name the Mac disk image is published under (D-101), beside the Windows installer.
pub const MAC_IMAGE: &str = "DiagnosticVault.dmg";

/// The Mac release: the disk image, and the CDHash of the one app in it.
pub type MacImage<'a> = (&'a [u8], [u8; update::CDHASH_LEN]);

/// The notice for one release, as bytes: the Windows installer (`installer`, read by every
/// version) and, when there is one, the Mac disk image with its app's CDHash (`macos`, read
/// by Mac copies from D-101 on; earlier versions ignore it).
pub fn notice_json(
    version: &str,
    installer: &[u8],
    mac: Option<MacImage<'_>>,
    notes: &[String],
    published: &str,
) -> Vec<u8> {
    let v = version.trim_start_matches('v');
    let file = |name: &str, bytes: &[u8]| {
        serde_json::json!({
            "url": format!("{BASE}/download/v{v}/{name}"),
            "sha256": update::to_hex(&update::sha256(bytes)),
            "size": bytes.len(),
        })
    };
    let mut body = serde_json::json!({
        "product": PRODUCT,
        "version": v,
        "published": published,
        "notes": notes,
        "installer": file("DiagnosticVault-Setup.exe", installer),
    });
    if let (Some((image, cdhash)), Some(o)) = (mac, body.as_object_mut()) {
        let mut entry = file(MAC_IMAGE, image);
        entry["cdhash"] = serde_json::json!(update::to_hex(&cdhash));
        o.insert("macos".to_owned(), entry);
    }
    serde_json::to_vec_pretty(&body).unwrap_or_default()
}

/// A CDHash as `codesign -dvvv` prints it (`CDHash=` then 40 hex digits), alone in a file.
#[must_use]
pub fn parse_cdhash(text: &str) -> Option<[u8; update::CDHASH_LEN]> {
    let t = text.trim();
    if t.len() != 2 * update::CDHASH_LEN || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; update::CDHASH_LEN];
    for (o, pair) in out.iter_mut().zip(t.as_bytes().chunks(2)) {
        *o = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// A disk image as `hdiutil` writes it: the UDIF trailer (`koly`) in its last 512 bytes.
#[must_use]
pub fn is_disk_image(bytes: &[u8]) -> bool {
    bytes.len() > 512 && bytes.get(bytes.len() - 512..bytes.len() - 508) == Some(b"koly".as_slice())
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
    mac: Option<MacImage<'_>>,
) -> Result<String, String> {
    let pair =
        Ed25519KeyPair::from_pkcs8(pair_pkcs8).map_err(|_| "the private key file is not valid")?;
    if !pinned.iter().any(|k| pair.public_key().as_ref() == k) {
        return Err(format!(
            "the private key does not match {PUBLIC_FILE}: installed copies would refuse this update"
        ));
    }
    let sig = update::to_hex(pair.sign(&update::signed_bytes(notice)).as_ref());
    let fail = |e: UpdateError| format!("self-check failed: {e}");
    let win = update::parse_notice_for(notice, "installer")
        .map_err(fail)?
        .ok_or("self-check failed: no Windows installer in the notice")?;
    let mac_info = update::parse_notice_for(notice, "macos").map_err(fail)?;
    let mut files = HashMap::from([
        (
            format!("{BASE}/latest/download/latest.json"),
            notice.to_vec(),
        ),
        (
            format!("{BASE}/latest/download/latest.json.sig"),
            sig.clone().into_bytes(),
        ),
        (win.installer_url.clone(), installer.to_vec()),
    ]);
    if let (Some(info), Some((bytes, _))) = (&mac_info, mac) {
        files.insert(info.installer_url.clone(), bytes.to_vec());
    }
    let server = Local(files);
    // The signature and this platform's file, exactly as an installed copy checks them.
    match update::check(&server, "0.0.0", pinned).map_err(fail)? {
        Some(info) => {
            update::download(&server, &info).map_err(fail)?;
        }
        // Signed on a Mac for a release without a Mac image: the signature was checked, and
        // there is no Mac file to fetch.
        None if update::PLATFORM_ENTRY == "macos" && mac.is_none() => {}
        None => return Err("self-check failed: not newer than 0.0.0".to_owned()),
    }
    // Each entry names exactly the file given for it.
    if win.sha256 != update::sha256(installer) || win.size != installer.len() as u64 {
        return Err("self-check failed: the Windows entry does not match the installer".to_owned());
    }
    match (mac, mac_info) {
        (None, None) => {}
        (Some((bytes, cdhash)), Some(info))
            if info.sha256 == update::sha256(bytes)
                && info.size == bytes.len() as u64
                && info.cdhash == Some(cdhash) => {}
        _ => {
            return Err("self-check failed: the Mac entry does not match the disk image".to_owned())
        }
    }
    Ok(sig)
}

pub fn notice(root: &Path, args: &[String]) -> Result<String, String> {
    // A rehearsal (`--test-key`): the whole notice is built, signed and checked with the
    // program's own code, but with a key made for this run only. Installed copies refuse it,
    // so nothing signed this way can update anyone.
    let (test_key, args) = match args {
        [flag, rest @ ..] if flag == "--test-key" => (true, rest),
        _ => (false, args),
    };
    let (version, installer, notes, out, mac) = match args {
        [version, installer, notes, out] => (version, installer, notes, out, None),
        [version, installer, notes, out, dmg, cdhash] => {
            (version, installer, notes, out, Some((dmg, cdhash)))
        }
        _ => {
            return Err(
                "usage: cargo xtask update-notice [--test-key] <version> <installer.exe> <notes.txt> <out-dir> [<mac.dmg> <app.cdhash>]"
                    .to_owned(),
            )
        }
    };
    update::parse_version(version).ok_or("the version must look like 1.2.3")?;
    let mut pinned = fs::read_to_string(root.join(PUBLIC_FILE))
        .ok()
        .map(|t| update::parse_keys(&t))
        .filter(|k| !k.is_empty())
        .ok_or("no public key yet: run `cargo xtask update-keygen` first")?;
    let key = if test_key {
        let doc =
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| "keygen failed")?;
        let pair = Ed25519KeyPair::from_pkcs8(doc.as_ref()).map_err(|_| "keygen failed")?;
        let public: [u8; 32] = pair
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| "keygen failed")?;
        pinned = vec![public];
        doc.as_ref().to_vec()
    } else {
        fs::read(default_key_path()?).map_err(|e| format!("private key: {e}"))?
    };
    let exe = fs::read(installer).map_err(|e| format!("{installer}: {e}"))?;
    if !exe.starts_with(b"MZ") {
        return Err(format!("{installer} is not a Windows program"));
    }
    let dmg = match mac {
        Some((path, cdhash_file)) => {
            let bytes = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            if !is_disk_image(&bytes) {
                return Err(format!("{path} is not a Mac disk image"));
            }
            let cdhash = fs::read_to_string(cdhash_file)
                .ok()
                .as_deref()
                .and_then(parse_cdhash)
                .ok_or_else(|| format!("{cdhash_file} does not hold one CDHash"))?;
            Some((bytes, cdhash))
        }
        None => None,
    };
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
    let mac = dmg
        .as_ref()
        .map(|(bytes, cdhash)| (bytes.as_slice(), *cdhash));
    let body = notice_json(version, &exe, mac, &lines, &today);
    let sig = sign_and_verify(&key, &pinned, &body, &exe, mac)?;
    let out = PathBuf::from(out);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    fs::write(out.join("latest.json"), &body).map_err(|e| e.to_string())?;
    fs::write(out.join("latest.json.sig"), sig).map_err(|e| e.to_string())?;
    Ok(format!(
        "signed and checked{}: {}",
        if test_key {
            " with a key for this run only (a rehearsal: installed copies refuse it)"
        } else {
            ""
        },
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
        let body = notice_json("v0.2.0", exe, None, &["שיפור".to_owned()], "2026-10-01");
        assert!(sign_and_verify(doc.as_ref(), &[pinned], &body, exe, None).is_ok());
        assert!(sign_and_verify(doc.as_ref(), &[[7; 32], pinned], &body, exe, None).is_ok());
        assert!(sign_and_verify(doc.as_ref(), &[[7; 32]], &body, exe, None).is_err());
        // With the Mac image: its entry must name exactly that image.
        let mut dmg = vec![0u8; 1024];
        dmg[512..516].copy_from_slice(b"koly");
        assert!(is_disk_image(&dmg) && !is_disk_image(exe));
        let cdhash = parse_cdhash("0123456789abcdef0123456789ABCDEF01234567\n").unwrap();
        let mac = Some((dmg.as_slice(), cdhash));
        let both = notice_json("v0.2.0", exe, mac, &["שיפור".to_owned()], "2026-10-01");
        assert!(sign_and_verify(doc.as_ref(), &[pinned], &both, exe, mac).is_ok());
        let other: &[u8] = b"other";
        assert!(
            sign_and_verify(doc.as_ref(), &[pinned], &both, exe, Some((other, cdhash))).is_err()
        );
        let mut wrong = cdhash;
        wrong[0] ^= 1;
        assert!(sign_and_verify(doc.as_ref(), &[pinned], &both, exe, Some((&dmg, wrong))).is_err());
        assert!(sign_and_verify(doc.as_ref(), &[pinned], &both, exe, None).is_err());
        assert!(sign_and_verify(doc.as_ref(), &[pinned], &body, exe, mac).is_err());
        let v: serde_json::Value = serde_json::from_slice(&both).unwrap();
        assert!(v["macos"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/v0.2.0/DiagnosticVault.dmg"));
        assert_eq!(
            v["macos"]["cdhash"],
            "0123456789abcdef0123456789abcdef01234567"
        );
        for bad in [
            "",
            "0123",
            "0123456789abcdef0123456789abcdef0123456z",
            "+123456789abcdef0123456789abcdef01234567",
        ] {
            assert!(parse_cdhash(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_rehearsal_signs_and_checks_with_a_key_of_its_own() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dir = std::env::temp_dir().join(format!("dv-notice-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("setup.exe");
        let notes = dir.join("notes.txt");
        fs::write(&exe, b"MZ invented installer").unwrap();
        fs::write(&notes, "שורה ראשונה\n\nשורה שנייה\n").unwrap();
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let args = [
            "--test-key".to_owned(),
            "0.9.1".to_owned(),
            s(&exe),
            s(&notes),
            s(&dir.join("out")),
        ];
        let said = notice(&root, &args).unwrap();
        assert!(said.contains("rehearsal"), "{said}");
        let body: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("out/latest.json")).unwrap()).unwrap();
        assert_eq!(body["version"], "0.9.1");
        assert_eq!(body["notes"].as_array().unwrap().len(), 2);
        assert_eq!(
            fs::read_to_string(dir.join("out/latest.json.sig"))
                .unwrap()
                .len(),
            128
        );
        // Without `--test-key` and without a key file, nothing is signed.
        std::env::set_var("DV_UPDATE_KEY", dir.join("no-such-key"));
        assert!(notice(&root, &args[1..]).is_err());
        fs::remove_dir_all(&dir).unwrap();
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
