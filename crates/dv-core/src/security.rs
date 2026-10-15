//! "Security status" (S4): one honest list of which protections are on right now, built from
//! facts the program can check itself. It never claims a protection it did not verify, and a
//! missing protection is shown, not hidden. The emergency stop for sending lives here too.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppStatus, Core};

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
    // The document readers run in a restricted process only on Windows (D-047).
    items.push(item(
        "isolation",
        if cfg!(windows) { "ok" } else { "warn" },
        None,
    ));
    SecurityStatus {
        items,
        send_stopped,
        daily_limit: u32::try_from(dv_egress::DAILY_REQUESTS).unwrap_or(u32::MAX),
        max_request_kb: u32::try_from(dv_egress::MAX_BODY_BYTES / 1024).unwrap_or(u32::MAX),
    }
}

/// The printed kit is checked again after this many days.
const KIT_CHECK_DAYS: i64 = 365;

impl Core {
    pub fn security_status(&mut self) -> SecurityStatus {
        let mut s = build(&self.status(), dv_egress::is_stopped());
        // The password opens the vault only on this computer (D-076).
        s.items
            .insert(1, item("computer_bound", self.binding_state(), None));
        // On another computer the printed kit is the way in: checked within a year, or a
        // reminder (the days since the last check, when there was one).
        if self.vault.is_some() {
            let days = self
                .kit_checked_at()
                .map(|t| (crate::dates::unix_now() - t).max(0) / dv_domain::dates::DAY_SECS);
            let fresh = days.is_some_and(|d| d < KIT_CHECK_DAYS);
            s.items.insert(
                2,
                item("recovery_kit", on(fresh), days.map(|d| d.to_string())),
            );
        }
        s
    }

    /// The emergency stop. Works while locked too: stopping must never need a password.
    pub fn set_send_stopped(&mut self, stopped: bool) -> SecurityStatus {
        dv_egress::set_stopped(stopped);
        self.security_status()
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
