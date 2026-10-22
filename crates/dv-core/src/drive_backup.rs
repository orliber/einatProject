//! An encrypted copy of each backup in her Google Drive (D-099). Off by default.
//!
//! After every local backup that was written and read back, the same `.vaultbak` file (sealed
//! with the vault's keys: it opens only with her password or her printed kit) goes up to the
//! app's hidden Drive folder, and only the three newest copies stay there. Nothing else leaves.
//!
//! * Turning it on needs her Google account connected on this computer (sign-in with Google
//!   for a forgotten password, D-041), and the same account again in her browser. Google then
//!   gives a lasting permission for the app's hidden folder only; it is kept as a vault secret
//!   (sealed inside the encrypted vault), so it is usable only while the vault is open.
//! * The upload runs with the core free (F2): the backup is recorded under the lock, the file
//!   goes up outside it, and the outcome is written under the lock again. A failed upload never
//!   fails or undoes the local backup: a quiet line in the settings, and the next backup tries
//!   again.
//! * Turning it off forgets the permission at once, then (with the core free) deletes the
//!   copies in her Drive and withdraws the permission at Google.

use std::sync::{Mutex, PoisonError};

use dv_egress::drive_backup as drive;
use dv_egress::google::{self, ClientConfig, GoogleError, Http, Session};
use dv_vault::{AuditEvent, Vault};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::google::{same_account, ComputerKeys};
use crate::{Core, CoreError};

/// The lasting permission, a vault secret (sealed inside the vault).
const TOKEN_SECRET: &str = "drive_backup_token";
/// The backup time of the newest copy that went up.
const LAST_KEY: &str = "drive_backup_last_at";
/// Why the last upload did not go up, or empty.
const PROBLEM_KEY: &str = "drive_backup_problem";

/// One upload or cleanup at a time: a cleanup after turning off waits for an upload that is
/// on its way, then deletes it too.
static DRIVE: Mutex<()> = Mutex::new(());

/// Why the last copy did not go up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DriveProblem {
    /// No internet (or the secure connection failed). The next backup tries again.
    Offline,
    /// The permission at Google was withdrawn: connect again.
    Reconnect,
    /// Google answered something else. The next backup tries again.
    Failed,
}

impl DriveProblem {
    fn as_str(self) -> &'static str {
        match self {
            DriveProblem::Offline => "offline",
            DriveProblem::Reconnect => "reconnect",
            DriveProblem::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "offline" => Some(DriveProblem::Offline),
            "reconnect" => Some(DriveProblem::Reconnect),
            "failed" => Some(DriveProblem::Failed),
            _ => None,
        }
    }

    fn of(e: &GoogleError) -> Self {
        match e {
            GoogleError::Revoked => DriveProblem::Reconnect,
            GoogleError::Offline | GoogleError::Tls => DriveProblem::Offline,
            _ => DriveProblem::Failed,
        }
    }
}

/// What the settings show about the copy in Drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DriveBackupStatus {
    /// This version can talk to Google on this computer (as `GoogleStatus::available`).
    pub available: bool,
    /// Her Google account is connected here (sign-in with Google is on).
    pub connected: bool,
    /// The copy is on. Off by default.
    pub on: bool,
    /// The backup time of the newest copy that went up.
    #[ts(type = "number | null")]
    pub last_at: Option<i64>,
    /// The last copy did not go up, and why.
    pub problem: Option<DriveProblem>,
}

fn refused(text: &str) -> CoreError {
    CoreError::Refused(text.to_owned())
}

fn not_connected() -> CoreError {
    refused(
        "קודם מחברים את חשבון הגוגל: הגדרות ← סיסמה ואבטחה ← כניסה עם גוגל כששוכחים סיסמה. \
         אחר כך אפשר להדליק כאן את העותק בגוגל דרייב.",
    )
}

/// Turning it on, step 2, with the core free, after her browser sign-in
/// ([`google::begin_lasting`]): the account is the one connected to the vault, and Google gave
/// a lasting permission. The short token is not revoked here: at Google that would withdraw
/// the lasting permission with it. It is dropped, and expires within the hour.
pub fn lasting_permission(
    http: &dyn Http,
    mut session: Session,
    vault_id: &str,
    tag: &str,
) -> Result<Zeroizing<String>, CoreError> {
    if !same_account(vault_id, tag, &session) {
        google::revoke(http, session);
        return Err(refused(
            "זה לא חשבון הגוגל שחובר לכספת. אפשר לנסות שוב ולבחור את החשבון שחובר.",
        ));
    }
    match session.take_refresh_token() {
        Some(token) => Ok(token),
        None => {
            google::revoke(http, session);
            Err(refused(
                "גוגל לא נתנה לתוכנה הרשאה קבועה. אפשר לנסות שוב ולאשר בחלון של גוגל.",
            ))
        }
    }
}

/// One copy to upload with the core free. Holds the sealed file and the lasting permission.
pub struct DriveUpload {
    session: u64,
    token: Zeroizing<String>,
    vault_id: String,
    created_at: i64,
    file: Vec<u8>,
}

impl std::fmt::Debug for DriveUpload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveUpload")
            .field("bytes", &self.file.len())
            .finish_non_exhaustive()
    }
}

/// How an upload went, waiting for [`Core::finish_drive_upload`].
#[derive(Debug)]
pub struct DriveUploaded {
    session: u64,
    created_at: i64,
    bytes: u64,
    outcome: Result<(), DriveProblem>,
}

impl DriveUploaded {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.outcome.is_ok()
    }
}

impl DriveUpload {
    /// Upload the sealed file, then keep only the three newest copies. Never touches the core.
    #[must_use]
    pub fn run(self, http: &dyn Http, cfg: &ClientConfig) -> DriveUploaded {
        let _one = DRIVE.lock().unwrap_or_else(PoisonError::into_inner);
        let Self {
            session,
            token,
            vault_id,
            created_at,
            file,
        } = self;
        let bytes = u64::try_from(file.len()).unwrap_or(u64::MAX);
        let outcome = (|| -> Result<(), GoogleError> {
            let s = google::session_from_refresh(http, cfg, &token)?;
            let id = drive::upload_copy(http, &s, &vault_id, created_at, file)?;
            // The copy is up; old ones that stay are deleted after the next backup.
            let _ = drive::keep_latest(http, &s, &vault_id, &id);
            Ok(())
        })()
        .map_err(|e| DriveProblem::of(&e));
        DriveUploaded {
            session,
            created_at,
            bytes,
            outcome,
        }
    }
}

/// After turning off, with the core free: delete the copies and withdraw the permission.
pub struct DriveCleanup {
    token: Zeroizing<String>,
    vault_id: String,
}

impl std::fmt::Debug for DriveCleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveCleanup").finish_non_exhaustive()
    }
}

impl DriveCleanup {
    /// `true` when the copies in her Drive were deleted. Best effort either way: the
    /// permission is forgotten in the vault already, and withdrawn at Google here.
    #[must_use]
    pub fn run(self, http: &dyn Http, cfg: &ClientConfig) -> bool {
        let _one = DRIVE.lock().unwrap_or_else(PoisonError::into_inner);
        let deleted = google::session_from_refresh(http, cfg, &self.token)
            .and_then(|s| drive::delete_all_copies(http, &s, &self.vault_id))
            .is_ok();
        google::revoke_token(http, &self.token);
        deleted
    }
}

impl Core {
    fn drive_token(&self) -> Option<Zeroizing<String>> {
        self.vault
            .as_ref()?
            .secret(TOKEN_SECRET)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty())
    }

    /// The copy is on (the vault holds the lasting permission).
    pub(crate) fn drive_backup_on(&self) -> bool {
        self.drive_token().is_some()
    }

    pub fn drive_backup_status(
        &mut self,
        keys: &dyn ComputerKeys,
    ) -> Result<DriveBackupStatus, CoreError> {
        let google = self.google_status(keys);
        let on = self.drive_backup_on();
        let v = self.vault_ref()?;
        let last_at = v.setting(LAST_KEY)?.and_then(|t| t.parse().ok());
        let problem = if on {
            v.setting(PROBLEM_KEY)?
                .as_deref()
                .and_then(DriveProblem::parse)
        } else {
            None
        };
        Ok(DriveBackupStatus {
            available: google.available,
            connected: google.on,
            on,
            last_at,
            problem,
        })
    }

    /// Turning it on, step 1 (under the lock): her Google account is connected on this
    /// computer. Returns the vault id and the tag of the connected account.
    pub fn drive_backup_turn_on_check(
        &mut self,
        keys: &dyn ComputerKeys,
    ) -> Result<(String, String), CoreError> {
        let vault_id = self.vault_ref()?.vault_id().to_owned();
        if !self.google_status(keys).on {
            return Err(not_connected());
        }
        let (_, tag) = Vault::google_slot(&self.dir)?;
        let tag = tag.ok_or_else(not_connected)?;
        Ok((vault_id, tag))
    }

    /// Turning it on, step 3 (under the lock): keep the lasting permission sealed in the vault.
    /// The next backup goes up.
    pub fn drive_backup_turn_on_finish(&mut self, token: &str) -> Result<(), CoreError> {
        if token.is_empty() {
            return Err(CoreError::Internal("an empty permission".to_owned()));
        }
        let v = self.vault_mut()?;
        v.set_secret(TOKEN_SECRET, token)?;
        v.set_state(PROBLEM_KEY, "")?;
        v.record(AuditEvent::DriveBackupOn, None, &serde_json::json!({}))?;
        Ok(())
    }

    /// Turn it off (under the lock): the permission is forgotten at once, and no further copy
    /// goes up. Returns the cleanup at Google, to run with the core free, when it was on.
    pub fn drive_backup_turn_off(&mut self) -> Result<Option<DriveCleanup>, CoreError> {
        let token = self.drive_token();
        let v = self.vault_mut()?;
        let vault_id = v.vault_id().to_owned();
        v.set_state(PROBLEM_KEY, "")?;
        v.set_state(LAST_KEY, "")?;
        let Some(token) = token else {
            return Ok(None);
        };
        v.delete_secret(TOKEN_SECRET)?;
        v.record(AuditEvent::DriveBackupOff, None, &serde_json::json!({}))?;
        Ok(Some(DriveCleanup { token, vault_id }))
    }

    /// What to upload after a backup was recorded, when the copy is on.
    pub(crate) fn begin_drive_upload(
        &mut self,
        session: u64,
        created_at: i64,
        file: Vec<u8>,
    ) -> Option<DriveUpload> {
        let token = self.drive_token()?;
        let vault_id = self.vault.as_ref()?.vault_id().to_owned();
        Some(DriveUpload {
            session,
            token,
            vault_id,
            created_at,
            file,
        })
    }

    /// Record how the upload went (under the lock). A failure is only noted, for a quiet line
    /// in the settings; the local backup stands as it was.
    pub fn finish_drive_upload(&mut self, up: DriveUploaded) -> Result<(), CoreError> {
        self.same_session(up.session)?;
        // Turned off meanwhile: the cleanup deletes what went up.
        if !self.drive_backup_on() {
            return Ok(());
        }
        let v = self.vault_mut()?;
        match up.outcome {
            Ok(()) => {
                let newer = v
                    .setting(LAST_KEY)?
                    .and_then(|t| t.parse::<i64>().ok())
                    .is_none_or(|t| t <= up.created_at);
                if newer {
                    v.set_state(LAST_KEY, &up.created_at.to_string())?;
                }
                v.set_state(PROBLEM_KEY, "")?;
                v.record(
                    AuditEvent::DriveBackupUploaded,
                    None,
                    &serde_json::json!({ "bytes": up.bytes }),
                )?;
            }
            Err(p) => v.set_state(PROBLEM_KEY, p.as_str())?,
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
