//! The copy of the clinic's calendar to Google Calendar (D-202), and messages to the family
//! opened on her own device (D-201).

use std::sync::{Arc, Mutex};

use dv_core::calendar_sync::{CalendarCleanup, CalendarJob, CalendarOff, CalendarStatus};
use dv_domain::clinic::{Channel, ClinicReply, ClinicRequest, Handed, MessageKind};

use crate::*;

/// One round of the copy with the core free, then its outcome under the lock. Quiet: a
/// failure is a line in the clinic's settings.
pub(crate) fn sync_in_background(core: Arc<Mutex<Core>>, job: CalendarJob) {
    std::thread::spawn(move || {
        let _ = run_round(&core, job);
    });
}

fn run_round(core: &Arc<Mutex<Core>>, job: CalendarJob) -> Option<()> {
    let done = match (
        dv_core::google::config(),
        dv_core::google::GoogleHttp::new(),
    ) {
        (Ok(cfg), Ok(http)) => job.run(&http, &cfg),
        _ => job.not_started(),
    };
    core.lock().ok()?.finish_calendar_sync(done).ok()
}

/// Delete the app's calendar and withdraw the permission at Google with the core free.
fn clean_up(core: &Arc<Mutex<Core>>, cleanup: CalendarCleanup) -> Option<CalendarOff> {
    let (Ok(cfg), Ok(http)) = (
        dv_core::google::config(),
        dv_core::google::GoogleHttp::new(),
    ) else {
        return Some(CalendarOff::Pending);
    };
    let cleaned = cleanup.run(&http, &cfg);
    core.lock().ok()?.finish_calendar_cleanup(cleaned).ok()
}

/// A cleanup still owed, on its own thread (the timer never waits for a slow line).
pub(crate) fn calendar_cleanup_in_background(core: Arc<Mutex<Core>>, cleanup: CalendarCleanup) {
    std::thread::spawn(move || {
        let _ = clean_up(&core, cleanup);
    });
}

#[tauri::command]
pub(crate) async fn calendar_status(state: tauri::State<'_, AppState>) -> Res<CalendarStatus> {
    with_core(&state, |c| c.calendar_status()).await
}

/// Turn it on: she signs in with Google in her browser and allows the app to manage the
/// calendars it makes itself. A permission given but not kept (the vault locked meanwhile) is
/// withdrawn at once, unless something else rests on the app's grant.
#[tauri::command]
pub(crate) async fn calendar_turn_on(state: tauri::State<'_, AppState>) -> Res<CalendarStatus> {
    let keep_grant = with_core(&state, |c| c.calendar_turn_on_check()).await?;
    let dv_core::calendar_sync::Permission {
        http,
        token,
        account_id,
    } = tauri::async_runtime::spawn_blocking(move || {
        dv_core::calendar_sync::lasting_permission(&open_in_browser, keep_grant)
            .map_err(|e| e.to_ui())
    })
    .await
    .map_err(|_| internal("task"))??;
    let kept = token.clone();
    let finished = with_core(&state, move |c| {
        c.calendar_turn_on_finish(&kept, &account_id)
    })
    .await;
    if let Err(e) = finished {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            dv_core::calendar_sync::withdraw_unkept(&http, &token, keep_grant);
        })
        .await;
        return Err(e);
    }
    // The first round goes at once.
    if let Ok(job) = with_core(&state, |c| c.calendar_sync_now()).await {
        sync_in_background(Arc::clone(&state.core), job);
    }
    with_core(&state, |c| c.calendar_status()).await
}

/// Turn it off: the permission is forgotten at once; then the app's calendar is deleted and
/// the permission withdrawn at Google. Answers how far that got.
#[tauri::command]
pub(crate) async fn calendar_turn_off(state: tauri::State<'_, AppState>) -> Res<CalendarOff> {
    let Some(cleanup) = with_core(&state, |c| c.calendar_turn_off()).await? else {
        return Ok(CalendarOff::Off);
    };
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        clean_up(&core, cleanup).unwrap_or(CalendarOff::Pending)
    })
    .await
    .map_err(|_| internal("task"))
}

/// A round now, because she asked; answers once it is done.
#[tauri::command]
pub(crate) async fn calendar_sync_now(state: tauri::State<'_, AppState>) -> Res<CalendarStatus> {
    let job = with_core(&state, |c| c.calendar_sync_now()).await?;
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || run_round(&core, job))
        .await
        .map_err(|_| internal("task"))?;
    with_core(&state, |c| c.calendar_status()).await
}

/// A message to the family (D-201): the filled text goes to the protected clipboard, and the
/// chat with the contact opens in WhatsApp, the SMS app or her mail program on this computer.
/// Nothing is sent: she pastes and presses send. Only an address of the three fixed shapes
/// is ever opened, and it carries the number or the email address only.
#[tauri::command]
pub(crate) async fn send_message(
    state: tauri::State<'_, AppState>,
    kind: MessageKind,
    patient_id: String,
    visit_id: Option<String>,
    channel: Channel,
    contact: Option<u8>,
) -> Res<Handed> {
    let request = ClinicRequest::Compose {
        kind,
        patient_id,
        visit_id,
        channel,
        contact,
    };
    let composed = with_core(&state, move |c| match c.clinic(request)? {
        ClinicReply::Composed(m) => Ok(m),
        _ => Err(CoreError::Internal("an unexpected answer".to_owned())),
    })
    .await?;
    if !dv_domain::clinic::is_chat_link(&composed.link) {
        return Err(internal("link"));
    }
    let text = zeroize::Zeroizing::new(composed.text);
    state.clipboard.copy(&text).map_err(|_| UiError {
        code: "clipboard".to_owned(),
        message: "לא הצלחתי להעתיק את ההודעה. אפשר לנסות שוב.".to_owned(),
        details: Vec::new(),
        action: dv_core::ErrorAction::retry(),
    })?;
    launch(&composed.link).map_err(|_| {
        CoreError::Refused(
            "התוכנה ששולחת את ההודעה לא נפתחה. ההודעה כבר בלוח המוגן, ואפשר להדביק אותה ידנית."
                .to_owned(),
        )
        .to_ui()
    })?;
    Ok(Handed {
        to: composed.to,
        clears_in: u32::try_from(secret_clipboard::CLEAR_AFTER.as_secs()).unwrap_or(60),
    })
}
