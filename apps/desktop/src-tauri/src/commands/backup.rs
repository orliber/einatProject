//! Backup (D-024).

use crate::*;

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
    off_lock(
        &state,
        move |c| c.begin_backup(&dest),
        |task| task.run(),
        |c, run| c.finish_backup(run).map(Some),
    )
    .await
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
    with_core(&state, move |c| {
        let size = std::fs::metadata(&path)
            .map_err(|e| CoreError::Refused(format!("אי אפשר לקרוא את הקובץ: {e}")))?
            .len();
        if size > dv_core::MAX_BACKUP_BYTES {
            return Err(CoreError::Refused(
                "הקובץ גדול מדי בשביל גיבוי של הכספת.".to_owned(),
            ));
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| CoreError::Refused(format!("אי אפשר לקרוא את הקובץ: {e}")))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        c.stage_backup(bytes, name).map(Some)
    })
    .await
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
