//! Cases, materials and score sheets.

use crate::*;

#[tauri::command]
pub(crate) async fn list_cases(state: tauri::State<'_, AppState>) -> Res<Vec<CaseSummary>> {
    with_core(&state, |c| c.list_cases()).await
}

#[tauri::command]
pub(crate) async fn create_case(
    state: tauri::State<'_, AppState>,
    meta: CaseMeta,
    identities: Vec<IdentityInput>,
) -> Res<String> {
    with_core(&state, move |c| c.create_case(meta, identities)).await
}

#[tauri::command]
pub(crate) async fn create_sample_case(state: tauri::State<'_, AppState>) -> Res<String> {
    with_core(&state, |c| c.create_sample_case()).await
}

#[tauri::command]
pub(crate) async fn update_case(
    state: tauri::State<'_, AppState>,
    case_id: String,
    meta: CaseMeta,
) -> Res<()> {
    with_core(&state, move |c| c.update_case(&case_id, meta)).await
}

#[tauri::command]
pub(crate) async fn delete_case(state: tauri::State<'_, AppState>, case_id: String) -> Res<()> {
    with_core(&state, move |c| c.delete_case(&case_id)).await
}

#[tauri::command]
pub(crate) async fn set_identities(
    state: tauri::State<'_, AppState>,
    case_id: String,
    identities: Vec<IdentityInput>,
) -> Res<Vec<Identity>> {
    with_core(&state, move |c| c.set_identities(&case_id, identities)).await
}

#[tauri::command]
pub(crate) async fn case_detail(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<CaseDetail> {
    with_core(&state, move |c| c.case_detail(&case_id)).await
}

#[tauri::command]
pub(crate) async fn add_input(
    state: tauri::State<'_, AppState>,
    case_id: String,
    kind: InputKind,
    title: String,
    content: String,
) -> Res<CaseInput> {
    with_core(&state, move |c| {
        c.add_input(&case_id, kind, &title, &content)
    })
    .await
}

#[tauri::command]
pub(crate) async fn update_input(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
    title: String,
    content: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.update_input(&case_id, &input_id, &title, &content)
    })
    .await
}

#[tauri::command]
pub(crate) fn score_instruments() -> Vec<dv_domain::Instrument> {
    dv_core::Core::score_instruments()
}

#[tauri::command]
pub(crate) fn preview_scores(sheet: dv_domain::ScoreSheet) -> Res<String> {
    dv_core::Core::preview_scores(&sheet).map_err(|e| e.to_ui())
}

#[tauri::command]
pub(crate) async fn save_scores(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: Option<String>,
    sheet: dv_domain::ScoreSheet,
) -> Res<dv_domain::CaseInput> {
    with_core(&state, move |c| {
        c.save_scores(&case_id, input_id.as_deref(), &sheet)
    })
    .await
}

/// "למה כתבת את זה?": the passages one paragraph leans on (local only).
#[tauri::command]
pub(crate) async fn paragraph_sources(
    state: tauri::State<'_, AppState>,
    case_id: String,
    draft_id: String,
) -> Res<Vec<dv_core::SourceExcerpt>> {
    with_core(&state, move |c| c.paragraph_sources(&case_id, &draft_id)).await
}

#[tauri::command]
pub(crate) async fn score_sheet(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
) -> Res<Option<dv_domain::ScoreSheet>> {
    with_core(&state, move |c| c.score_sheet(&case_id, &input_id)).await
}

#[tauri::command]
pub(crate) async fn delete_input(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
) -> Res<()> {
    with_core(&state, move |c| c.delete_input(&case_id, &input_id)).await
}

#[tauri::command]
pub(crate) async fn restore_input(
    state: tauri::State<'_, AppState>,
    case_id: String,
    input_id: String,
) -> Res<()> {
    with_core(&state, move |c| c.restore_input(&case_id, &input_id)).await
}

/// Percent-decoding for the (ASCII-only) IPC headers that carry a Hebrew file name.
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The file's bytes arrive as the raw request body (only what the psychologist picked or
/// dropped); the case id and file name come as headers.
#[tauri::command]
pub(crate) async fn import_document(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Res<ImportPreview> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(internal("body"));
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(percent_decode)
    };
    let case_id = header("x-case-id").ok_or_else(|| internal("case"))?;
    let file_name = header("x-file-name").unwrap_or_default();
    let bytes = bytes.clone();
    with_core(&state, move |c| {
        c.import_document(&case_id, &file_name, &bytes)
    })
    .await
}

#[tauri::command]
pub(crate) async fn preview_filter(
    state: tauri::State<'_, AppState>,
    case_id: String,
    text: String,
) -> Res<dv_privacy::FilterOutcome> {
    with_core(&state, move |c| c.preview_filter(&case_id, &text)).await
}

#[tauri::command]
pub(crate) async fn decide_suspect(
    state: tauri::State<'_, AppState>,
    case_id: String,
    token: String,
    decision: SuspectDecision,
) -> Res<()> {
    with_core(&state, move |c| {
        c.decide_suspect(&case_id, &token, decision)
    })
    .await
}

/// The button on a request stopped for a tag that is not the case's.
#[tauri::command]
pub(crate) async fn settle_foreign_tags(
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Res<usize> {
    with_core(&state, move |c| c.settle_foreign_tags(&case_id)).await
}

/// "להחזיר" on the summary card: keep what the filter hid as written, for this case.
#[tauri::command]
pub(crate) async fn confirm_auto_hidden(
    state: tauri::State<'_, AppState>,
    case_id: String,
    token: String,
    tag: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.confirm_auto_hidden(&case_id, &token, &tag)
    })
    .await
}

#[tauri::command]
pub(crate) async fn restore_auto_hidden(
    state: tauri::State<'_, AppState>,
    case_id: String,
    token: String,
    tag: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.restore_auto_hidden(&case_id, &token, &tag)
    })
    .await
}

/// The card's role menu for a name the filter kept.
#[tauri::command]
pub(crate) async fn change_role(
    state: tauri::State<'_, AppState>,
    case_id: String,
    tag: String,
    role: Role,
) -> Res<Vec<Identity>> {
    with_core(&state, move |c| c.change_role(&case_id, &tag, role)).await
}
