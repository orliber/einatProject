//! "Ready for real cases": one list of what must be true before a real child's material goes
//! into the program (ROADMAP stage 8, D-004). It informs and links; it never blocks work.
//!
//! Some items the program checks itself (a backup, disk encryption, an API key). The others
//! only she can confirm (a written ZDR agreement, the consent form, the score tables); each
//! confirmation is kept with its date, and shows in the activity log.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::dates::unix_now;
use crate::{Core, CoreError};

const PREFIX: &str = "ready/";

/// Items she confirms herself, in the order shown.
pub(crate) const CONFIRMED: &[&str] = &["zdr", "consent_form", "score_tables", "legal"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReadinessItem {
    /// `zdr` | `consent_form` | `score_tables` | `legal` | `backup` | `disk` | `api_key`.
    pub key: String,
    pub done: bool,
    /// True when the program checks it; false when she confirms it.
    pub checked_by_program: bool,
    /// When she confirmed it (unix seconds).
    #[ts(type = "number | null")]
    pub confirmed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Readiness {
    pub items: Vec<ReadinessItem>,
    pub all_done: bool,
}

impl Core {
    pub fn readiness(&mut self) -> Result<Readiness, CoreError> {
        let backup = self.backup_status()?;
        let disk_on = self.status().disk_encryption == "on";
        let provider = self.provider();
        let v = self.vault_ref()?;
        // A key for the AI she chose (the local model needs none; D-040).
        let api_key = crate::key_of(v, provider)?.is_some();
        let mut items = Vec::new();
        for key in CONFIRMED {
            let confirmed_at = v
                .setting(&format!("{PREFIX}{key}"))?
                .and_then(|s| s.parse::<i64>().ok());
            items.push(ReadinessItem {
                key: (*key).to_owned(),
                done: confirmed_at.is_some(),
                checked_by_program: false,
                confirmed_at,
            });
        }
        let auto = |key: &str, done: bool| ReadinessItem {
            key: key.to_owned(),
            done,
            checked_by_program: true,
            confirmed_at: None,
        };
        items.push(auto("api_key", api_key));
        items.push(auto(
            "backup",
            !backup.secret_changed && backup.days_since.is_some_and(|d| d <= 14),
        ));
        items.push(auto("disk", disk_on));
        let all_done = items.iter().all(|i| i.done);
        Ok(Readiness { items, all_done })
    }

    /// She confirms an item (or takes the confirmation back).
    pub fn confirm_readiness(&mut self, key: &str, done: bool) -> Result<Readiness, CoreError> {
        if !CONFIRMED.contains(&key) {
            return Err(CoreError::NotFound("פריט לא מוכר".to_owned()));
        }
        let v = self.vault_mut()?;
        let value = if done {
            unix_now().to_string()
        } else {
            // Empty does not parse as a date: not confirmed.
            String::new()
        };
        // Recorded in the activity log by the vault (`settings_changed`).
        v.set_setting(&format!("{PREFIX}{key}"), &value)?;
        self.readiness()
    }
}
