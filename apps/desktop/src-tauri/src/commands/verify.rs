//! Checking a draft against its sources (off until she turns it on in settings).

use crate::*;

#[tauri::command]
pub(crate) async fn verify_stage_on(state: tauri::State<'_, AppState>) -> Res<bool> {
    with_core(&state, |c| c.verify_stage_on()).await
}

#[tauri::command]
pub(crate) async fn set_verify_stage(state: tauri::State<'_, AppState>, on: bool) -> Res<()> {
    with_core(&state, move |c| c.set_verify_stage(on)).await
}

#[tauri::command]
pub(crate) async fn prepare_verify(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
) -> Res<Prepared> {
    warm_names(&state, &case_id, Vec::new()).await;
    with_core(&state, move |c| c.prepare_verify(&case_id, &section_key)).await
}

/// Same path as `send_section`: out of the core's lock, then the verdicts.
#[tauri::command]
pub(crate) async fn send_verify(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<VerifyResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| {
        c.begin_send(&id, dv_core::SendKind::Verify)
    })
    .await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_verify(out, response)).await
}
