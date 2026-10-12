//! Free consultation with the AI.

use crate::*;

#[tauri::command]
pub(crate) async fn prepare_consult(
    state: tauri::State<'_, AppState>,
    case_id: Option<String>,
    conversation_id: Option<String>,
    message: String,
) -> Res<Prepared> {
    with_core(&state, move |c| {
        c.prepare_consult(case_id.as_deref(), conversation_id.as_deref(), &message)
    })
    .await
}

#[tauri::command]
pub(crate) async fn consultations(
    state: tauri::State<'_, AppState>,
) -> Res<Vec<ConsultationSummary>> {
    with_core(&state, |c| c.consultations()).await
}

#[tauri::command]
pub(crate) async fn consultation(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Res<ConsultationView> {
    with_core(&state, move |c| c.consultation(&id)).await
}

#[tauri::command]
pub(crate) async fn delete_consultation(state: tauri::State<'_, AppState>, id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_consultation(&id)).await
}

#[tauri::command]
pub(crate) async fn send_consult(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<ConsultResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| {
        c.begin_send(&id, dv_core::SendKind::Consult)
    })
    .await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_consult(out, response)).await
}
