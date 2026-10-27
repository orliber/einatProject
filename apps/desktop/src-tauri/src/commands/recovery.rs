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
/// `keep_grant` as for [`dv_core::google::tidy_up`].
pub(crate) async fn with_google<T, F>(keep_grant: bool, then: F) -> Res<T>
where
    T: Send + 'static,
    F: FnOnce(&dv_core::google::GoogleHttp, dv_core::google::Session) -> Result<T, CoreError>
        + Send
        + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let (http, session) =
            dv_core::google::sign_in(&open_in_browser, keep_grant).map_err(|e| e.to_ui())?;
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
    // The password is checked away from the lock (F2) before her browser opens.
    let vault_id = off_lock(
        &state,
        move |c| c.begin_google_turn_on_check(&check),
        dv_core::PasswordCheckJob::run,
        |c, checked| c.finish_google_turn_on_check(checked),
    )
    .await?;
    // While the encrypted backup copy is on or its cleanup is owed, the session is not revoked
    // at the end: at Google that would withdraw the copy's lasting permission too (D-099).
    let keep_grant = with_core(&state, |c| Ok(c.drive_keeps_grant())).await?;
    let (drive, account, new_id, older, http_session) = with_google(
        keep_grant,
        move |http, session| match dv_core::google::put_new_key(http, &session, &vault_id) {
            Ok((key, id, older)) => Ok((key, session.account_id.clone(), id, older, Some(session))),
            Err(e) => {
                dv_core::google::tidy_up(http, session, &[], keep_grant);
                Err(e)
            }
        },
    )
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
            dv_core::google::tidy_up(&http, session, &stale, keep_grant);
        }
    })
    .await;
    finished
}

/// Disconnect Google. The encrypted backup copy rests on this connection, so it turns off too
/// (D-099): its copies are deleted and its permission withdrawn at Google, with the core free.
#[tauri::command]
pub(crate) async fn google_turn_off(
    state: tauri::State<'_, AppState>,
    password: String,
) -> Res<()> {
    let password = zeroize::Zeroizing::new(password);
    let cleanup = with_core(&state, move |c| {
        c.google_turn_off(&dv_core::google::WindowsAccount, &password)
    })
    .await?;
    if let Some(cleanup) = cleanup {
        super::cleanup_in_background(Arc::clone(&state.core), cleanup);
    }
    Ok(())
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
    // The session ends after the vault is open: only then is it known whether the encrypted
    // backup copy is on or its cleanup owed, which a revoke would strand (D-099). Until then
    // (locked) the grant is left alone, and the short token runs out within the hour.
    let (keys, session) = with_google(true, move |http, session| {
        let result = if dv_core::google::same_account(&vault_id, &tag, &session) {
            dv_core::google::read_keys(http, &session, &vault_id)
        } else {
            Err(CoreError::Refused(
                "זה לא חשבון הגוגל שחובר לכספת. אפשר לנסות שוב ולבחור את החשבון שחובר.".to_owned(),
            ))
        };
        match result {
            Ok(keys) => Ok((keys, session)),
            Err(e) => {
                dv_core::google::tidy_up(http, session, &[], true);
                Err(e)
            }
        }
    })
    .await?;
    let opened = with_core(&state, move |c| {
        c.google_recover_finish(&dv_core::google::WindowsAccount, &keys, &new_password)
    })
    .await;
    // Still locked (not opened): not known, left alone.
    let keep_grant = with_core(&state, |c| Ok(c.drive_keeps_grant()))
        .await
        .unwrap_or(true);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(http) = dv_core::google::GoogleHttp::new() {
            dv_core::google::tidy_up(&http, session, &[], keep_grant);
        }
    })
    .await;
    opened
}
