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
pub(crate) async fn set_case_guide(
    state: tauri::State<'_, AppState>,
    case_id: String,
    section_key: String,
    text: String,
) -> Res<()> {
    with_core(&state, move |c| {
        c.set_case_guide(&case_id, &section_key, &text)
    })
    .await
}

/// The DSM-5 severity she set (round 43): a level 1–3 for each domain, or none.
#[tauri::command]
pub(crate) async fn set_dsm_severity(
    state: tauri::State<'_, AppState>,
    case_id: String,
    social: Option<u8>,
    restricted: Option<u8>,
) -> Res<()> {
    with_core(&state, move |c| {
        c.set_dsm_severity(&case_id, social, restricted)
    })
    .await
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
    warm_names(&state, &case_id, vec![title.clone(), content.clone()]).await;
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
    warm_names(&state, &case_id, vec![title.clone(), content.clone()]).await;
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
pub(crate) async fn preview_scores(
    state: tauri::State<'_, AppState>,
    sheet: dv_domain::ScoreSheet,
) -> Res<String> {
    with_core(&state, move |c| c.preview_scores(&sheet)).await
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
    let mut out = Vec::with_capacity(s.len());
    let mut rest = s.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        let pair = match tail {
            [hi, lo, ..] if b == b'%' => (*hi as char).to_digit(16).zip((*lo as char).to_digit(16)),
            _ => None,
        };
        match pair {
            Some((hi, lo)) => {
                out.push((hi * 16 + lo) as u8);
                rest = tail.get(2..).unwrap_or_default();
            }
            None => {
                out.push(b);
                rest = tail;
            }
        }
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
    // Reading the file (and OCR for a scan) runs with the core free (F2), and so does the
    // name model on what was read.
    let job = {
        let case_id = case_id.clone();
        with_core(&state, move |c| {
            c.begin_import_document(&case_id, &file_name, bytes)
        })
        .await?
    };
    let read = tauri::async_runtime::spawn_blocking(move || job.run())
        .await
        .map_err(|_| internal("task"))?;
    warm_names(&state, &case_id, read.texts()).await;
    with_core(&state, move |c| c.finish_import_document(&case_id, read)).await
}

#[tauri::command]
pub(crate) async fn preview_filter(
    state: tauri::State<'_, AppState>,
    case_id: String,
    text: String,
) -> Res<dv_privacy::FilterOutcome> {
    warm_names(&state, &case_id, vec![text.clone()]).await;
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

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn headers_decode_hebrew_and_never_trip_on_a_broken_escape() {
        assert_eq!(
            percent_decode("%D7%93%D7%95%D7%97%20%D7%92%D7%9F%3F.docx"),
            "דוח גן?.docx"
        );
        // A cut or invalid escape stays as written; nothing past the end is read.
        for s in ["%", "%D", "a%", "%zz", "%+f", "100%"] {
            assert_eq!(percent_decode(s), s);
        }
        assert_eq!(percent_decode("%D7"), "\u{FFFD}");
    }
}
