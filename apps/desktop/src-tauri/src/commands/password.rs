//! Password and recovery kit.

use crate::*;

/// `current` is the password, or the recovery kit when `with_recovery`.
#[tauri::command]
pub(crate) async fn change_password(
    state: tauri::State<'_, AppState>,
    current: String,
    with_recovery: bool,
    new_password: String,
) -> Res<()> {
    let current = zeroize::Zeroizing::new(current);
    let new_password = zeroize::Zeroizing::new(new_password);
    off_lock(
        &state,
        move |c| c.begin_change_password(&current, with_recovery, &new_password),
        |job| job.run(),
        |c, made| c.finish_change_password(made),
    )
    .await
}

#[tauri::command]
pub(crate) async fn new_recovery_kit(
    state: tauri::State<'_, AppState>,
    current: String,
    with_recovery: bool,
) -> Res<CreatedVault> {
    let current = zeroize::Zeroizing::new(current);
    off_lock(
        &state,
        move |c| c.begin_new_recovery_kit(&current, with_recovery),
        |job| job.run(),
        |c, made| c.finish_new_recovery_kit(made),
    )
    .await
}
