//! Drafting: chat, paragraphs, sending a section.

use crate::*;

#[tauri::command]
pub(crate) async fn prepare_from_points(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    points: String,
) -> Res<Prepared> {
    warm_names(&state, &case_id, Vec::new()).await;
    prepare_off_lock(&state, move |c| {
        c.begin_prepare_from_points(&case_id, &section_key, &points)
    })
    .await
}

#[tauri::command]
pub(crate) async fn prepare_section(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    instruction: String,
    replaces: Option<String>,
) -> Res<Prepared> {
    warm_names(&state, &case_id, Vec::new()).await;
    prepare_off_lock(&state, move |c| {
        c.begin_prepare_section(&case_id, &section_key, &instruction, replaces.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn prepare_full_draft(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<Vec<(String, Prepared)>> {
    warm_names(&state, &case_id, Vec::new()).await;
    // Every section read in one moment under the lock, filtered side by side away from it
    // (D-109), and each landed under the lock; one whose ground changed is begun again.
    let mut jobs = with_core(&state, move |c| c.begin_full_draft(&case_id)).await?;
    let order: Vec<String> = jobs.iter().map(|(k, _)| k.clone()).collect();
    let mut out = Vec::new();
    while !jobs.is_empty() {
        let runs = tauri::async_runtime::spawn_blocking(move || dv_core::run_requests(jobs))
            .await
            .map_err(|_| internal("task"))?
            .map_err(|e| e.to_ui())?;
        let (ready, again) = with_core(&state, move |c| c.finish_requests(runs)).await?;
        out.extend(ready);
        jobs = again;
    }
    out.sort_by_key(|(k, _)| order.iter().position(|o| o == k));
    Ok(out)
}

/// Take the approved payload, send it without holding the core, then store the answer.
#[tauri::command]
pub(crate) async fn send_section(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<SectionResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| {
        c.begin_send(&id, dv_core::SendKind::Section)
    })
    .await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_section(out, response)).await
}

/// Copy the Word file's password: kept out of clipboard history and cloud sync, and taken
/// off the clipboard after 60 seconds (20 on a Mac) or when the vault locks (STANDARDS 5.9).
#[tauri::command]
pub(crate) async fn copy_secret(state: tauri::State<'_, AppState>, text: String) -> Res<u32> {
    let text = zeroize::Zeroizing::new(text);
    state.clipboard.copy(&text).map_err(|_| UiError {
        code: "clipboard".to_owned(),
        message: "לא הצלחתי להעתיק. אפשר להקליד את הסיסמה.".to_owned(),
        details: Vec::new(),
        action: dv_core::ErrorAction::retry(),
    })?;
    Ok(u32::try_from(secret_clipboard::CLEAR_AFTER.as_secs()).unwrap_or(60))
}
