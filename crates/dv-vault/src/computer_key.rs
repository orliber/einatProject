//! Keys that stay on this computer: the half of the Google slot (D-041), and the half of the
//! password slot once it is bound to this computer (D-076).
//!
//! Each is a random key kept only on this computer, for her account, with its own seal per
//! slot. The file in the vault folder (`google.computer`, `vault.computer`) is not part of a
//! backup, and is useless on another computer or under another account. With the key in her
//! Google Drive, or with her password, it opens its slot; neither opens anything alone.
//!
//! * **Windows:** the key is sealed with DPAPI to her Windows account (`CurrentUser`), and
//!   the sealed bytes are the file.
//! * **macOS (D-101):** the key is an item in her login Keychain, which only her macOS
//!   account opens; the file holds only the item's random name (`kc1:<id>`). An item made for
//!   one slot is never found under the other's name.
//!
//! No new dependency and no `unsafe`: each is reached through the system's own program (by
//! its full path: Windows PowerShell, `/usr/bin/security`), with the secret on stdin or
//! stdout, never on the command line. Elsewhere (Linux) the feature is not offered.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use zeroize::Zeroizing;

use crate::crypto::Key32;
use crate::VaultError;

pub const COMPUTER_KEY_FILE: &str = "google.computer";
pub const PASSWORD_KEY_FILE: &str = "vault.computer";

/// Which slot a computer key is half of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Google,
    Password,
}

impl Purpose {
    fn file(self) -> &'static str {
        match self {
            Purpose::Google => COMPUTER_KEY_FILE,
            Purpose::Password => PASSWORD_KEY_FILE,
        }
    }

    /// Bound into the seal, so a blob made by another program, or for the other slot, never
    /// fits.
    fn entropy(self) -> &'static str {
        match self {
            Purpose::Google => "dv/google-computer-key/v1",
            Purpose::Password => "dv/password-computer-key/v1",
        }
    }
}

/// True where this computer can keep a key for her account alone (Windows, macOS).
#[must_use]
pub fn available() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

#[must_use]
pub fn path(dir: &Path) -> PathBuf {
    path_for(dir, Purpose::Google)
}

#[must_use]
pub fn exists(dir: &Path) -> bool {
    exists_for(dir, Purpose::Google)
}

/// A new computer key for the Google slot, sealed and written (replacing an older one).
pub fn create(dir: &Path) -> Result<Key32, VaultError> {
    create_for(dir, Purpose::Google)
}

/// The Google slot's computer key, unsealed. Fails on another computer or Windows account.
pub fn load(dir: &Path) -> Result<Key32, VaultError> {
    load_for(dir, Purpose::Google)
}

pub fn remove(dir: &Path) {
    remove_for(dir, Purpose::Google);
}

#[must_use]
pub fn path_for(dir: &Path, purpose: Purpose) -> PathBuf {
    dir.join(purpose.file())
}

#[must_use]
pub fn exists_for(dir: &Path, purpose: Purpose) -> bool {
    path_for(dir, purpose).exists()
}

/// A new computer key, sealed and written (replacing an older one).
pub fn create_for(dir: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    let key = Key32::random()?;
    let sealed = protect(key.as_bytes(), purpose)?;
    let tmp = dir.join(format!("{}.tmp", purpose.file()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(sealed.as_bytes())?;
        f.sync_all()?;
    }
    let old = fs::read_to_string(path_for(dir, purpose)).ok();
    fs::rename(&tmp, path_for(dir, purpose))?;
    if let Some(old) = old {
        forget(old.trim(), purpose);
    }
    Ok(key)
}

/// A new computer key held aside (`vault.computer.next`), not yet replacing the one in use:
/// the slot it is for is written first, and only then is it promoted (review of stage 5, S5).
/// Until then the key in use stays, so a slot that was not written still opens.
pub fn create_next_for(dir: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    let key = Key32::random()?;
    let sealed = protect(key.as_bytes(), purpose)?;
    let tmp = dir.join(format!("{}.next.tmp", purpose.file()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(sealed.as_bytes())?;
        f.sync_all()?;
    }
    // A key held aside earlier and never promoted (the program closed in between) is replaced.
    let stale = fs::read_to_string(next_path(dir, purpose)).ok();
    fs::rename(&tmp, next_path(dir, purpose))?;
    if let Some(stale) = stale {
        forget(stale.trim(), purpose);
    }
    Ok(key)
}

/// The key held aside, unsealed (for an opening after the slot was written but before the
/// key was promoted).
pub fn load_next_for(dir: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    load_from(&next_path(dir, purpose), purpose, OPENING_WAIT)
}

/// The key held aside becomes the one in use.
pub fn promote_next_for(dir: &Path, purpose: Purpose) -> Result<(), VaultError> {
    let old = fs::read_to_string(path_for(dir, purpose)).ok();
    fs::rename(next_path(dir, purpose), path_for(dir, purpose))?;
    if let Some(old) = old {
        forget(old.trim(), purpose);
    }
    Ok(())
}

fn next_path(dir: &Path, purpose: Purpose) -> PathBuf {
    dir.join(format!("{}.next", purpose.file()))
}

/// The computer key, unsealed. Fails on another computer or Windows account.
pub fn load_for(dir: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    load_from(&path_for(dir, purpose), purpose, OPENING_WAIT)
}

/// The same while the vault is open, where the check runs under the core's lock: a shorter
/// wait, so a slow Windows never holds the program for long (review of stage 5).
pub fn load_in_session_for(dir: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    load_from(&path_for(dir, purpose), purpose, IN_SESSION_WAIT)
}

fn load_from(file: &Path, purpose: Purpose, wait: Duration) -> Result<Key32, VaultError> {
    let sealed = fs::read_to_string(file).map_err(|_| VaultError::NotFound)?;
    let plain = unprotect(sealed.trim(), purpose, wait)?;
    let bytes: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::Corrupt("computer key length".to_owned()))?;
    Ok(Key32::from_bytes(bytes))
}

pub fn remove_for(dir: &Path, purpose: Purpose) {
    if let Ok(old) = fs::read_to_string(path_for(dir, purpose)) {
        forget(old.trim(), purpose);
    }
    let _ = fs::remove_file(path_for(dir, purpose));
}

/// Longest wait for the sealing or unsealing child at an opening, which runs outside the
/// core's lock. A first PowerShell after the computer starts, beside a busy antivirus, took
/// over 20 s on a CI runner; past this it is `Busy` (she tries again).
const OPENING_WAIT: Duration = Duration::from_secs(45);
/// The same for a check while the vault is open, under the core's lock.
const IN_SESSION_WAIT: Duration = Duration::from_secs(20);

/// Run `program` with `args`, `input` on stdin; its stdout, trimmed, if it succeeded.
///
/// A child that never comes back (an antivirus holding it, a locked-down shell, a Keychain
/// waiting on a dialog) is stopped and the key is taken as not here: it runs on every
/// opening, under the core's lock, so it must not freeze the program (review of stage 5,
/// S6). Fail-closed.
fn run(
    program: &str,
    args: &[&str],
    input: &[u8],
    wait: Duration,
) -> Result<Zeroizing<String>, VaultError> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input)?;
    }
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Zeroizing::new(Vec::new());
        if let Some(out) = stdout.as_mut() {
            let _ = std::io::Read::read_to_end(out, &mut buf);
        }
        buf
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > wait {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(VaultError::Busy);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let out = reader
        .join()
        .map_err(|_| VaultError::Crypto("the key sealed to this computer could not be read"))?;
    let text = Zeroizing::new(String::from_utf8_lossy(&out).trim().to_owned());
    if !status.success() || text.is_empty() {
        return Err(VaultError::Crypto(
            "the key sealed to this computer could not be opened",
        ));
    }
    Ok(text)
}

fn not_here() -> VaultError {
    VaultError::Refused(
        "keeping a key for this computer's account is not available here".to_owned(),
    )
}

// ------------------------------------------------------------------ Windows: DPAPI

fn powershell() -> String {
    std::env::var_os("SystemRoot").map_or_else(
        || "powershell".to_owned(),
        |root| {
            Path::new(&root)
                .join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
                .to_string_lossy()
                .into_owned()
        },
    )
}

/// Run a fixed DPAPI script with `input` (base64) on stdin; its stdout is base64.
fn dpapi(
    script: &str,
    input: &str,
    purpose: Purpose,
    wait: Duration,
) -> Result<Zeroizing<String>, VaultError> {
    let entropy = purpose.entropy();
    let full = format!(
        "Add-Type -AssemblyName System.Security; $e=[Text.Encoding]::UTF8.GetBytes('{entropy}'); $b=[Convert]::FromBase64String([Console]::In.ReadToEnd().Trim()); {script}"
    );
    run(
        &powershell(),
        &["-NoProfile", "-NonInteractive", "-Command", &full],
        input.as_bytes(),
        wait,
    )
}

// ------------------------------------------------------------------ macOS: the Keychain

const SECURITY: &str = "/usr/bin/security";
/// The Keychain items' service; each item's account is the slot's purpose and a random id.
const KEYCHAIN_SERVICE: &str = "dv-diagnostic-vault";
/// What the file holds on a Mac: this, then the item's id.
const KEYCHAIN_PREFIX: &str = "kc1:";

fn keychain_account(purpose: Purpose, id: &str) -> String {
    format!("{}/{id}", purpose.entropy())
}

/// The id written in the file, if it is one this program made (hex only, so it is never an
/// option or a second command to `security`).
fn keychain_id(sealed: &str) -> Option<&str> {
    let id = sealed.strip_prefix(KEYCHAIN_PREFIX)?;
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

fn hex(bytes: &[u8]) -> Zeroizing<String> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = Zeroizing::new(String::with_capacity(bytes.len() * 2));
    for &b in bytes {
        for nibble in [b >> 4, b & 15] {
            out.push(char::from(
                DIGITS.get(usize::from(nibble)).copied().unwrap_or(b'0'),
            ));
        }
    }
    out
}

fn unhex(text: &str) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let bad = || VaultError::Crypto("the key kept for this computer is not readable");
    if text.len() % 2 != 0 {
        return Err(bad());
    }
    let mut out = Zeroizing::new(Vec::with_capacity(text.len() / 2));
    for pair in text.as_bytes().chunks(2) {
        let s = std::str::from_utf8(pair).map_err(|_| bad())?;
        out.push(u8::from_str_radix(s, 16).map_err(|_| bad())?);
    }
    Ok(out)
}

/// A new Keychain item holding `plain`. The secret goes to `security` on stdin (its
/// interactive mode reads the command there), never on a command line another process can
/// see; the item is then read back, so a key is only used once it is really kept.
fn keychain_store(plain: &[u8; 32], purpose: Purpose) -> Result<Zeroizing<String>, VaultError> {
    let mut idb = [0u8; 16];
    aws_lc_rs::rand::fill(&mut idb).map_err(|_| VaultError::Crypto("random"))?;
    let id = hex(&idb);
    let account = keychain_account(purpose, &id);
    let command = Zeroizing::new(format!(
        "add-generic-password -U -s {KEYCHAIN_SERVICE} -a {account} -l {KEYCHAIN_SERVICE} -w {}\n",
        hex(plain).as_str()
    ));
    // `security -i` prints its prompt and nothing else; success is the read-back below.
    let _ = run(SECURITY, &["-i"], command.as_bytes(), OPENING_WAIT);
    let sealed = Zeroizing::new(format!("{KEYCHAIN_PREFIX}{}", id.as_str()));
    match keychain_load(&sealed, purpose, OPENING_WAIT) {
        Ok(back) if back.as_slice() == plain.as_slice() => Ok(sealed),
        // An item `add` may have made is not left behind, whatever went wrong.
        other => {
            keychain_delete(&sealed, purpose);
            Err(other.err().unwrap_or(VaultError::Crypto(
                "the key could not be kept in this computer's Keychain",
            )))
        }
    }
}

fn keychain_load(
    sealed: &str,
    purpose: Purpose,
    wait: Duration,
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let id = keychain_id(sealed).ok_or(VaultError::Crypto(
        "the key kept for this computer is not readable",
    ))?;
    let account = keychain_account(purpose, id);
    let out = run(
        SECURITY,
        &[
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            &account,
            "-w",
        ],
        &[],
        wait,
    )?;
    unhex(&out)
}

fn keychain_delete(sealed: &str, purpose: Purpose) {
    if let Some(id) = keychain_id(sealed) {
        let account = keychain_account(purpose, id);
        let _ = run(
            SECURITY,
            &[
                "delete-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
                "-a",
                &account,
            ],
            &[],
            IN_SESSION_WAIT,
        );
    }
}

// ------------------------------------------------------------------ either

fn protect(plain: &[u8; 32], purpose: Purpose) -> Result<Zeroizing<String>, VaultError> {
    if cfg!(windows) {
        dpapi(
            "[Console]::Out.Write([Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect($b,$e,'CurrentUser')))",
            &b64(plain),
            purpose,
            OPENING_WAIT,
        )
    } else if cfg!(target_os = "macos") {
        keychain_store(plain, purpose)
    } else {
        Err(not_here())
    }
}

fn unprotect(
    sealed: &str,
    purpose: Purpose,
    wait: Duration,
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    if cfg!(windows) {
        let out = dpapi(
            "[Console]::Out.Write([Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Unprotect($b,$e,'CurrentUser')))",
            sealed,
            purpose,
            wait,
        )?;
        unb64(&out).map(Zeroizing::new)
    } else if cfg!(target_os = "macos") {
        keychain_load(sealed, purpose, wait)
    } else {
        Err(not_here())
    }
}

/// A key that is no longer in use: on a Mac its Keychain item goes too (a DPAPI seal is only
/// the file). Best effort.
fn forget(sealed: &str, purpose: Purpose) {
    if cfg!(target_os = "macos") {
        keychain_delete(sealed, purpose);
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(bytes: &[u8]) -> Zeroizing<String> {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let at = |k: usize| u32::from(chunk.get(k).copied().unwrap_or(0));
        let n = (at(0) << 16) | (at(1) << 8) | at(2);
        for i in 0..4 {
            if i <= chunk.len() {
                // Six bits always name one of the 64 letters.
                let six = ((n >> (18 - 6 * i)) & 63) as usize;
                out.push(B64.get(six).map_or('=', |&c| char::from(c)));
            } else {
                out.push('=');
            }
        }
    }
    Zeroizing::new(out)
}

fn unb64(text: &str) -> Result<Vec<u8>, VaultError> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in text.trim_end_matches('=').bytes() {
        let v = B64
            .iter()
            .position(|&a| a == c)
            .ok_or(VaultError::Crypto("bad base64"))?;
        acc = (acc << 6) | u32::try_from(v).map_err(|_| VaultError::Crypto("bad base64"))?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(
                u8::try_from((acc >> bits) & 0xff).map_err(|_| VaultError::Crypto("bad base64"))?,
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for n in 0..40u8 {
            let bytes: Vec<u8> = (0..n).map(|i| i.wrapping_mul(53)).collect();
            assert_eq!(unb64(&b64(&bytes)).unwrap(), bytes);
        }
        assert_eq!(b64(b"foobar").as_str(), "Zm9vYmFy");
        assert_eq!(b64(b"fo").as_str(), "Zm8=");
    }

    #[test]
    fn keychain_ids_are_only_ours() {
        assert_eq!(
            keychain_id("kc1:0123456789abcdef0123456789abcdef"),
            Some("0123456789abcdef0123456789abcdef")
        );
        for bad in [
            "kc1:",
            "kc1:-w x",
            "kc1:0123456789abcdef0123456789abcdeg",
            "kc1:0123456789abcdef0123456789abcdef\nadd",
            "AAAA",
        ] {
            assert_eq!(keychain_id(bad), None, "{bad:?}");
        }
        let bytes: Vec<u8> = (0..=255u8).collect();
        assert_eq!(unhex(&hex(&bytes)).unwrap().as_slice(), bytes.as_slice());
        assert!(unhex("abc").is_err() && unhex("zz").is_err());
    }

    #[test]
    fn sealed_to_this_account_and_back() {
        let dir = tempfile::tempdir().unwrap();
        if !available() {
            assert!(create(dir.path()).is_err());
            return;
        }
        let key = create(dir.path()).unwrap();
        let on_disk = fs::read(path(dir.path())).unwrap();
        assert!(!on_disk.windows(32).any(|w| w == key.as_bytes().as_slice()));
        assert_eq!(load(dir.path()).unwrap(), key);
        // The other slot's key is its own file, and one seal never opens as the other.
        let password = create_for(dir.path(), Purpose::Password).unwrap();
        assert_ne!(password, key);
        assert_eq!(load_for(dir.path(), Purpose::Password).unwrap(), password);
        // A key held aside opens as itself, and only replaces the one in use when promoted.
        let next = create_next_for(dir.path(), Purpose::Password).unwrap();
        assert_eq!(load_for(dir.path(), Purpose::Password).unwrap(), password);
        assert_eq!(load_next_for(dir.path(), Purpose::Password).unwrap(), next);
        promote_next_for(dir.path(), Purpose::Password).unwrap();
        assert_eq!(load_for(dir.path(), Purpose::Password).unwrap(), next);
        assert!(load_next_for(dir.path(), Purpose::Password).is_err());
        let google_seal = fs::read(path(dir.path())).unwrap();
        let password_seal = fs::read_to_string(path_for(dir.path(), Purpose::Password)).unwrap();
        fs::write(path_for(dir.path(), Purpose::Password), &google_seal).unwrap();
        assert!(load_for(dir.path(), Purpose::Password).is_err());
        // A changed seal does not open.
        fs::write(path(dir.path()), "AAAA").unwrap();
        assert!(load(dir.path()).is_err());
        // Nothing is left behind (on a Mac, in the Keychain): each key goes with its file.
        forget(
            String::from_utf8_lossy(&google_seal).trim(),
            Purpose::Google,
        );
        forget(password_seal.trim(), Purpose::Password);
        remove_for(dir.path(), Purpose::Password);
        assert!(!exists_for(dir.path(), Purpose::Password));
    }

    /// On a Mac the file holds only the item's name: the key itself is in the Keychain, and
    /// once the item is gone the file opens nothing.
    #[test]
    #[cfg(target_os = "macos")]
    fn on_a_mac_the_key_lives_in_the_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let key = create(dir.path()).unwrap();
        let on_disk = fs::read_to_string(path(dir.path())).unwrap();
        assert!(on_disk.starts_with(KEYCHAIN_PREFIX), "{on_disk}");
        assert!(!on_disk.contains(hex(key.as_bytes()).as_str()));
        // A replaced key takes its item with it; the new one opens.
        let again = create(dir.path()).unwrap();
        assert!(keychain_load(on_disk.trim(), Purpose::Google, OPENING_WAIT).is_err());
        assert_eq!(load(dir.path()).unwrap(), again);
        // A copy of the folder where the item is not (another Mac, another account).
        let copy = fs::read_to_string(path(dir.path())).unwrap();
        remove(dir.path());
        fs::write(path(dir.path()), copy).unwrap();
        assert!(load(dir.path()).is_err());
        remove(dir.path());
    }
}
