//! The clinic's service (beta, D-170): one door, [`Core::clinic`], for everything the clinic
//! screens ask. It loads the sealed records, applies the rules of `dv_domain::clinic` and
//! saves; it also keeps the link between a patient card and a diagnostic case.
//!
//! Nothing here leaves the computer and nothing here talks to a model: this file knows the
//! vault and the rules only. The one way clinic text reaches a case is `NoteToCase`, which
//! puts the formal record of a session (never the personal note) into the case as a
//! material, exactly as one she adds by hand (the first time written together with the note
//! that points to it, D-177; after that through the core's own `update_input`); from there
//! the case's usual filter applies, and the names of the card are names it knows. A test reads this file and fails if a name of the network or
//! model crates appears in it.
//!
//! The records are the domain types as JSON. A record's id is the vault's: it is set on
//! every read, so the id inside the JSON never matters.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use dv_domain::clinic::{
    self as rules, CaseChoice, CaseLink, Charge, ClinicReply, ClinicRequest, ClinicSettings,
    ClinicTask, ContactRole, ErasedCards, ErasedTally, HistoryEntry, IncomeExport, MessageKind,
    MessageValues, Overview, Patient, PatientCard, PatientRecords, PatientRow, PatientStatus,
    Payment, Repeat, SavedVisits, Scope, SessionNote, Today, Visit, VisitKind, VisitStatus,
    VisitView,
};
use dv_domain::{CaseMeta, DomainError, IdentityInput, InputKind, Role};
use dv_vault::{AuditEvent, StoredClinicRecord, Vault, VaultError};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::dates::{date_of, iso, today};
use crate::retention::{KEEP_UNTIL_AGE, KEEP_YEARS_AFTER_LAST_CHANGE};
use crate::{Core, CoreError};

const SETTINGS: &str = "settings";
/// The one settings record has a fixed id.
const SETTINGS_ID: &str = "settings";
/// The anonymous tally of the cards that were erased (D-179): one record, a fixed id.
const TALLY: &str = "tally";
const TALLY_ID: &str = "tally";
const CREATED: &str = "created";
/// The last date the clinic keeps (`rules::date_of_day`).
const LAST_DATE: &str = "2200-12-31";
/// The longest stretch of holidays one request may ask for, in days after the first.
const HOLIDAY_RANGE_DAYS: i64 = 400;
/// The furthest the visits are read when a pathway is proposed, in days: what the longest
/// pathway `validate_pathway` allows can reach, 12 steps of a 180-day gap and a 120-day search.
const PATHWAY_LOOK_DAYS: i64 = 12 * (180 + 120);
const CHANGED: &str = "changed";
const VOIDED: &str = "voided";

const NO_PATIENT: &str = "המטופל";
const NO_CASE: &str = "התיק";
/// An income export that would leave a row out is not made (D-194).
const EXPORT_NOT_READ: &str = "תקבול או סיכום של כרטיסים שנמחקו בתקופה הזו לא נקרא מהכספת, \
     ולכן הקובץ לרואה החשבון לא נוצר: הוא צריך להיות שלם. כדאי לשחזר מגיבוי, או לפנות לתמיכה.";

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
    /// The record holds no value of a later version (`Unknown`, D-178): only then is it
    /// written, so such a value is never turned into something else.
    fn known(&self) -> Result<(), DomainError> {
        Ok(())
    }
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
    fn known(&self) -> Result<(), DomainError> {
        rules::visit_known(self)
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
    fn known(&self) -> Result<(), DomainError> {
        rules::payment_known(self)
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
    fn known(&self) -> Result<(), DomainError> {
        rules::task_known(self)
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

/// Every row, or an error: for what is decided from a patient's records (an erase, what the
/// family owes), which is never decided with a row missing.
fn open_all<T: Kept>(rows: Vec<StoredClinicRecord>) -> Result<Vec<T>, CoreError> {
    rows.into_iter().map(open).collect()
}

/// The rows that read, and how many did not (D-178). A row that does not read is left in
/// the vault exactly as it is: it is shown nowhere, counted in no total and never rewritten.
fn open_readable<T: Kept>(rows: Vec<StoredClinicRecord>) -> (Vec<T>, u32) {
    let total = rows.len();
    let read: Vec<T> = rows.into_iter().filter_map(|row| open(row).ok()).collect();
    let skipped = count(total.saturating_sub(read.len()));
    (read, skipped)
}

fn all_counted<T: Kept>(v: &Vault) -> Result<(Vec<T>, u32), CoreError> {
    Ok(open_readable(v.clinic_kind(T::KIND)?))
}

fn all<T: Kept>(v: &Vault) -> Result<Vec<T>, CoreError> {
    Ok(all_counted(v)?.0)
}

/// The records of a kind dated from `from` to `to`, both included.
fn between<T: Kept>(v: &Vault, from: &str, to: &str) -> Result<Vec<T>, CoreError> {
    match (rules::day_number(from), rules::day_number(to)) {
        (Some(first), Some(last)) => Ok(open_readable(v.clinic_range(T::KIND, first, last)?).0),
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
    record.known().map_err(refusal)?;
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

/// The cards that read, and how many did not (D-178).
fn patients_counted(v: &Vault) -> Result<(Vec<Patient>, u32), CoreError> {
    let rows = v.clinic_patients()?;
    let total = rows.len();
    let read: Vec<Patient> = rows
        .into_iter()
        .filter_map(|(id, json)| open_patient(id, &json).ok())
        .collect();
    let skipped = count(total.saturating_sub(read.len()));
    Ok((read, skipped))
}

fn patients(v: &Vault) -> Result<Vec<Patient>, CoreError> {
    Ok(patients_counted(v)?.0)
}

fn patient(v: &Vault, id: &str) -> Result<Patient, CoreError> {
    match v.clinic_patient(id)? {
        Some(json) => open_patient(id.to_owned(), &json),
        None => Err(CoreError::NotFound(NO_PATIENT.to_owned())),
    }
}

fn write_patient(v: &mut Vault, patient: &mut Patient) -> Result<(), CoreError> {
    rules::patient_known(patient).map_err(refusal)?;
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

/// How the clinic works, and whether the stored settings could not be read (D-178): then
/// the defaults are shown, and the stored row stays as it is until she saves settings.
fn settings_counted(v: &Vault) -> (ClinicSettings, u32) {
    match v.clinic_get(SETTINGS_ID) {
        Ok(Some(row)) if row.kind == SETTINGS => match from_json(&row.json) {
            Ok(settings) => (settings, 0),
            Err(_) => (ClinicSettings::default(), 1),
        },
        Ok(_) => (ClinicSettings::default(), 0),
        // A row that does not open is counted by the vault's own scan.
        Err(_) => (ClinicSettings::default(), 0),
    }
}

fn settings(v: &Vault) -> ClinicSettings {
    settings_counted(v).0
}

/// The tally of erased cards as it must be read before it is written again: an error when
/// it is there and does not read, so an erase never replaces it with less.
/// The day the card (`card_saved`) or one of its records was last written, by the core's own
/// clock: a card corrected years after its last record was active then too.
pub(crate) fn last_saved_day(rows: &[StoredClinicRecord], card_saved: i64) -> Option<String> {
    rows.iter()
        .map(|row| row.updated_at)
        .chain(std::iter::once(card_saved))
        .max()
        .map(|at| iso(date_of(at)))
}

fn tally_strict(v: &Vault) -> Result<ErasedTally, CoreError> {
    match v.clinic_get(TALLY_ID)? {
        Some(row) if row.kind == TALLY => from_json(&row.json),
        _ => Ok(ErasedTally::default()),
    }
}

fn cards_strict(v: &Vault) -> Result<ErasedCards, CoreError> {
    match v.clinic_erased_cards()? {
        Some(json) => from_json(&json),
        None => Ok(ErasedCards::default()),
    }
}

/// The tallies for a screen: one that does not read counts as a row that was not read and
/// shows as empty, like any other row (D-178).
fn tally(v: &Vault) -> (ErasedTally, u32) {
    match v.clinic_get(TALLY_ID) {
        Ok(Some(row)) if row.kind == TALLY => match from_json(&row.json) {
            Ok(tally) => (tally, 0),
            Err(_) => (ErasedTally::default(), 1),
        },
        // A row that does not open is counted by the vault's own scan.
        _ => (ErasedTally::default(), 0),
    }
}

fn erased_cards(v: &Vault) -> (ErasedCards, u32) {
    match cards_strict(v) {
        Ok(cards) => (cards, 0),
        Err(_) => (ErasedCards::default(), 1),
    }
}

/// The count of unreadable rows last written to the log for each vault folder, in this run:
/// the log gets one line when the count changes, not one for every screen she opens.
static REPORTED: LazyLock<Mutex<HashMap<PathBuf, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether this count of unreadable rows is news for the log.
fn unreadable_is_news(dir: &Path, rows: u32) -> bool {
    let Ok(mut reported) = REPORTED.lock() else {
        return rows > 0;
    };
    if rows == 0 {
        reported.remove(dir);
        return false;
    }
    reported.insert(dir.to_owned(), rows) != Some(rows)
}

/// The visits that ever left "planned" (D-176): by what they say now, and by every state
/// of theirs that was saved. A state that cannot be read counts as a mark: nothing is
/// removed on the strength of a history that cannot be checked.
fn marked_visits(v: &Vault, visits: &[Visit]) -> Result<HashSet<String>, CoreError> {
    let mut marked = HashSet::new();
    for visit in visits {
        let mut was_marked = visit.status != VisitStatus::Planned;
        if !was_marked {
            was_marked = v.clinic_history(&visit.id)?.iter().any(|state| {
                from_json::<Visit>(&state.json)
                    .map_or(true, |saved| rules::ever_marked(std::iter::once(&saved)))
            });
        }
        if was_marked {
            marked.insert(visit.id.clone());
        }
    }
    Ok(marked)
}

/// The people of a card the filter of its case must know: the child, then every contact
/// that has a name, each once.
fn card_people(patient: &Patient) -> Vec<(Role, String)> {
    let mut people = vec![(Role::Child, rules::clean_line(&patient.name))];
    for contact in &patient.contacts {
        let name = rules::clean_line(&contact.name);
        if !people.iter().any(|(_, known)| *known == name) {
            people.push((contact_role(contact.role), name));
        }
    }
    people.retain(|(_, name)| !name.is_empty());
    people
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
    /// Rows that opened and hold something this version cannot read: they are in none of
    /// the lists above (D-178). Rows that do not open at all are counted by the vault.
    unreadable: u32,
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
    /// One row that does not read never fails the whole clinic: it is left out, and counted.
    fn load(v: &Vault) -> Result<Self, CoreError> {
        let (patients, a) = patients_counted(v)?;
        let (visits, b) = all_counted(v)?;
        let (notes, c) = all_counted(v)?;
        let (charges, d) = all_counted(v)?;
        let (payments, e) = all_counted(v)?;
        let (tasks, f) = all_counted(v)?;
        let (settings, g) = settings_counted(v);
        let unreadable = [a, b, c, d, e, f, g]
            .into_iter()
            .fold(0, u32::saturating_add);
        Ok(Self {
            patients,
            visits,
            notes,
            charges,
            payments,
            tasks,
            settings,
            unreadable,
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
        ContactRole::Guardian | ContactRole::Other | ContactRole::Unknown => Role::Other,
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
            ClinicRequest::Settings => Ok(ClinicReply::Settings(settings(self.vault_ref()?))),
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
            ClinicRequest::Holidays { from, to } => self.clinic_holidays(&from, &to),
            ClinicRequest::FreeSlots {
                today,
                from,
                minutes,
                count,
                skip_visit,
            } => self.clinic_free_slots(&today, &from, minutes, count, skip_visit.as_deref()),
            ClinicRequest::ProposePathway {
                patient_id,
                today,
                from,
            } => self.clinic_propose_pathway(&patient_id, &today, &from),
            ClinicRequest::SavePathway {
                patient_id,
                visits,
                package,
                today,
            } => self.clinic_save_pathway(&patient_id, visits, package, &today),
            ClinicRequest::StarterTasks { patient_id, today } => {
                self.clinic_starter_tasks(&patient_id, &today)
            }
            ClinicRequest::Insights {
                today,
                from_month,
                to_month,
            } => self.clinic_insights(&today, &from_month, &to_month),
        }
    }

    // -------------------------------------------------------------- planning (D-196..D-200)

    /// Israel's holidays of a stretch (D-196). Nothing is read from the vault, but a locked
    /// vault still says so first, like every other request of the clinic.
    fn clinic_holidays(&mut self, from: &str, to: &str) -> Result<ClinicReply, CoreError> {
        self.vault_ref()?;
        let (Some(first), Some(last)) = (rules::day_number(from), rules::day_number(to)) else {
            return refuse("צריך לבחור תאריכים תקינים ליומן.");
        };
        if last < first || last - first > HOLIDAY_RANGE_DAYS {
            return refuse("אפשר לבקש את החגים של עד שנה אחת בכל פעם.");
        }
        Ok(ClinicReply::Holidays(rules::holidays_between(from, to)))
    }

    /// The visits from the later of `from` and today, as far ahead as a search looks.
    fn visits_ahead(
        &mut self,
        today: &Today,
        from: &str,
        days: i64,
    ) -> Result<Vec<Visit>, CoreError> {
        let start = if from > today.date.as_str() {
            from
        } else {
            today.date.as_str()
        };
        if rules::day_number(start).is_none() {
            return refuse("צריך לבחור תאריך תקין.");
        }
        // Past the last date the clinic keeps, there is nothing more to read.
        let end = rules::add_days(start, days).unwrap_or_else(|| LAST_DATE.to_owned());
        between(self.vault_ref()?, start, &end)
    }

    fn clinic_free_slots(
        &mut self,
        today: &Today,
        from: &str,
        minutes: u16,
        count: u8,
        skip_visit: Option<&str>,
    ) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        if rules::day_number(from).is_none() {
            return refuse("צריך לבחור תאריך תקין.");
        }
        let visits = self.visits_ahead(today, from, rules::SLOT_SEARCH_DAYS)?;
        let settings = settings(self.vault_ref()?);
        let slots = rules::free_slots(&visits, &settings, today, from, minutes, count, skip_visit)
            .map_err(refusal)?;
        Ok(ClinicReply::Slots(slots))
    }

    fn clinic_propose_pathway(
        &mut self,
        patient_id: &str,
        today: &Today,
        from: &str,
    ) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        if rules::day_number(from).is_none() {
            return refuse("צריך לבחור תאריך תקין להתחלה.");
        }
        let v = self.vault_ref()?;
        let child = patient(v, patient_id)?;
        let settings = settings(v);
        // As far as the rule can reach: every step its gap (at least a day) and one search,
        // at most 12 × (180 + 120) days, as `validate_pathway` bounds the steps.
        let span = settings
            .pathway
            .iter()
            .map(|step| i64::from(step.gap_days.max(1)) + rules::SLOT_SEARCH_DAYS)
            .sum::<i64>()
            .clamp(rules::SLOT_SEARCH_DAYS, PATHWAY_LOOK_DAYS);
        let visits = self.visits_ahead(today, from, span)?;
        let proposed =
            rules::propose_pathway(&visits, &settings, &child, today, from).map_err(refusal)?;
        Ok(ClinicReply::Proposed(proposed))
    }

    /// The pathway's visits, and with `package` the one charge of the package, in one
    /// transaction (D-198). Only new planned visits of this patient, not before today.
    fn clinic_save_pathway(
        &mut self,
        patient_id: &str,
        visits: Vec<Visit>,
        package: bool,
        today: &Today,
    ) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        if visits.is_empty() {
            return refuse("אין פגישות לשמור במסלול.");
        }
        if visits.len() > rules::MAX_PATHWAY_STEPS {
            return refuse("יש יותר מדי פגישות במסלול אחד.");
        }
        let v = self.vault_ref()?;
        patient_exists(v, patient_id)?;
        let child = patient(v, patient_id)?;
        let settings = settings(v);
        let mut rows = Vec::with_capacity(visits.len());
        for visit in visits {
            let mut visit = rules::normalize_visit(visit);
            if !visit.id.is_empty() || visit.patient_id.as_deref() != Some(patient_id) {
                return refuse("במסלול יש פגישה שלא שייכת לכרטיס הזה. אפשר לפתוח את המסלול מחדש.");
            }
            if visit.kind == VisitKind::Block {
                return refuse("במסלול האבחון אין זמנים שמורים, רק פגישות.");
            }
            if visit.date < today.date {
                return refuse("אחת הפגישות במסלול נקבעה לתאריך שעבר. אפשר לבחור תאריך אחר.");
            }
            visit.status = VisitStatus::Planned;
            visit.series_id = None;
            if package {
                visit.fee = 0;
                visit.charge_missed = false;
            }
            rules::validate_visit(&visit).map_err(refusal)?;
            rows.push(visit);
        }
        let charge = if package {
            if settings.package_fee <= 0 {
                return refuse("עוד אין מחיר לחבילת האבחון. אפשר לקבוע אותו בהגדרות הקליניקה.");
            }
            let first = rows
                .iter()
                .map(|visit| visit.date.clone())
                .min()
                .unwrap_or_else(|| today.date.clone());
            let charge = Charge {
                id: String::new(),
                patient_id: patient_id.to_owned(),
                date: first,
                amount: settings.package_fee,
                what: settings.package_name.clone(),
                voided: false,
                void_reason: String::new(),
            };
            rules::validate_charge(&charge).map_err(refusal)?;
            Some(charge)
        } else {
            None
        };
        // A family on the waiting list that gets a visit is in the process now. The card is
        // checked before anything is written, so that a refusal leaves nothing behind.
        let activated = rules::activated(&child);
        if let Some(active) = &activated {
            rules::patient_known(active).map_err(refusal)?;
        }
        let put = |row: &dyn Fn() -> Result<String, CoreError>, patient: &str, day: i64| {
            Ok::<_, CoreError>((None, Some(patient.to_owned()), day, row()?))
        };
        let visit_rows = rows
            .iter()
            .map(|visit| put(&|| to_json(visit), patient_id, day_of(visit)))
            .collect::<Result<Vec<_>, _>>()?;
        let charge_rows = charge
            .iter()
            .map(|charge| put(&|| to_json(charge), patient_id, day_of(charge)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut groups: Vec<(&str, &[dv_vault::ClinicPut])> = vec![(Visit::KIND, &visit_rows)];
        if !charge_rows.is_empty() {
            groups.push((Charge::KIND, &charge_rows));
        }
        let ids = self
            .vault_mut()?
            .clinic_put_groups(&groups, Some(CREATED))?;
        let ids: Vec<String> = ids.into_iter().next().unwrap_or_default();
        if let Some(mut active) = activated {
            write_patient(self.vault_mut()?, &mut active)?;
        }
        let book = Book::load(self.vault_ref()?)?;
        let written: Vec<Visit> = book
            .visits
            .iter()
            .filter(|visit| ids.contains(&visit.id))
            .cloned()
            .collect();
        let views = book.views(&written);
        let overlapping = count(views.iter().filter(|view| view.overlaps).count());
        Ok(ClinicReply::Saved(SavedVisits {
            visits: views,
            overlapping,
            left_alone: 0,
        }))
    }

    /// The starter follow-ups of a family (D-199), in one transaction.
    fn clinic_starter_tasks(
        &mut self,
        patient_id: &str,
        today: &Today,
    ) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        let v = self.vault_ref()?;
        let child = patient(v, patient_id)?;
        let settings = settings(v);
        if settings.starter_tasks.is_empty() {
            return refuse("עוד אין ערכת פתיחה. אפשר להגדיר אותה בהגדרות הקליניקה.");
        }
        let tasks: Vec<ClinicTask> = all(v)?;
        let new = rules::starter_tasks(&settings, patient_id, &tasks, &today.date);
        if new.is_empty() {
            return refuse("כל המעקבים של ערכת הפתיחה כבר פתוחים בכרטיס הזה.");
        }
        let mut rows = Vec::with_capacity(new.len());
        for task in &new {
            rules::validate_task(task).map_err(refusal)?;
            rules::task_known(task).map_err(refusal)?;
            rows.push((
                None,
                Some(patient_id.to_owned()),
                day_of(task),
                to_json(task)?,
            ));
        }
        let ids = self
            .vault_mut()?
            .clinic_put_many(ClinicTask::KIND, &rows, None)?;
        let written: Vec<ClinicTask> = new
            .into_iter()
            .zip(ids)
            .map(|(task, id)| ClinicTask { id, ..task })
            .collect();
        let names = HashMap::from([(child.id.clone(), child.name.clone())]);
        Ok(ClinicReply::Tasks(rules::task_views(
            &written,
            &names,
            &today.date,
        )))
    }

    /// The picture of the clinic over months (D-200): counts and sums only.
    fn clinic_insights(
        &mut self,
        today: &Today,
        from_month: &str,
        to_month: &str,
    ) -> Result<ClinicReply, CoreError> {
        check_today(today)?;
        let v = self.vault_ref()?;
        let book = Book::load(v)?;
        // The report's progress plays no part in the picture: no case is read.
        let reports = HashMap::new();
        let records = rules::ClinicRecords {
            patients: &book.patients,
            visits: &book.visits,
            notes: &book.notes,
            charges: &book.charges,
            payments: &book.payments,
            tasks: &book.tasks,
            reports: &reports,
        };
        let insights = rules::insights(
            &records,
            &book.settings,
            from_month,
            to_month,
            &today.date,
            &tally(v).0,
            &erased_cards(v).0,
        )
        .map_err(refusal)?;
        Ok(ClinicReply::Insights(Box::new(insights)))
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
        let mut alerts = rules::clinic_alerts(&records, today, &book.settings);
        // Rows that were not read come first: every figure below is without them.
        // Rows that do not open at all (damaged, or changed in the file), rows that open and
        // hold something this version cannot read, and the two tallies of erased cards.
        let v = self.vault_ref()?;
        let unreadable = [book.unreadable, tally(v).1, erased_cards(v).1]
            .into_iter()
            .fold(v.clinic_unreadable()?, u32::saturating_add);
        if let Some(alert) = rules::unreadable_alert(unreadable, &today.date) {
            alerts.insert(0, alert);
        }
        let received =
            i64::from(rules::month_received(&book.payments, &today.date).map_err(refusal)?)
                .saturating_add(rules::erased_received(
                    &tally(self.vault_ref()?).0,
                    &today.date,
                ));
        let month_received = i32::try_from(received).map_err(|_| {
            CoreError::Refused(
                "הסכום הכולל גדול מדי לחישוב. כדאי לבדוק את הסכומים שנרשמו.".to_owned(),
            )
        })?;
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
        let overview = Overview {
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
            month_received,
            total_due: rules::total_due(&balances).map_err(refusal)?,
            waiting: status(PatientStatus::Waiting),
            active: status(PatientStatus::Active),
        };
        // The log says once that rows were not read, and how many: never which, or what.
        if unreadable_is_news(self.vault_ref()?.dir(), unreadable) {
            self.vault_mut()?.record(
                AuditEvent::ClinicUnreadable,
                None,
                &serde_json::json!({ "rows": unreadable }),
            )?;
        }
        Ok(ClinicReply::Overview(overview))
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
        // And the one anonymous line of the cards that were erased (D-179).
        let diary = rules::diary_with_erased(diary, &tally(v).0).map_err(refusal)?;
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
        // What the cards that were erased counted for stays in the year (D-179).
        let income =
            rules::income_with_erased(income, &tally(v).0, &erased_cards(v).0).map_err(refusal)?;
        Ok(ClinicReply::Income(income))
    }

    /// The income diary of a month (`2026-11`) or a year (`2026`) for the accountant, as a
    /// CSV file (D-194): its name and its bytes, for the shell to write. Only the payments of
    /// the period and the anonymous tally of the erased cards are read, never a card, so no
    /// name can reach the file. A payment of the period, or the tally, that does not read
    /// stops it: a file for the accountant is never made with a row missing. The log says
    /// that the file was made and for which period, nothing more.
    pub fn clinic_income_export(&mut self, period: &str) -> Result<IncomeExport, CoreError> {
        // Locked is said before anything about the request itself.
        self.vault_ref()?;
        let period = rules::parse_income_period(period).map_err(refusal)?;
        let (first, last) = period.bounds();
        let (Some(first), Some(last)) = (rules::day_number(&first), rules::day_number(&last))
        else {
            return refuse("צריך לבחור חודש או שנה לייצוא.");
        };
        let v = self.vault_ref()?;
        let not_read = || CoreError::Refused(EXPORT_NOT_READ.to_owned());
        let rows = v
            .clinic_range_strict(Payment::KIND, first, last)
            .map_err(|e| match e {
                VaultError::Authentication | VaultError::Corrupt(_) => not_read(),
                other => CoreError::Vault(other),
            })?;
        let payments: Vec<Payment> = open_all(rows).map_err(|_| not_read())?;
        let tally = tally_strict(v).map_err(|_| not_read())?;
        let export = rules::income_export(period, &payments, &tally);
        self.vault_mut()?.record(
            AuditEvent::ClinicIncomeExported,
            None,
            &serde_json::json!({ "period": period.key() }),
        )?;
        Ok(export)
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
        let settings = settings(v);
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
        let mut left_alone = 0_u32;
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
            // Which series a visit belongs to is set when the series is made. A reserved time
            // that is given a patient leaves its series: "this and the following" of the other
            // reserved times never reaches a patient's visit.
            visit.series_id.clone_from(&stored.series_id);
            if stored.patient_id.is_none() && visit.patient_id.is_some() {
                visit.series_id = None;
            }
            rules::validate_visit(&visit).map_err(refusal)?;
            if let Some(patient_id) = &visit.patient_id {
                patient_exists(v, patient_id)?;
            }
            // A record never moves to another card (D-177): its history, its charge and the
            // card's balance stay where they were written.
            if stored.patient_id.is_some() && visit.patient_id != stored.patient_id {
                if visit.patient_id.is_none() {
                    return refuse(
                        "אי אפשר להפוך פגישה של מטופל לזמן שמור. אפשר למחוק אותה (או לסמן אותה \
                         כמבוטלת, אם היא כבר סומנה) ולשמור זמן חדש ביומן.",
                    );
                }
                return refuse(
                    "אי אפשר להעביר פגישה למטופל אחר. אם היא נקבעה בכרטיס הלא נכון, אפשר למחוק \
                     אותה (או לסמן אותה כמבוטלת, אם היא כבר סומנה) ולקבוע פגישה חדשה בכרטיס הנכון.",
                );
            }
            let notes: Vec<SessionNote> = all(v)?;
            let noted = noted_of(&notes);
            // Taking a mark back is the correction of a wrong click only (D-176).
            if stored.status != VisitStatus::Planned && visit.status == VisitStatus::Planned {
                let (visits, charges, payments) = match stored.patient_id.as_deref() {
                    Some(patient_id) => {
                        let mine = v.clinic_of_patient(patient_id)?;
                        let of_kind = |kind: &str| -> Vec<StoredClinicRecord> {
                            mine.iter()
                                .filter(|row| row.kind == kind)
                                .cloned()
                                .collect()
                        };
                        (
                            open_all::<Visit>(of_kind(Visit::KIND))?,
                            open_all::<Charge>(of_kind(Charge::KIND))?,
                            open_all::<Payment>(of_kind(Payment::KIND))?,
                        )
                    }
                    None => (Vec::new(), Vec::new(), Vec::new()),
                };
                rules::validate_unmark(
                    &stored,
                    &visit,
                    noted.contains(&stored.id),
                    &visits,
                    &charges,
                    &payments,
                )
                .map_err(refusal)?;
            }
            action = CHANGED;
            let mut changed = Vec::new();
            if visit != stored {
                changed.push(visit.clone());
            }
            if scope == Scope::Following && visit.series_id.is_some() {
                let everything: Vec<Visit> = all(v)?;
                // A later visit that has a note, or is no longer planned, is left alone.
                let later = rules::apply_following(&visit, &stored, &everything, &noted)
                    .map_err(refusal)?;
                left_alone = later.left_alone;
                changed.extend(
                    later
                        .visits
                        .into_iter()
                        .filter(|new| everything.iter().find(|old| old.id == new.id) != Some(new)),
                );
            }
            rows = changed;
        }
        for row in &rows {
            rules::validate_visit(row).map_err(refusal)?;
        }
        // A family on the waiting list that gets a visit is in the process now. The card is
        // checked before anything is written, so that a refusal leaves nothing behind.
        let activated = match &visit.patient_id {
            Some(patient_id) => rules::activated(&patient(self.vault_ref()?, patient_id)?),
            None => None,
        };
        if let Some(active) = &activated {
            rules::patient_known(active).map_err(refusal)?;
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
        if let Some(mut active) = activated {
            write_patient(self.vault_mut()?, &mut active)?;
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
            left_alone,
        }))
    }

    fn clinic_delete_visit(&mut self, id: &str, scope: Scope) -> Result<ClinicReply, CoreError> {
        let v = self.vault_ref()?;
        let visit: Visit = find(v, id)?;
        // A visit that was ever held, cancelled or missed stays, whatever it says now (D-176).
        let marked = !marked_visits(v, std::slice::from_ref(&visit))?.is_empty();
        rules::validate_delete(&visit, marked).map_err(refusal)?;
        let notes: Vec<SessionNote> = all(v)?;
        let noted = noted_of(&notes);
        if noted.contains(id) {
            return refuse("לפגישה הזו יש סיכום, ולכן אי אפשר למחוק אותה. אפשר לסמן אותה כמבוטלת.");
        }
        let mut ids = vec![visit.id.clone()];
        if let (Scope::Following, Some(series)) = (scope, visit.series_id.as_deref()) {
            // The later ones of the series that are still planned; one that already has a
            // record written for it, or that was ever marked, stays. Only the same patient's
            // (or reserved times, for a reserved time): a series never reaches another card.
            let later: Vec<Visit> = all::<Visit>(v)?
                .into_iter()
                .filter(|other| {
                    other.series_id.as_deref() == Some(series)
                        && other.patient_id == visit.patient_id
                        && other.id != visit.id
                        && other.status == VisitStatus::Planned
                        && (other.date.as_str(), other.start) > (visit.date.as_str(), visit.start)
                        && !noted.contains(&other.id)
                })
                .collect();
            let marked = marked_visits(v, &later)?;
            ids.extend(
                later
                    .into_iter()
                    .filter(|other| !marked.contains(&other.id))
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
        // The request carries no "today", so the day is the core's own clock (UTC), as in an
        // erase. Her local day can be one ahead of it, so a birth date is refused only from
        // the day after the core's tomorrow.
        let latest_birth = rules::add_days(&iso(today()), 1);
        rules::validate_patient_on(&patient, latest_birth.as_deref()).map_err(refusal)?;
        // The names of a card that has a case are names its filter must know (D-177), before
        // the card itself is written: a card is never saved with a name its case does not hide.
        if let Some(case_id) = patient.case_id.clone() {
            if self.clinic_cases()?.contains_key(&case_id) {
                self.clinic_sync_identities(&case_id, &patient)?;
            }
        }
        if stored.as_ref() != Some(&patient) {
            write_patient(self.vault_mut()?, &mut patient)?;
        }
        Ok(ClinicReply::Patient(patient))
    }

    /// Erases a card and everything of theirs in the clinic, never the case (D-174): only a
    /// card where nothing happened, or one whose retention period has passed.
    fn clinic_erase_patient(&mut self, id: &str, name: &str) -> Result<ClinicReply, CoreError> {
        // The retention period is counted by the core's own clock, not the window's.
        self.clinic_erase_on(id, name, &iso(today()))
    }

    /// The erase, on a day given by the caller (the tests'; otherwise the core's clock).
    pub(crate) fn clinic_erase_on(
        &mut self,
        id: &str,
        name: &str,
        today: &str,
    ) -> Result<ClinicReply, CoreError> {
        let v = self.vault_ref()?;
        let patient = patient(v, id)?;
        if rules::clean_line(name) != rules::clean_line(&patient.name) {
            return refuse(
                "השם שהוקלד לא זהה לשם שבכרטיס. כדי למחוק צריך להקליד את השם בדיוק כמו שהוא כתוב בכרטיס.",
            );
        }
        let rows = v.clinic_of_patient(id)?;
        let Some((card_saved, counted)) = v.clinic_patient_state(id)? else {
            return Err(CoreError::NotFound(NO_PATIENT.to_owned()));
        };
        let last_saved = last_saved_day(&rows, card_saved);
        let of_kind = |kind: &str| -> Vec<StoredClinicRecord> {
            rows.iter()
                .filter(|row| row.kind == kind)
                .cloned()
                .collect()
        };
        // Every row of the card, or no erase: what is not read cannot be folded or judged.
        let visits: Vec<Visit> = open_all(of_kind(Visit::KIND))?;
        let notes: Vec<SessionNote> = open_all(of_kind(SessionNote::KIND))?;
        let charges: Vec<Charge> = open_all(of_kind(Charge::KIND))?;
        let payments: Vec<Payment> = open_all(of_kind(Payment::KIND))?;
        let tasks: Vec<ClinicTask> = open_all(of_kind(ClinicTask::KIND))?;
        let marked = marked_visits(v, &visits)?;
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
            &marked,
            today,
            last_saved.as_deref(),
            KEEP_YEARS_AFTER_LAST_CHANGE,
            KEEP_UNTIL_AGE,
        )
        .map_err(refusal)?;
        // What the card counted for in the summaries stays, with nothing that says whose it
        // was (D-179). A card where nothing happened was a mistake and leaves nothing. A card
        // an earlier erase already counted (its records went, the card's removal failed) is
        // counted again now, once, since that count was never written: its records are gone,
        // so it no longer looks like a card where something happened.
        let happened = rules::card_happened(&records, &marked);
        let tally = if happened {
            let mut tally = tally_strict(v)?;
            rules::fold_erased(&mut tally, &visits, &charges, &payments);
            Some(to_json(&tally)?)
        } else {
            None
        };
        let cards = if happened || counted {
            let mut cards = cards_strict(v)?;
            rules::fold_erased_card(&mut cards, &patient);
            Some(to_json(&cards)?)
        } else {
            None
        };
        self.vault_mut()?
            .erase_clinic_patient(id, tally.as_deref(), cards.as_deref())?;
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
        // Before the record goes: every name of the card is a name the case's filter knows
        // (D-177). If that cannot be written, nothing goes.
        self.clinic_sync_identities(&case_id, &patient)?;
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
                // The material and the note that points to it are written together (D-177):
                // a failure leaves neither, so asking again never adds the record twice.
                let input_id = dv_vault::crypto::random_id()?;
                note.case_input_id = Some(input_id.clone());
                let row = (
                    Some(note.id.clone()),
                    Some(note.patient_id.clone()),
                    day_of(&note),
                    to_json(&note)?,
                );
                self.vault_mut()?
                    .clinic_note_into_case(&case_id, &input_id, kind, &title, &record, &row)?;
                // As for a material she adds by hand: its new names are kept with the case.
                self.learn_input(&case_id, &title, &record)?;
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
        // Money never moves between cards (D-177): the mistake is voided and written again.
        if stored
            .as_ref()
            .is_some_and(|stored| stored.patient_id != payment.patient_id)
        {
            return refuse(
                "אי אפשר להעביר תקבול למטופל אחר. אפשר לבטל את התקבול הזה, לכתוב למה, \
                 ולרשום תקבול חדש בכרטיס הנכון.",
            );
        }
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
        if stored
            .as_ref()
            .is_some_and(|stored| stored.patient_id != charge.patient_id)
        {
            return refuse(
                "אי אפשר להעביר חיוב למטופל אחר. אפשר לבטל את החיוב הזה, לכתוב למה, \
                 ולרשום חיוב חדש בכרטיס הנכון.",
            );
        }
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
        if settings(self.vault_ref()?) != new {
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
            // The child and the contacts of the card become names the case's filter knows
            // (D-177), before the link is written: no link without them.
            self.clinic_sync_identities(case_id, &patient)?;
        }
        if patient.case_id != case_id {
            patient.case_id = case_id;
            write_patient(self.vault_mut()?, &mut patient)?;
        }
        Ok(ClinicReply::Patient(patient))
    }

    /// Makes every name of a card (the child, each named contact) a name its case's filter
    /// knows (D-177). Called whenever a card is linked to a case, whenever a linked card is
    /// saved, and before a record goes to the case. Names are only ever added: one that left
    /// the card stays known to the case, and what she set in the case herself is kept as it
    /// is. The child's name of the card joins the case's child (as another way the name is
    /// written) when the case already has one; a contact the case does not know is added as
    /// a person of their role. An error here stops the caller: nothing is linked, saved or
    /// sent with a name the filter does not know.
    fn clinic_sync_identities(
        &mut self,
        case_id: &str,
        patient: &Patient,
    ) -> Result<(), CoreError> {
        let mut list: Vec<IdentityInput> = self
            .vault_ref()?
            .identities(case_id)?
            .into_iter()
            .map(|known| IdentityInput {
                id: Some(known.id),
                role: known.role,
                value: known.value,
                aliases: known.aliases,
            })
            .collect();
        let knows = |list: &[IdentityInput], name: &str| {
            list.iter().any(|known| {
                std::iter::once(&known.value)
                    .chain(&known.aliases)
                    .any(|written| rules::clean_line(written) == name)
            })
        };
        let mut changed = false;
        for (role, name) in card_people(patient) {
            if knows(&list, &name) {
                continue;
            }
            changed = true;
            match list.iter_mut().find(|known| known.role == Role::Child) {
                Some(child) if role == Role::Child => child.aliases.push(name),
                _ => list.push(IdentityInput {
                    id: None,
                    role,
                    value: name,
                    aliases: Vec::new(),
                }),
            }
        }
        if changed {
            self.set_identities(case_id, list)?;
        }
        Ok(())
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
        let identities = card_people(&patient)
            .into_iter()
            .map(|(role, value)| IdentityInput {
                id: None,
                role,
                value,
                aliases: Vec::new(),
            })
            .collect();
        let case_id = self.create_case(meta, identities)?;
        patient.case_id = Some(case_id.clone());
        write_patient(self.vault_mut()?, &mut patient)?;
        match self.clinic_cases()?.get(&case_id) {
            Some(case) => Ok(ClinicReply::Case(case.link(&case_id))),
            None => Err(CoreError::NotFound(NO_CASE.to_owned())),
        }
    }
}
