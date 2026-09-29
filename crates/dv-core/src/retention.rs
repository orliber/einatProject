//! Retention reminders (`STANDARDS.md` §5.6, P-07 ⚖️). The vault never erases a case on its
//! own: once a case's retention date passes, it is pointed out, and Einat decides (keep it
//! longer, or move it to the recycle bin, where erasing still needs the password).
//!
//! The date is the one Einat set in the case, or else the proposed default, awaiting the
//! lawyer's answer: seven years after the last change, or when the child turns 25, whichever
//! is later.

use dv_domain::{Age, CaseSummary};

use crate::dates::{add_years, date_of, iso, parse_iso, today};
use crate::views::RetentionItem;
use crate::{Core, CoreError};

/// Years a case is kept after it last changed (proposed, P-07).
pub const KEEP_YEARS_AFTER_LAST_CHANGE: i32 = 7;
/// Or until the child is this old, whichever is later (proposed, P-07).
pub const KEEP_UNTIL_AGE: i32 = 25;

/// The proposed default for a case.
pub(crate) fn default_until(created_at: i64, updated_at: i64, age: Option<&Age>) -> String {
    let after_last = add_years(date_of(updated_at), KEEP_YEARS_AFTER_LAST_CHANGE);
    // The child's age was recorded when the case was opened; without it, the first rule alone.
    let adult = age.map(|a| {
        let years = i32::from(a.years.min(30));
        add_years(date_of(created_at), (KEEP_UNTIL_AGE - years).max(0))
    });
    iso(adult.map_or(after_last, |a| a.max(after_last)))
}

/// Einat's date when she set one, else the default.
pub(crate) fn until(case: &CaseSummary) -> String {
    case.meta
        .retention_until
        .as_deref()
        .filter(|d| parse_iso(d).is_some())
        .map_or_else(
            || default_until(case.created_at, case.updated_at, case.meta.age.as_ref()),
            str::to_owned,
        )
}

impl Core {
    /// Cases at work whose retention date has passed (the recycle bin is not included).
    pub fn retention_due(&mut self) -> Result<Vec<RetentionItem>, CoreError> {
        let today = iso(today());
        let mut out: Vec<RetentionItem> = self
            .vault_ref()?
            .list_cases()?
            .into_iter()
            .filter_map(|c| {
                let until = until(&c);
                (until <= today).then(|| RetentionItem {
                    case_id: c.id.clone(),
                    label: match &c.child_name {
                        Some(name) => format!("{name} · {}", c.meta.code),
                        None => c.meta.code.clone(),
                    },
                    until,
                    by_default: c.meta.retention_until.is_none(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.until.cmp(&b.until));
        Ok(out)
    }

    /// "Keep it longer": the reminder comes back `years` from today.
    pub fn keep_case_longer(&mut self, case_id: &str, years: u8) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        let mut meta = v.case_meta(case_id)?;
        meta.retention_until = Some(iso(add_years(today(), i32::from(years.clamp(1, 10)))));
        v.update_case_meta(case_id, &meta)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn the_later_of_seven_years_and_age_twenty_five() {
        // Opened 2026-09-29 at age 5: 25 in 2046, later than 2033.
        let opened = 1_790_683_200;
        let five = Age {
            years: 5,
            months: 4,
        };
        assert_eq!(default_until(opened, opened, Some(&five)), "2046-09-29");
        // Older at the assessment: seven years after the last change is later.
        let older = Age {
            years: 22,
            months: 0,
        };
        assert_eq!(
            default_until(opened, opened + 30 * DAY, Some(&older)),
            "2033-10-29"
        );
        // No age recorded: seven years.
        assert_eq!(default_until(opened, opened, None), "2033-09-29");
    }
}
