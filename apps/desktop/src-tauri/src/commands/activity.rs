//! Activity log and retention.

use crate::*;

#[tauri::command]
pub(crate) async fn activity(
    state: tauri::State<'_, AppState>,
    before: Option<i64>,
) -> Res<ActivityPage> {
    with_core(&state, move |c| c.activity(before)).await
}

/// "מה יצא מהמחשב": the requests kept with a case (one, or all), newest first.
#[tauri::command]
pub(crate) async fn sent_history(
    state: tauri::State<'_, AppState>,
    case_id: Option<String>,
    place: Option<String>,
    query: String,
) -> Res<Vec<SentItem>> {
    with_core(&state, move |c| {
        c.sent_history(case_id.as_deref(), place.as_deref(), &query)
    })
    .await
}

/// One request in full, as it left.
#[tauri::command]
pub(crate) async fn sent_text(
    state: tauri::State<'_, AppState>,
    case_id: String,
    id: String,
) -> Res<SentText> {
    with_core(&state, move |c| c.sent_text(&case_id, &id)).await
}

#[tauri::command]
pub(crate) async fn mark_activity_reviewed(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.mark_activity_reviewed()).await
}

#[tauri::command]
pub(crate) async fn retention_due(state: tauri::State<'_, AppState>) -> Res<Vec<RetentionItem>> {
    with_core(&state, |c| c.retention_due()).await
}

#[tauri::command]
pub(crate) async fn keep_case_longer(
    state: tauri::State<'_, AppState>,
    case_id: String,
    years: u8,
) -> Res<()> {
    with_core(&state, move |c| c.keep_case_longer(&case_id, years)).await
}

/// Print the page with the system's own print window. `window.print()` does nothing in the
/// macOS webview, so the recovery kit could not be printed there.
#[tauri::command]
pub(crate) fn print_page(window: tauri::WebviewWindow) -> Res<()> {
    window.print().map_err(|e| internal(&e.to_string()))
}
