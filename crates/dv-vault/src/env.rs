//! Environment checks: cloud-synced folders (refuse) and disk encryption (remind, D-013).

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Folder names used by sync clients. A vault inside one would be copied to the cloud.
const SYNCED_MARKERS: &[&str] = &[
    "dropbox",
    "onedrive",
    "icloud drive",
    "mobile documents",
    "com~apple~clouddocs",
    "google drive",
    "googledrive",
    "my drive",
    "box sync",
    "pcloud",
    "mega",
    "syncthing",
    "nextcloud",
];

/// The component of `path` that looks like a cloud-sync folder, if any.
#[must_use]
pub fn cloud_synced_component(path: &Path) -> Option<String> {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .find(|c| {
            SYNCED_MARKERS.iter().any(|m| {
                c == m || c.starts_with(&format!("{m} ")) || c.starts_with(&format!("{m}-"))
            })
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskEncryption {
    On,
    Off,
    Unknown,
}

/// Best-effort, no admin rights needed. Unknown is shown as a gentle reminder.
#[must_use]
pub fn disk_encryption() -> DiskEncryption {
    if cfg!(target_os = "macos") {
        return match run("fdesetup", &["status"]) {
            Some(out) if out.contains("FileVault is On") => DiskEncryption::On,
            Some(out) if out.contains("FileVault is Off") => DiskEncryption::Off,
            _ => DiskEncryption::Unknown,
        };
    }
    if cfg!(target_os = "windows") {
        let script = "(New-Object -ComObject Shell.Application).NameSpace('C:').Self.ExtendedProperty('System.Volume.BitLockerProtection')";
        return match powershell(script).as_deref().map(str::trim) {
            // 1 = on, 3 = encrypting, 6 = on (locked); 2 = off, 4 = decrypting, 5 = suspended.
            Some("1" | "3" | "6") => DiskEncryption::On,
            Some("2" | "4" | "5") => DiskEncryption::Off,
            _ => DiskEncryption::Unknown,
        };
    }
    match run("lsblk", &["-o", "TYPE", "-n"]) {
        Some(out) if out.lines().any(|l| l.trim() == "crypt") => DiskEncryption::On,
        Some(_) => DiskEncryption::Off,
        None => DiskEncryption::Unknown,
    }
}

/// The version of the operating system, for "בדיקת מערכת" (D-142): `Windows 10.0.22631.0`,
/// `macOS 14.5`, `Linux 6.8.0`. Digits and dots only after the name; `None` when unknown.
#[must_use]
pub fn os_version() -> Option<String> {
    let (name, out) = if cfg!(target_os = "windows") {
        (
            "Windows",
            powershell("[System.Environment]::OSVersion.Version.ToString()")?,
        )
    } else if cfg!(target_os = "macos") {
        ("macOS", run("sw_vers", &["-productVersion"])?)
    } else {
        ("Linux", run("uname", &["-r"])?)
    };
    version_digits(&out).map(|v| format!("{name} {v}"))
}

/// The leading run of digits and dots ("6.8.0" of "6.8.0-45-generic"), at most 24 long.
fn version_digits(out: &str) -> Option<String> {
    let v: String = out
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .take(24)
        .collect();
    v.chars().next().filter(char::is_ascii_digit).map(|_| v)
}

/// The free space, in bytes, on the drive that holds `path` (the vault's folder), for
/// "בדיקת מערכת". Asked of the system, never by writing anything; `None` when unknown.
#[must_use]
pub fn free_bytes(path: &Path) -> Option<u64> {
    if cfg!(target_os = "windows") {
        // Only the drive's letter goes into the script, never the path.
        let letter = drive_letter(path)?;
        let script = format!("[System.IO.DriveInfo]::new('{letter}').AvailableFreeSpace");
        return powershell(&script)?.trim().parse().ok();
    }
    let path = path.to_str()?;
    // POSIX output: a header, then "name blocks used available capacity mount" in KiB.
    let out = run("df", &["-Pk", "--", path])?;
    let available: u64 = out
        .lines()
        .nth(1)?
        .split_whitespace()
        .nth(3)?
        .parse()
        .ok()?;
    available.checked_mul(1024)
}

/// `C` of `C:\Users\…` (also of `\\?\C:\…`); `None` for any other kind of path.
fn drive_letter(path: &Path) -> Option<char> {
    use std::path::{Component, Prefix};
    match path.components().next()? {
        Component::Prefix(p) => match p.kind() {
            Prefix::Disk(b) | Prefix::VerbatimDisk(b) => {
                Some(char::from(b)).filter(char::is_ascii_alphabetic)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Windows PowerShell by its full path, so a `powershell.exe` elsewhere on PATH is never the
/// one that runs.
fn powershell(script: &str) -> Option<String> {
    let powershell = std::env::var_os("SystemRoot").map_or_else(
        || "powershell".to_owned(),
        |root| {
            Path::new(&root)
                .join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
                .to_string_lossy()
                .into_owned()
        },
    );
    run(
        &powershell,
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    // The program has no console on Windows, so without this a black window flashes on
    // every start while PowerShell answers.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_common_sync_folders() {
        for p in [
            r"C:\Users\x\OneDrive\Documents\vault",
            r"C:\Users\x\OneDrive - Clinic\vault",
            "/Users/x/Library/Mobile Documents/com~apple~CloudDocs/vault",
            "/Users/x/Dropbox/vault",
            "/Users/x/Google Drive/My Drive/vault",
        ] {
            let path = PathBuf::from(p.replace('\\', "/"));
            assert!(cloud_synced_component(&path).is_some(), "{p}");
        }
    }

    #[test]
    fn a_version_is_digits_and_dots_only() {
        assert_eq!(
            version_digits("10.0.22631.0\r\n").as_deref(),
            Some("10.0.22631.0")
        );
        assert_eq!(version_digits("6.8.0-45-generic").as_deref(), Some("6.8.0"));
        assert_eq!(version_digits(r"C:\Users"), None);
        assert_eq!(version_digits(""), None);
        assert_eq!(version_digits(".5"), None);
    }

    #[test]
    fn only_a_drive_letter_is_taken_from_a_path() {
        // Paths are read the Windows way only on Windows; elsewhere there is no drive.
        if cfg!(windows) {
            assert_eq!(drive_letter(Path::new(r"C:\Users\x\vault")), Some('C'));
            assert_eq!(drive_letter(Path::new(r"\\server\share\vault")), None);
        } else {
            assert_eq!(drive_letter(Path::new("/home/x/vault")), None);
        }
    }

    #[test]
    fn free_space_is_read_where_the_system_can_say() {
        let here = std::env::temp_dir();
        if !cfg!(windows) && Command::new("df").arg("-Pk").output().is_ok() {
            assert!(free_bytes(&here).is_some_and(|b| b > 0));
        }
    }

    #[test]
    fn local_app_data_is_not_synced() {
        let path = PathBuf::from("/Users/x/AppData/Local/il.diagnosticvault.desktop/vault");
        assert!(cloud_synced_component(&path).is_none());
        let megan = PathBuf::from("/home/megan/vault");
        assert!(cloud_synced_component(&megan).is_none());
    }
}
