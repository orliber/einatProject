//! The clock. The calendar arithmetic is in `dv_domain::dates` (A5).

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use dv_domain::dates::{add_years, date_of, iso, parse_iso};
use dv_domain::dates::{days_from_civil, DAY_SECS};

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

pub(crate) fn today() -> (i32, u32, u32) {
    date_of(unix_now())
}

/// The calendar date in Israel at `unix`: the date on her screen, for labels such as a backup
/// file's name (OPS-8). The program keeps no time-zone tables; Israel's rule (since 2013) is
/// UTC+2, and UTC+3 from the Friday before the last Sunday of March, 02:00, to the last
/// Sunday of October, 02:00. The clinic calendar is kept in the same fixed zone (D-202).
pub(crate) fn israel_date_of(unix: i64) -> (i32, u32, u32) {
    let (year, _, _) = date_of(unix);
    // 0 = Sunday (1970-01-01 was a Thursday).
    let last_sunday = |month: u32| {
        let last = days_from_civil((year, month, 31));
        last - (last + 4).rem_euclid(7)
    };
    // 02:00 at UTC+2 on that Friday is 00:00 UTC; 02:00 at UTC+3 on that Sunday is 23:00 UTC
    // the day before.
    let summer_from = (last_sunday(3) - 2) * DAY_SECS;
    let summer_to = last_sunday(10) * DAY_SECS - 3600;
    let offset = if (summer_from..summer_to).contains(&unix) {
        3 * 3600
    } else {
        2 * 3600
    };
    date_of(unix.saturating_add(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: &str, h: i64, m: i64) -> i64 {
        days_from_civil(parse_iso(day).unwrap_or((1970, 1, 1))) * DAY_SECS + h * 3600 + m * 60
    }

    #[test]
    fn the_date_in_israel_moves_with_its_summer_time() {
        // Winter, UTC+2: 22:30 UTC is 00:30 the next day.
        assert_eq!(iso(israel_date_of(at("2026-11-15", 22, 30))), "2026-11-16");
        assert_eq!(iso(israel_date_of(at("2026-11-15", 21, 59))), "2026-11-15");
        // Summer, UTC+3: 21:30 UTC is 00:30 the next day, 20:59 UTC is still the same day.
        assert_eq!(iso(israel_date_of(at("2026-07-01", 21, 30))), "2026-07-02");
        assert_eq!(iso(israel_date_of(at("2026-07-01", 20, 59))), "2026-07-01");
        // 2026: summer time from Friday 27.3 at 02:00 to Sunday 25.10 at 02:00.
        assert_eq!(iso(israel_date_of(at("2026-03-26", 21, 30))), "2026-03-26");
        assert_eq!(iso(israel_date_of(at("2026-03-27", 21, 30))), "2026-03-28");
        assert_eq!(iso(israel_date_of(at("2026-10-24", 21, 30))), "2026-10-25");
        assert_eq!(iso(israel_date_of(at("2026-10-24", 23, 30))), "2026-10-25");
        assert_eq!(iso(israel_date_of(at("2026-10-25", 21, 30))), "2026-10-25");
    }
}
