//! The activity log as Einat reads it: who entered, what went to Claude, what was exported or
//! deleted, and whether the chain of records is intact (STANDARDS.md §5.5).
//!
//! The log itself holds metadata only, and a case appears in it as a keyed reference, never by
//! id or name. The case is named here, on this computer, by matching the reference against the
//! cases in the vault; a case erased since shows as such.

use std::collections::HashMap;

use dv_domain::ReportStructure;
use dv_vault::AuditEvent;
use serde_json::Value;

use crate::dates::unix_now;
use crate::views::{ActivityEntry, ActivityPage};
use crate::{Core, CoreError};

/// Entries per page.
pub const ACTIVITY_PAGE: u32 = 50;
const REVIEWED_KEY: &str = "audit_reviewed_at";
/// Settings the program keeps for itself; each one only repeats an event already shown.
const INTERNAL_SETTINGS: &[&str] = &[
    "backup_last_at",
    "backup_last_dir",
    "backup_last_check_at",
    "secret_changed_at",
    "first_use_day",
    // An open edit kept at lock and given back at the next entry.
    "unsaved_edit",
    REVIEWED_KEY,
];

/// `(kind, text, warn)`, or `None` for an entry that only repeats another one.
fn describe(
    event: &str,
    meta: &Value,
    structure: Option<&ReportStructure>,
) -> Option<(&'static str, String, bool)> {
    let flag = |k: &str| meta.get(k).and_then(Value::as_bool).unwrap_or(false);
    let text = |k: &str| meta.get(k).and_then(Value::as_str).unwrap_or("");
    let number = |k: &str| meta.get(k).and_then(Value::as_i64).unwrap_or(0);
    let demo = if flag("demo") {
        " (הדגמה: לא יצא מהמחשב)"
    } else {
        ""
    };
    // Who answered, as recorded at the time; entries from before D-040 were all Claude.
    let ai = ["ChatGPT", "Gemini", "Mistral", "Ollama"]
        .into_iter()
        .find(|n| *n == text("ai"))
        .unwrap_or("Claude");
    Some(match event {
        "vault_created" => ("security", "הכספת נוצרה".to_owned(), false),
        "unlock" if text("method") == "recovery" => {
            ("access", "כניסה עם ערכת השחזור".to_owned(), true)
        }
        "unlock" if text("method") == "google" => (
            "access",
            "כניסה עם חשבון הגוגל (הסיסמה נשכחה) ובחירת סיסמה חדשה".to_owned(),
            true,
        ),
        "unlock" => ("access", "כניסה".to_owned(), false),
        "unlock_failed" => (
            "access",
            match number("count") {
                1 => "ניסיון כניסה שנכשל לפני הכניסה הזו".to_owned(),
                n => format!("{n} ניסיונות כניסה שנכשלו לפני הכניסה הזו"),
            },
            true,
        ),
        "lock" if text("reason") == "sleep" => {
            ("access", "נעילה (המחשב נכנס לשינה)".to_owned(), false)
        }
        "lock" if text("reason") == "computer_locked" => {
            ("access", "נעילה (המחשב ננעל)".to_owned(), false)
        }
        "lock" => ("access", "נעילה".to_owned(), false),
        "password_changed" => ("security", "הסיסמה הוחלפה".to_owned(), false),
        "recovery_key_rotated" => ("security", "נוצרה ערכת שחזור חדשה".to_owned(), false),
        "google_recovery_on" => (
            "security",
            "הופעלה כניסה עם גוגל למקרה ששוכחים את הסיסמה".to_owned(),
            true,
        ),
        "google_recovery_off" => (
            "security",
            "בוטלה הכניסה עם גוגל למקרה ששוכחים את הסיסמה".to_owned(),
            true,
        ),
        "case_created" => ("case", "תיק נפתח".to_owned(), false),
        "case_opened" => ("case", "תיק נפתח לצפייה".to_owned(), false),
        "case_trashed" => ("case", "תיק הועבר לסל המחזור".to_owned(), false),
        "case_restored" => ("case", "תיק הוחזר מסל המחזור".to_owned(), false),
        "case_deleted" => ("case", "תיק נמחק לצמיתות".to_owned(), true),
        "folders_changed" if flag("created") => ("case", "נוצרה תיקייה".to_owned(), false),
        "folders_changed" if flag("moved") => ("case", "תיקייה הועברה".to_owned(), false),
        "folders_changed" => ("case", "תיקייה נמחקה".to_owned(), false),
        "identities_changed" if flag("practitioner") => {
            ("case", "השמות שלך עודכנו".to_owned(), false)
        }
        "identities_changed" => ("case", "רשימת האנשים בתיק עודכנה".to_owned(), false),
        "send" if flag("consult") => ("send", format!("התייעצות נשלחה ל-{ai}{demo}"), false),
        "send" if flag("sorting") => (
            "send",
            format!("החומרים נשלחו ל-{ai} למיון לסעיפים{demo}"),
            false,
        ),
        "send" => {
            let key = text("section");
            let title = structure
                .and_then(|s| s.section(key))
                .map_or(key, |s| s.title.as_str());
            (
                "send",
                format!("הסעיף \"{title}\" נשלח ל-{ai}{demo}"),
                false,
            )
        }
        "blocked" => ("send", "השער עצר שליחה (נמצא מזהה)".to_owned(), true),
        "override" => ("send", "שליחה אושרה למרות אזהרה".to_owned(), true),
        "export" if flag("protected") => ("case", "דוח Word הופק, מוגן בסיסמה".to_owned(), false),
        "export" => ("case", "דוח Word הופק, בלי סיסמה".to_owned(), true),
        "backup_written" => ("security", "גיבוי מוצפן נשמר".to_owned(), false),
        "backup_checked" if flag("ok") => (
            "security",
            match number("cases") {
                1 => "תרגול שחזור: הגיבוי נפתח ותקין (תיק אחד)".to_owned(),
                n => format!("תרגול שחזור: הגיבוי נפתח ותקין ({n} תיקים)"),
            },
            false,
        ),
        "backup_checked" => (
            "security",
            "תרגול שחזור: הגיבוי נפתח עם חריגה בבדיקת השלמות".to_owned(),
            true,
        ),
        "restored" => ("security", "הכספת שוחזרה מגיבוי".to_owned(), true),
        "settings_changed" if INTERNAL_SETTINGS.contains(&text("key")) => return None,
        // This month's token counts change with every answer; the send itself is logged.
        "settings_changed" if text("key").starts_with("usage/") => return None,
        "settings_changed" if text("key") == "monthly_cap_usd" => (
            "security",
            "תקרת ההוצאה החודשית על AI עודכנה".to_owned(),
            false,
        ),
        "settings_changed" if text("key") == "backup_auto" => (
            "security",
            "הגיבוי האוטומטי הודלק או כובה".to_owned(),
            false,
        ),
        "settings_changed" if text("key").starts_with("ready/") => (
            "security",
            "עודכן אישור ברשימה \"מוכנה לעבודה אמיתית\"".to_owned(),
            false,
        ),
        "settings_changed" if !text("secret").is_empty() => {
            ("security", "מפתח ה-API נשמר".to_owned(), false)
        }
        "settings_changed" if !text("secret_deleted").is_empty() => {
            ("security", "מפתח ה-API נמחק".to_owned(), false)
        }
        "settings_changed" if text("key") == "screen_protection" => (
            "security",
            "ההגנה מצילום מסך ושיתוף מסך הודלקה או כובתה".to_owned(),
            true,
        ),
        "settings_changed" => ("security", "הגדרה עודכנה".to_owned(), false),
        "integrity_warning" => (
            "security",
            format!("בדיקת השלמות מצאה חריגה: {}", text("reason")),
            true,
        ),
        "audit_reviewed" => ("security", "היומן נבדק".to_owned(), false),
        "consultation_deleted" => ("case", "שיחת התייעצות נמחקה".to_owned(), false),
        "style_source_added" => (
            "security",
            "דוח ישן נוסף ל\"הדוחות שלי\" (נשמר רק הטקסט המנוטרל)".to_owned(),
            false,
        ),
        "style_source_deleted" => ("security", "דוח ישן נמחק מ\"הדוחות שלי\"".to_owned(), false),
        "style_profile_approved" => (
            "security",
            format!("פרופיל הסגנון אושר (גרסה {})", number("version")),
            false,
        ),
        other => ("security", other.to_owned(), false),
    })
}

impl Core {
    /// A page of the log, newest first. `before` is the `seq` of the last entry already shown.
    pub fn activity(&mut self, before: Option<i64>) -> Result<ActivityPage, CoreError> {
        let v = self.vault_ref()?;
        let structure = ReportStructure::load_default().ok();
        let mut names: HashMap<String, (String, bool)> = HashMap::new();
        for (case, trashed) in v
            .list_cases()?
            .into_iter()
            .map(|c| (c, false))
            .chain(v.list_trash()?.into_iter().map(|c| (c, true)))
        {
            let label = match &case.child_name {
                Some(name) => format!("{name} · {}", case.meta.code),
                None => case.meta.code.clone(),
            };
            names.insert(v.case_ref(&case.id), (label, trashed));
        }
        let raw = v.audit_page(before, ACTIVITY_PAGE)?;
        let more = raw.len() == ACTIVITY_PAGE as usize;
        let last_seq = raw.last().map(|e| e.seq);
        let entries = raw
            .into_iter()
            .filter_map(|e| {
                let meta: Value = serde_json::from_str(&e.meta).unwrap_or(Value::Null);
                let (kind, text, warn) = describe(&e.event, &meta, structure.as_ref())?;
                let case = e.case_ref.as_ref().map(|r| {
                    names.get(r).map_or_else(
                        || "תיק שנמחק".to_owned(),
                        |(label, trashed)| {
                            if *trashed {
                                format!("{label} (בסל המחזור)")
                            } else {
                                label.clone()
                            }
                        },
                    )
                });
                Some(ActivityEntry {
                    seq: e.seq,
                    ts: e.ts,
                    event: e.event,
                    kind: kind.to_owned(),
                    text,
                    warn,
                    case,
                })
            })
            .collect();
        Ok(ActivityPage {
            entries,
            more,
            last_seq,
            intact: before.is_some() || v.audit_intact()?,
            reviewed_at: v.setting(REVIEWED_KEY)?.and_then(|t| t.parse().ok()),
        })
    }

    /// "I went over the log" (periodic review of access records), kept in the log itself.
    pub fn mark_activity_reviewed(&mut self) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        v.record(AuditEvent::AuditReviewed, None, &serde_json::json!({}))?;
        v.set_setting(REVIEWED_KEY, &unix_now().to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_event_reads_as_hebrew() {
        let structure = ReportStructure::load_default().ok();
        for event in [
            "vault_created",
            "unlock",
            "unlock_failed",
            "lock",
            "password_changed",
            "recovery_key_rotated",
            "google_recovery_on",
            "google_recovery_off",
            "case_created",
            "case_opened",
            "case_trashed",
            "case_restored",
            "case_deleted",
            "folders_changed",
            "identities_changed",
            "send",
            "blocked",
            "override",
            "export",
            "backup_written",
            "backup_checked",
            "restored",
            "settings_changed",
            "integrity_warning",
            "audit_reviewed",
            "consultation_deleted",
            "style_source_added",
            "style_source_deleted",
            "style_profile_approved",
        ] {
            let (_, text, _) = describe(event, &Value::Null, structure.as_ref()).unwrap();
            assert!(!text.contains('_'), "{event} → {text}");
        }
        let send = serde_json::json!({ "section": "background", "demo": true });
        let (_, text, _) = describe("send", &send, structure.as_ref()).unwrap();
        assert!(
            text.contains("נשלח ל-Claude") && text.contains("הדגמה"),
            "{text}"
        );
        let consult = serde_json::json!({ "consult": true, "ai": "Gemini" });
        let (_, text, _) = describe("send", &consult, None).unwrap();
        assert!(text.contains("נשלחה ל-Gemini"), "{text}");
        let internal = serde_json::json!({ "key": "backup_last_at" });
        assert!(describe("settings_changed", &internal, None).is_none());
        let usage = serde_json::json!({ "key": "usage/2026-10" });
        assert!(describe("settings_changed", &usage, None).is_none());
        let ready = serde_json::json!({ "key": "ready/zdr" });
        let (_, text, _) = describe("settings_changed", &ready, None).unwrap();
        assert!(text.contains("מוכנה לעבודה אמיתית"), "{text}");
    }
}
