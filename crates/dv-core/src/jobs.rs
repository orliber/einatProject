//! Long work away from the core's lock (F2).
//!
//! The core sits behind one lock, so anything slow done under it stalls every other command:
//! saving a paragraph, the status, the 15-second timer. Each slow task is cut in three:
//!
//! 1. `begin_*` takes what the work needs, under the lock, in a moment.
//! 2. The job's `run` does the slow part (a key derivation, writing to a removable drive,
//!    reading it back) with no access to the core.
//! 3. `finish_*` writes the result back under the lock.
//!
//! A job remembers the session it began in. When the vault locked in between, `finish_*`
//! drops the result and says so: work begun for one opening of the vault never lands in
//! another. A lock also sets the jobs' [`Cancel`]: a job still running stops at its next
//! step and drops (and wipes) the texts it holds, instead of keeping them in memory until it
//! ends. The one-call versions (`Core::unlock`, `Core::write_backup`, ...) are the three
//! steps in a row, for tests and callers that hold no lock.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dv_vault::crypto::Key32;
use dv_vault::{Argon2Params, NewSlot, Secret, Vault, VaultError};
use zeroize::{Zeroize, Zeroizing};

use crate::binding::{bound_on_disk, key_for_new_slot, key_for_opening, key_here};
use crate::views::{BackupCheckView, BackupDone, CreatedVault};
use crate::{AppStatus, ComputerSeal, Core, CoreError, ErrorAction};

/// Set by a lock, for every job begun before it. Each lock starts a new one.
#[derive(Clone, Default)]
pub(crate) struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub(crate) fn set(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// What a document read holds: its text, wiped when it is dropped unused.
fn wipe(e: &mut dv_ingest::Extracted) {
    e.body.zeroize();
    e.margins.zeroize();
    for (k, v) in &mut e.metadata {
        k.zeroize();
        v.zeroize();
    }
}

/// The password or the printed kit, owned, so it can travel to the job's thread.
enum Proof {
    Password(Zeroizing<String>),
    Recovery(Zeroizing<String>),
}

impl Proof {
    fn new(current: &str, with_recovery: bool) -> Self {
        let owned = Zeroizing::new(current.to_owned());
        if with_recovery {
            Self::Recovery(owned)
        } else {
            Self::Password(owned)
        }
    }

    /// The proof as a secret; a password with this computer's key when there is one.
    fn secret<'a>(&'a self, here: Option<&'a Key32>) -> Secret<'a> {
        match self {
            Self::Password(p) => crate::binding::password_secret(p, here),
            Self::Recovery(r) => Secret::Recovery(r),
        }
    }
}

/// Opening the vault: the key derivation (Argon2id, about a second) runs in the job.
pub struct UnlockJob {
    dir: PathBuf,
    proof: Proof,
    ticket: u64,
    seal: Arc<dyn ComputerSeal>,
}

/// An opened vault (or why not), waiting for [`Core::finish_unlock`].
pub struct Unlocked {
    opened: Result<Vault, VaultError>,
    ticket: u64,
    /// The password slot was bound to this computer by this opening (D-076): said once.
    just_bound: bool,
}

impl UnlockJob {
    #[must_use]
    pub fn run(self) -> Unlocked {
        let mut just_bound = false;
        let opened = match &self.proof {
            Proof::Password(p) => {
                // Unsealing the key kept here takes a moment too, so it runs in the job.
                let bound = bound_on_disk(&self.dir);
                let mut opened = key_for_opening(self.seal.as_ref(), &self.dir, bound)
                    .and_then(|here| Vault::unlock_with_password_here(&self.dir, p, here.as_ref()));
                // A new password whose key was held aside and not yet promoted (the program
                // closed in between): that key opens it, and becomes the one in use.
                if bound
                    && matches!(
                        opened,
                        Err(VaultError::WrongSecret | VaultError::OtherComputer)
                    )
                {
                    if let Ok(next) = self.seal.load_next(&self.dir) {
                        if let Ok(v) = Vault::unlock_with_password_here(&self.dir, p, Some(&next)) {
                            let _ = self.seal.promote_next(&self.dir);
                            opened = Ok(v);
                        }
                    }
                }
                just_bound = !bound && opened.as_ref().is_ok_and(Vault::password_bound);
                opened
            }
            Proof::Recovery(r) => Vault::unlock_with_recovery(&self.dir, r),
        };
        Unlocked {
            opened,
            ticket: self.ticket,
            just_bound,
        }
    }
}

/// A new password or a new printed kit: the proof and the new slot are made in the job.
pub struct SlotJob {
    session: u64,
    ticket: u64,
    header: dv_vault::VaultHeader,
    proof: Proof,
    /// `Some` for a new password; `None` for a new kit.
    new_password: Option<Zeroizing<String>>,
    params: Option<Argon2Params>,
    dir: PathBuf,
    seal: Arc<dyn ComputerSeal>,
}

/// The slot made by a [`SlotJob`] (or why not), and the new kit when it was one.
pub struct SlotMade {
    session: u64,
    ticket: u64,
    made: Result<(NewSlot, Option<Zeroizing<String>>), VaultError>,
    /// The new slot uses a key held aside, promoted only once the slot is written.
    key_aside: bool,
}

impl SlotJob {
    #[must_use]
    pub fn run(self) -> SlotMade {
        let seal = self.seal.as_ref();
        // Loaded as for an opening: a Keychain or Windows that is only slow, locked or
        // cancelled is asked once more and then says to try again, before any new key is
        // made; never "another computer" (review of #136).
        let here = match key_for_opening(seal, &self.dir, self.header.password_bound()) {
            Ok(here) => here,
            Err(e) => {
                return SlotMade {
                    session: self.session,
                    ticket: self.ticket,
                    made: Err(e),
                    key_aside: false,
                };
            }
        };
        let proof = self.proof.secret(here.as_ref());
        let mut key_aside = false;
        let made = match &self.new_password {
            Some(new_password) => {
                // Calibrating the key derivation to this computer takes a moment too.
                let params = self.params.unwrap_or_else(Argon2Params::calibrate);
                // The new password is bound here, also when the kit was the proof on a
                // computer the old one was not bound to (D-076). The new key is held aside
                // until the slot is written, so the key in use is never lost to a slot
                // that was not (review of stage 5, S5).
                let fresh = if here.is_none() {
                    key_for_new_slot(seal, &self.dir).map(|(key, aside)| {
                        key_aside = aside;
                        key
                    })
                } else {
                    Ok(None)
                };
                fresh
                    .and_then(|fresh| {
                        NewSlot::password_here(
                            &self.header,
                            proof,
                            new_password,
                            params,
                            here.as_ref().or(fresh.as_ref()),
                        )
                    })
                    .map(|s| (s, None))
            }
            None => NewSlot::recovery(&self.header, proof).map(|(s, k)| (s, Some(k))),
        };
        SlotMade {
            session: self.session,
            ticket: self.ticket,
            made,
            key_aside,
        }
    }
}

/// A backup: the snapshot is taken under the lock; writing it to the drive and reading it
/// back run in the job.
pub struct BackupTask {
    session: u64,
    cancel: Cancel,
    /// The header the backup holds; its key slots say which password and kit open it.
    header: dv_vault::VaultHeader,
    job: dv_vault::BackupJob,
    dest: PathBuf,
    folder: PathBuf,
    auto: bool,
    /// The encrypted copy in her Drive is on (D-099): keep the file as it was read back.
    drive: bool,
}

/// A backup written and read back (or why not), waiting for [`Core::finish_backup`].
pub struct BackupRun {
    session: u64,
    header: dv_vault::VaultHeader,
    dest: PathBuf,
    folder: PathBuf,
    auto: bool,
    done: Result<dv_vault::BackupWritten, CoreError>,
    /// The verified file, only when the Drive copy is on.
    pub(crate) file: Option<Vec<u8>>,
}

impl BackupTask {
    #[must_use]
    pub fn run(self) -> BackupRun {
        let written = if self.cancel.is_set() {
            Err(locked_meanwhile())
        } else {
            Self::write(self.job, &self.dest)
        };
        let (done, file) = match written {
            Ok((done, file)) => (Ok(done), self.drive.then_some(file)),
            Err(e) => (Err(e), None),
        };
        BackupRun {
            session: self.session,
            header: self.header,
            dest: self.dest,
            folder: self.folder,
            auto: self.auto,
            done,
            file,
        }
    }

    fn write(
        job: dv_vault::BackupJob,
        dest: &Path,
    ) -> Result<(dv_vault::BackupWritten, Vec<u8>), CoreError> {
        job.write(dest)
            .map_err(|_| {
                CoreError::Needs(
                    "לא הצלחתי לכתוב את הגיבוי לתיקייה שנבחרה. אם זה דיסק נייד, כדאי לבדוק שהוא \
                     מחובר ושיש בו מקום."
                        .to_owned(),
                    ErrorAction::retry(),
                )
            })
            .and_then(|w| {
                w.verify_keeping_file().map_err(|_| {
                    CoreError::Needs(
                        "הגיבוי נכתב, אבל בבדיקה הוא לא נפתח כמו שצריך. כדאי לנסות שוב, ואם זה \
                         חוזר, לנסות דיסק אחר."
                            .to_owned(),
                        ErrorAction::retry(),
                    )
                })
            })
    }
}

/// The restore drill on the chosen file: the key derivation runs in the job.
pub struct DrillJob {
    session: u64,
    dir: PathBuf,
    bytes: Vec<u8>,
    password: Zeroizing<String>,
}

/// What the drill found (or why not), waiting for [`Core::finish_check_backup`].
pub struct Drilled {
    session: u64,
    bytes: Vec<u8>,
    check: Result<dv_vault::BackupCheck, VaultError>,
}

impl DrillJob {
    #[must_use]
    pub fn run(self) -> Drilled {
        let check =
            dv_vault::drill_backup(&self.dir, &self.bytes, Secret::Password(&self.password));
        Drilled {
            session: self.session,
            bytes: self.bytes,
            check,
        }
    }
}

/// Restoring a backup on a computer with no vault: opening it runs in the job.
pub struct RestoreJob {
    dir: PathBuf,
    bytes: Vec<u8>,
    proof: Proof,
    /// A restore writes the vault folder (and removes it again when it fails), so it takes
    /// the same single place as an unlock: nothing else creates or opens a vault meanwhile.
    ticket: u64,
    seal: Arc<dyn ComputerSeal>,
}

/// The restored vault (or why not), waiting for [`Core::finish_restore`].
pub struct Restored {
    bytes: Vec<u8>,
    vault: Result<Vault, VaultError>,
    ticket: u64,
}

impl RestoreJob {
    #[must_use]
    pub fn run(self) -> Restored {
        // A failed restore removes what it wrote. The copy in her Drive is never turned on by
        // a restore (D-099): when that cannot be made sure of, the restore fails.
        let vault = Vault::restore_backup_then(
            &self.bytes,
            &self.dir,
            self.proof.secret(None),
            crate::drive_backup::forget_on_restore,
        )
        .map(|(mut v, _)| {
            // The backup's password slot opens anywhere; here it is bound to this
            // computer at once (D-076). Best effort: otherwise at the next opening.
            if let Proof::Password(p) = &self.proof {
                if let Some(here) = key_here(self.seal.as_ref(), &self.dir, false) {
                    let _ = v.bind_password(p, &here);
                }
            }
            v
        });
        Restored {
            bytes: self.bytes,
            vault,
            ticket: self.ticket,
        }
    }
}

/// Reading a document she brought: the isolated reader process, and OCR for a scan (many
/// seconds for a long scan), run in the job.
pub struct ReadJob {
    session: u64,
    cancel: Cancel,
    exe: Option<PathBuf>,
    ocr: Option<dv_ingest::ocr::Engine>,
    file_name: String,
    /// The file as she brought it: wiped when the job ends.
    bytes: Zeroizing<Vec<u8>>,
    /// For one of her past reports: what neutralizing it needs, done in the job too.
    style: Option<crate::style::StyleData>,
}

/// A document read (or why not), waiting for its `finish_*`.
pub struct DocumentRead {
    session: u64,
    file_name: String,
    /// Taken by `finish_*`; whatever is left (a result that lands after a lock) is wiped.
    extracted: Result<dv_ingest::Extracted, CoreError>,
    style: Option<Result<crate::style::StyleWork, CoreError>>,
}

impl Drop for DocumentRead {
    fn drop(&mut self) {
        if let Ok(e) = &mut self.extracted {
            wipe(e);
        }
    }
}

impl DocumentRead {
    /// The text read, leaving nothing behind to wipe.
    fn take(&mut self) -> Result<dv_ingest::Extracted, CoreError> {
        std::mem::replace(
            &mut self.extracted,
            Err(CoreError::Internal("taken".to_owned())),
        )
    }

    /// The text the import filters (the body; the margins and file properties are never
    /// sent), for the name model to read first. Empty when the file could not be read.
    #[must_use]
    pub fn texts(&self) -> Vec<String> {
        self.extracted
            .as_ref()
            .map(|e| vec![e.body.clone()])
            .unwrap_or_default()
    }
}

impl ReadJob {
    #[must_use]
    pub fn run(self) -> DocumentRead {
        let mut extracted = if self.cancel.is_set() {
            Err(locked_meanwhile())
        } else {
            dv_ingest::import(
                self.exe.as_deref(),
                self.ocr.as_ref(),
                &self.file_name,
                &self.bytes,
            )
            .map_err(|e| CoreError::Refused(e.message_he()))
        };
        // Locked while it was read (OCR can take long): the text goes now, not later.
        if self.cancel.is_set() {
            if let Ok(e) = &mut extracted {
                wipe(e);
            }
            extracted = Err(locked_meanwhile());
        }
        let style = match (&self.style, &extracted) {
            (Some(data), Ok(e)) => Some(crate::style::style_work(data, &self.file_name, e)),
            _ => None,
        };
        DocumentRead {
            session: self.session,
            file_name: self.file_name,
            extracted,
            style,
        }
    }
}

/// The format of an export, with what rendering it needs.
enum ExportFormat {
    Word {
        template: Option<Vec<u8>>,
        password: Option<Zeroizing<String>>,
    },
    Pdf {
        password: Option<Zeroizing<String>>,
    },
}

/// Exporting the report: it is put together from approved paragraphs under the lock, and
/// rendered (Word or PDF, and encrypted) in the job.
pub struct ExportJob {
    session: u64,
    cancel: Cancel,
    case_id: String,
    report: WipedReport,
    check: crate::ExportCheck,
    format: ExportFormat,
    /// The parents' version (round 55), for the audit log.
    parents: bool,
}

/// The file's bytes (or why not), waiting for [`Core::finish_export`].
pub struct Exported {
    session: u64,
    case_id: String,
    check: crate::ExportCheck,
    pdf: bool,
    protected: bool,
    parents: bool,
    /// The report as a file, wiped if it is dropped unsaved (it landed after a lock).
    bytes: Result<Zeroizing<Vec<u8>>, CoreError>,
}

pub(crate) fn export_error(e: dv_export::ExportError) -> CoreError {
    match e {
        dv_export::ExportError::WeakPassword => CoreError::Refused(format!(
            "סיסמה לקובץ צריכה להיות באורך {} תווים לפחות.",
            dv_export::MIN_PASSWORD_CHARS
        )),
        dv_export::ExportError::Template(m)
        | dv_export::ExportError::Pdf(m)
        | dv_export::ExportError::Image(m) => CoreError::Refused(m),
        other => CoreError::Internal(other.to_string()),
    }
}

impl Exported {
    /// The file was made (not dropped because the vault locked while it was).
    #[must_use]
    pub fn rendered(&self) -> bool {
        self.bytes.is_ok()
    }
}

/// The report's texts, wiped when the job ends: after `run`, or if the job is dropped before it
/// ran (a lock in between).
struct WipedReport(dv_export::Report);

impl std::ops::Deref for WipedReport {
    type Target = dv_export::Report;
    fn deref(&self) -> &dv_export::Report {
        &self.0
    }
}

impl Drop for WipedReport {
    fn drop(&mut self) {
        self.0.wipe();
    }
}

impl ExportJob {
    #[must_use]
    pub fn run(mut self) -> Exported {
        let (pdf, protected, bytes) = self.render();
        // The report's texts (names, paragraphs, scores) are not needed once the file is made.
        self.report.0.wipe();
        // Locked while it was rendered: the file is wiped now, and nothing is saved.
        let bytes = if self.cancel.is_set() {
            Err(locked_meanwhile())
        } else {
            bytes.map(Zeroizing::new)
        };
        Exported {
            session: self.session,
            case_id: self.case_id,
            check: self.check,
            pdf,
            protected,
            parents: self.parents,
            bytes,
        }
    }

    fn render(&self) -> (bool, bool, Result<Vec<u8>, CoreError>) {
        if self.cancel.is_set() {
            let pdf = matches!(self.format, ExportFormat::Pdf { .. });
            return (pdf, false, Err(locked_meanwhile()));
        }
        match &self.format {
            ExportFormat::Word { template, password } => {
                let docx = match template {
                    Some(t) => dv_export::render_with_template(&self.report, t),
                    None => dv_export::render(&self.report),
                }
                .map_err(export_error);
                let bytes = docx.and_then(|d| match password {
                    Some(pw) => {
                        let d = Zeroizing::new(d);
                        dv_export::encrypt(&d, pw).map_err(export_error)
                    }
                    None => Ok(d),
                });
                (false, password.is_some(), bytes)
            }
            ExportFormat::Pdf { password } => {
                let bytes = crate::system_fonts(&self.report.font)
                    .ok_or_else(|| {
                        CoreError::Refused(
                            "לא נמצא במחשב גופן עברי ליצירת PDF (למשל David או Arial).".to_owned(),
                        )
                    })
                    .and_then(|(regular, bold)| {
                        dv_export::render_pdf(
                            &self.report,
                            dv_export::PdfFonts {
                                regular: &regular,
                                bold: bold.as_deref(),
                            },
                            password.as_deref().map(String::as_str),
                        )
                        .map_err(export_error)
                    });
                (true, password.is_some(), bytes)
            }
        }
    }
}

/// The jobs carry passwords and keys: their debug form names only the kind.
macro_rules! opaque_debug {
    ($($t:ty),*) => {$(
        impl std::fmt::Debug for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(stringify!($t))
            }
        }
    )*};
}
opaque_debug!(
    UnlockJob,
    Unlocked,
    SlotJob,
    SlotMade,
    BackupTask,
    BackupRun,
    DrillJob,
    Drilled,
    RestoreJob,
    Restored,
    ReadJob,
    DocumentRead,
    ExportJob,
    Exported
);

/// The vault locked while the work ran: its result is dropped.
fn locked_meanwhile() -> CoreError {
    CoreError::Needs(
        "הכספת ננעלה באמצע. אפשר לפתוח אותה ולנסות שוב.".to_owned(),
        ErrorAction::Unlock,
    )
}

impl Core {
    pub(crate) fn same_session(&self, session: u64) -> Result<(), CoreError> {
        if self.vault.is_none() {
            return Err(CoreError::Locked);
        }
        if self.session != session {
            return Err(locked_meanwhile());
        }
        Ok(())
    }

    // ------------------------------------------------------------ unlock

    pub fn begin_unlock(&mut self, password: &str) -> Result<UnlockJob, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        let ticket = self.claim_secret_work()?;
        Ok(UnlockJob {
            dir: self.dir.clone(),
            ticket,
            proof: Proof::new(password, false),
            seal: Arc::clone(&self.computer_seal),
        })
    }

    pub fn begin_unlock_with_recovery(&mut self, typed: &str) -> Result<UnlockJob, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        let ticket = self.claim_secret_work()?;
        Ok(UnlockJob {
            dir: self.dir.clone(),
            ticket,
            proof: Proof::new(typed, true),
            seal: Arc::clone(&self.computer_seal),
        })
    }

    /// Keep the opened vault. When one is already open (an opening that came back after the
    /// stale wait), this one is dropped, which wipes its keys, without writing anything: the
    /// vault already open stays as it is.
    pub fn finish_unlock(&mut self, opened: Unlocked) -> Result<AppStatus, CoreError> {
        self.release_secret_work(opened.ticket);
        if self.vault.is_some() {
            drop(opened);
            return Ok(self.status());
        }
        let just_bound = opened.just_bound;
        let status = self.after_unlock(opened.opened, true)?;
        if !just_bound {
            return Ok(status);
        }
        // An older vault bound at this opening: said once, with what to keep (review of
        // stage 5, S1).
        self.since_last = Some(match self.since_last.take() {
            Some(before) => format!("{before} {}", crate::binding::JUST_BOUND),
            None => crate::binding::JUST_BOUND.to_owned(),
        });
        Ok(self.status())
    }

    pub fn unlock(&mut self, password: &str) -> Result<AppStatus, CoreError> {
        let job = self.begin_unlock(password)?;
        self.finish_unlock(job.run())
    }

    pub fn unlock_with_recovery(&mut self, recovery_key: &str) -> Result<AppStatus, CoreError> {
        let job = self.begin_unlock_with_recovery(recovery_key)?;
        self.finish_unlock(job.run())
    }

    // ------------------------------------------------------------ password and kit

    /// A new password. Old backups keep opening with the old one, so a new backup is asked for.
    pub fn begin_change_password(
        &mut self,
        current: &str,
        with_recovery: bool,
        new_password: &str,
    ) -> Result<SlotJob, CoreError> {
        self.check_backoff()?;
        // The policy first, so a weak password is refused before any slow work.
        dv_vault::password::check_policy(new_password).map_err(VaultError::Policy)?;
        let header = self.vault_ref()?.header_copy();
        let ticket = self.claim_secret_work()?;
        Ok(SlotJob {
            session: self.session,
            ticket,
            header,
            proof: Proof::new(current, with_recovery),
            new_password: Some(Zeroizing::new(new_password.to_owned())),
            params: self.argon,
            dir: self.dir.clone(),
            seal: Arc::clone(&self.computer_seal),
        })
    }

    pub fn finish_change_password(&mut self, made: SlotMade) -> Result<(), CoreError> {
        self.finish_slot(made).map(|_| ())
    }

    pub fn change_password(
        &mut self,
        current: &str,
        with_recovery: bool,
        new_password: &str,
    ) -> Result<(), CoreError> {
        let job = self.begin_change_password(current, with_recovery, new_password)?;
        self.finish_change_password(job.run())
    }

    /// A new printed kit; the old one stops opening the vault (not old backups).
    pub fn begin_new_recovery_kit(
        &mut self,
        current: &str,
        with_recovery: bool,
    ) -> Result<SlotJob, CoreError> {
        self.check_backoff()?;
        let header = self.vault_ref()?.header_copy();
        let ticket = self.claim_secret_work()?;
        Ok(SlotJob {
            session: self.session,
            ticket,
            header,
            proof: Proof::new(current, with_recovery),
            new_password: None,
            params: None,
            dir: self.dir.clone(),
            seal: Arc::clone(&self.computer_seal),
        })
    }

    pub fn finish_new_recovery_kit(&mut self, made: SlotMade) -> Result<CreatedVault, CoreError> {
        let kit = self
            .finish_slot(made)?
            .ok_or_else(|| CoreError::Internal("no kit".to_owned()))?;
        Ok(CreatedVault {
            recovery_key: kit.to_string(),
        })
    }

    pub fn new_recovery_kit(
        &mut self,
        current: &str,
        with_recovery: bool,
    ) -> Result<CreatedVault, CoreError> {
        let job = self.begin_new_recovery_kit(current, with_recovery)?;
        self.finish_new_recovery_kit(job.run())
    }

    fn finish_slot(&mut self, made: SlotMade) -> Result<Option<Zeroizing<String>>, CoreError> {
        self.release_secret_work(made.ticket);
        self.same_session(made.session)?;
        let (slot, kit) = match made.made {
            Ok(m) => m,
            Err(e) => {
                self.count_failure(&e);
                return Err(e.into());
            }
        };
        self.vault_mut()?.apply_slot(slot).map_err(|e| match e {
            // Changed twice at once: the other change stands, and this one can be done again.
            VaultError::Integrity(_) => CoreError::Needs(
                "הסיסמה או הערכה השתנו בינתיים. אפשר לנסות שוב.".to_owned(),
                ErrorAction::retry(),
            ),
            other => other.into(),
        })?;
        if made.key_aside {
            self.promote_password_key();
        }
        self.failed_unlocks = 0;
        self.mark_secret_changed()?;
        Ok(kit)
    }

    // ------------------------------------------------------------ backup

    /// Check where the backup goes and take the snapshot. Writing it out is the task's.
    pub fn begin_backup(&mut self, dest: &Path) -> Result<BackupTask, CoreError> {
        let (dest, folder) = self.backup_destination(dest)?;
        let header = self.vault_ref()?.header_copy();
        let job = self.vault_mut()?.begin_backup()?;
        Ok(BackupTask {
            session: self.session,
            cancel: self.cancel.clone(),
            header,
            job,
            dest,
            folder,
            auto: false,
            drive: self.drive_backup_on(),
        })
    }

    /// Record a backup that was written and read back: it counts from now on.
    pub fn finish_backup(&mut self, run: BackupRun) -> Result<BackupDone, CoreError> {
        self.finish_backup_for_drive(run).0
    }

    /// As [`Core::finish_backup`], and when the encrypted copy in her Drive is on, what to
    /// upload with the core free ([`crate::drive_backup::DriveUpload::run`]). The local backup
    /// is recorded first and stands whatever happens to the upload (D-099).
    pub fn finish_backup_for_drive(
        &mut self,
        mut run: BackupRun,
    ) -> (
        Result<BackupDone, CoreError>,
        Option<crate::drive_backup::DriveUpload>,
    ) {
        let file = run.file.take();
        let session = run.session;
        // A password or kit changed while it was written: this backup opens only with the old
        // one, which may be why it was changed. It stays on her drive, and never goes up.
        let current = self
            .vault_ref()
            .is_ok_and(|v| v.header_copy().slots == run.header.slots);
        let done = self.finish_backup_only(run);
        let upload = match (&done, file) {
            (Ok(d), Some(file)) if current => self.begin_drive_upload(session, d.created_at, file),
            _ => None,
        };
        (done, upload)
    }

    fn finish_backup_only(&mut self, run: BackupRun) -> Result<BackupDone, CoreError> {
        let auto = run.auto;
        let result = self.record_backup(run);
        // A failed automatic backup waits before the next try (`auto_backup_tried`).
        if auto && result.is_ok() {
            self.auto_backup_tried = None;
        }
        result
    }

    fn record_backup(&mut self, run: BackupRun) -> Result<BackupDone, CoreError> {
        let done = run.done?;
        self.same_session(run.session)?;
        self.vault_mut()?.finish_backup(&done)?;
        // A password or kit changed while it was written: the backup still opens only with the
        // old one, so the reminder to make a new backup stays.
        let current = self.vault_ref()?.header_copy().slots == run.header.slots;
        self.backup_recorded(&run.folder, done.info.created_at, current)?;
        if run.auto {
            crate::backup::prune_auto_backups(&run.folder);
        }
        Ok(BackupDone {
            path: run.dest.display().to_string(),
            bytes: done.info.bytes,
            created_at: done.info.created_at,
        })
    }

    /// Write a backup to `dest` (chosen by Einat in the "Save as" window).
    pub fn write_backup(&mut self, dest: &Path) -> Result<BackupDone, CoreError> {
        let task = self.begin_backup(dest)?;
        self.finish_backup(task.run())
    }

    /// Called by the shell's timer. When a backup is due, the vault has cases, the automatic
    /// backup is on, and the folder of the last backup is there (the removable drive is
    /// connected), take one for that folder. `None` when nothing is due.
    ///
    /// The first backup is always hers: it chooses the folder, and the checks of
    /// `write_backup` (not in the cloud, not next to the vault) apply to every automatic one.
    pub fn begin_auto_backup(&mut self) -> Result<Option<BackupTask>, CoreError> {
        let Some(dir) = self.auto_backup_due()? else {
            return Ok(None);
        };
        self.auto_backup_tried = Some(std::time::Instant::now());
        let mut task = self.begin_backup(&dir.join(crate::backup::auto_backup_file_name(
            crate::dates::unix_now(),
        )))?;
        task.auto = true;
        Ok(Some(task))
    }

    pub fn auto_backup(&mut self) -> Result<Option<BackupDone>, CoreError> {
        match self.begin_auto_backup()? {
            Some(task) => self.finish_backup(task.run()).map(Some),
            None => Ok(None),
        }
    }

    // ------------------------------------------------------------ drill and restore

    /// Restore drill: open the chosen backup in a scratch folder with the password, count what
    /// it holds, and remove it again. Proves both the file and the password work.
    pub fn begin_check_backup(&mut self, password: &str) -> Result<DrillJob, CoreError> {
        self.check_backoff()?;
        let bytes = self.take_staged()?;
        let vault_id = match self.vault_ref() {
            Ok(v) => v.vault_id().to_owned(),
            Err(e) => {
                self.staged_backup = Some(bytes);
                return Err(e);
            }
        };
        match dv_vault::peek_backup(&bytes) {
            Ok(p) if p.vault_id == vault_id => {}
            Ok(_) => {
                self.staged_backup = Some(bytes);
                return Err(CoreError::Refused(
                    "זה גיבוי של כספת אחרת, לא של הכספת הזו.".to_owned(),
                ));
            }
            Err(e) => {
                self.staged_backup = Some(bytes);
                return Err(e.into());
            }
        }
        Ok(DrillJob {
            session: self.session,
            dir: self.dir.clone(),
            bytes,
            password: Zeroizing::new(password.to_owned()),
        })
    }

    pub fn finish_check_backup(&mut self, drilled: Drilled) -> Result<BackupCheckView, CoreError> {
        self.same_session(drilled.session)?;
        let check = match drilled.check {
            Ok(check) => check,
            Err(e) => {
                // Put back, so Einat can try another password.
                self.count_failure(&e);
                self.staged_backup = Some(drilled.bytes);
                return Err(e.into());
            }
        };
        self.failed_unlocks = 0;
        let v = self.vault_mut()?;
        v.record_drill(&check)?;
        self.drill_recorded()?;
        Ok(BackupCheckView {
            created_at: check.created_at,
            cases: check.cases,
            integrity_ok: check.integrity_ok,
        })
    }

    pub fn check_staged_backup(&mut self, password: &str) -> Result<BackupCheckView, CoreError> {
        let job = self.begin_check_backup(password)?;
        self.finish_check_backup(job.run())
    }

    /// On a computer with no vault: restore the chosen backup and open it.
    pub fn begin_restore(
        &mut self,
        password: Option<&str>,
        recovery_key: Option<&str>,
    ) -> Result<RestoreJob, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        if Vault::exists(&self.dir) {
            return Err(CoreError::Refused(
                "כבר יש כספת במחשב הזה. שחזור מגיבוי נעשה רק במחשב בלי כספת, כדי לא לדרוס עבודה."
                    .to_owned(),
            ));
        }
        let proof = match (password, recovery_key) {
            (Some(p), _) => Proof::new(p, false),
            (None, Some(r)) => Proof::new(r, true),
            (None, None) => return Err(CoreError::Refused("צריך סיסמה או ערכת שחזור.".to_owned())),
        };
        let bytes = self.take_staged()?;
        let ticket = match self.claim_secret_work() {
            Ok(t) => t,
            Err(e) => {
                self.staged_backup = Some(bytes);
                return Err(e);
            }
        };
        Ok(RestoreJob {
            dir: self.dir.clone(),
            bytes,
            proof,
            ticket,
            seal: Arc::clone(&self.computer_seal),
        })
    }

    pub fn finish_restore(&mut self, restored: Restored) -> Result<AppStatus, CoreError> {
        self.release_secret_work(restored.ticket);
        if restored.vault.is_err() {
            // Put back unless it succeeded, so Einat can try another password.
            self.staged_backup = Some(restored.bytes);
        }
        if self.vault.is_some() {
            drop(restored.vault);
            return Ok(self.status());
        }
        // No recycle-bin purge now: a restore is often how a mistaken deletion is undone.
        // after_unlock counts a wrong secret.
        self.after_unlock(restored.vault, false)
    }

    pub fn restore_staged_backup(
        &mut self,
        password: Option<&str>,
        recovery_key: Option<&str>,
    ) -> Result<AppStatus, CoreError> {
        let job = self.begin_restore(password, recovery_key)?;
        self.finish_restore(job.run())
    }

    // ------------------------------------------------------------ documents

    fn read_job(&self, file_name: &str, bytes: Vec<u8>) -> ReadJob {
        ReadJob {
            session: self.session,
            cancel: self.cancel.clone(),
            exe: self.ingest_exe.clone(),
            ocr: self.ocr.clone(),
            file_name: file_name.to_owned(),
            bytes: Zeroizing::new(bytes),
            style: None,
        }
    }

    /// A document for a case: read in the job, then shown with what would be stored and
    /// hidden ([`Core::finish_import_document`]).
    pub fn begin_import_document(
        &mut self,
        case_id: &str,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<ReadJob, CoreError> {
        self.vault_ref()?.case_meta(case_id)?;
        Ok(self.read_job(file_name, bytes))
    }

    pub fn finish_import_document(
        &mut self,
        case_id: &str,
        mut read: DocumentRead,
    ) -> Result<crate::ImportPreview, CoreError> {
        self.same_session(read.session)?;
        let extracted = read.take()?;
        self.preview_extracted(case_id, &read.file_name, extracted)
    }

    /// Read a document and show what would be stored and hidden. Nothing is saved until the
    /// psychologist confirms (then the UI calls [`Core::add_input`] with the reviewed text).
    pub fn import_document(
        &mut self,
        case_id: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<crate::ImportPreview, CoreError> {
        let job = self.begin_import_document(case_id, file_name, bytes.to_vec())?;
        self.finish_import_document(case_id, job.run())
    }

    /// One of her past reports, for her style: read in the job, then split and neutralized
    /// ([`Core::finish_import_style_source`]).
    pub fn begin_import_style_source(
        &mut self,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<ReadJob, CoreError> {
        self.style_room()?;
        let mut job = self.read_job(file_name, bytes);
        job.style = Some(self.style_data()?);
        Ok(job)
    }

    pub fn finish_import_style_source(
        &mut self,
        mut read: DocumentRead,
    ) -> Result<crate::StyleImportPreview, CoreError> {
        self.same_session(read.session)?;
        self.style_room()?;
        let extracted = read.take()?;
        let work = match read.style.take() {
            Some(work) => work?,
            None => crate::style::style_work(&self.style_data()?, &read.file_name, &extracted)?,
        };
        self.stage_style(work, &read.file_name, &extracted)
    }

    /// Read a past report and show what would be kept. Nothing is stored until she confirms
    /// with [`Core::save_style_source`].
    pub fn import_style_source(
        &mut self,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<crate::StyleImportPreview, CoreError> {
        let job = self.begin_import_style_source(file_name, bytes.to_vec())?;
        self.finish_import_style_source(job.run())
    }

    // ------------------------------------------------------------ export

    /// The report from approved paragraphs, with real names, and what stops the export.
    fn export_job(
        &mut self,
        case_id: &str,
        format: ExportFormat,
        parents: Option<crate::ParentsOptions>,
    ) -> Result<ExportJob, CoreError> {
        let (report, check) = match parents {
            Some(options) => self.build_parents_report(case_id, options)?,
            None => self.build_report(case_id)?,
        };
        if !check.blocking.is_empty() {
            return Err(CoreError::Refused(check.blocking.join(" · ")));
        }
        Ok(ExportJob {
            session: self.session,
            cancel: self.cancel.clone(),
            case_id: case_id.to_owned(),
            report: WipedReport(report),
            check,
            format,
            parents: parents.is_some(),
        })
    }

    /// The Word file. Refused while anything blocks; with a password it is encrypted
    /// (ECMA-376 Agile, AES-256), the way Word protects files.
    pub fn begin_export_report(
        &mut self,
        case_id: &str,
        password: Option<&str>,
    ) -> Result<ExportJob, CoreError> {
        self.begin_export_report_as(case_id, password, None)
    }

    /// The Word file, full or (with `parents`) the parents' version (round 55, D-095).
    pub fn begin_export_report_as(
        &mut self,
        case_id: &str,
        password: Option<&str>,
        parents: Option<crate::ParentsOptions>,
    ) -> Result<ExportJob, CoreError> {
        let template = self.template_bytes()?;
        self.export_job(
            case_id,
            ExportFormat::Word {
                template,
                password: password.map(|p| Zeroizing::new(p.to_owned())),
            },
            parents,
        )
    }

    /// The report as a locked PDF (EX-3, D-049): it can be printed but not changed, and its
    /// SHA-256 goes into the audit log so a copy can be checked later. With `password` it
    /// opens only with it; without one it opens for anyone (D-081).
    pub fn begin_export_pdf(
        &mut self,
        case_id: &str,
        password: Option<&str>,
    ) -> Result<ExportJob, CoreError> {
        self.begin_export_pdf_as(case_id, password, None)
    }

    /// The locked PDF, full or (with `parents`) the parents' version (round 55, D-095).
    pub fn begin_export_pdf_as(
        &mut self,
        case_id: &str,
        password: Option<&str>,
        parents: Option<crate::ParentsOptions>,
    ) -> Result<ExportJob, CoreError> {
        self.export_job(
            case_id,
            ExportFormat::Pdf {
                password: password
                    .filter(|p| !p.is_empty())
                    .map(|p| Zeroizing::new(p.to_owned())),
            },
            parents,
        )
    }

    /// Record the export (the PDF with its fingerprint) and learn from the final report.
    /// Returns the file's bytes and the check, whose file name has no child's name in it.
    pub fn finish_export(
        &mut self,
        exported: Exported,
    ) -> Result<(Vec<u8>, crate::ExportCheck), CoreError> {
        self.same_session(exported.session)?;
        // Moved out, not copied: no second copy is left to wipe.
        let bytes = std::mem::take(&mut *exported.bytes?);
        let sections = exported.check.included_sections;
        let meta = if exported.pdf {
            serde_json::json!({
                "protected": exported.protected,
                "pdf": true,
                "sha256": dv_vault::crypto::sha256_hex(&bytes),
                "sections": sections,
                "parents": exported.parents,
            })
        } else {
            serde_json::json!({
                "protected": exported.protected,
                "sections": sections,
                "parents": exported.parents,
            })
        };
        self.vault_mut()?
            .record(dv_vault::AuditEvent::Export, Some(&exported.case_id), &meta)?;
        self.learn_from_final(&exported.case_id);
        Ok((bytes, exported.check))
    }

    pub fn export_report(
        &mut self,
        case_id: &str,
        password: Option<&str>,
    ) -> Result<Vec<u8>, CoreError> {
        let job = self.begin_export_report(case_id, password)?;
        self.finish_export(job.run()).map(|(b, _)| b)
    }

    pub fn export_pdf(
        &mut self,
        case_id: &str,
        password: Option<&str>,
    ) -> Result<Vec<u8>, CoreError> {
        let job = self.begin_export_pdf(case_id, password)?;
        self.finish_export(job.run()).map(|(b, _)| b)
    }
}
