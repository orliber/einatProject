//! Clinic management (beta, D-170): patients and the waiting list, the calendar, session
//! notes, payments and follow-ups.
//!
//! Pure data and rules, like the rest of this crate: no I/O, no clock, no crypto. "Today" is
//! always given by the caller (the window knows her local date; the core's clock is UTC).
//!
//! Conventions of every type here:
//! * a date is ISO `YYYY-MM-DD`, a month `YYYY-MM`; a time of day is minutes from midnight;
//! * money is whole agorot ([`Agorot`]), never a float;
//! * an empty `id` on a record sent to be saved means "a new one";
//! * every record that is stored reads a missing field as its default and ignores a field it
//!   does not know (`#[serde(default)]`): a row written by an earlier version still loads after
//!   a field is added, and one written by a later version loads in the version before (R7);
//! * every enum that is stored has an `Unknown` variant that any value it does not know reads
//!   as (D-178). It is shown, and a record that carries it is never written back.
//!
//! Nothing here is ever sent to an AI. The one way clinic text reaches a case is
//! [`ClinicRequest::NoteToCase`], which copies the formal record of a session (never the
//! personal note) into the case as a material, where the case's own filter applies.

mod holidays;
mod income_export;
mod insights;
mod logic;
mod slots;
#[cfg(test)]
mod tests;

pub use holidays::*;
pub use income_export::*;
pub use insights::*;
pub use logic::*;
pub use slots::*;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::records::{Age, GrammaticalGender};

/// Money in agorot (1 ₪ = 100).
pub type Agorot = i32;

// ------------------------------------------------------------------ patients

/// Where a patient is on the way through the clinic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PatientStatus {
    /// On the waiting list: asked for an assessment, nothing scheduled yet.
    Waiting,
    #[default]
    Active,
    /// The work with the family ended; kept for the record.
    Closed,
    /// A value this version does not know (a row written by a later one, D-178). Shown as
    /// it is, and never saved: a record that carries it is refused until she picks a value.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ContactRole {
    Mother,
    Father,
    Guardian,
    #[default]
    Other,
    /// A value this version does not know (a row written by a later one, D-178). Shown as
    /// it is, and never saved: a record that carries it is refused until she picks a value.
    #[serde(other)]
    Unknown,
}

/// Someone to reach about the patient: a parent, a guardian.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct Contact {
    pub role: ContactRole,
    pub name: String,
    pub phone: String,
    pub email: String,
}

/// A patient card. Kept in the identity database (it is names and phone numbers), sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct Patient {
    pub id: String,
    /// The child's full name.
    pub name: String,
    pub birth_date: Option<String>,
    pub gender: Option<GrammaticalGender>,
    pub contacts: Vec<Contact>,
    /// Kindergarten or school.
    pub framework: String,
    pub referred_by: String,
    /// Why the family came, in a line or two.
    pub reason: String,
    pub notes: String,
    pub status: PatientStatus,
    /// The day the family joined the waiting list.
    pub waiting_since: Option<String>,
    /// Marked by her as one to bring in first.
    pub urgent: bool,
    /// Her usual fee for a session with this family; `None`: the default of the visit kind.
    pub session_fee: Option<Agorot>,
    /// The diagnostic case (the report) of this patient, once there is one.
    pub case_id: Option<String>,
    /// The day the card was opened.
    pub created_on: String,
}

// ------------------------------------------------------------------ the calendar

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum VisitKind {
    /// The first meeting with the parents.
    Intake,
    /// A testing session with the child.
    Assessment,
    /// An observation at the kindergarten or at school.
    Observation,
    /// Giving the findings and the report to the parents.
    Feedback,
    Therapy,
    /// Parent guidance.
    Guidance,
    /// A talk with a professional (a teacher, a therapist), a staff meeting.
    Meeting,
    #[default]
    Other,
    /// Time she blocks for herself: writing reports, a day off. Never has a patient or a fee.
    Block,
    /// A value this version does not know (a row written by a later one, D-178). Shown as
    /// it is, and never saved: a record that carries it is refused until she picks a value.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum VisitStatus {
    #[default]
    Planned,
    Held,
    Cancelled,
    NoShow,
    /// A value this version does not know (a row written by a later one, D-178). Shown as
    /// it is, and never saved: a record that carries it is refused until she picks a value.
    #[serde(other)]
    Unknown,
}

/// One entry in the calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct Visit {
    pub id: String,
    /// `None` only for [`VisitKind::Block`].
    pub patient_id: Option<String>,
    pub kind: VisitKind,
    /// Her own words for it; the calendar shows them instead of the kind's name when set.
    pub title: String,
    pub date: String,
    /// Minutes from midnight.
    pub start: u16,
    /// How long, in minutes.
    pub minutes: u16,
    pub place: String,
    pub status: VisitStatus,
    /// Why it was cancelled, or what happened.
    pub status_note: String,
    /// What a session costs; 0: no charge.
    pub fee: Agorot,
    /// A cancelled or missed session that is charged anyway.
    pub charge_missed: bool,
    /// Set on every visit of a repeating series.
    pub series_id: Option<String>,
    /// A short practical note ("להביא את השאלונים").
    pub note: String,
}

/// Repeat a new visit: every `every_weeks` weeks, `count` visits in all (the first included).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Repeat {
    pub every_weeks: u8,
    pub count: u8,
}

/// What a change to a visit of a series applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Scope {
    One,
    /// This visit and every later one of its series that is still planned.
    Following,
}

// ------------------------------------------------------------------ documentation

/// What she wrote about a session. Two texts, kept apart as the law keeps them apart
/// (Patient's Rights Law 1996, §17; the psychologists' code of ethics, §3.14):
/// the formal record, which the family may ask to see, and her personal note, which is hers
/// alone and is never printed, exported or copied into a case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct SessionNote {
    pub id: String,
    pub patient_id: String,
    /// The visit it documents; `None`: a note on its own (a phone call).
    pub visit_id: Option<String>,
    pub date: String,
    /// The formal record (רשומה).
    pub record: String,
    /// The personal note (תרשומת אישית).
    pub personal: String,
    /// The material in the patient's case that holds a copy of `record`, once she sent it.
    pub case_input_id: Option<String>,
}

// ------------------------------------------------------------------ money

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PayMethod {
    Cash,
    Transfer,
    Bit,
    Paybox,
    Check,
    Card,
    #[default]
    Other,
    /// A value this version does not know (a row written by a later one, D-178). Shown as
    /// it is, and never saved: a record that carries it is refused until she picks a value.
    #[serde(other)]
    Unknown,
}

/// Money received. Never deleted: a mistake is voided, with a reason, and stays in the diary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct Payment {
    pub id: String,
    pub patient_id: String,
    pub date: String,
    pub amount: Agorot,
    pub method: PayMethod,
    /// The number of the receipt she issued in her invoicing program.
    pub receipt: String,
    /// Who paid, when it was not the parents (a fund, an institution).
    pub payer: String,
    pub note: String,
    pub voided: bool,
    pub void_reason: String,
}

/// A charge that is not a session: the assessment as a package, writing the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct Charge {
    pub id: String,
    pub patient_id: String,
    pub date: String,
    pub amount: Agorot,
    pub what: String,
    pub voided: bool,
    pub void_reason: String,
}

/// What a patient was charged, what was received, and what is still owed (`due` may be
/// negative: the family paid ahead).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Balance {
    pub charged: Agorot,
    pub paid: Agorot,
    pub due: Agorot,
}

// ------------------------------------------------------------------ follow-ups

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TaskKind {
    Call,
    /// A questionnaire to send, or one to get back.
    Questionnaire,
    Document,
    Report,
    Payment,
    /// Check in with the family after a while.
    FollowUp,
    #[default]
    Other,
    /// A value this version does not know (a row written by a later one, D-178). Shown as
    /// it is, and never saved: a record that carries it is refused until she picks a value.
    #[serde(other)]
    Unknown,
}

/// Something to do, with or without a patient and a date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct ClinicTask {
    pub id: String,
    pub patient_id: Option<String>,
    pub kind: TaskKind,
    pub title: String,
    pub due: Option<String>,
    /// The day it was done; `None`: still open.
    pub done_on: Option<String>,
    pub note: String,
}

// ------------------------------------------------------------------ settings

/// How long a kind of visit usually is, and what it costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct KindDefault {
    pub kind: VisitKind,
    pub minutes: u16,
    pub fee: Agorot,
}

/// A skeleton for a session record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct NoteTemplate {
    pub name: String,
    pub text: String,
}

/// One step of the assessment pathway (D-198): a visit of this kind, at least `gap_days`
/// after the step before, and at least the next day. The first step is from the day she
/// starts from: its gap is not used, and saved settings keep it at 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct PathwayStep {
    pub kind: VisitKind,
    pub gap_days: u16,
}

impl Default for PathwayStep {
    fn default() -> Self {
        Self {
            kind: VisitKind::Assessment,
            gap_days: 7,
        }
    }
}

/// A follow-up opened for a new family with one click (D-199): its kind, its title, and how
/// many days from the day she opens it it is due.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct StarterTask {
    pub kind: TaskKind,
    pub title: String,
    pub due_days: u16,
}

/// How the clinic works. No personal data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ClinicSettings {
    /// The hours the calendar shows, minutes from midnight.
    pub day_start: u16,
    pub day_end: u16,
    /// Her working days: 0 = Sunday … 6 = Saturday.
    pub work_days: Vec<u8>,
    /// One entry for each [`VisitKind`].
    pub defaults: Vec<KindDefault>,
    pub places: Vec<String>,
    /// The reminder she copies into a message. Placeholders: see [`PLACEHOLDERS`].
    pub reminder_text: String,
    pub payment_text: String,
    pub note_templates: Vec<NoteTemplate>,
    /// Days from the last testing session until the report should be ready.
    pub report_days: u16,
    /// Days a debt may stay open before it is pointed out.
    pub debt_days: u16,
    /// Weeks on the waiting list before a family is pointed out.
    pub waiting_weeks: u16,
    /// Minutes kept free before and after a visit when a free time is looked for (D-197).
    pub buffer_minutes: u16,
    /// Show Israel's holidays in the calendar, and skip the days the clinic is closed for
    /// one when a free time is looked for (D-196).
    pub holidays: bool,
    /// The assessment pathway she schedules with one click (D-198).
    pub pathway: Vec<PathwayStep>,
    /// The price of a whole assessment charged as one package (D-198); 0: no package.
    pub package_fee: Agorot,
    /// The line of the package's charge, e.g. "חבילת אבחון".
    pub package_name: String,
    /// The follow-ups a new family starts with (D-199): questionnaires and documents.
    pub starter_tasks: Vec<StarterTask>,
}

/// The placeholders of a message text, as she types them.
pub const PLACEHOLDERS: &[&str] = &[
    "{הורה}",
    "{ילד}",
    "{יום}",
    "{תאריך}",
    "{שעה}",
    "{מקום}",
    "{סכום}",
    "{שמי}",
];

impl Default for ClinicSettings {
    fn default() -> Self {
        let d = |kind, minutes| KindDefault {
            kind,
            minutes,
            fee: 0,
        };
        let s = |kind, title: &str, due_days| StarterTask {
            kind,
            title: title.to_owned(),
            due_days,
        };
        let t = |name: &str, text: &str| NoteTemplate {
            name: name.to_owned(),
            text: text.to_owned(),
        };
        Self {
            day_start: 8 * 60,
            day_end: 20 * 60,
            work_days: vec![0, 1, 2, 3, 4],
            defaults: vec![
                d(VisitKind::Intake, 60),
                d(VisitKind::Assessment, 90),
                d(VisitKind::Observation, 60),
                d(VisitKind::Feedback, 60),
                d(VisitKind::Therapy, 50),
                d(VisitKind::Guidance, 50),
                d(VisitKind::Meeting, 30),
                d(VisitKind::Other, 50),
                d(VisitKind::Block, 60),
            ],
            places: vec!["הקליניקה".to_owned()],
            // The strictest default: no child's name in a message that leaves by her hand.
            reminder_text: "שלום {הורה}, תזכורת לפגישה ביום {יום}, {תאריך}, בשעה {שעה}. להתראות, {שמי}".to_owned(),
            payment_text: "שלום {הורה}, היתרה לתשלום היא {סכום} ₪. תודה רבה, {שמי}".to_owned(),
            note_templates: vec![
                t(
                    "פגישת היכרות עם ההורים",
                    "סיבת הפנייה:\n\nרקע התפתחותי:\n\nרקע משפחתי:\n\nמסגרת חינוכית:\n\nטיפולים ואבחונים קודמים:\n\nציפיות ההורים מהאבחון:\n",
                ),
                t(
                    "מפגש אבחון",
                    "כלים שהועברו:\n\nהתנהגות במפגש:\n\nשיתוף פעולה וקשב:\n\nרשמים ראשוניים:\n\nלהמשך:\n",
                ),
                t(
                    "תצפית",
                    "מקום התצפית ומשכה:\n\nמה נצפה:\n\nקשר עם ילדים ועם הצוות:\n\nדברי הצוות:\n",
                ),
                t(
                    "שיחת משוב",
                    "מה נמסר להורים:\n\nתגובת ההורים ושאלות שעלו:\n\nהמלצות שסוכמו:\n\nמה נמסר או נשלח:\n",
                ),
            ],
            report_days: 21,
            debt_days: 30,
            waiting_weeks: 6,
            buffer_minutes: 15,
            holidays: true,
            pathway: vec![
                PathwayStep {
                    kind: VisitKind::Intake,
                    gap_days: 0,
                },
                PathwayStep {
                    kind: VisitKind::Assessment,
                    gap_days: 7,
                },
                PathwayStep {
                    kind: VisitKind::Assessment,
                    gap_days: 7,
                },
                PathwayStep {
                    kind: VisitKind::Feedback,
                    gap_days: 21,
                },
            ],
            package_fee: 0,
            package_name: "חבילת אבחון".to_owned(),
            starter_tasks: vec![
                s(TaskKind::Questionnaire, "שאלון להורים", 7),
                s(TaskKind::Questionnaire, "שאלון למורה או לגננת", 14),
                s(TaskKind::Document, "דוח או סיכום מהמסגרת החינוכית", 14),
                s(TaskKind::Document, "אבחונים וסיכומים קודמים", 14),
            ],
        }
    }
}

// ------------------------------------------------------------------ what the screens show

/// The caller's local "now": the window knows her time zone, the core does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Today {
    pub date: String,
    /// Minutes from midnight.
    pub minute: u16,
}

/// A visit as the calendar shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisitView {
    pub visit: Visit,
    pub patient_name: Option<String>,
    /// A session note exists for it.
    pub has_note: bool,
    /// It overlaps another visit that is not cancelled.
    pub overlaps: bool,
}

/// When the next visit of a patient is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisitWhen {
    pub date: String,
    pub start: u16,
    pub kind: VisitKind,
}

/// One row of the patient list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PatientRow {
    pub patient: Patient,
    pub age: Option<Age>,
    pub next_visit: Option<VisitWhen>,
    /// The date of the last visit that was held.
    pub last_visit: Option<String>,
    pub due: Agorot,
    pub open_tasks: u32,
}

/// The diagnostic case of a patient, as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaseLink {
    pub id: String,
    pub code: String,
    /// Report sections approved, out of `total`.
    pub approved: u32,
    pub total: u32,
    pub in_trash: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StepState {
    Done,
    Now,
    Todo,
}

/// One step of an assessment, from the first meeting to the report in the parents' hands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProcessStep {
    /// `intake` | `sessions` | `observation` | `report` | `feedback` | `payment`.
    pub key: String,
    pub label: String,
    pub state: StepState,
    /// "2 מתוך 3 התקיימו", "הדוח: 9 מתוך 16 סעיפים".
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AlertKind {
    /// A visit whose time passed and is still "planned".
    Unmarked,
    /// A session that was held and has no record.
    NoNote,
    /// Money owed for longer than the setting allows.
    Debt,
    /// A family waiting longer than the setting allows.
    Waiting,
    /// The testing ended and the report is not finished within the days she set.
    ReportDue,
    /// An active patient with nothing ahead in the calendar and nothing open to do.
    NoNextVisit,
    TaskOverdue,
    /// The feedback meeting is within a week and the report is not finished.
    FeedbackSoon,
    /// The feedback meeting is within a week and questionnaires or documents of the family
    /// are still open (D-199).
    MaterialsMissing,
    /// Rows of the clinic that could not be read (D-178). They are left as they are, shown
    /// nowhere and counted in no total; the text says how many. On the overview only.
    Unreadable,
}

/// Something the clinic noticed by itself. Computed on every read, never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Alert {
    pub kind: AlertKind,
    pub patient_id: Option<String>,
    pub patient_name: Option<String>,
    pub visit_id: Option<String>,
    pub task_id: Option<String>,
    /// The full sentence shown to her, without the patient's name.
    pub text: String,
    /// The date it is about, for ordering (oldest first).
    pub since: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskView {
    pub task: ClinicTask,
    pub patient_name: Option<String>,
    pub overdue: bool,
}

/// Everything about one patient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PatientCard {
    pub patient: Patient,
    pub age: Option<Age>,
    /// Newest first.
    pub visits: Vec<VisitView>,
    /// Newest first.
    pub notes: Vec<SessionNote>,
    pub payments: Vec<Payment>,
    pub charges: Vec<Charge>,
    pub tasks: Vec<ClinicTask>,
    pub balance: Balance,
    pub process: Vec<ProcessStep>,
    pub case: Option<CaseLink>,
    pub alerts: Vec<Alert>,
}

/// The clinic's front page: today, and what needs her.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Overview {
    pub date: String,
    /// Today's visits, by time.
    pub visits: Vec<VisitView>,
    /// The next working day that has visits, when today has none left.
    pub next_day: Option<String>,
    pub next_visits: Vec<VisitView>,
    /// Open tasks: overdue first, then by date, then the undated.
    pub tasks: Vec<TaskView>,
    pub alerts: Vec<Alert>,
    /// Visits in the week of `date` (Sunday to Saturday) that are not cancelled.
    pub week_visits: u32,
    pub month_received: Agorot,
    pub total_due: Agorot,
    pub waiting: u32,
    pub active: u32,
}

// ------------------------------------------------------------------ the diary and income

/// One line of the day in the income diary ("יומן רופא"): a session, a charge or a payment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiaryLine {
    /// The visit, charge or payment behind the line (for its history).
    pub record_id: String,
    pub time: Option<u16>,
    pub patient_name: String,
    /// "מפגש אבחון", "תשלום", or the charge's own words.
    pub what: String,
    pub status: Option<VisitStatus>,
    pub charged: Agorot,
    pub received: Agorot,
    pub method: Option<PayMethod>,
    pub receipt: String,
    pub payer: String,
    pub note: String,
    pub voided: bool,
    /// It was changed after it was first written (its history has more than one entry).
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiaryDay {
    pub date: String,
    pub lines: Vec<DiaryLine>,
    pub charged: Agorot,
    pub received: Agorot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MethodTotal {
    pub method: PayMethod,
    pub amount: Agorot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiaryMonth {
    pub month: String,
    /// Only days with at least one line, oldest first.
    pub days: Vec<DiaryDay>,
    pub charged: Agorot,
    pub received: Agorot,
    pub by_method: Vec<MethodTotal>,
    /// Payments of the month with no receipt number.
    pub missing_receipts: u32,
    /// What cards that were erased added to this month (D-179): one anonymous line. Its
    /// sums are part of `charged`, `received`, `by_method` and `missing_receipts` above, and
    /// of no day. `None`: nothing of an erased card falls in this month.
    pub erased: Option<DiaryErased>,
}

/// The one line of a month that stands for every card that was erased: totals only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiaryErased {
    /// "רשומות של כרטיסים שנמחקו".
    pub label: String,
    pub held: u32,
    pub cancelled: u32,
    pub no_show: u32,
    pub charged: Agorot,
    pub received: Agorot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IncomeMonth {
    pub month: String,
    pub held: u32,
    pub cancelled: u32,
    pub no_show: u32,
    pub charged: Agorot,
    pub received: Agorot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(default)]
#[ts(export)]
pub struct KindCount {
    pub kind: VisitKind,
    pub count: u32,
}

/// A year at a glance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IncomeYear {
    pub year: u16,
    /// Twelve months, January first.
    pub months: Vec<IncomeMonth>,
    pub charged: Agorot,
    pub received: Agorot,
    pub new_patients: u32,
    /// Visits held, by kind, most frequent first.
    pub by_kind: Vec<KindCount>,
}

/// One saved state of a visit, a charge or a payment. The diary keeps every one of them, so a
/// change made later can always be seen (what a digital income diary is required to show).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HistoryEntry {
    /// Unix seconds.
    #[ts(type = "number")]
    pub at: i64,
    /// `created` | `changed` | `voided` | `deleted`.
    pub action: String,
    /// The state that was saved, in her words, a line for each field.
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MessageKind {
    /// A reminder of a visit (`visit_id` is needed).
    Reminder,
    /// The balance still owed.
    Payment,
}

/// A case that can be linked to a patient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaseChoice {
    pub id: String,
    /// The child's name and the case code, as the case list shows them.
    pub label: String,
}

/// What saving a visit did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SavedVisits {
    /// The visits written (one, or the whole series), by date.
    pub visits: Vec<VisitView>,
    /// How many of them overlap another visit.
    pub overlapping: u32,
    /// With `Scope::Following`: later visits of the series that were left exactly as they
    /// were, because they have a session note or are no longer planned (D-176).
    pub left_alone: u32,
}

// ------------------------------------------------------------------ what an erased card leaves

/// What one month keeps of the cards that were erased (D-179): counts and sums only, the
/// ones the yearly summary and the income diary show. No id, no name, no date finer than
/// the month and no free text: there is no field here that could hold one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ErasedMonth {
    /// Visits that were held, by kind.
    pub held: Vec<KindCount>,
    pub cancelled: u32,
    pub no_show: u32,
    /// Visit charges and charges that were not voided, in agorot.
    pub charged: i64,
    /// Payments that were not voided, by method, in agorot.
    pub received: Vec<ErasedMethod>,
    /// Payments that were not voided and carried no receipt number.
    pub missing_receipts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ErasedMethod {
    pub method: PayMethod,
    pub amount: i64,
}

/// The anonymous tally of every card that was erased, by month (`YYYY-MM`). Kept sealed in
/// the main database, next to the records it replaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ErasedTally {
    pub months: std::collections::BTreeMap<String, ErasedMonth>,
}

/// How many cards that were erased had been opened in each month (`YYYY-MM`): the yearly
/// summary's "new patients". Kept sealed in the identity database, next to the cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ErasedCards {
    pub months: std::collections::BTreeMap<String, u32>,
}

// ------------------------------------------------------------------ the one command

/// Everything the window can ask of the clinic. One command carries them all (D-170), so the
/// shell's allowlists grow by one line and the whole module sits behind one door.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum ClinicRequest {
    Overview {
        today: Today,
    },
    /// Visits from `from` to `to`, both included.
    Calendar {
        from: String,
        to: String,
    },
    /// `repeat` only with a new visit. `scope` matters only for a visit of a series.
    SaveVisit {
        visit: Visit,
        repeat: Option<Repeat>,
        scope: Scope,
    },
    /// Removes a visit that is planned and was never anything else (D-176). One that
    /// happened, or was ever marked, is cancelled, not removed.
    DeleteVisit {
        id: String,
        scope: Scope,
    },
    Patients {
        today: Today,
    },
    PatientCard {
        id: String,
        today: Today,
    },
    SavePatient {
        patient: Patient,
    },
    /// Erases the patient and everything of theirs in the clinic (never the case). `name`
    /// must be the patient's name as typed again by her. What the card counted for in the
    /// summaries stays as anonymous totals (D-179).
    ErasePatient {
        id: String,
        name: String,
    },
    SaveNote {
        note: SessionNote,
    },
    DeleteNote {
        id: String,
    },
    /// Copies the formal record into the patient's case as a material.
    NoteToCase {
        id: String,
    },
    SavePayment {
        payment: Payment,
    },
    VoidPayment {
        id: String,
        reason: String,
    },
    SaveCharge {
        charge: Charge,
    },
    VoidCharge {
        id: String,
        reason: String,
    },
    Tasks {
        today: Today,
    },
    SaveTask {
        task: ClinicTask,
    },
    DeleteTask {
        id: String,
    },
    /// `month`: `YYYY-MM`.
    Diary {
        month: String,
    },
    Income {
        year: u16,
    },
    History {
        record_id: String,
    },
    Settings,
    SaveSettings {
        settings: ClinicSettings,
    },
    /// The text of a message to the parents, filled in, for her to copy.
    Message {
        kind: MessageKind,
        patient_id: String,
        visit_id: Option<String>,
    },
    /// Cases that are not linked to any patient yet.
    CaseChoices,
    /// `case_id` = `None` unlinks.
    LinkCase {
        patient_id: String,
        case_id: Option<String>,
    },
    /// Opens a new diagnostic case for the patient, with the child's name and age.
    CreateCase {
        patient_id: String,
        today: Today,
    },
    /// Israel's holidays from `from` to `to`, both included (D-196).
    Holidays {
        from: String,
        to: String,
    },
    /// Free times of `minutes` from `from` on (D-197), at most `count`. `skip_visit`: the
    /// visit being moved, whose own time is not in the way.
    FreeSlots {
        today: Today,
        from: String,
        minutes: u16,
        count: u8,
        skip_visit: Option<String>,
    },
    /// The visits of the assessment pathway, each at the first free time after the step
    /// before (D-198). Nothing is saved: she looks, changes, and then saves.
    ProposePathway {
        patient_id: String,
        today: Today,
        from: String,
    },
    /// Saves the pathway's visits in one go. With `package`, each visit is free and one
    /// charge of the package's price is written with them, all or nothing (D-198).
    SavePathway {
        patient_id: String,
        visits: Vec<Visit>,
        package: bool,
        today: Today,
    },
    /// Opens the starter follow-ups for the patient (D-199); one with the same title that
    /// is still open is not opened twice.
    StarterTasks {
        patient_id: String,
        today: Today,
    },
    /// The picture of the clinic over the months `from_month` to `to_month` (D-200).
    Insights {
        today: Today,
        from_month: String,
        to_month: String,
    },
}

/// The answer to each [`ClinicRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum ClinicReply {
    Overview(Overview),
    Visits(Vec<VisitView>),
    Saved(SavedVisits),
    Patients(Vec<PatientRow>),
    Card(Box<PatientCard>),
    Patient(Patient),
    Note(SessionNote),
    Payment(Payment),
    Charge(Charge),
    Tasks(Vec<TaskView>),
    Task(ClinicTask),
    Diary(DiaryMonth),
    Income(IncomeYear),
    History(Vec<HistoryEntry>),
    Settings(ClinicSettings),
    Text(String),
    Cases(Vec<CaseChoice>),
    Case(CaseLink),
    Holidays(Vec<Holiday>),
    Slots(Vec<Slot>),
    Proposed(Vec<Visit>),
    Insights(Box<Insights>),
    Done,
}
