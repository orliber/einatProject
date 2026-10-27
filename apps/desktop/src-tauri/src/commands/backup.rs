//! Backup (D-024).

use std::sync::mpsc::{sync_channel, SyncSender};

use crate::*;

/// The automatic backup, on a worker of its own (D-119). The shell's timer only nudges it and
/// never waits for it: a backup to a slow drive takes longer than the gap the timer reads as
/// "the computer slept", and while the timer waited, the idle lock, the lock with the computer
/// and the last-minute warning waited too. One worker means one backup at a time; while it
/// writes, at most one nudge waits, and when it is done that nudge finds nothing due.
pub(crate) struct AutoBackup {
    nudge: SyncSender<()>,
}

impl AutoBackup {
    /// Start the worker. `backed_up` is called after each backup that was recorded.
    pub(crate) fn start(core: Arc<Mutex<Core>>, backed_up: impl Fn() + Send + 'static) -> Self {
        Self::with_job(move || {
            if auto_backup_once(&core) {
                backed_up();
            }
        })
    }

    /// The worker around any job (the tests give it one that waits).
    fn with_job(mut job: impl FnMut() + Send + 'static) -> Self {
        let (nudge, nudged) = sync_channel::<()>(1);
        std::thread::spawn(move || {
            for () in nudged {
                job();
            }
        });
        Self { nudge }
    }

    /// See whether a backup is due. Returns at once, whatever the worker is doing.
    pub(crate) fn nudge(&self) {
        let _ = self.nudge.try_send(());
    }
}

/// A backup that is due goes by itself to the drive of the last one, if it is connected (see
/// `Core::auto_backup`). Only taking the snapshot and recording it hold the core; writing to
/// the drive and reading it back run with the core free (F2). A failure waits and is shown
/// nowhere: the backup reminder stays until a backup is made. True when one was recorded.
fn auto_backup_once(core: &Arc<Mutex<Core>>) -> bool {
    let task = core
        .lock()
        .ok()
        .and_then(|mut c| c.begin_auto_backup().ok().flatten());
    let Some(task) = task else {
        return false;
    };
    let run = task.run();
    let Ok(mut c) = core.lock() else {
        return false;
    };
    let (done, upload) = c.finish_backup_for_drive(run);
    drop(c);
    // The encrypted copy in her Drive, when on (D-099), on its own thread too.
    if let Some(up) = upload {
        super::upload_in_background(Arc::clone(core), up);
    }
    done.is_ok()
}

#[tauri::command]
pub(crate) async fn backup_status(state: tauri::State<'_, AppState>) -> Res<BackupStatus> {
    with_core(&state, |c| c.backup_status()).await
}

/// Ask where to save (the system's "Save as"), then write the encrypted backup there.
/// `None` when Einat cancelled.
#[tauri::command]
pub(crate) async fn write_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Res<Option<BackupDone>> {
    let start = with_core(&state, |c| c.backup_dir()).await?;
    let start = start.or_else(|| app.path().document_dir().ok());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let Some(dest) = file_dialog::save(&app, start, dv_core::backup_file_name(now)).await else {
        return Ok(None);
    };
    let (done, upload) = off_lock(
        &state,
        move |c| c.begin_backup(&dest),
        |task| task.run(),
        |c, run| Ok(c.finish_backup_for_drive(run)),
    )
    .await?;
    // The encrypted copy in her Drive, when on (D-099): after the backup is recorded, with
    // the core free, and never holding up or undoing the backup.
    if let Some(upload) = upload {
        super::upload_in_background(Arc::clone(&state.core), upload);
    }
    done.map(Some).map_err(|e| e.to_ui())
}

/// Ask for a backup file (the system's "Open") and say what it is. `None` when cancelled.
#[tauri::command]
pub(crate) async fn choose_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Res<Option<StagedBackup>> {
    // Locked or no vault yet: start in Documents.
    let start = with_core(&state, |c| Ok(c.backup_dir().ok().flatten()))
        .await?
        .or_else(|| app.path().document_dir().ok());
    let Some(path) = file_dialog::open(&app, start).await else {
        return Ok(None);
    };
    // Read with the core free (a slow drive holds up nothing else); a file error is said by
    // its kind, never with what the system wrote (the path, D-120).
    let read =
        tauri::async_runtime::spawn_blocking(move || -> Result<(Vec<u8>, String), CoreError> {
            let size = std::fs::metadata(&path).map_err(dv_core::file_error)?.len();
            if size > dv_core::MAX_BACKUP_BYTES {
                return Err(CoreError::Refused(
                    "הקובץ גדול מדי בשביל גיבוי של הכספת.".to_owned(),
                ));
            }
            let bytes = std::fs::read(&path).map_err(dv_core::file_error)?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok((bytes, name))
        })
        .await
        .map_err(|_| internal("task"))?;
    let (bytes, name) = read.map_err(|e| e.to_ui())?;
    with_core(&state, move |c| c.stage_backup(bytes, name).map(Some)).await
}

/// Restore drill on the chosen file, with the password.
/// The automatic backup to the drive of the last backup, on or off.
#[tauri::command]
pub(crate) async fn set_auto_backup(
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Res<BackupStatus> {
    with_core(&state, move |c| c.set_auto_backup(on)).await
}

#[tauri::command]
pub(crate) async fn check_backup(
    state: tauri::State<'_, AppState>,
    password: String,
) -> Res<BackupCheckView> {
    let password = zeroize::Zeroizing::new(password);
    off_lock(
        &state,
        move |c| c.begin_check_backup(&password),
        |job| job.run(),
        |c, drilled| c.finish_check_backup(drilled),
    )
    .await
}

/// On a computer with no vault: restore the chosen file and open it.
#[tauri::command]
pub(crate) async fn restore_backup(
    state: tauri::State<'_, AppState>,
    password: Option<String>,
    recovery_key: Option<String>,
) -> Res<AppStatus> {
    let password = password.map(zeroize::Zeroizing::new);
    let recovery_key = recovery_key.map(zeroize::Zeroizing::new);
    off_lock(
        &state,
        move |c| {
            c.begin_restore(
                password.as_deref().map(String::as_str),
                recovery_key.as_deref().map(String::as_str),
            )
        },
        |job| job.run(),
        |c, restored| c.finish_restore(restored),
    )
    .await
}

#[tauri::command]
pub(crate) async fn forget_backup(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| {
        c.forget_staged_backup();
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    #[test]
    fn a_slow_backup_never_holds_up_the_timer_and_runs_one_at_a_time() {
        let running = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let runs = Arc::new(AtomicUsize::new(0));
        let (release, wait) = channel::<()>();
        let wait = Mutex::new(wait);
        let (started, first_started) = channel::<()>();
        let backup = {
            let (running, most, runs) =
                (Arc::clone(&running), Arc::clone(&most), Arc::clone(&runs));
            AutoBackup::with_job(move || {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                let _ = started.send(());
                // A drive that takes as long as the test says.
                let _ = wait.lock().unwrap().recv();
                running.fetch_sub(1, Ordering::SeqCst);
                runs.fetch_add(1, Ordering::SeqCst);
            })
        };
        backup.nudge();
        first_started.recv_timeout(Duration::from_secs(5)).unwrap();
        // The timer goes on nudging while the backup writes: every nudge returns at once.
        let at = Instant::now();
        for _ in 0..20 {
            backup.nudge();
        }
        assert!(at.elapsed() < Duration::from_secs(1));
        // Let the first backup and the one nudge kept for after it finish.
        release.send(()).unwrap();
        first_started.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while runs.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(runs.load(Ordering::SeqCst), 2, "one kept nudge, not twenty");
        assert_eq!(most.load(Ordering::SeqCst), 1, "never two at once");
    }

    #[test]
    fn with_nothing_due_the_worker_does_nothing() {
        let dir = std::env::temp_dir().join(format!("dv-auto-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let core = Arc::new(Mutex::new(Core::for_tests(&dir, None)));
        // No vault yet: nothing is due, nothing is written.
        assert!(!auto_backup_once(&core));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
