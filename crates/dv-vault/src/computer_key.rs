//! Keys that stay on this computer: the half of the Google slot (D-041), and the half of the
//! password slot once it is bound to this computer (D-076).
//!
//! Each is a random key, sealed with Windows DPAPI to her Windows account (`CurrentUser`),
//! kept in the vault folder (`google.computer`, `vault.computer`), each with its own seal. It
//! is not part of a backup, and the sealed file is useless on another computer or under
//! another Windows user. With the key in her Google Drive, or with her password, it opens its
//! slot; neither opens anything alone.
//!
//! No new dependency and no `unsafe`: DPAPI is reached through Windows PowerShell (by its
//! full path, like the disk-encryption check), with the secret on stdin, never on the
//! command line. Elsewhere than Windows the feature is not offered.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// True where this computer can seal a key to the Windows account.
#[must_use]
pub fn available() -> bool {
    cfg!(windows)
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
    fs::rename(&tmp, path_for(dir, purpose))?;
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
    fs::rename(&tmp, next_path(dir, purpose))?;
    Ok(key)
}

/// The key held aside, unsealed (for an opening after the slot was written but before the
/// key was promoted).
pub fn load_next_for(dir: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    load_from(&next_path(dir, purpose), purpose)
}

/// The key held aside becomes the one in use.
pub fn promote_next_for(dir: &Path, purpose: Purpose) -> Result<(), VaultError> {
    fs::rename(next_path(dir, purpose), path_for(dir, purpose))?;
    Ok(())
}

fn next_path(dir: &Path, purpose: Purpose) -> PathBuf {
    dir.join(format!("{}.next", purpose.file()))
}

/// The computer key, unsealed. Fails on another computer or Windows account.
pub fn load_for(dir: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    load_from(&path_for(dir, purpose), purpose)
}

fn load_from(file: &Path, purpose: Purpose) -> Result<Key32, VaultError> {
    let sealed = fs::read_to_string(file).map_err(|_| VaultError::NotFound)?;
    let plain = unprotect(sealed.trim(), purpose)?;
    let bytes: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::Corrupt("computer key length".to_owned()))?;
    Ok(Key32::from_bytes(bytes))
}

pub fn remove_for(dir: &Path, purpose: Purpose) {
    let _ = fs::remove_file(path_for(dir, purpose));
}

/// Longest wait for the sealing or unsealing child. A first PowerShell after the computer
/// starts, beside a busy antivirus, took over 20 s on a CI runner; past this it is `Busy`.
const DPAPI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

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
fn dpapi(script: &str, input: &str, purpose: Purpose) -> Result<Zeroizing<String>, VaultError> {
    if !available() {
        return Err(VaultError::Refused(
            "sealing to the Windows account is only available on Windows".to_owned(),
        ));
    }
    let entropy = purpose.entropy();
    let full = format!(
        "Add-Type -AssemblyName System.Security; $e=[Text.Encoding]::UTF8.GetBytes('{entropy}'); $b=[Convert]::FromBase64String([Console]::In.ReadToEnd().Trim()); {script}"
    );
    let mut cmd = Command::new(powershell());
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &full])
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
        stdin.write_all(input.as_bytes())?;
    }
    // A PowerShell that never comes back (an antivirus holding it, a locked-down shell) is
    // stopped and the key is taken as not here: it runs on every opening, under the core's
    // lock, so it must not freeze the program (review of stage 5, S6). Fail-closed.
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
        if started.elapsed() > DPAPI_TIMEOUT {
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

fn protect(plain: &[u8; 32], purpose: Purpose) -> Result<Zeroizing<String>, VaultError> {
    dpapi(
        "[Console]::Out.Write([Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect($b,$e,'CurrentUser')))",
        &b64(plain),
        purpose,
    )
}

fn unprotect(sealed: &str, purpose: Purpose) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let out = dpapi(
        "[Console]::Out.Write([Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Unprotect($b,$e,'CurrentUser')))",
        sealed,
        purpose,
    )?;
    unb64(&out).map(Zeroizing::new)
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
    fn sealed_to_this_windows_account_and_back() {
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
        fs::write(path_for(dir.path(), Purpose::Password), &google_seal).unwrap();
        assert!(load_for(dir.path(), Purpose::Password).is_err());
        // A changed seal does not open.
        fs::write(path(dir.path()), "AAAA").unwrap();
        assert!(load(dir.path()).is_err());
    }
}
