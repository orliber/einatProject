//! Calendar dates without a date library: ISO "YYYY-MM-DD", proleptic Gregorian. The one
//! place for this arithmetic (A5); the clock itself is read in dv-core.

/// A calendar date: (year, month, day).
pub type Ymd = (i32, u32, u32);

/// Seconds in a day.
pub const DAY_SECS: i64 = 86_400;

/// Days since 1970-01-01 → (year, month, day) (H. Hinnant's algorithm).
#[must_use]
pub fn civil_from_days(z: i64) -> Ymd {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (i32::try_from(y).unwrap_or(1970), m, d)
}

/// (year, month, day) → days since 1970-01-01 (the inverse of [`civil_from_days`]).
#[must_use]
pub fn days_from_civil((y, m, d): Ymd) -> i64 {
    let y = i64::from(if m <= 2 { y - 1 } else { y });
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The UTC calendar date of a unix time.
#[must_use]
pub fn date_of(unix: i64) -> Ymd {
    civil_from_days(unix.div_euclid(DAY_SECS))
}

#[must_use]
pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// "2026-09-29" → (2026, 9, 29); `None` unless it is a real date between 2000 and 2200.
#[must_use]
pub fn parse_iso(text: &str) -> Option<Ymd> {
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return None;
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d) = (
        y.parse::<i32>().ok()?,
        m.parse::<u32>().ok()?,
        d.parse::<u32>().ok()?,
    );
    ((2000..=2200).contains(&y) && (1..=days_in_month(y, m)).contains(&d)).then_some((y, m, d))
}

#[must_use]
pub fn iso((y, m, d): Ymd) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// The same day `years` later (29 February → 28 February).
#[must_use]
pub fn add_years((y, m, d): Ymd, years: i32) -> Ymd {
    let y = y + years;
    (y, m, d.min(days_in_month(y, m)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_and_leap_days_hold() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(date_of(1_790_683_200), (2026, 9, 29));
        assert_eq!(parse_iso("2026-09-29"), Some((2026, 9, 29)));
        assert_eq!(parse_iso("2026-02-29"), None);
        assert_eq!(parse_iso("2026-9-29"), None);
        assert_eq!(add_years((2028, 2, 29), 7), (2035, 2, 28));
        assert_eq!(iso((2033, 9, 1)), "2033-09-01");
        for z in [-800_000, -1, 0, 11_016, 20_725, 1_000_000] {
            assert_eq!(days_from_civil(civil_from_days(z)), z);
        }
    }
}
