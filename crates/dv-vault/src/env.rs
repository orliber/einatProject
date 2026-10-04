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
        // By its full path, so a `powershell.exe` elsewhere on PATH is never the one that runs.
        let powershell = std::env::var_os("SystemRoot").map_or_else(
            || "powershell".to_owned(),
            |root| {
                Path::new(&root)
                    .join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
                    .to_string_lossy()
                    .into_owned()
            },
        );
        return match run(
            &powershell,
            &["-NoProfile", "-NonInteractive", "-Command", script],
        )
        .as_deref()
        .map(str::trim)
        {
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
    fn local_app_data_is_not_synced() {
        let path = PathBuf::from("/Users/x/AppData/Local/il.diagnosticvault.desktop/vault");
        assert!(cloud_synced_component(&path).is_none());
        let megan = PathBuf::from("/home/megan/vault");
        assert!(cloud_synced_component(&megan).is_none());
    }
}
