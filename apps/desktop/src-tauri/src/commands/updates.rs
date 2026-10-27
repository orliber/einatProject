//! Updates (D-033).

use crate::*;

/// Is there a newer version? Nothing from the vault is involved; the core's lock is not held.
#[tauri::command]
pub(crate) async fn check_update() -> Res<Option<dv_core::update::UpdateView>> {
    tauri::async_runtime::spawn_blocking(|| dv_core::update::check().map_err(|e| e.to_ui()))
        .await
        .map_err(|_| internal("task"))?
}

/// A version that cannot open the vault (R7): the page of released versions, in her browser,
/// to install the one before. Works with the vault locked; nothing from it is involved.
#[tauri::command]
pub(crate) fn open_releases_page() -> Res<()> {
    super::recovery::open_in_browser(dv_core::update::RELEASES_PAGE).map_err(|e| e.to_ui())
}

/// Fetch and verify the newest version in the background, so the click only restarts
/// (D-038). Nothing is installed here. `true` when a verified installer is waiting.
#[tauri::command]
pub(crate) async fn prepare_update(state: tauri::State<'_, AppState>) -> Res<bool> {
    if !cfg!(any(windows, target_os = "macos")) {
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

/// Run the verified installer, then close. Everything that can fail and needs nothing from
/// the vault (the file, its hash, the version, on a Mac the whole install) happens first,
/// while the vault is still open; it is locked only right before the installer starts, so a
/// refusal never leaves her at the lock screen for nothing. The installer (passive, update
/// mode) replaces the program and opens it again; the vault folder is not touched. A version
/// fetched in the background is used when it is still intact; otherwise it is fetched now.
/// On a Mac (D-101) the new app from the verified disk image takes the old one's place and is
/// opened. Only where every check held and the app's folder cannot be written does a new
/// private copy of the verified image open in Finder, and she is told what to do; a failed
/// check opens nothing.
#[tauri::command]
pub(crate) async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Res<()> {
    if !cfg!(any(windows, target_os = "macos")) {
        return Err(UiError {
            code: "update".to_owned(),
            message: "כאן העדכון לא מותקן לבד. מתקינים את הגרסה החדשה מהקישור ששלח אור.".to_owned(),
            details: Vec::new(),
            action: dv_core::ErrorAction::Dismiss,
        });
    }
    let dir = state.dir.clone();
    let prepared = Arc::clone(&state.prepared);
    // 1. The installer, checked once and off the UI thread: the one fetched in the background
    //    when it is still the verified file, or fetched and checked now. Never an older
    //    version, nor this one again (D-104). The vault is not involved.
    let file = tauri::async_runtime::spawn_blocking(move || {
        let waiting = prepared.lock().ok().and_then(|mut p| p.take());
        let file = match waiting {
            Some(file) if dv_core::update::still_intact(&file) => file,
            _ => dv_core::update::download(&dir).map_err(|e| e.to_ui())?,
        };
        dv_core::update::refuse_downgrade(&file).map_err(|e| e.to_ui())?;
        Ok::<_, UiError>(file)
    })
    .await
    .map_err(|_| internal("task"))??;
    if cfg!(target_os = "macos") {
        return install_on_mac(&app, &state, file).await;
    }
    // 2. Windows: the file is opened and hashed through one handle, held until the installer
    //    has started: while it is held no one may write the file, so what runs is what was
    //    checked (the gap D-038 accepted is closed).
    let held = dv_core::update::open_intact(&file).ok_or_else(|| UiError {
        code: "update".to_owned(),
        message: "קובץ ההתקנה שהורד השתנה או נמחק לפני ההתקנה (אולי תוכנת אנטי-וירוס). התוכנה ממשיכה לעבוד כרגיל; אפשר לנסות שוב מההגדרות, ואם זה חוזר, כדאי לספר לאור.".to_owned(),
        details: Vec::new(),
        action: dv_core::ErrorAction::Dismiss,
    })?;
    // 3. Only now the vault locks, and the installer starts at once.
    lock_vault(&state).await?;
    let started = std::process::Command::new(&file.path)
        .args(["/P", "/UPDATE", "/R"])
        .spawn();
    drop(held);
    match started {
        Ok(_) => {
            app.exit(0);
            Ok(())
        }
        Err(_) => {
            give_back(&state, file);
            Err(locked_after(
                "המתקין לא התחיל, והכספת כבר ננעלה ליתר ביטחון. הגרסה הנוכחית נשארת כמו שהיא: פותחים את הכספת מחדש ומנסים שוב מההגדרות. אם זה חוזר, כדאי לספר לאור.",
            ))
        }
    }
}

/// Lock the vault (what she was typing a moment ago is kept by the core), and clear a secret
/// left on the clipboard.
async fn lock_vault(state: &tauri::State<'_, AppState>) -> Res<()> {
    state.clipboard.clear_now();
    with_core(state, |c| {
        c.lock();
        Ok(())
    })
    .await
}

/// A verified installer that did not run goes back to waiting for the next click, so it is
/// not fetched again.
fn give_back(state: &tauri::State<'_, AppState>, file: dv_core::update::Downloaded) {
    if let Ok(mut slot) = state.prepared.lock() {
        if dv_core::update::still_intact(&file) {
            *slot = Some(file);
        }
    }
}

/// Something failed after the vault was locked: the window goes to the lock screen (the
/// code `locked` is what sends it there) with what happened and what to do.
fn locked_after(message: &str) -> UiError {
    UiError {
        code: "locked".to_owned(),
        message: message.to_owned(),
        details: Vec::new(),
        action: dv_core::ErrorAction::Unlock,
    }
}

/// The Mac half of [`install_update`]. The install itself never touches the vault, so it
/// runs whole, and checked, while the vault is open; the lock comes right before the new
/// version is started.
async fn install_on_mac(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
    file: dv_core::update::Downloaded,
) -> Res<()> {
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|exe| dv_core::update::bundle_of(&exe));
    let work = app.path().app_local_data_dir().ok();
    let (Some(bundle), Some(work)) = (bundle, work) else {
        give_back(state, file);
        return Err(UiError {
            code: "update".to_owned(),
            message: "התוכנה לא רצה מתוך אפליקציה רגילה (למשל מתוך קובץ הדמות או מתיקייה זמנית), ולכן העדכון לא מותקן לבד. הגרסה הנוכחית נשארת כמו שהיא. מורידים את הגרסה החדשה מדף הגרסאות ומתקינים אותה כרגיל.".to_owned(),
            details: Vec::new(),
            action: dv_core::ErrorAction::Dismiss,
        });
    };
    let placed = tauri::async_runtime::spawn_blocking(move || {
        use dv_core::update::{clean_private, install_macos, private_image, MacInstallError};
        clean_private(&work);
        match install_macos(&file, &bundle, &work) {
            Ok(()) => Placed::Installed(bundle),
            Err(MacInstallError::NotWritable) => match private_image(&file, &work) {
                Ok(image) => Placed::ByHand(image),
                Err(_) => Placed::Not(file),
            },
            Err(MacInstallError::Refused(_)) => Placed::Not(file),
        }
    })
    .await
    .map_err(|_| internal("task"))?;
    match placed {
        Placed::Installed(bundle) => {
            // The new version starts only once this one has exited, so the two never hold the
            // vault at once. The paths are arguments to the script, never part of it.
            lock_vault(state).await?;
            let started = std::process::Command::new("/bin/sh")
                .args([
                    "-c",
                    "while kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open -n \"$2\"",
                    "dv-relaunch",
                    &std::process::id().to_string(),
                ])
                .arg(&bundle)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if started.is_err() {
                // The new version is already in place; only its start is missing.
                return Err(locked_after(
                    "הגרסה החדשה הותקנה, אבל לא נפתחה לבד, והכספת כבר ננעלה ליתר ביטחון. סוגרים את התוכנה ופותחים אותה מחדש מתיקיית Applications.",
                ));
            }
            app.exit(0);
            Ok(())
        }
        Placed::ByHand(image) => {
            // The checked app, in a new private copy of the verified image, opens in Finder.
            // The vault stays open and the program keeps running: Finder will not replace an
            // app that is open, so she is told to close it and drag the new one herself.
            // TODO(UI): `update_by_hand` deserves its own wording in the dialog (a note, not
            // an error line); until then the message below is shown as the error text.
            let opened = std::process::Command::new("/usr/bin/open")
                .arg(&image)
                .spawn()
                .is_ok();
            Err(UiError {
                code: "update_by_hand".to_owned(),
                message: if opened {
                    "הגרסה החדשה נבדקה ונפתחה בחלון Finder. בתיקייה שבה התוכנה נמצאת אי אפשר להחליף אותה לבד: סוגרים את כספת האבחון, גוררים את האפליקציה החדשה מהחלון שנפתח לתיקיית Applications (ומאשרים החלפה), ופותחים אותה משם. התיקים והסיסמה נשארים.".to_owned()
                } else {
                    "הגרסה החדשה נבדקה, אבל בתיקייה שבה התוכנה נמצאת אי אפשר להחליף אותה לבד, וגם קובץ הדמות לא נפתח. מורידים את הגרסה החדשה מדף הגרסאות ומתקינים אותה כרגיל. הגרסה הנוכחית נשארת כמו שהיא.".to_owned()
                },
                details: Vec::new(),
                action: dv_core::ErrorAction::Dismiss,
            })
        }
        Placed::Not(file) => {
            give_back(state, file);
            Err(not_installed())
        }
    }
}

/// What the Mac install came to.
enum Placed {
    /// In place: start it once this version has exited.
    Installed(std::path::PathBuf),
    /// Checked, but the app's folder cannot be written: this private image opens in Finder.
    ByHand(std::path::PathBuf),
    /// Not installed: the version in place stays and nothing is opened.
    Not(dv_core::update::Downloaded),
}

fn not_installed() -> UiError {
    UiError {
        code: "update".to_owned(),
        message: "הגרסה החדשה לא הותקנה, והגרסה הנוכחית נשארת כמו שהיא. אפשר לנסות שוב מההגדרות. אם זה חוזר, כדאי לספר לאור.".to_owned(),
        details: Vec::new(),
        action: dv_core::ErrorAction::Dismiss,
    }
}
