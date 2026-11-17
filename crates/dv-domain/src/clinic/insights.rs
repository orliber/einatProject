//! "תמונת מצב": how the clinic is doing over a stretch of whole months (D-200).
//!
//! Counts, sums, rates and medians only: no name, no id and no text she wrote ever leaves
//! this function, so the screen is the same in discreet mode. Pure, like the rest of the
//! rules; "today" is given by the caller.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::slots::working_day;
use super::{
    balance, clean_line, date_of_day, day_number, income_with_erased, income_year, month_bounds,
    oldest_uncovered, parse_date, parse_month, weekday, Agorot, Charge, ClinicRecords,
    ClinicSettings, ErasedCards, ErasedMonth, ErasedTally, IncomeMonth, KindCount, MethodTotal,
    Patient, PatientStatus, PayMethod, Payment, Visit, VisitKind, VisitStatus, PAY_METHODS,
    VISIT_KINDS,
};
use crate::DomainError;

/// The longest stretch the screen may ask for, in months.
pub const MAX_INSIGHT_MONTHS: u32 = 24;

/// Visits of one day of the week (0 = Sunday … 6 = Saturday) in the stretch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WeekdayCount {
    pub weekday: u8,
    pub held: u32,
    pub cancelled: u32,
    pub no_show: u32,
}

/// Where the new patients of the stretch came from (`Patient::referred_by`, as she typed it,
/// grouped without regard to case or spaces). A blank one is `לא צוין`; past the first
/// eight, the rest are one row `אחרים`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReferralCount {
    pub source: String,
    pub count: u32,
}

/// What is owed today, by how old the oldest uncovered charge of each family is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DebtBucket {
    /// `עד 30 יום`, `31–60 יום`, `61–90 יום`, `מעל 90 יום`.
    pub label: String,
    /// The bucket's first day of age (0, 31, 61, 91).
    pub from_days: u32,
    pub patients: u32,
    pub amount: Agorot,
}

/// The stretch of the same length just before, for "compared with".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InsightsBefore {
    pub from_month: String,
    pub to_month: String,
    pub held: u32,
    pub received: Agorot,
    pub new_patients: u32,
}

/// The whole picture of a stretch of months.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Insights {
    /// `YYYY-MM`, both included.
    pub from_month: String,
    pub to_month: String,
    /// Patient visits (blocks never count), erased cards included (D-179).
    pub held: u32,
    pub cancelled: u32,
    pub no_show: u32,
    /// Per thousand of held + cancelled + no-show; 0 when there were none.
    pub cancel_rate: u16,
    pub no_show_rate: u16,
    /// Seven rows, Sunday first. Cards that still exist only.
    pub by_weekday: Vec<WeekdayCount>,
    /// Held visits by kind, erased cards included; kinds with none are left out.
    pub by_kind: Vec<KindCount>,
    /// Minutes of held visits, and the minutes of her working hours on working days (without
    /// the days closed for a holiday when she has the holidays on, as free times count them),
    /// from the first day of the stretch up to today (or the stretch's end, the earlier).
    /// Cards that still exist only.
    pub held_minutes: u32,
    pub work_minutes: u32,
    /// `held_minutes` per thousand of `work_minutes`; 0 when there are no working minutes.
    pub occupancy: u16,
    /// Cards opened in the stretch (by `created_on`), erased ones included.
    pub new_patients: u32,
    pub referrals: Vec<ReferralCount>,
    /// Feedback meetings held in the stretch: assessments brought to an end. Erased cards
    /// included, as in `by_kind`.
    pub assessments_done: u32,
    /// Medians in days, over the patients whose feedback was held in the stretch: from the
    /// first held intake (or the first held visit, when there was no intake) to the feedback;
    /// and from the last held assessment before it to the feedback. `None` when there is none.
    pub intake_to_feedback_days: Option<u32>,
    pub assessment_to_feedback_days: Option<u32>,
    /// Median days from `waiting_since` to the first held visit, over the patients whose first
    /// held visit was in the stretch and who had a `waiting_since`.
    pub waited_days: Option<u32>,
    /// On the waiting list today, and the median days they have waited so far.
    pub waiting_now: u32,
    pub waiting_now_days: Option<u32>,
    /// Charged and received in the stretch, erased cards included; received by method.
    pub charged: Agorot,
    pub received: Agorot,
    pub by_method: Vec<MethodTotal>,
    /// Today's debt by age, four rows always.
    pub debt: Vec<DebtBucket>,
    /// One row a month of the stretch, as the income year counts it (erased cards included).
    pub months: Vec<IncomeMonth>,
    pub before: InsightsBefore,
}

/// The referral sources listed one by one; the rest are one row, [`REFERRAL_OTHERS`].
const REFERRALS_SHOWN: usize = 8;
const REFERRAL_BLANK: &str = "לא צוין";
const REFERRAL_OTHERS: &str = "אחרים";

/// The ages of a debt: the first day of each bucket, and its label.
const DEBT_BUCKETS: [(u32, &str); 4] = [
    (0, "עד 30 יום"),
    (31, "31–60 יום"),
    (61, "61–90 יום"),
    (91, "מעל 90 יום"),
];

/// The picture of the months `from_month` to `to_month` (`YYYY-MM`, both included, at most
/// [`MAX_INSIGHT_MONTHS`]). `records` are all the clinic's records; `tally` and `cards` what
/// erased cards counted for (D-179).
///
/// How the figures are counted:
/// * a visit counts when it has a patient (a block never does) and is held, cancelled or
///   missed on a date of the stretch. The counts, the money and `months` are the income
///   year's ([`income_year`] with [`income_with_erased`]), so they read as that screen does;
///   `by_kind`, `by_method` and `assessments_done` add the same erased months to the cards
///   that still exist;
/// * `by_method` lists the methods in the screens' order, and a method of a later version
///   last, as the diary does, so the methods add up to `received`;
/// * a rate or the occupancy is per thousand, rounded to the nearest (a half up);
/// * a median of an even number of values is the mean of the two middle ones, rounded down;
/// * the times to the feedback are taken to the patient's latest feedback held in the
///   stretch: from the first intake held up to it (or the first visit held, when no intake
///   was), and from the last assessment held before it (a patient with none is left out);
/// * a family on the waiting list with no `waiting_since` has waited since its card was
///   opened, as the waiting alert counts it; a negative wait is left out of a median;
/// * a debt is aged from its oldest uncovered charge to `today` (a charge dated ahead is 0
///   days old); an amount owed is never left out of the total, even with no date to age it;
/// * the stretch before has the same number of months; a month of it before 2000 counts
///   nothing.
///
/// Refused, with a sentence she can act on: a month that is not one, a stretch in the wrong
/// order or longer than [`MAX_INSIGHT_MONTHS`], a `today` that is not a date, and a sum that
/// does not fit [`Agorot`].
pub fn insights(
    records: &ClinicRecords<'_>,
    settings: &ClinicSettings,
    from_month: &str,
    to_month: &str,
    today: &str,
    tally: &ErasedTally,
    cards: &ErasedCards,
) -> Result<Insights, DomainError> {
    let (Some(first), Some(last)) = (month_index(from_month), month_index(to_month)) else {
        return refuse("צריך לבחור חודש התחלה וחודש סיום לתמונת המצב.");
    };
    if last < first {
        return refuse("חודש הסיום צריך להיות אותו חודש או אחרי חודש ההתחלה.");
    }
    if i64::from(last - first) >= i64::from(MAX_INSIGHT_MONTHS) {
        return refuse(format!(
            "אפשר להציג עד {MAX_INSIGHT_MONTHS} חודשים בבת אחת. כדאי לבחור תקופה קצרה יותר."
        ));
    }
    let Some(now) = day_number(today) else {
        return refuse("התאריך של היום לא התקבל כמו שצריך. כדאי לסגור את המסך ולפתוח אותו שוב.");
    };
    let stretch = Months { first, last };
    let before = Months {
        first: first - (last - first + 1),
        last: first - 1,
    };

    let income = income_months(records, tally, cards, before.first, last)?;
    let months: Vec<IncomeMonth> = stretch.indexes().map(|i| month_row(&income, i)).collect();
    let sums = Sums::of(&months);
    let before_sums = Sums::of(
        &before
            .indexes()
            .map(|i| month_row(&income, i))
            .collect::<Vec<_>>(),
    );

    let live = live_visits(records.visits, stretch);
    let seen = held_by_patient(records.visits);
    let (intake_to_feedback_days, assessment_to_feedback_days) = to_feedback(&seen, stretch);
    let (waiting_now, waiting_now_days) = waiting_now(records.patients, now);
    let work_minutes = work_minutes(settings, from_month, to_month, now);
    Ok(Insights {
        from_month: month_text(first),
        to_month: month_text(last),
        held: sums.held,
        cancelled: sums.cancelled,
        no_show: sums.no_show,
        cancel_rate: per_thousand(sums.cancelled, sums.visits()),
        no_show_rate: per_thousand(sums.no_show, sums.visits()),
        by_weekday: live.by_weekday,
        by_kind: by_kind(live.kinds, tally, stretch),
        held_minutes: live.held_minutes,
        work_minutes,
        occupancy: per_thousand(live.held_minutes, u64::from(work_minutes)),
        new_patients: opened(records.patients, cards, stretch),
        referrals: referrals(records.patients, stretch),
        assessments_done: feedbacks_with_erased(live.feedbacks, tally, stretch),
        intake_to_feedback_days,
        assessment_to_feedback_days,
        waited_days: waited(records.patients, &seen, stretch),
        waiting_now,
        waiting_now_days,
        charged: to_agorot(sums.charged)?,
        received: to_agorot(sums.received)?,
        by_method: by_method(records.payments, tally, stretch)?,
        debt: debt(records, now)?,
        months,
        before: InsightsBefore {
            from_month: month_text(before.first),
            to_month: month_text(before.last),
            held: before_sums.held,
            received: to_agorot(before_sums.received)?,
            new_patients: opened(records.patients, cards, before),
        },
    })
}

// ------------------------------------------------------------------ helpers

fn refuse<T>(text: impl Into<String>) -> Result<T, DomainError> {
    Err(DomainError::Invalid(text.into()))
}

/// A sum that does not fit is refused, never wrapped or clamped into a wrong figure (the
/// sentence of the income screen).
fn to_agorot(total: i64) -> Result<Agorot, DomainError> {
    Agorot::try_from(total)
        .or_else(|_| refuse("הסכום הכולל גדול מדי לחישוב. כדאי לבדוק את הסכומים שנרשמו."))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `part` per thousand of `whole`, rounded to the nearest; 0 when `whole` is 0.
fn per_thousand(part: impl Into<u64>, whole: u64) -> u16 {
    if whole == 0 {
        return 0;
    }
    let value = part.into().saturating_mul(1_000).saturating_add(whole / 2) / whole;
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The middle value; for an even number of values, the mean of the two middle ones,
/// rounded down.
fn median(mut values: Vec<u32>) -> Option<u32> {
    values.sort_unstable();
    let middle = values.len() / 2;
    let upper = *values.get(middle)?;
    if values.len() % 2 == 1 {
        return Some(upper);
    }
    let lower = *values.get(middle.checked_sub(1)?)?;
    Some(lower + (upper - lower) / 2)
}

/// A visit of a patient: never a block, never one without a patient.
fn is_patient_visit(visit: &Visit) -> bool {
    visit.kind != VisitKind::Block
        && visit
            .patient_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
}

// ------------------------------------------------------------------ months

/// A month as one number, `year × 12 + month − 1`, so a stretch is a plain range.
fn month_index(month: &str) -> Option<i32> {
    let (y, m) = parse_month(month)?;
    Some(y * 12 + i32::try_from(m).ok()? - 1)
}

/// The month of an ISO date, numbered as [`month_index`] numbers it.
fn date_month(date: &str) -> Option<i32> {
    let (y, m, _) = parse_date(date)?;
    Some(y * 12 + i32::try_from(m).ok()? - 1)
}

fn month_text(index: i32) -> String {
    format!(
        "{:04}-{:02}",
        index.div_euclid(12),
        index.rem_euclid(12) + 1
    )
}

/// Whole months, both ends included.
#[derive(Clone, Copy)]
struct Months {
    first: i32,
    last: i32,
}

impl Months {
    fn holds(self, index: i32) -> bool {
        (self.first..=self.last).contains(&index)
    }

    fn has_date(self, date: &str) -> bool {
        date_month(date).is_some_and(|i| self.holds(i))
    }

    fn has_month(self, month: &str) -> bool {
        month_index(month).is_some_and(|i| self.holds(i))
    }

    fn indexes(self) -> std::ops::RangeInclusive<i32> {
        self.first..=self.last
    }
}

/// Every month of the years from `first` to `last` as the income year counts it, erased
/// cards included, by month number. A year before 2000 has no records and no row.
fn income_months(
    records: &ClinicRecords<'_>,
    tally: &ErasedTally,
    cards: &ErasedCards,
    first: i32,
    last: i32,
) -> Result<HashMap<i32, IncomeMonth>, DomainError> {
    let wanted = Months { first, last };
    // The income year counts a visit by its patient; a block never has one once it is
    // saved, and here it never counts even if it had.
    let visits: Vec<Visit> = records
        .visits
        .iter()
        .filter(|v| is_patient_visit(v) && wanted.has_date(&v.date))
        .cloned()
        .collect();
    let mut rows = HashMap::new();
    for year in first.div_euclid(12).max(2000)..=last.div_euclid(12) {
        let Ok(year) = u16::try_from(year) else {
            continue;
        };
        // New patients are counted by month below, so no card is given here.
        let income = income_year(year, &visits, records.charges, records.payments, &[])?;
        for row in income_with_erased(income, tally, cards)?.months {
            if let Some(index) = month_index(&row.month) {
                rows.insert(index, row);
            }
        }
    }
    Ok(rows)
}

fn month_row(rows: &HashMap<i32, IncomeMonth>, index: i32) -> IncomeMonth {
    rows.get(&index).cloned().unwrap_or_else(|| IncomeMonth {
        month: month_text(index),
        held: 0,
        cancelled: 0,
        no_show: 0,
        charged: 0,
        received: 0,
    })
}

/// The months' counts and sums added up, the money in a width that cannot overflow.
struct Sums {
    held: u32,
    cancelled: u32,
    no_show: u32,
    charged: i64,
    received: i64,
}

impl Sums {
    fn of(rows: &[IncomeMonth]) -> Self {
        rows.iter().fold(
            Self {
                held: 0,
                cancelled: 0,
                no_show: 0,
                charged: 0,
                received: 0,
            },
            |sum, row| Self {
                held: sum.held.saturating_add(row.held),
                cancelled: sum.cancelled.saturating_add(row.cancelled),
                no_show: sum.no_show.saturating_add(row.no_show),
                charged: sum.charged.saturating_add(i64::from(row.charged)),
                received: sum.received.saturating_add(i64::from(row.received)),
            },
        )
    }

    /// Held, cancelled and missed: what the rates are out of.
    fn visits(&self) -> u64 {
        u64::from(self.held) + u64::from(self.cancelled) + u64::from(self.no_show)
    }
}

/// The months of the tally that fall in `months`.
fn erased_in(tally: &ErasedTally, months: Months) -> impl Iterator<Item = &ErasedMonth> {
    tally
        .months
        .iter()
        .filter(move |(month, _)| months.has_month(month))
        .map(|(_, erased)| erased)
}

// ------------------------------------------------------------------ visits

/// What the cards that still exist show of the stretch's visits.
struct Live {
    by_weekday: Vec<WeekdayCount>,
    /// Held visits by kind, every kind of the screens, in their order.
    kinds: Vec<KindCount>,
    held_minutes: u32,
    /// Feedback meetings held.
    feedbacks: u32,
}

fn live_visits(visits: &[Visit], stretch: Months) -> Live {
    let mut live = Live {
        by_weekday: (0..7)
            .map(|weekday| WeekdayCount {
                weekday,
                held: 0,
                cancelled: 0,
                no_show: 0,
            })
            .collect(),
        kinds: VISIT_KINDS
            .iter()
            .map(|kind| KindCount {
                kind: *kind,
                count: 0,
            })
            .collect(),
        held_minutes: 0,
        feedbacks: 0,
    };
    for visit in visits
        .iter()
        .filter(|v| is_patient_visit(v) && stretch.has_date(&v.date))
    {
        let Some(day) = weekday(&visit.date).and_then(|d| live.by_weekday.get_mut(usize::from(d)))
        else {
            continue;
        };
        match visit.status {
            VisitStatus::Held => {
                day.held = day.held.saturating_add(1);
                live.held_minutes = live.held_minutes.saturating_add(u32::from(visit.minutes));
                for entry in live.kinds.iter_mut().filter(|k| k.kind == visit.kind) {
                    entry.count = entry.count.saturating_add(1);
                }
                if visit.kind == VisitKind::Feedback {
                    live.feedbacks = live.feedbacks.saturating_add(1);
                }
            }
            VisitStatus::Cancelled => day.cancelled = day.cancelled.saturating_add(1),
            VisitStatus::NoShow => day.no_show = day.no_show.saturating_add(1),
            VisitStatus::Planned | VisitStatus::Unknown => {}
        }
    }
    live
}

/// Held visits by kind, the erased months added as [`income_with_erased`] adds them: kinds
/// with none left out, the most frequent first, as frequent ones in the screens' order.
fn by_kind(mut kinds: Vec<KindCount>, tally: &ErasedTally, stretch: Months) -> Vec<KindCount> {
    for erased in erased_in(tally, stretch) {
        for entry in &erased.held {
            for kind in kinds.iter_mut().filter(|k| k.kind == entry.kind) {
                kind.count = kind.count.saturating_add(entry.count);
            }
        }
    }
    kinds.retain(|k| k.count > 0);
    // Stable: `kinds` starts in the screens' order.
    kinds.sort_by(|a, b| b.count.cmp(&a.count));
    kinds
}

/// The feedback meetings held: those of the cards that still exist, and what the erased
/// cards counted for, as [`by_kind`] counts them.
fn feedbacks_with_erased(live: u32, tally: &ErasedTally, stretch: Months) -> u32 {
    erased_in(tally, stretch)
        .flat_map(|erased| &erased.held)
        .filter(|entry| entry.kind == VisitKind::Feedback)
        .fold(live, |sum, entry| sum.saturating_add(entry.count))
}

/// One held visit of a patient, as the times of the pathway need it.
#[derive(Clone, Copy)]
struct Seen {
    day: i64,
    start: u16,
    month: i32,
    kind: VisitKind,
}

impl Seen {
    fn when(self) -> (i64, u16) {
        (self.day, self.start)
    }
}

/// Every patient's held visits (never a block), oldest first.
fn held_by_patient(visits: &[Visit]) -> HashMap<&str, Vec<Seen>> {
    let mut held: HashMap<&str, Vec<Seen>> = HashMap::new();
    for visit in visits
        .iter()
        .filter(|v| is_patient_visit(v) && v.status == VisitStatus::Held)
    {
        let (Some(id), Some(day), Some(month)) = (
            visit.patient_id.as_deref(),
            day_number(&visit.date),
            date_month(&visit.date),
        ) else {
            continue;
        };
        held.entry(id).or_default().push(Seen {
            day,
            start: visit.start,
            month,
            kind: visit.kind,
        });
    }
    for seen in held.values_mut() {
        seen.sort_by_key(|s| s.when());
    }
    held
}

fn days(from: i64, to: i64) -> Option<u32> {
    u32::try_from(to.checked_sub(from)?).ok()
}

/// The medians of (first intake → feedback, last assessment → feedback), over the patients
/// whose feedback was held in the stretch.
fn to_feedback(seen: &HashMap<&str, Vec<Seen>>, stretch: Months) -> (Option<u32>, Option<u32>) {
    let (mut from_intake, mut from_assessment) = (Vec::new(), Vec::new());
    for visits in seen.values() {
        let Some(feedback) = visits
            .iter()
            .rev()
            .find(|s| s.kind == VisitKind::Feedback && stretch.holds(s.month))
        else {
            continue;
        };
        let up_to: Vec<&Seen> = visits
            .iter()
            .take_while(|s| s.when() <= feedback.when())
            .collect();
        let first = up_to
            .iter()
            .find(|s| s.kind == VisitKind::Intake)
            .or_else(|| up_to.first());
        if let Some(wait) = first.and_then(|s| days(s.day, feedback.day)) {
            from_intake.push(wait);
        }
        let assessment = up_to
            .iter()
            .rev()
            .find(|s| s.kind == VisitKind::Assessment && s.when() < feedback.when());
        if let Some(wait) = assessment.and_then(|s| days(s.day, feedback.day)) {
            from_assessment.push(wait);
        }
    }
    (median(from_intake), median(from_assessment))
}

/// The median days from `waiting_since` to the first held visit, over the patients whose
/// first held visit was in the stretch.
fn waited(patients: &[Patient], seen: &HashMap<&str, Vec<Seen>>, stretch: Months) -> Option<u32> {
    let waits = patients.iter().filter_map(|p| {
        let since = day_number(p.waiting_since.as_deref()?)?;
        let first = seen.get(p.id.as_str())?.first()?;
        if !stretch.holds(first.month) {
            return None;
        }
        days(since, first.day)
    });
    median(waits.collect())
}

/// How many are on the waiting list today, and the median days they have waited.
fn waiting_now(patients: &[Patient], now: i64) -> (u32, Option<u32>) {
    let waiting: Vec<&Patient> = patients
        .iter()
        .filter(|p| p.status == PatientStatus::Waiting)
        .collect();
    let waits = waiting.iter().filter_map(|p| {
        // As the waiting alert has it: with no day it joined, since the card was opened.
        let since = p
            .waiting_since
            .as_deref()
            .and_then(day_number)
            .or_else(|| day_number(&p.created_on))?;
        days(since, now)
    });
    (count(waiting.len()), median(waits.collect()))
}

/// Her working minutes on her working days (without the days closed for a holiday when she
/// has the holidays on, as free times count them), from the first day of the stretch to its
/// last or to today, the earlier.
fn work_minutes(settings: &ClinicSettings, from_month: &str, to_month: &str, now: i64) -> u32 {
    let first = month_bounds(from_month).and_then(|(first, _)| day_number(&first));
    let last = month_bounds(to_month).and_then(|(_, last)| day_number(&last));
    let (Some(first), Some(last)) = (first, last) else {
        return 0;
    };
    let per_day = u32::from(settings.day_end.saturating_sub(settings.day_start));
    (first..=last.min(now))
        .filter_map(date_of_day)
        .filter(|date| working_day(date, settings))
        .fold(0_u32, |sum, _| sum.saturating_add(per_day))
}

// ------------------------------------------------------------------ patients

/// Cards opened in `months`: those that still exist, and the erased ones (D-179).
fn opened(patients: &[Patient], cards: &ErasedCards, months: Months) -> u32 {
    let live = count(
        patients
            .iter()
            .filter(|p| months.has_date(&p.created_on))
            .count(),
    );
    cards
        .months
        .iter()
        .filter(|(month, _)| months.has_month(month))
        .fold(live, |sum, (_, opened)| sum.saturating_add(*opened))
}

/// Where the new patients of the stretch came from. A source is grouped without regard to
/// case or spaces and shown as the card opened first wrote it.
fn referrals(patients: &[Patient], stretch: Months) -> Vec<ReferralCount> {
    let mut new: Vec<&Patient> = patients
        .iter()
        .filter(|p| stretch.has_date(&p.created_on))
        .collect();
    new.sort_by(|a, b| (&a.created_on, &a.id).cmp(&(&b.created_on, &b.id)));
    let mut groups: Vec<(String, ReferralCount)> = Vec::new();
    for patient in new {
        let mut source = clean_line(&patient.referred_by);
        if source.is_empty() {
            source = REFERRAL_BLANK.to_owned();
        }
        let key = source.to_lowercase();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.count = group.count.saturating_add(1),
            None => groups.push((key, ReferralCount { source, count: 1 })),
        }
    }
    let mut rows: Vec<ReferralCount> = groups.into_iter().map(|(_, group)| group).collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.source.cmp(&b.source)));
    if rows.len() > REFERRALS_SHOWN {
        let others = rows
            .split_off(REFERRALS_SHOWN)
            .iter()
            .fold(0_u32, |sum, row| sum.saturating_add(row.count));
        rows.push(ReferralCount {
            source: REFERRAL_OTHERS.to_owned(),
            count: others,
        });
    }
    rows
}

// ------------------------------------------------------------------ money

/// Received in the stretch by method: payments not voided, and the erased months.
fn by_method(
    payments: &[Payment],
    tally: &ErasedTally,
    stretch: Months,
) -> Result<Vec<MethodTotal>, DomainError> {
    // A method of a later version is a total of its own, as in the diary.
    let mut methods: Vec<(PayMethod, i64)> = PAY_METHODS
        .iter()
        .chain(std::iter::once(&PayMethod::Unknown))
        .map(|m| (*m, 0))
        .collect();
    let mut add = |method: PayMethod, amount: i64| {
        for (_, total) in methods.iter_mut().filter(|(m, _)| *m == method) {
            *total = total.saturating_add(amount);
        }
    };
    for payment in payments
        .iter()
        .filter(|p| !p.voided && stretch.has_date(&p.date))
    {
        add(payment.method, i64::from(payment.amount));
    }
    for erased in erased_in(tally, stretch) {
        for entry in &erased.received {
            add(entry.method, entry.amount);
        }
    }
    methods
        .into_iter()
        .filter(|(_, total)| *total > 0)
        .map(|(method, total)| {
            Ok(MethodTotal {
                method,
                amount: to_agorot(total)?,
            })
        })
        .collect()
}

/// What is owed today by the cards that still exist, by the age of each family's oldest
/// uncovered charge.
fn debt(records: &ClinicRecords<'_>, now: i64) -> Result<Vec<DebtBucket>, DomainError> {
    type Own = (Vec<Visit>, Vec<Charge>, Vec<Payment>);
    let mut of: HashMap<&str, Own> = records
        .patients
        .iter()
        .map(|p| (p.id.as_str(), Own::default()))
        .collect();
    for visit in records.visits {
        if let Some(own) = visit.patient_id.as_deref().and_then(|id| of.get_mut(id)) {
            own.0.push(visit.clone());
        }
    }
    for charge in records.charges {
        if let Some(own) = of.get_mut(charge.patient_id.as_str()) {
            own.1.push(charge.clone());
        }
    }
    for payment in records.payments {
        if let Some(own) = of.get_mut(payment.patient_id.as_str()) {
            own.2.push(payment.clone());
        }
    }
    let mut totals = [(0_u32, 0_i64); DEBT_BUCKETS.len()];
    for (visits, charges, payments) in of.values() {
        let due = balance(visits, charges, payments)?.due;
        if due <= 0 {
            continue;
        }
        let age = oldest_uncovered(visits, charges, payments)
            .and_then(|date| day_number(&date))
            .map_or(0, |day| now.saturating_sub(day).max(0));
        let bucket = DEBT_BUCKETS
            .iter()
            .rposition(|(from, _)| i64::from(*from) <= age)
            .unwrap_or(0);
        if let Some((patients, amount)) = totals.get_mut(bucket) {
            *patients = patients.saturating_add(1);
            *amount = amount.saturating_add(i64::from(due));
        }
    }
    DEBT_BUCKETS
        .iter()
        .zip(totals)
        .map(|((from_days, label), (patients, amount))| {
            Ok(DebtBucket {
                label: (*label).to_owned(),
                from_days: *from_days,
                patients,
                amount: to_agorot(amount)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Every name, date and sum here is invented.

    use std::collections::BTreeMap;

    use super::*;
    use crate::clinic::{add_days, ErasedMethod};

    /// A Tuesday. November 2026 has no holiday the clinic closes for.
    const TODAY: &str = "2026-11-17";

    fn card(id: &str, created_on: &str) -> Patient {
        Patient {
            id: id.to_owned(),
            name: "ילד בדוי".to_owned(),
            created_on: created_on.to_owned(),
            ..Patient::default()
        }
    }

    fn referred(id: &str, created_on: &str, by: &str) -> Patient {
        Patient {
            referred_by: by.to_owned(),
            ..card(id, created_on)
        }
    }

    fn listed(id: &str, since: Option<&str>, status: PatientStatus) -> Patient {
        Patient {
            status,
            waiting_since: since.map(str::to_owned),
            ..card(id, "2026-01-05")
        }
    }

    /// An hour at 9:00 with no fee.
    fn seen(id: &str, patient: &str, kind: VisitKind, date: &str, status: VisitStatus) -> Visit {
        Visit {
            id: id.to_owned(),
            patient_id: Some(patient.to_owned()),
            kind,
            date: date.to_owned(),
            start: 9 * 60,
            minutes: 60,
            status,
            ..Visit::default()
        }
    }

    fn held(id: &str, patient: &str, kind: VisitKind, date: &str) -> Visit {
        seen(id, patient, kind, date, VisitStatus::Held)
    }

    fn paid(id: &str, patient: &str, date: &str, amount: Agorot, method: PayMethod) -> Payment {
        Payment {
            id: id.to_owned(),
            patient_id: patient.to_owned(),
            date: date.to_owned(),
            amount,
            method,
            receipt: "1001".to_owned(),
            ..Payment::default()
        }
    }

    fn charge(id: &str, patient: &str, date: &str, amount: Agorot) -> Charge {
        Charge {
            id: id.to_owned(),
            patient_id: patient.to_owned(),
            date: date.to_owned(),
            amount,
            what: "חבילת אבחון".to_owned(),
            ..Charge::default()
        }
    }

    fn kinds(counts: &[(VisitKind, u32)]) -> Vec<KindCount> {
        counts
            .iter()
            .map(|(kind, count)| KindCount {
                kind: *kind,
                count: *count,
            })
            .collect()
    }

    fn methods(totals: &[(PayMethod, Agorot)]) -> Vec<MethodTotal> {
        totals
            .iter()
            .map(|(method, amount)| MethodTotal {
                method: *method,
                amount: *amount,
            })
            .collect()
    }

    fn erased_received(method: PayMethod, amount: i64) -> ErasedMethod {
        ErasedMethod { method, amount }
    }

    /// `days` before [`TODAY`] (after it, when negative).
    fn ago(days: i64) -> String {
        add_days(TODAY, -days).unwrap()
    }

    #[derive(Default)]
    struct Clinic {
        patients: Vec<Patient>,
        visits: Vec<Visit>,
        charges: Vec<Charge>,
        payments: Vec<Payment>,
        tally: ErasedTally,
        cards: ErasedCards,
        reports: HashMap<String, (u32, u32)>,
    }

    impl Clinic {
        fn ask(
            &self,
            settings: &ClinicSettings,
            from: &str,
            to: &str,
            today: &str,
        ) -> Result<Insights, DomainError> {
            let records = ClinicRecords {
                patients: &self.patients,
                visits: &self.visits,
                notes: &[],
                charges: &self.charges,
                payments: &self.payments,
                tasks: &[],
                reports: &self.reports,
            };
            insights(
                &records,
                settings,
                from,
                to,
                today,
                &self.tally,
                &self.cards,
            )
        }

        fn picture(&self, from: &str, to: &str, today: &str) -> Insights {
            self.ask(&ClinicSettings::default(), from, to, today)
                .unwrap()
        }

        /// The income year as its own screen shows it, erased cards included.
        fn income(&self, year: u16) -> Vec<IncomeMonth> {
            let income = income_year(
                year,
                &self.visits,
                &self.charges,
                &self.payments,
                &self.patients,
            )
            .unwrap();
            income_with_erased(income, &self.tally, &self.cards)
                .unwrap()
                .months
        }
    }

    /// The refusal's sentence: Hebrew, with no em-dash.
    fn refusal(result: Result<Insights, DomainError>) -> String {
        match result {
            Err(DomainError::Invalid(text)) => {
                assert!(
                    text.chars().any(|c| ('א'..='ת').contains(&c)),
                    "not Hebrew: {text}"
                );
                assert!(!text.contains('—'), "an em-dash in: {text}");
                text
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn refuses_a_stretch_she_cannot_ask_for() {
        let clinic = Clinic::default();
        let ask = |from: &str, to: &str, today: &str| {
            clinic.ask(&ClinicSettings::default(), from, to, today)
        };
        let not_a_month = refusal(ask("2026-13", "2026-12", TODAY));
        for (from, to) in [
            ("", "2026-11"),
            ("2026-11", "2026-1"),
            ("2026/11", "2026-11"),
            ("11-2026", "2026-11"),
            ("1999-12", "2000-01"),
            ("2200-12", "2201-01"),
            ("2026-11-01", "2026-11"),
        ] {
            assert_eq!(refusal(ask(from, to, TODAY)), not_a_month, "{from} to {to}");
        }
        let wrong_order = refusal(ask("2026-11", "2026-10", TODAY));
        let too_long = refusal(ask("2025-01", "2027-01", TODAY));
        assert!(too_long.contains("24"), "{too_long}");
        let bad_today = refusal(ask("2026-11", "2026-11", "2026-02-30"));
        for today in ["", "17.11.2026", "2026-11-17 ", "2026-11"] {
            assert_eq!(
                refusal(ask("2026-11", "2026-11", today)),
                bad_today,
                "{today}"
            );
        }
        let all = [&not_a_month, &wrong_order, &too_long, &bad_today];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }

        // The longest stretch is fine, and so is one whose stretch before is before 2000.
        assert_eq!(ask("2025-01", "2026-12", TODAY).unwrap().months.len(), 24);
        let first = ask("2000-01", "2000-03", TODAY).unwrap();
        assert_eq!(
            first.before,
            InsightsBefore {
                from_month: "1999-10".to_owned(),
                to_month: "1999-12".to_owned(),
                held: 0,
                received: 0,
                new_patients: 0,
            }
        );
    }

    #[test]
    fn an_empty_clinic_reads_as_nothing_yet() {
        let picture = Clinic::default().picture("2026-11", "2026-11", TODAY);
        let bucket = |label: &str, from_days: u32| DebtBucket {
            label: label.to_owned(),
            from_days,
            patients: 0,
            amount: 0,
        };
        assert_eq!(
            picture,
            Insights {
                from_month: "2026-11".to_owned(),
                to_month: "2026-11".to_owned(),
                held: 0,
                cancelled: 0,
                no_show: 0,
                cancel_rate: 0,
                no_show_rate: 0,
                by_weekday: (0..7)
                    .map(|weekday| WeekdayCount {
                        weekday,
                        held: 0,
                        cancelled: 0,
                        no_show: 0,
                    })
                    .collect(),
                by_kind: Vec::new(),
                held_minutes: 0,
                // The 1st to the 17th: thirteen days, Sunday to Thursday, 8:00 to 20:00.
                work_minutes: 13 * 720,
                occupancy: 0,
                new_patients: 0,
                referrals: Vec::new(),
                assessments_done: 0,
                intake_to_feedback_days: None,
                assessment_to_feedback_days: None,
                waited_days: None,
                waiting_now: 0,
                waiting_now_days: None,
                charged: 0,
                received: 0,
                by_method: Vec::new(),
                debt: vec![
                    bucket("עד 30 יום", 0),
                    bucket("31–60 יום", 31),
                    bucket("61–90 יום", 61),
                    bucket("מעל 90 יום", 91),
                ],
                months: vec![IncomeMonth {
                    month: "2026-11".to_owned(),
                    held: 0,
                    cancelled: 0,
                    no_show: 0,
                    charged: 0,
                    received: 0,
                }],
                before: InsightsBefore {
                    from_month: "2026-10".to_owned(),
                    to_month: "2026-10".to_owned(),
                    held: 0,
                    received: 0,
                    new_patients: 0,
                },
            }
        );
    }

    #[test]
    fn rates_round_to_the_nearest_and_medians_take_the_middle() {
        assert_eq!(per_thousand(0_u32, 0), 0);
        assert_eq!(per_thousand(5_u32, 0), 0);
        assert_eq!(per_thousand(1_u32, 16), 63); // 62.5: a half goes up
        assert_eq!(per_thousand(1_u32, 3), 333);
        assert_eq!(per_thousand(2_u32, 3), 667);
        assert_eq!(per_thousand(3_u32, 3), 1_000);
        assert_eq!(per_thousand(u32::MAX, 1), u16::MAX);
        assert_eq!(median(Vec::new()), None);
        assert_eq!(median(vec![7]), Some(7));
        assert_eq!(median(vec![30, 10, 20]), Some(20));
        assert_eq!(median(vec![15, 32]), Some(23)); // 23.5, rounded down
        assert_eq!(median(vec![4, 1, 3, 2]), Some(2));
        assert_eq!(median(vec![u32::MAX, u32::MAX]), Some(u32::MAX));
    }

    #[test]
    fn counts_patient_visits_and_their_rates() {
        let long = |visit: Visit| Visit {
            minutes: 90,
            ..visit
        };
        let clinic = Clinic {
            patients: vec![card("p1", "2026-10-01")],
            visits: vec![
                held("v1", "p1", VisitKind::Intake, "2026-11-01"), // a Sunday
                long(held("v2", "p1", VisitKind::Assessment, "2026-11-02")),
                long(held("v3", "p1", VisitKind::Assessment, "2026-11-09")),
                held("v4", "p1", VisitKind::Feedback, "2026-11-16"),
                seen(
                    "v5",
                    "p1",
                    VisitKind::Assessment,
                    "2026-11-03",
                    VisitStatus::Cancelled,
                ),
                seen(
                    "v6",
                    "p1",
                    VisitKind::Therapy,
                    "2026-11-10",
                    VisitStatus::Cancelled,
                ),
                seen(
                    "v7",
                    "p1",
                    VisitKind::Therapy,
                    "2026-11-05",
                    VisitStatus::NoShow,
                ),
                seen(
                    "v8",
                    "p1",
                    VisitKind::Assessment,
                    "2026-11-24",
                    VisitStatus::Planned,
                ),
                // Time she kept for herself never counts, even one that says it has a patient,
                // and neither does a visit with no patient.
                Visit {
                    patient_id: None,
                    ..held("b1", "p1", VisitKind::Block, "2026-11-04")
                },
                held("b2", "p1", VisitKind::Block, "2026-11-04"),
                Visit {
                    patient_id: None,
                    ..held("x1", "p1", VisitKind::Other, "2026-11-04")
                },
                Visit {
                    patient_id: Some(" ".to_owned()),
                    ..held("x2", "p1", VisitKind::Other, "2026-11-04")
                },
                // Outside the stretch, or with no real date.
                held("o1", "p1", VisitKind::Assessment, "2026-10-31"),
                held("o2", "p1", VisitKind::Assessment, "2026-12-01"),
                held("o3", "p1", VisitKind::Assessment, "2026-11-31"),
            ],
            ..Clinic::default()
        };
        let picture = clinic.picture("2026-11", "2026-11", "2026-11-30");
        assert_eq!(
            (picture.held, picture.cancelled, picture.no_show),
            (4, 2, 1)
        );
        // 2 and 1 of 7, per thousand: 285.7 and 142.9.
        assert_eq!((picture.cancel_rate, picture.no_show_rate), (286, 143));
        let week: Vec<(u8, u32, u32, u32)> = picture
            .by_weekday
            .iter()
            .map(|d| (d.weekday, d.held, d.cancelled, d.no_show))
            .collect();
        assert_eq!(
            week,
            [
                (0, 1, 0, 0),
                (1, 3, 0, 0),
                (2, 0, 2, 0),
                (3, 0, 0, 0),
                (4, 0, 0, 1),
                (5, 0, 0, 0),
                (6, 0, 0, 0),
            ]
        );
        assert_eq!(
            picture.by_kind,
            kinds(&[
                (VisitKind::Assessment, 2),
                (VisitKind::Intake, 1),
                (VisitKind::Feedback, 1),
            ])
        );
        assert_eq!(picture.held_minutes, 60 + 90 + 90 + 60);
        assert_eq!(picture.assessments_done, 1);
        assert_eq!(picture.months.first().map(|m| m.held), Some(4));
    }

    #[test]
    fn erased_cards_count_as_in_the_income_year() {
        let mut clinic = Clinic {
            patients: vec![card("p1", "2026-11-03")],
            visits: vec![Visit {
                fee: 45_000,
                ..held("v1", "p1", VisitKind::Assessment, "2026-11-02")
            }],
            payments: vec![
                paid("m1", "p1", "2026-11-05", 20_000, PayMethod::Bit),
                Payment {
                    voided: true,
                    void_reason: "נרשם פעמיים".to_owned(),
                    ..paid("m2", "p1", "2026-11-06", 5_000, PayMethod::Transfer)
                },
            ],
            charges: vec![
                charge("c1", "p1", "2026-12-01", 15_000),
                Charge {
                    voided: true,
                    void_reason: "טעות".to_owned(),
                    ..charge("c2", "p1", "2026-11-07", 10_000)
                },
            ],
            ..Clinic::default()
        };
        clinic.tally.months = BTreeMap::from([
            (
                "2026-10".to_owned(),
                ErasedMonth {
                    held: kinds(&[(VisitKind::Feedback, 5)]),
                    received: vec![erased_received(PayMethod::Cash, 7_000)],
                    ..ErasedMonth::default()
                },
            ),
            (
                "2026-11".to_owned(),
                ErasedMonth {
                    held: kinds(&[(VisitKind::Assessment, 2), (VisitKind::Intake, 1)]),
                    cancelled: 1,
                    no_show: 2,
                    charged: 90_000,
                    received: vec![
                        erased_received(PayMethod::Cash, 30_000),
                        erased_received(PayMethod::Bit, 10_000),
                        erased_received(PayMethod::Unknown, 1_000),
                    ],
                    missing_receipts: 1,
                },
            ),
            (
                "2027-01".to_owned(),
                ErasedMonth {
                    cancelled: 9,
                    ..ErasedMonth::default()
                },
            ),
        ]);
        clinic.cards.months = BTreeMap::from([
            ("2026-10".to_owned(), 1),
            ("2026-11".to_owned(), 2),
            ("2026-12".to_owned(), 1),
            ("2027-01".to_owned(), 4),
        ]);
        let picture = clinic.picture("2026-11", "2026-12", TODAY);
        assert_eq!(
            (picture.held, picture.cancelled, picture.no_show),
            (4, 1, 2)
        );
        assert_eq!((picture.cancel_rate, picture.no_show_rate), (143, 286));
        // A voided charge and a voided payment count nowhere.
        assert_eq!(picture.charged, 45_000 + 15_000 + 90_000);
        assert_eq!(picture.received, 20_000 + 30_000 + 10_000 + 1_000);
        // A method of a later version comes last, so the methods add up to what was received.
        assert_eq!(
            picture.by_method,
            methods(&[
                (PayMethod::Cash, 30_000),
                (PayMethod::Bit, 30_000),
                (PayMethod::Unknown, 1_000),
            ])
        );
        assert_eq!(
            picture.by_kind,
            kinds(&[(VisitKind::Assessment, 3), (VisitKind::Intake, 1)])
        );
        assert_eq!(picture.new_patients, 1 + 2 + 1);
        // The days of the week and the minutes know only the cards that still exist.
        assert_eq!(picture.by_weekday.iter().map(|d| d.held).sum::<u32>(), 1);
        assert_eq!(picture.held_minutes, 60);
        assert_eq!(picture.months, clinic.income(2026)[10..12]);
        assert_eq!(
            (
                picture.before.held,
                picture.before.received,
                picture.before.new_patients
            ),
            (5, 7_000, 1)
        );
    }

    #[test]
    fn months_cross_the_new_year_as_the_income_years_count_them() {
        let mut clinic = Clinic {
            patients: vec![card("p1", "2025-11-20")],
            visits: vec![
                Visit {
                    fee: 30_000,
                    ..held("v1", "p1", VisitKind::Assessment, "2025-12-14")
                },
                Visit {
                    fee: 30_000,
                    charge_missed: true,
                    ..seen(
                        "v2",
                        "p1",
                        VisitKind::Assessment,
                        "2026-01-11",
                        VisitStatus::Cancelled,
                    )
                },
                seen(
                    "v3",
                    "p1",
                    VisitKind::Therapy,
                    "2026-02-03",
                    VisitStatus::NoShow,
                ),
            ],
            payments: vec![paid("m1", "p1", "2026-01-12", 60_000, PayMethod::Cash)],
            ..Clinic::default()
        };
        clinic.tally.months = BTreeMap::from([(
            "2025-11".to_owned(),
            ErasedMonth {
                cancelled: 1,
                received: vec![erased_received(PayMethod::Check, 5_000)],
                ..ErasedMonth::default()
            },
        )]);
        let picture = clinic.picture("2025-11", "2026-02", TODAY);
        let names: Vec<&str> = picture.months.iter().map(|m| m.month.as_str()).collect();
        assert_eq!(names, ["2025-11", "2025-12", "2026-01", "2026-02"]);
        let expected: Vec<IncomeMonth> = clinic.income(2025)[10..]
            .iter()
            .chain(&clinic.income(2026)[..2])
            .cloned()
            .collect();
        assert_eq!(picture.months, expected);
        assert_eq!(
            (picture.held, picture.cancelled, picture.no_show),
            (1, 2, 1)
        );
        assert_eq!((picture.charged, picture.received), (60_000, 65_000));
        assert_eq!(
            picture.by_method,
            methods(&[(PayMethod::Cash, 60_000), (PayMethod::Check, 5_000)])
        );
    }

    #[test]
    fn new_patients_and_where_they_came_from() {
        let mut patients = vec![
            // One source written again with other spaces and case, by a card opened later:
            // one row, as the card opened first wrote it.
            referred("a2", "2026-09-10", "  dr   LEVI "),
            referred("a1", "2026-09-02", "Dr Levi"),
            referred("b1", "2026-09-03", "גננת"),
            referred("b2", "2026-10-03", "גננת"),
            referred("b3", "2026-11-03", " גננת"),
            referred("c1", "2026-09-04", ""),
            referred("c2", "2026-10-04", "   "),
            // Opened before the stretch: not new in it.
            referred("z1", "2026-08-31", "גננת"),
        ];
        for (i, source) in [
            "שפ\"ח",
            "רופאת ילדים",
            "קופת חולים",
            "פייסבוק",
            "חברה",
            "בית ספר",
            "אחות",
        ]
        .iter()
        .enumerate()
        {
            patients.push(referred(&format!("d{i}"), "2026-10-20", source));
        }
        let clinic = Clinic {
            patients,
            cards: ErasedCards {
                months: BTreeMap::from([("2026-08".to_owned(), 5), ("2026-10".to_owned(), 3)]),
            },
            ..Clinic::default()
        };
        let picture = clinic.picture("2026-09", "2026-11", TODAY);
        // Fourteen cards that still exist, and three erased ones (which have no source).
        assert_eq!(picture.new_patients, 14 + 3);
        let rows: Vec<(&str, u32)> = picture
            .referrals
            .iter()
            .map(|r| (r.source.as_str(), r.count))
            .collect();
        assert_eq!(
            rows,
            [
                ("גננת", 3),
                ("Dr Levi", 2),
                ("לא צוין", 2),
                ("אחות", 1),
                ("בית ספר", 1),
                ("חברה", 1),
                ("פייסבוק", 1),
                ("קופת חולים", 1),
                ("אחרים", 2),
            ]
        );
    }

    #[test]
    fn eight_sources_need_no_row_of_others() {
        let sources = ["", "לא צוין", "א", "ב", "ג", "ד", "ה", "ו", "ז"];
        let mut clinic = Clinic {
            patients: sources
                .iter()
                .enumerate()
                .map(|(i, source)| referred(&format!("p{i}"), "2026-11-01", source))
                .collect(),
            ..Clinic::default()
        };
        let rows = |clinic: &Clinic| -> Vec<(String, u32)> {
            clinic
                .picture("2026-11", "2026-11", TODAY)
                .referrals
                .into_iter()
                .map(|r| (r.source, r.count))
                .collect()
        };
        // A blank source and one written as "not given" are one row.
        let mut expected: Vec<(String, u32)> = vec![("לא צוין".to_owned(), 2)];
        expected.extend(sources.iter().skip(2).map(|s| ((*s).to_owned(), 1)));
        assert_eq!(rows(&clinic), expected);

        clinic.patients.push(referred("p9", "2026-11-02", "ח"));
        expected.push(("אחרים".to_owned(), 1));
        assert_eq!(rows(&clinic), expected);
    }

    #[test]
    fn times_from_intake_and_testing_to_the_feedback() {
        let clinic = Clinic {
            visits: vec![
                // An intake, two testing sessions and the feedback; a new intake after it.
                held("a1", "pa", VisitKind::Intake, "2026-09-01"),
                held("a2", "pa", VisitKind::Assessment, "2026-09-10"),
                held("a3", "pa", VisitKind::Assessment, "2026-09-20"),
                held("a4", "pa", VisitKind::Feedback, "2026-10-05"),
                held("a5", "pa", VisitKind::Intake, "2026-11-12"),
                // No intake, and two feedback meetings: the later one counts.
                held("b1", "pb", VisitKind::Assessment, "2026-10-01"),
                held("b2", "pb", VisitKind::Assessment, "2026-10-08"),
                seen(
                    "b3",
                    "pb",
                    VisitKind::Assessment,
                    "2026-10-29",
                    VisitStatus::Cancelled,
                ),
                held("b4", "pb", VisitKind::Feedback, "2026-11-02"),
                held("b5", "pb", VisitKind::Feedback, "2026-11-09"),
                // An observation and a feedback, no testing session.
                held("c1", "pc", VisitKind::Observation, "2026-10-20"),
                held("c2", "pc", VisitKind::Feedback, "2026-11-03"),
                // A feedback still planned, and one held before the stretch.
                held("d1", "pd", VisitKind::Intake, "2026-10-01"),
                seen(
                    "d2",
                    "pd",
                    VisitKind::Feedback,
                    "2026-11-20",
                    VisitStatus::Planned,
                ),
                held("e1", "pe", VisitKind::Intake, "2026-08-01"),
                held("e2", "pe", VisitKind::Feedback, "2026-09-15"),
            ],
            ..Clinic::default()
        };
        let picture = clinic.picture("2026-10", "2026-11", TODAY);
        assert_eq!(picture.assessments_done, 4);
        // 34 days, 39 (from the first visit, with no intake) and 14 (from the observation).
        assert_eq!(picture.intake_to_feedback_days, Some(34));
        // 15 days and 32; the patient with no testing session is left out.
        assert_eq!(picture.assessment_to_feedback_days, Some(23));

        let quiet = clinic.picture("2026-12", "2026-12", TODAY);
        assert_eq!(
            (
                quiet.assessments_done,
                quiet.intake_to_feedback_days,
                quiet.assessment_to_feedback_days
            ),
            (0, None, None)
        );
    }

    #[test]
    fn how_long_families_wait() {
        let active = PatientStatus::Active;
        let waiting = PatientStatus::Waiting;
        let clinic = Clinic {
            patients: vec![
                listed("w1", Some("2026-09-01"), active),
                listed("w2", Some("2026-10-01"), active),
                listed("w3", Some("2026-06-01"), active),
                listed("w4", None, active),
                listed("w5", Some("2026-11-10"), active),
                listed("w6", Some("2026-10-05"), active),
                listed("s1", Some("2026-11-01"), waiting),
                listed("s2", Some("2026-10-18"), waiting),
                Patient {
                    created_on: "2026-11-07".to_owned(),
                    ..listed("s3", None, waiting)
                },
            ],
            visits: vec![
                held("v1", "w1", VisitKind::Intake, "2026-10-11"),
                held("v2", "w1", VisitKind::Assessment, "2026-11-01"),
                held("v3", "w2", VisitKind::Intake, "2026-11-05"),
                // First seen before the stretch.
                held("v4", "w3", VisitKind::Intake, "2026-08-01"),
                held("v5", "w3", VisitKind::Assessment, "2026-10-15"),
                // Never on the list.
                held("v6", "w4", VisitKind::Intake, "2026-10-20"),
                // Seen before the day written as the day they joined: left out.
                held("v7", "w5", VisitKind::Intake, "2026-11-01"),
                // Nothing held yet.
                seen(
                    "v8",
                    "w6",
                    VisitKind::Intake,
                    "2026-10-12",
                    VisitStatus::Cancelled,
                ),
                seen(
                    "v9",
                    "w6",
                    VisitKind::Intake,
                    "2026-11-20",
                    VisitStatus::Planned,
                ),
            ],
            ..Clinic::default()
        };
        let picture = clinic.picture("2026-10", "2026-11", TODAY);
        // 40 days and 35: 37.5, rounded down.
        assert_eq!(picture.waited_days, Some(37));
        // 16 days, 30, and 10 since the card of the one with no day was opened.
        assert_eq!(
            (picture.waiting_now, picture.waiting_now_days),
            (3, Some(16))
        );
    }

    #[test]
    fn todays_debt_by_its_age() {
        let owing =
            |id: &str, age: i64, amount: Agorot| charge(&format!("c-{id}"), id, &ago(age), amount);
        let ids = [
            "d0", "d30", "part", "future", "d31", "dv", "d60", "d61", "d90", "d91", "ahead",
            "settled", "voided",
        ];
        let clinic = Clinic {
            patients: ids.iter().map(|id| card(id, "2026-01-05")).collect(),
            charges: vec![
                owing("d0", 0, 1_000),
                owing("d30", 30, 2_000),
                // Paying the oldest first, what is left uncovered is 10 days old.
                charge("c-part-1", "part", &ago(100), 100),
                charge("c-part-2", "part", &ago(10), 200),
                // Charged ahead of time: no age yet.
                owing("future", -10, 500),
                owing("d31", 31, 4_000),
                owing("d60", 60, 8_000),
                owing("d61", 61, 16_000),
                owing("d90", 90, 32_000),
                owing("d91", 91, 64_000),
                owing("ahead", 200, 1_000),
                owing("settled", 200, 1_000),
                Charge {
                    voided: true,
                    void_reason: "טעות".to_owned(),
                    ..owing("voided", 200, 9_000)
                },
                // A card that is not among the clinic's records.
                owing("gone", 200, 70_000),
            ],
            visits: vec![Visit {
                fee: 3_000,
                ..held("v1", "dv", VisitKind::Assessment, &ago(45))
            }],
            payments: vec![
                paid("m1", "part", &ago(5), 150, PayMethod::Cash),
                paid("m2", "ahead", &ago(5), 5_000, PayMethod::Cash),
                paid("m3", "settled", &ago(5), 1_000, PayMethod::Cash),
            ],
            ..Clinic::default()
        };
        let picture = clinic.picture("2026-11", "2026-11", TODAY);
        let rows: Vec<(&str, u32, u32, Agorot)> = picture
            .debt
            .iter()
            .map(|b| (b.label.as_str(), b.from_days, b.patients, b.amount))
            .collect();
        assert_eq!(
            rows,
            [
                ("עד 30 יום", 0, 4, 1_000 + 2_000 + 150 + 500),
                ("31–60 יום", 31, 3, 4_000 + 3_000 + 8_000),
                ("61–90 יום", 61, 2, 16_000 + 32_000),
                ("מעל 90 יום", 91, 1, 64_000),
            ]
        );
    }

    #[test]
    fn the_stretch_just_before() {
        let mut clinic = Clinic {
            patients: vec![
                card("p1", "2025-12-01"),
                card("p2", "2026-02-10"),
                card("p3", "2025-09-30"),
            ],
            visits: vec![
                held("v1", "p1", VisitKind::Intake, "2025-10-05"),
                held("v2", "p1", VisitKind::Assessment, "2025-12-20"),
                held("v3", "p1", VisitKind::Assessment, "2025-09-30"),
                seen(
                    "v4",
                    "p1",
                    VisitKind::Assessment,
                    "2025-11-11",
                    VisitStatus::Cancelled,
                ),
                held("v5", "p2", VisitKind::Intake, "2026-02-11"),
            ],
            payments: vec![
                paid("m1", "p1", "2025-11-01", 10_000, PayMethod::Transfer),
                Payment {
                    voided: true,
                    void_reason: "טעות".to_owned(),
                    ..paid("m2", "p1", "2025-11-02", 4_000, PayMethod::Transfer)
                },
                paid("m3", "p2", "2026-02-12", 9_000, PayMethod::Cash),
            ],
            ..Clinic::default()
        };
        clinic.tally.months = BTreeMap::from([(
            "2025-12".to_owned(),
            ErasedMonth {
                held: kinds(&[(VisitKind::Other, 1)]),
                received: vec![erased_received(PayMethod::Cash, 500)],
                ..ErasedMonth::default()
            },
        )]);
        clinic.cards.months =
            BTreeMap::from([("2025-10".to_owned(), 2), ("2026-01".to_owned(), 1)]);
        let picture = clinic.picture("2026-01", "2026-03", TODAY);
        assert_eq!(
            picture.before,
            InsightsBefore {
                from_month: "2025-10".to_owned(),
                to_month: "2025-12".to_owned(),
                held: 3,
                received: 10_500,
                new_patients: 3,
            }
        );
        assert_eq!(
            (picture.held, picture.received, picture.new_patients),
            (1, 9_000, 2)
        );
    }

    #[test]
    fn occupancy_of_her_working_hours() {
        // January 2026 has no holiday the clinic closes for; the 1st is a Thursday.
        let long = |id: &str, date: &str| Visit {
            minutes: 540,
            ..held(id, "p1", VisitKind::Therapy, date)
        };
        let clinic = Clinic {
            visits: vec![
                long("v1", "2026-01-04"),
                long("v2", "2026-01-05"),
                Visit {
                    minutes: 600,
                    ..seen(
                        "v3",
                        "p1",
                        VisitKind::Therapy,
                        "2026-01-06",
                        VisitStatus::Cancelled,
                    )
                },
            ],
            ..Clinic::default()
        };
        // Up to the 10th: the 1st, and Sunday to Thursday from the 4th: six days of 12 hours.
        let early = clinic.picture("2026-01", "2026-01", "2026-01-10");
        assert_eq!(
            (early.held_minutes, early.work_minutes, early.occupancy),
            (1_080, 6 * 720, 250)
        );
        // After the month: all its 21 working days (71.4 per thousand).
        let late = clinic.picture("2026-01", "2026-01", "2026-03-01");
        assert_eq!((late.work_minutes, late.occupancy), (21 * 720, 71));
        // Before the month began: no working minutes yet.
        let ahead = clinic.picture("2026-01", "2026-01", "2025-12-31");
        assert_eq!(
            (ahead.held_minutes, ahead.work_minutes, ahead.occupancy),
            (1_080, 0, 0)
        );
        // Her own days and hours: Fridays, 8:00 to 8:05. Far more was held than that.
        let settings = ClinicSettings {
            work_days: vec![5],
            day_start: 480,
            day_end: 485,
            ..ClinicSettings::default()
        };
        let tight = clinic
            .ask(&settings, "2026-01", "2026-01", "2026-01-02")
            .unwrap();
        assert_eq!((tight.work_minutes, tight.occupancy), (5, u16::MAX));
        // September 2026: Rosh Hashana's Sunday and Yom Kippur's Monday are closed days. With
        // the holidays off, they are working days, as free times count them (D-196).
        let closed = clinic.picture("2026-09", "2026-09", TODAY);
        let open = ClinicSettings {
            holidays: false,
            ..ClinicSettings::default()
        };
        let open = clinic.ask(&open, "2026-09", "2026-09", TODAY).unwrap();
        assert_eq!(
            (closed.work_minutes, open.work_minutes),
            (20 * 720, 22 * 720)
        );
    }

    #[test]
    fn finished_assessments_count_the_erased_cards_too() {
        let mut clinic = Clinic {
            patients: vec![card("p1", "2026-01-01")],
            visits: vec![held("v1", "p1", VisitKind::Feedback, "2026-03-10")],
            ..Clinic::default()
        };
        clinic.tally.months = BTreeMap::from([(
            "2026-03".to_owned(),
            ErasedMonth {
                held: kinds(&[(VisitKind::Feedback, 1), (VisitKind::Intake, 2)]),
                ..ErasedMonth::default()
            },
        )]);
        let picture = clinic.picture("2026-03", "2026-03", TODAY);
        let feedbacks = picture
            .by_kind
            .iter()
            .find(|k| k.kind == VisitKind::Feedback)
            .map(|k| k.count);
        assert_eq!((feedbacks, picture.assessments_done), (Some(2), 2));
        // The breakdowns by day know only the cards that still exist.
        assert_eq!(picture.by_weekday.iter().map(|d| d.held).sum::<u32>(), 1);
    }

    #[test]
    fn a_sum_that_does_not_fit_is_refused() {
        let big = || ErasedMonth {
            received: vec![erased_received(PayMethod::Cash, 2_000_000_000)],
            ..ErasedMonth::default()
        };
        let clinic = Clinic {
            tally: ErasedTally {
                months: BTreeMap::from([
                    ("2025-12".to_owned(), big()),
                    ("2026-01".to_owned(), big()),
                ]),
            },
            ..Clinic::default()
        };
        // Each month, and each year, fits; the two months together do not.
        assert_eq!(
            clinic.picture("2025-12", "2025-12", TODAY).received,
            2_000_000_000
        );
        let settings = ClinicSettings::default();
        refusal(clinic.ask(&settings, "2025-12", "2026-01", TODAY));
        // Nor as the stretch before.
        refusal(clinic.ask(&settings, "2026-02", "2026-03", TODAY));
    }
}
