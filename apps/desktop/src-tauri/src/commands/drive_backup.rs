//! The encrypted copy of each backup in her Google Drive (D-099). Off by default.

use std::sync::{Arc, Mutex};

use dv_core::drive_backup::{DriveBackupStatus, DriveUpload};

use crate::*;

/// Upload one copy on its own thread, with the core free, and note how it went. Nothing waits
/// for it and nothing is shown while it runs; a failure is a quiet line in the settings.
pub(crate) fn upload_in_background(core: Arc<Mutex<Core>>, upload: DriveUpload) {
    std::thread::spawn(move || {
        let (Ok(cfg), Ok(http)) = (
            dv_core::google::config(),
            dv_core::google::GoogleHttp::new(),
        ) else {
            return;
        };
        let uploaded = upload.run(&http, &cfg);
        if let Ok(mut c) = core.lock() {
            let _ = c.finish_drive_upload(uploaded);
        }
    });
}

#[tauri::command]
pub(crate) async fn drive_backup_status(
    state: tauri::State<'_, AppState>,
) -> Res<DriveBackupStatus> {
    with_core(&state, |c| {
        c.drive_backup_status(&dv_core::google::WindowsAccount)
    })
    .await
}

/// Turn it on: her Google account must be connected here; she signs in with the same account
/// in her browser and allows a lasting permission for the app's hidden folder.
#[tauri::command]
pub(crate) async fn drive_backup_turn_on(
    state: tauri::State<'_, AppState>,
) -> Res<DriveBackupStatus> {
    let (vault_id, tag) = with_core(&state, |c| {
        c.drive_backup_turn_on_check(&dv_core::google::WindowsAccount)
    })
    .await?;
    let token = tauri::async_runtime::spawn_blocking(move || {
        let (http, session) =
            dv_core::google::sign_in_lasting(&open_in_browser).map_err(|e| e.to_ui())?;
        dv_core::drive_backup::lasting_permission(&http, session, &vault_id, &tag)
            .map_err(|e| e.to_ui())
    })
    .await
    .map_err(|_| internal("task"))??;
    with_core(&state, move |c| {
        c.drive_backup_turn_on_finish(&token)?;
        c.drive_backup_status(&dv_core::google::WindowsAccount)
    })
    .await
}

/// Turn it off: the permission is forgotten at once; then the copies in her Drive are deleted
/// and the permission withdrawn at Google. Answers whether the copies were deleted.
#[tauri::command]
pub(crate) async fn drive_backup_turn_off(state: tauri::State<'_, AppState>) -> Res<bool> {
    let Some(cleanup) = with_core(&state, |c| c.drive_backup_turn_off()).await? else {
        return Ok(true);
    };
    tauri::async_runtime::spawn_blocking(move || {
        let (Ok(cfg), Ok(http)) = (
            dv_core::google::config(),
            dv_core::google::GoogleHttp::new(),
        ) else {
            return false;
        };
        cleanup.run(&http, &cfg)
    })
    .await
    .map_err(|_| internal("task"))
}
