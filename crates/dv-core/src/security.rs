//! "Security status" (S4): one honest list of which protections are on right now, built from
//! facts the program can check itself. It never claims a protection it did not verify, and a
//! missing protection is shown, not hidden. The emergency stop for sending lives here too.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::layers::{self, SecurityLayer, SelfChecks};
use crate::{AppStatus, Core, CoreError};

/// `ok` – on and verified · `warn` – off or unknown, she should look · `info` – a fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SecurityItem {
    pub key: String,
    pub state: String,
    /// A number or name that goes into the sentence (minutes, a folder), if any.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SecurityStatus {
    pub items: Vec<SecurityItem>,
    /// Sending to any outside AI is switched off right now (emergency stop).
    pub send_stopped: bool,
    /// Requests the program may send per day, and the largest request, as the program enforces.
    pub daily_limit: u32,
    pub max_request_kb: u32,
    /// The same, told as the ten layers of protection, checked now (L6).
    pub layers: Vec<SecurityLayer>,
    /// What depends on her: the disk, the cloud folder, the printed kit, the backup.
    pub yours: Vec<SecurityLayer>,
}

fn item(key: &str, state: &str, detail: Option<String>) -> SecurityItem {
    SecurityItem {
        key: key.to_owned(),
        state: state.to_owned(),
        detail,
    }
}

fn on(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "warn"
    }
}

/// The list for a given status. Pure, so it is tested without a vault.
pub(crate) fn build(s: &AppStatus, send_stopped: bool) -> SecurityStatus {
    let mut items = vec![
        item("encrypted", on(s.vault_exists), None),
        item("fips", if s.fips_active { "ok" } else { "info" }, None),
        item(
            "disk",
            match s.disk_encryption.as_str() {
                "on" => "ok",
                _ => "warn",
            },
            None,
        ),
        item(
            "cloud_folder",
            on(s.cloud_synced_folder.is_none()),
            s.cloud_synced_folder.clone(),
        ),
        item(
            "log_integrity",
            on(s.integrity_warning.is_none()),
            s.integrity_warning.clone(),
        ),
        // The program itself counts the idle time and the computer's own lock and sleep; a
        // stuck window cannot keep the vault open (S5).
        item("auto_lock", "ok", Some(s.lock_minutes.to_string())),
        item("screen", on(s.screen_protection), None),
        item("send", on(!send_stopped), None),
    ];
    // The document readers run in a restricted process on Windows (D-047) and macOS (D-101).
    items.push(item(
        "isolation",
        if dv_ingest::worker::CONTAINED {
            "ok"
        } else {
            "warn"
        },
        None,
    ));
    SecurityStatus {
        items,
        send_stopped,
        daily_limit: u32::try_from(dv_egress::DAILY_REQUESTS).unwrap_or(u32::MAX),
        max_request_kb: u32::try_from(dv_egress::MAX_BODY_BYTES / 1024).unwrap_or(u32::MAX),
        layers: Vec::new(),
        yours: Vec::new(),
    }
}

impl Core {
    pub fn security_status(&mut self) -> SecurityStatus {
        let checks = SelfChecks::run();
        layers::stop_if_broken(checks);
        let status = self.status();
        let mut s = build(&status, dv_egress::is_stopped());
        // The password opens the vault only on this computer (D-076).
        let bound = self.binding_state();
        s.items.insert(
            1,
            match bound {
                "elsewhere" => item("computer_bound", "warn", Some(bound.to_owned())),
                "busy" => item("computer_bound", "info", Some(bound.to_owned())),
                _ => item("computer_bound", bound, None),
            },
        );
        // The vault's pages stay in RAM, never in the page file (D-047).
        let memory = match self.memory_lock {
            Some(true) => "ok",
            Some(false) => "warn",
            None => "info",
        };
        s.items.push(item("memory", memory, None));
        // A forgotten password is opened with her Google account, not a printed kit (D-103):
        // a reminder until it is on. Where this version has no Google, it is only said.
        if self.vault.is_some() {
            // The same answer as the lock screen's: the slot, and the half kept here. A restored
            // backup on a new computer has the slot but not the half: it needs turning on again.
            let google = self.google_status(&crate::google::WindowsAccount);
            let (state, detail) = if google.on {
                ("ok", None)
            } else if google.needs_setup_here {
                ("warn", Some("setup_here".to_owned()))
            } else if google.available {
                ("warn", None)
            } else {
                ("info", None)
            };
            s.items.insert(2, item("forgot_password", state, detail));
        }
        s.layers = layers::build(&s.items, checks, s.send_stopped, status.review_always);
        let backup = self
            .vault
            .is_some()
            .then(|| self.backup_status().ok())
            .flatten()
            .map(|b| (b.due, b.has_cases));
        s.yours = layers::yours(&s.items, backup);
        s
    }

    /// The emergency stop. Works while locked too: stopping must never need a password.
    /// Turning sending back on tries the gate and the sending rules first, and is refused
    /// while either fails, so it is never open for a moment (review of stage 5, U1).
    pub fn set_send_stopped(&mut self, stopped: bool) -> Result<SecurityStatus, CoreError> {
        if !stopped && layers::stop_if_broken(SelfChecks::run()) {
            return Err(CoreError::Refused(
                "אי אפשר להפעיל שליחה מחדש: השער או כללי השליחה לא עברו את הבדיקה עכשיו. כדאי לפנות לאור."
                    .to_owned(),
            ));
        }
        dv_egress::set_stopped(stopped);
        Ok(self.security_status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> AppStatus {
        AppStatus {
            vault_exists: true,
            unlocked: true,
            disk_encryption: "on".to_owned(),
            cloud_synced_folder: None,
            fips_active: false,
            demo_mode: true,
            model: String::new(),
            provider: "anthropic".to_owned(),
            ai_name: "Claude".to_owned(),
            keys: Vec::new(),
            speed: "balanced".to_owned(),
            integrity_warning: None,
            lock_minutes: 10,
            idle_lock_in: None,
            practitioner: Vec::new(),
            review_always: true,
            screen_protection: true,
            since_last: None,
            newer_version: None,
        }
    }

    fn state<'a>(s: &'a SecurityStatus, key: &str) -> &'a str {
        &s.items.iter().find(|i| i.key == key).unwrap().state
    }

    #[test]
    fn a_missing_protection_is_shown_as_a_warning() {
        let mut s = status();
        s.disk_encryption = "unknown".to_owned();
        s.cloud_synced_folder = Some("OneDrive".to_owned());
        s.integrity_warning = Some("x".to_owned());
        s.screen_protection = false;
        let r = build(&s, true);
        for key in ["disk", "cloud_folder", "log_integrity", "screen", "send"] {
            assert_eq!(state(&r, key), "warn", "{key}");
        }
        assert!(r.send_stopped);
    }

    #[test]
    fn a_healthy_machine_shows_ok_and_the_real_limits() {
        let r = build(&status(), false);
        assert_eq!(state(&r, "disk"), "ok");
        assert_eq!(state(&r, "send"), "ok");
        assert_eq!(r.max_request_kb, 2048);
        assert_eq!(
            r.items
                .iter()
                .find(|i| i.key == "auto_lock")
                .unwrap()
                .detail,
            Some("10".to_owned())
        );
    }
}
