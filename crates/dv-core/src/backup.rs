//! Encrypted backup, the restore drill, and restoring on a new computer (D-024).
//!
//! The shell picks the file with the system's own "Save as" / "Open" window and hands the core
//! a path or the file's bytes; the webview never sees either. A backup is sealed with the
//! vault's own keys (see `dv_vault` `store/backup.rs`), so it opens with the password or the
//! printed recovery kit that were in use when it was made.

use std::path::{Path, PathBuf};

use dv_vault::{peek_backup, Secret, Vault, VaultError, BACKUP_EXTENSION};

use crate::dates::unix_now;
use crate::views::{BackupCheckView, BackupDone, BackupStatus, StagedBackup};
use crate::{AppStatus, Core, CoreError};

/// A reminder appears when the last backup is older than this.
pub const BACKUP_DAYS: i64 = 7;
/// Far larger than a vault of text can grow; a bigger file is not a backup of ours.
pub const MAX_BACKUP_BYTES: u64 = 1 << 30;
const LAST_BACKUP_KEY: &str = "backup_last_at";
const LAST_DIR_KEY: &str = "backup_last_dir";
const LAST_CHECK_KEY: &str = "backup_last_check_at";
/// When the password or the kit last changed: backups older than this open only with the old one.
const SECRET_CHANGED_KEY: &str = "secret_changed_at";

/// The file name offered in the "Save as" window. A date, nothing about any case.
#[must_use]
pub fn backup_file_name(now: i64) -> String {
    format!(
        "גיבוי כספת האבחון {}.{BACKUP_EXTENSION}",
        crate::dates::iso(crate::dates::date_of(now))
    )
}

impl Core {
    pub fn backup_status(&mut self) -> Result<BackupStatus, CoreError> {
        let v = self.vault_ref()?;
        let read = |key: &str| -> Option<i64> { v.setting(key).ok().flatten()?.parse().ok() };
        let last_at = read(LAST_BACKUP_KEY);
        let last_check_at = read(LAST_CHECK_KEY);
        let days_since = last_at.map(|t| (unix_now() - t).max(0) / 86_400);
        // Cleared by the next backup (see `write_backup`).
        let secret_changed = read(SECRET_CHANGED_KEY).is_some();
        Ok(BackupStatus {
            last_at,
            days_since,
            due: secret_changed || days_since.is_none_or(|d| d >= BACKUP_DAYS),
            secret_changed,
            last_check_at,
            has_cases: !v.list_cases()?.is_empty() || !v.list_trash()?.is_empty(),
        })
    }

    pub(crate) fn mark_secret_changed(&mut self) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(SECRET_CHANGED_KEY, &unix_now().to_string())?;
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

    /// Write a backup to `dest` (chosen by Einat in the "Save as" window).
    pub fn write_backup(&mut self, dest: &Path) -> Result<BackupDone, CoreError> {
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
            return Err(CoreError::Refused(
                "התיקייה שנבחרה לא נמצאה. אם זה דיסק נייד, כדאי לבדוק שהוא מחובר.".to_owned(),
            ));
        };
        if folder_real.starts_with(&vault_real) {
            return Err(CoreError::Refused(
                "גיבוי בתוך תיקיית הכספת לא שומר על כלום. עדיף דיסק נייד.".to_owned(),
            ));
        }
        let v = self.vault_mut()?;
        let info = v.write_backup(&dest)?;
        v.set_setting(LAST_BACKUP_KEY, &info.created_at.to_string())?;
        v.set_setting(LAST_DIR_KEY, &folder_real.to_string_lossy())?;
        v.set_setting(SECRET_CHANGED_KEY, "")?;
        Ok(BackupDone {
            path: dest.display().to_string(),
            bytes: info.bytes,
            created_at: info.created_at,
        })
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

    fn staged(&self) -> Result<&[u8], CoreError> {
        self.staged_backup
            .as_deref()
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

    /// Restore drill: open the chosen backup in a scratch folder with the password, count what
    /// it holds, and remove it again. Proves both the file and the password work.
    pub fn check_staged_backup(&mut self, password: &str) -> Result<BackupCheckView, CoreError> {
        self.check_backoff()?;
        let bytes = self.staged()?.to_vec();
        let v = self.vault_mut()?;
        if peek_backup(&bytes)?.vault_id != v.vault_id() {
            return Err(CoreError::Refused(
                "זה גיבוי של כספת אחרת, לא של הכספת הזו.".to_owned(),
            ));
        }
        let result = v.check_backup(&bytes, Secret::Password(password));
        let check = match result {
            Ok(check) => check,
            Err(e) => {
                self.count_failure(&e);
                return Err(e.into());
            }
        };
        self.failed_unlocks = 0;
        let v = self.vault_mut()?;
        v.set_setting(LAST_CHECK_KEY, &unix_now().to_string())?;
        self.staged_backup = None;
        Ok(BackupCheckView {
            created_at: check.created_at,
            cases: check.cases,
            integrity_ok: check.integrity_ok,
        })
    }

    /// On a computer with no vault: restore the chosen backup and open it.
    pub fn restore_staged_backup(
        &mut self,
        password: Option<&str>,
        recovery_key: Option<&str>,
    ) -> Result<AppStatus, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        if Vault::exists(&self.dir) {
            return Err(CoreError::Refused(
                "כבר יש כספת במחשב הזה. שחזור מגיבוי נעשה רק במחשב בלי כספת, כדי לא לדרוס עבודה."
                    .to_owned(),
            ));
        }
        let secret = match (password, recovery_key) {
            (Some(p), _) => Secret::Password(p),
            (None, Some(r)) => Secret::Recovery(r),
            (None, None) => return Err(CoreError::Refused("צריך סיסמה או ערכת שחזור.".to_owned())),
        };
        let bytes = self.staged()?.to_vec();
        // A failed restore removes what it wrote; after_unlock counts a wrong secret.
        let result = Vault::restore_backup(&bytes, &self.dir, secret).map(|(v, _)| v);
        if result.is_ok() {
            self.staged_backup = None;
        }
        self.after_unlock(result)
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
