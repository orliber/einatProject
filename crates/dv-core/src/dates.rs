//! The clock. The calendar arithmetic is in `dv_domain::dates` (A5).

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use dv_domain::dates::{add_years, date_of, iso, parse_iso};

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

pub(crate) fn today() -> (i32, u32, u32) {
    date_of(unix_now())
}
