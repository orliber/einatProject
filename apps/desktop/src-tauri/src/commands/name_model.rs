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
        if !claim_download(&progress) {
            return Err(UiError {
                code: "refused".to_owned(),
                message: "ההורדה כבר רצה.".to_owned(),
                details: Vec::new(),
                action: dv_core::ErrorAction::Dismiss,
            });
        }
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

/// Mark a download as running, unless one already is (OPS-2). The mark stays from the click
/// until the download ends, also between the progress reports; a second download beside the
/// first would write into the same `.part` files.
fn claim_download(progress: &Mutex<Option<(u64, u64)>>) -> bool {
    let mut slot = progress
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_some() {
        return false;
    }
    *slot = Some((0, 0));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_download_is_refused_while_one_runs() {
        let progress = Mutex::new(None);
        assert!(claim_download(&progress));
        assert!(!claim_download(&progress), "the first is still running");
        *progress.lock().unwrap() = Some((10, 100));
        assert!(!claim_download(&progress), "still running, between reports");
        *progress.lock().unwrap() = None;
        assert!(
            claim_download(&progress),
            "the first ended: a new one may start"
        );
    }
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
