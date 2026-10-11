//! Drafting: chat, paragraphs, sending a section.

use crate::*;

#[tauri::command]
pub(crate) async fn prepare_section(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    instruction: String,
    replaces: Option<String>,
) -> Res<Prepared> {
    with_core(&state, move |c| {
        c.prepare_section_replacing(&case_id, &section_key, &instruction, replaces.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn prepare_full_draft(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<Vec<(String, Prepared)>> {
    with_core(&state, move |c| c.prepare_full_draft(&case_id)).await
}

/// Take the approved payload, send it without holding the core, then store the answer.
#[tauri::command]
pub(crate) async fn send_section(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<SectionResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_section(out, response)).await
}

/// Copy the Word file's password: kept out of clipboard history and cloud sync, and taken
/// off the clipboard after 60 seconds or when the vault locks (STANDARDS 5.9).
#[tauri::command]
pub(crate) async fn copy_secret(state: tauri::State<'_, AppState>, text: String) -> Res<u32> {
    let text = zeroize::Zeroizing::new(text);
    state.clipboard.copy(&text).map_err(|_| UiError {
        code: "clipboard".to_owned(),
        message: "לא הצלחתי להעתיק. אפשר להקליד את הסיסמה.".to_owned(),
        details: Vec::new(),
    })?;
    Ok(u32::try_from(secret_clipboard::CLEAR_AFTER.as_secs()).unwrap_or(60))
}
