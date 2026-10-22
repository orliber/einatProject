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
//! * The upload runs with the core free (F2), one at a time, the newest waiting one only: the
//!   backup is recorded under the lock, the file goes up outside it, and the outcome is written
//!   under the lock again. A failed upload never fails or undoes the local backup: a quiet line
//!   in the settings, and the next backup tries again. A lock, or turning it off, stops an
//!   upload before its body goes out.
//! * Turning it off forgets the permission at once and keeps it, sealed, only as a cleanup
//!   that is owed: delete the copies in her Drive and withdraw the permission at Google. Until
//!   Google confirms both, the cleanup is tried again while the vault is open.
//! * A restored backup never turns it on: the permission it carries is forgotten, and she
//!   turns the copy on again herself.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dv_egress::drive_backup as drive;
use dv_egress::google::{self, ClientConfig, GoogleError, Http, Session};
use dv_vault::{AuditEvent, Vault};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::google::{same_account, ComputerKeys};
use crate::jobs::Cancel;
use crate::{Core, CoreError};

/// The lasting permission, a vault secret (sealed inside the vault).
const TOKEN_SECRET: &str = "drive_backup_token";
/// A permission she turned off whose cleanup at Google (delete the copies, withdraw it) is
/// not confirmed yet. Sealed like the permission; tried again until Google confirms.
const CLEANUP_SECRET: &str = "drive_backup_cleanup";
/// The backup time of the newest copy that went up.
const LAST_KEY: &str = "drive_backup_last_at";
/// Why the last upload did not go up, or empty.
const PROBLEM_KEY: &str = "drive_backup_problem";
/// When her password or kit last changed, until a copy sealed after it is the only one in her
/// Drive: copies sealed before it open with the old one.
pub(crate) const REKEYED_KEY: &str = "drive_backup_rekeyed";
/// How long a cleanup that did not finish waits before it is tried again.
const CLEANUP_RETRY: Duration = Duration::from_secs(15 * 60);

/// One upload or cleanup at a time: a cleanup after turning off waits for an upload that is
/// on its way, then deletes it too.
static DRIVE: Mutex<()> = Mutex::new(());

/// The newest upload waiting for the one worker, and whether the worker runs. Each upload
/// holds a whole backup in memory, so an older one still waiting is dropped for a newer one.
static QUEUE: Mutex<(Option<DriveUpload>, bool)> = Mutex::new((None, false));

/// Why the last copy did not go up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DriveProblem {
    /// No internet (or the secure connection failed). The next backup tries again.
    Offline,
    /// The permission at Google was withdrawn: connect again.
    Reconnect,
    /// Google answered something else, or the connection could not be set up. The next
    /// backup tries again.
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

/// How turning it off went at Google, as she is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DriveCleanupResult {
    /// It was off, and nothing was owed at Google.
    Off,
    /// The copies in her Drive were deleted and the permission withdrawn.
    Cleaned,
    /// Not finished (no internet, or Google failed): no copy goes up any more, and the
    /// deleting and withdrawing are tried again while the vault is open.
    Pending,
    /// The permission was already withdrawn at Google, so the copies there can no longer be
    /// reached from this program. She can delete them in her Drive's settings.
    Withdrawn,
}

impl DriveCleanupResult {
    fn as_str(self) -> &'static str {
        match self {
            DriveCleanupResult::Off => "off",
            DriveCleanupResult::Cleaned => "cleaned",
            DriveCleanupResult::Pending => "pending",
            DriveCleanupResult::Withdrawn => "withdrawn",
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
    /// Turned off, but deleting the copies and withdrawing the permission at Google is not
    /// confirmed yet (tried again while the vault is open).
    pub cleanup_pending: bool,
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

/// A lasting permission that was given but is not kept (the vault locked during the sign-in,
/// or the account changed): withdrawn at Google, so no unused permission stays there.
pub fn withdraw_unkept(http: &dyn Http, token: &str) {
    let _ = google::revoke_token(http, token);
}

/// Queue an upload for the one worker; a newer one replaces one still waiting. `true` when
/// the caller must start the worker, which takes uploads with [`next_upload`] until none is
/// left.
pub fn queue_upload(up: DriveUpload) -> bool {
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    q.0 = Some(up);
    !std::mem::replace(&mut q.1, true)
}

/// The worker's next upload. `None` ends the worker (the next [`queue_upload`] starts one).
#[must_use]
pub fn next_upload() -> Option<DriveUpload> {
    let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    let next = q.0.take();
    if next.is_none() {
        q.1 = false;
    }
    next
}

/// One copy to upload with the core free. Holds the sealed file and the lasting permission.
pub struct DriveUpload {
    session: u64,
    /// Set by a lock: the body is not sent after it.
    cancel: Cancel,
    /// Moves when the copy turns off (or on): the body is not sent after it.
    generation: Arc<AtomicU64>,
    at: u64,
    token: Zeroizing<String>,
    vault_id: String,
    created_at: i64,
    file: Vec<u8>,
    /// How many copies stay (one after her password or kit changed).
    keep: usize,
    /// The password change this upload follows, cleared once only its copy is left.
    rekeyed: Option<String>,
}

impl std::fmt::Debug for DriveUpload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveUpload")
            .field("bytes", &self.file.len())
            .finish_non_exhaustive()
    }
}

enum Uploaded {
    /// The copy is up; `pruned` once the older ones beyond the number kept were deleted.
    Up {
        pruned: bool,
    },
    Failed(DriveProblem),
    /// Locked or turned off before the body went out: nothing to record.
    Stopped,
}

/// How an upload went, waiting for [`Core::finish_drive_upload`].
pub struct DriveUploaded {
    session: u64,
    created_at: i64,
    bytes: u64,
    rekeyed: Option<String>,
    outcome: Uploaded,
}

impl std::fmt::Debug for DriveUploaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveUploaded")
            .field("ok", &self.ok())
            .finish_non_exhaustive()
    }
}

impl DriveUploaded {
    #[must_use]
    pub fn ok(&self) -> bool {
        matches!(self.outcome, Uploaded::Up { .. })
    }
}

impl DriveUpload {
    fn wanted(cancel: &Cancel, generation: &AtomicU64, at: u64) -> bool {
        !cancel.is_set() && generation.load(Ordering::Acquire) == at
    }

    /// Upload the sealed file, then keep only the newest copies. Never touches the core.
    /// Checked after waiting for its turn and again right before the body goes out: a lock or
    /// turning it off meanwhile stops it there.
    #[must_use]
    pub fn run(self, http: &dyn Http, cfg: &ClientConfig) -> DriveUploaded {
        let _one = DRIVE.lock().unwrap_or_else(PoisonError::into_inner);
        let Self {
            session,
            cancel,
            generation,
            at,
            token,
            vault_id,
            created_at,
            file,
            keep,
            rekeyed,
        } = self;
        let bytes = u64::try_from(file.len()).unwrap_or(u64::MAX);
        let still = || Self::wanted(&cancel, &generation, at);
        let outcome = if still() {
            let s = google::session_from_refresh(http, cfg, &token);
            // The lasting permission is not held for the length of the upload.
            drop(token);
            let up = s.and_then(|s| {
                let id = drive::upload_copy(http, &s, &vault_id, created_at, file, &still)?;
                // The copy is up; old ones that stay are deleted after the next backup.
                let pruned = drive::keep_latest(http, &s, &vault_id, &id, keep).is_ok();
                Ok(pruned)
            });
            match up {
                Ok(pruned) => Uploaded::Up { pruned },
                Err(GoogleError::Cancelled) => Uploaded::Stopped,
                Err(e) => Uploaded::Failed(DriveProblem::of(&e)),
            }
        } else {
            Uploaded::Stopped
        };
        DriveUploaded {
            session,
            created_at,
            bytes,
            rekeyed,
            outcome,
        }
    }

    /// The connection to Google could not be set up on this computer: noted as a failure,
    /// so the switch does not say "on" with nothing going up.
    #[must_use]
    pub fn not_started(self) -> DriveUploaded {
        DriveUploaded {
            session: self.session,
            created_at: self.created_at,
            bytes: 0,
            rekeyed: None,
            outcome: Uploaded::Failed(DriveProblem::Failed),
        }
    }
}

/// After turning off, with the core free: delete the copies and withdraw the permission.
pub struct DriveCleanup {
    session: u64,
    token: Zeroizing<String>,
    vault_id: String,
    generation: Arc<AtomicU64>,
    at: u64,
}

impl std::fmt::Debug for DriveCleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveCleanup").finish_non_exhaustive()
    }
}

enum Cleaned {
    Done(DriveCleanupResult),
    /// Turned on again meanwhile: the new permission is the same grant, so withdrawing the old
    /// one would withdraw it too. Not withdrawn.
    Superseded,
}

/// How a cleanup went, waiting for [`Core::finish_drive_cleanup`].
pub struct DriveCleaned {
    session: u64,
    token: Zeroizing<String>,
    outcome: Cleaned,
}

impl std::fmt::Debug for DriveCleaned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveCleaned").finish_non_exhaustive()
    }
}

impl DriveCleanup {
    /// Delete this vault's copies, then withdraw the permission. Done only when Google
    /// confirms both; anything short of that is tried again later.
    #[must_use]
    pub fn run(self, http: &dyn Http, cfg: &ClientConfig) -> DriveCleaned {
        let _one = DRIVE.lock().unwrap_or_else(PoisonError::into_inner);
        let moved = || self.generation.load(Ordering::Acquire) != self.at;
        let outcome = if moved() {
            Cleaned::Superseded
        } else {
            match google::session_from_refresh(http, cfg, &self.token) {
                Err(GoogleError::Revoked) => Cleaned::Done(DriveCleanupResult::Withdrawn),
                Err(_) => Cleaned::Done(DriveCleanupResult::Pending),
                Ok(s) => match drive::delete_all_copies(http, &s, &self.vault_id) {
                    Err(_) => Cleaned::Done(DriveCleanupResult::Pending),
                    Ok(_) if moved() => Cleaned::Superseded,
                    Ok(_) => match google::revoke_token(http, &self.token) {
                        Ok(()) => Cleaned::Done(DriveCleanupResult::Cleaned),
                        Err(_) => Cleaned::Done(DriveCleanupResult::Pending),
                    },
                },
            }
        };
        DriveCleaned {
            session: self.session,
            token: self.token,
            outcome,
        }
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

    fn drive_cleanup_token(&self) -> Option<Zeroizing<String>> {
        self.vault
            .as_ref()?
            .secret(CLEANUP_SECRET)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty())
    }

    /// Moves the generation: an upload or cleanup begun before it stops (see their `run`).
    fn drive_moved(&self) -> u64 {
        self.drive_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// The copy is on (the vault holds the lasting permission).
    #[must_use]
    pub fn drive_backup_on(&self) -> bool {
        self.drive_token().is_some()
    }

    pub fn drive_backup_status(
        &mut self,
        keys: &dyn ComputerKeys,
    ) -> Result<DriveBackupStatus, CoreError> {
        let google = self.google_status(keys);
        let on = self.drive_backup_on();
        let cleanup_pending = self.drive_cleanup_token().is_some();
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
            cleanup_pending,
        })
    }

    /// Turning it on, step 1 (under the lock): her Google account is connected on this
    /// computer, and the copy is not on already. Returns the vault id and the tag of the
    /// connected account. A cleanup still running from an earlier turning off does not
    /// withdraw the permission she is about to give (the same grant at Google).
    pub fn drive_backup_turn_on_check(
        &mut self,
        keys: &dyn ComputerKeys,
    ) -> Result<(String, String), CoreError> {
        let vault_id = self.vault_ref()?.vault_id().to_owned();
        if self.drive_backup_on() {
            return Err(refused("העותק בגוגל דרייב כבר דלוק."));
        }
        if !self.google_status(keys).on {
            return Err(not_connected());
        }
        let (_, tag) = Vault::google_slot(&self.dir)?;
        let tag = tag.ok_or_else(not_connected)?;
        self.drive_moved();
        Ok((vault_id, tag))
    }

    /// Turning it on, step 3 (under the lock): Google is still connected here with the same
    /// account, so keep the lasting permission sealed in the vault. The next backup goes up.
    /// On an error the caller withdraws the new permission ([`withdraw_unkept`]); when the copy
    /// is on already (two turn-ons at once), the one kept stays and the new one, the same grant
    /// at Google, is let go without withdrawing it.
    pub fn drive_backup_turn_on_finish(
        &mut self,
        keys: &dyn ComputerKeys,
        tag: &str,
        token: &str,
    ) -> Result<(), CoreError> {
        if token.is_empty() {
            return Err(CoreError::Internal("an empty permission".to_owned()));
        }
        self.vault_ref()?;
        if self.drive_backup_on() {
            return Ok(());
        }
        let (_, connected) = Vault::google_slot(&self.dir)?;
        if connected.as_deref() != Some(tag) || !keys.exists(&self.dir) {
            return Err(not_connected());
        }
        self.drive_moved();
        let owed = self.drive_cleanup_token().is_some();
        let v = self.vault_mut()?;
        v.set_secret(TOKEN_SECRET, token)?;
        // A cleanup still owed would withdraw this new permission with the old one (the same
        // grant at Google). Its copies are this vault's, and the next uploads keep only the
        // newest.
        if owed {
            v.delete_secret(CLEANUP_SECRET)?;
        }
        v.set_state(PROBLEM_KEY, "")?;
        v.record(AuditEvent::DriveBackupOn, None, &serde_json::json!({}))?;
        Ok(())
    }

    /// Turn it off (under the lock): the permission is forgotten at once, and no further copy
    /// goes up (an upload on its way stops before its body). The permission stays sealed only
    /// as the cleanup owed at Google, returned to run with the core free. Also when it was
    /// already off but a cleanup is still owed, so a second click tries it again.
    pub fn drive_backup_turn_off(&mut self) -> Result<Option<DriveCleanup>, CoreError> {
        let session = self.session;
        let v = self.vault_mut()?;
        let read = v.secret(TOKEN_SECRET);
        // Also when it could not be read: the permission must not stay.
        if !matches!(read, Ok(None)) {
            v.delete_secret(TOKEN_SECRET)?;
        }
        let token = read.ok().flatten().filter(|t| !t.is_empty());
        v.set_state(PROBLEM_KEY, "")?;
        v.set_state(LAST_KEY, "")?;
        if let Some(token) = &token {
            v.set_secret(CLEANUP_SECRET, token)?;
            v.record(AuditEvent::DriveBackupOff, None, &serde_json::json!({}))?;
        }
        let at = self.drive_moved();
        let Some(owed) = self.drive_cleanup_token() else {
            return Ok(None);
        };
        self.drive_cleanup_at = Some(Instant::now());
        Ok(Some(DriveCleanup {
            session,
            token: owed,
            vault_id: self.vault_ref()?.vault_id().to_owned(),
            generation: Arc::clone(&self.drive_generation),
            at,
        }))
    }

    /// The cleanup still owed at Google, to try again with the core free: after an unlock,
    /// and then at most every 15 minutes while the vault is open. The shell's timer asks.
    pub fn take_drive_cleanup(&mut self) -> Option<DriveCleanup> {
        if self
            .drive_cleanup_at
            .is_some_and(|t| t.elapsed() < CLEANUP_RETRY)
        {
            return None;
        }
        let token = self.drive_cleanup_token()?;
        let vault_id = self.vault.as_ref()?.vault_id().to_owned();
        self.drive_cleanup_at = Some(Instant::now());
        Some(DriveCleanup {
            session: self.session,
            token,
            vault_id,
            generation: Arc::clone(&self.drive_generation),
            at: self.drive_generation.load(Ordering::Acquire),
        })
    }

    /// Record how a cleanup went (under the lock). Done only when Google confirmed it, or
    /// when the permission is gone there already; otherwise it stays owed.
    pub fn finish_drive_cleanup(
        &mut self,
        done: DriveCleaned,
    ) -> Result<DriveCleanupResult, CoreError> {
        self.same_session(done.session)?;
        let ours = self
            .drive_cleanup_token()
            .is_some_and(|t| t.as_str() == done.token.as_str());
        let result = match done.outcome {
            Cleaned::Superseded => {
                // On again with a new permission: nothing is owed any more. Off again: still
                // owed, tried later.
                if ours && self.drive_backup_on() {
                    self.vault_mut()?.delete_secret(CLEANUP_SECRET)?;
                }
                return Ok(if self.drive_cleanup_token().is_some() {
                    DriveCleanupResult::Pending
                } else {
                    DriveCleanupResult::Off
                });
            }
            Cleaned::Done(r) => r,
        };
        let v = self.vault_mut()?;
        if ours && result != DriveCleanupResult::Pending {
            v.delete_secret(CLEANUP_SECRET)?;
        }
        v.record(
            AuditEvent::DriveBackupCleanup,
            None,
            &serde_json::json!({ "result": result.as_str() }),
        )?;
        Ok(result)
    }

    /// What to upload after a backup was recorded, when the copy is on.
    pub(crate) fn begin_drive_upload(
        &mut self,
        session: u64,
        created_at: i64,
        file: Vec<u8>,
    ) -> Option<DriveUpload> {
        let token = self.drive_token()?;
        let v = self.vault.as_ref()?;
        let vault_id = v.vault_id().to_owned();
        let rekeyed = v
            .setting(REKEYED_KEY)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty());
        Some(DriveUpload {
            session,
            cancel: self.cancel.clone(),
            generation: Arc::clone(&self.drive_generation),
            at: self.drive_generation.load(Ordering::Acquire),
            token,
            vault_id,
            created_at,
            file,
            keep: if rekeyed.is_some() { 1 } else { drive::KEEP },
            rekeyed,
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
            Uploaded::Stopped => {}
            Uploaded::Up { pruned } => {
                let newer = v
                    .setting(LAST_KEY)?
                    .and_then(|t| t.parse::<i64>().ok())
                    .is_none_or(|t| t <= up.created_at);
                if newer {
                    v.set_state(LAST_KEY, &up.created_at.to_string())?;
                }
                // Only the copy sealed after her new password is left: done with the old ones,
                // unless it changed again meanwhile.
                if pruned && up.rekeyed.is_some() && v.setting(REKEYED_KEY)? == up.rekeyed {
                    v.set_state(REKEYED_KEY, "")?;
                }
                v.set_state(PROBLEM_KEY, "")?;
                v.record(
                    AuditEvent::DriveBackupUploaded,
                    None,
                    &serde_json::json!({ "bytes": up.bytes }),
                )?;
            }
            Uploaded::Failed(p) => {
                v.set_state(PROBLEM_KEY, p.as_str())?;
                v.record(
                    AuditEvent::DriveBackupFailed,
                    None,
                    &serde_json::json!({ "problem": p.as_str() }),
                )?;
            }
        }
        Ok(())
    }

    /// Right after a restore: a backup taken while the copy was on carries its permission,
    /// and a restore must never turn it on without her. Forgotten here; she turns it on again
    /// herself. A cleanup the backup still owed stays owed.
    pub(crate) fn drive_backup_forget_after_restore(&mut self) -> Result<(), CoreError> {
        let had = self.drive_token().is_some();
        let v = self.vault_mut()?;
        if had {
            v.delete_secret(TOKEN_SECRET)?;
            v.record(
                AuditEvent::DriveBackupOff,
                None,
                &serde_json::json!({ "restore": true }),
            )?;
        }
        v.set_state(PROBLEM_KEY, "")?;
        v.set_state(LAST_KEY, "")?;
        v.set_state(REKEYED_KEY, "")?;
        self.drive_moved();
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
