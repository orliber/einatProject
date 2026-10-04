//! A measured benchmark of the filter on fabricated clinical sentences (D-034).
//!
//! Each sentence lists what must not leave as is (`hide`: hidden, or at least held as a
//! question that blocks sending) and ordinary words that must pass untouched (`keep`). The
//! test prints recall and false alarms per group and fails under the thresholds, so a change
//! that makes the filter weaker, or noisier, shows up as a number. All data is invented.

#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::panic)]

use dv_domain::{Identity, Role};
use dv_privacy::text::normalize;
use dv_privacy::{filter, PrivacyContext};

const CASE: &str = "case-bench";

fn id(role: Role, tag: &str, value: &str) -> Identity {
    Identity {
        id: format!("{CASE}-{tag}"),
        case_id: CASE.to_owned(),
        role,
        tag: tag.to_owned(),
        value: value.to_owned(),
        aliases: Vec::new(),
    }
}

struct Item {
    group: &'static str,
    text: &'static str,
    hide: &'static [&'static str],
    keep: &'static [&'static str],
}

const fn it(
    group: &'static str,
    text: &'static str,
    hide: &'static [&'static str],
    keep: &'static [&'static str],
) -> Item {
    Item {
        group,
        text,
        hide,
        keep,
    }
}

/// Fabricated sentences, the way reports and parents' letters are written.
const ITEMS: &[Item] = &[
    // Declared names, with prefixes and spellings.
    it(
        "declared",
        "ליאור הגיע עם אמו גלית ונפרד בקלות.",
        &["ליאור", "גלית"],
        &["הגיע", "ונפרד"],
    ),
    it(
        "declared",
        "לליאור קשה במעברים, וכשגלית יוצאת הוא בוכה.",
        &["ליאור", "גלית"],
        &["במעברים"],
    ),
    it(
        "declared",
        "המורה אסנת ציינה שליאור עייף בבקרים.",
        &["ליאור", "אסנת"],
        &["עייף", "בבקרים"],
    ),
    // Names nobody declared.
    it(
        "undeclared",
        "בהפסקות הוא משחק בעיקר עם אביגיל ועם נחמן.",
        &["אביגיל", "נחמן"],
        &["בהפסקות", "משחק"],
    ),
    it(
        "undeclared",
        "האח הגדול, יהונתן, מתגייס בקרוב.",
        &["יהונתן"],
        &["מתגייס"],
    ),
    it(
        "undeclared",
        "ביקר אצל סבתא ציפורה בחופש.",
        &["ציפורה"],
        &["בחופש"],
    ),
    it(
        "undeclared",
        "החבר הקרוב שלו הוא מוחמד מהכיתה המקבילה.",
        &["מוחמד"],
        &["המקבילה"],
    ),
    it(
        "undeclared",
        "הקלינאית, אינה, המליצה על המשך טיפול.",
        &["אינה"],
        &["המליצה"],
    ),
    it(
        "undeclared",
        "הגננת הקודמת, ברכה, תיארה ילד שקט.",
        &["ברכה"],
        &["שקט"],
    ),
    it(
        "undeclared",
        "הוא מספר הרבה על בן הדוד אליאב.",
        &["אליאב"],
        &["מספר"],
    ),
    it(
        "undeclared",
        "המטפלת ברגשית, טליה, שלחה סיכום.",
        &["טליה"],
        &["סיכום"],
    ),
    it(
        "undeclared",
        "משחק עם דימה ועם ולדיסלב בחצר.",
        &["דימה", "ולדיסלב"],
        &["בחצר"],
    ),
    it(
        "undeclared",
        "האחות הקטנה, אייבי, נולדה בקיץ.",
        &["אייבי"],
        &["נולדה"],
    ),
    // Surnames.
    it(
        "surname",
        "האם, גלית אברג'יל, ציינה שהלידה הייתה תקינה.",
        &["אברג'יל"],
        &["תקינה"],
    ),
    it(
        "surname",
        "פגישה עם משפחת סויסה בבית.",
        &["סויסה"],
        &["פגישה"],
    ),
    it("surname", "ליאור ביטון הגיע בזמן.", &["ביטון"], &["בזמן"]),
    it(
        "surname",
        "ד\"ר קורנבליט המליץ על בדיקת שמיעה.",
        &["קורנבליט"],
        &["שמיעה"],
    ),
    // Places and institutions.
    it(
        "place",
        "המשפחה עברה מנתיבות לקריית גת לפני שנה.",
        &["נתיבות", "קריית גת"],
        &["המשפחה", "עברה"],
    ),
    it(
        "place",
        "הוא לומד בגן השקד שבשכונה.",
        &["השקד"],
        &["לומד", "שבשכונה"],
    ),
    it(
        "place",
        "בגן \"הרימונים\" יש שלוש סייעות.",
        &["הרימונים"],
        &["סייעות"],
    ),
    it(
        "place",
        "עבר לבית ספר \"אופקים חדשים\" בכיתה א'.",
        &["אופקים חדשים"],
        &["עבר", "בכיתה"],
    ),
    it(
        "place",
        "בבית הספר ע\"ש רבין יש כיתת תקשורת.",
        &["רבין"],
        &["תקשורת"],
    ),
    it(
        "place",
        "אושפז בשניידר בגיל שנתיים.",
        &["שניידר"],
        &["אושפז", "שנתיים"],
    ),
    it(
        "place",
        "גרים ברחוב הזית 12 קומה שלישית.",
        &["הזית 12"],
        &["קומה", "שלישית"],
    ),
    it(
        "place",
        "הילד לומד במעון \"פרפרים\".",
        &["פרפרים"],
        &["לומד"],
    ),
    // Numbers and contact details.
    it(
        "number",
        concat!("ת.ז. 000000018, טלפון 052-", "1234567."),
        &["000000018", "1234567"],
        &[],
    ),
    it(
        "number",
        "אפשר לפנות במייל fake.parent@example.com",
        &["fake.parent@example.com"],
        &["לפנות"],
    ),
    it(
        "number",
        concat!("טלפון בבית (03) ", "555-1234."),
        &["555-1234"],
        &["בבית"],
    ),
    it(
        "number",
        "מספר תיק בקופה 48213977.",
        &["48213977"],
        &["בקופה"],
    ),
    it(
        "number",
        "חשבון להחזר: IL62 0108 0000 0009 9999 999.",
        &["0108 0000 0009"],
        &["להחזר"],
    ),
    it(
        "number",
        "כרטיס 4580 1234 5678 9012 חויב.",
        &["4580 1234 5678 9012"],
        &["חויב"],
    ),
    it(
        "number",
        "נולד ב-3.4.2020 בבית חולים.",
        &["3.4.2020"],
        &["נולד"],
    ),
    it(
        "number",
        "נולד בי\"ג באדר תש\"ף.",
        &["אדר תש\"ף"],
        &["נולד"],
    ),
    // Clinical text that must pass as is (false alarms cost Einat a click each).
    it(
        "clean",
        "הוא עובד היטב כשהמשימה מובנית, ומתקשה כשיש הסחות.",
        &[],
        &["עובד", "מובנית", "הסחות"],
    ),
    it(
        "clean",
        "בשאלון הסתגלות ההורים דיווחו על שיפור.",
        &[],
        &["בשאלון", "הסתגלות", "שיפור"],
    ),
    it(
        "clean",
        "ציון 112, אחוזון 79 – ממוצע גבוה. זיכרון עבודה 95.",
        &[],
        &["112", "79", "95"],
    ),
    it(
        "clean",
        "בגן הוא משחק בעיקר לבד, ובבית הספר ישתלב בכיתה רגילה.",
        &[],
        &["משחק", "לבד", "ישתלב", "רגילה"],
    ),
    it(
        "clean",
        "בגן חובה יש 30 ילדים ושתי גננות.",
        &[],
        &["חובה", "ילדים"],
    ),
    it(
        "clean",
        "בבית הספר היסודי הוא ילמד בכיתה עם סייעת.",
        &[],
        &["היסודי", "סייעת"],
    ),
    it(
        "clean",
        "שמחה גדולה הייתה כשהצליח לבנות מגדל גבוה.",
        &[],
        &["שמחה", "מגדל", "גבוה"],
    ),
    it(
        "clean",
        "ההורים עובדים שניהם במשרה מלאה.",
        &[],
        &["עובדים", "במשרה"],
    ),
    it(
        "clean",
        "הגננת מנהלת את הקבוצה בעקביות.",
        &[],
        &["מנהלת", "בעקביות"],
    ),
    it(
        "clean",
        "הוא אוהב פאזלים, ספרים ומשחקי קופסה.",
        &[],
        &["פאזלים", "ספרים"],
    ),
    it(
        "clean",
        "הדיבור ברור, אוצר המילים רחב לגילו.",
        &[],
        &["הדיבור", "אוצר", "רחב"],
    ),
    it(
        "clean",
        "הומלץ על ריפוי בעיסוק פעמיים בשבוע.",
        &[],
        &["ריפוי", "בעיסוק"],
    ),
    it("clean", "בית הספר של אחיו רחוק מהבית.", &[], &["רחוק"]),
    it("clean", "הגן הקודם היה קטן יותר.", &[], &["הקודם", "קטן"]),
    it(
        "clean",
        "היא אינה מצליחה להתרכז לאורך זמן.",
        &[],
        &["אינה", "מצליחה"],
    ),
    it(
        "clean",
        "עלי להדגיש שהממצאים ראשוניים.",
        &[],
        &["עלי", "ראשוניים"],
    ),
    it("clean", "החיה האהובה עליו היא כלב.", &[], &["החיה", "כלב"]),
    it(
        "clean",
        "באופק נראה שיפור הדרגתי.",
        &[],
        &["באופק", "הדרגתי"],
    ),
    it(
        "clean",
        "ניסים קטנים של התקדמות נראו בכל מפגש.",
        &[],
        &["ניסים", "התקדמות"],
    ),
    it(
        "clean",
        "הגננת, בשיחה טלפונית, תיארה שיפור בוויסות.",
        &[],
        &["בשיחה", "טלפונית"],
    ),
    it(
        "clean",
        "האם, כמו האב, מודאגת מההתפרצויות.",
        &[],
        &["כמו", "מודאגת"],
    ),
    it(
        "clean",
        "המורה – לדבריה – רואה שינוי מאז החופש.",
        &[],
        &["לדבריה", "רואה"],
    ),
    it(
        "clean",
        "הסייעת (לעתים קרובות) יושבת לידו.",
        &[],
        &["לעתים", "קרובות"],
    ),
    it(
        "clean",
        "האח הגדול, בן 12, עוזר לו בשיעורים.",
        &[],
        &["עוזר", "בשיעורים"],
    ),
    it(
        "clean",
        "ד\"ר, כפי שנמסר, לא מצא ממצא נוירולוגי.",
        &[],
        &["נמסר", "נוירולוגי"],
    ),
    it(
        "clean",
        "בגן הוא מתקשה, ובבית הספר המורה מתארת ילד אחר.",
        &[],
        &["מתקשה", "מתארת"],
    ),
    it("clean", "בית הספר הזה מתאים לו יותר.", &[], &["מתאים"]),
    it("clean", "במעון היו שלוש מטפלות.", &[], &["מטפלות"]),
    it(
        "clean",
        "הומלץ על גן תקשורת או גן שפה.",
        &[],
        &["תקשורת", "שפה"],
    ),
    it(
        "clean",
        "המשפחה גרה בעיר גדולה במרכז הארץ.",
        &[],
        &["גדולה", "הארץ"],
    ),
    it(
        "clean",
        "סבתא מגיעה לאסוף אותו פעמיים בשבוע.",
        &[],
        &["מגיעה", "לאסוף"],
    ),
    it(
        "clean",
        "הוא מקסים ומלא הומור, אך נמנע ממגע.",
        &[],
        &["מקסים", "הומור"],
    ),
    it(
        "clean",
        "הכרים על הספה שימשו לבניית מבצר.",
        &[],
        &["הכרים", "מבצר"],
    ),
    it(
        "clean",
        "WISC-V, WPPSI-IV ו-Vineland נוספו לתיק.",
        &[],
        &["נוספו"],
    ),
    it(
        "clean",
        "ציונים: 112 104 106 95 84.",
        &[],
        &["112", "104", "106"],
    ),
    it(
        "clean",
        "הוא אוהב חיות, בעיקר כלבים וחתולים.",
        &[],
        &["חיות", "כלבים"],
    ),
    it(
        "clean",
        "בגן השנה יש שני ילדים חדשים.",
        &[],
        &["השנה", "חדשים"],
    ),
    it(
        "clean",
        "בית הספר היסודי הממלכתי קרוב לבית.",
        &[],
        &["היסודי", "הממלכתי"],
    ),
    // More to hide.
    it(
        "undeclared",
        "כשסבתא ציפורה מגיעה, הוא רגוע יותר.",
        &["ציפורה"],
        &["רגוע"],
    ),
    it(
        "undeclared",
        "המטפלת, ז'ניה, דיווחה על שיתוף פעולה.",
        &["ז'ניה"],
        &["דיווחה"],
    ),
    it(
        "undeclared",
        "החבר הכי טוב שלו הוא איתמר.",
        &["איתמר"],
        &["טוב"],
    ),
    it(
        "undeclared",
        "האחות הקטנה (אלמז) בת שלוש.",
        &["אלמז"],
        &["שלוש"],
    ),
    it(
        "surname",
        "פרופ' גרינבאום אבחן בגיל שלוש.",
        &["גרינבאום"],
        &["אבחן"],
    ),
    it(
        "surname",
        "הרב אלמוזנינו ביקש לצרף מכתב.",
        &["אלמוזנינו"],
        &["לצרף"],
    ),
    it(
        "place",
        "גרים בפסגת זאב, קרוב לסבים.",
        &["פסגת זאב"],
        &["קרוב"],
    ),
    it(
        "place",
        "למד בבית ספר ע\"ש הרצוג עד כיתה ב'.",
        &["הרצוג"],
        &["כיתה"],
    ),
    it(
        "number",
        "מספר רכב 12-345-67 נרשם בטופס.",
        &["12-345-67"],
        &["נרשם"],
    ),
    it("number", "מיקוד 9876543.", &["9876543"], &[]),
    // Stage 2 of the filter (D-042): names an ordinary word can hide, institutions, labels.
    it(
        "undeclared",
        "הילדים קוראים לה נונה, והיא גרה קרוב.",
        &["נונה"],
        &["קוראים", "קרוב"],
    ),
    it(
        "undeclared",
        "לקבל מהאם את הסיכום של אביבה-טגסט.",
        &["אביבה-טגסט"],
        &["הסיכום"],
    ),
    it(
        "undeclared",
        "בבושקה לודה מכינה לו בלינצ'יקי.",
        &["לודה"],
        &["מכינה", "בלינצ'יקי"],
    ),
    it(
        "undeclared",
        "בהפסקות היא משחקת בעיקר עם אלה.",
        &["אלה"],
        &["בהפסקות", "משחקת"],
    ),
    it(
        "place",
        "המשפחה עברה אחרי שריפה ברחוב התשבי.",
        &["התשבי"],
        &["שריפה"],
    ),
    it(
        "place",
        "מכון צמיחה – היחידה להתפתחות הילד",
        &["צמיחה"],
        &["היחידה", "להתפתחות"],
    ),
    it(
        "place",
        "טופלה במכון \"צעדים קטנים\" כשנה, ובסיכום מ\"צעדים קטנים\" צוין שיפור.",
        &["צעדים קטנים"],
        &["כשנה", "שיפור"],
    ),
    it(
        "place",
        "קופ\"ח מכבי, הפניה מרופאת הילדים.",
        &["מכבי"],
        &["הפניה"],
    ),
    it(
        "number",
        "מומחית ברפואת ילדים, מ.ר. 4471",
        &["4471"],
        &["מומחית"],
    ),
    // Ordinary words that only look like names, dates or institutions.
    it(
        "place",
        "בגן הוכנסו התאמות: פינה שקטה ואוזניות. הסבתא שרה לו שירים בערבית.",
        &[],
        &["הוכנסו", "שרה"],
    ),
    it(
        "number",
        "אחיו של האב אובחן בילדותו. לידה רגילה, אפגר 9/10. בקבוצה ילדים בני 4.5–6.5.",
        &[],
        &["האב", "9/10", "4.5", "6.5"],
    ),
    it(
        "undeclared",
        "הגננת תיווכה בין הילדים, ואמרתי שכן, עדיף.",
        &[],
        &["תיווכה", "עדיף"],
    ),
];

/// Recall per group (what must be hidden) and the false-alarm ceiling on clean text.
const RECALL_MIN: &[(&str, f64)] = &[
    ("declared", 1.0),
    ("number", 1.0),
    ("undeclared", 0.9),
    ("surname", 0.75),
    ("place", 0.85),
];
const FALSE_ALARM_MAX: f64 = 0.05;

#[test]
fn the_filter_meets_the_benchmark() {
    let ids = vec![
        id(Role::Child, "[ילד]", "ליאור"),
        id(Role::Mother, "[אם]", "גלית"),
        id(Role::Teacher, "[מורה]", "אסנת"),
    ];
    let practitioner: Vec<String> = Vec::new();
    let allow = |_: &str| false;
    let confirmed = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: CASE,
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &allow,
        confirmed_names: &confirmed,
        today: (2026, 9, 30),
    };

    let mut stats: std::collections::BTreeMap<&str, (u32, u32)> = Default::default();
    let (mut keeps, mut alarms) = (0u32, 0u32);
    let mut misses = Vec::new();
    for item in ITEMS {
        let out = filter(item.text, &ctx).unwrap();
        let tagged = normalize(&out.tagged);
        let held = |s: &str| {
            let n = normalize(s);
            out.suspects.iter().any(|x| {
                let t = normalize(&x.token);
                n.contains(&t) || t.contains(&n)
            })
        };
        for h in item.hide {
            let caught = !tagged.contains(&normalize(h)) || held(h);
            let e = stats.entry(item.group).or_default();
            e.1 += 1;
            if caught {
                e.0 += 1;
            } else {
                misses.push(format!("missed  [{}] {h}  ←  {}", item.group, item.text));
            }
        }
        for k in item.keep {
            keeps += 1;
            if !tagged.contains(&normalize(k)) || held(k) {
                alarms += 1;
                misses.push(format!("alarm   [{}] {k}  ←  {}", item.group, item.text));
            }
        }
    }

    println!("\nprivacy benchmark ({} sentences)", ITEMS.len());
    let mut failed = Vec::new();
    for (group, (caught, total)) in &stats {
        let recall = f64::from(*caught) / f64::from(*total);
        let min = RECALL_MIN
            .iter()
            .find(|(g, _)| g == group)
            .map_or(1.0, |(_, m)| *m);
        println!(
            "  {group:<11} recall {caught}/{total} = {:.0}%  (min {:.0}%)",
            recall * 100.0,
            min * 100.0
        );
        if recall < min {
            failed.push(format!("{group}: recall {recall:.2} < {min:.2}"));
        }
    }
    let rate = f64::from(alarms) / f64::from(keeps.max(1));
    println!(
        "  false alarms {alarms}/{keeps} = {:.1}%  (max {:.0}%)",
        rate * 100.0,
        FALSE_ALARM_MAX * 100.0
    );
    for m in &misses {
        println!("  {m}");
    }
    if rate > FALSE_ALARM_MAX {
        failed.push(format!("false alarms {rate:.3} > {FALSE_ALARM_MAX}"));
    }
    assert!(failed.is_empty(), "benchmark below the line: {failed:?}");
}
