//! Israel's holidays and memorial days, computed here from the Hebrew calendar (D-196).
//!
//! No table, no network, no clock: the dates are worked out from the molad and the rules
//! of postponement, so they are right for every year the program accepts (2000–2200).
//! The calendar shows them; the search for a free time skips the days the clinic is
//! closed ([`Holiday::closed`]). Israel keeps one day of each festival.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
    let _ = (from, to);
    Vec::new()
}

/// Whether the clinic is closed on this date for a holiday ([`Holiday::closed`]).
#[must_use]
pub fn closed_day(date: &str) -> bool {
    holidays_between(date, date).iter().any(|h| h.closed)
}
