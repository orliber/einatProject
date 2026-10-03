//! The half of the Google slot that stays on this computer (D-041).
//!
//! A random key, sealed with Windows DPAPI to her Windows account (`CurrentUser`), kept as
//! `google.computer` in the vault folder. It is not part of a backup, and the sealed file is
//! useless on another computer or under another Windows user. With the key in her Google
//! Drive it opens the Google slot; neither opens anything alone.
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

/// Bound into the seal, so a blob made by another program for another purpose never fits.
const ENTROPY: &str = "dv/google-computer-key/v1";

/// True where this computer can seal a key to the Windows account.
#[must_use]
pub fn available() -> bool {
    cfg!(windows)
}

#[must_use]
pub fn path(dir: &Path) -> PathBuf {
    dir.join(COMPUTER_KEY_FILE)
}

#[must_use]
pub fn exists(dir: &Path) -> bool {
    path(dir).exists()
}

/// A new computer key, sealed and written (replacing an older one).
pub fn create(dir: &Path) -> Result<Key32, VaultError> {
    let key = Key32::random()?;
    let sealed = protect(key.as_bytes())?;
    let tmp = dir.join(format!("{COMPUTER_KEY_FILE}.tmp"));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(sealed.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path(dir))?;
    Ok(key)
}

/// The computer key, unsealed. Fails on another computer or Windows account.
pub fn load(dir: &Path) -> Result<Key32, VaultError> {
    let sealed = fs::read_to_string(path(dir)).map_err(|_| VaultError::NotFound)?;
    let plain = unprotect(sealed.trim())?;
    let bytes: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::Corrupt("computer key length".to_owned()))?;
    Ok(Key32::from_bytes(bytes))
}

pub fn remove(dir: &Path) {
    let _ = fs::remove_file(path(dir));
}

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
fn dpapi(script: &str, input: &str) -> Result<Zeroizing<String>, VaultError> {
    if !available() {
        return Err(VaultError::Refused(
            "sealing to the Windows account is only available on Windows".to_owned(),
        ));
    }
    let full = format!(
        "Add-Type -AssemblyName System.Security; $e=[Text.Encoding]::UTF8.GetBytes('{ENTROPY}'); $b=[Convert]::FromBase64String([Console]::In.ReadToEnd().Trim()); {script}"
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
    let out = child.wait_with_output()?;
    let text = Zeroizing::new(String::from_utf8_lossy(&out.stdout).trim().to_owned());
    if !out.status.success() || text.is_empty() {
        return Err(VaultError::Crypto(
            "the key sealed to this computer could not be opened",
        ));
    }
    Ok(text)
}

fn protect(plain: &[u8; 32]) -> Result<Zeroizing<String>, VaultError> {
    dpapi(
        "[Console]::Out.Write([Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect($b,$e,'CurrentUser')))",
        &b64(plain),
    )
}

fn unprotect(sealed: &str) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let out = dpapi(
        "[Console]::Out.Write([Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Unprotect($b,$e,'CurrentUser')))",
        sealed,
    )?;
    unb64(&out).map(Zeroizing::new)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(bytes: &[u8]) -> Zeroizing<String> {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
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
        // A changed seal does not open.
        fs::write(path(dir.path()), "AAAA").unwrap();
        assert!(load(dir.path()).is_err());
    }
}
