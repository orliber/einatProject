//! Writing style (D-043).

use crate::*;

#[tauri::command]
pub(crate) async fn style_overview(state: tauri::State<'_, AppState>) -> Res<StyleOverview> {
    with_core(&state, |c| c.style_overview()).await
}

/// A past report: the bytes arrive as the raw request body, the file name as a header.
#[tauri::command]
pub(crate) async fn import_style_source(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Res<StyleImportPreview> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(internal("body"));
    };
    let file_name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .unwrap_or_default();
    let bytes = bytes.clone();
    with_core(&state, move |c| c.import_style_source(&file_name, &bytes)).await
}

#[tauri::command]
pub(crate) async fn save_style_source(
    state: tauri::State<'_, AppState>,
    token: String,
    included: Vec<u32>,
    title: Option<String>,
) -> Res<StyleSourceView> {
    with_core(&state, move |c| {
        c.save_style_source(&token, &included, title.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn discard_style_upload(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| {
        c.discard_style_upload();
        Ok(())
    })
    .await
}

#[tauri::command]
pub(crate) async fn delete_style_source(state: tauri::State<'_, AppState>, id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_style_source(&id)).await
}

#[tauri::command]
pub(crate) async fn prepare_style_analysis(
    state: tauri::State<'_, AppState>,
    source_id: String,
) -> Res<Prepared> {
    with_core(&state, move |c| c.prepare_style_analysis(&source_id)).await
}

#[tauri::command]
pub(crate) async fn send_style_analysis(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<StyleAnalysisResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_style_analysis(out, response)).await
}

#[tauri::command]
pub(crate) async fn prepare_style_profile(state: tauri::State<'_, AppState>) -> Res<Prepared> {
    with_core(&state, |c| c.prepare_style_profile()).await
}

#[tauri::command]
pub(crate) async fn send_style_profile(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<StyleProfileView> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_style_profile(out, response)).await
}

#[tauri::command]
pub(crate) async fn save_style_draft(
    state: tauri::State<'_, AppState>,
    profile: dv_core::StyleProfile,
) -> Res<StyleProfileView> {
    with_core(&state, move |c| c.save_style_draft(profile)).await
}

#[tauri::command]
pub(crate) async fn approve_style_draft(
    state: tauri::State<'_, AppState>,
) -> Res<StyleProfileView> {
    with_core(&state, |c| c.approve_style_draft()).await
}

#[tauri::command]
pub(crate) async fn discard_style_draft(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.discard_style_draft()).await
}

#[tauri::command]
pub(crate) async fn restore_style_version(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Res<StyleProfileView> {
    with_core(&state, move |c| c.restore_style_version(&id)).await
}

#[tauri::command]
pub(crate) async fn set_style_enabled(state: tauri::State<'_, AppState>, on: bool) -> Res<()> {
    with_core(&state, move |c| c.set_style_enabled(on)).await
}

#[tauri::command]
pub(crate) async fn reset_style(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.reset_style()).await
}

#[tauri::command]
pub(crate) async fn accept_style_suggestion(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Res<StyleProfileView> {
    with_core(&state, move |c| c.accept_style_suggestion(&id)).await
}

#[tauri::command]
pub(crate) async fn dismiss_style_suggestion(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Res<()> {
    with_core(&state, move |c| c.dismiss_style_suggestion(&id)).await
}
