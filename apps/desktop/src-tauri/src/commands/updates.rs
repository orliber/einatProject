//! Updates (D-033).

use crate::*;

/// Is there a newer version? Nothing from the vault is involved; the core's lock is not held.
#[tauri::command]
pub(crate) async fn check_update() -> Res<Option<dv_core::update::UpdateView>> {
    tauri::async_runtime::spawn_blocking(|| dv_core::update::check().map_err(|e| e.to_ui()))
        .await
        .map_err(|_| internal("task"))?
}

/// A version that cannot open the vault (R7): the page of released versions, in her browser,
/// to install the one before. Works with the vault locked; nothing from it is involved.
#[tauri::command]
pub(crate) fn open_releases_page() -> Res<()> {
    super::recovery::open_in_browser(dv_core::update::RELEASES_PAGE).map_err(|e| e.to_ui())
}

/// Fetch and verify the newest version in the background, so the click only restarts
/// (D-038). Nothing is installed here. `true` when a verified installer is waiting.
#[tauri::command]
pub(crate) async fn prepare_update(state: tauri::State<'_, AppState>) -> Res<bool> {
    if !cfg!(windows) {
        return Ok(false);
    }
    let dir = state.dir.clone();
    let prepared = Arc::clone(&state.prepared);
    tauri::async_runtime::spawn_blocking(move || {
        let mut slot = prepared.lock().map_err(|_| internal("update"))?;
        let ready = dv_core::update::prepare(&dir, slot.take()).map_err(|e| e.to_ui())?;
        *slot = ready;
        Ok(slot.is_some())
    })
    .await
    .map_err(|_| internal("task"))?
}

/// Lock the vault and run the verified installer, then close. The installer (passive, update
/// mode) replaces the program and opens it again; the vault folder is not touched. A version
/// fetched in the background is used when it is still intact; otherwise it is fetched now.
#[tauri::command]
pub(crate) async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Res<()> {
    if !cfg!(windows) {
        return Err(UiError {
            code: "update".to_owned(),
            message: "כאן העדכון לא מותקן לבד. מתקינים את הגרסה החדשה מהקישור ששלח אור.".to_owned(),
            details: Vec::new(),
        });
    }
    let dir = state.dir.clone();
    let prepared = Arc::clone(&state.prepared);
    let file = tauri::async_runtime::spawn_blocking(move || {
        let waiting = prepared.lock().ok().and_then(|mut p| p.take());
        match waiting {
            Some(file) if dv_core::update::still_intact(&file) => Ok(file),
            _ => dv_core::update::download(&dir),
        }
    })
    .await
    .map_err(|_| internal("task"))?
    .map_err(|e| e.to_ui())?;
    if !dv_core::update::still_intact(&file) {
        return Err(internal("the installer changed after it was checked"));
    }
    state.clipboard.clear_now();
    with_core(&state, |c| {
        c.lock();
        Ok(())
    })
    .await?;
    std::process::Command::new(&file.path)
        .args(["/P", "/UPDATE", "/R"])
        .spawn()
        .map_err(|_| internal("the installer did not start"))?;
    app.exit(0);
    Ok(())
}
