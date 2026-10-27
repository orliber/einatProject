//! Her approval of the score tables (round 59, D-098): for each instrument of the fixed tables
//! (`dv_domain::instruments`), "מאושר" or "לא מעבירה את זה". Until she answers, every one is
//! "מחכה לאישור".
//!
//! - **Not given** ("לא מעבירה את זה"): left out wherever a score could reach the model. The
//!   score table does not offer it and refuses to save it; a sheet saved before stays in the
//!   case as it was stored, but feeds no section, no sorting, no interpretation of her style and
//!   no follow-up comparison, and its table is not appended to the report.
//! - **Awaiting approval:** works as before (for invented cases), is marked in the score table,
//!   and keeps the readiness line "טבלאות המבחנים אושרו" off while it is in an open case.
//!
//! The tables themselves are not changed here: a norm is never typed from memory.

use std::collections::{HashMap, HashSet};

use dv_domain::{CaseInput, InputKind, Instrument, ScoreSheet};
use dv_vault::Vault;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Core, CoreError};

const PREFIX: &str = "instrument/";

/// Her answer for one instrument's tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum InstrumentStatus {
    /// Not answered yet (the default): usable, marked as a draft table.
    Awaiting,
    /// "מאושר".
    Approved,
    /// "לא מעבירה את זה": in no request, not in the score table.
    NotUsed,
}

impl InstrumentStatus {
    /// What is stored: only her two answers are ever written.
    fn stored(self) -> Option<&'static str> {
        match self {
            Self::Awaiting => None,
            Self::Approved => Some("approved"),
            Self::NotUsed => Some("not_used"),
        }
    }

    /// A value this program did not write is read as "not given": the stricter answer.
    fn read(value: Option<&str>) -> Self {
        match value {
            None | Some("") => Self::Awaiting,
            Some("approved") => Self::Approved,
            Some(_) => Self::NotUsed,
        }
    }
}

/// One instrument as the settings and the score table show it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstrumentApproval {
    pub instrument: Instrument,
    pub status: InstrumentStatus,
    /// Open cases with a score table of this instrument (the readiness line looks at these).
    pub open_cases: u32,
}

/// Her answer for the instrument `key`.
pub(crate) fn status_of(v: &Vault, key: &str) -> Result<InstrumentStatus, CoreError> {
    Ok(InstrumentStatus::read(
        v.setting(&format!("{PREFIX}{key}"))?.as_deref(),
    ))
}

/// The instruments she marked "לא מעבירה את זה".
pub(crate) fn not_used(v: &Vault) -> Result<HashSet<String>, CoreError> {
    let mut out = HashSet::new();
    for i in dv_domain::instruments() {
        if status_of(v, &i.key)? == InstrumentStatus::NotUsed {
            out.insert(i.key);
        }
    }
    Ok(out)
}

fn name_of(key: &str) -> String {
    dv_domain::instruments()
        .into_iter()
        .find(|i| i.key == key)
        .map_or_else(|| key.to_owned(), |i| i.name)
}

/// The sheet behind a scores material entered in the score table. `Ok(None)`: another
/// material, or scores typed as text. `Err(())`: a sheet that cannot be read.
fn sheet_of(
    v: &Vault,
    case_id: &str,
    input: &CaseInput,
) -> Result<Result<Option<ScoreSheet>, ()>, CoreError> {
    if input.kind != InputKind::TestScores {
        return Ok(Ok(None));
    }
    let Some(data) = v.input_data(case_id, &input.id)? else {
        return Ok(Ok(None));
    };
    Ok(serde_json::from_str(&data).map(Some).map_err(|_| ()))
}

/// Why a material goes into no request, or `None`: a score table of an instrument she does not
/// give, or one that cannot be read (in doubt, it stays in the computer).
pub(crate) fn left_out(
    v: &Vault,
    case_id: &str,
    input: &CaseInput,
    not_used: &HashSet<String>,
) -> Result<Option<String>, CoreError> {
    Ok(match sheet_of(v, case_id, input)? {
        Ok(Some(sheet)) if not_used.contains(&sheet.instrument) => Some(format!(
            "סימנת בהגדרות שאת לא מעבירה את {}. הציונים נשארים בתיק כמו שנשמרו, אבל לא נשלחים ולא נכנסים לדוח.",
            name_of(&sheet.instrument)
        )),
        Ok(_) => None,
        Err(()) => Some("טבלת הציונים הזו לא נקראה, ולכן היא לא נשלחת.".to_owned()),
    })
}

/// The case's materials that may go into a request: all but the left-out score tables.
pub(crate) fn request_inputs(v: &Vault, case_id: &str) -> Result<Vec<CaseInput>, CoreError> {
    let skip = not_used(v)?;
    let mut out = Vec::new();
    for input in v.inputs(case_id)? {
        if left_out(v, case_id, &input, &skip)?.is_none() {
            out.push(input);
        }
    }
    Ok(out)
}

/// The score tables in open cases: for each instrument, the cases with a table of it, and
/// whether some table could not be read. An unreadable one does not hide the others; the
/// materials list says which it is ("טבלת הציונים הזו לא נקראה").
struct OpenTables {
    by_instrument: HashMap<String, HashSet<String>>,
    unreadable: bool,
}

fn in_open_cases(v: &Vault) -> Result<OpenTables, CoreError> {
    let mut out = OpenTables {
        by_instrument: HashMap::new(),
        unreadable: false,
    };
    for case in v
        .list_cases()?
        .into_iter()
        .filter(|c| c.deleted_at.is_none())
    {
        for input in v
            .inputs(&case.id)?
            .into_iter()
            .filter(|i| i.kind == InputKind::TestScores)
        {
            match sheet_of(v, &case.id, &input)? {
                Ok(Some(sheet)) => {
                    out.by_instrument
                        .entry(sheet.instrument)
                        .or_default()
                        .insert(case.id.clone());
                }
                Ok(None) => {}
                Err(()) => out.unreadable = true,
            }
        }
    }
    Ok(out)
}

/// "טבלאות המבחנים אושרו": she has answered for every instrument of the fixed tables ("מאושר"
/// or "לא מעבירה את זה"), and every score table in an open case can be read. Fail closed: a
/// new vault, with nothing answered, shows it as not done, and it does not change as cases come
/// and go.
pub(crate) fn tables_approved(v: &Vault) -> Result<bool, CoreError> {
    if in_open_cases(v)?.unreadable {
        return Ok(false);
    }
    for i in dv_domain::instruments() {
        if status_of(v, &i.key)? == InstrumentStatus::Awaiting {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Refuses a score table of an instrument she does not give.
pub(crate) fn check_given(v: &Vault, key: &str) -> Result<(), CoreError> {
    if status_of(v, key)? == InstrumentStatus::NotUsed {
        return Err(CoreError::Refused(format!(
            "סימנת בהגדרות שאת לא מעבירה את {}. אפשר לשנות את זה בהגדרות ← המבחנים.",
            name_of(key)
        )));
    }
    Ok(())
}

impl Core {
    /// Every instrument with her answer, for Settings → המבחנים.
    pub fn instrument_approvals(&mut self) -> Result<Vec<InstrumentApproval>, CoreError> {
        let v = self.vault_ref()?;
        let used = in_open_cases(v)?.by_instrument;
        dv_domain::instruments()
            .into_iter()
            .map(|instrument| {
                let status = status_of(v, &instrument.key)?;
                let open_cases = used
                    .get(&instrument.key)
                    .map_or(0, |c| u32::try_from(c.len()).unwrap_or(u32::MAX));
                Ok(InstrumentApproval {
                    instrument,
                    status,
                    open_cases,
                })
            })
            .collect()
    }

    /// The instruments the score table offers: all but the ones she does not give, each with
    /// her answer (one awaiting approval is marked there).
    pub fn score_instruments(&mut self) -> Result<Vec<InstrumentApproval>, CoreError> {
        Ok(self
            .instrument_approvals()?
            .into_iter()
            .filter(|a| a.status != InstrumentStatus::NotUsed)
            .collect())
    }

    /// Her answer for one instrument: "מאושר" or "לא מעבירה את זה" (logged as a change of
    /// settings). "Awaiting" is where every instrument starts, not an answer.
    pub fn set_instrument_status(
        &mut self,
        key: &str,
        status: InstrumentStatus,
    ) -> Result<Vec<InstrumentApproval>, CoreError> {
        if !dv_domain::instruments().iter().any(|i| i.key == key) {
            return Err(CoreError::NotFound("המבחן".to_owned()));
        }
        let Some(value) = status.stored() else {
            return Err(CoreError::Refused(
                "אפשר לבחור \"מאושר\" או \"לא מעבירה את זה\".".to_owned(),
            ));
        };
        let setting = format!("{PREFIX}{key}");
        let v = self.vault_mut()?;
        v.set_state(&setting, value)?;
        // Logged with her answer, so the activity log can say which test and what she chose.
        v.record(
            dv_vault::AuditEvent::SettingsChanged,
            None,
            &serde_json::json!({ "key": setting, "value": value }),
        )?;
        // The comparison with an earlier assessment is text: written again without its scores.
        self.refresh_comparisons()?;
        // A request prepared before the answer may hold its scores: prepared again.
        self.drop_pending();
        self.instrument_approvals()
    }
}
