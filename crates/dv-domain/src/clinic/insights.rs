//! "תמונת מצב": how the clinic is doing over a stretch of whole months (D-200).
//!
//! Counts, sums, rates and medians only: no name, no id and no text she wrote ever leaves
//! this function, so the screen is the same in discreet mode. Pure, like the rest of the
//! rules; "today" is given by the caller.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{
    Agorot, ClinicRecords, ClinicSettings, ErasedCards, ErasedTally, IncomeMonth, KindCount,
    MethodTotal,
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
    /// Minutes of held visits, and the minutes of her working hours on working days that
    /// were not closed for a holiday, from the first day of the stretch up to today (or the
    /// stretch's end, the earlier). Cards that still exist only.
    pub held_minutes: u32,
    pub work_minutes: u32,
    /// `held_minutes` per thousand of `work_minutes`; 0 when there are no working minutes.
    pub occupancy: u16,
    /// Cards opened in the stretch (by `created_on`), erased ones included.
    pub new_patients: u32,
    pub referrals: Vec<ReferralCount>,
    /// Feedback meetings held in the stretch: assessments brought to an end.
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

/// The picture of the months `from_month` to `to_month` (`YYYY-MM`, both included, at most
/// [`MAX_INSIGHT_MONTHS`]). `records` are all the clinic's records; `tally` and `cards` what
/// erased cards counted for (D-179).
pub fn insights(
    records: &ClinicRecords<'_>,
    settings: &ClinicSettings,
    from_month: &str,
    to_month: &str,
    today: &str,
    tally: &ErasedTally,
    cards: &ErasedCards,
) -> Result<Insights, DomainError> {
    let _ = (records, settings, from_month, to_month, today, tally, cards);
    Err(DomainError::Invalid("עוד לא מוכן.".to_owned()))
}
