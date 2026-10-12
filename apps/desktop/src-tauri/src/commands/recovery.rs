//! Forgot password: Google (D-041).

use crate::*;

/// Open Google's sign-in page, or the page of released versions (R7), in her default browser
/// (never inside the program). Only those fixed addresses are ever opened from here.
pub(crate) fn open_in_browser(url: &str) -> Result<(), CoreError> {
    let allowed =
        url.starts_with(dv_core::google::AUTH_URL) || url == dv_core::update::RELEASES_PAGE;
    if !allowed || url.contains(['"', '\'', ' ', '\n']) {
        return Err(CoreError::Internal(
            "refused to open an unexpected address".to_owned(),
        ));
    }
    let opened = if cfg!(windows) {
        // By full path; `url.dll` takes the address as one argument, no shell in between.
        let rundll = std::env::var_os("SystemRoot").map_or_else(
            || PathBuf::from("rundll32.exe"),
            |root| PathBuf::from(root).join(r"System32\rundll32.exe"),
        );
        std::process::Command::new(rundll)
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    opened
        .map(|_| ())
        .map_err(|_| CoreError::Refused("הדפדפן לא נפתח. אפשר לנסות שוב.".to_owned()))
}

/// Sign in outside the core's lock, then run `then` with the session (still outside it).
pub(crate) async fn with_google<T, F>(then: F) -> Res<T>
where
    T: Send + 'static,
    F: FnOnce(&dv_core::google::GoogleHttp, dv_core::google::Session) -> Result<T, CoreError>
        + Send
        + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let (http, session) = dv_core::google::sign_in(&open_in_browser).map_err(|e| e.to_ui())?;
        then(&http, session).map_err(|e| e.to_ui())
    })
    .await
    .map_err(|_| internal("task"))?
}

#[tauri::command]
pub(crate) async fn google_status(
    state: tauri::State<'_, AppState>,
) -> Res<dv_core::google::GoogleStatus> {
    with_core(&state, |c| {
        Ok(c.google_status(&dv_core::google::WindowsAccount))
    })
    .await
}

/// Stop a sign-in that waits for the browser.
#[tauri::command]
pub(crate) fn google_cancel() {
    dv_core::google::cancel();
}

/// Turn on "forgot password → sign in with Google": check the password, sign in, put a new
/// key in her Drive's hidden app folder, write the slot, delete older keys.
#[tauri::command]
pub(crate) async fn google_turn_on(state: tauri::State<'_, AppState>, password: String) -> Res<()> {
    let password = zeroize::Zeroizing::new(password);
    let check = password.clone();
    let vault_id = with_core(&state, move |c| c.google_turn_on_check(&check)).await?;
    let (drive, account, new_id, older, http_session) = with_google(move |http, session| {
        match dv_core::google::put_new_key(http, &session, &vault_id) {
            Ok((key, id, older)) => Ok((key, session.account_id.clone(), id, older, Some(session))),
            Err(e) => {
                dv_core::google::tidy_up(http, session, &[]);
                Err(e)
            }
        }
    })
    .await?;
    let finished = with_core(&state, move |c| {
        c.google_turn_on_finish(
            &dv_core::google::WindowsAccount,
            &password,
            &drive,
            &account,
        )
    })
    .await;
    // Clean up at Google: on success the older keys, on failure the new one.
    let stale = if finished.is_ok() {
        older
    } else {
        vec![new_id]
    };
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let (Ok(http), Some(session)) = (dv_core::google::GoogleHttp::new(), http_session) {
            dv_core::google::tidy_up(&http, session, &stale);
        }
    })
    .await;
    finished
}

#[tauri::command]
pub(crate) async fn google_turn_off(
    state: tauri::State<'_, AppState>,
    password: String,
) -> Res<()> {
    let password = zeroize::Zeroizing::new(password);
    with_core(&state, move |c| {
        c.google_turn_off(&dv_core::google::WindowsAccount, &password)
    })
    .await
}

/// Forgot the password: sign in with the connected Google account on this computer, open
/// the vault and set `new_password` at once.
#[tauri::command]
pub(crate) async fn google_recover(
    state: tauri::State<'_, AppState>,
    new_password: String,
) -> Res<AppStatus> {
    let new_password = zeroize::Zeroizing::new(new_password);
    let check = new_password.clone();
    let (vault_id, tag) = with_core(&state, move |c| {
        c.google_recover_check(&dv_core::google::WindowsAccount, &check)
    })
    .await?;
    let keys = with_google(move |http, session| {
        let result = if dv_core::google::same_account(&vault_id, &tag, &session) {
            dv_core::google::read_keys(http, &session, &vault_id)
        } else {
            Err(CoreError::Refused(
                "זה לא חשבון הגוגל שחובר לכספת. אפשר לנסות שוב ולבחור את החשבון שחובר.".to_owned(),
            ))
        };
        dv_core::google::tidy_up(http, session, &[]);
        result
    })
    .await?;
    with_core(&state, move |c| {
        c.google_recover_finish(&dv_core::google::WindowsAccount, &keys, &new_password)
    })
    .await
}
