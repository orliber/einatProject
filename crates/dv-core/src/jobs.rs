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
//! another. The one-call versions (`Core::unlock`, `Core::write_backup`, ...) are the three
//! steps in a row, for tests and callers that hold no lock.

use std::path::{Path, PathBuf};

use dv_vault::{Argon2Params, NewSlot, Secret, Vault, VaultError};
use zeroize::Zeroizing;

use crate::views::{BackupCheckView, BackupDone, CreatedVault};
use crate::{AppStatus, Core, CoreError, ErrorAction};

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

    fn secret(&self) -> Secret<'_> {
        match self {
            Self::Password(p) => Secret::Password(p),
            Self::Recovery(r) => Secret::Recovery(r),
        }
    }
}

/// Opening the vault: the key derivation (Argon2id, about a second) runs in the job.
pub struct UnlockJob {
    dir: PathBuf,
    proof: Proof,
    ticket: u64,
}

/// An opened vault (or why not), waiting for [`Core::finish_unlock`].
pub struct Unlocked {
    opened: Result<Vault, VaultError>,
    ticket: u64,
}

impl UnlockJob {
    #[must_use]
    pub fn run(self) -> Unlocked {
        let opened = match &self.proof {
            Proof::Password(p) => Vault::unlock_with_password(&self.dir, p),
            Proof::Recovery(r) => Vault::unlock_with_recovery(&self.dir, r),
        };
        Unlocked {
            opened,
            ticket: self.ticket,
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
}

/// The slot made by a [`SlotJob`] (or why not), and the new kit when it was one.
pub struct SlotMade {
    session: u64,
    ticket: u64,
    made: Result<(NewSlot, Option<Zeroizing<String>>), VaultError>,
}

impl SlotJob {
    #[must_use]
    pub fn run(self) -> SlotMade {
        let made = match &self.new_password {
            Some(new_password) => {
                // Calibrating the key derivation to this computer takes a moment too.
                let params = self.params.unwrap_or_else(Argon2Params::calibrate);
                NewSlot::password(&self.header, self.proof.secret(), new_password, params)
                    .map(|s| (s, None))
            }
            None => NewSlot::recovery(&self.header, self.proof.secret()).map(|(s, k)| (s, Some(k))),
        };
        SlotMade {
            session: self.session,
            ticket: self.ticket,
            made,
        }
    }
}

/// A backup: the snapshot is taken under the lock; writing it to the drive and reading it
/// back run in the job.
pub struct BackupTask {
    session: u64,
    job: dv_vault::BackupJob,
    dest: PathBuf,
    folder: PathBuf,
    auto: bool,
}

/// A backup written and read back (or why not), waiting for [`Core::finish_backup`].
pub struct BackupRun {
    session: u64,
    dest: PathBuf,
    folder: PathBuf,
    auto: bool,
    done: Result<dv_vault::BackupWritten, CoreError>,
}

impl BackupTask {
    #[must_use]
    pub fn run(self) -> BackupRun {
        let done = self
            .job
            .write(&self.dest)
            .map_err(|_| {
                CoreError::Needs(
                    "לא הצלחתי לכתוב את הגיבוי לתיקייה שנבחרה. אם זה דיסק נייד, כדאי לבדוק שהוא \
                     מחובר ושיש בו מקום."
                        .to_owned(),
                    ErrorAction::retry(),
                )
            })
            .and_then(|w| {
                w.verify().map_err(|_| {
                    CoreError::Needs(
                        "הגיבוי נכתב, אבל בבדיקה הוא לא נפתח כמו שצריך. כדאי לנסות שוב, ואם זה \
                         חוזר, לנסות דיסק אחר."
                            .to_owned(),
                        ErrorAction::retry(),
                    )
                })
            });
        BackupRun {
            session: self.session,
            dest: self.dest,
            folder: self.folder,
            auto: self.auto,
            done,
        }
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
}

/// The restored vault (or why not), waiting for [`Core::finish_restore`].
pub struct Restored {
    bytes: Vec<u8>,
    vault: Result<Vault, VaultError>,
}

impl RestoreJob {
    #[must_use]
    pub fn run(self) -> Restored {
        // A failed restore removes what it wrote.
        let vault =
            Vault::restore_backup(&self.bytes, &self.dir, self.proof.secret()).map(|(v, _)| v);
        Restored {
            bytes: self.bytes,
            vault,
        }
    }
}

/// Reading a document she brought: the isolated reader process, and OCR for a scan (many
/// seconds for a long scan), run in the job.
pub struct ReadJob {
    session: u64,
    exe: Option<PathBuf>,
    ocr: Option<dv_ingest::ocr::Engine>,
    file_name: String,
    bytes: Vec<u8>,
    /// For one of her past reports: what neutralizing it needs, done in the job too.
    style: Option<crate::style::StyleData>,
}

/// A document read (or why not), waiting for its `finish_*`.
pub struct DocumentRead {
    session: u64,
    file_name: String,
    extracted: Result<dv_ingest::Extracted, CoreError>,
    style: Option<Result<crate::style::StyleWork, CoreError>>,
}

impl DocumentRead {
    /// The texts the import filters (the body, and the title the file gives), for the name
    /// model to read first. Empty when the file could not be read.
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
        let extracted = dv_ingest::import(
            self.exe.as_deref(),
            self.ocr.as_ref(),
            &self.file_name,
            &self.bytes,
        )
        .map_err(|e| CoreError::Refused(e.message_he()));
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
        password: Zeroizing<String>,
    },
}

/// Exporting the report: it is put together from approved paragraphs under the lock, and
/// rendered (Word or PDF, and encrypted) in the job.
pub struct ExportJob {
    session: u64,
    case_id: String,
    report: dv_export::Report,
    check: crate::ExportCheck,
    format: ExportFormat,
}

/// The file's bytes (or why not), waiting for [`Core::finish_export`].
pub struct Exported {
    session: u64,
    case_id: String,
    check: crate::ExportCheck,
    pdf: bool,
    protected: bool,
    bytes: Result<Vec<u8>, CoreError>,
}

fn export_error(e: dv_export::ExportError) -> CoreError {
    match e {
        dv_export::ExportError::WeakPassword => CoreError::Refused(format!(
            "סיסמה לקובץ צריכה להיות באורך {} תווים לפחות.",
            dv_export::MIN_PASSWORD_CHARS
        )),
        dv_export::ExportError::Template(m) | dv_export::ExportError::Pdf(m) => {
            CoreError::Refused(m)
        }
        other => CoreError::Internal(other.to_string()),
    }
}

impl ExportJob {
    #[must_use]
    pub fn run(self) -> Exported {
        let (pdf, protected, bytes) = match &self.format {
            ExportFormat::Word { template, password } => {
                let docx = match template {
                    Some(t) => dv_export::render_with_template(&self.report, t),
                    None => dv_export::render(&self.report),
                }
                .map_err(export_error);
                let bytes = docx.and_then(|d| match password {
                    Some(pw) => dv_export::encrypt(&d, pw).map_err(export_error),
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
                            password,
                        )
                        .map_err(export_error)
                    });
                (true, true, bytes)
            }
        };
        Exported {
            session: self.session,
            case_id: self.case_id,
            check: self.check,
            pdf,
            protected,
            bytes,
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
    fn same_session(&self, session: u64) -> Result<(), CoreError> {
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
        self.after_unlock(opened.opened, true)
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
        self.failed_unlocks = 0;
        self.mark_secret_changed()?;
        Ok(kit)
    }

    // ------------------------------------------------------------ backup

    /// Check where the backup goes and take the snapshot. Writing it out is the task's.
    pub fn begin_backup(&mut self, dest: &Path) -> Result<BackupTask, CoreError> {
        let (dest, folder) = self.backup_destination(dest)?;
        let job = self.vault_mut()?.begin_backup()?;
        Ok(BackupTask {
            session: self.session,
            job,
            dest,
            folder,
            auto: false,
        })
    }

    /// Record a backup that was written and read back: it counts from now on.
    pub fn finish_backup(&mut self, run: BackupRun) -> Result<BackupDone, CoreError> {
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
        self.backup_recorded(&run.folder, done.info.created_at)?;
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
        Ok(RestoreJob {
            dir: self.dir.clone(),
            bytes: self.take_staged()?,
            proof,
        })
    }

    pub fn finish_restore(&mut self, restored: Restored) -> Result<AppStatus, CoreError> {
        if restored.vault.is_err() {
            // Put back unless it succeeded, so Einat can try another password.
            self.staged_backup = Some(restored.bytes);
        }
        if self.vault.is_some() {
            if let Ok(extra) = restored.vault {
                let _ = extra.lock();
            }
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
            exe: self.ingest_exe.clone(),
            ocr: self.ocr.clone(),
            file_name: file_name.to_owned(),
            bytes,
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
        read: DocumentRead,
    ) -> Result<crate::ImportPreview, CoreError> {
        self.same_session(read.session)?;
        self.preview_extracted(case_id, &read.file_name, read.extracted?)
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
        read: DocumentRead,
    ) -> Result<crate::StyleImportPreview, CoreError> {
        self.same_session(read.session)?;
        self.style_room()?;
        let extracted = read.extracted?;
        let work = match read.style {
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
    fn export_job(&mut self, case_id: &str, format: ExportFormat) -> Result<ExportJob, CoreError> {
        let (report, check) = self.build_report(case_id)?;
        if !check.blocking.is_empty() {
            return Err(CoreError::Refused(check.blocking.join(" · ")));
        }
        Ok(ExportJob {
            session: self.session,
            case_id: case_id.to_owned(),
            report,
            check,
            format,
        })
    }

    /// The Word file. Refused while anything blocks; with a password it is encrypted
    /// (ECMA-376 Agile, AES-256), the way Word protects files.
    pub fn begin_export_report(
        &mut self,
        case_id: &str,
        password: Option<&str>,
    ) -> Result<ExportJob, CoreError> {
        let template = self.template_bytes()?;
        self.export_job(
            case_id,
            ExportFormat::Word {
                template,
                password: password.map(|p| Zeroizing::new(p.to_owned())),
            },
        )
    }

    /// The report as a locked PDF (EX-3, D-049): it opens with `password`, can be printed but
    /// not changed, and its SHA-256 goes into the audit log so a copy can be checked later.
    pub fn begin_export_pdf(
        &mut self,
        case_id: &str,
        password: &str,
    ) -> Result<ExportJob, CoreError> {
        self.export_job(
            case_id,
            ExportFormat::Pdf {
                password: Zeroizing::new(password.to_owned()),
            },
        )
    }

    /// Record the export (the PDF with its fingerprint) and learn from the final report.
    /// Returns the file's bytes and the check, whose file name has no child's name in it.
    pub fn finish_export(
        &mut self,
        exported: Exported,
    ) -> Result<(Vec<u8>, crate::ExportCheck), CoreError> {
        self.same_session(exported.session)?;
        let bytes = exported.bytes?;
        let sections = exported.check.included_sections;
        let meta = if exported.pdf {
            serde_json::json!({
                "protected": true,
                "pdf": true,
                "sha256": dv_vault::crypto::sha256_hex(&bytes),
                "sections": sections,
            })
        } else {
            serde_json::json!({ "protected": exported.protected, "sections": sections })
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

    pub fn export_pdf(&mut self, case_id: &str, password: &str) -> Result<Vec<u8>, CoreError> {
        let job = self.begin_export_pdf(case_id, password)?;
        self.finish_export(job.run()).map(|(b, _)| b)
    }
}
