//! Lifecycle: status, create, unlock, lock.

use crate::*;

#[tauri::command]
pub(crate) fn ping() -> dv_ipc::PingResponse {
    dv_core::ping()
}

/// The last crashes, for the bug report she copies for Or. Read from the crash log next to the
/// vault folder (see `remember_crashes`); works while the vault is locked.
#[tauri::command]
pub(crate) fn recent_crashes(state: tauri::State<'_, AppState>) -> Vec<String> {
    use std::io::Read;
    let Some(path) = state.dir.parent().map(|d| d.join(CRASH_LOG)) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|f| f.take(CRASH_LOG_MAX * 2).read_to_end(&mut bytes));
    if read.is_err() {
        return Vec::new();
    }
    dv_core::recent_crashes(&String::from_utf8_lossy(&bytes))
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

/// What the window was last told: 0 nothing yet, 1 not protected, 2 protected.
static PROTECTED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Screenshots and screen sharing see a blank window unless she turned that off (D-037).
/// The status is read every 15 seconds; Windows is told only when the answer changes, so
/// the window is not touched while she types.
pub(crate) fn protect(window: &tauri::WebviewWindow, on: bool) {
    use std::sync::atomic::Ordering;
    let on = SCREEN_PROTECTION_ENABLED && on;
    let state = if on { 2 } else { 1 };
    if PROTECTED.swap(state, Ordering::SeqCst) == state {
        return;
    }
    if window.set_content_protected(on).is_err() {
        // Try again next time.
        PROTECTED.store(0, Ordering::SeqCst);
    }
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
    let password = zeroize::Zeroizing::new(password);
    off_lock(
        &state,
        move |c| c.begin_unlock(&password),
        |job| job.run(),
        |c, opened| c.finish_unlock(opened),
    )
    .await
}

#[tauri::command]
pub(crate) async fn unlock_with_recovery(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Res<AppStatus> {
    let key = zeroize::Zeroizing::new(key);
    off_lock(
        &state,
        move |c| c.begin_unlock_with_recovery(&key),
        |job| job.run(),
        |c, opened| c.finish_unlock(opened),
    )
    .await
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
