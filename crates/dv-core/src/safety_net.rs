//! "The safety net" (D-107): one row, on General and on the security page, that says whether
//! she can still get in if the password is forgotten (sign-in with Google) and whether her
//! cases survive a broken or lost computer (a recent backup). When one is missing the row
//! turns red and says what to do; where Google sign-in cannot work at all, it is amber and
//! says so, and it is green only when both are really there.
//!
//! It is made from the two statuses that already exist (Google, backup), so it can never
//! disagree with them. A backup counts while no backup reminder is due: newer than
//! [`BACKUP_DAYS`](crate::BACKUP_DAYS), and made with the current password. That is the
//! strictest threshold the program already uses; "ready for real cases" uses it too since D-107.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::google::{ComputerKeys, GoogleStatus};
use crate::views::BackupStatus;
use crate::{Core, CoreError};

/// How one part of the net stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NetState {
    Ok,
    /// Missing or out of date: the row turns red, with a way to fix it.
    Missing,
    /// Cannot work on this computer (this version has no Google sign-in): no button can fix
    /// it, but the net is not whole either, so the row is never green (amber, said honestly).
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NetPart {
    pub state: NetState,
    /// The part as she reads it in the row ("גיבוי מלפני יומיים").
    pub text: String,
    /// What to do, when it is missing.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SafetyNet {
    pub google: NetPart,
    pub backup: NetPart,
    /// Both parts are there. False also when Google cannot work here.
    pub ok: bool,
}

/// How long ago, in her words: "היום", "אתמול", "לפני יומיים", "לפני 9 ימים".
#[must_use]
pub fn days_ago(days: i64) -> String {
    match days {
        i64::MIN..=0 => "היום".to_owned(),
        1 => "אתמול".to_owned(),
        2 => "לפני יומיים".to_owned(),
        d => format!("לפני {d} ימים"),
    }
}

fn part(state: NetState, text: &str, hint: Option<&str>) -> NetPart {
    NetPart {
        state,
        text: text.to_owned(),
        hint: hint.map(str::to_owned),
    }
}

/// The net, from the two statuses.
#[must_use]
pub fn safety_net(google: &GoogleStatus, backup: &BackupStatus) -> SafetyNet {
    let google = if !google.available {
        part(
            NetState::Unavailable,
            "כניסה עם גוגל לא זמינה במחשב הזה",
            Some("אם הסיסמה תישכח, אין כאן כניסה עם גוגל. כדאי לשמור את הסיסמה במקום בטוח."),
        )
    } else if google.on {
        part(NetState::Ok, "כניסה עם גוגל", None)
    } else if google.needs_setup_here {
        part(
            NetState::Missing,
            "כניסה עם גוגל לא מחוברת במחשב הזה",
            Some("היא הופעלה במחשב אחר. מחברים אותה שוב כאן, וכך אפשר להיכנס גם אם הסיסמה תישכח."),
        )
    } else {
        part(
            NetState::Missing,
            "כניסה עם גוגל כבויה",
            Some("מפעילים אותה פעם אחת, וכך אפשר להיכנס ולבחור סיסמה חדשה גם אם הסיסמה תישכח."),
        )
    };
    let backup = match backup.days_since {
        None => part(
            NetState::Missing,
            "אין עדיין גיבוי",
            Some("גיבוי מוצפן על דיסק נפרד שומר את התיקים אם המחשב יתקלקל או ילך לאיבוד."),
        ),
        Some(_) if backup.secret_changed => part(
            NetState::Missing,
            "הסיסמה הוחלפה אחרי הגיבוי האחרון",
            Some("הגיבוי הקודם נפתח רק בסיסמה הישנה. כדאי לגבות עכשיו."),
        ),
        Some(d) if backup.due => NetPart {
            state: NetState::Missing,
            text: format!("הגיבוי האחרון {}", days_ago(d)),
            hint: Some(format!(
                "גיבוי נחשב עדכני עד {} ימים. כדאי לגבות עכשיו.",
                crate::BACKUP_DAYS
            )),
        },
        Some(d) => NetPart {
            state: NetState::Ok,
            text: format!("גיבוי מ{}", days_ago(d)),
            hint: None,
        },
    };
    // The strictest reading (D-107): green only when both are really there.
    let ok = google.state == NetState::Ok && backup.state == NetState::Ok;
    SafetyNet { google, backup, ok }
}

impl Core {
    pub fn safety_net(&mut self, keys: &dyn ComputerKeys) -> Result<SafetyNet, CoreError> {
        let backup = self.backup_status()?;
        Ok(safety_net(&self.google_status(keys), &backup))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn google(available: bool, on: bool, needs_setup_here: bool) -> GoogleStatus {
        GoogleStatus {
            available,
            on,
            needs_setup_here,
        }
    }

    fn backup(days_since: Option<i64>, secret_changed: bool) -> BackupStatus {
        BackupStatus {
            last_at: days_since.map(|d| 1_790_000_000 - d * 86_400),
            days_since,
            // As `Core::backup_status` computes it.
            due: secret_changed || days_since.is_none_or(|d| d >= crate::BACKUP_DAYS),
            secret_changed,
            last_check_at: None,
            has_cases: true,
            auto: true,
        }
    }

    #[test]
    fn days_ago_reads_as_she_would_say_it() {
        assert_eq!(days_ago(0), "היום");
        assert_eq!(days_ago(-3), "היום", "a clock set back is not the future");
        assert_eq!(days_ago(1), "אתמול");
        assert_eq!(days_ago(2), "לפני יומיים");
        assert_eq!(days_ago(3), "לפני 3 ימים");
        assert_eq!(days_ago(9), "לפני 9 ימים");
        assert_eq!(days_ago(120), "לפני 120 ימים");
    }

    #[test]
    fn all_there_is_one_green_row() {
        let net = safety_net(&google(true, true, false), &backup(Some(2), false));
        assert!(net.ok);
        assert_eq!(net.google.text, "כניסה עם גוגל");
        assert_eq!(net.backup.text, "גיבוי מלפני יומיים");
        assert_eq!((net.google.hint, net.backup.hint), (None, None));
        let today = safety_net(&google(true, true, false), &backup(Some(0), false));
        assert_eq!(today.backup.text, "גיבוי מהיום");
        let yesterday = safety_net(&google(true, true, false), &backup(Some(1), false));
        assert_eq!(yesterday.backup.text, "גיבוי מאתמול");
    }

    #[test]
    fn a_backup_counts_exactly_as_long_as_no_reminder_is_due() {
        let last_good = crate::BACKUP_DAYS - 1;
        let net = safety_net(&google(true, true, false), &backup(Some(last_good), false));
        assert!(net.ok, "{last_good} days is still recent");
        let net = safety_net(
            &google(true, true, false),
            &backup(Some(crate::BACKUP_DAYS), false),
        );
        assert!(!net.ok);
        assert_eq!(net.backup.state, NetState::Missing);
        assert_eq!(net.backup.text, "הגיבוי האחרון לפני 7 ימים");
        assert!(net.backup.hint.is_some());
        let old = safety_net(&google(true, true, false), &backup(Some(9), false));
        assert_eq!(old.backup.text, "הגיבוי האחרון לפני 9 ימים");
    }

    #[test]
    fn no_backup_or_one_sealed_with_the_old_password_is_missing() {
        let none = safety_net(&google(true, true, false), &backup(None, false));
        assert!(!none.ok);
        assert_eq!(none.backup.text, "אין עדיין גיבוי");
        let rekeyed = safety_net(&google(true, true, false), &backup(Some(0), true));
        assert!(
            !rekeyed.ok,
            "a backup from today with the old password does not count"
        );
        assert_eq!(rekeyed.backup.text, "הסיסמה הוחלפה אחרי הגיבוי האחרון");
    }

    #[test]
    fn google_off_or_not_here_is_missing_and_says_what_to_do() {
        let off = safety_net(&google(true, false, false), &backup(Some(0), false));
        assert!(!off.ok);
        assert_eq!(off.google.state, NetState::Missing);
        assert_eq!(off.google.text, "כניסה עם גוגל כבויה");
        assert!(off.google.hint.is_some());
        let elsewhere = safety_net(&google(true, false, true), &backup(Some(0), false));
        assert!(!elsewhere.ok);
        assert_eq!(elsewhere.google.text, "כניסה עם גוגל לא מחוברת במחשב הזה");
    }

    #[test]
    fn where_google_cannot_work_the_row_is_never_green() {
        let net = safety_net(&google(false, false, false), &backup(Some(1), false));
        assert_eq!(net.google.state, NetState::Unavailable);
        assert!(!net.ok, "no forgotten-password path here: not a whole net");
        assert!(net.google.hint.is_some(), "said honestly");
        assert_eq!(net.backup.state, NetState::Ok);
        let net = safety_net(&google(false, false, false), &backup(None, false));
        assert!(!net.ok);
    }
}
