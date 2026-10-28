//! "בדיקת מערכת" (D-142): one self-test at a time, so the window shows each as it ends.

use dv_core::{ExportPlace, SystemCheck, SystemCheckResult};

use crate::*;

/// Run one check. What it needs is taken under the core's lock in a moment; the check itself
/// (the reader, the OCR engine, a handshake with the AI service, a trial file) runs on a
/// worker thread with the core free, so typing, the idle lock and every other command go on.
/// Never fails for what it finds: each finding is a line of the answer.
#[tauri::command]
pub(crate) async fn system_check(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    check: SystemCheck,
) -> Res<SystemCheckResult> {
    let export = if check == SystemCheck::Folders {
        export_candidates(&app)
    } else {
        Vec::new()
    };
    let job = with_core(&state, move |c| Ok(c.begin_system_check(check, export))).await?;
    tauri::async_runtime::spawn_blocking(move || job.run())
        .await
        .map_err(|_| internal("task"))
}

/// The folders a report is saved to, in the order saving tries them (`export_dir`). Nothing is
/// made and no folder window opens: the check looks only at what is there.
fn export_candidates(app: &tauri::AppHandle) -> Vec<(ExportPlace, PathBuf)> {
    let paths = app.path();
    [
        (ExportPlace::Downloads, paths.download_dir()),
        (ExportPlace::Documents, paths.document_dir()),
        (ExportPlace::Home, paths.home_dir()),
    ]
    .into_iter()
    .filter_map(|(place, dir)| dir.ok().map(|d| (place, d)))
    .collect()
}
