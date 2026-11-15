//! The clinic's service (beta, D-170): one door, [`Core::clinic`], for everything the clinic
//! screens ask. It loads the sealed records, applies the rules of `dv_domain::clinic` and
//! saves; it also keeps the link between a patient card and a diagnostic case.
//!
//! Nothing here leaves the computer and nothing here talks to a model: this file knows the
//! vault and the rules only. The one way clinic text reaches a case is `NoteToCase`, which
//! hands the formal record of a session (never the personal note) to the core's own
//! `add_input` / `update_input`, exactly as a material she adds by hand; from there the
//! case's usual filter applies. A test reads this file and fails if a name of the network or
//! model crates appears in it.
//!
//! The records are the domain types as JSON. A record's id is the vault's: it is set on
//! every read, so the id inside the JSON never matters.

use std::collections::{HashMap, HashSet};

use dv_domain::clinic::{
    self as rules, CaseChoice, CaseLink, Charge, ClinicReply, ClinicRequest, ClinicSettings,
    ClinicTask, ContactRole, HistoryEntry, MessageKind, MessageValues, Overview, Patient,
    PatientCard, PatientRecords, PatientRow, PatientStatus, Payment, Repeat, SavedVisits, Scope,
    SessionNote, Today, Visit, VisitKind, VisitStatus, VisitView,
};
use dv_domain::{CaseMeta, DomainError, IdentityInput, InputKind, Role};
use dv_vault::{AuditEvent, StoredClinicRecord, Vault};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::dates::{date_of, iso, today};
use crate::retention::{KEEP_UNTIL_AGE, KEEP_YEARS_AFTER_LAST_CHANGE};
use crate::{Core, CoreError};

const SETTINGS: &str = "settings";
/// The one settings record has a fixed id.
const SETTINGS_ID: &str = "settings";
const CREATED: &str = "created";
const CHANGED: &str = "changed";
const VOIDED: &str = "voided";

const NO_PATIENT: &str = "המטופל";
const NO_CASE: &str = "התיק";

// ------------------------------------------------------------------ small helpers

/// A rule's refusal is already a Hebrew sentence she can act on.
fn refusal(error: DomainError) -> CoreError {
    match error {
        DomainError::Invalid(text) => CoreError::Refused(text),
        other => CoreError::Internal(other.to_string()),
    }
}

fn refuse<T>(text: &str) -> Result<T, CoreError> {
    Err(CoreError::Refused(text.to_owned()))
}

/// The parser's own message may quote a value of the record, so it is never passed on:
/// an error's details can be copied into a problem report.
fn to_json<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value)
        .map_err(|_| CoreError::Internal("a clinic record could not be written".to_owned()))
}

fn from_json<T: DeserializeOwned>(json: &str) -> Result<T, CoreError> {
    serde_json::from_str(json)
        .map_err(|_| CoreError::Internal("a clinic record could not be read".to_owned()))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A clinic record as the vault keeps it: its kind, and what the vault indexes it by.
trait Kept: Serialize + DeserializeOwned + PartialEq {
    const KIND: &'static str;
    /// What she is told was not found.
    const NAME: &'static str;
    fn id(&self) -> &str;
    fn set_id(&mut self, id: String);
    fn patient(&self) -> Option<&str>;
    /// The record's date; `None`: it has none.
    fn date(&self) -> Option<&str>;
}

impl Kept for Visit {
    const KIND: &'static str = "visit";
    const NAME: &'static str = "הפגישה";
    fn id(&self) -> &str {
        &self.id
    }
    fn set_id(&mut self, id: String) {
        self.id = id;
    }
    fn patient(&self) -> Option<&str> {
        self.patient_id.as_deref()
    }
    fn date(&self) -> Option<&str> {
        Some(&self.date)
    }
}

impl Kept for SessionNote {
    const KIND: &'static str = "note";
    const NAME: &'static str = "הסיכום";
    fn id(&self) -> &str {
        &self.id
    }
    fn set_id(&mut self, id: String) {
        self.id = id;
    }
    fn patient(&self) -> Option<&str> {
        Some(&self.patient_id)
    }
    fn date(&self) -> Option<&str> {
        Some(&self.date)
    }
}

impl Kept for Payment {
    const KIND: &'static str = "payment";
    const NAME: &'static str = "התקבול";
    fn id(&self) -> &str {
        &self.id
    }
    fn set_id(&mut self, id: String) {
        self.id = id;
    }
    fn patient(&self) -> Option<&str> {
        Some(&self.patient_id)
    }
    fn date(&self) -> Option<&str> {
        Some(&self.date)
    }
}

impl Kept for Charge {
    const KIND: &'static str = "charge";
    const NAME: &'static str = "החיוב";
    fn id(&self) -> &str {
        &self.id
    }
    fn set_id(&mut self, id: String) {
        self.id = id;
    }
    fn patient(&self) -> Option<&str> {
        Some(&self.patient_id)
    }
    fn date(&self) -> Option<&str> {
        Some(&self.date)
    }
}

impl Kept for ClinicTask {
    const KIND: &'static str = "task";
    const NAME: &'static str = "המשימה";
    fn id(&self) -> &str {
        &self.id
    }
    fn set_id(&mut self, id: String) {
        self.id = id;
    }
    fn patient(&self) -> Option<&str> {
        self.patient_id.as_deref()
    }
    fn date(&self) -> Option<&str> {
        self.due.as_deref()
    }
}

fn day_of<T: Kept>(record: &T) -> i64 {
    record.date().and_then(rules::day_number).unwrap_or(0)
}

fn open<T: Kept>(row: StoredClinicRecord) -> Result<T, CoreError> {
    let mut record: T = from_json(&row.json)?;
    record.set_id(row.id);
    Ok(record)
}

fn open_all<T: Kept>(rows: Vec<StoredClinicRecord>) -> Result<Vec<T>, CoreError> {
    rows.into_iter().map(open).collect()
}

fn all<T: Kept>(v: &Vault) -> Result<Vec<T>, CoreError> {
    open_all(v.clinic_kind(T::KIND)?)
}

/// The records of a kind dated from `from` to `to`, both included.
fn between<T: Kept>(v: &Vault, from: &str, to: &str) -> Result<Vec<T>, CoreError> {
    match (rules::day_number(from), rules::day_number(to)) {
        (Some(first), Some(last)) => open_all(v.clinic_range(T::KIND, first, last)?),
        _ => Ok(Vec::new()),
    }
}

/// The record with this id, when it is one of this kind.
fn find<T: Kept>(v: &Vault, id: &str) -> Result<T, CoreError> {
    match v.clinic_get(id)? {
        Some(row) if row.kind == T::KIND => open(row),
        _ => Err(CoreError::NotFound(T::NAME.to_owned())),
    }
}

/// Writes one record, new when its id is empty, and gives it the id it is kept under.
fn write<T: Kept>(v: &mut Vault, record: &mut T, action: Option<&str>) -> Result<(), CoreError> {
    let json = to_json(record)?;
    let id = (!record.id().is_empty()).then(|| record.id().to_owned());
    let kept = v.clinic_put(
        T::KIND,
        id.as_deref(),
        record.patient(),
        day_of(record),
        &json,
        action,
    )?;
    record.set_id(kept);
    Ok(())
}

fn open_patient(id: String, json: &str) -> Result<Patient, CoreError> {
    let mut patient: Patient = from_json(json)?;
    patient.id = id;
    Ok(patient)
}

fn patients(v: &Vault) -> Result<Vec<Patient>, CoreError> {
    v.clinic_patients()?
        .into_iter()
        .map(|(id, json)| open_patient(id, &json))
        .collect()
}

fn patient(v: &Vault, id: &str) -> Result<Patient, CoreError> {
    match v.clinic_patient(id)? {
        Some(json) => open_patient(id.to_owned(), &json),
        None => Err(CoreError::NotFound(NO_PATIENT.to_owned())),
    }
}

fn write_patient(v: &mut Vault, patient: &mut Patient) -> Result<(), CoreError> {
    let json = to_json(patient)?;
    let id = (!patient.id.is_empty()).then(|| patient.id.clone());
    patient.id = v.save_clinic_patient(id.as_deref(), &json)?;
    Ok(())
}

/// A record names a patient: the card has to be there.
fn patient_exists(v: &Vault, id: &str) -> Result<(), CoreError> {
    if v.clinic_patient(id)?.is_none() {
        return refuse("המטופל שנבחר לא נמצא. ייתכן שהכרטיס נמחק.");
    }
    Ok(())
}

fn settings(v: &Vault) -> Result<ClinicSettings, CoreError> {
    match v.clinic_get(SETTINGS_ID)? {
        Some(row) if row.kind == SETTINGS => from_json(&row.json),
        _ => Ok(ClinicSettings::default()),
    }
}

fn names_of(patients: &[Patient]) -> HashMap<String, String> {
    patients
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect()
}

fn noted_of(notes: &[SessionNote]) -> HashSet<String> {
    notes.iter().filter_map(|n| n.visit_id.clone()).collect()
}

fn check_today(today: &Today) -> Result<(), CoreError> {
    rules::validate_today(today).map_err(refusal)
}

// ------------------------------------------------------------------ the clinic, loaded once

/// Everything the clinic holds, read once for a request and grouped in memory.
struct Book {
    patients: Vec<Patient>,
    visits: Vec<Visit>,
    notes: Vec<SessionNote>,
    charges: Vec<Charge>,
    payments: Vec<Payment>,
    tasks: Vec<ClinicTask>,
    settings: ClinicSettings,
}

/// One patient's records, picked out of the book.
#[derive(Default)]
struct Mine {
    visits: Vec<Visit>,
    notes: Vec<SessionNote>,
    charges: Vec<Charge>,
    payments: Vec<Payment>,
    tasks: Vec<ClinicTask>,
}

impl Book {
    fn load(v: &Vault) -> Result<Self, CoreError> {
        Ok(Self {
            patients: patients(v)?,
            visits: all(v)?,
            notes: all(v)?,
            charges: all(v)?,
            payments: all(v)?,
            tasks: all(v)?,
            settings: settings(v)?,
        })
    }

    /// Each patient's records. A record of a card that is not there is left out.
    fn by_patient(&self) -> HashMap<&str, Mine> {
        let mut map: HashMap<&str, Mine> = self
            .patients
            .iter()
            .map(|p| (p.id.as_str(), Mine::default()))
            .collect();
        for visit in &self.visits {
            if let Some(mine) = visit.patient_id.as_deref().and_then(|id| map.get_mut(id)) {
                mine.visits.push(visit.clone());
            }
        }
        for note in &self.notes {
            if let Some(mine) = map.get_mut(note.patient_id.as_str()) {
                mine.notes.push(note.clone());
            }
        }
        for charge in &self.charges {
            if let Some(mine) = map.get_mut(charge.patient_id.as_str()) {
                mine.charges.push(charge.clone());
            }
        }
        for payment in &self.payments {
            if let Some(mine) = map.get_mut(payment.patient_id.as_str()) {
                mine.payments.push(payment.clone());
            }
        }
        for task in &self.tasks {
            if let Some(mine) = task.patient_id.as_deref().and_then(|id| map.get_mut(id)) {
                mine.tasks.push(task.clone());
            }
        }
        map
    }

    /// The visits of `shown` as the calendar shows them; an overlap is looked for among
    /// every visit of the clinic, so one outside `shown` counts too.
    fn views(&self, shown: &[Visit]) -> Vec<VisitView> {
        rules::visit_views(
            shown,
            &names_of(&self.patients),
            &noted_of(&self.notes),
            &rules::overlapping_ids(&self.visits),
        )
    }

    fn views_on(&self, date: &str) -> Vec<VisitView> {
        let shown: Vec<Visit> = self
            .visits
            .iter()
            .filter(|v| v.date == date)
            .cloned()
            .collect();
        self.views(&shown)
    }
}

// ------------------------------------------------------------------ the link to cases

/// A diagnostic case as the clinic needs it.
struct CaseInfo {
    code: String,
    approved: u32,
    total: u32,
    in_trash: bool,
}

impl CaseInfo {
    fn link(&self, id: &str) -> CaseLink {
        CaseLink {
            id: id.to_owned(),
            code: self.code.clone(),
            approved: self.approved,
            total: self.total,
            in_trash: self.in_trash,
        }
    }
}

/// The case a card points to. A case that was purged leaves an id that points nowhere:
/// that reads as "no case", never as an error.
fn case_of<'a>(
    patient: &Patient,
    cases: &'a HashMap<String, CaseInfo>,
) -> Option<(&'a str, &'a CaseInfo)> {
    let id = patient.case_id.as_deref()?;
    cases
        .get_key_value(id)
        .map(|(id, case)| (id.as_str(), case))
}

/// The report's sections (approved, total) of each patient that has a case.
fn reports_of(
    patients: &[Patient],
    cases: &HashMap<String, CaseInfo>,
) -> HashMap<String, (u32, u32)> {
    patients
        .iter()
        .filter_map(|p| case_of(p, cases).map(|(_, c)| (p.id.clone(), (c.approved, c.total))))
        .collect()
}

fn contact_role(role: ContactRole) -> Role {
    match role {
        ContactRole::Mother => Role::Mother,
        ContactRole::Father => Role::Father,
        // A guardian is not always a relative: the neutral role says nothing that is not known.
        ContactRole::Guardian | ContactRole::Other => Role::Other,
    }
}

/// Which kind of record a history row holds, when the record itself is gone. Every state is
/// written with all the fields of its type, so one field that only that type has tells.
fn kind_of_state(json: &str) -> &'static str {
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(json) else {
        return Visit::KIND;
    };
    if fields.contains_key("record") {
        SessionNote::KIND
    } else if fields.contains_key("method") {
        Payment::KIND
    } else if fields.contains_key("what") {
        Charge::KIND
    } else {
        Visit::KIND
    }
}

impl Core {
    /// Everything the clinic screens ask (D-170). Locked, idle and "not found" behave as in
    /// every other command; a rule's refusal comes back as its Hebrew sentence.
    pub fn clinic(&mut self, request: ClinicRequest) -> Result<ClinicReply, CoreError> {
        // Locked is said before anything about the request itself.
        self.vault_ref()?;
        match request {
            ClinicRequest::Overview { today } => self.clinic_overview(&today),
            ClinicRequest::Calendar { from, to } => self.clinic_calendar(&from, &to),
            ClinicRequest::SaveVisit {
                visit,
                repeat,
                scope,
            } => self.clinic_save_visit(visit, repeat, scope),
            ClinicRequest::DeleteVisit { id, scope } => self.clinic_delete_visit(&id, scope),
            ClinicRequest::Patients { today } => self.clinic_patients(&today),
            ClinicRequest::PatientCard { id, today } => self.clinic_card(&id, &today),
            ClinicRequest::SavePatient { patient } => self.clinic_save_patient(patient),
            ClinicRequest::ErasePatient { id, name } => self.clinic_erase_patient(&id, &name),
            ClinicRequest::SaveNote { note } => self.clinic_save_note(note),
            ClinicRequest::DeleteNote { id } => self.clinic_delete_note(&id),
            ClinicRequest::NoteToCase { id } => self.clinic_note_to_case(&id),
            ClinicRequest::SavePayment { payment } => self.clinic_save_payment(payment),
            ClinicRequest::VoidPayment { id, reason } => self.clinic_void_payment(&id, &reason),
            ClinicRequest::SaveCharge { charge } => self.clinic_save_charge(charge),
            ClinicRequest::VoidCharge { id, reason } => self.clinic_void_charge(&id, &reason),
            ClinicRequest::Tasks { today } => self.clinic_tasks(&today),
            ClinicRequest::SaveTask { task } => self.clinic_save_task(task),
            ClinicRequest::DeleteTask { id } => self.clinic_delete_task(&id),
            ClinicRequest::Diary { month } => self.clinic_diary(&month),
            ClinicRequest::Income { year } => self.clinic_income(year),
            ClinicRequest::History { record_id } => self.clinic_history(&record_id),
            ClinicRequest::Settings => Ok(ClinicReply::Settings(settings(self.vault_ref()?)?)),
            ClinicRequest::SaveSettings { settings } => self.clinic_save_settings(settings),
            ClinicRequest::Message {
                kind,
                patient_id,
                visit_id,
            } => self.clinic_message(kind, &patient_id, visit_id.as_deref()),
            ClinicRequest::CaseChoices => self.clinic_case_choices(),
            ClinicRequest::LinkCase {
                patient_id,
                case_id,
            } => self.clinic_link_case(&patient_id, case_id),
            ClinicRequest::CreateCase { patient_id, today } => {
                self.clinic_create_case(&patient_id, &today)
            }
        }
    }

    /// Every case, at work or in the recycle bin, by id. Read once for a request.
    fn clinic_cases(&mut self) -> Result<HashMap<String, CaseInfo>, CoreError> {
        let live = self.list_cases()?;
        let trashed = self.vault_ref()?.list_trash()?;
        // The same count of sections as the case list shows.
        let trashed = self.with_failures(trashed)?;
        Ok(live
            .into_iter()
            .chain(trashed)
            .map(|case| {
                let approved = case
                    .approved_sections
                    .iter()
                    .filter(|key| key.as_str() != "signature")
                    .count();
                let info = CaseInfo {
                    code: case.meta.code,
                    approved: count(approved),
                    total: case.total_sections,
                    in_trash: case.deleted_at.is_some(),
                };
                (case.id, info)
            })
            .collect())
    }

    // -------------------------------------------------------------- reading

    fn clinic_overview(&mut self, today: &Today) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        let cases = self.clinic_cases()?;
        let book = Book::load(self.vault_ref()?)?;
        let names = names_of(&book.patients);
        let reports = reports_of(&book.patients, &cases);
        let records = rules::ClinicRecords {
            patients: &book.patients,
            visits: &book.visits,
            notes: &book.notes,
            charges: &book.charges,
            payments: &book.payments,
            tasks: &book.tasks,
            reports: &reports,
        };
        let alerts = rules::clinic_alerts(&records, today, &book.settings);
        let balances = book
            .by_patient()
            .values()
            .map(|mine| rules::balance(&mine.visits, &mine.charges, &mine.payments))
            .collect::<Result<Vec<_>, _>>()
            .map_err(refusal)?;
        let open: Vec<ClinicTask> = book
            .tasks
            .iter()
            .filter(|t| t.done_on.is_none())
            .cloned()
            .collect();
        let next_day = rules::next_visit_day(&book.visits, today);
        let status = |status: PatientStatus| {
            count(book.patients.iter().filter(|p| p.status == status).count())
        };
        Ok(ClinicReply::Overview(Overview {
            date: today.date.clone(),
            visits: book.views_on(&today.date),
            next_visits: next_day
                .as_deref()
                .map(|day| book.views_on(day))
                .unwrap_or_default(),
            next_day,
            tasks: rules::task_views(&open, &names, &today.date),
            alerts,
            week_visits: rules::week_visit_count(&book.visits, &today.date),
            month_received: rules::month_received(&book.payments, &today.date).map_err(refusal)?,
            total_due: rules::total_due(&balances).map_err(refusal)?,
            waiting: status(PatientStatus::Waiting),
            active: status(PatientStatus::Active),
        }))
    }

    fn clinic_calendar(&mut self, from: &str, to: &str) -> Result<ClinicReply, CoreError> {
        rules::validate_range(from, to).map_err(refusal)?;
        let v = self.vault_ref()?;
        // Every visit of the stretch is shown, so an overlap is found among them.
        let visits: Vec<Visit> = between(v, from, to)?;
        let notes: Vec<SessionNote> = all(v)?;
        Ok(ClinicReply::Visits(rules::visit_views(
            &visits,
            &names_of(&patients(v)?),
            &noted_of(&notes),
            &rules::overlapping_ids(&visits),
        )))
    }

    fn clinic_patients(&mut self, today: &Today) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        let book = Book::load(self.vault_ref()?)?;
        let by_patient = book.by_patient();
        let empty = Mine::default();
        let mut rows = book
            .patients
            .iter()
            .map(|patient| {
                let mine = by_patient.get(patient.id.as_str()).unwrap_or(&empty);
                let balance =
                    rules::balance(&mine.visits, &mine.charges, &mine.payments).map_err(refusal)?;
                Ok(PatientRow {
                    patient: patient.clone(),
                    age: rules::age_on(patient.birth_date.as_deref(), &today.date),
                    next_visit: rules::next_visit(&mine.visits, &today.date),
                    last_visit: rules::last_held(&mine.visits),
                    due: balance.due,
                    open_tasks: rules::open_task_count(&mine.tasks),
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        rows.sort_by(|a, b| {
            (&a.patient.name, &a.patient.id).cmp(&(&b.patient.name, &b.patient.id))
        });
        Ok(ClinicReply::Patients(rows))
    }

    fn clinic_card(&mut self, id: &str, today: &Today) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        let cases = self.clinic_cases()?;
        let book = Book::load(self.vault_ref()?)?;
        let Some(patient) = book.patients.iter().find(|p| p.id == id) else {
            return Err(CoreError::NotFound(NO_PATIENT.to_owned()));
        };
        let mut mine = book.by_patient().remove(id).unwrap_or_default();
        let case = case_of(patient, &cases);
        let report = case.map(|(_, c)| (c.approved, c.total));
        let records = PatientRecords {
            patient,
            visits: &mine.visits,
            notes: &mine.notes,
            charges: &mine.charges,
            payments: &mine.payments,
            tasks: &mine.tasks,
            report,
        };
        let alerts = rules::patient_alerts(&records, today, &book.settings);
        let balance =
            rules::balance(&mine.visits, &mine.charges, &mine.payments).map_err(refusal)?;
        let process = rules::process(&mine.visits, &mine.charges, &mine.payments, report);
        let mut visits = book.views(&mine.visits);
        visits.reverse();
        // Newest first; the id only keeps the order the same on every read.
        mine.notes
            .sort_by(|a, b| (&b.date, &b.id).cmp(&(&a.date, &a.id)));
        mine.payments
            .sort_by(|a, b| (&b.date, &b.id).cmp(&(&a.date, &a.id)));
        mine.charges
            .sort_by(|a, b| (&b.date, &b.id).cmp(&(&a.date, &a.id)));
        let tasks = rules::task_views(&mine.tasks, &HashMap::new(), &today.date)
            .into_iter()
            .map(|view| view.task)
            .collect();
        Ok(ClinicReply::Card(Box::new(PatientCard {
            patient: patient.clone(),
            age: rules::age_on(patient.birth_date.as_deref(), &today.date),
            visits,
            notes: mine.notes,
            payments: mine.payments,
            charges: mine.charges,
            tasks,
            balance,
            process,
            case: case.map(|(id, c)| c.link(id)),
            alerts,
        })))
    }

    fn clinic_tasks(&mut self, today: &Today) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        let v = self.vault_ref()?;
        let tasks: Vec<ClinicTask> = all(v)?;
        Ok(ClinicReply::Tasks(rules::task_views(
            &tasks,
            &names_of(&patients(v)?),
            &today.date,
        )))
    }

    fn clinic_diary(&mut self, month: &str) -> Result<ClinicReply, CoreError> {
        rules::validate_month(month).map_err(refusal)?;
        let Some((first, last)) = rules::month_bounds(month) else {
            return refuse("צריך לבחור חודש.");
        };
        let v = self.vault_ref()?;
        let visits: Vec<Visit> = between(v, &first, &last)?;
        let charges: Vec<Charge> = between(v, &first, &last)?;
        let payments: Vec<Payment> = between(v, &first, &last)?;
        // Changed after it was first written: its history has more than one entry.
        let changed: HashSet<String> = v
            .clinic_history_counts()?
            .into_iter()
            .filter(|(_, entries)| *entries > 1)
            .map(|(id, _)| id)
            .collect();
        let diary = rules::diary_month(
            month,
            &visits,
            &charges,
            &payments,
            &names_of(&patients(v)?),
            &changed,
        )
        .map_err(refusal)?;
        Ok(ClinicReply::Diary(diary))
    }

    fn clinic_income(&mut self, year: u16) -> Result<ClinicReply, CoreError> {
        rules::validate_year(year).map_err(refusal)?;
        let (first, last) = (format!("{year:04}-01-01"), format!("{year:04}-12-31"));
        let v = self.vault_ref()?;
        let visits: Vec<Visit> = between(v, &first, &last)?;
        let charges: Vec<Charge> = between(v, &first, &last)?;
        let payments: Vec<Payment> = between(v, &first, &last)?;
        let income = rules::income_year(year, &visits, &charges, &payments, &patients(v)?)
            .map_err(refusal)?;
        Ok(ClinicReply::Income(income))
    }

    /// Every saved state of a visit, a note, a charge or a payment, oldest first, in her
    /// words. A visit that was removed still has its history.
    fn clinic_history(&mut self, record_id: &str) -> Result<ClinicReply, CoreError> {
        let v = self.vault_ref()?;
        let states = v.clinic_history(record_id)?;
        let kept = v.clinic_get(record_id)?.map(|row| row.kind);
        if kept.is_none() && states.is_empty() {
            return Err(CoreError::NotFound("הרישום".to_owned()));
        }
        let names = names_of(&patients(v)?);
        let name = |id: &str| names.get(id).map(String::as_str).unwrap_or_default();
        let mut before: Option<SessionNote> = None;
        let mut entries = Vec::with_capacity(states.len());
        for state in states {
            let kind = kept
                .as_deref()
                .unwrap_or_else(|| kind_of_state(&state.json));
            let lines = match kind {
                Visit::KIND => {
                    let visit: Visit = from_json(&state.json)?;
                    let patient = visit.patient_id.as_deref().map(name);
                    rules::visit_history_lines(&visit, patient)
                }
                SessionNote::KIND => {
                    let note: SessionNote = from_json(&state.json)?;
                    let lines =
                        rules::note_history_lines(&note, name(&note.patient_id), before.as_ref());
                    before = Some(note);
                    lines
                }
                Payment::KIND => {
                    let payment: Payment = from_json(&state.json)?;
                    rules::payment_history_lines(&payment, name(&payment.patient_id))
                }
                Charge::KIND => {
                    let charge: Charge = from_json(&state.json)?;
                    rules::charge_history_lines(&charge, name(&charge.patient_id))
                }
                _ => Vec::new(),
            };
            entries.push(HistoryEntry {
                at: state.saved_at,
                action: state.action,
                lines,
            });
        }
        Ok(ClinicReply::History(entries))
    }

    /// The text of a message to the parents, filled in, for her to copy. Nothing is sent.
    fn clinic_message(
        &mut self,
        kind: MessageKind,
        patient_id: &str,
        visit_id: Option<&str>,
    ) -> Result<ClinicReply, CoreError> {
        let v = self.vault_ref()?;
        let patient = patient(v, patient_id)?;
        let visit: Option<Visit> = visit_id.map(|id| find(v, id)).transpose()?;
        if visit
            .as_ref()
            .is_some_and(|visit| visit.patient_id.as_deref() != Some(patient_id))
        {
            return refuse("הפגישה שייכת למטופל אחר.");
        }
        let mine: Vec<StoredClinicRecord> = v.clinic_of_patient(patient_id)?;
        let of_kind = |kind: &str| -> Vec<StoredClinicRecord> {
            mine.iter()
                .filter(|row| row.kind == kind)
                .cloned()
                .collect()
        };
        let visits: Vec<Visit> = open_all(of_kind(Visit::KIND))?;
        let charges: Vec<Charge> = open_all(of_kind(Charge::KIND))?;
        let payments: Vec<Payment> = open_all(of_kind(Payment::KIND))?;
        let due = rules::balance(&visits, &charges, &payments)
            .map_err(refusal)?
            .due;
        let settings = settings(v)?;
        let template = match kind {
            MessageKind::Reminder => {
                if visit.is_none() {
                    return refuse("כדי להכין תזכורת צריך לבחור פגישה.");
                }
                settings.reminder_text
            }
            MessageKind::Payment => {
                if due <= 0 {
                    return refuse("אין יתרה לתשלום, ולכן אין מה לשלוח.");
                }
                settings.payment_text
            }
        };
        let values = MessageValues {
            parent: rules::message_parent(&patient),
            child: patient.name.clone(),
            date: visit.as_ref().map(|visit| visit.date.clone()),
            start: visit.as_ref().map(|visit| visit.start),
            place: visit.map(|visit| visit.place).unwrap_or_default(),
            // What the family still owes; a family that paid ahead owes nothing.
            amount: Some(due.max(0)),
            me: v
                .practitioner()?
                .names
                .into_iter()
                .next()
                .unwrap_or_default(),
        };
        Ok(ClinicReply::Text(rules::fill_message(&template, &values)))
    }

    // -------------------------------------------------------------- visits

    fn clinic_save_visit(
        &mut self,
        visit: Visit,
        repeat: Option<Repeat>,
        scope: Scope,
    ) -> Result<ClinicReply, CoreError> {
        let mut visit = rules::normalize_visit(visit);
        let v = self.vault_ref()?;
        // What is written: nothing when the save changes nothing.
        let rows: Vec<Visit>;
        let action;
        if visit.id.is_empty() {
            // A new visit is planned, whatever was sent, and joins no series by itself.
            visit.status = VisitStatus::Planned;
            visit.series_id = None;
            rules::validate_visit(&visit).map_err(refusal)?;
            if let Some(patient_id) = &visit.patient_id {
                patient_exists(v, patient_id)?;
            }
            action = CREATED;
            rows = match repeat {
                Some(repeat) => {
                    let dates = rules::series_dates(&visit.date, repeat).map_err(refusal)?;
                    let series = dv_vault::crypto::random_id()?;
                    dates
                        .into_iter()
                        .map(|date| Visit {
                            date,
                            series_id: Some(series.clone()),
                            ..visit.clone()
                        })
                        .collect()
                }
                None => vec![visit.clone()],
            };
        } else {
            if repeat.is_some() {
                return refuse("אפשר לקבוע סדרה חוזרת רק כשקובעים פגישה חדשה.");
            }
            let stored: Visit = find(v, &visit.id)?;
            // Which series a visit belongs to is set when the series is made.
            visit.series_id.clone_from(&stored.series_id);
            rules::validate_visit(&visit).map_err(refusal)?;
            if let Some(patient_id) = &visit.patient_id {
                patient_exists(v, patient_id)?;
            }
            if visit.patient_id != stored.patient_id {
                let notes: Vec<SessionNote> = all(v)?;
                if noted_of(&notes).contains(&visit.id) {
                    return refuse("לפגישה הזו יש סיכום, ולכן אי אפשר להעביר אותה למטופל אחר.");
                }
            }
            action = CHANGED;
            let mut changed = Vec::new();
            if visit != stored {
                changed.push(visit.clone());
            }
            if scope == Scope::Following && stored.series_id.is_some() {
                let everything: Vec<Visit> = all(v)?;
                let later =
                    rules::apply_following(&visit, &stored, &everything).map_err(refusal)?;
                changed.extend(
                    later
                        .into_iter()
                        .filter(|new| everything.iter().find(|old| old.id == new.id) != Some(new)),
                );
            }
            rows = changed;
        }
        for row in &rows {
            rules::validate_visit(row).map_err(refusal)?;
        }
        let to_put = rows
            .iter()
            .map(|row| {
                Ok((
                    (!row.id.is_empty()).then(|| row.id.clone()),
                    row.patient_id.clone(),
                    day_of(row),
                    to_json(row)?,
                ))
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        // One transaction: the whole series, or this visit and the later ones, or nothing.
        let mut ids = self
            .vault_mut()?
            .clinic_put_many(Visit::KIND, &to_put, Some(action))?;
        if ids.is_empty() {
            ids.push(visit.id.clone());
        }
        // A family on the waiting list that gets a visit is in the process now.
        if let Some(patient_id) = &visit.patient_id {
            let v = self.vault_mut()?;
            if let Some(mut active) = rules::activated(&patient(v, patient_id)?) {
                write_patient(v, &mut active)?;
            }
        }
        let book = Book::load(self.vault_ref()?)?;
        let written: Vec<Visit> = book
            .visits
            .iter()
            .filter(|v| ids.contains(&v.id))
            .cloned()
            .collect();
        let visits = book.views(&written);
        let overlapping = count(visits.iter().filter(|view| view.overlaps).count());
        Ok(ClinicReply::Saved(SavedVisits {
            visits,
            overlapping,
        }))
    }

    fn clinic_delete_visit(&mut self, id: &str, scope: Scope) -> Result<ClinicReply, CoreError> {
        let v = self.vault_ref()?;
        let visit: Visit = find(v, id)?;
        rules::validate_delete(&visit).map_err(refusal)?;
        let notes: Vec<SessionNote> = all(v)?;
        let noted = noted_of(&notes);
        if noted.contains(id) {
            return refuse("לפגישה הזו יש סיכום, ולכן אי אפשר למחוק אותה. אפשר לסמן אותה כמבוטלת.");
        }
        let mut ids = vec![visit.id.clone()];
        if let (Scope::Following, Some(series)) = (scope, visit.series_id.as_deref()) {
            let everything: Vec<Visit> = all(v)?;
            // The later ones of the series that are still planned; one that already has a
            // record written for it stays.
            ids.extend(
                everything
                    .into_iter()
                    .filter(|other| {
                        other.series_id.as_deref() == Some(series)
                            && other.id != visit.id
                            && other.status == VisitStatus::Planned
                            && (other.date.as_str(), other.start)
                                > (visit.date.as_str(), visit.start)
                            && !noted.contains(&other.id)
                    })
                    .map(|other| other.id),
            );
        }
        // Each one leaves a "deleted" line in the history (D-172).
        self.vault_mut()?.clinic_delete_many(&ids, true)?;
        Ok(ClinicReply::Done)
    }

    // -------------------------------------------------------------- patients

    fn clinic_save_patient(&mut self, patient: Patient) -> Result<ClinicReply, CoreError> {
        let mut patient = rules::normalize_patient(patient);
        let v = self.vault_ref()?;
        let stored = if patient.id.is_empty() {
            None
        } else {
            Some(self::patient(v, &patient.id)?)
        };
        match &stored {
            // The link to a case and the day the card was opened are never the window's.
            Some(stored) => {
                patient.case_id.clone_from(&stored.case_id);
                patient.created_on.clone_from(&stored.created_on);
            }
            None => {
                patient.case_id = None;
                if patient.created_on.is_empty() {
                    patient.created_on = iso(today());
                }
            }
        }
        // A family on the list waits since some day: the day the card was opened, unless
        // she said otherwise.
        if patient.status == PatientStatus::Waiting && patient.waiting_since.is_none() {
            patient.waiting_since = Some(patient.created_on.clone());
        }
        rules::validate_patient(&patient).map_err(refusal)?;
        if stored.as_ref() != Some(&patient) {
            write_patient(self.vault_mut()?, &mut patient)?;
        }
        Ok(ClinicReply::Patient(patient))
    }

    /// Erases a card and everything of theirs in the clinic, never the case (D-174): only a
    /// card where nothing happened, or one whose retention period has passed.
    fn clinic_erase_patient(&mut self, id: &str, name: &str) -> Result<ClinicReply, CoreError> {
        let v = self.vault_ref()?;
        let patient = patient(v, id)?;
        if rules::clean_line(name) != rules::clean_line(&patient.name) {
            return refuse(
                "השם שהוקלד לא זהה לשם שבכרטיס. כדי למחוק צריך להקליד את השם בדיוק כמו שהוא כתוב בכרטיס.",
            );
        }
        let rows = v.clinic_of_patient(id)?;
        // The day one of the card's records was last written, by the core's own clock.
        let last_saved = rows
            .iter()
            .map(|row| row.updated_at)
            .max()
            .map(|at| iso(date_of(at)));
        let of_kind = |kind: &str| -> Vec<StoredClinicRecord> {
            rows.iter()
                .filter(|row| row.kind == kind)
                .cloned()
                .collect()
        };
        let visits: Vec<Visit> = open_all(of_kind(Visit::KIND))?;
        let notes: Vec<SessionNote> = open_all(of_kind(SessionNote::KIND))?;
        let charges: Vec<Charge> = open_all(of_kind(Charge::KIND))?;
        let payments: Vec<Payment> = open_all(of_kind(Payment::KIND))?;
        let tasks: Vec<ClinicTask> = open_all(of_kind(ClinicTask::KIND))?;
        let records = PatientRecords {
            patient: &patient,
            visits: &visits,
            notes: &notes,
            charges: &charges,
            payments: &payments,
            tasks: &tasks,
            report: None,
        };
        rules::can_erase(
            &records,
            &iso(today()),
            last_saved.as_deref(),
            KEEP_YEARS_AFTER_LAST_CHANGE,
            KEEP_UNTIL_AGE,
        )
        .map_err(refusal)?;
        self.vault_mut()?.erase_clinic_patient(id)?;
        Ok(ClinicReply::Done)
    }

    // -------------------------------------------------------------- session notes

    fn clinic_save_note(&mut self, note: SessionNote) -> Result<ClinicReply, CoreError> {
        let mut note = rules::normalize_note(note);
        let v = self.vault_ref()?;
        let stored: Option<SessionNote> = if note.id.is_empty() {
            None
        } else {
            Some(find(v, &note.id)?)
        };
        // Whether the record was copied to a case is the core's to say, never the window's.
        note.case_input_id = stored.as_ref().and_then(|s| s.case_input_id.clone());
        rules::validate_note(&note).map_err(refusal)?;
        if stored
            .as_ref()
            .is_some_and(|stored| stored.patient_id != note.patient_id)
        {
            return refuse("אי אפשר להעביר סיכום למטופל אחר. אפשר לכתוב סיכום חדש בכרטיס הנכון.");
        }
        patient_exists(v, &note.patient_id)?;
        if let Some(visit_id) = &note.visit_id {
            let visit: Visit = find(v, visit_id)?;
            if visit.patient_id.as_deref() != Some(note.patient_id.as_str()) {
                return refuse("הפגישה שנבחרה שייכת למטופל אחר.");
            }
            // One note for a visit: the one that exists is opened and edited.
            let notes: Vec<SessionNote> = all(v)?;
            let taken = notes
                .iter()
                .any(|other| other.id != note.id && other.visit_id.as_ref() == Some(visit_id));
            if taken {
                return refuse("לפגישה הזו כבר יש סיכום. אפשר לפתוח אותו ולערוך אותו.");
            }
        }
        if stored.as_ref() != Some(&note) {
            let action = if stored.is_some() { CHANGED } else { CREATED };
            write(self.vault_mut()?, &mut note, Some(action))?;
        }
        Ok(ClinicReply::Note(note))
    }

    /// A session note is the clinical record: once it says something it is edited, with its
    /// history, and never removed. Only one that holds nothing at all can go.
    fn clinic_delete_note(&mut self, id: &str) -> Result<ClinicReply, CoreError> {
        let note: SessionNote = find(self.vault_ref()?, id)?;
        if !(note.record.trim().is_empty() && note.personal.trim().is_empty()) {
            return refuse(
                "סיכום פגישה הוא חלק מהרשומה, ולכן לא מוחקים אותו. אפשר לערוך אותו, וכל שינוי נשמר בהיסטוריה.",
            );
        }
        self.vault_mut()?.clinic_delete(id, true)?;
        Ok(ClinicReply::Done)
    }

    /// Copies the formal record of a note into the patient's case as a material, through the
    /// same functions as a material she adds by hand. The personal note stays here.
    fn clinic_note_to_case(&mut self, id: &str) -> Result<ClinicReply, CoreError> {
        let v = self.vault_ref()?;
        let mut note: SessionNote = find(v, id)?;
        let patient = patient(v, &note.patient_id)?;
        // A note whose visit was removed is a note on its own.
        let visit_kind: Option<VisitKind> = note
            .visit_id
            .as_deref()
            .and_then(|visit_id| find::<Visit>(v, visit_id).ok())
            .map(|visit| visit.kind);
        let cases = self.clinic_cases()?;
        let Some((case_id, case)) = case_of(&patient, &cases) else {
            return refuse(
                "למטופל עוד אין תיק אבחון. אפשר לפתוח תיק מהכרטיס, ואז להעביר את הסיכום.",
            );
        };
        if case.in_trash {
            return refuse(
                "תיק האבחון נמצא בסל המחזור. אפשר לשחזר אותו ברשימת התיקים, ואז להעביר.",
            );
        }
        if note.record.trim().is_empty() {
            return refuse("הרשומה ריקה, ואין מה להעביר לתיק. התרשומת האישית נשארת רק אצלך.");
        }
        let case_id = case_id.to_owned();
        let kind = match visit_kind {
            Some(VisitKind::Intake) => InputKind::Intake,
            Some(VisitKind::Observation) => InputKind::Observation,
            _ => InputKind::SessionNote,
        };
        let title = rules::note_case_title_for(visit_kind, &note.date);
        // Only the formal record. The material sent before is replaced while it is still
        // in the case; otherwise a new one is added.
        let record = note.record.clone();
        let sent_before = match &note.case_input_id {
            Some(input_id) => self
                .vault_ref()?
                .inputs(&case_id)?
                .into_iter()
                .any(|input| &input.id == input_id),
            None => false,
        };
        match note.case_input_id.clone().filter(|_| sent_before) {
            Some(input_id) => self.update_input(&case_id, &input_id, &title, &record)?,
            None => {
                let input = self.add_input(&case_id, kind, &title, &record)?;
                note.case_input_id = Some(input.id);
                write(self.vault_mut()?, &mut note, Some(CHANGED))?;
            }
        }
        self.vault_mut()?.record(
            AuditEvent::ClinicNoteToCase,
            Some(&case_id),
            &serde_json::json!({}),
        )?;
        Ok(ClinicReply::Note(note))
    }

    // -------------------------------------------------------------- money

    fn clinic_save_payment(&mut self, payment: Payment) -> Result<ClinicReply, CoreError> {
        let mut payment = rules::normalize_payment(payment);
        let v = self.vault_ref()?;
        let stored: Option<Payment> = if payment.id.is_empty() {
            None
        } else {
            Some(find(v, &payment.id)?)
        };
        if stored.as_ref().is_some_and(|stored| stored.voided) {
            return refuse("תקבול שבוטל אי אפשר לערוך. אפשר לרשום תקבול חדש.");
        }
        // Voiding has its own request, with a reason.
        payment.voided = false;
        payment.void_reason = String::new();
        rules::validate_payment(&payment).map_err(refusal)?;
        patient_exists(v, &payment.patient_id)?;
        if stored.as_ref() != Some(&payment) {
            let action = if stored.is_some() { CHANGED } else { CREATED };
            write(self.vault_mut()?, &mut payment, Some(action))?;
        }
        Ok(ClinicReply::Payment(payment))
    }

    fn clinic_void_payment(&mut self, id: &str, reason: &str) -> Result<ClinicReply, CoreError> {
        let mut payment: Payment = find(self.vault_ref()?, id)?;
        if payment.voided {
            return refuse("התקבול הזה כבר מבוטל.");
        }
        let reason = rules::clean_line(reason);
        rules::validate_void_reason(&reason).map_err(refusal)?;
        payment.voided = true;
        payment.void_reason = reason;
        write(self.vault_mut()?, &mut payment, Some(VOIDED))?;
        Ok(ClinicReply::Payment(payment))
    }

    fn clinic_save_charge(&mut self, charge: Charge) -> Result<ClinicReply, CoreError> {
        let mut charge = rules::normalize_charge(charge);
        let v = self.vault_ref()?;
        let stored: Option<Charge> = if charge.id.is_empty() {
            None
        } else {
            Some(find(v, &charge.id)?)
        };
        if stored.as_ref().is_some_and(|stored| stored.voided) {
            return refuse("חיוב שבוטל אי אפשר לערוך. אפשר לרשום חיוב חדש.");
        }
        charge.voided = false;
        charge.void_reason = String::new();
        rules::validate_charge(&charge).map_err(refusal)?;
        patient_exists(v, &charge.patient_id)?;
        if stored.as_ref() != Some(&charge) {
            let action = if stored.is_some() { CHANGED } else { CREATED };
            write(self.vault_mut()?, &mut charge, Some(action))?;
        }
        Ok(ClinicReply::Charge(charge))
    }

    fn clinic_void_charge(&mut self, id: &str, reason: &str) -> Result<ClinicReply, CoreError> {
        let mut charge: Charge = find(self.vault_ref()?, id)?;
        if charge.voided {
            return refuse("החיוב הזה כבר מבוטל.");
        }
        let reason = rules::clean_line(reason);
        rules::validate_void_reason(&reason).map_err(refusal)?;
        charge.voided = true;
        charge.void_reason = reason;
        write(self.vault_mut()?, &mut charge, Some(VOIDED))?;
        Ok(ClinicReply::Charge(charge))
    }

    // -------------------------------------------------------------- tasks and settings

    fn clinic_save_task(&mut self, task: ClinicTask) -> Result<ClinicReply, CoreError> {
        let mut task = rules::normalize_task(task);
        let v = self.vault_ref()?;
        let stored: Option<ClinicTask> = if task.id.is_empty() {
            None
        } else {
            Some(find(v, &task.id)?)
        };
        rules::validate_task(&task).map_err(refusal)?;
        if let Some(patient_id) = &task.patient_id {
            patient_exists(v, patient_id)?;
        }
        if stored.as_ref() != Some(&task) {
            write(self.vault_mut()?, &mut task, None)?;
        }
        Ok(ClinicReply::Task(task))
    }

    fn clinic_delete_task(&mut self, id: &str) -> Result<ClinicReply, CoreError> {
        let task: ClinicTask = find(self.vault_ref()?, id)?;
        self.vault_mut()?.clinic_delete(&task.id, false)?;
        Ok(ClinicReply::Done)
    }

    fn clinic_save_settings(&mut self, new: ClinicSettings) -> Result<ClinicReply, CoreError> {
        let new = rules::normalize_settings(new);
        rules::validate_settings(&new).map_err(refusal)?;
        if settings(self.vault_ref()?)? != new {
            let json = to_json(&new)?;
            self.vault_mut()?
                .clinic_put(SETTINGS, None, None, 0, &json, None)?;
        }
        Ok(ClinicReply::Settings(new))
    }

    // -------------------------------------------------------------- cases

    /// Cases at work that no patient card is linked to yet.
    fn clinic_case_choices(&mut self) -> Result<ClinicReply, CoreError> {
        let linked: HashSet<String> = patients(self.vault_ref()?)?
            .into_iter()
            .filter_map(|p| p.case_id)
            .collect();
        // The order of the case list: the one changed last comes first.
        let choices = self
            .list_cases()?
            .into_iter()
            .filter(|case| !linked.contains(&case.id))
            .map(|case| CaseChoice {
                label: match &case.child_name {
                    Some(name) => format!("{name} · {}", case.meta.code),
                    None => case.meta.code.clone(),
                },
                id: case.id,
            })
            .collect();
        Ok(ClinicReply::Cases(choices))
    }

    fn clinic_link_case(
        &mut self,
        patient_id: &str,
        case_id: Option<String>,
    ) -> Result<ClinicReply, CoreError> {
        let all_patients = patients(self.vault_ref()?)?;
        let Some(mut patient) = all_patients.iter().find(|p| p.id == patient_id).cloned() else {
            return Err(CoreError::NotFound(NO_PATIENT.to_owned()));
        };
        if let Some(case_id) = &case_id {
            let cases = self.clinic_cases()?;
            let Some(case) = cases.get(case_id) else {
                return Err(CoreError::NotFound(NO_CASE.to_owned()));
            };
            if case.in_trash {
                return refuse(
                    "התיק הזה נמצא בסל המחזור. אפשר לשחזר אותו ברשימת התיקים, ואז לקשר.",
                );
            }
            // A case is the report of one child.
            let taken = all_patients
                .iter()
                .any(|p| p.id != patient_id && p.case_id.as_ref() == Some(case_id));
            if taken {
                return refuse("התיק הזה כבר מקושר למטופל אחר.");
            }
        }
        if patient.case_id != case_id {
            patient.case_id = case_id;
            write_patient(self.vault_mut()?, &mut patient)?;
        }
        Ok(ClinicReply::Patient(patient))
    }

    /// Opens a diagnostic case for the patient through [`Core::create_case`], so every rule
    /// of a case holds. Every named contact is registered as a person of the case, so a
    /// session record copied into it later has their names hidden by the case's filter.
    fn clinic_create_case(
        &mut self,
        patient_id: &str,
        today: &Today,
    ) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        let mut patient = patient(self.vault_ref()?, patient_id)?;
        let cases = self.clinic_cases()?;
        if case_of(&patient, &cases).is_some() {
            return refuse("למטופל הזה כבר יש תיק אבחון.");
        }
        let Some((year, month, day)) = rules::parse_date(&today.date) else {
            return refuse(
                "התאריך של היום לא התקבל כמו שצריך. כדאי לסגור את המסך ולפתוח אותו שוב.",
            );
        };
        let base = format!("תיק-{year:04}-{month:02}{day:02}");
        let taken = |code: &str| cases.values().any(|case| case.code == code);
        let code = std::iter::once(base.clone())
            .chain((2_u32..).map(|n| format!("{base}-{n}")))
            .find(|code| !taken(code))
            .unwrap_or(base);
        let meta = CaseMeta {
            code,
            // A case holds an age only within what a case accepts; otherwise none.
            age: rules::age_on(patient.birth_date.as_deref(), &today.date)
                .filter(|age| crate::age_ok(*age)),
            child_gender: patient.gender,
            consent: None,
            ..CaseMeta::default()
        };
        let mut identities = vec![IdentityInput {
            id: None,
            role: Role::Child,
            value: patient.name.clone(),
            aliases: Vec::new(),
        }];
        for contact in &patient.contacts {
            let name = rules::clean_line(&contact.name);
            if name.is_empty() || identities.iter().any(|known| known.value == name) {
                continue;
            }
            identities.push(IdentityInput {
                id: None,
                role: contact_role(contact.role),
                value: name,
                aliases: Vec::new(),
            });
        }
        let case_id = self.create_case(meta, identities)?;
        patient.case_id = Some(case_id.clone());
        write_patient(self.vault_mut()?, &mut patient)?;
        match self.clinic_cases()?.get(&case_id) {
            Some(case) => Ok(ClinicReply::Case(case.link(&case_id))),
            None => Err(CoreError::NotFound(NO_CASE.to_owned())),
        }
    }
}
