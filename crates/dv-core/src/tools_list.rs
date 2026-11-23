//! The list of the assessment's tools, ready in "כלי האבחון" (D-215): she asked for the
//! section to name the tools and nothing more, so it is built here from what the case holds,
//! with no request: the instruments whose scores were entered, and the interview, the report
//! of the educational setting and the observation when the case has such a material.
//!
//! Like the summary of an earlier report (D-209) it is a proposal she edits or deletes. It
//! follows the case while it is still the list written here; once she approved it, edited it
//! or wrote the section herself, nothing is touched.

use dv_domain::{Author, DraftStatus, InputKind, SectionMode};
use dv_vault::Vault;

use crate::{score_sheets, structure_for, CoreError};

const SECTION: &str = "tools";
/// How the list opens. Also how a paragraph is known to be the list written here.
const OPENING: &str = "במסגרת האבחון נעשה שימוש בכלים אלו:";

/// The tools of the case, in the order a report names them, each once.
fn tools(v: &Vault, case_id: &str) -> Result<(Vec<String>, Vec<String>), CoreError> {
    let inputs = v.inputs(case_id)?;
    let has = |kinds: &[InputKind]| inputs.iter().any(|i| kinds.contains(&i.kind));
    let mut names: Vec<String> = Vec::new();
    if has(&[InputKind::Intake]) {
        names.push("ראיון הורים".to_owned());
    }
    if has(&[InputKind::Kindergarten]) {
        names.push("דיווח מהמסגרת החינוכית".to_owned());
    }
    if has(&[InputKind::Observation, InputKind::SessionNote]) {
        names.push("תצפית קלינית".to_owned());
    }
    let table = dv_domain::instruments();
    let mut refs = Vec::new();
    for input in inputs.iter().filter(|i| i.kind == InputKind::TestScores) {
        let Some(data) = v.input_data(case_id, &input.id)? else {
            continue;
        };
        let Ok(sheet) = serde_json::from_str::<dv_domain::ScoreSheet>(&data) else {
            continue;
        };
        refs.push(input.id.clone());
        let Some(inst) = table.iter().find(|i| i.key == sheet.instrument) else {
            continue;
        };
        let module = sheet.module.trim();
        let name = if module.is_empty() {
            inst.name.clone()
        } else {
            format!("{} (מודול {module})", inst.name)
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    // An instrument she does not give is in no list (D-098): only the sheets the requests see.
    let given = score_sheets(v, case_id, &inputs)?;
    if given.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    names.retain(|n| {
        table
            .iter()
            .filter(|i| n == &i.name || n.starts_with(&format!("{} (", i.name)))
            .all(|i| given.iter().any(|s| s.instrument == i.key))
    });
    Ok((names, refs))
}

/// Writes or refreshes the list. Does nothing when the section is hers to write, when it has
/// an approved paragraph or one she wrote, or when a draft that is not this list waits there.
pub(crate) fn refresh(v: &mut Vault, case_id: &str) -> Result<(), CoreError> {
    let writes = structure_for(v, case_id)?
        .section(SECTION)
        .is_some_and(|s| s.mode == SectionMode::Ai);
    if !writes {
        return Ok(());
    }
    let on_page: Vec<dv_domain::DraftParagraph> = v
        .drafts(case_id, SECTION)?
        .into_iter()
        .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
        .collect();
    let ours = |d: &dv_domain::DraftParagraph| {
        d.status == DraftStatus::Proposed
            && d.author == Author::Ai
            && d.replaces.is_none()
            && d.text_tagged.starts_with(OPENING)
    };
    if on_page.iter().any(|d| !ours(d)) {
        return Ok(());
    }
    let (names, refs) = tools(v, case_id)?;
    if names.is_empty() {
        return Ok(());
    }
    let text = format!("{OPENING} {}.", names.join(", "));
    match on_page.first() {
        Some(d) if d.text_tagged == text => {}
        Some(d) => {
            v.rewrite_proposal(case_id, &d.id, &text, &refs)?;
        }
        None => {
            v.add_draft(case_id, SECTION, &text, Author::Ai, &refs)?;
        }
    }
    Ok(())
}
