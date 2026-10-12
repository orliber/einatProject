//! Desktop shell. Every command delegates to `dv-core`; the shell adds only what needs the
//! OS: the app's data folder, the Downloads folder for the report, the worker binary, and a
//! timer that locks the vault when idle.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod file_dialog;
mod os_lock;
mod secret_clipboard;
mod win_platform;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use commands::*;
use dv_core::{
    ActivityPage, AppStatus, BackupCheckView, BackupDone, BackupStatus, CaseDetail, ChatView,
    ConsultResult, ConsultationSummary, ConsultationView, Core, CoreError, CreatedVault,
    ExportCheck, ImportPreview, NameMatch, ParagraphVersionView, ParagraphView, Prepared,
    Readiness, ReportSettings, RetentionItem, SectionResult, SecurityStatus, SortResult,
    StagedBackup, StyleAnalysisResult, StyleImportPreview, StyleOverview, StyleProfileView,
    StyleSourceView, SuspectDecision, TemplateView, UiError, UnsavedEdit, UsageSummary,
    VerifyResult,
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
    /// Where the optional name model is downloaded (next to the vault, not inside it; D-053).
    models: PathBuf,
    /// Bytes done and total while the name model downloads; held for the whole download.
    model_progress: Arc<Mutex<Option<(u64, u64)>>>,
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

/// How many words the AI has written so far for a request on its way.
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
        // Errors name the AI she chose ("שגיאה בחיבור ל-Gemini").
        f(&mut c).map_err(|e| e.to_ui_for(c.ai_name()))
    })
    .await
    .map_err(|_| internal("task"))?
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
    // Before anything holds case data: room to lock the vault's memory in RAM, and no crash
    // dump with the heap in it (D-047). The document worker below gets the same.
    let _ = win_platform::harden_process();
    // A document worker: the same binary, started by the core for one file.
    if std::env::args().nth(1).as_deref() == Some(dv_ingest::worker::WORKER_ARG) {
        std::process::exit(dv_ingest::worker::worker_main());
    }

    let result = tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            remember_crashes(data.join(CRASH_LOG));
            // The window's own crash reports (WebView2 keeps them under its data folder) could
            // hold what was on screen. They are never sent; they are deleted at every start.
            let _ =
                std::fs::remove_dir_all(data.join("EBWebView").join("Crashpad").join("reports"));
            let dir = data.join("vault");
            std::fs::create_dir_all(&dir)?;
            // The installer of the last update has done its work.
            dv_core::update::clean(&dir);
            let models = data.join("models");
            let mut core = Core::new(&dir).with_models_dir(models.clone());
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
                models,
                model_progress: Arc::default(),
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
            security_status,
            set_send_stopped,
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
            restore_input,
            score_instruments,
            preview_scores,
            save_scores,
            score_sheet,
            paragraph_sources,
            import_document,
            preview_filter,
            decide_suspect,
            settle_foreign_tags,
            restore_auto_hidden,
            change_role,
            prepare_section,
            prepare_full_draft,
            send_section,
            verify_stage_on,
            set_verify_stage,
            name_model_status,
            download_name_model,
            name_model_progress,
            set_name_model,
            prepare_verify,
            send_verify,
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
            paragraph_versions,
            restore_paragraph_version,
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
            forget_style_learning,
            accept_style_suggestion,
            dismiss_style_suggestion,
            check_export,
            export_report,
            export_pdf,
            check_original,
            prepare_letter,
            letter,
            export_letter,
            print_page,
            activity,
            mark_activity_reviewed,
            retention_due,
            keep_case_longer,
            change_password,
            new_recovery_kit,
            google_status,
            google_cancel,
            google_turn_on,
            google_turn_off,
            google_recover,
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
