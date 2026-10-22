//! Settings: usage, keys, model, report settings.

use crate::*;

/// What must be true before real cases (stage 8): informs, never blocks.
#[tauri::command]
pub(crate) async fn readiness(state: tauri::State<'_, AppState>) -> Res<Readiness> {
    with_core(&state, |c| c.readiness()).await
}

#[tauri::command]
pub(crate) async fn confirm_readiness(
    state: tauri::State<'_, AppState>,
    key: String,
    done: bool,
) -> Res<Readiness> {
    with_core(&state, move |c| c.confirm_readiness(&key, done)).await
}

/// Settings → המבחנים: every instrument with her answer (D-098).
#[tauri::command]
pub(crate) async fn instrument_approvals(
    state: tauri::State<'_, AppState>,
) -> Res<Vec<dv_core::InstrumentApproval>> {
    with_core(&state, |c| c.instrument_approvals()).await
}

/// Her answer for one instrument: approved, or not given.
#[tauri::command]
pub(crate) async fn set_instrument_status(
    state: tauri::State<'_, AppState>,
    key: String,
    status: dv_core::InstrumentStatus,
) -> Res<Vec<dv_core::InstrumentApproval>> {
    with_core(&state, move |c| c.set_instrument_status(&key, status)).await
}

/// The security status list (S4): what is on, what is not.
#[tauri::command]
pub(crate) async fn security_status(state: tauri::State<'_, AppState>) -> Res<SecurityStatus> {
    with_core(&state, |c| Ok(c.security_status())).await
}

/// The emergency stop for sending to an outside AI (S7). Needs no unlocked vault.
#[tauri::command]
pub(crate) async fn set_send_stopped(
    state: tauri::State<'_, AppState>,
    stopped: bool,
) -> Res<SecurityStatus> {
    with_core(&state, move |c| c.set_send_stopped(stopped)).await
}

/// The texts being edited now (memory only), kept in the vault if it locks meanwhile.
#[tauri::command]
pub(crate) async fn hold_unsaved(
    state: tauri::State<'_, AppState>,
    edits: Vec<UnsavedEdit>,
) -> Res<()> {
    with_core(&state, move |c| {
        c.hold_unsaved(edits);
        Ok(())
    })
    .await
}

/// After entering: the edits kept at the last lock, offered once.
#[tauri::command]
pub(crate) async fn take_unsaved(state: tauri::State<'_, AppState>) -> Res<Vec<UnsavedEdit>> {
    with_core(&state, |c| c.take_unsaved()).await
}

/// This month's use of the AI (numbers only) and the monthly ceiling.
#[tauri::command]
pub(crate) async fn usage_summary(state: tauri::State<'_, AppState>) -> Res<UsageSummary> {
    with_core(&state, |c| c.usage_summary()).await
}

#[tauri::command]
pub(crate) async fn set_monthly_cap(
    state: tauri::State<'_, AppState>,
    cap_usd: Option<u32>,
) -> Res<()> {
    with_core(&state, move |c| c.set_monthly_cap(cap_usd)).await
}

#[tauri::command]
pub(crate) async fn set_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
    key: String,
    retention_ack: bool,
) -> Res<()> {
    with_core(&state, move |c| {
        c.set_api_key(&provider, &key, retention_ack)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_speed(state: tauri::State<'_, AppState>, speed: String) -> Res<()> {
    with_core(&state, move |c| c.set_speed(&speed)).await
}

#[tauri::command]
pub(crate) async fn set_model(state: tauri::State<'_, AppState>, model: String) -> Res<()> {
    with_core(&state, move |c| c.set_model(&model)).await
}

#[tauri::command]
pub(crate) async fn set_lock_minutes(state: tauri::State<'_, AppState>, minutes: u32) -> Res<()> {
    with_core(&state, move |c| c.set_lock_minutes(minutes)).await
}

#[tauri::command]
pub(crate) async fn section_modes(
    state: tauri::State<'_, AppState>,
) -> Res<Vec<dv_core::SectionModeView>> {
    with_core(&state, |c| c.section_modes()).await
}

#[tauri::command]
pub(crate) async fn set_section_mode(
    state: tauri::State<'_, AppState>,
    section_key: String,
    mode: dv_domain::SectionMode,
) -> Res<()> {
    with_core(&state, move |c| c.set_section_mode(&section_key, mode)).await
}

#[tauri::command]
pub(crate) async fn set_section_guide(
    state: tauri::State<'_, AppState>,
    section_key: String,
    text: String,
) -> Res<()> {
    with_core(&state, move |c| c.set_section_guide(&section_key, &text)).await
}

#[tauri::command]
pub(crate) async fn set_practitioner(
    state: tauri::State<'_, AppState>,
    names: Vec<String>,
) -> Res<()> {
    with_core(&state, move |c| c.set_practitioner(names)).await
}

#[tauri::command]
pub(crate) async fn set_review_always(state: tauri::State<'_, AppState>, on: bool) -> Res<()> {
    with_core(&state, move |c| c.set_review_always(on)).await
}

#[tauri::command]
pub(crate) async fn report_settings(state: tauri::State<'_, AppState>) -> Res<ReportSettings> {
    with_core(&state, |c| c.report_settings()).await
}

#[tauri::command]
pub(crate) async fn set_report_settings(
    state: tauri::State<'_, AppState>,
    settings: ReportSettings,
) -> Res<()> {
    with_core(&state, move |c| c.set_report_settings(&settings)).await
}

/// Her Word template: the file's bytes are the raw body, like an imported document.
#[tauri::command]
pub(crate) async fn set_report_template(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Res<TemplateView> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(internal("body"));
    };
    let bytes = bytes.clone();
    with_core(&state, move |c| c.set_report_template(&bytes)).await
}

#[tauri::command]
pub(crate) async fn clear_report_template(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.clear_report_template()).await
}

#[tauri::command]
pub(crate) async fn report_template(
    state: tauri::State<'_, AppState>,
) -> Res<Option<TemplateView>> {
    with_core(&state, |c| c.report_template()).await
}
