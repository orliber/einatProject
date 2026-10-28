//! Library of materials, sorting, import (D-023).

use crate::*;

#[tauri::command]
pub(crate) async fn list_trash(state: tauri::State<'_, AppState>) -> Res<Vec<CaseSummary>> {
    with_core(&state, |c| c.list_trash()).await
}

#[tauri::command]
pub(crate) async fn restore_case(state: tauri::State<'_, AppState>, case_id: String) -> Res<()> {
    with_core(&state, move |c| c.restore_case(&case_id)).await
}

/// Erase a case from the recycle bin now; asks for the password again.
#[tauri::command]
pub(crate) async fn purge_case(
    state: tauri::State<'_, AppState>,
    case_id: String,
    password: String,
) -> Res<()> {
    let password = zeroize::Zeroizing::new(password);
    let id = case_id.clone();
    off_lock(
        &state,
        move |c| c.begin_purge_case(&id, &password),
        dv_core::PasswordCheckJob::run,
        move |c, checked| c.finish_purge_case(&case_id, checked),
    )
    .await
}

#[tauri::command]
pub(crate) async fn folders(state: tauri::State<'_, AppState>) -> Res<Vec<Folder>> {
    with_core(&state, |c| c.folders()).await
}

#[tauri::command]
pub(crate) async fn create_folder(
    state: tauri::State<'_, AppState>,
    parent_id: Option<String>,
    name: String,
) -> Res<Folder> {
    with_core(&state, move |c| {
        c.create_folder(parent_id.as_deref(), &name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn rename_folder(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
) -> Res<()> {
    with_core(&state, move |c| c.rename_folder(&id, &name)).await
}

#[tauri::command]
pub(crate) async fn move_folder(
    state: tauri::State<'_, AppState>,
    id: String,
    parent_id: Option<String>,
) -> Res<()> {
    with_core(&state, move |c| c.move_folder(&id, parent_id.as_deref())).await
}

#[tauri::command]
pub(crate) async fn delete_folder(state: tauri::State<'_, AppState>, id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_folder(&id)).await
}

#[tauri::command]
pub(crate) async fn move_case(
    state: tauri::State<'_, AppState>,
    case_id: String,
    folder_id: Option<String>,
) -> Res<()> {
    with_core(&state, move |c| c.move_case(&case_id, folder_id.as_deref())).await
}

#[tauri::command]
pub(crate) async fn find_name_matches(
    state: tauri::State<'_, AppState>,
    case_id: Option<String>,
    names: Vec<String>,
) -> Res<Vec<NameMatch>> {
    with_core(&state, move |c| {
        c.find_name_matches(case_id.as_deref(), &names)
    })
    .await
}

/// Sorting materials into sections (D-022): everything not sorted yet, one review screen.
#[tauri::command]
pub(crate) async fn prepare_sort(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<Prepared> {
    warm_names(&state, &case_id, Vec::new()).await;
    prepare_off_lock(&state, move |c| c.begin_prepare_sort(&case_id)).await
}

/// Sorting one request per material not sorted yet, each filtered side by side away from the
/// lock and landed under it, keyed by the material (D-134); they are sent side by side too.
#[tauri::command]
pub(crate) async fn prepare_sort_parts(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<Vec<(String, Prepared)>> {
    warm_names(&state, &case_id, Vec::new()).await;
    let mut jobs = with_core(&state, move |c| c.begin_sort_parts(&case_id)).await?;
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

/// One material's sorting prepared again (a part that was refused, for "לנסות שוב").
#[tauri::command]
pub(crate) async fn prepare_sort_one(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
) -> Res<Prepared> {
    warm_names(&state, &case_id, Vec::new()).await;
    prepare_off_lock(&state, move |c| {
        c.begin_prepare_sort_one(&case_id, &input_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn send_sort(
    state: tauri::State<'_, AppState>,
    approval_id: String,
) -> Res<SortResult> {
    let id = approval_id.clone();
    let out = with_core(&state, move |c| c.begin_send(&id, dv_core::SendKind::Sort)).await?;
    let (out, response) = transmit(&state, approval_id, out).await?;
    with_core(&state, move |c| c.finish_sort(out, response)).await
}

/// Einat's choice of sections for one material; it always wins.
#[tauri::command]
pub(crate) async fn set_input_sections(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
    sections: Vec<String>,
) -> Res<()> {
    with_core(&state, move |c| {
        c.set_input_sections(&case_id, &input_id, &sections)
    })
    .await
}

#[tauri::command]
pub(crate) async fn chat(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
) -> Res<Vec<ChatView>> {
    with_core(&state, move |c| c.chat(&case_id, &section_key)).await
}

#[tauri::command]
pub(crate) async fn create_follow_up(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<String> {
    with_core(&state, move |c| c.create_follow_up(&case_id)).await
}

#[tauri::command]
pub(crate) async fn follow_up(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<Option<dv_core::FollowUpView>> {
    with_core(&state, move |c| c.follow_up(&case_id)).await
}

#[tauri::command]
pub(crate) async fn add_comparison_material(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<dv_domain::CaseInput> {
    with_core(&state, move |c| c.add_comparison_material(&case_id)).await
}

#[tauri::command]
pub(crate) async fn approve_section(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
) -> Res<u32> {
    with_core(&state, move |c| c.approve_section(&case_id, &section_key)).await
}

#[tauri::command]
pub(crate) async fn approve_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<()> {
    with_core(&state, move |c| c.approve_paragraph(&case_id, &draft_id)).await
}

#[tauri::command]
pub(crate) async fn reject_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<()> {
    with_core(&state, move |c| c.reject_paragraph(&case_id, &draft_id)).await
}

#[tauri::command]
pub(crate) async fn edit_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
    text: String,
    opened_with: Option<String>,
) -> Res<bool> {
    warm_names(&state, &case_id, vec![text.clone()]).await;
    with_core(&state, move |c| {
        c.edit_paragraph_opened(&case_id, &draft_id, &text, opened_with.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn paragraph_versions(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<Vec<ParagraphVersionView>> {
    with_core(&state, move |c| c.paragraph_versions(&case_id, &draft_id)).await
}

/// The paragraph of her past reports closest to a draft paragraph (ST2). Local only.
#[tauri::command]
pub(crate) async fn own_section_help(
    state: tauri::State<'_, AppState>,
    section_key: String,
) -> Res<dv_core::OwnSectionHelp> {
    with_core(&state, move |c| c.own_section_help(&section_key)).await
}

/// Her diagnoses, recommendations and the reports most like the case (round 36; local only).
#[tauri::command]
pub(crate) async fn section_library(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
) -> Res<dv_core::SectionLibrary> {
    with_core(&state, move |c| c.section_library(&case_id, &section_key)).await
}

#[tauri::command]
pub(crate) async fn similar_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<Option<dv_core::SimilarParagraph>> {
    with_core(&state, move |c| c.similar_paragraph(&case_id, &draft_id)).await
}

#[tauri::command]
pub(crate) async fn restore_paragraph_version(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
    version_id: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.restore_paragraph_version(&case_id, &draft_id, &version_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn add_own_paragraph(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    text: String,
    first: Option<bool>,
    after: Option<String>,
) -> Res<()> {
    warm_names(&state, &case_id, vec![text.clone()]).await;
    with_core(&state, move |c| {
        // Where it goes (D-036): after a paragraph, first in the section, or at the end.
        let at = match (after.as_deref(), first.unwrap_or(false)) {
            (Some(id), _) => Some(Some(id)),
            (None, true) => Some(None),
            (None, false) => None,
        };
        c.add_own_paragraph_at(&case_id, &section_key, &text, at)
    })
    .await
}
