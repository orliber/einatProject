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
    with_core(&state, move |c| {
        c.change_password(&current, with_recovery, &new_password)
    })
    .await
}

#[tauri::command]
pub(crate) async fn new_recovery_kit(
    state: tauri::State<'_, AppState>,
    current: String,
    with_recovery: bool,
) -> Res<CreatedVault> {
    let current = zeroize::Zeroizing::new(current);
    with_core(&state, move |c| c.new_recovery_kit(&current, with_recovery)).await
}
