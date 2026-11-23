//! What the one-way copy of the calendar to Google Calendar shows (D-202): the rules only.
//!
//! Each visit that is planned or was held becomes one event in a calendar of its own that
//! the app made in her Google account. What an event says is set by the detail level; the
//! first and only level today is the strictest, [`CalendarDetail::TimesOnly`]: its time and
//! one fixed word. No name, no kind, no place, no note: nothing that is not on this page.
//!
//! Pure, like the rest of this crate: the event ids are made by the caller (a keyed hash, so
//! no id of the vault shows at Google), and "today" is given by the caller.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{add_days, day_number, Visit, VisitKind, VisitStatus};

/// The time zone of every event (her clinic is in Israel).
pub const CALENDAR_TIME_ZONE: &str = "Asia/Jerusalem";
/// The name of the calendar the app makes in her account. Fixed: it says nothing about her
/// work or anyone in it.
pub const CALENDAR_NAME: &str = "פגישות";
/// The days copied: from this many days back…
pub const SYNC_DAYS_BACK: i64 = 14;
/// …to this many days ahead.
pub const SYNC_DAYS_AHEAD: i64 = 180;
/// The title of an event with a patient, at [`CalendarDetail::TimesOnly`].
pub const TITLE_VISIT: &str = "פגישה";
/// The title of time she blocked for herself.
pub const TITLE_BLOCK: &str = "לא פנויה";

/// How much an event in Google Calendar says. More levels may come later (a kind of visit,
/// initials), each one hers to turn on; a level this version does not know reads as the
/// strictest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CalendarDetail {
    /// The time, and "פגישה" (or "לא פנויה" for her own blocked time). Nothing else.
    #[default]
    TimesOnly,
    #[serde(other)]
    Unknown,
}

/// One event as it should be in her Google calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedEvent {
    /// The visit it comes from (never sent: the caller makes the event's id from it).
    pub visit_id: String,
    /// Local wall time, `YYYY-MM-DDTHH:MM:00`, in [`CALENDAR_TIME_ZONE`].
    pub start: String,
    pub end: String,
    pub title: &'static str,
}

/// The first and last day copied, around `today` (`YYYY-MM-DD`).
#[must_use]
pub fn sync_window(today: &str) -> Option<(String, String)> {
    Some((
        add_days(today, -SYNC_DAYS_BACK)?,
        add_days(today, SYNC_DAYS_AHEAD)?,
    ))
}

/// `date` + `minutes` from its midnight, as local wall time (a visit may end after midnight).
fn wall_time(date: &str, minutes: u32) -> Option<String> {
    let day = add_days(date, i64::from(minutes / (24 * 60)))?;
    let m = minutes % (24 * 60);
    Some(format!("{day}T{:02}:{:02}:00", m / 60, m % 60))
}

/// The events the visits make, between `from` and `to` (both included), by start time.
/// A visit cancelled or missed is not there (so its event goes); one of a kind or a status
/// this version does not know is left out too (strictest).
#[must_use]
pub fn wanted_events(
    visits: &[Visit],
    from: &str,
    to: &str,
    detail: CalendarDetail,
) -> Vec<WantedEvent> {
    let (Some(first), Some(last)) = (day_number(from), day_number(to)) else {
        return Vec::new();
    };
    let mut out: Vec<WantedEvent> = visits
        .iter()
        .filter(|v| matches!(v.status, VisitStatus::Planned | VisitStatus::Held))
        .filter(|v| v.kind != VisitKind::Unknown && !v.id.is_empty() && v.minutes > 0)
        .filter(|v| day_number(&v.date).is_some_and(|d| (first..=last).contains(&d)))
        .filter_map(|v| {
            let start = u32::from(v.start);
            let title = match detail {
                CalendarDetail::TimesOnly | CalendarDetail::Unknown => {
                    if v.own_time() {
                        TITLE_BLOCK
                    } else {
                        TITLE_VISIT
                    }
                }
            };
            Some(WantedEvent {
                visit_id: v.id.clone(),
                start: wall_time(&v.date, start)?,
                end: wall_time(&v.date, start + u32::from(v.minutes))?,
                title,
            })
        })
        .collect();
    out.sort_by(|a, b| a.start.cmp(&b.start).then(a.visit_id.cmp(&b.visit_id)));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn visit(
        id: &str,
        date: &str,
        start: u16,
        minutes: u16,
        kind: VisitKind,
        status: VisitStatus,
    ) -> Visit {
        Visit {
            id: id.to_owned(),
            patient_id: (kind != VisitKind::Block).then(|| "p1".to_owned()),
            kind,
            title: "שם בדוי בכותרת".to_owned(),
            date: date.to_owned(),
            start,
            minutes,
            place: "רחוב בדוי 1".to_owned(),
            status,
            note: "הערה בדויה".to_owned(),
            ..Visit::default()
        }
    }

    #[test]
    fn only_the_time_and_one_word_go() {
        let visits = [
            visit(
                "a",
                "2026-11-18",
                9 * 60,
                90,
                VisitKind::Assessment,
                VisitStatus::Planned,
            ),
            visit(
                "b",
                "2026-11-18",
                13 * 60,
                60,
                VisitKind::Block,
                VisitStatus::Planned,
            ),
            visit(
                "c",
                "2026-11-17",
                8 * 60,
                50,
                VisitKind::Therapy,
                VisitStatus::Held,
            ),
        ];
        let got = wanted_events(
            &visits,
            "2026-11-01",
            "2026-12-31",
            CalendarDetail::TimesOnly,
        );
        assert_eq!(
            got,
            vec![
                WantedEvent {
                    visit_id: "c".into(),
                    start: "2026-11-17T08:00:00".into(),
                    end: "2026-11-17T08:50:00".into(),
                    title: TITLE_VISIT
                },
                WantedEvent {
                    visit_id: "a".into(),
                    start: "2026-11-18T09:00:00".into(),
                    end: "2026-11-18T10:30:00".into(),
                    title: TITLE_VISIT
                },
                WantedEvent {
                    visit_id: "b".into(),
                    start: "2026-11-18T13:00:00".into(),
                    end: "2026-11-18T14:00:00".into(),
                    title: TITLE_BLOCK
                },
            ]
        );
    }

    #[test]
    fn cancelled_missed_unknown_and_out_of_range_stay_out() {
        let visits = [
            visit(
                "x",
                "2026-11-18",
                600,
                60,
                VisitKind::Therapy,
                VisitStatus::Cancelled,
            ),
            visit(
                "y",
                "2026-11-18",
                600,
                60,
                VisitKind::Therapy,
                VisitStatus::NoShow,
            ),
            visit(
                "z",
                "2026-11-18",
                600,
                60,
                VisitKind::Therapy,
                VisitStatus::Unknown,
            ),
            visit(
                "u",
                "2026-11-18",
                600,
                60,
                VisitKind::Unknown,
                VisitStatus::Planned,
            ),
            visit(
                "o",
                "2027-11-18",
                600,
                60,
                VisitKind::Therapy,
                VisitStatus::Planned,
            ),
            visit(
                "",
                "2026-11-18",
                600,
                60,
                VisitKind::Therapy,
                VisitStatus::Planned,
            ),
            visit(
                "n",
                "2026-11-18",
                600,
                0,
                VisitKind::Therapy,
                VisitStatus::Planned,
            ),
        ];
        assert!(wanted_events(
            &visits,
            "2026-11-01",
            "2026-12-31",
            CalendarDetail::TimesOnly
        )
        .is_empty());
    }

    #[test]
    fn a_visit_past_midnight_ends_the_next_day() {
        let visits = [visit(
            "a",
            "2026-12-31",
            23 * 60 + 30,
            60,
            VisitKind::Other,
            VisitStatus::Planned,
        )];
        let got = wanted_events(&visits, "2026-12-01", "2027-01-31", CalendarDetail::Unknown);
        assert_eq!(got[0].end, "2027-01-01T00:30:00");
        assert_eq!(got[0].title, TITLE_VISIT);
    }

    #[test]
    fn the_window_is_two_weeks_back_and_half_a_year_ahead() {
        assert_eq!(
            sync_window("2026-11-17").unwrap(),
            ("2026-11-03".to_owned(), "2027-05-16".to_owned())
        );
    }
}
