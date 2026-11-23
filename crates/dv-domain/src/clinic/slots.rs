//! Free times in her calendar (D-197), the assessment pathway (D-198) and the starter
//! follow-ups of a new family (D-199).
//!
//! Pure, like the rest of the rules: the caller gives the visits, the settings and "today".

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{
    add_days, closed_day, day_number, default_fee, kind_default, weekday, ClinicSettings,
    ClinicTask, Patient, Today, Visit, VisitKind, VisitStatus,
};
use crate::DomainError;

/// A free time: a date and the minute of the day it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Slot {
    pub date: String,
    pub start: u16,
}

/// Free times start on the quarter hour.
pub const SLOT_STEP: u16 = 15;
/// How far ahead a free time is looked for, in days.
pub const SLOT_SEARCH_DAYS: i64 = 120;
/// The most free times one search gives.
pub const MAX_SLOTS: u8 = 12;
/// At most this many free times are offered on one day, so the offer spans several days.
pub const SLOTS_PER_DAY: usize = 3;
/// The longest gap kept before and after a visit.
pub const MAX_BUFFER: u16 = 120;
/// The most steps a pathway may have, and the longest gap between two.
pub const MAX_PATHWAY_STEPS: usize = 12;
pub const MAX_GAP_DAYS: u16 = 180;
/// The most starter follow-ups, and the latest one may be due.
pub const MAX_STARTER_TASKS: usize = 20;
pub const MAX_STARTER_DAYS: u16 = 365;

fn refuse<T>(text: impl Into<String>) -> Result<T, DomainError> {
    Err(DomainError::Invalid(text.into()))
}

/// The minutes of one day that a visit takes, with the gap kept around it.
fn busy_span(visit: &Visit, buffer: u16) -> (u32, u32) {
    let start = u32::from(visit.start).saturating_sub(u32::from(buffer));
    let end = u32::from(visit.start)
        .saturating_add(u32::from(visit.minutes))
        .saturating_add(u32::from(buffer));
    (start, end)
}

/// Whether the clinic works on this date: one of her working days, and not a day closed for
/// a holiday (when she has the holidays on).
pub(super) fn working_day(date: &str, settings: &ClinicSettings) -> bool {
    weekday(date).is_some_and(|day| settings.work_days.contains(&day))
        && !(settings.holidays && closed_day(date))
}

/// The free starts of `minutes` on one date, in order. `from_minute`: nothing earlier
/// (today, the time that already passed).
fn free_on(
    date: &str,
    visits: &[&Visit],
    settings: &ClinicSettings,
    minutes: u16,
    from_minute: u16,
) -> Vec<u16> {
    let busy: Vec<(u32, u32)> = visits
        .iter()
        .filter(|v| v.date == date && v.status != VisitStatus::Cancelled)
        .map(|v| busy_span(v, settings.buffer_minutes))
        .collect();
    let first = settings.day_start.max(from_minute);
    // Round up to the step, counted from midnight.
    let first = first.div_ceil(SLOT_STEP).saturating_mul(SLOT_STEP);
    let mut starts = Vec::new();
    let mut start = first;
    while u32::from(start) + u32::from(minutes) <= u32::from(settings.day_end) {
        let end = u32::from(start) + u32::from(minutes);
        if !busy
            .iter()
            .any(|(b0, b1)| u32::from(start) < *b1 && *b0 < end)
        {
            starts.push(start);
        }
        match start.checked_add(SLOT_STEP) {
            Some(next) => start = next,
            None => break,
        }
    }
    starts
}

/// At most [`SLOTS_PER_DAY`] of a day's free starts, none overlapping the one before.
fn spread(starts: &[u16], minutes: u16) -> Vec<u16> {
    let mut picked: Vec<u16> = Vec::new();
    for start in starts {
        if picked.len() == SLOTS_PER_DAY {
            break;
        }
        let after_last = picked
            .last()
            .is_none_or(|last| u32::from(*start) >= u32::from(*last) + u32::from(minutes));
        if after_last {
            picked.push(*start);
        }
    }
    picked
}

fn check_search(minutes: u16, count: u8) -> Result<(), DomainError> {
    if !(super::MIN_MINUTES..=super::MAX_MINUTES).contains(&minutes) {
        return refuse("משך הפגישה צריך להיות בין 5 דקות ל-12 שעות.");
    }
    if count == 0 || count > MAX_SLOTS {
        return refuse(format!("אפשר לבקש עד {MAX_SLOTS} שעות פנויות."));
    }
    Ok(())
}

/// The first free times of `minutes` from `from` (or today, the later) on, at most `count`,
/// within [`SLOT_SEARCH_DAYS`]: on her working days, inside her hours, not on a day closed
/// for a holiday, and clear of every visit that is not cancelled (blocks included) by
/// `buffer_minutes` on each side. Today, only times that have not passed. `skip_visit`: the
/// visit being moved, whose own time is free for it.
pub fn free_slots(
    visits: &[Visit],
    settings: &ClinicSettings,
    today: &Today,
    from: &str,
    minutes: u16,
    count: u8,
    skip_visit: Option<&str>,
) -> Result<Vec<Slot>, DomainError> {
    check_search(minutes, count)?;
    let (Some(from_day), Some(now)) = (day_number(from), day_number(&today.date)) else {
        return refuse("צריך לבחור תאריך תקין.");
    };
    let mine: Vec<&Visit> = visits
        .iter()
        .filter(|v| skip_visit != Some(v.id.as_str()) || v.id.is_empty())
        .collect();
    let first = from_day.max(now);
    let mut slots = Vec::new();
    for offset in 0..SLOT_SEARCH_DAYS {
        let Some(date) = add_days(&today.date, first - now + offset) else {
            break;
        };
        if !working_day(&date, settings) {
            continue;
        }
        let from_minute = if first + offset == now {
            today.minute
        } else {
            0
        };
        for start in spread(
            &free_on(&date, &mine, settings, minutes, from_minute),
            minutes,
        ) {
            slots.push(Slot {
                date: date.clone(),
                start,
            });
            if slots.len() >= usize::from(count) {
                return Ok(slots);
            }
        }
    }
    Ok(slots)
}

/// The first free time of `minutes` on or after `from`, or `None` within the search.
fn first_free(
    visits: &[&Visit],
    settings: &ClinicSettings,
    today: &Today,
    from: &str,
    minutes: u16,
) -> Option<Slot> {
    let from_day = day_number(from)?;
    let now = day_number(&today.date)?;
    let first = from_day.max(now);
    (0..SLOT_SEARCH_DAYS).find_map(|offset| {
        let date = add_days(&today.date, first - now + offset)?;
        if !working_day(&date, settings) {
            return None;
        }
        let from_minute = if first + offset == now {
            today.minute
        } else {
            0
        };
        free_on(&date, visits, settings, minutes, from_minute)
            .first()
            .map(|start| Slot {
                date: date.clone(),
                start: *start,
            })
    })
}

/// The pathway as she set it: the steps in order, with sensible values.
pub fn validate_pathway(settings: &ClinicSettings) -> Result<(), DomainError> {
    if settings.pathway.len() > MAX_PATHWAY_STEPS {
        return refuse(format!(
            "אפשר להגדיר עד {MAX_PATHWAY_STEPS} שלבים במסלול האבחון."
        ));
    }
    for step in &settings.pathway {
        if step.kind.never_patient() || step.kind == VisitKind::Unknown {
            return refuse("כל שלב במסלול האבחון צריך להיות פגישה עם המשפחה או עם הילד.");
        }
        if step.gap_days > MAX_GAP_DAYS {
            return refuse(format!(
                "הרווח בין שני שלבים במסלול יכול להיות עד {MAX_GAP_DAYS} יום."
            ));
        }
    }
    Ok(())
}

/// What a pathway step says when it finds no room.
const NO_ROOM_FOR_PATHWAY: &str = "לא נמצאה שעה פנויה לכל שלבי המסלול בחודשים הקרובים. אפשר \
     להתחיל מתאריך אחר, או לקבוע את הפגישות אחת אחת.";

/// The visits of the assessment pathway for this patient (D-198): each step at the first
/// free time at least its gap after the step before (the first, from `from`), with the
/// kind's length, the patient's fee (or the kind's), and her first place. Nothing is
/// saved. Refused when there is no pathway, or a step finds no free time within
/// [`SLOT_SEARCH_DAYS`].
pub fn propose_pathway(
    visits: &[Visit],
    settings: &ClinicSettings,
    patient: &Patient,
    today: &Today,
    from: &str,
) -> Result<Vec<Visit>, DomainError> {
    validate_pathway(settings)?;
    if settings.pathway.is_empty() {
        return refuse(
            "עוד אין מסלול אבחון. אפשר להגדיר אותו בהגדרות, בחלק \"מסלול ומשפחה חדשה\".",
        );
    }
    if day_number(from).is_none() {
        return refuse("צריך לבחור תאריך תקין להתחלה.");
    }
    let mut proposed: Vec<Visit> = Vec::new();
    let mut earliest = from.to_owned();
    for step in &settings.pathway {
        let minutes = kind_default(settings, step.kind).map_or(60, |d| d.minutes);
        let taken: Vec<&Visit> = visits.iter().chain(proposed.iter()).collect();
        let Some(slot) = first_free(&taken, settings, today, &earliest, minutes) else {
            return refuse(NO_ROOM_FOR_PATHWAY);
        };
        let visit = Visit {
            id: String::new(),
            patient_id: Some(patient.id.clone()),
            kind: step.kind,
            title: String::new(),
            date: slot.date.clone(),
            start: slot.start,
            minutes,
            place: settings.places.first().cloned().unwrap_or_default(),
            status: VisitStatus::Planned,
            status_note: String::new(),
            fee: default_fee(Some(patient), settings, step.kind),
            charge_missed: false,
            series_id: None,
            note: String::new(),
            remember: Vec::new(),
        };
        proposed.push(visit);
        // The next step: its gap after this one, at least the next day; past the last date
        // the clinic keeps, there is no room for it.
        if let Some(next) = settings.pathway.get(proposed.len()) {
            let Some(day) = add_days(&slot.date, i64::from(next.gap_days).max(1)) else {
                return refuse(NO_ROOM_FOR_PATHWAY);
            };
            earliest = day;
        }
    }
    Ok(proposed)
}

/// The starter follow-ups as she set them.
pub fn validate_starter_tasks(settings: &ClinicSettings) -> Result<(), DomainError> {
    if settings.starter_tasks.len() > MAX_STARTER_TASKS {
        return refuse(format!(
            "אפשר להגדיר עד {MAX_STARTER_TASKS} מעקבים בערכת הפתיחה."
        ));
    }
    for task in &settings.starter_tasks {
        if task.title.trim().is_empty() {
            return refuse("לכל מעקב בערכת הפתיחה צריכה להיות כותרת.");
        }
        if task.title.trim().chars().count() > super::SHORT_MAX {
            return refuse("כותרת של מעקב בערכת הפתיחה ארוכה מדי.");
        }
        if task.due_days > MAX_STARTER_DAYS {
            return refuse("מעקב בערכת הפתיחה יכול להיות עד שנה מהיום.");
        }
        if task.kind == super::TaskKind::Unknown {
            return refuse("צריך לבחור סוג לכל מעקב בערכת הפתיחה.");
        }
    }
    Ok(())
}

/// The starter follow-ups to open for this patient today (D-199): one for each in the
/// settings, due `due_days` from today, except one whose title (ignoring case and spaces) is
/// already an open task of the patient. New tasks, not saved.
#[must_use]
pub fn starter_tasks(
    settings: &ClinicSettings,
    patient_id: &str,
    open: &[ClinicTask],
    today: &str,
) -> Vec<ClinicTask> {
    let key = |title: &str| {
        title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let mut have: Vec<String> = open
        .iter()
        .filter(|t| t.patient_id.as_deref() == Some(patient_id) && t.done_on.is_none())
        .map(|t| key(&t.title))
        .collect();
    let mut tasks = Vec::new();
    for starter in &settings.starter_tasks {
        let title = starter
            .title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if title.is_empty() || have.contains(&key(&title)) {
            continue;
        }
        have.push(key(&title));
        tasks.push(ClinicTask {
            id: String::new(),
            patient_id: Some(patient_id.to_owned()),
            kind: starter.kind,
            title,
            due: add_days(today, i64::from(starter.due_days)),
            done_on: None,
            note: String::new(),
        });
    }
    tasks
}
