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

/// Downloads, else Documents, else the home folder. A folder that a cloud service syncs
/// (Documents moved to OneDrive, say) is skipped: the report holds the real names and would be
/// uploaded on its own. When none of them can take it, she chooses the folder herself, told
/// why; never a hidden folder of the app's own, where a report would stay unseen (D-120).
pub(crate) async fn export_dir(app: &tauri::AppHandle) -> Result<PathBuf, UiError> {
    let paths = app.path();
    let usual = usual_export_dir([paths.download_dir(), paths.document_dir(), paths.home_dir()]);
    if let Some(dir) = usual {
        return Ok(dir);
    }
    let chosen = file_dialog::pick_folder(app, EXPORT_FOLDER_TITLE).await;
    chosen_export_dir(chosen).map_err(|e| e.to_ui())
}

/// Said in the folder window when no usual folder can take the file.
const EXPORT_FOLDER_TITLE: &str =
    "תיקיית ההורדות והמסמכים לא זמינות, או שהן מסונכרנות לענן. בוחרים תיקייה לשמירת הקובץ";

/// The first usual folder that is not synced to the cloud and can be made.
fn usual_export_dir<E>(
    candidates: impl IntoIterator<Item = Result<PathBuf, E>>,
) -> Option<PathBuf> {
    candidates
        .into_iter()
        .flatten()
        .filter(|d| dv_core::cloud_synced_folder(d).is_none())
        .find(|d| std::fs::create_dir_all(d).is_ok())
}

/// The folder she chose for an export: not synced to the cloud, and there.
fn chosen_export_dir(chosen: Option<PathBuf>) -> Result<PathBuf, CoreError> {
    let Some(dir) = chosen else {
        return Err(CoreError::Refused(
            "לא נבחרה תיקייה, ולכן הקובץ לא נשמר. אפשר להפיק שוב ולבחור תיקייה.".to_owned(),
        ));
    };
    if dv_core::cloud_synced_folder(&dir).is_some() {
        return Err(CoreError::Refused(
            "התיקייה שנבחרה מסונכרנת לענן, והדוח היה עולה אליו לבד. בוחרים תיקייה אחרת, למשל בדיסק נייד."
                .to_owned(),
        ));
    }
    if !dir.is_dir() {
        return Err(CoreError::Refused(
            "התיקייה שנבחרה לא נמצאה. אפשר להפיק שוב ולבחור תיקייה אחרת.".to_owned(),
        ));
    }
    Ok(dir)
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
    let dir = export_dir(&app).await?;
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
        std::fs::write(&path, bytes).map_err(|e| dv_core::file_error(e).to_ui())?;
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
    let dir = export_dir(&app).await?;
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
    let dir = export_dir(&app).await?;
    let bytes = with_core(&state, |c| c.security_layers_pdf()).await?;
    save_export(dir, "שכבות ההגנה.pdf".to_owned(), bytes).await
}

/// Whether a file is one this vault exported, unchanged: when it was made, or `None`. A file
/// over the cap is refused before it is copied; the hash is made with the core free, and only
/// the fingerprint goes to the core (D-120).
#[tauri::command]
pub(crate) async fn check_original(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Res<Option<i64>> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(internal("body"));
    };
    if bytes.len() > dv_core::MAX_ORIGINAL_BYTES {
        return Err(dv_core::original_too_large().to_ui());
    }
    let bytes = bytes.clone();
    let sha256 = tauri::async_runtime::spawn_blocking(move || dv_core::sha256_hex(&bytes))
        .await
        .map_err(|_| internal("task"))?;
    with_core(&state, move |c| c.check_original_hash(&sha256)).await
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
/// Rendered, encrypted and written with the core free, and recorded with its fingerprint,
/// like the report (D-120).
#[tauri::command]
pub(crate) async fn export_letter(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    case_id: String,
    audience: String,
    password: Option<String>,
) -> Res<String> {
    let dir = export_dir(&app).await?;
    let password = password
        .filter(|p| !p.is_empty())
        .map(zeroize::Zeroizing::new);
    let who = if audience == "school" {
        "צוות"
    } else {
        "הורים"
    };
    let (bytes, check) = off_lock(
        &state,
        move |c| {
            c.begin_export_letter(&case_id, &audience, password.as_deref().map(String::as_str))
        },
        |job| job.run(),
        |c, exported| c.finish_export(exported),
    )
    .await?;
    save_export(dir, format!("מכתב-{who}-{}", check.file_name), bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder of this test's own under the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dv-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_usual_folder_is_taken_when_one_is_there() {
        let dir = scratch("usual");
        let missing: Result<PathBuf, ()> = Err(());
        let got = usual_export_dir([missing, Ok(dir.join("הורדות"))]);
        assert_eq!(got, Some(dir.join("הורדות")));
        assert!(dir.join("הורדות").is_dir());
        // A synced one is passed over.
        let synced = dir.join("OneDrive").join("מסמכים");
        assert_eq!(usual_export_dir([Ok::<_, ()>(synced)]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_usual_folder_means_she_chooses_and_nothing_hidden_is_used() {
        let none: [Result<PathBuf, ()>; 2] = [Err(()), Err(())];
        assert_eq!(usual_export_dir(none), None);
        // Cancelled: refused with a sentence, nothing saved anywhere.
        let e = chosen_export_dir(None).unwrap_err().to_ui();
        assert_eq!(e.code, "refused");
        assert!(e.message.contains("לא נשמר"), "{}", e.message);
        // A folder synced to the cloud is refused, even when it is there.
        let dir = scratch("chosen");
        let synced = dir.join("Dropbox");
        std::fs::create_dir_all(&synced).unwrap();
        let e = chosen_export_dir(Some(synced)).unwrap_err().to_ui();
        assert!(e.message.contains("לענן"), "{}", e.message);
        assert_eq!(chosen_export_dir(Some(dir.clone())).unwrap(), dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
