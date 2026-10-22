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
//! * While it is on, or a cleanup is owed, nothing else withdraws the app's grant at Google:
//!   the grant is one per account, so a withdrawal by a sign-in's tidy-up would strand the
//!   cleanup (the copies could no longer be deleted). The cleanup withdraws it itself.
//! * A restored backup never turns it on: the permission it carries is forgotten at once and
//!   withdrawn at Google (tried again like a cleanup), while its copies stay: they may be her
//!   only copies away from this computer. The settings say that turning the copy on and off
//!   deletes them.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dv_egress::drive_backup as drive;
use dv_egress::google::{self, ClientConfig, GoogleError, Http, Session};
use dv_vault::recovery::google_account_tag;
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
/// A permission a restored backup carried: withdrawn at Google, its copies left alone. Sealed
/// like the permission; tried again until Google confirms.
const REVOKE_SECRET: &str = "drive_backup_revoke";
/// How the last cleanup went (and the last withdrawal after a restore): a retry that ends the
/// same way writes nothing more to the activity log.
const CLEANUP_RESULT_KEY: &str = "drive_backup_cleanup_result";
const REVOKE_RESULT_KEY: &str = "drive_backup_revoke_result";
/// "1" after a restore of a backup taken while the copy was on: copies from before the restore
/// may be in her Drive, until a cleanup deletes them.
const COPIES_LEFT_KEY: &str = "drive_backup_copies_left";
/// The account (its tag, as the Google slot keeps it) each permission and each debt belongs
/// to. A debt is dropped only for the same account: another account is another grant.
const TOKEN_ACCOUNT_KEY: &str = "drive_backup_account";
const CLEANUP_ACCOUNT_KEY: &str = "drive_backup_cleanup_account";
const REVOKE_ACCOUNT_KEY: &str = "drive_backup_revoke_account";
/// The account of a permission given before accounts were kept: matches none.
const UNKNOWN_ACCOUNT: &str = "unknown";
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
    /// No internet. The next backup tries again.
    Offline,
    /// The permission at Google was withdrawn: connect again.
    Reconnect,
    /// Google answered something else, the secure connection failed, or the connection could
    /// not be set up. The next backup tries again.
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
            GoogleError::Offline => DriveProblem::Offline,
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
    fn parse(s: &str) -> Option<Self> {
        [
            DriveCleanupResult::Off,
            DriveCleanupResult::Cleaned,
            DriveCleanupResult::Pending,
            DriveCleanupResult::Withdrawn,
        ]
        .into_iter()
        .find(|r| r.as_str() == s)
    }

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
    /// A restored backup was taken while the copy was on: copies from before the restore may
    /// be in her Drive (turning the copy on and off deletes them).
    pub copies_left: bool,
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

/// Turning it on, as checked before her browser opens ([`Core::drive_backup_turn_on_check`]).
#[derive(Debug, Clone)]
pub struct DriveTurnOn {
    pub vault_id: String,
    /// The tag of the Google account connected to the vault.
    pub tag: String,
    /// Something is owed at Google: the sign-in's end leaves the app's grant alone
    /// ([`Core::drive_keeps_grant`]).
    pub keep_grant: bool,
    /// The accounts of the debts owed (a cleanup, a withdrawal after a restore).
    owed: Vec<String>,
}

impl DriveTurnOn {
    /// A debt is owed on this account: its grant is not withdrawn here, or the debt could no
    /// longer delete the copies. The debt withdraws the whole grant itself.
    fn owes(&self, tag: &str) -> bool {
        self.owed.iter().any(|o| o == tag || o == UNKNOWN_ACCOUNT)
    }
}

/// Turning it on, step 2, with the core free, after her browser sign-in
/// ([`google::begin_lasting`]): the account is the one connected to the vault, and Google gave
/// a lasting permission. The short token is not revoked here: at Google that would withdraw
/// the lasting permission with it. It is dropped, and expires within the hour. Nothing is
/// withdrawn either on an account a debt is owed on ([`DriveTurnOn`]).
pub fn lasting_permission(
    http: &dyn Http,
    mut session: Session,
    turn_on: &DriveTurnOn,
) -> Result<Zeroizing<String>, CoreError> {
    if !same_account(&turn_on.vault_id, &turn_on.tag, &session) {
        // Another account: another grant, withdrawn unless a debt is owed on it.
        if !turn_on.owes(&google_account_tag(&turn_on.vault_id, &session.account_id)) {
            google::revoke(http, session);
        }
        return Err(refused(
            "זה לא חשבון הגוגל שחובר לכספת. אפשר לנסות שוב ולבחור את החשבון שחובר.",
        ));
    }
    match session.take_refresh_token() {
        Some(token) => Ok(token),
        None => {
            if !turn_on.owes(&turn_on.tag) {
                google::revoke(http, session);
            }
            Err(refused(
                "גוגל לא נתנה לתוכנה הרשאה קבועה. אפשר לנסות שוב ולאשר בחלון של גוגל.",
            ))
        }
    }
}

/// A lasting permission that was given but is not kept (the vault locked during the sign-in,
/// or Google was disconnected): withdrawn at Google, so no unused permission stays there.
/// Not when a debt is owed on the same account (see [`lasting_permission`]): the debt
/// withdraws this permission with its own, after deleting the copies.
pub fn withdraw_unkept(http: &dyn Http, token: &str, turn_on: &DriveTurnOn) {
    if !turn_on.owes(&turn_on.tag) {
        let _ = google::revoke_token(http, token);
    }
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

/// After turning off, with the core free: delete the copies and withdraw the permission. After
/// a restore (`copies: false`): withdraw the restored permission only.
pub struct DriveCleanup {
    session: u64,
    token: Zeroizing<String>,
    /// The account it is owed on.
    account: String,
    vault_id: String,
    copies: bool,
    generation: Arc<AtomicU64>,
    at: u64,
    _running: Running,
}

/// Counts a cleanup on its way, until it is dropped: the timer hands out no other meanwhile.
struct Running(Arc<AtomicUsize>);

impl Running {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Running(Arc::clone(count))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
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
    account: String,
    copies: bool,
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
        } else if !self.copies {
            match google::revoke_token(http, &self.token) {
                Ok(()) => Cleaned::Done(DriveCleanupResult::Cleaned),
                Err(_) => Cleaned::Done(DriveCleanupResult::Pending),
            }
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
            account: self.account,
            copies: self.copies,
            outcome,
        }
    }
}

/// Right after a restore, inside the restore itself (a failure here undoes the restore): a
/// backup taken while the copy was on carries its permission, and a restore must never turn it
/// on without her. The permission is forgotten and kept, sealed, only to be withdrawn at Google
/// (by [`Core::take_drive_cleanup`]); the copies stay, and the settings say how to delete them.
/// A cleanup the backup still owed stays owed.
pub(crate) fn forget_on_restore(v: &mut Vault) -> Result<(), dv_vault::VaultError> {
    let read = v.secret(TOKEN_SECRET);
    // Also when it could not be read: the permission must not stay.
    let had = !matches!(read, Ok(None));
    let token = read.ok().flatten().filter(|t| !t.is_empty());
    if had {
        let account = v
            .setting(TOKEN_ACCOUNT_KEY)?
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| UNKNOWN_ACCOUNT.to_owned());
        v.delete_secret(TOKEN_SECRET)?;
        v.set_state(TOKEN_ACCOUNT_KEY, "")?;
        if let Some(token) = &token {
            v.set_secret(REVOKE_SECRET, token)?;
            v.set_state(REVOKE_ACCOUNT_KEY, &account)?;
            v.set_state(REVOKE_RESULT_KEY, "")?;
        }
        // Whose copies they are: only a cleanup on that account deletes them.
        v.set_state(COPIES_LEFT_KEY, &account)?;
        v.record(
            AuditEvent::DriveBackupOff,
            None,
            &serde_json::json!({ "restore": true }),
        )?;
    }
    v.set_state(PROBLEM_KEY, "")?;
    v.set_state(LAST_KEY, "")?;
    v.set_state(REKEYED_KEY, "")?;
    Ok(())
}

impl Core {
    fn drive_token(&self) -> Option<Zeroizing<String>> {
        self.drive_secret(TOKEN_SECRET)
    }

    fn drive_secret(&self, name: &str) -> Option<Zeroizing<String>> {
        self.vault
            .as_ref()?
            .secret(name)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty())
    }

    fn drive_cleanup_token(&self) -> Option<Zeroizing<String>> {
        self.drive_secret(CLEANUP_SECRET)
    }

    /// Moves the generation: an upload or cleanup begun before it stops (see their `run`).
    fn drive_moved(&self) -> u64 {
        self.drive_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// The account kept under `key`, or [`UNKNOWN_ACCOUNT`].
    fn drive_account(&self, key: &str) -> String {
        self.vault
            .as_ref()
            .and_then(|v| v.setting(key).ok().flatten())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| UNKNOWN_ACCOUNT.to_owned())
    }

    fn drive_cleanup(
        &self,
        token: Zeroizing<String>,
        copies: bool,
        at: u64,
    ) -> Option<DriveCleanup> {
        let account = self.drive_account(if copies {
            CLEANUP_ACCOUNT_KEY
        } else {
            REVOKE_ACCOUNT_KEY
        });
        Some(DriveCleanup {
            session: self.session,
            token,
            account,
            vault_id: self.vault.as_ref()?.vault_id().to_owned(),
            copies,
            generation: Arc::clone(&self.drive_generation),
            at,
            _running: Running::new(&self.drive_cleanups),
        })
    }

    /// The copy is on (the vault holds the lasting permission).
    #[must_use]
    pub fn drive_backup_on(&self) -> bool {
        self.drive_token().is_some()
    }

    /// Whether the end of a Google sign-in must leave the app's grant at Google alone: the
    /// copy is on, or a cleanup is owed (withdrawing the grant would strand it), or the vault
    /// is locked and that is not known. A short token kept this way runs out within the hour.
    #[must_use]
    pub fn drive_keeps_grant(&self) -> bool {
        self.vault.is_none() || self.drive_backup_on() || self.drive_cleanup_token().is_some()
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
        let copies_left = v.setting(COPIES_LEFT_KEY)?.is_some_and(|a| !a.is_empty());
        Ok(DriveBackupStatus {
            available: google.available,
            connected: google.on,
            on,
            last_at,
            problem,
            cleanup_pending,
            copies_left,
        })
    }

    /// Turning it on, step 1 (under the lock): her Google account is connected on this
    /// computer, the copy is not on already, and no cleanup is owed on another account (the
    /// next turning off would need its place). Returns what the sign-in needs ([`DriveTurnOn`]).
    /// A cleanup still running from an earlier turning off does not withdraw the permission
    /// she is about to give (the same grant at Google).
    pub fn drive_backup_turn_on_check(
        &mut self,
        keys: &dyn ComputerKeys,
    ) -> Result<DriveTurnOn, CoreError> {
        let vault_id = self.vault_ref()?.vault_id().to_owned();
        if self.drive_backup_on() {
            return Err(refused("העותק בגוגל דרייב כבר דלוק."));
        }
        if !self.google_status(keys).on {
            return Err(not_connected());
        }
        let (_, tag) = Vault::google_slot(&self.dir)?;
        let tag = tag.ok_or_else(not_connected)?;
        self.refuse_cleanup_elsewhere(&tag)?;
        let mut owed = Vec::new();
        if self.drive_cleanup_token().is_some() {
            owed.push(self.drive_account(CLEANUP_ACCOUNT_KEY));
        }
        if self.drive_secret(REVOKE_SECRET).is_some() {
            owed.push(self.drive_account(REVOKE_ACCOUNT_KEY));
        }
        self.drive_moved();
        Ok(DriveTurnOn {
            vault_id,
            tag,
            keep_grant: self.drive_keeps_grant(),
            owed,
        })
    }

    /// A cleanup owed on another account than `tag` keeps its place until it is done.
    fn refuse_cleanup_elsewhere(&self, tag: &str) -> Result<(), CoreError> {
        if self.drive_cleanup_token().is_some() && self.drive_account(CLEANUP_ACCOUNT_KEY) != tag {
            return Err(refused(
                "עוד לא הושלמה המחיקה של העותקים מחשבון הגוגל הקודם. התוכנה מנסה שוב מעצמה \
                 בזמן שהכספת פתוחה, ואחרי שזה יושלם אפשר להדליק.",
            ));
        }
        Ok(())
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
        self.refuse_cleanup_elsewhere(tag)?;
        self.drive_moved();
        // A cleanup still owed, or a restored permission still to withdraw, on this same
        // account would withdraw this new permission with the old one (the same grant at
        // Google): dropped. Their copies are this vault's, and the next uploads keep only the
        // newest. On another account they stay owed: another grant, other copies.
        let drop_cleanup =
            self.drive_cleanup_token().is_some() && self.drive_account(CLEANUP_ACCOUNT_KEY) == tag;
        let drop_revoke = self.drive_secret(REVOKE_SECRET).is_some()
            && self.drive_account(REVOKE_ACCOUNT_KEY) == tag;
        let v = self.vault_mut()?;
        v.set_secret(TOKEN_SECRET, token)?;
        v.set_state(TOKEN_ACCOUNT_KEY, tag)?;
        if drop_cleanup {
            v.delete_secret(CLEANUP_SECRET)?;
            v.set_state(CLEANUP_ACCOUNT_KEY, "")?;
        }
        if drop_revoke {
            v.delete_secret(REVOKE_SECRET)?;
            v.set_state(REVOKE_ACCOUNT_KEY, "")?;
        }
        v.set_state(PROBLEM_KEY, "")?;
        v.record(AuditEvent::DriveBackupOn, None, &serde_json::json!({}))?;
        Ok(())
    }

    /// Turn it off (under the lock): the permission is forgotten at once, and no further copy
    /// goes up (an upload on its way stops before its body). The permission stays sealed only
    /// as the cleanup owed at Google, returned to run with the core free. Also when it was
    /// already off but a cleanup is still owed (disconnecting Google asks again).
    pub fn drive_backup_turn_off(&mut self) -> Result<Option<DriveCleanup>, CoreError> {
        let v = self.vault_mut()?;
        let read = v.secret(TOKEN_SECRET);
        // Also when it could not be read: the permission must not stay.
        let was_on = !matches!(read, Ok(None));
        if was_on {
            v.delete_secret(TOKEN_SECRET)?;
        }
        let token = read.ok().flatten().filter(|t| !t.is_empty());
        let account = v
            .setting(TOKEN_ACCOUNT_KEY)?
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| UNKNOWN_ACCOUNT.to_owned());
        v.set_state(TOKEN_ACCOUNT_KEY, "")?;
        v.set_state(PROBLEM_KEY, "")?;
        v.set_state(LAST_KEY, "")?;
        if let Some(token) = &token {
            v.set_secret(CLEANUP_SECRET, token)?;
            v.set_state(CLEANUP_ACCOUNT_KEY, &account)?;
            v.set_state(CLEANUP_RESULT_KEY, "")?;
            v.record(AuditEvent::DriveBackupOff, None, &serde_json::json!({}))?;
        }
        // Uploads on their way stop; a cleanup on its way while it was off already goes on.
        let at = if was_on {
            self.drive_moved()
        } else {
            self.drive_generation.load(Ordering::Acquire)
        };
        let Some(owed) = self.drive_cleanup_token() else {
            return Ok(None);
        };
        self.drive_cleanup_at = Some(Instant::now());
        Ok(self.drive_cleanup(owed, true, at))
    }

    /// What is owed at Google to try again with the core free: the cleanup after turning off,
    /// or else the withdrawal after a restore. After an unlock, then at most every 15 minutes
    /// while the vault is open, never while another is on its way, and never while a Google
    /// sign-in waits for her browser (it could withdraw the grant being given). The shell's
    /// timer asks.
    pub fn take_drive_cleanup(&mut self) -> Option<DriveCleanup> {
        if self
            .drive_cleanup_at
            .is_some_and(|t| t.elapsed() < CLEANUP_RETRY)
            || self.drive_cleanups.load(Ordering::Acquire) > 0
            || crate::google::signing_in()
        {
            return None;
        }
        let (token, copies) = match self.drive_cleanup_token() {
            Some(t) => (t, true),
            None => (self.drive_secret(REVOKE_SECRET)?, false),
        };
        self.drive_cleanup_at = Some(Instant::now());
        let at = self.drive_generation.load(Ordering::Acquire);
        self.drive_cleanup(token, copies, at)
    }

    /// What is owed now, as she is told: a cleanup still owed, or else how the last one went.
    fn drive_cleanup_now(&mut self) -> Result<DriveCleanupResult, CoreError> {
        if self.drive_cleanup_token().is_some() {
            return Ok(DriveCleanupResult::Pending);
        }
        Ok(self
            .vault_ref()?
            .setting(CLEANUP_RESULT_KEY)?
            .as_deref()
            .and_then(DriveCleanupResult::parse)
            .filter(|r| *r != DriveCleanupResult::Pending)
            .unwrap_or(DriveCleanupResult::Off))
    }

    /// Record how a cleanup went (under the lock). Done only when Google confirmed it, or
    /// when the permission is gone there already; otherwise it stays owed. A retry that ends
    /// as the last one did writes nothing more to the activity log.
    pub fn finish_drive_cleanup(
        &mut self,
        done: DriveCleaned,
    ) -> Result<DriveCleanupResult, CoreError> {
        self.same_session(done.session)?;
        let (secret, account_key, result_key) = if done.copies {
            (CLEANUP_SECRET, CLEANUP_ACCOUNT_KEY, CLEANUP_RESULT_KEY)
        } else {
            (REVOKE_SECRET, REVOKE_ACCOUNT_KEY, REVOKE_RESULT_KEY)
        };
        let ours = self
            .drive_secret(secret)
            .is_some_and(|t| t.as_str() == done.token.as_str());
        let result = match done.outcome {
            Cleaned::Superseded => {
                // On again with a new permission on the same account: nothing is owed any
                // more. Off again, or on with another account: still owed, tried later.
                if ours
                    && self.drive_backup_on()
                    && self.drive_account(TOKEN_ACCOUNT_KEY) == done.account
                    && done.account != UNKNOWN_ACCOUNT
                {
                    let v = self.vault_mut()?;
                    v.delete_secret(secret)?;
                    v.set_state(account_key, "")?;
                }
                return self.drive_cleanup_now();
            }
            // Finished by another run meanwhile (or turned on again): nothing more to record.
            Cleaned::Done(_) if !ours => return self.drive_cleanup_now(),
            Cleaned::Done(r) => r,
        };
        let v = self.vault_mut()?;
        if result != DriveCleanupResult::Pending {
            v.delete_secret(secret)?;
            v.set_state(account_key, "")?;
            // The copies left by a restore are deleted only by a cleanup on their account.
            if done.copies
                && result == DriveCleanupResult::Cleaned
                && v.setting(COPIES_LEFT_KEY)?.as_deref() == Some(done.account.as_str())
            {
                v.set_state(COPIES_LEFT_KEY, "")?;
            }
        }
        let last = v.setting(result_key)?;
        if last.as_deref() != Some(result.as_str()) {
            v.set_state(result_key, result.as_str())?;
            v.record(
                AuditEvent::DriveBackupCleanup,
                None,
                &serde_json::json!({ "result": result.as_str(), "restore": !done.copies }),
            )?;
        }
        Ok(result)
    }

    /// What to upload after a backup was recorded, when the copy is on and Google is still
    /// connected (a copy is never sent on a permission whose turning off did not finish).
    pub(crate) fn begin_drive_upload(
        &mut self,
        session: u64,
        created_at: i64,
        file: Vec<u8>,
    ) -> Option<DriveUpload> {
        let token = self.drive_token()?;
        Vault::google_slot(&self.dir).ok()?.1?;
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
