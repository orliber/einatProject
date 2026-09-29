//! Desktop shell. Every command delegates to `dv-core`; the shell adds only what needs the
//! OS: the app's data folder, the Downloads folder for the report, the worker binary, and a
//! timer that locks the vault when idle.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dv_core::{
    AppStatus, CaseDetail, ChatView, ConsultResult, Core, CoreError, CreatedVault, ExportCheck,
    ImportPreview, NameMatch, Prepared, ReportSettings, SectionResult, SortResult, SuspectDecision,
    UiError,
};
use dv_domain::{CaseInput, CaseMeta, CaseSummary, Folder, Identity, IdentityInput, InputKind};
use tauri::Manager;

struct AppState {
    core: Arc<Mutex<Core>>,
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
async fn app_status(state: tauri::State<'_, AppState>) -> Res<AppStatus> {
    with_core(&state, |c| Ok(c.status())).await
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
async fn lock(state: tauri::State<'_, AppState>) -> Res<()> {
    with_core(&state, |c| {
        c.lock();
        Ok(())
    })
    .await
}

// ------------------------------------------------------------------ settings

#[tauri::command]
async fn set_api_key(state: tauri::State<'_, AppState>, key: String) -> Res<()> {
    with_core(&state, move |c| c.set_api_key(&key)).await
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

// ------------------------------------------------------------------ drafting

#[tauri::command]
async fn prepare_section(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    instruction: String,
) -> Res<Prepared> {
    with_core(&state, move |c| {
        c.prepare_section(&case_id, &section_key, &instruction)
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
    let out = with_core(&state, move |c| c.begin_send(&approval_id)).await?;
    let (out, response) = tauri::async_runtime::spawn_blocking(move || {
        let r = out.transmit();
        (out, r)
    })
    .await
    .map_err(|_| internal("send"))?;
    with_core(&state, move |c| c.finish_section(out, response)).await
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
    let out = with_core(&state, move |c| c.begin_send(&approval_id)).await?;
    let (out, response) = tauri::async_runtime::spawn_blocking(move || {
        let r = out.transmit();
        (out, r)
    })
    .await
    .map_err(|_| internal("send"))?;
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
) -> Res<()> {
    with_core(&state, move |c| {
        c.add_own_paragraph(&case_id, &section_key, &text)
    })
    .await
}

// ------------------------------------------------------------------ consultation

#[tauri::command]
async fn prepare_consult(
    state: tauri::State<'_, AppState>,
    case_id: Option<String>,
    message: String,
) -> Res<Prepared> {
    with_core(&state, move |c| {
        c.prepare_consult(case_id.as_deref(), &message)
    })
    .await
}

#[tauri::command]
async fn send_consult(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<ConsultResult> {
    let out = with_core(&state, move |c| c.begin_send(&approval_id)).await?;
    let (out, response) = tauri::async_runtime::spawn_blocking(move || {
        let r = out.transmit();
        (out, r)
    })
    .await
    .map_err(|_| internal("send"))?;
    with_core(&state, move |c| c.finish_consult(out, response)).await
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
    // Downloads, else Documents, else the home folder, else the app's own folder.
    let paths = app.path();
    let dir = [
        paths.download_dir(),
        paths.document_dir(),
        paths.home_dir(),
        paths.app_local_data_dir().map(|d| d.join("exports")),
    ]
    .into_iter()
    .flatten()
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

fn main() {
    // A document worker: the same binary, started by the core for one file.
    if std::env::args().nth(1).as_deref() == Some(dv_ingest::worker::WORKER_ARG) {
        std::process::exit(dv_ingest::worker::worker_main());
    }

    let result = tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?.join("vault");
            std::fs::create_dir_all(&dir)?;
            let mut core = Core::new(&dir);
            if let Ok(exe) = std::env::current_exe() {
                core = core.with_ingest_worker(exe);
            }
            let core = Arc::new(Mutex::new(core));
            // Lock after the idle time even when nothing is clicked.
            let timer = Arc::clone(&core);
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(15));
                if let Ok(mut c) = timer.lock() {
                    c.lock_if_idle();
                }
            });
            app.manage(AppState { core });
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
            set_api_key,
            set_model,
            set_lock_minutes,
            set_practitioner,
            set_review_only_suspect,
            report_settings,
            set_report_settings,
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
            import_document,
            preview_filter,
            decide_suspect,
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
            chat,
            approve_paragraph,
            reject_paragraph,
            edit_paragraph,
            add_own_paragraph,
            prepare_consult,
            send_consult,
            check_export,
            export_report,
        ])
        .run(tauri::generate_context!());
    if result.is_err() {
        std::process::exit(1);
    }
}
