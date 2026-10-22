//! The encrypted copy of each backup in her Google Drive (D-099). Off by default.

use std::sync::{Arc, Mutex};

use dv_core::drive_backup::{DriveBackupStatus, DriveCleanup, DriveCleanupResult, DriveUpload};

use crate::*;

/// Upload one copy with the core free, and note how it went. Nothing waits for it and nothing
/// is shown while it runs; a failure is a quiet line in the settings. One worker uploads, and
/// only the newest copy waits for it (each holds a whole backup in memory).
pub(crate) fn upload_in_background(core: Arc<Mutex<Core>>, upload: DriveUpload) {
    if !dv_core::drive_backup::queue_upload(upload) {
        return;
    }
    std::thread::spawn(move || {
        while let Some(up) = dv_core::drive_backup::next_upload() {
            let uploaded = match (
                dv_core::google::config(),
                dv_core::google::GoogleHttp::new(),
            ) {
                (Ok(cfg), Ok(http)) => up.run(&http, &cfg),
                _ => up.not_started(),
            };
            if let Ok(mut c) = core.lock() {
                let _ = c.finish_drive_upload(uploaded);
            }
        }
    });
}

/// Delete the copies and withdraw the permission at Google with the core free, then note how
/// it went. Also used by the timer, for a cleanup still owed.
fn clean_up(core: &Arc<Mutex<Core>>, cleanup: DriveCleanup) -> Option<DriveCleanupResult> {
    let (Ok(cfg), Ok(http)) = (
        dv_core::google::config(),
        dv_core::google::GoogleHttp::new(),
    ) else {
        // Not set up on this computer: still owed, tried again later.
        return Some(DriveCleanupResult::Pending);
    };
    let cleaned = cleanup.run(&http, &cfg);
    core.lock().ok()?.finish_drive_cleanup(cleaned).ok()
}

/// A cleanup still owed at Google, on its own thread: the timer must not wait for a slow line.
pub(crate) fn cleanup_in_background(core: Arc<Mutex<Core>>, cleanup: DriveCleanup) {
    std::thread::spawn(move || {
        let _ = clean_up(&core, cleanup);
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
/// in her browser and allows a lasting permission for the app's hidden folder. A permission
/// that is given but cannot be kept (the vault locked meanwhile, Google disconnected) is
/// withdrawn at Google at once, unless a cleanup is owed: that cleanup withdraws it.
#[tauri::command]
pub(crate) async fn drive_backup_turn_on(
    state: tauri::State<'_, AppState>,
) -> Res<DriveBackupStatus> {
    let (vault_id, tag, keep_grant) = with_core(&state, |c| {
        c.drive_backup_turn_on_check(&dv_core::google::WindowsAccount)
    })
    .await?;
    let kept_tag = tag.clone();
    let (http, token) = tauri::async_runtime::spawn_blocking(move || {
        let (http, session) = dv_core::google::sign_in_lasting(&open_in_browser, keep_grant)
            .map_err(|e| e.to_ui())?;
        let token =
            dv_core::drive_backup::lasting_permission(&http, session, &vault_id, &tag, keep_grant)
                .map_err(|e| e.to_ui())?;
        Ok::<_, UiError>((http, token))
    })
    .await
    .map_err(|_| internal("task"))??;
    let kept = token.clone();
    let finished = with_core(&state, move |c| {
        c.drive_backup_turn_on_finish(&dv_core::google::WindowsAccount, &kept_tag, &kept)
    })
    .await;
    if let Err(e) = finished {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            dv_core::drive_backup::withdraw_unkept(&http, &token, keep_grant);
        })
        .await;
        return Err(e);
    }
    with_core(&state, |c| {
        c.drive_backup_status(&dv_core::google::WindowsAccount)
    })
    .await
}

/// Turn it off: the permission is forgotten at once; then the copies in her Drive are deleted
/// and the permission withdrawn at Google. Answers how far that got: done, still owed (tried
/// again while the vault is open), or out of reach (the permission was withdrawn at Google).
#[tauri::command]
pub(crate) async fn drive_backup_turn_off(
    state: tauri::State<'_, AppState>,
) -> Res<DriveCleanupResult> {
    let Some(cleanup) = with_core(&state, |c| c.drive_backup_turn_off()).await? else {
        return Ok(DriveCleanupResult::Off);
    };
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        clean_up(&core, cleanup).unwrap_or(DriveCleanupResult::Pending)
    })
    .await
    .map_err(|_| internal("task"))
}
