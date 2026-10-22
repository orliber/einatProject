//! Encrypted backup, the restore drill, and restoring on a new computer (D-024).
//!
//! The shell picks the file with the system's own "Save as" / "Open" window and hands the core
//! a path or the file's bytes; the webview never sees either. A backup is sealed with the
//! vault's own keys (see `dv_vault` `store/backup.rs`), so it opens with the password or the
//! printed recovery kit that were in use when it was made.

use std::path::{Path, PathBuf};

use dv_vault::{peek_backup, VaultError, BACKUP_EXTENSION};

use crate::dates::unix_now;
use crate::views::{BackupStatus, StagedBackup};
use crate::{Core, CoreError};

/// A reminder appears when the last backup is older than this.
pub const BACKUP_DAYS: i64 = 7;
/// Far larger than a vault of text can grow; a bigger file is not a backup of ours.
pub const MAX_BACKUP_BYTES: u64 = 1 << 30;
const LAST_BACKUP_KEY: &str = "backup_last_at";
const LAST_DIR_KEY: &str = "backup_last_dir";
const LAST_CHECK_KEY: &str = "backup_last_check_at";
/// When the password or the kit last changed: backups older than this open only with the old one.
const SECRET_CHANGED_KEY: &str = "secret_changed_at";
/// "0" turns the automatic backup off; anything else (or nothing) leaves it on.
pub(crate) const AUTO_KEY: &str = "backup_auto";
/// The start of an automatic backup's file name. Only files named so are ever removed.
const AUTO_PREFIX: &str = "גיבוי אוטומטי כספת האבחון ";
/// Automatic backups kept in the folder; older ones are removed (hers never are).
pub const AUTO_KEEP: usize = 3;
/// After a failed automatic backup (a full drive), the next try waits this long.
const AUTO_RETRY: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// The file name offered in the "Save as" window. A date, nothing about any case.
#[must_use]
pub fn backup_file_name(now: i64) -> String {
    format!(
        "גיבוי כספת האבחון {}.{BACKUP_EXTENSION}",
        crate::dates::iso(crate::dates::date_of(now))
    )
}

/// The name of an automatic backup. A date, nothing about any case.
#[must_use]
pub fn auto_backup_file_name(now: i64) -> String {
    format!(
        "{AUTO_PREFIX}{}.{BACKUP_EXTENSION}",
        crate::dates::iso(crate::dates::date_of(now))
    )
}

/// Remove the automatic backups in `dir` beyond the newest `AUTO_KEEP` (the date in the name
/// sorts them). Files she saved herself are not touched.
pub(crate) fn prune_auto_backups(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let suffix = format!(".{BACKUP_EXTENSION}");
    let mut ours: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(AUTO_PREFIX) && n.ends_with(&suffix))
        })
        .map(|e| e.path())
        .collect();
    ours.sort();
    let extra = ours.len().saturating_sub(AUTO_KEEP);
    for old in ours.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

impl Core {
    pub fn backup_status(&mut self) -> Result<BackupStatus, CoreError> {
        let v = self.vault_ref()?;
        let read = |key: &str| -> Option<i64> { v.setting(key).ok().flatten()?.parse().ok() };
        let last_at = read(LAST_BACKUP_KEY);
        let last_check_at = read(LAST_CHECK_KEY);
        // A corrupt value in the setting must not stop the program (overflow aborts).
        let days_since =
            last_at.map(|t| unix_now().saturating_sub(t).max(0) / dv_domain::dates::DAY_SECS);
        // Cleared by the next backup (see `write_backup`).
        let secret_changed = read(SECRET_CHANGED_KEY).is_some();
        Ok(BackupStatus {
            last_at,
            days_since,
            due: secret_changed || days_since.is_none_or(|d| d >= BACKUP_DAYS),
            secret_changed,
            last_check_at,
            has_cases: v.has_cases()?,
            auto: v.setting(AUTO_KEY)?.as_deref() != Some("0"),
        })
    }

    pub fn set_auto_backup(&mut self, on: bool) -> Result<BackupStatus, CoreError> {
        self.vault_mut()?
            .set_setting(AUTO_KEY, if on { "1" } else { "0" })?;
        self.backup_status()
    }

    /// The folder an automatic backup is due for, if one is: the vault has cases, the
    /// automatic backup is on, a backup is due, no failed try in the last hour, and the folder
    /// of the last backup is there (the removable drive is connected).
    pub(crate) fn auto_backup_due(&mut self) -> Result<Option<PathBuf>, CoreError> {
        if self.vault.is_none()
            || self
                .auto_backup_tried
                .is_some_and(|t| t.elapsed() < AUTO_RETRY)
        {
            return Ok(None);
        }
        let status = self.backup_status()?;
        if !(status.auto && status.due && status.has_cases) {
            return Ok(None);
        }
        self.backup_dir()
    }

    pub(crate) fn mark_secret_changed(&mut self) -> Result<(), CoreError> {
        let now = unix_now().to_string();
        let v = self.vault_mut()?;
        v.set_state(SECRET_CHANGED_KEY, &now)?;
        // Copies in her Drive sealed before this open with the old password or kit: the next
        // copy that goes up is then the only one kept (D-099).
        v.set_state(crate::drive_backup::REKEYED_KEY, &now)?;
        Ok(())
    }

    /// Where the "Save as" / "Open" window starts: the folder of the last backup, if it is
    /// still there (a removable drive may be out).
    pub fn backup_dir(&mut self) -> Result<Option<PathBuf>, CoreError> {
        Ok(self
            .vault_ref()?
            .setting(LAST_DIR_KEY)?
            .map(PathBuf::from)
            .filter(|d| d.is_dir()))
    }

    /// Where a backup to `dest` goes (with the backup extension) and its folder, checked: not
    /// in a folder synced to the cloud, not inside the vault's own folder.
    pub(crate) fn backup_destination(&self, dest: &Path) -> Result<(PathBuf, PathBuf), CoreError> {
        let dest = if dest
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(BACKUP_EXTENSION))
        {
            dest.to_path_buf()
        } else {
            let mut name = dest.as_os_str().to_owned();
            name.push(format!(".{BACKUP_EXTENSION}"));
            PathBuf::from(name)
        };
        let folder = dest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| CoreError::Refused("צריך לבחור תיקייה לגיבוי.".to_owned()))?;
        if let Some(cloud) = dv_vault::env::cloud_synced_component(folder) {
            return Err(CoreError::Refused(format!(
                "התיקייה שנבחרה מסונכרנת לענן ({cloud}). גיבוי נשמר רק במחשב או בדיסק נייד: \
                 מידע על מטופלים לא עולה לענן."
            )));
        }
        let (Ok(folder_real), Ok(vault_real)) = (folder.canonicalize(), self.dir.canonicalize())
        else {
            return Err(CoreError::Needs(
                "התיקייה שנבחרה לא נמצאה. אם זה דיסק נייד, כדאי לבדוק שהוא מחובר.".to_owned(),
                crate::ErrorAction::retry(),
            ));
        };
        if folder_real.starts_with(&vault_real) {
            return Err(CoreError::Refused(
                "גיבוי בתוך תיקיית הכספת לא שומר על כלום. עדיף דיסק נייד.".to_owned(),
            ));
        }
        Ok((dest, folder_real))
    }

    /// A backup in `folder` was written and read back: it counts from now on. `current`: it
    /// opens with today's password and kit (none changed while it was written).
    pub(crate) fn backup_recorded(
        &mut self,
        folder: &Path,
        created_at: i64,
        current: bool,
    ) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        v.set_state(LAST_CHECK_KEY, &unix_now().to_string())?;
        v.set_state(LAST_BACKUP_KEY, &created_at.to_string())?;
        v.set_state(LAST_DIR_KEY, &folder.to_string_lossy())?;
        if current {
            v.set_state(SECRET_CHANGED_KEY, "")?;
        }
        Ok(())
    }

    /// A restore drill passed.
    pub(crate) fn drill_recorded(&mut self) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_state(LAST_CHECK_KEY, &unix_now().to_string())?;
        Ok(())
    }

    /// Hold a backup file the shell read, and say what it is before any password is typed.
    pub fn stage_backup(
        &mut self,
        bytes: Vec<u8>,
        file_name: String,
    ) -> Result<StagedBackup, CoreError> {
        let peek = peek_backup(&bytes)
            .map_err(|_| CoreError::Refused("הקובץ הזה הוא לא גיבוי של כספת האבחון.".to_owned()))?;
        let same_vault = self.vault.as_ref().map(|v| v.vault_id() == peek.vault_id);
        self.staged_backup = Some(bytes);
        Ok(StagedBackup {
            file_name,
            created_at: peek.created_at,
            same_vault,
        })
    }

    pub fn forget_staged_backup(&mut self) {
        self.staged_backup = None;
    }

    /// The chosen file, taken rather than copied (a backup can be large).
    pub(crate) fn take_staged(&mut self) -> Result<Vec<u8>, CoreError> {
        self.staged_backup
            .take()
            .ok_or_else(|| CoreError::Refused("צריך לבחור קובץ גיבוי.".to_owned()))
    }

    /// Count a wrong secret toward the same waiting time as unlocking.
    pub(crate) fn count_failure(&mut self, e: &VaultError) {
        if matches!(e, VaultError::WrongSecret | VaultError::RecoveryKeyInvalid) {
            self.failed_unlocks += 1;
            let wait = 2u64.saturating_pow(self.failed_unlocks.min(6)).min(60);
            self.not_before =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(wait));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offered_file_name_is_a_date_only() {
        assert_eq!(backup_file_name(0), "גיבוי כספת האבחון 1970-01-01.vaultbak");
        // 2026-09-29 12:00 UTC
        assert_eq!(
            backup_file_name(1_790_683_200),
            "גיבוי כספת האבחון 2026-09-29.vaultbak"
        );
    }
}
