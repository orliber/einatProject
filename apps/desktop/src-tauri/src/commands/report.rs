//! Word report and export.

use crate::*;

#[tauri::command]
pub(crate) async fn check_export(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<ExportCheck> {
    with_core(&state, move |c| c.check_export(&case_id)).await
}

/// Of the notes the finish page shows (by the page's keys), the ones she left as they are
/// (D-193).
#[tauri::command]
pub(crate) async fn left_notes(
    state: tauri::State<'_, AppState>,
    case_id: String,
    keys: Vec<String>,
) -> Res<Vec<String>> {
    with_core(&state, move |c| c.left_notes(&case_id, &keys)).await
}

/// "בסדר, להשאיר" (`left`) and "להחזיר" on the finish page, kept with the case (D-193).
#[tauri::command]
pub(crate) async fn set_note_left(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: Option<String>,
    key: String,
    left: bool,
) -> Res<()> {
    with_core(&state, move |c| {
        c.set_note_left(&case_id, draft_id.as_deref(), &key, left)
    })
    .await
}

/// A new file under a free name in the folder: "name.docx", "name (2).docx", … The file is
/// created, not only named, so a file that appears in between is never overwritten.
pub(crate) fn create_free(
    dir: &std::path::Path,
    file_name: &str,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, "docx"));
    for n in 1..10_000u32 {
        let path = if n == 1 {
            dir.join(file_name)
        } else {
            dir.join(format!("{stem} ({n}).{ext}"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no free name",
    ))
}

/// Write `bytes` into a new file under a free name and return its path. A write that fails
/// leaves no half-written file behind.
pub(crate) fn write_new(
    dir: &std::path::Path,
    file_name: &str,
    bytes: &[u8],
) -> std::io::Result<PathBuf> {
    use std::io::Write as _;
    let (path, mut file) = create_free(dir, file_name)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
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

/// The app's own folder where versions before D-120 saved a report when no usual folder could
/// take it. Nothing is written there any more; what is left is shown to her, never deleted.
fn old_exports_dir(state: &AppState) -> Option<PathBuf> {
    state.dir.parent().map(|d| d.join("exports"))
}

/// How many files an older version left in that folder: only the number, never their names.
fn count_files(dir: &std::path::Path) -> u32 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        let files = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .count();
        u32::try_from(files).unwrap_or(u32::MAX)
    })
}

/// Files an older version saved in the app's own folder (D-120): she is told, so reports with
/// real names do not stay there forgotten.
#[tauri::command]
pub(crate) fn old_exports(state: tauri::State<'_, AppState>) -> u32 {
    old_exports_dir(&state).map_or(0, |d| count_files(&d))
}

/// Open that folder in the system's file window, so she can move or delete what is there.
/// The folder is the app's own; nothing from the window chooses it.
#[tauri::command]
pub(crate) fn open_old_exports(state: tauri::State<'_, AppState>) -> Res<()> {
    let dir = old_exports_dir(&state)
        .filter(|d| d.is_dir())
        .ok_or_else(|| CoreError::Refused("התיקייה כבר לא קיימת.".to_owned()).to_ui())?;
    let opened = if cfg!(windows) {
        let explorer = std::env::var_os("SystemRoot").map_or_else(
            || PathBuf::from("explorer.exe"),
            |root| PathBuf::from(root).join("explorer.exe"),
        );
        std::process::Command::new(explorer).arg(&dir).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/open")
            .arg(&dir)
            .spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&dir).spawn()
    };
    opened
        .map(|_| ())
        .map_err(|_| CoreError::Refused("חלון התיקייה לא נפתח. אפשר לנסות שוב.".to_owned()).to_ui())
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
    let (bytes, check, record) = off_lock(
        &state,
        move |c| {
            c.begin_export_report_as(&case_id, password.as_deref().map(String::as_str), parents)
        },
        |job| job.run(),
        |c, exported| c.take_export(exported),
    )
    .await?;
    save_and_record(&state, dir, check.file_name, bytes, record).await
}

/// Write an exported file into `dir` under a free name, off the core.
async fn save_export(dir: PathBuf, file_name: String, bytes: Vec<u8>) -> Res<String> {
    write_file(dir, file_name, bytes)
        .await
        .map(|path| path.display().to_string())
}

async fn write_file(dir: PathBuf, file_name: String, bytes: Vec<u8>) -> Res<PathBuf> {
    tauri::async_runtime::spawn_blocking(move || {
        write_new(&dir, &file_name, &bytes).map_err(|e| dv_core::file_error(e).to_ui())
    })
    .await
    .map_err(|_| internal("task"))?
}

/// A report or a letter: the file is written first, and only then is the export recorded in
/// the vault (the audit log with its fingerprint, and her style learning from it). A write
/// that fails records nothing (EXPORT-6, JOBS-5).
async fn save_and_record(
    state: &AppState,
    dir: PathBuf,
    file_name: String,
    bytes: Vec<u8>,
    record: dv_core::ExportRecord,
) -> Res<String> {
    // Counted while the file is written and recorded: closing the program waits for it (D-218),
    // so it never ends between the two and leaves a report the vault does not know of.
    let _writing = crate::Exporting::start(&state.exporting);
    write_then_record(dir, file_name, bytes, || {
        with_core(state, move |c| c.record_export(record))
    })
    .await
}

/// Write the file, then `record` it. When the record is refused (the vault locked while the
/// file was written), the file just written is removed again: no report with the names in it
/// is left that the vault does not know of.
async fn write_then_record<F, Fut>(
    dir: PathBuf,
    file_name: String,
    bytes: Vec<u8>,
    record: F,
) -> Res<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Res<()>>,
{
    let path = write_file(dir, file_name, bytes).await?;
    if let Err(e) = record().await {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path.display().to_string())
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
    let (bytes, check, record) = off_lock(
        &state,
        move |c| c.begin_export_pdf_as(&case_id, password.as_deref().map(String::as_str), parents),
        |job| job.run(),
        |c, exported| c.take_export(exported),
    )
    .await?;
    let stem = check
        .file_name
        .rsplit_once('.')
        .map_or(check.file_name.as_str(), |(s, _)| s);
    save_and_record(&state, dir, format!("{stem}.pdf"), bytes, record).await
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

/// D-194: the clinic's income diary of a month (`2026-11`) or a year (`2026`) for the
/// accountant, as a CSV with no name, phone or id of a patient in it, into the same folder
/// as the reports. The core makes the file and logs the period; the window never sees it.
#[tauri::command]
pub(crate) async fn export_income(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    period: String,
) -> Res<String> {
    let dir = export_dir(&app).await?;
    let export = with_core(&state, move |c| c.clinic_income_export(&period)).await?;
    save_export(dir, export.file_name, export.bytes).await
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
    let (bytes, check, record) = off_lock(
        &state,
        move |c| {
            c.begin_export_letter(&case_id, &audience, password.as_deref().map(String::as_str))
        },
        |job| job.run(),
        |c, exported| c.take_export(exported),
    )
    .await?;
    let name = format!("מכתב-{who}-{}", check.file_name);
    save_and_record(&state, dir, name, bytes, record).await
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

    /// An existing file is never overwritten: the second export of the same name gets "(2)",
    /// and the file is created in the same step that takes the name.
    #[test]
    fn a_new_file_is_created_beside_an_existing_one() {
        let dir = scratch("free");
        let first = write_new(&dir, "דוח אבחון TEST-1.docx", b"one").unwrap();
        let second = write_new(&dir, "דוח אבחון TEST-1.docx", b"two").unwrap();
        assert_eq!(first, dir.join("דוח אבחון TEST-1.docx"));
        assert_eq!(second, dir.join("דוח אבחון TEST-1 (2).docx"));
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
        assert_eq!(
            write_new(&dir, "שכבות", b"x").unwrap(),
            dir.join("שכבות"),
            "a name without an extension"
        );
        assert_eq!(
            write_new(&dir, "שכבות", b"y").unwrap(),
            dir.join("שכבות (2).docx")
        );
        // A folder that is not there: an error, and nothing written anywhere.
        assert!(write_new(&dir.join("אין"), "a.docx", b"x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// EXPORT-6, JOBS-5: an export is recorded only once its file is written, and a file
    /// whose record is refused does not stay.
    #[test]
    fn an_export_is_recorded_only_after_its_file_is_written() {
        let dir = scratch("record");
        let recorded = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mark = |r: &std::sync::Arc<std::sync::atomic::AtomicBool>| {
            let r = std::sync::Arc::clone(r);
            move || async move {
                r.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        };
        // A folder that is not there: nothing is recorded.
        let failed = tauri::async_runtime::block_on(write_then_record(
            dir.join("אין"),
            "דוח.docx".to_owned(),
            b"x".to_vec(),
            mark(&recorded),
        ));
        assert!(failed.is_err());
        assert!(!recorded.load(std::sync::atomic::Ordering::SeqCst));
        // Written, then recorded.
        let saved = tauri::async_runtime::block_on(write_then_record(
            dir.clone(),
            "דוח.docx".to_owned(),
            b"x".to_vec(),
            mark(&recorded),
        ))
        .unwrap();
        assert!(recorded.load(std::sync::atomic::Ordering::SeqCst));
        assert!(PathBuf::from(&saved).is_file());
        // The record refused (locked meanwhile): the file is removed.
        let refused = tauri::async_runtime::block_on(write_then_record(
            dir.clone(),
            "מכתב.docx".to_owned(),
            b"y".to_vec(),
            || async { Err(CoreError::Locked.to_ui()) },
        ));
        assert!(refused.is_err());
        assert!(!dir.join("מכתב.docx").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_exports_are_counted_by_number_only() {
        let dir = scratch("old");
        assert_eq!(count_files(&dir.join("exports")), 0);
        std::fs::create_dir_all(dir.join("exports").join("תת-תיקייה")).unwrap();
        std::fs::write(dir.join("exports").join("דוח אבחון TEST-0042.docx"), b"x").unwrap();
        std::fs::write(dir.join("exports").join("מכתב.docx"), b"y").unwrap();
        assert_eq!(count_files(&dir.join("exports")), 2);
        let _ = std::fs::remove_dir_all(&dir);
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
