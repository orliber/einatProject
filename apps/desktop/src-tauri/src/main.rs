//! Desktop shell. Every command delegates to `dv-core`; the shell adds only what needs the
//! OS: the app's data folder, the Downloads folder for the report, the worker binary, and a
//! timer that locks the vault when idle.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod file_dialog;
mod os_lock;
mod secret_clipboard;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dv_core::{
    ActivityPage, AppStatus, BackupCheckView, BackupDone, BackupStatus, CaseDetail, ChatView,
    ConsultResult, ConsultationSummary, ConsultationView, Core, CoreError, CreatedVault,
    ExportCheck, ImportPreview, NameMatch, Prepared, Readiness, ReportSettings, RetentionItem,
    SectionResult, SortResult, StagedBackup, StyleAnalysisResult, StyleImportPreview,
    StyleOverview, StyleProfileView, StyleSourceView, SuspectDecision, TemplateView, UiError,
    UnsavedEdit, UsageSummary,
};
use dv_domain::{
    CaseInput, CaseMeta, CaseSummary, Folder, Identity, IdentityInput, InputKind, Role,
};
use tauri::Manager;

struct AppState {
    core: Arc<Mutex<Core>>,
    clipboard: secret_clipboard::SecretClipboard,
    /// The app's data folder: the vault, and the verified installer of an update (D-033).
    dir: PathBuf,
    /// Words Claude has written so far, per request on its way (D-035). Counts only.
    progress: Arc<Mutex<std::collections::HashMap<String, u32>>>,
    /// A newer version fetched and verified in the background, waiting for her click (D-038).
    /// Held while it is being fetched, so two fetches never run at once.
    prepared: Arc<Mutex<Option<dv_core::update::Downloaded>>>,
}

/// Send outside the core's lock, counting the words as the answer arrives.
async fn transmit(
    state: &AppState,
    approval_id: String,
    out: dv_core::Outgoing,
) -> Res<(
    dv_core::Outgoing,
    Result<(serde_json::Value, bool), CoreError>,
)> {
    let progress = Arc::clone(&state.progress);
    let done = Arc::clone(&state.progress);
    let id = approval_id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let report = |words: u32| {
            if let Ok(mut p) = progress.lock() {
                p.insert(id.clone(), words);
            }
        };
        let r = out.transmit_with(&report);
        (out, r)
    })
    .await
    .map_err(|_| internal("send"));
    if let Ok(mut p) = done.lock() {
        p.remove(&approval_id);
    }
    result
}

/// How many words Claude has written so far for a request on its way.
#[tauri::command]
fn send_progress(state: tauri::State<'_, AppState>, approval_id: String) -> u32 {
    state
        .progress
        .lock()
        .ok()
        .and_then(|p| p.get(&approval_id).copied())
        .unwrap_or(0)
}

type Res<T> = Result<T, UiError>;

fn internal(what: &str) -> UiError {
    UiError {
        code: "internal".to_owned(),
        message: format!("שגיאה פנימית: {what}"),
        details: Vec::new(),
    }
}

/// Run `f` on the core off the UI thread.
async fn with_core<T, F>(state: &AppState, f: F) -> Res<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Core) -> Result<T, CoreError> + Send + 'static,
{
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = core.lock().map_err(|_| internal("lock"))?;
        f(&mut c).map_err(|e| e.to_ui())
    })
    .await
    .map_err(|_| internal("task"))?
}

// ------------------------------------------------------------------ lifecycle

#[tauri::command]
fn ping() -> dv_ipc::PingResponse {
    dv_core::ping()
}

#[tauri::command]
async fn app_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Res<AppStatus> {
    let status = with_core(&state, |c| Ok(c.status())).await?;
    protect(&window, status.screen_protection);
    Ok(status)
}

/// Hiding the window from screenshots and screen sharing is switched off for now (D-039):
/// it blacked out Zoom and Teams. Setting this back to `true` restores D-037 as it was.
const SCREEN_PROTECTION_ENABLED: bool = false;

/// Screenshots and screen sharing see a blank window unless she turned that off (D-037).
fn protect(window: &tauri::WebviewWindow, on: bool) {
    let _ = window.set_content_protected(SCREEN_PROTECTION_ENABLED && on);
}

#[tauri::command]
async fn set_screen_protection(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Res<()> {
    with_core(&state, move |c| c.set_screen_protection(on)).await?;
    protect(&window, on);
    Ok(())
}

#[tauri::command]
async fn create_vault(state: tauri::State<'_, AppState>, password: String) -> Res<CreatedVault> {
    with_core(&state, move |c| c.create_vault(&password)).await
}

#[tauri::command]
async fn confirm_recovery_key(state: tauri::State<'_, AppState>, typed: String) -> Res<bool> {
    with_core(&state, move |c| c.confirm_recovery_key(&typed)).await
}

#[tauri::command]
async fn unlock(state: tauri::State<'_, AppState>, password: String) -> Res<AppStatus> {
    with_core(&state, move |c| c.unlock(&password)).await
}

#[tauri::command]
async fn unlock_with_recovery(state: tauri::State<'_, AppState>, key: String) -> Res<AppStatus> {
    with_core(&state, move |c| c.unlock_with_recovery(&key)).await
}

#[tauri::command]
async fn lock(window: tauri::WebviewWindow, state: tauri::State<'_, AppState>) -> Res<()> {
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
async fn touch(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| {
        c.touch();
        Ok(())
    })
    .await
}

// ------------------------------------------------------------------ settings

/// What must be true before real cases (stage 8): informs, never blocks.
#[tauri::command]
async fn readiness(state: tauri::State<'_, AppState>) -> Res<Readiness> {
    with_core(&state, |c| c.readiness()).await
}

#[tauri::command]
async fn confirm_readiness(
    state: tauri::State<'_, AppState>,
    key: String,
    done: bool,
) -> Res<Readiness> {
    with_core(&state, move |c| c.confirm_readiness(&key, done)).await
}

/// The paragraph being edited now (memory only), kept in the vault if it locks meanwhile.
#[tauri::command]
async fn hold_unsaved(state: tauri::State<'_, AppState>, edit: Option<UnsavedEdit>) -> Res<()> {
    with_core(&state, move |c| {
        c.hold_unsaved(edit);
        Ok(())
    })
    .await
}

/// After entering: the edit kept at the last lock, offered once.
#[tauri::command]
async fn take_unsaved(state: tauri::State<'_, AppState>) -> Res<Option<UnsavedEdit>> {
    with_core(&state, |c| c.take_unsaved()).await
}

/// This month's use of the AI (numbers only) and the monthly ceiling.
#[tauri::command]
async fn usage_summary(state: tauri::State<'_, AppState>) -> Res<UsageSummary> {
    with_core(&state, |c| c.usage_summary()).await
}

#[tauri::command]
async fn set_monthly_cap(state: tauri::State<'_, AppState>, cap_usd: Option<u32>) -> Res<()> {
    with_core(&state, move |c| c.set_monthly_cap(cap_usd)).await
}

#[tauri::command]
async fn set_api_key(state: tauri::State<'_, AppState>, key: String) -> Res<()> {
    with_core(&state, move |c| c.set_api_key(&key)).await
}

#[tauri::command]
async fn set_speed(state: tauri::State<'_, AppState>, speed: String) -> Res<()> {
    with_core(&state, move |c| c.set_speed(&speed)).await
}

#[tauri::command]
async fn set_model(state: tauri::State<'_, AppState>, model: String) -> Res<()> {
    with_core(&state, move |c| c.set_model(&model)).await
}

#[tauri::command]
async fn set_lock_minutes(state: tauri::State<'_, AppState>, minutes: u32) -> Res<()> {
    with_core(&state, move |c| c.set_lock_minutes(minutes)).await
}

#[tauri::command]
async fn set_practitioner(state: tauri::State<'_, AppState>, names: Vec<String>) -> Res<()> {
    with_core(&state, move |c| c.set_practitioner(names)).await
}

#[tauri::command]
async fn set_review_only_suspect(state: tauri::State<'_, AppState>, on: bool) -> Res<()> {
    with_core(&state, move |c| c.set_review_only_suspect(on)).await
}

#[tauri::command]
async fn report_settings(state: tauri::State<'_, AppState>) -> Res<ReportSettings> {
    with_core(&state, |c| c.report_settings()).await
}

#[tauri::command]
async fn set_report_settings(
    state: tauri::State<'_, AppState>,
    settings: ReportSettings,
) -> Res<()> {
    with_core(&state, move |c| c.set_report_settings(&settings)).await
}

/// Her Word template: the file's bytes are the raw body, like an imported document.
#[tauri::command]
async fn set_report_template(
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
async fn clear_report_template(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.clear_report_template()).await
}

#[tauri::command]
async fn report_template(state: tauri::State<'_, AppState>) -> Res<Option<TemplateView>> {
    with_core(&state, |c| c.report_template()).await
}

// ------------------------------------------------------------------ cases

#[tauri::command]
async fn list_cases(state: tauri::State<'_, AppState>) -> Res<Vec<CaseSummary>> {
    with_core(&state, |c| c.list_cases()).await
}

#[tauri::command]
async fn create_case(
    state: tauri::State<'_, AppState>,
    meta: CaseMeta,
    identities: Vec<IdentityInput>,
) -> Res<String> {
    with_core(&state, move |c| c.create_case(meta, identities)).await
}

#[tauri::command]
async fn update_case(
    state: tauri::State<'_, AppState>,
    case_id: String,
    meta: CaseMeta,
) -> Res<()> {
    with_core(&state, move |c| c.update_case(&case_id, meta)).await
}

#[tauri::command]
async fn delete_case(state: tauri::State<'_, AppState>, case_id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_case(&case_id)).await
}

#[tauri::command]
async fn set_identities(
    state: tauri::State<'_, AppState>,
    case_id: String,
    identities: Vec<IdentityInput>,
) -> Res<Vec<Identity>> {
    with_core(&state, move |c| c.set_identities(&case_id, identities)).await
}

#[tauri::command]
async fn case_detail(state: tauri::State<'_, AppState>, case_id: String) -> Res<CaseDetail> {
    with_core(&state, move |c| c.case_detail(&case_id)).await
}

#[tauri::command]
async fn add_input(
    state: tauri::State<'_, AppState>,
    case_id: String,
    kind: InputKind,
    title: String,
    content: String,
) -> Res<CaseInput> {
    with_core(&state, move |c| {
        c.add_input(&case_id, kind, &title, &content)
    })
    .await
}

#[tauri::command]
async fn update_input(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
    title: String,
    content: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.update_input(&case_id, &input_id, &title, &content)
    })
    .await
}

#[tauri::command]
fn score_instruments() -> Vec<dv_domain::Instrument> {
    dv_core::Core::score_instruments()
}

#[tauri::command]
fn preview_scores(sheet: dv_domain::ScoreSheet) -> Res<String> {
    dv_core::Core::preview_scores(&sheet).map_err(|e| e.to_ui())
}

#[tauri::command]
async fn save_scores(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: Option<String>,
    sheet: dv_domain::ScoreSheet,
) -> Res<dv_domain::CaseInput> {
    with_core(&state, move |c| {
        c.save_scores(&case_id, input_id.as_deref(), &sheet)
    })
    .await
}

/// "למה כתבת את זה?": the passages one paragraph leans on (local only).
#[tauri::command]
async fn paragraph_sources(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<Vec<dv_core::SourceExcerpt>> {
    with_core(&state, move |c| c.paragraph_sources(&case_id, &draft_id)).await
}

#[tauri::command]
async fn score_sheet(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
) -> Res<Option<dv_domain::ScoreSheet>> {
    with_core(&state, move |c| c.score_sheet(&case_id, &input_id)).await
}

#[tauri::command]
async fn delete_input(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
) -> Res<()> {
    with_core(&state, move |c| c.delete_input(&case_id, &input_id)).await
}

/// Percent-decoding for the (ASCII-only) IPC headers that carry a Hebrew file name.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The file's bytes arrive as the raw request body (only what the psychologist picked or
/// dropped); the case id and file name come as headers.
#[tauri::command]
async fn import_document(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Res<ImportPreview> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(internal("body"));
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(percent_decode)
    };
    let case_id = header("x-case-id").ok_or_else(|| internal("case"))?;
    let file_name = header("x-file-name").unwrap_or_default();
    let bytes = bytes.clone();
    with_core(&state, move |c| {
        c.import_document(&case_id, &file_name, &bytes)
    })
    .await
}

#[tauri::command]
async fn preview_filter(
    state: tauri::State<'_, AppState>,
    case_id: String,
    text: String,
) -> Res<dv_privacy::FilterOutcome> {
    with_core(&state, move |c| c.preview_filter(&case_id, &text)).await
}

#[tauri::command]
async fn decide_suspect(
    state: tauri::State<'_, AppState>,
    case_id: String,
    token: String,
    decision: SuspectDecision,
) -> Res<()> {
    with_core(&state, move |c| {
        c.decide_suspect(&case_id, &token, decision)
    })
    .await
}

/// "להחזיר" on the summary card: keep what the filter hid as written, for this case.
#[tauri::command]
async fn restore_auto_hidden(
    state: tauri::State<'_, AppState>,
    case_id: String,
    token: String,
    tag: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.restore_auto_hidden(&case_id, &token, &tag)
    })
    .await
}

/// The card's role menu for a name the filter kept.
#[tauri::command]
async fn change_role(
    state: tauri::State<'_, AppState>,
    case_id: String,
    tag: String,
    role: Role,
) -> Res<Vec<Identity>> {
    with_core(&state, move |c| c.change_role(&case_id, &tag, role)).await
}

// ------------------------------------------------------------------ drafting

#[tauri::command]
async fn prepare_section(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    instruction: String,
    replaces: Option<String>,
) -> Res<Prepared> {
    with_core(&state, move |c| {
        c.prepare_section_replacing(&case_id, &section_key, &instruction, replaces.as_deref())
    })
    .await
}

#[tauri::command]
async fn prepare_full_draft(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<Vec<(String, Prepared)>> {
    with_core(&state, move |c| c.prepare_full_draft(&case_id)).await
}

/// Take the approved payload, send it without holding the core, then store the answer.
#[tauri::command]
async fn send_section(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<SectionResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_section(out, response)).await
}

/// Copy the Word file's password: kept out of clipboard history and cloud sync, and taken
/// off the clipboard after 60 seconds or when the vault locks (STANDARDS 5.9).
#[tauri::command]
async fn copy_secret(state: tauri::State<'_, AppState>, text: String) -> Res<u32> {
    let text = zeroize::Zeroizing::new(text);
    state.clipboard.copy(&text).map_err(|_| UiError {
        code: "clipboard".to_owned(),
        message: "לא הצלחתי להעתיק. אפשר להקליד את הסיסמה.".to_owned(),
        details: Vec::new(),
    })?;
    Ok(u32::try_from(secret_clipboard::CLEAR_AFTER.as_secs()).unwrap_or(60))
}

// ------------------------------------------------------------------ library (D-023)

#[tauri::command]
async fn list_trash(state: tauri::State<'_, AppState>) -> Res<Vec<CaseSummary>> {
    with_core(&state, |c| c.list_trash()).await
}

#[tauri::command]
async fn restore_case(state: tauri::State<'_, AppState>, case_id: String) -> Res<()> {
    with_core(&state, move |c| c.restore_case(&case_id)).await
}

/// Erase a case from the recycle bin now; asks for the password again.
#[tauri::command]
async fn purge_case(
    state: tauri::State<'_, AppState>,
    case_id: String,
    password: String,
) -> Res<()> {
    with_core(&state, move |c| c.purge_case(&case_id, &password)).await
}

#[tauri::command]
async fn folders(state: tauri::State<'_, AppState>) -> Res<Vec<Folder>> {
    with_core(&state, |c| c.folders()).await
}

#[tauri::command]
async fn create_folder(
    state: tauri::State<'_, AppState>,
    parent_id: Option<String>,
    name: String,
) -> Res<Folder> {
    with_core(&state, move |c| {
        c.create_folder(parent_id.as_deref(), &name)
    })
    .await
}

#[tauri::command]
async fn rename_folder(state: tauri::State<'_, AppState>, id: String, name: String) -> Res<()> {
    with_core(&state, move |c| c.rename_folder(&id, &name)).await
}

#[tauri::command]
async fn move_folder(
    state: tauri::State<'_, AppState>,
    id: String,
    parent_id: Option<String>,
) -> Res<()> {
    with_core(&state, move |c| c.move_folder(&id, parent_id.as_deref())).await
}

#[tauri::command]
async fn delete_folder(state: tauri::State<'_, AppState>, id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_folder(&id)).await
}

#[tauri::command]
async fn move_case(
    state: tauri::State<'_, AppState>,
    case_id: String,
    folder_id: Option<String>,
) -> Res<()> {
    with_core(&state, move |c| c.move_case(&case_id, folder_id.as_deref())).await
}

#[tauri::command]
async fn find_name_matches(
    state: tauri::State<'_, AppState>,
    case_id: Option<String>,
    names: Vec<String>,
) -> Res<Vec<NameMatch>> {
    with_core(&state, move |c| {
        c.find_name_matches(case_id.as_deref(), &names)
    })
    .await
}

/// Sorting materials into sections (D-022): everything not sorted yet, one review screen.
#[tauri::command]
async fn prepare_sort(state: tauri::State<'_, AppState>, case_id: String) -> Res<Prepared> {
    with_core(&state, move |c| c.prepare_sort(&case_id)).await
}

#[tauri::command]
async fn send_sort(state: tauri::State<'_, AppState>, approval_id: String) -> Res<SortResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_sort(out, response)).await
}

/// Einat's choice of sections for one material; it always wins.
#[tauri::command]
async fn set_input_sections(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
    sections: Vec<String>,
) -> Res<()> {
    with_core(&state, move |c| {
        c.set_input_sections(&case_id, &input_id, &sections)
    })
    .await
}

#[tauri::command]
async fn chat(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
) -> Res<Vec<ChatView>> {
    with_core(&state, move |c| c.chat(&case_id, &section_key)).await
}

#[tauri::command]
async fn create_follow_up(state: tauri::State<'_, AppState>, case_id: String) -> Res<String> {
    with_core(&state, move |c| c.create_follow_up(&case_id)).await
}

#[tauri::command]
async fn follow_up(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<Option<dv_core::FollowUpView>> {
    with_core(&state, move |c| c.follow_up(&case_id)).await
}

#[tauri::command]
async fn add_comparison_material(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<dv_domain::CaseInput> {
    with_core(&state, move |c| c.add_comparison_material(&case_id)).await
}

#[tauri::command]
async fn approve_section(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
) -> Res<u32> {
    with_core(&state, move |c| c.approve_section(&case_id, &section_key)).await
}

#[tauri::command]
async fn approve_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<()> {
    with_core(&state, move |c| c.approve_paragraph(&case_id, &draft_id)).await
}

#[tauri::command]
async fn reject_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<()> {
    with_core(&state, move |c| c.reject_paragraph(&case_id, &draft_id)).await
}

#[tauri::command]
async fn edit_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
    text: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.edit_paragraph(&case_id, &draft_id, &text)
    })
    .await
}

#[tauri::command]
async fn add_own_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    text: String,
    first: Option<bool>,
    after: Option<String>,
) -> Res<()> {
    with_core(&state, move |c| {
        // Where it goes (D-036): after a paragraph, first in the section, or at the end.
        let at = match (after.as_deref(), first.unwrap_or(false)) {
            (Some(id), _) => Some(Some(id)),
            (None, true) => Some(None),
            (None, false) => None,
        };
        c.add_own_paragraph_at(&case_id, &section_key, &text, at)
    })
    .await
}

// ------------------------------------------------------------------ consultation

#[tauri::command]
async fn prepare_consult(
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
async fn consultations(state: tauri::State<'_, AppState>) -> Res<Vec<ConsultationSummary>> {
    with_core(&state, |c| c.consultations()).await
}

#[tauri::command]
async fn consultation(state: tauri::State<'_, AppState>, id: String) -> Res<ConsultationView> {
    with_core(&state, move |c| c.consultation(&id)).await
}

#[tauri::command]
async fn delete_consultation(state: tauri::State<'_, AppState>, id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_consultation(&id)).await
}

#[tauri::command]
async fn send_consult(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<ConsultResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_consult(out, response)).await
}

// ------------------------------------------------------------------ writing style (D-043)

#[tauri::command]
async fn style_overview(state: tauri::State<'_, AppState>) -> Res<StyleOverview> {
    with_core(&state, |c| c.style_overview()).await
}

/// A past report: the bytes arrive as the raw request body, the file name as a header.
#[tauri::command]
async fn import_style_source(
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
async fn save_style_source(
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
async fn discard_style_upload(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| {
        c.discard_style_upload();
        Ok(())
    })
    .await
}

#[tauri::command]
async fn delete_style_source(state: tauri::State<'_, AppState>, id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_style_source(&id)).await
}

#[tauri::command]
async fn prepare_style_analysis(
    state: tauri::State<'_, AppState>,
    source_id: String,
) -> Res<Prepared> {
    with_core(&state, move |c| c.prepare_style_analysis(&source_id)).await
}

#[tauri::command]
async fn send_style_analysis(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<StyleAnalysisResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_style_analysis(out, response)).await
}

#[tauri::command]
async fn prepare_style_profile(state: tauri::State<'_, AppState>) -> Res<Prepared> {
    with_core(&state, |c| c.prepare_style_profile()).await
}

#[tauri::command]
async fn send_style_profile(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<StyleProfileView> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_style_profile(out, response)).await
}

#[tauri::command]
async fn save_style_draft(
    state: tauri::State<'_, AppState>,
    profile: dv_core::StyleProfile,
) -> Res<StyleProfileView> {
    with_core(&state, move |c| c.save_style_draft(profile)).await
}

#[tauri::command]
async fn approve_style_draft(state: tauri::State<'_, AppState>) -> Res<StyleProfileView> {
    with_core(&state, |c| c.approve_style_draft()).await
}

#[tauri::command]
async fn discard_style_draft(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.discard_style_draft()).await
}

#[tauri::command]
async fn restore_style_version(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Res<StyleProfileView> {
    with_core(&state, move |c| c.restore_style_version(&id)).await
}

#[tauri::command]
async fn set_style_enabled(state: tauri::State<'_, AppState>, on: bool) -> Res<()> {
    with_core(&state, move |c| c.set_style_enabled(on)).await
}

#[tauri::command]
async fn reset_style(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.reset_style()).await
}

#[tauri::command]
async fn accept_style_suggestion(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Res<StyleProfileView> {
    with_core(&state, move |c| c.accept_style_suggestion(&id)).await
}

#[tauri::command]
async fn dismiss_style_suggestion(state: tauri::State<'_, AppState>, id: String) -> Res<()> {
    with_core(&state, move |c| c.dismiss_style_suggestion(&id)).await
}

// ------------------------------------------------------------------ Word report

#[tauri::command]
async fn check_export(state: tauri::State<'_, AppState>, case_id: String) -> Res<ExportCheck> {
    with_core(&state, move |c| c.check_export(&case_id)).await
}

/// A free name in the folder: "name.docx", "name (2).docx", …
fn free_path(dir: &std::path::Path, file_name: &str) -> PathBuf {
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, "docx"));
    let mut path = dir.join(file_name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// Write the report into the Downloads folder and return where it was saved.
#[tauri::command]
async fn export_report(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    case_id: String,
    password: Option<String>,
) -> Res<String> {
    // Downloads, else Documents, else the home folder, else the app's own folder. A folder
    // that a cloud service syncs (Documents moved to OneDrive, say) is skipped: the report
    // holds the real names and would be uploaded on its own.
    let paths = app.path();
    let dir = [
        paths.download_dir(),
        paths.document_dir(),
        paths.home_dir(),
        paths.app_local_data_dir().map(|d| d.join("exports")),
    ]
    .into_iter()
    .flatten()
    .filter(|d| dv_core::cloud_synced_folder(d).is_none())
    .find(|d| std::fs::create_dir_all(d).is_ok())
    .ok_or_else(|| UiError {
        code: "no_folder".to_owned(),
        message: "לא נמצאה תיקייה לשמירת הדוח (הורדות או מסמכים).".to_owned(),
        details: Vec::new(),
    })?;
    with_core(&state, move |c| {
        let check = c.check_export(&case_id)?;
        let bytes = c.export_report(&case_id, password.as_deref().filter(|p| !p.is_empty()))?;
        let path = free_path(&dir, &check.file_name);
        std::fs::write(&path, bytes).map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(path.display().to_string())
    })
    .await
}

// ------------------------------------------------------------------ activity and retention

#[tauri::command]
async fn activity(state: tauri::State<'_, AppState>, before: Option<i64>) -> Res<ActivityPage> {
    with_core(&state, move |c| c.activity(before)).await
}

#[tauri::command]
async fn mark_activity_reviewed(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| c.mark_activity_reviewed()).await
}

#[tauri::command]
async fn retention_due(state: tauri::State<'_, AppState>) -> Res<Vec<RetentionItem>> {
    with_core(&state, |c| c.retention_due()).await
}

#[tauri::command]
async fn keep_case_longer(
    state: tauri::State<'_, AppState>,
    case_id: String,
    years: u8,
) -> Res<()> {
    with_core(&state, move |c| c.keep_case_longer(&case_id, years)).await
}

/// Print the page with the system's own print window. `window.print()` does nothing in the
/// macOS webview, so the recovery kit could not be printed there.
#[tauri::command]
fn print_page(window: tauri::WebviewWindow) -> Res<()> {
    window.print().map_err(|e| internal(&e.to_string()))
}

// ------------------------------------------------------------------ password and kit

/// `current` is the password, or the recovery kit when `with_recovery`.
#[tauri::command]
async fn change_password(
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
async fn new_recovery_kit(
    state: tauri::State<'_, AppState>,
    current: String,
    with_recovery: bool,
) -> Res<CreatedVault> {
    let current = zeroize::Zeroizing::new(current);
    with_core(&state, move |c| c.new_recovery_kit(&current, with_recovery)).await
}

// ------------------------------------------------------------------ backup (D-024)

#[tauri::command]
async fn backup_status(state: tauri::State<'_, AppState>) -> Res<BackupStatus> {
    with_core(&state, |c| c.backup_status()).await
}

/// Ask where to save (the system's "Save as"), then write the encrypted backup there.
/// `None` when Einat cancelled.
#[tauri::command]
async fn write_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Res<Option<BackupDone>> {
    let start = with_core(&state, |c| c.backup_dir()).await?;
    let start = start.or_else(|| app.path().document_dir().ok());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let Some(dest) = file_dialog::save(&app, start, dv_core::backup_file_name(now)).await else {
        return Ok(None);
    };
    with_core(&state, move |c| c.write_backup(&dest).map(Some)).await
}

/// Ask for a backup file (the system's "Open") and say what it is. `None` when cancelled.
#[tauri::command]
async fn choose_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Res<Option<StagedBackup>> {
    // Locked or no vault yet: start in Documents.
    let start = with_core(&state, |c| Ok(c.backup_dir().ok().flatten()))
        .await?
        .or_else(|| app.path().document_dir().ok());
    let Some(path) = file_dialog::open(&app, start).await else {
        return Ok(None);
    };
    with_core(&state, move |c| {
        let size = std::fs::metadata(&path)
            .map_err(|e| CoreError::Refused(format!("אי אפשר לקרוא את הקובץ: {e}")))?
            .len();
        if size > dv_core::MAX_BACKUP_BYTES {
            return Err(CoreError::Refused(
                "הקובץ גדול מדי בשביל גיבוי של הכספת.".to_owned(),
            ));
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| CoreError::Refused(format!("אי אפשר לקרוא את הקובץ: {e}")))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        c.stage_backup(bytes, name).map(Some)
    })
    .await
}

/// Restore drill on the chosen file, with the password.
/// The automatic backup to the drive of the last backup, on or off.
#[tauri::command]
async fn set_auto_backup(state: tauri::State<'_, AppState>, on: bool) -> Res<BackupStatus> {
    with_core(&state, move |c| c.set_auto_backup(on)).await
}

#[tauri::command]
async fn check_backup(state: tauri::State<'_, AppState>, password: String) -> Res<BackupCheckView> {
    let password = zeroize::Zeroizing::new(password);
    with_core(&state, move |c| c.check_staged_backup(&password)).await
}

/// On a computer with no vault: restore the chosen file and open it.
#[tauri::command]
async fn restore_backup(
    state: tauri::State<'_, AppState>,
    password: Option<String>,
    recovery_key: Option<String>,
) -> Res<AppStatus> {
    let password = password.map(zeroize::Zeroizing::new);
    let recovery_key = recovery_key.map(zeroize::Zeroizing::new);
    with_core(&state, move |c| {
        c.restore_staged_backup(
            password.as_deref().map(String::as_str),
            recovery_key.as_deref().map(String::as_str),
        )
    })
    .await
}

#[tauri::command]
async fn forget_backup(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| {
        c.forget_staged_backup();
        Ok(())
    })
    .await
}

// ------------------------------------------------------------------ updates (D-033)

/// Is there a newer version? Nothing from the vault is involved; the core's lock is not held.
#[tauri::command]
async fn check_update() -> Res<Option<dv_core::update::UpdateView>> {
    tauri::async_runtime::spawn_blocking(|| dv_core::update::check().map_err(|e| e.to_ui()))
        .await
        .map_err(|_| internal("task"))?
}

/// Fetch and verify the newest version in the background, so the click only restarts
/// (D-038). Nothing is installed here. `true` when a verified installer is waiting.
#[tauri::command]
async fn prepare_update(state: tauri::State<'_, AppState>) -> Res<bool> {
    if !cfg!(windows) {
        return Ok(false);
    }
    let dir = state.dir.clone();
    let prepared = Arc::clone(&state.prepared);
    tauri::async_runtime::spawn_blocking(move || {
        let mut slot = prepared.lock().map_err(|_| internal("update"))?;
        let ready = dv_core::update::prepare(&dir, slot.take()).map_err(|e| e.to_ui())?;
        *slot = ready;
        Ok(slot.is_some())
    })
    .await
    .map_err(|_| internal("task"))?
}

/// Lock the vault and run the verified installer, then close. The installer (passive, update
/// mode) replaces the program and opens it again; the vault folder is not touched. A version
/// fetched in the background is used when it is still intact; otherwise it is fetched now.
#[tauri::command]
async fn install_update(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Res<()> {
    if !cfg!(windows) {
        return Err(UiError {
            code: "update".to_owned(),
            message: "כאן העדכון לא מותקן לבד. מתקינים את הגרסה החדשה מהקישור ששלח אור.".to_owned(),
            details: Vec::new(),
        });
    }
    let dir = state.dir.clone();
    let prepared = Arc::clone(&state.prepared);
    let file = tauri::async_runtime::spawn_blocking(move || {
        let waiting = prepared.lock().ok().and_then(|mut p| p.take());
        match waiting {
            Some(file) if dv_core::update::still_intact(&file) => Ok(file),
            _ => dv_core::update::download(&dir),
        }
    })
    .await
    .map_err(|_| internal("task"))?
    .map_err(|e| e.to_ui())?;
    if !dv_core::update::still_intact(&file) {
        return Err(internal("the installer changed after it was checked"));
    }
    state.clipboard.clear_now();
    with_core(&state, |c| {
        c.lock();
        Ok(())
    })
    .await?;
    std::process::Command::new(&file.path)
        .args(["/P", "/UPDATE", "/R"])
        .spawn()
        .map_err(|_| internal("the installer did not start"))?;
    app.exit(0);
    Ok(())
}

/// Next to the vault folder, never inside it. Kept small.
const CRASH_LOG: &str = "crash-log.txt";
const CRASH_LOG_MAX: u64 = 64 * 1024;

/// A release build stops at the first panic (`panic = "abort"`), and on Windows it has no
/// console, so the program just vanished. Write where it stopped: the time, version, thread
/// and source line, and the message only when it is fixed text in the code. A message built
/// at run time could carry something from a case, so it is left out.
fn remember_crashes(path: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let fixed = info.payload().downcast_ref::<&'static str>().copied();
        let line = format!(
            "{} · v{} · thread {} · {} · {}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            env!("CARGO_PKG_VERSION"),
            std::thread::current().name().unwrap_or("?"),
            info.location()
                .map_or_else(|| "?".to_owned(), |l| format!("{}:{}", l.file(), l.line())),
            fixed.unwrap_or("(message left out)"),
        );
        let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > CRASH_LOG_MAX);
        if too_big {
            let _ = std::fs::remove_file(&path);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = f.write_all(line.as_bytes());
        }
        previous(info);
    }));
}

fn main() {
    // A document worker: the same binary, started by the core for one file.
    if std::env::args().nth(1).as_deref() == Some(dv_ingest::worker::WORKER_ARG) {
        std::process::exit(dv_ingest::worker::worker_main());
    }

    let result = tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            remember_crashes(data.join(CRASH_LOG));
            let dir = data.join("vault");
            std::fs::create_dir_all(&dir)?;
            // The installer of the last update has done its work.
            dv_core::update::clean(&dir);
            let mut core = Core::new(&dir);
            if let Ok(exe) = std::env::current_exe() {
                core = core.with_ingest_worker(exe);
            }
            let core = Arc::new(Mutex::new(core));
            let clipboard = secret_clipboard::SecretClipboard::default();
            // Lock after the idle time, or after the computer slept, even when nothing is
            // clicked; a secret left on the clipboard goes with it.
            let timer = Arc::clone(&core);
            let timer_clipboard = clipboard.clone();
            let timer_window = app.get_webview_window("main");
            // The window opens protected (tauri.conf.json); this lifts it while D-039 holds.
            if let Some(w) = &timer_window {
                protect(w, true);
            }
            let mut computer = os_lock::LockWatch::default();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(15));
                // Read the clock before waiting for the core: a long command holding it must not
                // look like the computer slept.
                let now = std::time::SystemTime::now();
                let mut locked = timer.lock().is_ok_and(|mut c| c.tick(now));
                // The computer was locked (Win+L): the vault locks with it. Asked without
                // holding the core, and only while the vault is open.
                if !locked && timer.lock().is_ok_and(|c| c.is_unlocked()) {
                    let answer = os_lock::computer_locked();
                    if computer.just_locked(answer) {
                        locked = timer.lock().is_ok_and(|mut c| c.lock_with_computer());
                    }
                }
                // A backup that is due goes by itself to the drive of the last one, if it is
                // connected (see `Core::auto_backup`). A failure waits and is shown nowhere: the
                // backup reminder stays until a backup is made.
                if !locked {
                    if let Ok(mut c) = timer.lock() {
                        let _ = c.auto_backup();
                    }
                }
                if locked {
                    timer_clipboard.clear_now();
                    if let Some(w) = &timer_window {
                        protect(w, true);
                    }
                }
            });
            app.manage(AppState {
                core,
                clipboard,
                dir,
                progress: Arc::default(),
                prepared: Arc::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            app_status,
            create_vault,
            confirm_recovery_key,
            unlock,
            unlock_with_recovery,
            lock,
            touch,
            readiness,
            confirm_readiness,
            usage_summary,
            set_monthly_cap,
            hold_unsaved,
            take_unsaved,
            set_api_key,
            set_model,
            set_speed,
            set_lock_minutes,
            set_practitioner,
            set_review_only_suspect,
            set_screen_protection,
            report_settings,
            set_report_settings,
            set_report_template,
            clear_report_template,
            report_template,
            list_cases,
            create_case,
            update_case,
            delete_case,
            set_identities,
            case_detail,
            add_input,
            update_input,
            delete_input,
            score_instruments,
            preview_scores,
            save_scores,
            score_sheet,
            paragraph_sources,
            import_document,
            preview_filter,
            decide_suspect,
            restore_auto_hidden,
            change_role,
            prepare_section,
            prepare_full_draft,
            send_section,
            prepare_sort,
            send_sort,
            set_input_sections,
            list_trash,
            restore_case,
            purge_case,
            folders,
            create_folder,
            rename_folder,
            move_folder,
            delete_folder,
            move_case,
            find_name_matches,
            copy_secret,
            chat,
            approve_paragraph,
            approve_section,
            create_follow_up,
            follow_up,
            add_comparison_material,
            reject_paragraph,
            edit_paragraph,
            add_own_paragraph,
            prepare_consult,
            send_consult,
            consultations,
            consultation,
            delete_consultation,
            style_overview,
            import_style_source,
            save_style_source,
            discard_style_upload,
            delete_style_source,
            prepare_style_analysis,
            send_style_analysis,
            prepare_style_profile,
            send_style_profile,
            save_style_draft,
            approve_style_draft,
            discard_style_draft,
            restore_style_version,
            set_style_enabled,
            reset_style,
            accept_style_suggestion,
            dismiss_style_suggestion,
            check_export,
            export_report,
            print_page,
            activity,
            mark_activity_reviewed,
            retention_due,
            keep_case_longer,
            change_password,
            new_recovery_kit,
            backup_status,
            write_backup,
            choose_backup,
            check_backup,
            restore_backup,
            forget_backup,
            set_auto_backup,
            check_update,
            prepare_update,
            install_update,
            send_progress,
        ])
        .run(tauri::generate_context!());
    if result.is_err() {
        std::process::exit(1);
    }
}
