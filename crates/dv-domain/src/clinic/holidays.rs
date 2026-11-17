//! Israel's holidays and memorial days, computed here from the Hebrew calendar (D-196).
//!
//! No table, no network, no clock: the dates are worked out from the molad and the rules
//! of postponement, so they are right for every year the program accepts (2000–2200).
//! The calendar shows them; the search for a free time skips the days the clinic is
//! closed ([`Holiday::closed`]). Israel keeps one day of each festival.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::dates::{civil_from_days, days_from_civil, iso, parse_iso};

/// One holiday or memorial day on one date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Holiday {
    /// ISO `YYYY-MM-DD`.
    pub date: String,
    /// Its Hebrew name as the calendar shows it, e.g. `ראש השנה`, `ערב פסח`, `חנוכה (נר 3)`.
    pub name: String,
    /// A day the clinic does not work: a festival day (יום טוב), Yom Kippur, Yom Ha'atzmaut.
    pub closed: bool,
}

/// Every holiday from `from` to `to` (ISO dates, both included), by date. A date may carry
/// more than one. Dates that do not parse, or a stretch in the wrong order, give nothing.
#[must_use]
pub fn holidays_between(from: &str, to: &str) -> Vec<Holiday> {
    let (Some(from), Some(to)) = (parse_iso(from), parse_iso(to)) else {
        return Vec::new();
    };
    let (first, last) = (days_from_civil(from), days_from_civil(to));
    if first > last {
        return Vec::new();
    }
    // The Hebrew year Y + 3761 begins in the autumn of Y. Every year under way between `from`
    // and `to`, with one to spare at each end.
    let years = i64::from(from.0) + 3759..=i64::from(to.0) + 3761;
    let mut found: Vec<Day> = years
        .flat_map(year_holidays)
        .filter(|day| (first..=last).contains(&day.0))
        .collect();
    // Stable: two on one date keep the order of the year.
    found.sort_by_key(|day| day.0);
    found
        .into_iter()
        .map(|(day, name, closed)| Holiday {
            date: iso(civil_from_days(day)),
            name,
            closed,
        })
        .collect()
}

/// Whether the clinic is closed on this date for a holiday ([`Holiday::closed`]).
#[must_use]
pub fn closed_day(date: &str) -> bool {
    holidays_between(date, date).iter().any(|h| h.closed)
}

// ------------------------------------------------------------------ the calendar

/// A holiday before it gets its ISO date: (days from 1970-01-01, name, closed).
type Day = (i64, String, bool);

/// 1 Tishrei of the year 1 (Monday, 7 October 3761 BCE, Julian), in days from 1970-01-01.
const EPOCH: i64 = -2_092_590;
/// Parts (1/1080 of an hour) in a day.
const DAY_PARTS: i64 = 25_920;
/// A mean lunar month is 29 days and this many parts (12h 793p).
const MONTH_PARTS: i64 = 13_753;
/// The molad of Tishrei of the year 1 (BaHaRaD: Monday, 5h 204p after nightfall), plus six
/// hours, so that a molad at noon or later counts on the next day (molad zaken).
const FIRST_MOLAD: i64 = 12_084;

/// Days from 1 Tishrei of the year 1 to Rosh Hashana of `year`, after the two postponements
/// that look at this year alone: molad zaken, then lo ADU Rosh.
fn elapsed_days(year: i64) -> i64 {
    // Lunar months before this year: 235 in each 19-year cycle.
    let months = (235 * year - 234).div_euclid(19);
    let days = 29 * months + (FIRST_MOLAD + MONTH_PARTS * months).div_euclid(DAY_PARTS);
    // Day 0 is a Monday: Wednesday, Friday and Sunday are 2, 4 and 6.
    if matches!(days.rem_euclid(7), 2 | 4 | 6) {
        days + 1
    } else {
        days
    }
}

/// 1 Tishrei of `year`, in days from 1970-01-01. The other two postponements show as a year
/// of a length that cannot be: a common year of 356 days (GaTaRaD) starts two days later,
/// and the year after a leap year, which would make that one 382 days (BeTUTaKPaT), one day.
fn new_year(year: i64) -> i64 {
    let this = elapsed_days(year);
    let correction = if elapsed_days(year + 1) - this == 356 {
        2
    } else if this - elapsed_days(year - 1) == 382 {
        1
    } else {
        0
    };
    EPOCH + this + correction
}

/// Sunday is 0, Saturday 6.
fn weekday(day: i64) -> i64 {
    // 1970-01-01 was a Thursday.
    (day + 4).rem_euclid(7)
}

const SUNDAY: i64 = 0;
const MONDAY: i64 = 1;
const FRIDAY: i64 = 5;
const SATURDAY: i64 = 6;

/// The months in the order of the year. `Adar` is the one with Purim: Adar II in a leap year,
/// when `AdarI` comes before it; in a common year `AdarI` has no days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Month {
    Tishrei,
    Heshvan,
    Kislev,
    Tevet,
    Shevat,
    AdarI,
    Adar,
    Nisan,
    Iyar,
    Sivan,
    Tammuz,
    Av,
    Elul,
}

const MONTHS: [Month; 13] = [
    Month::Tishrei,
    Month::Heshvan,
    Month::Kislev,
    Month::Tevet,
    Month::Shevat,
    Month::AdarI,
    Month::Adar,
    Month::Nisan,
    Month::Iyar,
    Month::Sivan,
    Month::Tammuz,
    Month::Av,
    Month::Elul,
];

/// One Hebrew year: its 1 Tishrei (days from 1970-01-01) and its length in days.
#[derive(Debug, Clone, Copy)]
struct Year {
    start: i64,
    length: i64,
}

impl Year {
    fn of(year: i64) -> Self {
        let start = new_year(year);
        Self {
            start,
            length: new_year(year + 1) - start,
        }
    }

    fn leap(self) -> bool {
        self.length > 355
    }

    /// A complete year (355/385) lengthens Heshvan, a deficient one (353/383) shortens Kislev.
    fn month_length(self, month: Month) -> i64 {
        match month {
            Month::Heshvan if self.length % 10 == 5 => 30,
            Month::Kislev if self.length % 10 == 3 => 29,
            Month::AdarI if self.leap() => 30,
            Month::AdarI => 0,
            Month::Tishrei | Month::Kislev | Month::Shevat | Month::Nisan | Month::Sivan => 30,
            Month::Av => 30,
            Month::Heshvan | Month::Tevet | Month::Adar | Month::Iyar | Month::Tammuz => 29,
            Month::Elul => 29,
        }
    }

    /// Day `day` of `month`, in days from 1970-01-01.
    fn day(self, month: Month, day: i64) -> i64 {
        let before: i64 = MONTHS
            .iter()
            .take_while(|&&m| m != month)
            .map(|&m| self.month_length(m))
            .sum();
        self.start + before + day - 1
    }
}

/// The holidays of one Hebrew year, from Rosh Hashana to the eve of the next, in order.
fn year_holidays(year: i64) -> Vec<Day> {
    use Month::{Adar, Av, Elul, Iyar, Kislev, Nisan, Shevat, Sivan, Tishrei};

    let y = Year::of(year);
    let on = |month, day| y.day(month, day);
    let mut out: Vec<Day> = Vec::new();
    let mut add = |day: i64, name: &str, closed: bool| out.push((day, name.to_owned(), closed));

    add(on(Tishrei, 1), "ראש השנה", true);
    add(on(Tishrei, 2), "ראש השנה", true);
    add(on(Tishrei, 9), "ערב יום כיפור", false);
    add(on(Tishrei, 10), "יום כיפור", true);
    add(on(Tishrei, 14), "ערב סוכות", false);
    add(on(Tishrei, 15), "סוכות", true);
    for day in 16..=20 {
        add(on(Tishrei, day), "חול המועד סוכות", false);
    }
    add(on(Tishrei, 21), "הושענא רבה", false);
    add(on(Tishrei, 22), "שמיני עצרת ושמחת תורה", true);

    // Candle n on the day whose evening lights it, as the eves above are marked on their
    // day (the first on 24 Kislev), then the eighth day; it runs into Tevet.
    let first_candle = on(Kislev, 24);
    for candle in 1..=8 {
        add(
            first_candle + candle - 1,
            &format!("חנוכה (נר {candle})"),
            false,
        );
    }
    add(first_candle + 8, "חנוכה (יום אחרון)", false);
    add(on(Shevat, 15), "ט\"ו בשבט", false);

    // A fast is never on Shabbat: Esther's moves back to Thursday, Av's on to Sunday.
    let esther = on(Adar, 13);
    let esther = if weekday(esther) == SATURDAY {
        esther - 2
    } else {
        esther
    };
    add(esther, "תענית אסתר", false);
    add(on(Adar, 14), "פורים", false);
    add(on(Adar, 15), "שושן פורים (ירושלים)", false);

    add(on(Nisan, 14), "ערב פסח", false);
    add(on(Nisan, 15), "פסח", true);
    for day in 16..=20 {
        add(on(Nisan, day), "חול המועד פסח", false);
    }
    add(on(Nisan, 21), "שביעי של פסח", true);

    // Kept off the day before and after Shabbat.
    let shoah = on(Nisan, 27);
    let shoah = match weekday(shoah) {
        FRIDAY => shoah - 1,
        SUNDAY => shoah + 1,
        _ => shoah,
    };
    add(shoah, "יום השואה", false);
    // Neither day may touch Shabbat: Yom Ha'atzmaut not on Friday or Saturday (back to
    // Thursday), Yom HaZikaron not on Sunday (both a day on).
    let independence = on(Iyar, 5);
    let independence = match weekday(independence) {
        FRIDAY => independence - 1,
        SATURDAY => independence - 2,
        MONDAY => independence + 1,
        _ => independence,
    };
    add(independence - 1, "יום הזיכרון", false);
    add(independence, "יום העצמאות", true);
    add(on(Iyar, 18), "ל\"ג בעומר", false);
    add(on(Iyar, 28), "יום ירושלים", false);

    add(on(Sivan, 5), "ערב שבועות", false);
    add(on(Sivan, 6), "שבועות", true);

    let av = on(Av, 9);
    let av = if weekday(av) == SATURDAY { av + 1 } else { av };
    add(av, "תשעה באב", false);

    add(on(Elul, 29), "ערב ראש השנה", false);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEDNESDAY: i64 = 3;

    /// The names on one date.
    fn names_on(date: &str) -> Vec<String> {
        holidays_between(date, date)
            .into_iter()
            .map(|h| h.name)
            .collect()
    }

    fn has(date: &str, name: &str) {
        let names = names_on(date);
        assert!(
            names.iter().any(|n| n == name),
            "{date}: {names:?}, not {name}"
        );
    }

    #[test]
    fn holidays_fall_on_their_known_dates() {
        // 1 Tishrei: Rosh Hashana, after the eve.
        for date in [
            "2023-09-16",
            "2024-10-03",
            "2025-09-23",
            "2026-09-12",
            "2027-10-02",
        ] {
            has(date, "ראש השנה");
            let (y, m, d) = parse_iso(date).unwrap();
            let eve = iso(civil_from_days(days_from_civil((y, m, d)) - 1));
            assert_eq!(names_on(&eve), ["ערב ראש השנה"], "{eve}");
        }
        has("2025-10-02", "יום כיפור");
        has("2026-09-21", "יום כיפור");
        has("2026-09-26", "סוכות");
        has("2026-10-02", "הושענא רבה");
        has("2026-10-03", "שמיני עצרת ושמחת תורה");
        // 15 Nisan.
        for date in ["2024-04-23", "2025-04-13", "2026-04-02", "2027-04-22"] {
            has(date, "פסח");
        }
        has("2026-04-08", "שביעי של פסח");
        has("2025-06-02", "שבועות");
        has("2026-05-22", "שבועות");
        // The first candle is lit on the evening of 2025-12-14, the eighth on 2025-12-21.
        has("2025-12-14", "חנוכה (נר 1)");
        has("2025-12-15", "חנוכה (נר 2)");
        has("2025-12-21", "חנוכה (נר 8)");
        has("2025-12-22", "חנוכה (יום אחרון)");
        has("2026-12-04", "חנוכה (נר 1)");
        has("2026-12-12", "חנוכה (יום אחרון)");
        // 2024 is a leap year: Purim in Adar II, and the fast moved off Shabbat to Thursday.
        has("2024-03-24", "פורים");
        has("2024-03-21", "תענית אסתר");
        has("2025-03-14", "פורים");
        has("2026-03-03", "פורים");
        has("2026-03-04", "שושן פורים (ירושלים)");
        // Yom HaShoah off Friday (2025) and Sunday (2024).
        has("2025-04-24", "יום השואה");
        has("2024-05-06", "יום השואה");
        has("2026-04-14", "יום השואה");
        // Yom Ha'atzmaut moved from Monday (2024) and from Saturday (2025).
        has("2024-05-14", "יום העצמאות");
        has("2024-05-13", "יום הזיכרון");
        has("2025-05-01", "יום העצמאות");
        has("2025-04-30", "יום הזיכרון");
        has("2026-04-22", "יום העצמאות");
        has("2026-04-21", "יום הזיכרון");
        // Postponed from Shabbat (2025).
        has("2025-08-03", "תשעה באב");
        assert!(names_on("2025-08-02").is_empty());
        has("2026-07-23", "תשעה באב");
    }

    #[test]
    fn every_year_of_the_range_keeps_the_calendar_rules() {
        // The Hebrew years under way from 2000-01-01 to 2200-12-31.
        for year in 5760..=5962 {
            let y = Year::of(year);
            assert!(
                [353, 354, 355, 383, 384, 385].contains(&y.length),
                "{year}: {} days",
                y.length
            );
            assert_eq!(y.leap(), (7 * year + 1).rem_euclid(19) < 7, "{year}");
            // Lo ADU Rosh.
            assert!(
                ![SUNDAY, WEDNESDAY, FRIDAY].contains(&weekday(y.start)),
                "{year} begins on day {}",
                weekday(y.start)
            );
            // The months add up to the year.
            assert_eq!(y.day(Month::Elul, 29) + 1, new_year(year + 1), "{year}");
        }
        for year in 2000..=2200 {
            let all = holidays_between(&format!("{year}-01-01"), &format!("{year}-12-31"));
            assert!(all.windows(2).all(|w| w[0].date <= w[1].date), "{year}");
            assert!(all.iter().all(|h| h.date.starts_with(&year.to_string())));
            let closed: Vec<&str> = all
                .iter()
                .filter(|h| h.closed)
                .map(|h| h.name.as_str())
                .collect();
            assert_eq!(
                closed,
                [
                    "פסח",
                    "שביעי של פסח",
                    "יום העצמאות",
                    "שבועות",
                    "ראש השנה",
                    "ראש השנה",
                    "יום כיפור",
                    "סוכות",
                    "שמיני עצרת ושמחת תורה",
                ],
                "{year}"
            );
            // No moved day lands on Shabbat, and no two fall on one date.
            for h in &all {
                let day = days_from_civil(parse_iso(&h.date).unwrap());
                let moved = [
                    "תענית אסתר",
                    "יום השואה",
                    "יום הזיכרון",
                    "יום העצמאות",
                    "תשעה באב",
                ];
                if moved.contains(&h.name.as_str()) {
                    assert_ne!(weekday(day), SATURDAY, "{year}: {}", h.name);
                }
            }
            assert!(all.windows(2).all(|w| w[0].date != w[1].date), "{year}");
        }
    }

    #[test]
    fn a_stretch_may_cross_years() {
        // From the eve of Rosh Hashana 5787 to the end of Hanukkah, which ends in Tevet.
        let all = holidays_between("2026-09-11", "2026-12-12");
        assert_eq!(all.first().map(|h| h.name.as_str()), Some("ערב ראש השנה"));
        assert_eq!(
            all.last().map(|h| h.name.as_str()),
            Some("חנוכה (יום אחרון)")
        );
        assert_eq!(all.iter().filter(|h| h.closed).count(), 5);
        // Hanukkah 5788 runs from December 2027 into January 2028.
        let hanukkah = holidays_between("2027-12-01", "2028-01-31");
        assert_eq!(
            hanukkah
                .iter()
                .filter(|h| h.name.starts_with("חנוכה"))
                .count(),
            9
        );
        // A stretch inside January still finds the Hebrew year that began the autumn before.
        assert_eq!(names_on("2027-12-31"), ["חנוכה (נר 8)"]);
        assert_eq!(names_on("2028-01-01"), ["חנוכה (יום אחרון)"]);
        // The first and last days of the range.
        assert!(!holidays_between("2000-01-01", "2000-12-31").is_empty());
        assert!(!holidays_between("2200-01-01", "2200-12-31").is_empty());
    }

    #[test]
    fn bad_input_gives_nothing() {
        assert!(holidays_between("not a date", "2026-12-31").is_empty());
        assert!(holidays_between("2026-01-01", "2026-13-01").is_empty());
        assert!(holidays_between("2026-12-31", "2026-01-01").is_empty());
        assert!(holidays_between("1999-09-11", "2000-12-31").is_empty());
        assert!(!closed_day("not a date"));
    }

    #[test]
    fn the_clinic_closes_on_festival_days_only() {
        assert!(closed_day("2026-09-12"));
        assert!(closed_day("2026-09-13"));
        // Tzom Gedaliah: not one of these.
        assert!(!closed_day("2026-09-14"));
        // Hol HaMoed and the eves are open.
        assert!(!closed_day("2026-09-27"));
        assert!(!closed_day("2026-09-11"));
        assert!(closed_day("2026-04-22"));
        assert!(!closed_day("2026-04-21"));
    }
}
