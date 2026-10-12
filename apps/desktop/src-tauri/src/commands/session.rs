//! Lifecycle: status, create, unlock, lock.

use crate::*;

#[tauri::command]
pub(crate) fn ping() -> dv_ipc::PingResponse {
    dv_core::ping()
}

#[tauri::command]
pub(crate) async fn app_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Res<AppStatus> {
    let status = with_core(&state, |c| Ok(c.status())).await?;
    protect(&window, status.screen_protection);
    Ok(status)
}

/// Hiding the window from screenshots and screen sharing is switched off for now (D-039):
/// it blacked out Zoom and Teams. Setting this back to `true` restores D-037 as it was.
pub(crate) const SCREEN_PROTECTION_ENABLED: bool = false;

/// Screenshots and screen sharing see a blank window unless she turned that off (D-037).
pub(crate) fn protect(window: &tauri::WebviewWindow, on: bool) {
    let _ = window.set_content_protected(SCREEN_PROTECTION_ENABLED && on);
}

#[tauri::command]
pub(crate) async fn set_screen_protection(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Res<()> {
    with_core(&state, move |c| c.set_screen_protection(on)).await?;
    protect(&window, on);
    Ok(())
}

#[tauri::command]
pub(crate) async fn create_vault(
    state: tauri::State<'_, AppState>,
    password: String,
) -> Res<CreatedVault> {
    with_core(&state, move |c| c.create_vault(&password)).await
}

#[tauri::command]
pub(crate) async fn confirm_recovery_key(
    state: tauri::State<'_, AppState>,
    typed: String,
) -> Res<bool> {
    with_core(&state, move |c| c.confirm_recovery_key(&typed)).await
}

#[tauri::command]
pub(crate) async fn unlock(state: tauri::State<'_, AppState>, password: String) -> Res<AppStatus> {
    with_core(&state, move |c| c.unlock(&password)).await
}

#[tauri::command]
pub(crate) async fn unlock_with_recovery(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Res<AppStatus> {
    with_core(&state, move |c| c.unlock_with_recovery(&key)).await
}

#[tauri::command]
pub(crate) async fn lock(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Res<()> {
    state.clipboard.clear_now();
    protect(&window, true);
    with_core(&state, |c| {
        c.lock();
        Ok(())
    })
    .await
}

/// The psychologist is working in the window (typing, scrolling) without calling the core.
#[tauri::command]
pub(crate) async fn touch(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| {
        c.touch();
        Ok(())
    })
    .await
}
