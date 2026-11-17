//! The rules of the clinic: pure functions over the records in [`super`].
//!
//! The single place for them (docs/CLINIC.md, "Rules"): validation, calendar arithmetic,
//! repeating series, overlaps, charges and balances, alerts, the assessment process, the
//! income diary, message filling, Hebrew labels and history lines. No I/O, no clock, no
//! randomness: ids and "today" are given by the caller.
//!
//! Every refusal is a Hebrew sentence she can act on, in [`DomainError::Invalid`].

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::{
    Agorot, Alert, AlertKind, Balance, Charge, ClinicSettings, ClinicTask, Contact, ContactRole,
    DiaryDay, DiaryErased, DiaryLine, DiaryMonth, ErasedCards, ErasedMethod, ErasedMonth,
    ErasedTally, IncomeMonth, IncomeYear, KindCount, KindDefault, MethodTotal, Patient,
    PatientStatus, PayMethod, Payment, ProcessStep, Repeat, SessionNote, StepState, TaskKind,
    TaskView, Today, Visit, VisitKind, VisitStatus, VisitView, VisitWhen,
};
use crate::dates::{
    add_years, civil_from_days, days_from_civil, days_in_month, iso, parse_iso, Ymd,
};
use crate::records::Age;
use crate::DomainError;

// ------------------------------------------------------------------ limits

/// A person's name, in chars.
pub const NAME_MAX: usize = 120;
/// A one-line field: a place, a title, a receipt number, a phone, a reason.
pub const SHORT_MAX: usize = 200;
/// A free note: the reason for coming, notes on a patient, a task or a visit.
pub const LONG_MAX: usize = 4_000;
/// The formal record and the personal note of a session.
pub const NOTE_MAX: usize = 20_000;
/// The reminder and the payment message.
pub const MESSAGE_MAX: usize = 1_000;
pub const MAX_CONTACTS: usize = 6;
pub const MAX_PLACES: usize = 12;
pub const MAX_TEMPLATES: usize = 20;
pub const TEMPLATE_MAX: usize = 5_000;
pub const MIN_MINUTES: u16 = 5;
pub const MAX_MINUTES: u16 = 720;
/// Minutes in a day: a visit ends by midnight.
pub const DAY_MINUTES: u16 = 1_440;
/// The highest fee and the highest single amount: 100,000 ₪.
pub const MAX_AMOUNT: Agorot = 10_000_000;
pub const MIN_EVERY_WEEKS: u8 = 1;
pub const MAX_EVERY_WEEKS: u8 = 8;
pub const MIN_REPEAT_COUNT: u8 = 2;
pub const MAX_REPEAT_COUNT: u8 = 52;
/// The longest stretch the calendar may ask for, both ends included.
pub const MAX_RANGE_DAYS: i64 = 100;
/// A held session without a record is pointed out for this long.
pub const NO_NOTE_DAYS: i64 = 60;
/// A feedback meeting this close is "soon".
pub const FEEDBACK_SOON_DAYS: i64 = 7;
/// An active patient with nothing ahead is pointed out after this many days.
pub const NO_NEXT_VISIT_DAYS: i64 = 14;

/// Every kind of visit, in the order the screens list them.
pub const VISIT_KINDS: [VisitKind; 9] = [
    VisitKind::Intake,
    VisitKind::Assessment,
    VisitKind::Observation,
    VisitKind::Feedback,
    VisitKind::Therapy,
    VisitKind::Guidance,
    VisitKind::Meeting,
    VisitKind::Other,
    VisitKind::Block,
];

/// Every way of paying, in the order the screens list them.
pub const PAY_METHODS: [PayMethod; 7] = [
    PayMethod::Cash,
    PayMethod::Transfer,
    PayMethod::Bit,
    PayMethod::Paybox,
    PayMethod::Check,
    PayMethod::Card,
    PayMethod::Other,
];

// ------------------------------------------------------------------ small helpers

fn refuse<T>(text: impl Into<String>) -> Result<T, DomainError> {
    Err(DomainError::Invalid(text.into()))
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// Length as she sees it: chars, not bytes (a Hebrew letter is two bytes).
fn at_most(text: &str, max: usize, field: &str) -> Result<(), DomainError> {
    if text.chars().count() > max {
        return refuse(format!(
            "יש יותר מדי טקסט בשדה \"{field}\". אפשר לכתוב עד {max} תווים."
        ));
    }
    Ok(())
}

fn real_date(text: &str, field: &str) -> Result<(), DomainError> {
    if parse_date(text).is_none() {
        return refuse(format!("צריך לבחור תאריך תקין בשדה \"{field}\"."));
    }
    Ok(())
}

fn amount_ok(amount: Agorot) -> Result<(), DomainError> {
    if !(1..=MAX_AMOUNT).contains(&amount) {
        return refuse("הסכום צריך להיות גדול מאפס, ועד 100,000 ₪.");
    }
    Ok(())
}

/// A value of a later version (`Unknown`, D-178) is shown and never written back: saving
/// is refused until she picks one this version knows.
fn known(unknown: bool, field: &str) -> Result<(), DomainError> {
    if unknown {
        return refuse(format!(
            "בשדה \"{field}\" יש ערך שהגרסה הזו של התוכנה לא מכירה. \
             כדי לשמור צריך לבחור ערך מהרשימה, או לעדכן את התוכנה."
        ));
    }
    Ok(())
}

/// A visit holds no value of a later version (D-178).
pub fn visit_known(visit: &Visit) -> Result<(), DomainError> {
    known(visit.kind == VisitKind::Unknown, "סוג הפגישה")?;
    known(visit.status == VisitStatus::Unknown, "מצב הפגישה")
}

/// A payment holds no value of a later version (D-178).
pub fn payment_known(payment: &Payment) -> Result<(), DomainError> {
    known(payment.method == PayMethod::Unknown, "אמצעי התשלום")
}

/// A task holds no value of a later version (D-178).
pub fn task_known(task: &ClinicTask) -> Result<(), DomainError> {
    known(task.kind == TaskKind::Unknown, "סוג המשימה")
}

/// A card holds no value of a later version (D-178).
pub fn patient_known(patient: &Patient) -> Result<(), DomainError> {
    known(patient.status == PatientStatus::Unknown, "מצב הכרטיס")?;
    patient
        .contacts
        .iter()
        .try_for_each(|contact| known(contact.role == ContactRole::Unknown, "הקשר לילד"))
}

fn patient_chosen(patient_id: &str) -> Result<(), DomainError> {
    if blank(patient_id) {
        return refuse("צריך לבחור מטופל.");
    }
    Ok(())
}

/// A voided record says why; a live one may still carry an old reason of any length allowed.
fn void_ok(voided: bool, reason: &str) -> Result<(), DomainError> {
    at_most(reason, SHORT_MAX, "סיבת הביטול")?;
    if voided && blank(reason) {
        return refuse("צריך לכתוב למה הרישום מבוטל.");
    }
    Ok(())
}

/// A sum that does not fit is refused, never wrapped or clamped into a wrong figure.
fn to_agorot(total: i64) -> Result<Agorot, DomainError> {
    Agorot::try_from(total)
        .or_else(|_| refuse("הסכום הכולל גדול מדי לחישוב. כדאי לבדוק את הסכומים שנרשמו."))
}

/// One line, single spaces: what a name, a phone or a place is stored as.
#[must_use]
pub fn clean_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

// ------------------------------------------------------------------ dates

/// An ISO date, strictly: ten chars of digits and hyphens that name a real day.
/// (`dates::parse_iso` alone lets a signed part such as `+9` through.)
#[must_use]
pub fn parse_date(text: &str) -> Option<Ymd> {
    let shaped = text.len() == 10 && text.bytes().all(|b| b.is_ascii_digit() || b == b'-');
    if shaped {
        parse_iso(text)
    } else {
        None
    }
}

/// The first year a birth date may carry. Every other date of the clinic is one of its own
/// days (2000 onward, as `dates::parse_iso` has it); a birth date is older than the clinic:
/// an older sibling, or an adult child whose parent came to consult.
pub const BIRTH_YEAR_MIN: i32 = 1900;

/// A birth date: an ISO date, as strictly as [`parse_date`], from [`BIRTH_YEAR_MIN`] on.
#[must_use]
pub fn parse_birth_date(text: &str) -> Option<Ymd> {
    if let Some(date) = parse_date(text) {
        return Some(date);
    }
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        });
    if !shaped {
        return None;
    }
    let y = text.get(0..4)?.parse::<i32>().ok()?;
    let m = text.get(5..7)?.parse::<u32>().ok()?;
    let d = text.get(8..10)?.parse::<u32>().ok()?;
    // From 2000 on `parse_date` has already answered; here only the years before it.
    ((BIRTH_YEAR_MIN..2000).contains(&y) && (1..=days_in_month(y, m)).contains(&d))
        .then_some((y, m, d))
}

/// A birth date she can save: a real date from [`BIRTH_YEAR_MIN`] on, and not after `today`
/// when the caller knows the day (a `today` that is not a date is "not known").
pub fn validate_birth_date(birth: &str, today: Option<&str>) -> Result<(), DomainError> {
    let Some(born) = parse_birth_date(birth) else {
        return refuse(format!(
            "צריך לכתוב תאריך לידה תקין, משנת {BIRTH_YEAR_MIN} ואילך."
        ));
    };
    if today.and_then(parse_date).is_some_and(|now| born > now) {
        return refuse("תאריך הלידה עוד לא הגיע. כדאי לבדוק את השנה שנכתבה.");
    }
    Ok(())
}

/// Days since 1970-01-01 of an ISO date.
#[must_use]
pub fn day_number(date: &str) -> Option<i64> {
    parse_date(date).map(days_from_civil)
}

/// The ISO date of a day number; `None` outside the years a date may have (2000–2200).
#[must_use]
pub fn date_of_day(day: i64) -> Option<String> {
    let first = days_from_civil((2000, 1, 1));
    let last = days_from_civil((2200, 12, 31));
    (first..=last)
        .contains(&day)
        .then(|| iso(civil_from_days(day)))
}

/// 0 = Sunday … 6 = Saturday.
#[must_use]
pub fn weekday(date: &str) -> Option<u8> {
    // 1970-01-01 was a Thursday.
    day_number(date).and_then(|day| u8::try_from((day + 4).rem_euclid(7)).ok())
}

/// "ראשון" … "שבת" for 0 … 6; empty for anything else.
#[must_use]
pub fn day_name(weekday: u8) -> &'static str {
    match weekday {
        0 => "ראשון",
        1 => "שני",
        2 => "שלישי",
        3 => "רביעי",
        4 => "חמישי",
        5 => "שישי",
        6 => "שבת",
        _ => "",
    }
}

/// The Hebrew day name of a date; empty when the date is not one.
#[must_use]
pub fn day_name_of(date: &str) -> &'static str {
    weekday(date).map_or("", day_name)
}

/// `2026-11-15` → `15.11.2026`. A text that is not a date is shown as it is.
#[must_use]
pub fn display_date(date: &str) -> String {
    match parse_date(date) {
        Some((y, m, d)) => format!("{d:02}.{m:02}.{y:04}"),
        None => date.to_owned(),
    }
}

/// Minutes from midnight → `09:30`.
#[must_use]
pub fn display_time(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The date `days` later (earlier when negative); `None` when either end is not a date.
#[must_use]
pub fn add_days(date: &str, days: i64) -> Option<String> {
    day_number(date)?.checked_add(days).and_then(date_of_day)
}

/// How many days from `from` to `to` (negative when `to` is earlier).
#[must_use]
pub fn days_between(from: &str, to: &str) -> Option<i64> {
    Some(day_number(to)? - day_number(from)?)
}

/// The Sunday and the Saturday of the week of `date`. At the very edge of the supported
/// years an end may fall a few days outside them; it is still a correct ISO date.
#[must_use]
pub fn week_of(date: &str) -> Option<(String, String)> {
    let day = day_number(date)?;
    let sunday = day - i64::from(weekday(date)?);
    Some((
        iso(civil_from_days(sunday)),
        iso(civil_from_days(sunday + 6)),
    ))
}

/// `2026-11` → (2026, 11).
#[must_use]
pub fn parse_month(month: &str) -> Option<(i32, u32)> {
    let shaped = month.len() == 7 && month.bytes().all(|b| b.is_ascii_digit() || b == b'-');
    let (y, m) = month.split_once('-')?;
    if !shaped || y.len() != 4 || m.len() != 2 {
        return None;
    }
    let (y, m) = (y.parse::<i32>().ok()?, m.parse::<u32>().ok()?);
    ((2000..=2200).contains(&y) && (1..=12).contains(&m)).then_some((y, m))
}

/// The first and the last day of a month.
#[must_use]
pub fn month_bounds(month: &str) -> Option<(String, String)> {
    let (y, m) = parse_month(month)?;
    Some((iso((y, m, 1)), iso((y, m, days_in_month(y, m)))))
}

/// The `YYYY-MM` a date belongs to.
#[must_use]
pub fn month_of(date: &str) -> Option<String> {
    parse_date(date).map(|(y, m, _)| format!("{y:04}-{m:02}"))
}

/// The patient's age on `today`. `None` when the birth date is missing, not a birth date
/// ([`parse_birth_date`]), in the future, or the age does not fit the [`Age`] type.
#[must_use]
pub fn age_on(birth_date: Option<&str>, today: &str) -> Option<Age> {
    let (by, bm, bd) = parse_birth_date(birth_date?)?;
    let (ty, tm, td) = parse_date(today)?;
    if (by, bm, bd) > (ty, tm, td) {
        return None;
    }
    let mut months = i64::from(ty - by) * 12 + i64::from(tm) - i64::from(bm);
    // A month is complete on the birth day, or on the month's last day when it is shorter
    // (born on the 31st: complete on the 30th of a 30-day month), as `dates::add_years` does.
    if td < bd.min(days_in_month(ty, tm)) {
        months -= 1;
    }
    Some(Age {
        years: u8::try_from(months.div_euclid(12)).ok()?,
        months: u8::try_from(months.rem_euclid(12)).ok()?,
    })
}

// ------------------------------------------------------------------ normalizing

/// The card as it is stored: names and phones on one line with single spaces, free text
/// trimmed, empty optional fields gone, and contacts with nothing in them dropped.
#[must_use]
pub fn normalize_patient(mut patient: Patient) -> Patient {
    patient.name = clean_line(&patient.name);
    patient.birth_date = none_if_blank(patient.birth_date);
    patient.contacts = patient
        .contacts
        .into_iter()
        .map(normalize_contact)
        .filter(|c| !(c.name.is_empty() && c.phone.is_empty() && c.email.is_empty()))
        .collect();
    patient.framework = clean_line(&patient.framework);
    patient.referred_by = clean_line(&patient.referred_by);
    patient.reason = patient.reason.trim().to_owned();
    patient.notes = patient.notes.trim().to_owned();
    patient.waiting_since = none_if_blank(patient.waiting_since);
    patient.case_id = none_if_blank(patient.case_id);
    patient.created_on = patient.created_on.trim().to_owned();
    patient
}

#[must_use]
pub fn normalize_contact(mut contact: Contact) -> Contact {
    contact.name = clean_line(&contact.name);
    contact.phone = clean_line(&contact.phone);
    contact.email = contact.email.trim().to_owned();
    contact
}

#[must_use]
pub fn normalize_visit(mut visit: Visit) -> Visit {
    visit.patient_id = none_if_blank(visit.patient_id);
    visit.title = clean_line(&visit.title);
    visit.date = visit.date.trim().to_owned();
    visit.place = clean_line(&visit.place);
    visit.status_note = clean_line(&visit.status_note);
    visit.series_id = none_if_blank(visit.series_id);
    visit.note = visit.note.trim().to_owned();
    visit
}

#[must_use]
pub fn normalize_note(mut note: SessionNote) -> SessionNote {
    note.visit_id = none_if_blank(note.visit_id);
    note.date = note.date.trim().to_owned();
    note.record = note.record.trim().to_owned();
    note.personal = note.personal.trim().to_owned();
    note.case_input_id = none_if_blank(note.case_input_id);
    note
}

#[must_use]
pub fn normalize_payment(mut payment: Payment) -> Payment {
    payment.date = payment.date.trim().to_owned();
    payment.receipt = clean_line(&payment.receipt);
    payment.payer = clean_line(&payment.payer);
    payment.note = payment.note.trim().to_owned();
    payment.void_reason = clean_line(&payment.void_reason);
    payment
}

#[must_use]
pub fn normalize_charge(mut charge: Charge) -> Charge {
    charge.date = charge.date.trim().to_owned();
    charge.what = clean_line(&charge.what);
    charge.void_reason = clean_line(&charge.void_reason);
    charge
}

#[must_use]
pub fn normalize_task(mut task: ClinicTask) -> ClinicTask {
    task.patient_id = none_if_blank(task.patient_id);
    task.title = clean_line(&task.title);
    task.due = none_if_blank(task.due);
    task.done_on = none_if_blank(task.done_on);
    task.note = task.note.trim().to_owned();
    task
}

/// Working days sorted and without repeats; places and template names on one line, the
/// empty and the repeated ones dropped.
#[must_use]
pub fn normalize_settings(mut settings: ClinicSettings) -> ClinicSettings {
    settings.work_days.sort_unstable();
    settings.work_days.dedup();
    let mut places: Vec<String> = Vec::new();
    for place in settings.places.iter().map(|p| clean_line(p)) {
        if !place.is_empty() && !places.contains(&place) {
            places.push(place);
        }
    }
    settings.places = places;
    settings.reminder_text = settings.reminder_text.trim().to_owned();
    settings.payment_text = settings.payment_text.trim().to_owned();
    for template in &mut settings.note_templates {
        template.name = clean_line(&template.name);
    }
    settings.package_name = clean_line(&settings.package_name);
    for task in &mut settings.starter_tasks {
        task.title = clean_line(&task.title);
    }
    settings
}

// ------------------------------------------------------------------ validation

/// The card by itself, with no "today": a birth date in the future is not refused here.
/// The service uses [`validate_patient_on`].
pub fn validate_patient(patient: &Patient) -> Result<(), DomainError> {
    validate_patient_on(patient, None)
}

/// The card, on a day the caller knows: a birth date after `today` is refused.
pub fn validate_patient_on(patient: &Patient, today: Option<&str>) -> Result<(), DomainError> {
    if blank(&patient.name) {
        return refuse("צריך לכתוב את שם הילד או הילדה.");
    }
    at_most(patient.name.trim(), NAME_MAX, "שם")?;
    known(patient.status == PatientStatus::Unknown, "מצב הכרטיס")?;
    if let Some(birth) = &patient.birth_date {
        validate_birth_date(birth, today)?;
    }
    if patient.contacts.len() > MAX_CONTACTS {
        return refuse(format!("אפשר לשמור עד {MAX_CONTACTS} אנשי קשר למטופל."));
    }
    for contact in &patient.contacts {
        validate_contact(contact)?;
    }
    at_most(&patient.framework, SHORT_MAX, "מסגרת חינוכית")?;
    at_most(&patient.referred_by, SHORT_MAX, "גורם מפנה")?;
    at_most(&patient.reason, LONG_MAX, "סיבת הפנייה")?;
    at_most(&patient.notes, LONG_MAX, "הערות")?;
    if let Some(since) = &patient.waiting_since {
        real_date(since, "ממתינים מאז")?;
    }
    if let Some(fee) = patient.session_fee {
        fee_ok(fee)?;
    }
    real_date(&patient.created_on, "תאריך פתיחת הכרטיס")
}

pub fn validate_contact(contact: &Contact) -> Result<(), DomainError> {
    known(contact.role == ContactRole::Unknown, "הקשר לילד")?;
    if blank(&contact.name) && blank(&contact.phone) && blank(&contact.email) {
        return refuse("לאיש קשר צריך שם, טלפון או כתובת מייל.");
    }
    at_most(contact.name.trim(), NAME_MAX, "שם איש הקשר")?;
    at_most(&contact.phone, SHORT_MAX, "טלפון")?;
    at_most(&contact.email, SHORT_MAX, "מייל")
}

fn fee_ok(fee: Agorot) -> Result<(), DomainError> {
    if !(0..=MAX_AMOUNT).contains(&fee) {
        return refuse("מחיר הפגישה צריך להיות בין 0 ל-100,000 ₪.");
    }
    Ok(())
}

fn length_ok(start: u16, minutes: u16) -> Result<(), DomainError> {
    if !(MIN_MINUTES..=MAX_MINUTES).contains(&minutes) {
        return refuse("משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
    }
    if u32::from(start) + u32::from(minutes) > u32::from(DAY_MINUTES) {
        return refuse("הפגישה צריכה להסתיים עד חצות. אפשר לקצר אותה או להקדים את שעת ההתחלה.");
    }
    Ok(())
}

/// The visit by itself. That the patient exists is for the service to check.
pub fn validate_visit(visit: &Visit) -> Result<(), DomainError> {
    visit_known(visit)?;
    let has_patient = visit.patient_id.as_deref().is_some_and(|id| !blank(id));
    if visit.kind == VisitKind::Block {
        if has_patient {
            return refuse(
                "זמן שמור הוא בלי מטופל. כדי לקבוע פגישה עם מטופל צריך לבחור סוג פגישה אחר.",
            );
        }
        if visit.fee != 0 {
            return refuse("לזמן שמור אין מחיר.");
        }
    } else if !has_patient {
        return refuse("צריך לבחור מטופל לפגישה.");
    }
    real_date(&visit.date, "תאריך")?;
    length_ok(visit.start, visit.minutes)?;
    fee_ok(visit.fee)?;
    at_most(&visit.title, SHORT_MAX, "כותרת")?;
    at_most(&visit.place, SHORT_MAX, "מקום")?;
    at_most(&visit.status_note, SHORT_MAX, "מה קרה")?;
    at_most(&visit.note, LONG_MAX, "הערה")
}

pub fn validate_repeat(repeat: Repeat) -> Result<(), DomainError> {
    if !(MIN_EVERY_WEEKS..=MAX_EVERY_WEEKS).contains(&repeat.every_weeks) {
        return refuse("פגישה חוזרת יכולה לחזור כל שבוע ועד כל 8 שבועות.");
    }
    if !(MIN_REPEAT_COUNT..=MAX_REPEAT_COUNT).contains(&repeat.count) {
        return refuse("סדרה של פגישות חוזרות היא בין 2 ל-52 פגישות.");
    }
    Ok(())
}

pub fn validate_note(note: &SessionNote) -> Result<(), DomainError> {
    patient_chosen(&note.patient_id)?;
    real_date(&note.date, "תאריך")?;
    if blank(&note.record) && blank(&note.personal) {
        return refuse("הסיכום ריק. צריך לכתוב משהו ברשומה או בתרשומת האישית.");
    }
    at_most(&note.record, NOTE_MAX, "רשומה")?;
    at_most(&note.personal, NOTE_MAX, "תרשומת אישית")
}

pub fn validate_payment(payment: &Payment) -> Result<(), DomainError> {
    patient_chosen(&payment.patient_id)?;
    payment_known(payment)?;
    real_date(&payment.date, "תאריך")?;
    amount_ok(payment.amount)?;
    at_most(&payment.receipt, SHORT_MAX, "מספר קבלה")?;
    at_most(&payment.payer, SHORT_MAX, "מי שילם")?;
    at_most(&payment.note, LONG_MAX, "הערה")?;
    void_ok(payment.voided, &payment.void_reason)
}

pub fn validate_charge(charge: &Charge) -> Result<(), DomainError> {
    patient_chosen(&charge.patient_id)?;
    real_date(&charge.date, "תאריך")?;
    amount_ok(charge.amount)?;
    if blank(&charge.what) {
        return refuse("צריך לכתוב על מה החיוב.");
    }
    at_most(&charge.what, SHORT_MAX, "על מה החיוב")?;
    void_ok(charge.voided, &charge.void_reason)
}

/// The reason given when a payment or a charge is voided.
pub fn validate_void_reason(reason: &str) -> Result<(), DomainError> {
    void_ok(true, reason)
}

pub fn validate_task(task: &ClinicTask) -> Result<(), DomainError> {
    if blank(&task.title) {
        return refuse("צריך לכתוב מה המשימה.");
    }
    at_most(task.title.trim(), SHORT_MAX, "משימה")?;
    task_known(task)?;
    if let Some(due) = &task.due {
        real_date(due, "תאריך יעד")?;
    }
    if let Some(done) = &task.done_on {
        real_date(done, "תאריך ביצוע")?;
    }
    at_most(&task.note, LONG_MAX, "הערה")
}

pub fn validate_settings(settings: &ClinicSettings) -> Result<(), DomainError> {
    if settings.day_start >= settings.day_end || settings.day_end > DAY_MINUTES {
        return refuse("שעת הסיום של היום ביומן צריכה להיות אחרי שעת ההתחלה, ועד חצות.");
    }
    if settings.work_days.is_empty() {
        return refuse("צריך לבחור לפחות יום עבודה אחד.");
    }
    let days: BTreeSet<u8> = settings.work_days.iter().copied().collect();
    if days.len() != settings.work_days.len() || days.iter().any(|d| *d > 6) {
        return refuse("ימי העבודה לא נשמרו כמו שצריך. כדאי לסמן אותם מחדש.");
    }
    let one_each = VISIT_KINDS
        .iter()
        .all(|kind| settings.defaults.iter().filter(|d| d.kind == *kind).count() == 1);
    if !one_each || settings.defaults.len() != VISIT_KINDS.len() {
        return refuse("לכל סוג פגישה צריכה להיות שורה אחת של משך ומחיר.");
    }
    for default in &settings.defaults {
        if !(MIN_MINUTES..=MAX_MINUTES).contains(&default.minutes) {
            return refuse("משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
        }
        fee_ok(default.fee)?;
        if default.kind == VisitKind::Block && default.fee != 0 {
            return refuse("לזמן שמור אין מחיר.");
        }
    }
    if settings.places.len() > MAX_PLACES {
        return refuse(format!("אפשר לשמור עד {MAX_PLACES} מקומות."));
    }
    for place in &settings.places {
        if blank(place) {
            return refuse("יש מקום בלי שם. אפשר למחוק אותו או לתת לו שם.");
        }
        at_most(place, SHORT_MAX, "מקום")?;
    }
    at_most(&settings.reminder_text, MESSAGE_MAX, "נוסח התזכורת")?;
    at_most(&settings.payment_text, MESSAGE_MAX, "נוסח הודעת התשלום")?;
    if settings.note_templates.len() > MAX_TEMPLATES {
        return refuse(format!("אפשר לשמור עד {MAX_TEMPLATES} תבניות לסיכום."));
    }
    for template in &settings.note_templates {
        if blank(&template.name) {
            return refuse("צריך לתת שם לכל תבנית של סיכום.");
        }
        at_most(template.name.trim(), SHORT_MAX, "שם התבנית")?;
        at_most(&template.text, TEMPLATE_MAX, "תבנית לסיכום")?;
    }
    if settings.buffer_minutes > super::MAX_BUFFER {
        return refuse(format!(
            "הזמן הפנוי בין פגישות יכול להיות עד {} דקות.",
            super::MAX_BUFFER
        ));
    }
    fee_ok(settings.package_fee)?;
    at_most(&settings.package_name, SHORT_MAX, "שם החבילה")?;
    if settings.package_fee > 0 && blank(&settings.package_name) {
        return refuse("צריך לתת שם לחבילת האבחון, כדי שיופיע בחיוב.");
    }
    super::validate_pathway(settings)?;
    super::validate_starter_tasks(settings)?;
    Ok(())
}

/// A month as the diary asks for it: `YYYY-MM`.
pub fn validate_month(month: &str) -> Result<(), DomainError> {
    if parse_month(month).is_none() {
        return refuse("צריך לבחור חודש.");
    }
    Ok(())
}

/// A year the income summary can show.
pub fn validate_year(year: u16) -> Result<(), DomainError> {
    if !(2000..=2200).contains(&year) {
        return refuse("צריך לבחור שנה.");
    }
    Ok(())
}

/// The stretch the calendar asks for: two real dates in order, at most
/// [`MAX_RANGE_DAYS`] days with both ends counted.
pub fn validate_range(from: &str, to: &str) -> Result<(), DomainError> {
    let (Some(first), Some(last)) = (day_number(from), day_number(to)) else {
        return refuse("צריך לבחור תאריכים תקינים ליומן.");
    };
    if last < first {
        return refuse("תאריך הסיום ביומן צריך להיות אחרי תאריך ההתחלה.");
    }
    if last - first >= MAX_RANGE_DAYS {
        return refuse(format!(
            "אפשר להציג ביומן עד {MAX_RANGE_DAYS} ימים בבת אחת."
        ));
    }
    Ok(())
}

/// The window's "now": a real date and a minute of the day.
pub fn validate_today(today: &Today) -> Result<(), DomainError> {
    if parse_date(&today.date).is_none() || today.minute >= DAY_MINUTES {
        return refuse("התאריך של היום לא התקבל כמו שצריך. כדאי לסגור את המסך ולפתוח אותו שוב.");
    }
    Ok(())
}

/// Only a visit that is planned, and was never anything else, can be removed (D-176).
/// `marked`: one of its saved states was not "planned" ([`ever_marked`]): it was held,
/// cancelled or missed once, and so it stays in the diary whatever it says now.
pub fn validate_delete(visit: &Visit, marked: bool) -> Result<(), DomainError> {
    if visit.status != VisitStatus::Planned {
        return refuse("אפשר למחוק רק פגישה שעדיין מתוכננת. פגישה שכבר סומנה אפשר לסמן כמבוטלת.");
    }
    if marked {
        return refuse(
            "הפגישה הזו כבר סומנה פעם (התקיימה, בוטלה או לא הגיעו), ולכן היא נשארת ביומן. \
             אם היא לא תתקיים, אפשר לסמן אותה כמבוטלת.",
        );
    }
    Ok(())
}

/// Whether a visit ever left "planned", by its saved states (its history, and what it says
/// now). Such a visit is never removed and counts as something that happened in the card.
#[must_use]
pub fn ever_marked<'a>(states: impl IntoIterator<Item = &'a Visit>) -> bool {
    states
        .into_iter()
        .any(|state| state.status != VisitStatus::Planned)
}

/// Taking a mark back (D-176): a visit that was held, cancelled or missed goes back to
/// "planned" only as the correction of a wrong click, when nothing leans on the mark yet:
/// it has no session note, and what the family already paid is not more than what is left
/// charged without it. Any other change of status is always allowed; the history keeps it.
///
/// `stored` is the visit as it is kept; the slices are the clinic's or the patient's (only
/// the records of the visit's patient count).
pub fn validate_unmark(
    stored: &Visit,
    edited: &Visit,
    has_note: bool,
    visits: &[Visit],
    charges: &[Charge],
    payments: &[Payment],
) -> Result<(), DomainError> {
    if stored.status == VisitStatus::Planned || edited.status != VisitStatus::Planned {
        return Ok(());
    }
    if has_note {
        return refuse(
            "לפגישה הזו יש סיכום, ולכן היא לא יכולה לחזור להיות \"מתוכננת\". \
             אם היא לא התקיימה, אפשר לסמן אותה כמבוטלת ולכתוב מה קרה.",
        );
    }
    let charge = i64::from(visit_charge(stored));
    if charge == 0 {
        return Ok(());
    }
    let Some(patient_id) = stored.patient_id.as_deref() else {
        return Ok(());
    };
    let (charged, paid) = charged_paid(
        visits
            .iter()
            .filter(|v| v.patient_id.as_deref() == Some(patient_id)),
        charges.iter().filter(|c| c.patient_id == patient_id),
        payments.iter().filter(|p| p.patient_id == patient_id),
    );
    if paid > charged.saturating_sub(charge) {
        return refuse(
            "על הפגישה הזו כבר נרשם תשלום, ולכן היא לא יכולה לחזור להיות \"מתוכננת\". \
             אם היא סומנה בטעות, אפשר לסמן אותה כמבוטלת בלי חיוב ולכתוב מה קרה, \
             או לבטל קודם את התקבול.",
        );
    }
    Ok(())
}

// ------------------------------------------------------------------ visits

/// The dates of a repeating series: `count` dates, `7 × every_weeks` days apart, the
/// first included.
pub fn series_dates(first_date: &str, repeat: Repeat) -> Result<Vec<String>, DomainError> {
    validate_repeat(repeat)?;
    let Some(first) = day_number(first_date) else {
        return refuse("צריך לבחור תאריך תקין לפגישה הראשונה.");
    };
    let step = 7 * i64::from(repeat.every_weeks);
    (0..i64::from(repeat.count))
        .map(|n| {
            date_of_day(first + n * step)
                .map_or_else(|| refuse("הסדרה נמשכת רחוק מדי. כדאי לקצר אותה."), Ok)
        })
        .collect()
}

/// What "this one and every later one" does to the later visits of a series.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Following {
    /// The later visits as they should be saved, by date.
    pub visits: Vec<Visit>,
    /// Later visits of the series left exactly as they are: they have a session note, or
    /// they are no longer planned.
    pub left_alone: u32,
}

/// "This one and every later one" (`Scope::Following`): the later visits of the series that
/// are still planned and have no session note, as they should be saved. Each takes the
/// edited visit's kind, title, start, minutes, place, fee and note, and moves by the number
/// of days the edited one moved. Status, patient and everything else stay as they were.
///
/// A later visit that has a note (`noted`: the ids of visits with one), is not planned, or
/// holds a value of a later version (D-178) is never moved or altered: the note would stay on
/// the old date, a visit that happened is a line of the diary, and a newer value is never
/// rewritten. They are counted in `left_alone` (D-176). Only visits of the same patient as
/// the edited one (or reserved times, for a reserved time) follow it.
///
/// `previous` is the edited visit as it is stored; `series` may hold any visits (those of
/// another series and the edited one itself are skipped). The edited visit itself is not in
/// the result.
pub fn apply_following(
    edited: &Visit,
    previous: &Visit,
    series: &[Visit],
    noted: &HashSet<String>,
) -> Result<Following, DomainError> {
    let Some(series_id) = previous.series_id.as_deref() else {
        return Ok(Following::default());
    };
    let Some(shift) = days_between(&previous.date, &edited.date) else {
        return refuse("צריך לבחור תאריך תקין לפגישה.");
    };
    let was = (previous.date.as_str(), previous.start);
    let mut later = Vec::new();
    let mut left_alone = 0_u32;
    for visit in series {
        // Only the same patient's visits (or reserved times, for a reserved time): a series
        // never reaches another card, whatever its rows say.
        let follows = visit.series_id.as_deref() == Some(series_id)
            && visit.patient_id == previous.patient_id
            && visit.id != previous.id
            && (visit.date.as_str(), visit.start) > was;
        if !follows {
            continue;
        }
        // A value of a later version is never rewritten (D-178): such a visit stays as it is.
        if visit.status != VisitStatus::Planned
            || noted.contains(&visit.id)
            || visit_known(visit).is_err()
        {
            left_alone = left_alone.saturating_add(1);
            continue;
        }
        let Some(date) = add_days(&visit.date, shift) else {
            return refuse("אחת הפגישות הבאות בסדרה יוצאת מטווח התאריכים האפשרי.");
        };
        let moved = Visit {
            kind: edited.kind,
            title: edited.title.clone(),
            date,
            start: edited.start,
            minutes: edited.minutes,
            place: edited.place.clone(),
            fee: edited.fee,
            note: edited.note.clone(),
            ..visit.clone()
        };
        validate_visit(&moved)?;
        later.push(moved);
    }
    later.sort_by(|a, b| (&a.date, a.start, &a.id).cmp(&(&b.date, b.start, &b.id)));
    Ok(Following {
        visits: later,
        left_alone,
    })
}

fn visit_end(visit: &Visit) -> u32 {
    u32::from(visit.start) + u32::from(visit.minutes)
}

/// Both not cancelled, on the same date, and their times intersect. Ending exactly when
/// the other starts is not an overlap.
#[must_use]
pub fn visits_overlap(a: &Visit, b: &Visit) -> bool {
    a.status != VisitStatus::Cancelled
        && b.status != VisitStatus::Cancelled
        && a.date == b.date
        && u32::from(a.start) < visit_end(b)
        && u32::from(b.start) < visit_end(a)
}

/// The ids of the visits that overlap another one of the set.
#[must_use]
pub fn overlapping_ids(visits: &[Visit]) -> BTreeSet<String> {
    let mut live: Vec<&Visit> = visits
        .iter()
        .filter(|v| v.status != VisitStatus::Cancelled)
        .collect();
    live.sort_by(|a, b| (&a.date, a.start).cmp(&(&b.date, b.start)));
    let mut ids = BTreeSet::new();
    for (at, visit) in live.iter().enumerate() {
        // Sorted by start: once one starts at or after this one's end, the rest do too.
        let after = live
            .iter()
            .skip(at + 1)
            .take_while(|next| next.date == visit.date && u32::from(next.start) < visit_end(visit));
        for next in after {
            if visits_overlap(visit, next) {
                ids.insert(visit.id.clone());
                ids.insert(next.id.clone());
            }
        }
    }
    ids
}

/// The patient with the status a saved visit gives them: a family on the waiting list
/// becomes active. `None` when nothing changes.
#[must_use]
pub fn activated(patient: &Patient) -> Option<Patient> {
    (patient.status == PatientStatus::Waiting).then(|| Patient {
        status: PatientStatus::Active,
        ..patient.clone()
    })
}

/// How long a kind of visit usually is and what it costs, from the settings.
#[must_use]
pub fn kind_default(settings: &ClinicSettings, kind: VisitKind) -> Option<KindDefault> {
    settings.defaults.iter().copied().find(|d| d.kind == kind)
}

/// The fee a new visit starts with: the family's own fee when she set one, otherwise the
/// kind's default. A block never has one.
#[must_use]
pub fn default_fee(
    patient: Option<&Patient>,
    settings: &ClinicSettings,
    kind: VisitKind,
) -> Agorot {
    if kind == VisitKind::Block {
        return 0;
    }
    patient
        .and_then(|p| p.session_fee)
        .or_else(|| kind_default(settings, kind).map(|d| d.fee))
        .unwrap_or(0)
}

/// The next planned visit from `today` on (one earlier today is still "next" until marked).
#[must_use]
pub fn next_visit(visits: &[Visit], today: &str) -> Option<VisitWhen> {
    visits
        .iter()
        .filter(|v| v.status == VisitStatus::Planned && v.date.as_str() >= today)
        .min_by(|a, b| (&a.date, a.start).cmp(&(&b.date, b.start)))
        .map(|v| VisitWhen {
            date: v.date.clone(),
            start: v.start,
            kind: v.kind,
        })
}

/// The date of the last visit that was held.
#[must_use]
pub fn last_held(visits: &[Visit]) -> Option<String> {
    visits
        .iter()
        .filter(|v| v.status == VisitStatus::Held)
        .map(|v| v.date.as_str())
        .max()
        .map(str::to_owned)
}

/// Visits as the calendar shows them, by date and time. `noted` holds the ids of visits
/// that have a session note; `overlapping` comes from [`overlapping_ids`] over every visit
/// of the days shown.
#[must_use]
pub fn visit_views(
    visits: &[Visit],
    names: &HashMap<String, String>,
    noted: &HashSet<String>,
    overlapping: &BTreeSet<String>,
) -> Vec<VisitView> {
    let mut views: Vec<VisitView> = visits
        .iter()
        .map(|visit| VisitView {
            patient_name: visit
                .patient_id
                .as_ref()
                .and_then(|id| names.get(id))
                .cloned(),
            has_note: noted.contains(&visit.id),
            overlaps: overlapping.contains(&visit.id),
            visit: visit.clone(),
        })
        .collect();
    views.sort_by(|a, b| {
        (&a.visit.date, a.visit.start, &a.visit.id).cmp(&(
            &b.visit.date,
            b.visit.start,
            &b.visit.id,
        ))
    });
    views
}

// ------------------------------------------------------------------ money

/// What a visit costs the family: its fee when it was held, or when it was cancelled or
/// missed and she charges for it anyway; otherwise nothing.
#[must_use]
pub fn visit_charge(visit: &Visit) -> Agorot {
    let charged = match visit.status {
        VisitStatus::Held => true,
        VisitStatus::Cancelled | VisitStatus::NoShow => visit.charge_missed,
        VisitStatus::Planned | VisitStatus::Unknown => false,
    };
    if charged {
        visit.fee.max(0)
    } else {
        0
    }
}

/// (charged, paid), in a width that cannot overflow.
fn charged_paid<'a>(
    visits: impl IntoIterator<Item = &'a Visit>,
    charges: impl IntoIterator<Item = &'a Charge>,
    payments: impl IntoIterator<Item = &'a Payment>,
) -> (i64, i64) {
    let from_visits = visits.into_iter().fold(0_i64, |sum, v| {
        sum.saturating_add(i64::from(visit_charge(v)))
    });
    let charged = charges
        .into_iter()
        .filter(|c| !c.voided)
        .fold(from_visits, |sum, c| {
            sum.saturating_add(i64::from(c.amount))
        });
    let paid = payments
        .into_iter()
        .filter(|p| !p.voided)
        .fold(0_i64, |sum, p| sum.saturating_add(i64::from(p.amount)));
    (charged, paid)
}

/// charged = visit charges + charges not voided; paid = payments not voided;
/// due = charged − paid. Refused when a figure does not fit, never wrong.
pub fn balance(
    visits: &[Visit],
    charges: &[Charge],
    payments: &[Payment],
) -> Result<Balance, DomainError> {
    let (charged, paid) = charged_paid(visits, charges, payments);
    Ok(Balance {
        charged: to_agorot(charged)?,
        paid: to_agorot(paid)?,
        due: to_agorot(charged.saturating_sub(paid))?,
    })
}

fn oldest_uncovered_of<'a>(
    visits: impl IntoIterator<Item = &'a Visit>,
    charges: impl IntoIterator<Item = &'a Charge>,
    payments: impl IntoIterator<Item = &'a Payment>,
) -> Option<&'a str> {
    let mut owed: Vec<(&str, i64)> = visits
        .into_iter()
        .map(|v| (v.date.as_str(), i64::from(visit_charge(v))))
        .chain(
            charges
                .into_iter()
                .filter(|c| !c.voided)
                .map(|c| (c.date.as_str(), i64::from(c.amount))),
        )
        .filter(|(_, amount)| *amount > 0)
        .collect();
    owed.sort_by(|a, b| a.0.cmp(b.0));
    let mut money = payments
        .into_iter()
        .filter(|p| !p.voided)
        .fold(0_i64, |sum, p| sum.saturating_add(i64::from(p.amount)));
    for (date, amount) in owed {
        if money < amount {
            return Some(date);
        }
        money -= amount;
    }
    None
}

/// Paying the oldest charges first with everything that was received: the date of the
/// oldest charge that is still not fully covered. `None`: nothing is owed.
#[must_use]
pub fn oldest_uncovered(
    visits: &[Visit],
    charges: &[Charge],
    payments: &[Payment],
) -> Option<String> {
    oldest_uncovered_of(visits, charges, payments).map(str::to_owned)
}

fn money_text(amount: i64) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let (whole, part) = (amount.unsigned_abs() / 100, amount.unsigned_abs() % 100);
    if part == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{part:02}")
    }
}

/// Agorot as she reads them: `450`, `450.50` (the ₪ is the caller's).
#[must_use]
pub fn money(amount: Agorot) -> String {
    money_text(i64::from(amount))
}

/// What the clinic is owed in all: the sum of what each family still owes. A family that
/// paid ahead does not hide another family's debt.
pub fn total_due(balances: &[Balance]) -> Result<Agorot, DomainError> {
    to_agorot(
        balances
            .iter()
            .fold(0_i64, |sum, b| sum.saturating_add(i64::from(b.due.max(0)))),
    )
}

/// What was received (not voided) in the month of `date`.
pub fn month_received(payments: &[Payment], date: &str) -> Result<Agorot, DomainError> {
    let Some(month) = month_of(date) else {
        return refuse("צריך לבחור תאריך תקין.");
    };
    to_agorot(
        payments
            .iter()
            .filter(|p| !p.voided && month_of(&p.date).as_deref() == Some(month.as_str()))
            .fold(0_i64, |sum, p| sum.saturating_add(i64::from(p.amount))),
    )
}

// ------------------------------------------------------------------ labels

#[must_use]
pub fn kind_label(kind: VisitKind) -> &'static str {
    match kind {
        VisitKind::Intake => "פגישת היכרות",
        VisitKind::Assessment => "מפגש אבחון",
        VisitKind::Observation => "תצפית",
        VisitKind::Feedback => "שיחת משוב",
        VisitKind::Therapy => "פגישת טיפול",
        VisitKind::Guidance => "הדרכת הורים",
        VisitKind::Meeting => "פגישה מקצועית",
        VisitKind::Other => "פגישה אחרת",
        VisitKind::Block => "זמן שמור",
        VisitKind::Unknown => "סוג לא מוכר",
    }
}

#[must_use]
pub fn status_label(status: VisitStatus) -> &'static str {
    match status {
        VisitStatus::Planned => "מתוכננת",
        VisitStatus::Held => "התקיימה",
        VisitStatus::Cancelled => "בוטלה",
        VisitStatus::NoShow => "לא הגיעו",
        VisitStatus::Unknown => "מצב לא מוכר",
    }
}

#[must_use]
pub fn method_label(method: PayMethod) -> &'static str {
    match method {
        PayMethod::Cash => "מזומן",
        PayMethod::Transfer => "העברה בנקאית",
        PayMethod::Bit => "ביט",
        PayMethod::Paybox => "פייבוקס",
        PayMethod::Check => "צ'ק",
        PayMethod::Card => "כרטיס אשראי",
        PayMethod::Other => "אחר",
        PayMethod::Unknown => "אמצעי לא מוכר",
    }
}

#[must_use]
pub fn task_kind_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Call => "שיחת טלפון",
        TaskKind::Questionnaire => "שאלון",
        TaskKind::Document => "מסמך",
        TaskKind::Report => "דוח",
        TaskKind::Payment => "תשלום",
        TaskKind::FollowUp => "מעקב",
        TaskKind::Other => "אחר",
        TaskKind::Unknown => "סוג לא מוכר",
    }
}

#[must_use]
pub fn patient_status_label(status: PatientStatus) -> &'static str {
    match status {
        PatientStatus::Waiting => "ברשימת ההמתנה",
        PatientStatus::Active => "בתהליך",
        PatientStatus::Closed => "הסתיים",
        PatientStatus::Unknown => "מצב לא מוכר",
    }
}

#[must_use]
pub fn contact_role_label(role: ContactRole) -> &'static str {
    match role {
        ContactRole::Mother => "אמא",
        ContactRole::Father => "אבא",
        ContactRole::Guardian => "אפוטרופוס",
        ContactRole::Other => "אחר",
        ContactRole::Unknown => "קשר לא מוכר",
    }
}

/// What the calendar calls a visit: her own words when she wrote any, otherwise the kind.
#[must_use]
pub fn visit_title(visit: &Visit) -> &str {
    let title = visit.title.trim();
    if title.is_empty() {
        kind_label(visit.kind)
    } else {
        title
    }
}

/// The title of the case material that holds a session's record: `סיכום <kind> · <date>`.
#[must_use]
pub fn note_case_title(kind: VisitKind, date: &str) -> String {
    format!("סיכום {} · {}", kind_label(kind), display_date(date))
}

/// The title of the case material for a note: the kind of its visit, or "a talk" for a note
/// that stands on its own (a phone call).
#[must_use]
pub fn note_case_title_for(kind: Option<VisitKind>, date: &str) -> String {
    match kind {
        Some(kind) => note_case_title(kind, date),
        None => format!("סיכום שיחה · {}", display_date(date)),
    }
}

// ------------------------------------------------------------------ alerts

/// Everything the alerts of one patient are computed from. The slices may hold other
/// patients' records too: only this patient's are looked at.
#[derive(Debug, Clone, Copy)]
pub struct PatientRecords<'a> {
    pub patient: &'a Patient,
    pub visits: &'a [Visit],
    pub notes: &'a [SessionNote],
    pub charges: &'a [Charge],
    pub payments: &'a [Payment],
    pub tasks: &'a [ClinicTask],
    /// The report's sections (approved, total); `None`: the patient has no case.
    pub report: Option<(u32, u32)>,
}

/// Everything the alerts of the whole clinic are computed from.
#[derive(Debug, Clone, Copy)]
pub struct ClinicRecords<'a> {
    pub patients: &'a [Patient],
    pub visits: &'a [Visit],
    pub notes: &'a [SessionNote],
    pub charges: &'a [Charge],
    pub payments: &'a [Payment],
    pub tasks: &'a [ClinicTask],
    /// The report's sections (approved, total) by patient id, for patients with a case.
    pub reports: &'a HashMap<String, (u32, u32)>,
}

/// A report is finished when every one of its sections is approved. A report with no
/// sections at all is not a finished one.
#[must_use]
pub fn report_finished(report: Option<(u32, u32)>) -> bool {
    report.is_some_and(|(approved, total)| total > 0 && approved >= total)
}

fn report_words(report: Option<(u32, u32)>) -> String {
    match report {
        Some((approved, total)) => {
            format!("הדוח עדיין לא גמור ({approved} מתוך {total} סעיפים)")
        }
        None => "עדיין לא נפתח תיק לדוח".to_owned(),
    }
}

fn weeks_words(weeks: i64) -> String {
    match weeks {
        1 => "שבוע".to_owned(),
        2 => "שבועיים".to_owned(),
        n => format!("{n} שבועות"),
    }
}

fn needs_note(kind: VisitKind) -> bool {
    matches!(
        kind,
        VisitKind::Intake
            | VisitKind::Assessment
            | VisitKind::Observation
            | VisitKind::Feedback
            | VisitKind::Therapy
            | VisitKind::Guidance
    )
}

fn kind_rank(kind: AlertKind) -> u8 {
    match kind {
        AlertKind::Unmarked => 0,
        AlertKind::NoNote => 1,
        AlertKind::TaskOverdue => 2,
        AlertKind::FeedbackSoon => 3,
        AlertKind::MaterialsMissing => 4,
        AlertKind::ReportDue => 5,
        AlertKind::Debt => 6,
        AlertKind::NoNextVisit => 7,
        AlertKind::Waiting => 8,
        AlertKind::Unreadable => 9,
    }
}

/// Oldest first; the rest of the key only makes the order the same on every read.
fn sort_alerts(alerts: &mut [Alert]) {
    alerts.sort_by(|a, b| {
        (
            &a.since,
            kind_rank(a.kind),
            &a.patient_id,
            &a.visit_id,
            &a.task_id,
        )
            .cmp(&(
                &b.since,
                kind_rank(b.kind),
                &b.patient_id,
                &b.visit_id,
                &b.task_id,
            ))
    });
}

fn task_alert(task: &ClinicTask, patient: Option<&Patient>, today: i64) -> Option<Alert> {
    let due = task.due.as_deref()?;
    let late = task.done_on.is_none() && day_number(due)? < today;
    late.then(|| Alert {
        kind: AlertKind::TaskOverdue,
        patient_id: patient.map(|p| p.id.clone()),
        patient_name: patient.map(|p| p.name.clone()),
        visit_id: None,
        task_id: Some(task.id.clone()),
        text: format!(
            "המשימה \"{}\" הייתה אמורה להיעשות עד {}.",
            clean_line(&task.title),
            display_date(due)
        ),
        since: due.to_owned(),
    })
}

/// One patient's records, picked out of the clinic's.
struct Of<'a> {
    patient: &'a Patient,
    visits: Vec<&'a Visit>,
    notes: Vec<&'a SessionNote>,
    charges: Vec<&'a Charge>,
    payments: Vec<&'a Payment>,
    tasks: Vec<&'a ClinicTask>,
    report: Option<(u32, u32)>,
}

impl<'a> Of<'a> {
    fn new(patient: &'a Patient, report: Option<(u32, u32)>) -> Self {
        Self {
            patient,
            visits: Vec::new(),
            notes: Vec::new(),
            charges: Vec::new(),
            payments: Vec::new(),
            tasks: Vec::new(),
            report,
        }
    }

    fn alert(&self, kind: AlertKind, since: &str, text: String) -> Alert {
        Alert {
            kind,
            patient_id: Some(self.patient.id.clone()),
            patient_name: Some(self.patient.name.clone()),
            visit_id: None,
            task_id: None,
            text,
            since: since.to_owned(),
        }
    }

    fn visit_alert(&self, kind: AlertKind, visit: &Visit, text: String) -> Alert {
        Alert {
            visit_id: Some(visit.id.clone()),
            ..self.alert(kind, &visit.date, text)
        }
    }

    fn dates(&self, kind: VisitKind, status: VisitStatus) -> impl Iterator<Item = &'a str> + '_ {
        self.visits
            .iter()
            .filter(move |v| v.kind == kind && v.status == status)
            .map(|v| v.date.as_str())
    }

    fn due(&self) -> i64 {
        let (charged, paid) = charged_paid(
            self.visits.iter().copied(),
            self.charges.iter().copied(),
            self.payments.iter().copied(),
        );
        charged.saturating_sub(paid)
    }

    fn alerts(&self, today: &Today, settings: &ClinicSettings) -> Vec<Alert> {
        let Some(now) = day_number(&today.date) else {
            return Vec::new();
        };
        let mut alerts = Vec::new();
        self.visit_alerts(now, today, &mut alerts);
        alerts.extend(self.debt(now, settings));
        alerts.extend(self.waiting(now, settings));
        alerts.extend(self.report_due(now, today, settings));
        alerts.extend(self.no_next_visit(now, today));
        alerts.extend(
            self.tasks
                .iter()
                .filter_map(|task| task_alert(task, Some(self.patient), now)),
        );
        alerts
    }

    /// `Unmarked`, `NoNote` and `FeedbackSoon`: the alerts that are about one visit.
    fn visit_alerts(&self, now: i64, today: &Today, alerts: &mut Vec<Alert>) {
        let noted: HashSet<&str> = self
            .notes
            .iter()
            .filter_map(|n| n.visit_id.as_deref())
            .collect();
        for visit in &self.visits {
            let Some(day) = day_number(&visit.date) else {
                continue;
            };
            let when = format!(
                "{} בשעה {}",
                display_date(&visit.date),
                display_time(visit.start)
            );
            match visit.status {
                VisitStatus::Planned if visit.kind != VisitKind::Block => {
                    let ended =
                        day < now || (day == now && visit_end(visit) <= u32::from(today.minute));
                    if ended {
                        alerts.push(self.visit_alert(
                            AlertKind::Unmarked,
                            visit,
                            format!(
                                "הפגישה מיום {when} עדיין מסומנת כמתוכננת. אפשר לסמן אם התקיימה."
                            ),
                        ));
                    }
                    let soon = visit.kind == VisitKind::Feedback
                        && (0..=FEEDBACK_SOON_DAYS).contains(&(day - now))
                        && !report_finished(self.report);
                    if soon {
                        alerts.push(self.visit_alert(
                            AlertKind::FeedbackSoon,
                            visit,
                            format!(
                                "שיחת המשוב נקבעה ליום {when}, ו{}.",
                                report_words(self.report)
                            ),
                        ));
                    }
                    // Questionnaires and documents still open a week before the feedback
                    // (D-199). Only how many: a task's title is hers, and may name someone.
                    let missing = self
                        .tasks
                        .iter()
                        .filter(|t| {
                            t.done_on.is_none()
                                && matches!(t.kind, TaskKind::Questionnaire | TaskKind::Document)
                        })
                        .count();
                    let near = visit.kind == VisitKind::Feedback
                        && (0..=FEEDBACK_SOON_DAYS).contains(&(day - now));
                    if near && missing > 0 {
                        let what = if missing == 1 {
                            "שאלון או מסמך אחד עדיין פתוח".to_owned()
                        } else {
                            format!("{missing} שאלונים או מסמכים עדיין פתוחים")
                        };
                        alerts.push(self.visit_alert(
                            AlertKind::MaterialsMissing,
                            visit,
                            format!("שיחת המשוב נקבעה ליום {when}, ו{what} במעקבים."),
                        ));
                    }
                }
                VisitStatus::Held
                    if needs_note(visit.kind)
                        && (0..=NO_NOTE_DAYS).contains(&(now - day))
                        && !noted.contains(visit.id.as_str()) =>
                {
                    alerts.push(self.visit_alert(
                        AlertKind::NoNote,
                        visit,
                        format!(
                            "עדיין אין סיכום ל{} מיום {}.",
                            kind_label(visit.kind),
                            display_date(&visit.date)
                        ),
                    ));
                }
                _ => {}
            }
        }
    }

    fn debt(&self, now: i64, settings: &ClinicSettings) -> Option<Alert> {
        let due = self.due();
        if due <= 0 {
            return None;
        }
        let oldest = oldest_uncovered_of(
            self.visits.iter().copied(),
            self.charges.iter().copied(),
            self.payments.iter().copied(),
        )?;
        (now - day_number(oldest)? >= i64::from(settings.debt_days)).then(|| {
            self.alert(
                AlertKind::Debt,
                oldest,
                format!(
                    "יתרה פתוחה של {} ₪, מאז {}.",
                    money_text(due),
                    display_date(oldest)
                ),
            )
        })
    }

    fn waiting(&self, now: i64, settings: &ClinicSettings) -> Option<Alert> {
        if self.patient.status != PatientStatus::Waiting {
            return None;
        }
        // A card without the day it joined the list waits since the day it was opened.
        let since = self
            .patient
            .waiting_since
            .as_deref()
            .filter(|date| parse_date(date).is_some())
            .unwrap_or(&self.patient.created_on);
        let days = now - day_number(since)?;
        (days >= 7 * i64::from(settings.waiting_weeks)).then(|| {
            let text = if days >= 7 {
                format!(
                    "ברשימת ההמתנה כבר {}, מאז {}.",
                    weeks_words(days / 7),
                    display_date(since)
                )
            } else {
                format!("ברשימת ההמתנה מאז {}.", display_date(since))
            };
            self.alert(AlertKind::Waiting, since, text)
        })
    }

    fn report_due(&self, now: i64, today: &Today, settings: &ClinicSettings) -> Option<Alert> {
        if self.patient.status != PatientStatus::Active || report_finished(self.report) {
            return None;
        }
        let last = self.dates(VisitKind::Assessment, VisitStatus::Held).max()?;
        let more_ahead = self
            .dates(VisitKind::Assessment, VisitStatus::Planned)
            .any(|date| date >= today.date.as_str());
        let feedback_held = self
            .dates(VisitKind::Feedback, VisitStatus::Held)
            .next()
            .is_some();
        if more_ahead || feedback_held {
            return None;
        }
        (day_number(last)? + i64::from(settings.report_days) < now).then(|| {
            self.alert(
                AlertKind::ReportDue,
                last,
                format!(
                    "מפגש האבחון האחרון היה ביום {}, ו{}.",
                    display_date(last),
                    report_words(self.report)
                ),
            )
        })
    }

    fn no_next_visit(&self, now: i64, today: &Today) -> Option<Alert> {
        if self.patient.status != PatientStatus::Active {
            return None;
        }
        let today_date = today.date.as_str();
        let last = self
            .visits
            .iter()
            .map(|v| v.date.as_str())
            .filter(|date| *date <= today_date)
            .max()?;
        let ahead = self
            .visits
            .iter()
            .any(|v| v.status == VisitStatus::Planned && v.date.as_str() >= today_date);
        let open_task = self.tasks.iter().any(|t| t.done_on.is_none());
        let finished = self
            .dates(VisitKind::Feedback, VisitStatus::Held)
            .next()
            .is_some()
            && self.due() <= 0;
        if ahead || open_task || finished {
            return None;
        }
        (now - day_number(last)? > NO_NEXT_VISIT_DAYS).then(|| {
            self.alert(
                AlertKind::NoNextVisit,
                last,
                format!(
                    "הפגישה האחרונה הייתה ביום {}, ואין פגישה הבאה ביומן.",
                    display_date(last)
                ),
            )
        })
    }
}

/// What needs her about one patient, oldest first.
#[must_use]
pub fn patient_alerts(
    records: &PatientRecords<'_>,
    today: &Today,
    settings: &ClinicSettings,
) -> Vec<Alert> {
    let id = records.patient.id.as_str();
    let of = Of {
        patient: records.patient,
        visits: records
            .visits
            .iter()
            .filter(|v| v.patient_id.as_deref() == Some(id))
            .collect(),
        notes: records
            .notes
            .iter()
            .filter(|n| n.patient_id == id)
            .collect(),
        charges: records
            .charges
            .iter()
            .filter(|c| c.patient_id == id)
            .collect(),
        payments: records
            .payments
            .iter()
            .filter(|p| p.patient_id == id)
            .collect(),
        tasks: records
            .tasks
            .iter()
            .filter(|t| t.patient_id.as_deref() == Some(id))
            .collect(),
        report: records.report,
    };
    let mut alerts = of.alerts(today, settings);
    sort_alerts(&mut alerts);
    alerts
}

/// What needs her in the whole clinic, oldest first: every patient's alerts, and the
/// overdue tasks that belong to no patient. A record of a patient that is not in
/// `patients` is not looked at (a task of one is shown without a name).
#[must_use]
pub fn clinic_alerts(
    records: &ClinicRecords<'_>,
    today: &Today,
    settings: &ClinicSettings,
) -> Vec<Alert> {
    let Some(now) = day_number(&today.date) else {
        return Vec::new();
    };
    let mut by_patient: HashMap<&str, Of<'_>> = records
        .patients
        .iter()
        .map(|p| {
            (
                p.id.as_str(),
                Of::new(p, records.reports.get(&p.id).copied()),
            )
        })
        .collect();
    for visit in records.visits {
        if let Some(of) = visit
            .patient_id
            .as_deref()
            .and_then(|id| by_patient.get_mut(id))
        {
            of.visits.push(visit);
        }
    }
    for note in records.notes {
        if let Some(of) = by_patient.get_mut(note.patient_id.as_str()) {
            of.notes.push(note);
        }
    }
    for charge in records.charges {
        if let Some(of) = by_patient.get_mut(charge.patient_id.as_str()) {
            of.charges.push(charge);
        }
    }
    for payment in records.payments {
        if let Some(of) = by_patient.get_mut(payment.patient_id.as_str()) {
            of.payments.push(payment);
        }
    }
    let mut alerts = Vec::new();
    for task in records.tasks {
        match task
            .patient_id
            .as_deref()
            .and_then(|id| by_patient.get_mut(id))
        {
            Some(of) => of.tasks.push(task),
            None => alerts.extend(task_alert(task, None, now)),
        }
    }
    for patient in records.patients {
        if let Some(of) = by_patient.get(patient.id.as_str()) {
            alerts.extend(of.alerts(today, settings));
        }
    }
    sort_alerts(&mut alerts);
    alerts
}

// ------------------------------------------------------------------ erasing a patient

/// Whether a patient card may be erased (D-174, D-176). A card where nothing happened (no
/// visit that ever left "planned", no note, no charge and no payment, voided ones included)
/// goes at once: a typing mistake, a double card. `marked` holds the ids of visits that
/// were held, cancelled or missed at some time by their saved history ([`ever_marked`]): a
/// visit put back to "planned" still happened. Any other card is a record that has to be
/// kept, and goes only after the retention period: `keep_years` after the last activity in
/// the card, and not before the child is `keep_until_age` when the birth date is known,
/// whichever is later.
///
/// The last activity is the latest date anything in the card carries (the day it was
/// opened, every visit, note, charge, payment and task) and `last_saved`, the day one of its
/// records was last written, when the caller knows it: a record corrected years later was
/// active then. The slices may hold other patients' records too.
///
/// The refusal says the card can be closed instead, and from which day it can be erased.
/// Whether anything happened in a card (D-174, D-176): a visit that ever left "planned"
/// (`marked`: by its saved history), a note, a charge or a payment, voided ones included.
#[must_use]
pub fn card_happened(records: &PatientRecords<'_>, marked: &HashSet<String>) -> bool {
    let id = records.patient.id.as_str();
    records.visits.iter().any(|v| {
        v.patient_id.as_deref() == Some(id)
            && (v.status != VisitStatus::Planned || marked.contains(&v.id))
    }) || records.notes.iter().any(|n| n.patient_id == id)
        || records.charges.iter().any(|c| c.patient_id == id)
        || records.payments.iter().any(|p| p.patient_id == id)
}

pub fn can_erase(
    records: &PatientRecords<'_>,
    marked: &HashSet<String>,
    today: &str,
    last_saved: Option<&str>,
    keep_years: i32,
    keep_until_age: i32,
) -> Result<(), DomainError> {
    let id = records.patient.id.as_str();
    let visits = || {
        records
            .visits
            .iter()
            .filter(|v| v.patient_id.as_deref() == Some(id))
    };
    let notes = || records.notes.iter().filter(|n| n.patient_id == id);
    let charges = || records.charges.iter().filter(|c| c.patient_id == id);
    let payments = || records.payments.iter().filter(|p| p.patient_id == id);
    if !card_happened(records, marked) {
        return Ok(());
    }
    let Some(now) = parse_date(today) else {
        return refuse("התאריך של היום לא התקבל כמו שצריך, ולכן אי אפשר למחוק עכשיו.");
    };
    let tasks = records
        .tasks
        .iter()
        .filter(|t| t.patient_id.as_deref() == Some(id));
    let last = std::iter::once(records.patient.created_on.as_str())
        .chain(last_saved)
        .chain(visits().map(|v| v.date.as_str()))
        .chain(notes().map(|n| n.date.as_str()))
        .chain(charges().map(|c| c.date.as_str()))
        .chain(payments().map(|p| p.date.as_str()))
        .chain(tasks.flat_map(|t| t.due.iter().chain(t.done_on.iter()).map(String::as_str)))
        .filter_map(parse_date)
        .max();
    // A card whose dates cannot be read is kept as if it changed today.
    let after_last = add_years(last.unwrap_or(now), keep_years);
    let grown = records
        .patient
        .birth_date
        .as_deref()
        .and_then(parse_birth_date)
        .map(|birth| add_years(birth, keep_until_age));
    let from = grown.map_or(after_last, |grown| grown.max(after_last));
    if now >= from {
        return Ok(());
    }
    refuse(format!(
        "בכרטיס הזה יש פגישות, סיכומים או רישומי כספים, ורשומה כזו צריך לשמור. \
         אפשר לסגור את הכרטיס במקום למחוק אותו. מחיקה תהיה אפשרית החל מיום {}.",
        display_date(&iso(from))
    ))
}

// ------------------------------------------------------------------ the assessment process

fn step(key: &str, label: &str, state: StepState, detail: String) -> ProcessStep {
    ProcessStep {
        key: key.to_owned(),
        label: label.to_owned(),
        state,
        detail,
    }
}

/// A step that is one meeting: done once held, under way once planned.
fn meeting_step(key: &str, label: &str, visits: &[Visit], kind: VisitKind) -> ProcessStep {
    let date = |status: VisitStatus| {
        visits
            .iter()
            .filter(move |v| v.kind == kind && v.status == status)
            .map(|v| v.date.as_str())
    };
    if let Some(held) = date(VisitStatus::Held).max() {
        let detail = format!("התקיימה ביום {}", display_date(held));
        return step(key, label, StepState::Done, detail);
    }
    if let Some(planned) = date(VisitStatus::Planned).min() {
        let detail = format!("נקבעה ליום {}", display_date(planned));
        return step(key, label, StepState::Now, detail);
    }
    step(key, label, StepState::Todo, "עוד לא נקבעה".to_owned())
}

fn sessions_step(visits: &[Visit]) -> ProcessStep {
    let of = |status: VisitStatus| {
        visits
            .iter()
            .filter(move |v| v.kind == VisitKind::Assessment && v.status == status)
    };
    let (held, planned) = (
        of(VisitStatus::Held).count(),
        of(VisitStatus::Planned).count(),
    );
    let (state, detail) = match (held, planned) {
        (0, 0) => (StepState::Todo, "עוד לא נקבעו מפגשים".to_owned()),
        (0, _) => (
            StepState::Now,
            of(VisitStatus::Planned)
                .map(|v| v.date.as_str())
                .min()
                .map(|date| format!("המפגש הראשון נקבע ליום {}", display_date(date)))
                .unwrap_or_default(),
        ),
        (1, 0) => (StepState::Done, "מפגש אחד התקיים".to_owned()),
        (_, 0) => (StepState::Done, format!("{held} מפגשים התקיימו")),
        _ => (
            StepState::Now,
            format!("{held} מתוך {} התקיימו", held + planned),
        ),
    };
    step("sessions", "מפגשי אבחון", state, detail)
}

fn report_step(report: Option<(u32, u32)>, sessions_done: bool) -> ProcessStep {
    let detail = match report {
        Some((approved, total)) => format!("הדוח: {approved} מתוך {total} סעיפים"),
        None => "עוד לא נפתח תיק לדוח".to_owned(),
    };
    let started = report.is_some_and(|(approved, _)| approved > 0);
    let state = if report_finished(report) {
        StepState::Done
    } else if sessions_done || started {
        StepState::Now
    } else {
        StepState::Todo
    };
    step("report", "כתיבת הדוח", state, detail)
}

fn payment_step(visits: &[Visit], charges: &[Charge], payments: &[Payment]) -> ProcessStep {
    let (charged, paid) = charged_paid(visits, charges, payments);
    let due = charged.saturating_sub(paid);
    let (state, detail) = if charged == 0 && paid == 0 {
        (StepState::Todo, "עוד לא נרשם חיוב".to_owned())
    } else if due > 0 {
        (
            StepState::Now,
            format!("נותרו {} ₪ לתשלום", money_text(due)),
        )
    } else if due < 0 {
        (
            StepState::Done,
            format!("שולם, ויש יתרת זכות של {} ₪", money_text(-due)),
        )
    } else {
        (StepState::Done, "שולם במלואו".to_owned())
    };
    step("payment", "תשלום", state, detail)
}

/// The way of one assessment, for the patient card: `intake`, `sessions`, `observation`
/// (only when an observation exists), `report`, `feedback`, `payment`. `visits`, `charges`
/// and `payments` are this patient's. When no step is under way, the first one still to do
/// is the one marked `Now`: it is what comes next.
#[must_use]
pub fn process(
    visits: &[Visit],
    charges: &[Charge],
    payments: &[Payment],
    report: Option<(u32, u32)>,
) -> Vec<ProcessStep> {
    let sessions = sessions_step(visits);
    let sessions_done = sessions.state == StepState::Done;
    let mut steps = vec![
        meeting_step("intake", "פגישת היכרות", visits, VisitKind::Intake),
        sessions,
    ];
    let observed = visits
        .iter()
        .any(|v| v.kind == VisitKind::Observation && v.status != VisitStatus::Cancelled);
    if observed {
        let mut observation = meeting_step("observation", "תצפית", visits, VisitKind::Observation);
        if observation.state == StepState::Todo {
            // It exists and was neither held nor is planned: the family did not come.
            observation.detail = "לא התקיימה".to_owned();
        }
        steps.push(observation);
    }
    steps.push(report_step(report, sessions_done));
    steps.push(meeting_step(
        "feedback",
        "שיחת משוב",
        visits,
        VisitKind::Feedback,
    ));
    steps.push(payment_step(visits, charges, payments));
    if !steps.iter().any(|s| s.state == StepState::Now) {
        if let Some(next) = steps.iter_mut().find(|s| s.state == StepState::Todo) {
            next.state = StepState::Now;
        }
    }
    steps
}

// ------------------------------------------------------------------ the diary and income

/// Where a line stands within its day: visits (by time), then charges, then payments; the
/// id only keeps the order the same on every read.
type DiaryKey<'a> = (u8, u16, &'a str);

fn voided_note(note: &str, voided: bool, reason: &str) -> String {
    let note = clean_line(note);
    if !voided {
        return note;
    }
    let reason = format!("בוטל: {}", clean_line(reason));
    if note.is_empty() {
        reason
    } else {
        format!("{note} · {reason}")
    }
}

/// The income diary of a month: every patient visit, every charge and every payment, by
/// date, then visits by time, then charges, then payments. A voided line is shown with its
/// amount and counted nowhere. `names`: patient names by id; `changed`: the ids of records
/// whose history has more than one entry.
pub fn diary_month(
    month: &str,
    visits: &[Visit],
    charges: &[Charge],
    payments: &[Payment],
    names: &HashMap<String, String>,
    changed: &HashSet<String>,
) -> Result<DiaryMonth, DomainError> {
    validate_month(month)?;
    let in_month = |date: &str| month_of(date).as_deref() == Some(month);
    let name = |id: &str| names.get(id).cloned().unwrap_or_default();
    let mut days: BTreeMap<&str, Vec<(DiaryKey<'_>, DiaryLine)>> = BTreeMap::new();
    for visit in visits.iter().filter(|v| in_month(&v.date)) {
        let Some(patient_id) = visit.patient_id.as_deref() else {
            continue;
        };
        let line = DiaryLine {
            record_id: visit.id.clone(),
            time: Some(visit.start),
            patient_name: name(patient_id),
            what: kind_label(visit.kind).to_owned(),
            status: Some(visit.status),
            charged: visit_charge(visit),
            received: 0,
            method: None,
            receipt: String::new(),
            payer: String::new(),
            note: clean_line(&visit.status_note),
            voided: false,
            changed: changed.contains(&visit.id),
        };
        days.entry(&visit.date)
            .or_default()
            .push(((0, visit.start, &visit.id), line));
    }
    for charge in charges.iter().filter(|c| in_month(&c.date)) {
        let line = DiaryLine {
            record_id: charge.id.clone(),
            time: None,
            patient_name: name(&charge.patient_id),
            what: clean_line(&charge.what),
            status: None,
            charged: charge.amount,
            received: 0,
            method: None,
            receipt: String::new(),
            payer: String::new(),
            note: voided_note("", charge.voided, &charge.void_reason),
            voided: charge.voided,
            changed: changed.contains(&charge.id),
        };
        days.entry(&charge.date)
            .or_default()
            .push(((1, 0, &charge.id), line));
    }
    let mut missing_receipts = 0_u32;
    // A method of a later version is a total of its own, so the methods still add up.
    let mut methods: Vec<(PayMethod, i64)> = PAY_METHODS
        .iter()
        .chain(std::iter::once(&PayMethod::Unknown))
        .map(|m| (*m, 0))
        .collect();
    for payment in payments.iter().filter(|p| in_month(&p.date)) {
        if !payment.voided {
            if blank(&payment.receipt) {
                missing_receipts = missing_receipts.saturating_add(1);
            }
            for (method, total) in &mut methods {
                if *method == payment.method {
                    *total = total.saturating_add(i64::from(payment.amount));
                }
            }
        }
        let line = DiaryLine {
            record_id: payment.id.clone(),
            time: None,
            patient_name: name(&payment.patient_id),
            what: "תשלום".to_owned(),
            status: None,
            charged: 0,
            received: payment.amount,
            method: Some(payment.method),
            receipt: clean_line(&payment.receipt),
            payer: clean_line(&payment.payer),
            note: voided_note(&payment.note, payment.voided, &payment.void_reason),
            voided: payment.voided,
            changed: changed.contains(&payment.id),
        };
        days.entry(&payment.date)
            .or_default()
            .push(((2, 0, &payment.id), line));
    }

    let (mut month_charged, mut month_received) = (0_i64, 0_i64);
    let mut diary_days = Vec::with_capacity(days.len());
    for (date, mut lines) in days {
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        let lines: Vec<DiaryLine> = lines.into_iter().map(|(_, line)| line).collect();
        let (mut charged, mut received) = (0_i64, 0_i64);
        for line in lines.iter().filter(|l| !l.voided) {
            charged = charged.saturating_add(i64::from(line.charged));
            received = received.saturating_add(i64::from(line.received));
        }
        month_charged = month_charged.saturating_add(charged);
        month_received = month_received.saturating_add(received);
        diary_days.push(DiaryDay {
            date: date.to_owned(),
            lines,
            charged: to_agorot(charged)?,
            received: to_agorot(received)?,
        });
    }
    let by_method = methods
        .into_iter()
        .filter(|(_, total)| *total > 0)
        .map(|(method, total)| {
            Ok(MethodTotal {
                method,
                amount: to_agorot(total)?,
            })
        })
        .collect::<Result<Vec<_>, DomainError>>()?;
    Ok(DiaryMonth {
        month: month.to_owned(),
        days: diary_days,
        charged: to_agorot(month_charged)?,
        received: to_agorot(month_received)?,
        by_method,
        missing_receipts,
        erased: None,
    })
}

#[derive(Default, Clone, Copy)]
struct MonthSums {
    held: u32,
    cancelled: u32,
    no_show: u32,
    charged: i64,
    received: i64,
}

/// A year at a glance: twelve months of held, cancelled and missed patient visits, what
/// was charged and what was received; the cards opened in the year; held visits by kind.
pub fn income_year(
    year: u16,
    visits: &[Visit],
    charges: &[Charge],
    payments: &[Payment],
    patients: &[Patient],
) -> Result<IncomeYear, DomainError> {
    validate_year(year)?;
    // The month (1–12) of a date in this year.
    let month_in_year = |date: &str| {
        parse_date(date)
            .filter(|(y, _, _)| *y == i32::from(year))
            .map(|(_, m, _)| m)
    };
    let mut sums: BTreeMap<u32, MonthSums> = (1..=12).map(|m| (m, MonthSums::default())).collect();
    let mut kinds: Vec<KindCount> = VISIT_KINDS
        .iter()
        .map(|kind| KindCount {
            kind: *kind,
            count: 0,
        })
        .collect();
    for visit in visits.iter().filter(|v| v.patient_id.is_some()) {
        let Some(sum) = month_in_year(&visit.date).and_then(|m| sums.get_mut(&m)) else {
            continue;
        };
        match visit.status {
            VisitStatus::Held => {
                sum.held = sum.held.saturating_add(1);
                for entry in kinds.iter_mut().filter(|k| k.kind == visit.kind) {
                    entry.count = entry.count.saturating_add(1);
                }
            }
            VisitStatus::Cancelled => sum.cancelled = sum.cancelled.saturating_add(1),
            VisitStatus::NoShow => sum.no_show = sum.no_show.saturating_add(1),
            VisitStatus::Planned | VisitStatus::Unknown => {}
        }
        sum.charged = sum.charged.saturating_add(i64::from(visit_charge(visit)));
    }
    for charge in charges.iter().filter(|c| !c.voided) {
        if let Some(sum) = month_in_year(&charge.date).and_then(|m| sums.get_mut(&m)) {
            sum.charged = sum.charged.saturating_add(i64::from(charge.amount));
        }
    }
    for payment in payments.iter().filter(|p| !p.voided) {
        if let Some(sum) = month_in_year(&payment.date).and_then(|m| sums.get_mut(&m)) {
            sum.received = sum.received.saturating_add(i64::from(payment.amount));
        }
    }

    let (mut charged, mut received) = (0_i64, 0_i64);
    let mut months = Vec::with_capacity(12);
    for (month, sum) in sums {
        charged = charged.saturating_add(sum.charged);
        received = received.saturating_add(sum.received);
        months.push(IncomeMonth {
            month: format!("{year:04}-{month:02}"),
            held: sum.held,
            cancelled: sum.cancelled,
            no_show: sum.no_show,
            charged: to_agorot(sum.charged)?,
            received: to_agorot(sum.received)?,
        });
    }
    let new_patients = patients
        .iter()
        .filter(|p| month_in_year(&p.created_on).is_some())
        .count();
    kinds.retain(|k| k.count > 0);
    // Stable: kinds that are as frequent stay in the order the screens list them.
    kinds.sort_by(|a, b| b.count.cmp(&a.count));
    Ok(IncomeYear {
        year,
        months,
        charged: to_agorot(charged)?,
        received: to_agorot(received)?,
        new_patients: u32::try_from(new_patients).unwrap_or(u32::MAX),
        by_kind: kinds,
    })
}

// ------------------------------------------------------------------ what an erased card leaves

/// What the diary calls the line of the cards that were erased.
pub const ERASED_LABEL: &str = "רשומות של כרטיסים שנמחקו";

fn kind_total(held: &[KindCount]) -> u32 {
    held.iter()
        .fold(0_u32, |sum, entry| sum.saturating_add(entry.count))
}

fn method_total(received: &[ErasedMethod]) -> i64 {
    received
        .iter()
        .fold(0_i64, |sum, entry| sum.saturating_add(entry.amount))
}

fn add_kind(held: &mut Vec<KindCount>, kind: VisitKind, count: u32) {
    match held.iter_mut().find(|entry| entry.kind == kind) {
        Some(entry) => entry.count = entry.count.saturating_add(count),
        None => held.push(KindCount { kind, count }),
    }
}

fn add_method(received: &mut Vec<ErasedMethod>, method: PayMethod, amount: i64) {
    match received.iter_mut().find(|entry| entry.method == method) {
        Some(entry) => entry.amount = entry.amount.saturating_add(amount),
        None => received.push(ErasedMethod { method, amount }),
    }
}

/// Folds a card that is about to be erased into the anonymous tally (D-179): exactly what
/// [`income_year`] and [`diary_month`] count of these records, by month, and nothing else.
/// The records are one patient's. A record whose date is not a date is counted nowhere, as
/// in the summaries.
pub fn fold_erased(
    tally: &mut ErasedTally,
    visits: &[Visit],
    charges: &[Charge],
    payments: &[Payment],
) {
    fn month<'a>(tally: &'a mut ErasedTally, date: &str) -> Option<&'a mut ErasedMonth> {
        month_of(date).map(|key| tally.months.entry(key).or_default())
    }
    for visit in visits.iter().filter(|v| v.patient_id.is_some()) {
        let charge = i64::from(visit_charge(visit));
        // A planned visit that costs nothing is in no summary: it leaves no month behind.
        if visit.status == VisitStatus::Planned || visit.status == VisitStatus::Unknown {
            continue;
        }
        let Some(sum) = month(tally, &visit.date) else {
            continue;
        };
        match visit.status {
            VisitStatus::Held => add_kind(&mut sum.held, visit.kind, 1),
            VisitStatus::Cancelled => sum.cancelled = sum.cancelled.saturating_add(1),
            VisitStatus::NoShow => sum.no_show = sum.no_show.saturating_add(1),
            VisitStatus::Planned | VisitStatus::Unknown => {}
        }
        sum.charged = sum.charged.saturating_add(charge);
    }
    for charge in charges.iter().filter(|c| !c.voided) {
        if let Some(sum) = month(tally, &charge.date) {
            sum.charged = sum.charged.saturating_add(i64::from(charge.amount));
        }
    }
    for payment in payments.iter().filter(|p| !p.voided) {
        if let Some(sum) = month(tally, &payment.date) {
            add_method(&mut sum.received, payment.method, i64::from(payment.amount));
            if blank(&payment.receipt) {
                sum.missing_receipts = sum.missing_receipts.saturating_add(1);
            }
        }
    }
}

/// Counts a card that is about to be erased among the cards opened in its month.
pub fn fold_erased_card(cards: &mut ErasedCards, patient: &Patient) {
    if let Some(month) = month_of(&patient.created_on) {
        let count = cards.months.entry(month).or_default();
        *count = count.saturating_add(1);
    }
}

/// The yearly summary with what the erased cards counted for in it (D-179): the same
/// figures it showed while the cards were there.
pub fn income_with_erased(
    mut income: IncomeYear,
    tally: &ErasedTally,
    cards: &ErasedCards,
) -> Result<IncomeYear, DomainError> {
    let (mut charged, mut received) = (0_i64, 0_i64);
    for month in &mut income.months {
        if let Some(erased) = tally.months.get(&month.month) {
            month.held = month.held.saturating_add(kind_total(&erased.held));
            month.cancelled = month.cancelled.saturating_add(erased.cancelled);
            month.no_show = month.no_show.saturating_add(erased.no_show);
            month.charged = to_agorot(i64::from(month.charged).saturating_add(erased.charged))?;
            month.received = to_agorot(
                i64::from(month.received).saturating_add(method_total(&erased.received)),
            )?;
            // A kind of a later version is counted among the held visits of the month but
            // in no kind, as `income_year` counts such a visit.
            for entry in erased.held.iter().filter(|e| VISIT_KINDS.contains(&e.kind)) {
                add_kind(&mut income.by_kind, entry.kind, entry.count);
            }
        }
        charged = charged.saturating_add(i64::from(month.charged));
        received = received.saturating_add(i64::from(month.received));
    }
    income.charged = to_agorot(charged)?;
    income.received = to_agorot(received)?;
    let year = format!("{:04}-", income.year);
    let opened = cards
        .months
        .iter()
        .filter(|(month, _)| month.starts_with(&year))
        .fold(0_u32, |sum, (_, count)| sum.saturating_add(*count));
    income.new_patients = income.new_patients.saturating_add(opened);
    income.by_kind.retain(|entry| entry.count > 0);
    // The order of `income_year`: the screens' own order, then the most frequent first.
    let place = |kind: VisitKind| VISIT_KINDS.iter().position(|k| *k == kind);
    income
        .by_kind
        .sort_by_key(|entry| place(entry.kind).unwrap_or(VISIT_KINDS.len()));
    income.by_kind.sort_by(|a, b| b.count.cmp(&a.count));
    Ok(income)
}

/// The diary of a month with the one anonymous line of the erased cards (D-179). Its sums
/// are added to the month's totals, to the totals by method and to the payments with no
/// receipt; they belong to no day.
pub fn diary_with_erased(
    mut diary: DiaryMonth,
    tally: &ErasedTally,
) -> Result<DiaryMonth, DomainError> {
    let Some(erased) = tally.months.get(&diary.month) else {
        return Ok(diary);
    };
    let received = method_total(&erased.received);
    let line = DiaryErased {
        label: ERASED_LABEL.to_owned(),
        held: kind_total(&erased.held),
        cancelled: erased.cancelled,
        no_show: erased.no_show,
        charged: to_agorot(erased.charged)?,
        received: to_agorot(received)?,
    };
    diary.charged = to_agorot(i64::from(diary.charged).saturating_add(erased.charged))?;
    diary.received = to_agorot(i64::from(diary.received).saturating_add(received))?;
    for entry in erased.received.iter().filter(|entry| entry.amount > 0) {
        match diary
            .by_method
            .iter_mut()
            .find(|total| total.method == entry.method)
        {
            Some(total) => {
                total.amount = to_agorot(i64::from(total.amount).saturating_add(entry.amount))?;
            }
            None => diary.by_method.push(MethodTotal {
                method: entry.method,
                amount: to_agorot(entry.amount)?,
            }),
        }
    }
    // The order of `diary_month`: the order the screens list the methods in.
    let place = |method: PayMethod| PAY_METHODS.iter().position(|m| *m == method);
    diary
        .by_method
        .sort_by_key(|total| place(total.method).unwrap_or(PAY_METHODS.len()));
    diary.missing_receipts = diary
        .missing_receipts
        .saturating_add(erased.missing_receipts);
    let empty = line.held == 0
        && line.cancelled == 0
        && line.no_show == 0
        && line.charged == 0
        && line.received == 0;
    diary.erased = (!empty).then_some(line);
    Ok(diary)
}

/// What the erased cards received (not voided) in the month of `date`, for the overview.
#[must_use]
pub fn erased_received(tally: &ErasedTally, date: &str) -> i64 {
    month_of(date)
        .and_then(|month| tally.months.get(&month))
        .map_or(0, |erased| method_total(&erased.received))
}

/// The alert of rows that could not be read (D-178); `None` when every row was read.
#[must_use]
pub fn unreadable_alert(rows: u32, today: &str) -> Option<Alert> {
    let text = match rows {
        0 => return None,
        1 => "רישום אחד בקליניקה לא נקרא, ולכן הוא לא מוצג ולא נספר בסיכומים. \
              הוא לא נמחק ולא שונה. כדאי לשחזר מגיבוי, או לפנות לתמיכה."
            .to_owned(),
        n => format!(
            "{n} רישומים בקליניקה לא נקראו, ולכן הם לא מוצגים ולא נספרים בסיכומים. \
             הם לא נמחקו ולא שונו. כדאי לשחזר מגיבוי, או לפנות לתמיכה."
        ),
    };
    Some(Alert {
        kind: AlertKind::Unreadable,
        patient_id: None,
        patient_name: None,
        visit_id: None,
        task_id: None,
        text,
        since: today.to_owned(),
    })
}

// ------------------------------------------------------------------ messages

/// What goes into the placeholders of a message. The date and the time are given as they
/// are stored; [`fill_message`] writes them the way she reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageValues {
    /// The parent the message is for; empty: "רב", so the text opens "שלום רב".
    pub parent: String,
    pub child: String,
    /// The visit's ISO date: fills `{תאריך}` and `{יום}`.
    pub date: Option<String>,
    /// The visit's start, minutes from midnight.
    pub start: Option<u16>,
    pub place: String,
    pub amount: Option<Agorot>,
    /// Her own name, as she signs.
    pub me: String,
}

/// The name to greet in a message: the first contact that has one, in her order.
#[must_use]
pub fn message_parent(patient: &Patient) -> String {
    patient
        .contacts
        .iter()
        .map(|c| clean_line(&c.name))
        .find(|name| !name.is_empty())
        .unwrap_or_default()
}

fn placeholder_value(placeholder: &str, values: &MessageValues) -> String {
    match placeholder {
        "{הורה}" => {
            let parent = clean_line(&values.parent);
            if parent.is_empty() {
                "רב".to_owned()
            } else {
                parent
            }
        }
        "{ילד}" => clean_line(&values.child),
        "{יום}" => values
            .date
            .as_deref()
            .map(day_name_of)
            .unwrap_or_default()
            .to_owned(),
        "{תאריך}" => values.date.as_deref().map(display_date).unwrap_or_default(),
        "{שעה}" => values.start.map(display_time).unwrap_or_default(),
        "{מקום}" => clean_line(&values.place),
        "{סכום}" => values.amount.map(money).unwrap_or_default(),
        "{שמי}" => clean_line(&values.me),
        _ => String::new(),
    }
}

/// Her message text with every placeholder of [`super::PLACEHOLDERS`] filled in. One pass:
/// a value is never read again as a placeholder, and anything else in braces stays as typed.
#[must_use]
pub fn fill_message(template: &str, values: &MessageValues) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('{') {
        let (before, from_brace) = rest.split_at(at);
        out.push_str(before);
        let known = super::PLACEHOLDERS.iter().find_map(|placeholder| {
            from_brace
                .strip_prefix(placeholder)
                .map(|after| (*placeholder, after))
        });
        match known {
            Some((placeholder, after)) => {
                out.push_str(&placeholder_value(placeholder, values));
                rest = after;
            }
            None => {
                out.push('{');
                rest = from_brace.strip_prefix('{').unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------ history lines

fn push_field(lines: &mut Vec<String>, label: &str, value: &str) {
    let value = clean_line(value);
    if !value.is_empty() {
        lines.push(format!("{label}: {value}"));
    }
}

fn push_voided(lines: &mut Vec<String>, voided: bool, reason: &str) {
    if voided {
        lines.push("בוטל: כן".to_owned());
        push_field(lines, "סיבת הביטול", reason);
    }
}

/// A saved state of a visit in her words, a line for each field that says something.
#[must_use]
pub fn visit_history_lines(visit: &Visit, patient_name: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    push_field(&mut lines, "מטופל", patient_name.unwrap_or_default());
    push_field(&mut lines, "סוג", kind_label(visit.kind));
    push_field(&mut lines, "כותרת", &visit.title);
    push_field(&mut lines, "תאריך", &display_date(&visit.date));
    push_field(&mut lines, "שעה", &display_time(visit.start));
    lines.push(format!("משך: {} דקות", visit.minutes));
    push_field(&mut lines, "מקום", &visit.place);
    push_field(&mut lines, "מצב", status_label(visit.status));
    push_field(&mut lines, "מה קרה", &visit.status_note);
    if visit.kind != VisitKind::Block {
        if visit.fee == 0 {
            lines.push("מחיר: ללא חיוב".to_owned());
        } else {
            lines.push(format!("מחיר: {} ₪", money(visit.fee)));
        }
    }
    if visit.charge_missed {
        lines.push("חיוב גם אם לא התקיימה: כן".to_owned());
    }
    if visit.series_id.is_some() {
        lines.push("פגישה חוזרת: כן".to_owned());
    }
    push_field(&mut lines, "הערה", &visit.note);
    lines
}

/// A saved state of a payment in her words.
#[must_use]
pub fn payment_history_lines(payment: &Payment, patient_name: &str) -> Vec<String> {
    let mut lines = Vec::new();
    push_field(&mut lines, "מטופל", patient_name);
    push_field(&mut lines, "תאריך", &display_date(&payment.date));
    lines.push(format!("סכום: {} ₪", money(payment.amount)));
    push_field(&mut lines, "אמצעי תשלום", method_label(payment.method));
    push_field(&mut lines, "מספר קבלה", &payment.receipt);
    push_field(&mut lines, "מי שילם", &payment.payer);
    push_field(&mut lines, "הערה", &payment.note);
    push_voided(&mut lines, payment.voided, &payment.void_reason);
    lines
}

/// A saved state of a charge in her words.
#[must_use]
pub fn charge_history_lines(charge: &Charge, patient_name: &str) -> Vec<String> {
    let mut lines = Vec::new();
    push_field(&mut lines, "מטופל", patient_name);
    push_field(&mut lines, "תאריך", &display_date(&charge.date));
    lines.push(format!("סכום: {} ₪", money(charge.amount)));
    push_field(&mut lines, "על מה", &charge.what);
    push_voided(&mut lines, charge.voided, &charge.void_reason);
    lines
}

/// A saved state of a session note in her words: the formal record, line by line, and one
/// line that says only whether the personal note was written or changed since `previous`
/// (the state saved before this one; `None` for the first). The personal note's own text is
/// never among the lines: it is hers alone (D-171).
#[must_use]
pub fn note_history_lines(
    note: &SessionNote,
    patient_name: &str,
    previous: Option<&SessionNote>,
) -> Vec<String> {
    let mut lines = Vec::new();
    push_field(&mut lines, "מטופל", patient_name);
    push_field(&mut lines, "תאריך", &display_date(&note.date));
    let record: Vec<String> = note
        .record
        .lines()
        .map(clean_line)
        .filter(|line| !line.is_empty())
        .collect();
    if record.is_empty() {
        lines.push("רשומה: ריקה".to_owned());
    } else {
        lines.push("רשומה:".to_owned());
        lines.extend(record);
    }
    let personal = note.personal.trim();
    let about_personal = match previous.map(|p| p.personal.trim()) {
        None if personal.is_empty() => "תרשומת אישית: אין",
        None => "תרשומת אישית: נכתבה",
        Some(before) if before == personal && personal.is_empty() => "תרשומת אישית: אין",
        Some(before) if before == personal => "תרשומת אישית: בלי שינוי",
        Some(_) if personal.is_empty() => "תרשומת אישית: נמחקה",
        Some("") => "תרשומת אישית: נכתבה",
        Some(_) => "תרשומת אישית: השתנתה",
    };
    lines.push(about_personal.to_owned());
    if note.case_input_id.is_some() {
        lines.push("הועבר לחומרי התיק: כן".to_owned());
    }
    lines
}

// ------------------------------------------------------------------ the overview

/// An open task whose date passed.
#[must_use]
pub fn task_overdue(task: &ClinicTask, today: &str) -> bool {
    let (Some(due), Some(now)) = (task.due.as_deref().and_then(day_number), day_number(today))
    else {
        return false;
    };
    task.done_on.is_none() && due < now
}

/// Open tasks: overdue first, then by date, then the undated (which is: by date, the
/// undated last). Tasks that were done come after them, the latest done first.
fn task_order(a: &ClinicTask, b: &ClinicTask) -> Ordering {
    let open = a.done_on.is_some().cmp(&b.done_on.is_some());
    let by_date = match (&a.done_on, &b.done_on) {
        (Some(done_a), Some(done_b)) => done_b.cmp(done_a),
        _ => match (&a.due, &b.due) {
            (Some(due_a), Some(due_b)) => due_a.cmp(due_b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
    };
    open.then(by_date)
        .then_with(|| a.title.cmp(&b.title))
        .then_with(|| a.id.cmp(&b.id))
}

/// Tasks as the screens show them, in order: the open ones (overdue first, then by date,
/// then the undated), then the ones that were done. The overview keeps only the open ones.
#[must_use]
pub fn task_views(
    tasks: &[ClinicTask],
    names: &HashMap<String, String>,
    today: &str,
) -> Vec<TaskView> {
    let mut ordered: Vec<&ClinicTask> = tasks.iter().collect();
    ordered.sort_by(|a, b| task_order(a, b));
    ordered
        .into_iter()
        .map(|task| TaskView {
            patient_name: task
                .patient_id
                .as_ref()
                .and_then(|id| names.get(id))
                .cloned(),
            overdue: task_overdue(task, today),
            task: task.clone(),
        })
        .collect()
}

/// How many tasks of a patient are still open.
#[must_use]
pub fn open_task_count(tasks: &[ClinicTask]) -> u32 {
    u32::try_from(tasks.iter().filter(|t| t.done_on.is_none()).count()).unwrap_or(u32::MAX)
}

/// A visit with a patient that was not cancelled: what "a visit" means on the front page
/// (time she blocked for herself is not one).
fn counts_as_visit(visit: &Visit) -> bool {
    visit.kind != VisitKind::Block && visit.status != VisitStatus::Cancelled
}

/// Visits in the week of `date` (Sunday to Saturday) that are not cancelled.
#[must_use]
pub fn week_visit_count(visits: &[Visit], date: &str) -> u32 {
    let Some((sunday, saturday)) = week_of(date) else {
        return 0;
    };
    let count = visits
        .iter()
        .filter(|v| {
            counts_as_visit(v) && (sunday.as_str()..=saturday.as_str()).contains(&v.date.as_str())
        })
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The next day that has visits, when today has none left (none that has not ended yet).
/// `None` while today still has one, or when nothing is ahead. A day she works on is a day
/// with a visit in it, whatever the usual working days are.
#[must_use]
pub fn next_visit_day(visits: &[Visit], today: &Today) -> Option<String> {
    let today_date = today.date.as_str();
    let left_today = visits.iter().any(|v| {
        counts_as_visit(v) && v.date == today_date && visit_end(v) > u32::from(today.minute)
    });
    if left_today {
        return None;
    }
    visits
        .iter()
        .filter(|v| counts_as_visit(v) && v.date.as_str() > today_date)
        .map(|v| v.date.as_str())
        .min()
        .map(str::to_owned)
}
