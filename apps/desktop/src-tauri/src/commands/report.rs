//! Word report and export.

use crate::*;

#[tauri::command]
pub(crate) async fn check_export(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<ExportCheck> {
    with_core(&state, move |c| c.check_export(&case_id)).await
}

/// A free name in the folder: "name.docx", "name (2).docx", …
pub(crate) fn free_path(dir: &std::path::Path, file_name: &str) -> PathBuf {
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, "docx"));
    let mut path = dir.join(file_name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// Downloads, else Documents, else the home folder, else the app's own folder. A folder that
/// a cloud service syncs (Documents moved to OneDrive, say) is skipped: the report holds the
/// real names and would be uploaded on its own.
pub(crate) fn export_dir(app: &tauri::AppHandle) -> Result<PathBuf, UiError> {
    let paths = app.path();
    [
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
        action: dv_core::ErrorAction::Report,
    })
}

/// Write the report into the Downloads folder and return where it was saved. With `parents`,
/// the parents' version (round 55, D-095).
#[tauri::command]
pub(crate) async fn export_report(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    case_id: String,
    password: Option<String>,
    parents: Option<ParentsOptions>,
) -> Res<String> {
    let dir = export_dir(&app)?;
    let password = password
        .filter(|p| !p.is_empty())
        .map(zeroize::Zeroizing::new);
    // Rendering, encrypting and writing the file run with the core free (F2).
    let (bytes, check) = off_lock(
        &state,
        move |c| {
            c.begin_export_report_as(&case_id, password.as_deref().map(String::as_str), parents)
        },
        |job| job.run(),
        |c, exported| c.finish_export(exported),
    )
    .await?;
    save_export(dir, check.file_name, bytes).await
}

/// Write an exported file into `dir` under a free name, off the core.
async fn save_export(dir: PathBuf, file_name: String, bytes: Vec<u8>) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = free_path(&dir, &file_name);
        std::fs::write(&path, bytes).map_err(|e| CoreError::Internal(e.to_string()).to_ui())?;
        Ok(path.display().to_string())
    })
    .await
    .map_err(|_| internal("task"))?
}

/// EX-3: the report as a locked PDF, into the same folder as the Word file.
#[tauri::command]
pub(crate) async fn export_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    case_id: String,
    password: Option<String>,
    parents: Option<ParentsOptions>,
) -> Res<String> {
    let dir = export_dir(&app)?;
    let password = password
        .filter(|p| !p.is_empty())
        .map(zeroize::Zeroizing::new);
    let (bytes, check) = off_lock(
        &state,
        move |c| c.begin_export_pdf_as(&case_id, password.as_deref().map(String::as_str), parents),
        |job| job.run(),
        |c, exported| c.finish_export(exported),
    )
    .await?;
    let stem = check
        .file_name
        .rsplit_once('.')
        .map_or(check.file_name.as_str(), |(s, _)| s);
    save_export(dir, format!("{stem}.pdf"), bytes).await
}

/// L6: the protection layers on paper, for the parents and the institute (nothing about a
/// child), into the same folder.
#[tauri::command]
pub(crate) async fn export_security_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Res<String> {
    let dir = export_dir(&app)?;
    let bytes = with_core(&state, |c| c.security_layers_pdf()).await?;
    save_export(dir, "שכבות ההגנה.pdf".to_owned(), bytes).await
}

/// Whether a PDF is one this vault exported, unchanged: when it was made, or `None`.
#[tauri::command]
pub(crate) async fn check_original(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Res<Option<i64>> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(internal("body"));
    };
    let bytes = bytes.clone();
    with_core(&state, move |c| c.check_original(&bytes)).await
}

/// EX-4: a short letter to the parents or the school, from the approved report.
#[tauri::command]
pub(crate) async fn prepare_letter(
    state: tauri::State<'_, AppState>,
    case_id: String,
    audience: String,
    note: String,
) -> Res<Prepared> {
    with_core(&state, move |c| {
        c.prepare_letter(&case_id, &audience, &note)
    })
    .await
}

#[tauri::command]
pub(crate) async fn letter(
    state: tauri::State<'_, AppState>,
    case_id: String,
    audience: String,
) -> Res<Vec<ParagraphView>> {
    with_core(&state, move |c| c.letter(&case_id, &audience)).await
}

/// The letter into the same folder as the report, under a name without the child's name.
#[tauri::command]
pub(crate) async fn export_letter(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    case_id: String,
    audience: String,
    password: Option<String>,
) -> Res<String> {
    let dir = export_dir(&app)?;
    with_core(&state, move |c| {
        let check = c.check_export(&case_id)?;
        let bytes = c.export_letter(
            &case_id,
            &audience,
            password.as_deref().filter(|p| !p.is_empty()),
        )?;
        let who = if audience == "school" {
            "צוות"
        } else {
            "הורים"
        };
        let path = free_path(&dir, &format!("מכתב-{who}-{}", check.file_name));
        std::fs::write(&path, bytes).map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(path.display().to_string())
    })
    .await
}
