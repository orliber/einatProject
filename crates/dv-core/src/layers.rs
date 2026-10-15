//! "The protection layers" (L6, D-078): the security page told as ten layers, each with what it
//! does, what it protects against, and its state as the program checked it just now. Three of
//! them are not read from a setting but tried on the spot: the gate is given an invented
//! child's name and must refuse it, the sending rules are given requests they must refuse,
//! and the pins of the AI service must all be in the program. The same list makes the PDF for
//! the parents and the institute, so the page and the paper never say different things.
//!
//! When the gate or the rules fail their own check, sending stops at once (fail-closed): the
//! emergency stop is switched on, and the page says why.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::security::SecurityItem;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SecurityLayer {
    pub key: String,
    pub title: String,
    /// What it does, in one sentence.
    pub what: String,
    /// What it protects against.
    pub against: String,
    /// `ok` – on and checked now · `warn` – she should look · `info` – a fact.
    pub state: String,
    /// What was found just now.
    pub now: String,
}

/// The checks tried on the spot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelfChecks {
    pub gate: bool,
    pub rules: bool,
    pub pins: bool,
    pub signed_updates: bool,
}

impl SelfChecks {
    pub(crate) fn run() -> Self {
        Self {
            gate: dv_privacy::gate_self_check(crate::dates::today()),
            rules: dv_egress::policy_self_check(),
            pins: dv_egress::pinning_in_place(),
            signed_updates: !dv_egress::update::pinned_keys().is_empty(),
        }
    }
}

/// The gate or the rules failed their own check: sending stops (fail-closed).
pub(crate) fn stop_if_broken(checks: SelfChecks) -> bool {
    let broken = !checks.gate || !checks.rules;
    if broken {
        dv_egress::set_stopped(true);
    }
    broken
}

fn layer(
    key: &str,
    title: &str,
    what: &str,
    against: &str,
    state: &str,
    now: &str,
) -> SecurityLayer {
    SecurityLayer {
        key: key.to_owned(),
        title: title.to_owned(),
        what: what.to_owned(),
        against: against.to_owned(),
        state: state.to_owned(),
        now: now.to_owned(),
    }
}

fn find<'a>(items: &'a [SecurityItem], key: &str) -> Option<&'a SecurityItem> {
    items.iter().find(|i| i.key == key)
}

fn state_of<'a>(items: &'a [SecurityItem], key: &str) -> &'a str {
    find(items, key).map_or("warn", |i| i.state.as_str())
}

fn detail_of<'a>(items: &'a [SecurityItem], key: &str) -> Option<&'a str> {
    find(items, key).and_then(|i| i.detail.as_deref())
}

fn ok_if(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "warn"
    }
}

/// The ten layers, from the status items and the checks tried now. Pure, so it is tested
/// with a failing check too.
pub(crate) fn build(
    items: &[SecurityItem],
    checks: SelfChecks,
    send_stopped: bool,
    review_always: bool,
) -> Vec<SecurityLayer> {
    let encrypted = state_of(items, "encrypted") == "ok";
    let fips = state_of(items, "fips") == "ok";
    let bound = state_of(items, "computer_bound");
    let elsewhere = detail_of(items, "computer_bound") == Some("elsewhere");
    let minutes = detail_of(items, "auto_lock").unwrap_or("10");
    let screen = state_of(items, "screen") == "ok";
    let isolation = state_of(items, "isolation");
    let memory = state_of(items, "memory");
    let log = find(items, "log_integrity");

    let pins = dv_egress::pinned_root_count();
    let send_state = if !checks.rules {
        "warn"
    } else if send_stopped {
        "info"
    } else {
        "ok"
    };
    let send_now = if !checks.rules {
        "הבדיקה של כללי השליחה נכשלה עכשיו, ולכן השליחה נעצרה. כדאי לפנות לאור.".to_owned()
    } else {
        let review = if review_always {
            "כל שליחה מוצגת על המסך לאישור לפני שהיא יוצאת."
        } else {
            "המסך \"לפני ששולחים\" כבוי, והשער עדיין בודק כל שליחה."
        };
        let stop = if send_stopped {
            " השליחה כבויה עכשיו (עצירת חירום)."
        } else {
            ""
        };
        format!("נבדק עכשיו: מודל אחר, שדה מיותר ואזור אחר נחסמו. {review}{stop}")
    };

    vec![
        layer(
            "encryption",
            "הצפנת הכספת",
            "כל התיקים שמורים מוצפנים (AES-256), עם מפתח נפרד לכל תיק.",
            "מי שמעתיק את קבצי הכספת או לוקח את המחשב.",
            ok_if(encrypted),
            match (encrypted, fips) {
                (false, _) => "עוד אין כספת במחשב הזה.",
                (true, true) => "הכספת מוצפנת, במודול הצפנה מאושר FIPS 140-3.",
                (true, false) => "הכספת מוצפנת.",
            },
        ),
        layer(
            "computer_bound",
            "קשירה למחשב",
            "הסיסמה פותחת את הכספת רק במחשב הזה ובחשבון ה-Windows הזה. אם המחשב התקלקל או אבד, משחזרים את הגיבוי במחשב החדש עם הסיסמה או עם ערכת השחזור. בעותק של תיקיית הכספת, אחרי התקנה מחדש של Windows או אחרי איפוס החשבון, פותחת אותה רק ערכת השחזור.",
            "ניסיון לנחש את הסיסמה על עותק של הכספת במחשב אחר.",
            bound,
            match bound {
                _ if elsewhere => "הסיסמה עוד לא פותחת את הכספת במחשב הזה. בוחרים סיסמה ב\"החלפת סיסמה…\" עם ערכת השחזור.",
                "ok" => "הכספת קשורה למחשב הזה.",
                "info" => "במחשב הזה אין אפשרות כזו.",
                _ => "הכספת תיקשר למחשב בכניסה הבאה עם הסיסמה.",
            },
        ),
        layer(
            "privacy_gate",
            "הסתרת פרטים לפני שליחה",
            "שמות, תאריכים ומספרים מוחלפים בתגיות לפני שמשהו יוצא, ושער אחרון חוסם כל טקסט שלא עבר את הסינון.",
            "דליפה של פרטים מזהים של ילד אל שירות ה-AI.",
            ok_if(checks.gate),
            if checks.gate {
                "נבדק עכשיו: השער חסם שם גלוי של ילד בדוי ומחרוזת מלכודת, והעביר את אותו משפט עם תגית."
            } else {
                "הבדיקה של השער נכשלה עכשיו, ולכן השליחה נעצרה. כדאי לפנות לאור."
            },
        ),
        layer(
            "send_rules",
            "שליחה רק לפי הכללים",
            "רק למודלים שברשימה, באזור ארה\"ב, בלי שמירת מידע אצל השירות (ZDR), ועם תקרה יומית.",
            "שליחה של יותר ממה שצריך, או למקום שלא אושר.",
            send_state,
            &send_now,
        ),
        layer(
            "pinned",
            "חיבור מאומת לשירות ה-AI",
            "החיבור מוצפן, ומתקבל רק עם תעודה מהגורמים שמנפיקים לשירות את התעודות שלו.",
            "מי שמנסה לקרוא את התעבורה בדרך (רשת ציבורית, תוכנה במחשב).",
            ok_if(checks.pins),
            &if checks.pins {
                format!("נבדק עכשיו: כל {pins} התעודות המוכרות נמצאות בתוכנה.")
            } else {
                "חסרה בתוכנה תעודה מוכרת. צריך עדכון של התוכנה.".to_owned()
            },
        ),
        layer(
            "isolation",
            "קריאת מסמכים מבודדת",
            "קבצי Word ו-PDF נקראים בתהליך נפרד, בלי רשת ובלי גישה לקבצים אחרים.",
            "מסמך זדוני שמנסה להריץ קוד או להוציא מידע.",
            isolation,
            if isolation == "ok" {
                "פעיל במחשב הזה."
            } else {
                "במחשב הזה אין בידוד כזה."
            },
        ),
        layer(
            "lock",
            "נעילה והגנת מסך",
            "הכספת ננעלת לבד כשאין פעילות, כשנועלים את המחשב וכשהוא נרדם. צילום מסך ושיתוף מסך מציגים חלון ריק.",
            "מי שניגש למחשב פתוח, או שיתוף מסך בטעות.",
            ok_if(screen),
            &if screen {
                format!("נעילה אחרי {minutes} דקות, והגנת המסך פעילה.")
            } else {
                format!("נעילה אחרי {minutes} דקות. הגנת המסך לא פעילה בחלון הזה.")
            },
        ),
        layer(
            "memory",
            "זיכרון",
            "מה שפתוח בכספת נשאר בזיכרון, לא נכתב לקובץ הדפדוף, ונמחק בנעילה.",
            "שאריות מידע בדיסק של המחשב.",
            memory,
            match memory {
                "ok" => "פעיל בהפעלה הזו.",
                "info" => "במחשב הזה אין אפשרות כזו.",
                _ => "Windows לא אישר את המקום לזה בהפעלה הזו.",
            },
        ),
        layer(
            "log",
            "יומן שלא ניתן לשנות",
            "כל פתיחה של תיק, שליחה וייצוא נרשמים ביומן חתום, ושינוי בו מתגלה.",
            "שינוי או מחיקה של עקבות.",
            log.map_or("info", |i| i.state.as_str()),
            &match log {
                Some(i) if i.state == "ok" => "נבדק עכשיו: היומן שלם.".to_owned(),
                Some(i) => format!(
                    "נמצא שינוי ביומן: {}",
                    i.detail.as_deref().unwrap_or("כדאי לפנות לאור.")
                ),
                None => "נבדק כשהכספת פתוחה.".to_owned(),
            },
        ),
        layer(
            "updates",
            "עדכונים חתומים",
            "עדכון מותקן רק אם הוא חתום במפתח של התוכנה, וטביעת האצבע של הקובץ תואמת.",
            "עדכון מזויף.",
            ok_if(checks.signed_updates),
            if checks.signed_updates {
                "מפתח החתימה נמצא בתוכנה."
            } else {
                "בגרסה הזו אין מפתח חתימה, ולכן לא מותקנים בה עדכונים."
            },
        ),
    ]
}

/// What depends on her, and is not a layer of the program.
pub(crate) fn yours(items: &[SecurityItem], backup: Option<(bool, bool)>) -> Vec<SecurityLayer> {
    let mut out = Vec::new();
    let disk = state_of(items, "disk");
    out.push(layer(
        "disk",
        "הצפנת הדיסק (BitLocker)",
        "מצפינה את כל המחשב, גם מה שמחוץ לכספת.",
        "גניבה של המחשב.",
        disk,
        if disk == "ok" {
            "פעילה."
        } else {
            "לא נמצאה. כדאי להפעיל אותה ב-Windows."
        },
    ));
    let cloud = detail_of(items, "cloud_folder");
    out.push(layer(
        "cloud_folder",
        "הכספת לא בתיקיית ענן",
        "הכספת נשארת במחשב ולא מסונכרנת לשום שירות.",
        "עותקים של הכספת אצל שירות ענן.",
        ok_if(cloud.is_none()),
        &match cloud {
            None => "הכספת לא בתיקיית ענן.".to_owned(),
            Some(c) => format!("הכספת בתיקייה ש-{c} מסנכרן. כדאי להעביר אותה."),
        },
    ));
    if let Some(kit) = find(items, "recovery_kit") {
        out.push(layer(
            "recovery_kit",
            "ערכת השחזור המודפסת",
            "פותחת את הכספת אם הסיסמה נשכחה או במחשב אחר.",
            "אובדן של כל התיקים.",
            &kit.state,
            &match kit.detail.as_deref() {
                None => "עוד לא נבדקה. בודקים אותה ב\"סיסמה ואבטחה\".".to_owned(),
                Some("0") => "נבדקה היום.".to_owned(),
                Some(d) if kit.state == "ok" => format!("נבדקה לפני {d} ימים."),
                Some(d) => format!("נבדקה לפני {d} ימים. כדאי לבדוק אותה שוב."),
            },
        ));
    }
    if let Some((due, has_cases)) = backup {
        out.push(layer(
            "backup",
            "גיבוי לכונן חיצוני",
            "עותק מוצפן של הכספת על כונן שנשמר מחוץ למחשב.",
            "מחשב שנגנב, נשרף או התקלקל.",
            ok_if(!due || !has_cases),
            if !has_cases {
                "עוד אין תיקים לגבות."
            } else if due {
                "הגיע הזמן לגיבוי."
            } else {
                "הגיבוי עדכני."
            },
        ));
    }
    out
}

/// "פעיל" or "לבדוק" after a layer's title on paper; a fact has no word.
fn state_word(state: &str) -> &'static str {
    match state {
        "ok" => ": פעיל",
        "warn" => ": לבדוק",
        _ => "",
    }
}

/// The layers on paper, for the parents and the institute: nothing about any child.
pub(crate) fn notice(layers: &[SecurityLayer], today: (i32, u32, u32)) -> dv_export::Notice {
    let (y, m, d) = today;
    let mut sections = vec![dv_export::ReportSection {
        title: String::new(),
        paragraphs: vec![
            format!(
                "כך שמור המידע על הילד ועל האבחון, נכון ל-{d}.{m}.{y}. שלוש שכבות (הסתרת הפרטים, כללי השליחה והחיבור המאומת) נוסו בידי התוכנה באותו רגע; בשאר מתואר המצב שלהן באותו יום."
            ),
            "התיקים נשמרים מוצפנים במחשב של המטפלת, ובגיבוי מוצפן על כונן שבידיה. לשירות ה-AI, שעוזר לנסח, נשלח רק טקסט בלי שמות, תאריכים או מספרים מזהים, ורק אחרי שהשער האחרון אישר אותו. האבחון עצמו נעשה בידי המטפלת."
                .to_owned(),
        ],
    }];
    for (n, l) in layers.iter().enumerate() {
        sections.push(dv_export::ReportSection {
            title: format!("{}. {}{}", n + 1, l.title, state_word(&l.state)),
            paragraphs: vec![
                l.what.clone(),
                format!("מגינה מפני: {}", l.against),
                l.now.clone(),
            ],
        });
    }
    dv_export::Notice {
        title: "שכבות ההגנה של כספת האבחון".to_owned(),
        sections,
        footer: format!(
            "כספת האבחון {}, נוצר ב-{d}.{m}.{y}",
            env!("CARGO_PKG_VERSION")
        ),
    }
}

impl crate::Core {
    /// The protection layers as a PDF that opens without a password and can only be printed.
    pub fn security_layers_pdf(&mut self) -> Result<Vec<u8>, crate::CoreError> {
        let layers = self.security_status().layers;
        let (regular, bold) = crate::system_fonts("David").ok_or_else(|| {
            crate::CoreError::Refused(
                "לא נמצא במחשב גופן עברי ליצירת PDF (למשל David או Arial).".to_owned(),
            )
        })?;
        dv_export::render_notice_pdf(
            &notice(&layers, crate::dates::today()),
            dv_export::PdfFonts {
                regular: &regular,
                bold: bold.as_deref(),
            },
        )
        .map_err(crate::jobs::export_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, state: &str, detail: Option<&str>) -> SecurityItem {
        SecurityItem {
            key: key.to_owned(),
            state: state.to_owned(),
            detail: detail.map(str::to_owned),
        }
    }

    fn healthy() -> Vec<SecurityItem> {
        vec![
            item("encrypted", "ok", None),
            item("computer_bound", "ok", None),
            item("auto_lock", "ok", Some("10")),
            item("screen", "ok", None),
            item("isolation", "ok", None),
            item("memory", "ok", None),
            item("log_integrity", "ok", None),
        ]
    }

    const ALL_PASS: SelfChecks = SelfChecks {
        gate: true,
        rules: true,
        pins: true,
        signed_updates: true,
    };

    #[test]
    fn ten_layers_each_with_its_sentence_and_what_it_protects_against() {
        let l = build(&healthy(), ALL_PASS, false, true);
        assert_eq!(l.len(), 10);
        for x in &l {
            assert_eq!(x.state, "ok", "{}", x.key);
            assert!(!x.what.is_empty() && !x.against.is_empty() && !x.now.is_empty());
        }
        assert!(l[3].now.contains("לאישור לפני"));
    }

    #[test]
    fn a_check_that_fails_now_is_shown_as_failing() {
        let checks = SelfChecks {
            gate: false,
            rules: false,
            pins: false,
            signed_updates: false,
        };
        let l = build(&healthy(), checks, false, true);
        for key in ["privacy_gate", "send_rules", "pinned", "updates"] {
            let x = l.iter().find(|x| x.key == key).unwrap();
            assert_eq!(x.state, "warn", "{key}");
        }
        assert!(l[2].now.contains("נעצרה"));
    }

    #[test]
    fn the_emergency_stop_is_a_fact_not_a_fault() {
        let l = build(&healthy(), ALL_PASS, true, false);
        let send = l.iter().find(|x| x.key == "send_rules").unwrap();
        assert_eq!(send.state, "info");
        assert!(send.now.contains("עצירת חירום") && send.now.contains("כבוי"));
    }

    #[test]
    fn what_depends_on_her_names_the_disk_the_kit_and_the_backup() {
        let mut items = healthy();
        items.push(item("disk", "warn", None));
        items.push(item("cloud_folder", "ok", None));
        items.push(item("recovery_kit", "warn", Some("400")));
        let y = yours(&items, Some((true, true)));
        let keys: Vec<_> = y.iter().map(|x| x.key.as_str()).collect();
        assert_eq!(keys, ["disk", "cloud_folder", "recovery_kit", "backup"]);
        assert!(y
            .iter()
            .all(|x| x.key == "cloud_folder" || x.state == "warn"));
        assert!(y[2].now.contains("400"));
    }

    #[test]
    fn the_paper_says_what_the_page_says() {
        let l = build(&healthy(), ALL_PASS, false, true);
        let n = notice(&l, (2026, 10, 15));
        assert_eq!(n.sections.len(), 11);
        assert_eq!(n.sections[1].title, "1. הצפנת הכספת: פעיל");
        for (s, x) in n.sections[1..].iter().zip(&l) {
            assert!(s.paragraphs.contains(&x.now));
        }
        assert!(n.footer.contains("15.10.2026"));
    }
}
