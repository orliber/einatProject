//! The optional name model (D-053): off by default, downloaded only when she turns it on.

use crate::*;
use dv_core::name_model::NameModelView;

#[tauri::command]
pub(crate) async fn name_model_status(state: tauri::State<'_, AppState>) -> Res<NameModelView> {
    with_core(&state, |c| c.name_model_status()).await
}

/// Download the pinned files without the core's lock (work goes on meanwhile), then turn the
/// model on. One download at a time.
#[tauri::command]
pub(crate) async fn download_name_model(state: tauri::State<'_, AppState>) -> Res<NameModelView> {
    let models = state.models.clone();
    let progress = Arc::clone(&state.model_progress);
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(mut slot) = progress.try_lock() else {
            return Err(UiError {
                code: "refused".to_owned(),
                message: "ההורדה כבר רצה.".to_owned(),
                details: Vec::new(),
                action: dv_core::ErrorAction::Dismiss,
            });
        };
        *slot = Some((0, 0));
        drop(slot);
        let report = |done: u64, total: u64| {
            if let Ok(mut p) = progress.lock() {
                *p = Some((done, total));
            }
        };
        let result = dv_core::name_model::download(&models, &report);
        if let Ok(mut p) = progress.lock() {
            *p = None;
        }
        result.map_err(|e| e.to_ui())
    })
    .await
    .map_err(|_| internal("task"))??;
    with_core(&state, |c| c.set_name_model(true)).await
}

/// Bytes downloaded so far and the total, while a download runs.
#[tauri::command]
pub(crate) async fn name_model_progress(
    state: tauri::State<'_, AppState>,
) -> Res<Option<(u64, u64)>> {
    Ok(state.model_progress.lock().ok().and_then(|p| *p))
}

/// Off: the model stops being used and its files are deleted.
#[tauri::command]
pub(crate) async fn set_name_model(
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Res<NameModelView> {
    with_core(&state, move |c| c.set_name_model(on)).await
}
