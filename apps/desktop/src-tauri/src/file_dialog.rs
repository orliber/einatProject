//! The system's own "Save as" and "Open" windows, for the encrypted backup (D-024).
//!
//! They run on the main thread (macOS requires it, GTK expects it) and are awaited from a
//! command. The webview never gets a path or a file from here: only what the command returns.

use std::path::PathBuf;

use dv_core::BACKUP_EXTENSION;
use tauri::{AppHandle, Manager};

const FILTER: &str = "גיבוי כספת האבחון";

/// Run `f` on the main thread and wait for it without blocking the async runtime.
async fn on_main_thread<T, F>(app: &AppHandle, f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce(Option<tauri::WebviewWindow>) -> T + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let window = app.get_webview_window("main");
    app.run_on_main_thread(move || {
        let _ = tx.send(f(window));
    })
    .ok()?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().ok())
        .await
        .ok()
        .flatten()
}

/// Debug builds only: the end-to-end tests cannot click a system window, so a folder named in
/// the environment stands in for it. Release builds always show the window.
fn scripted_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        std::env::var_os("DV_E2E_DIALOG_DIR").map(PathBuf::from)
    } else {
        None
    }
}

/// "Save as", starting in `dir` with `name` filled in. `None` when cancelled.
pub async fn save(app: &AppHandle, dir: Option<PathBuf>, name: String) -> Option<PathBuf> {
    if let Some(d) = scripted_dir() {
        return Some(d.join(name));
    }
    on_main_thread(app, move |window| {
        let mut dialog = rfd::FileDialog::new()
            .set_title("שמירת גיבוי מוצפן")
            .set_file_name(name)
            .add_filter(FILTER, &[BACKUP_EXTENSION]);
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        if let Some(w) = window.as_ref() {
            dialog = dialog.set_parent(w);
        }
        dialog.save_file()
    })
    .await
    .flatten()
}

/// "Open" for a backup file. `None` when cancelled.
pub async fn open(app: &AppHandle, dir: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(d) = scripted_dir() {
        return std::fs::read_dir(d)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == BACKUP_EXTENSION))
            .max();
    }
    on_main_thread(app, move |window| {
        let mut dialog = rfd::FileDialog::new()
            .set_title("בחירת קובץ גיבוי")
            .add_filter(FILTER, &[BACKUP_EXTENSION]);
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        if let Some(w) = window.as_ref() {
            dialog = dialog.set_parent(w);
        }
        dialog.pick_file()
    })
    .await
    .flatten()
}
