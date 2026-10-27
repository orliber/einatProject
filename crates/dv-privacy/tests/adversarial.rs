//! Adversarial, canary and property tests. Merge condition: ZERO leaks.
//!
//! A leak = the gate cleared a request whose bytes still contain a protected string.
//! Blocking is never a leak (it costs the psychologist a click, not a child's privacy).
//! All data is fabricated (tests/fixtures/fake_case_noam.yaml, fake_case_metadata_leak.yaml).

#![allow(clippy::string_slice)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::indexing_slicing
)]

use std::collections::HashSet;

use dv_domain::{passage_ranges, Identity, Role};
use dv_privacy::text::{is_valid_prefix, normalize};
use dv_privacy::{
    clear, filter, filter_split, ocr_misreads, AutoKind, FilterOutcome, GateRequest, MaskBook,
    PrivacyContext,
};
use proptest::prelude::*;

const TODAY: (i32, u32, u32) = (2026, 9, 28);
const CASE: &str = "case-noam";
const OTHER: &str = "case-alon";
const CANARY: &str = "זרובבלית";

fn id(case: &str, role: Role, tag: &str, value: &str, aliases: &[&str]) -> Identity {
    Identity {
        id: format!("{case}-{tag}"),
        case_id: case.to_owned(),
        role,
        tag: tag.to_owned(),
        value: value.to_owned(),
        aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
        source: dv_domain::IdentitySource::Manual,
        reason: String::new(),
    }
}

fn identities() -> Vec<Identity> {
    vec![
        id(CASE, Role::Child, "[ילד]", "נועם", &["נועמי", "Noam"]),
        id(CASE, Role::Mother, "[אם]", "רותם", &[]),
        id(CASE, Role::Father, "[אב]", "עידו", &[]),
        id(CASE, Role::Sister, "[אחות_1]", "מאיה", &[]),
        id(CASE, Role::Teacher, "[גננת]", "מיכל", &[]),
        id(CASE, Role::Doctor, "[רופא_1]", "ד\"ר אבנר שטרן", &[]),
        id(CASE, Role::Slp, "[קלינאית_1]", "הילה", &[]),
        id(CASE, Role::Kindergarten, "[גן]", "גן הדקל", &[]),
        id(CASE, Role::Town, "[יישוב]", "כפר ורדים", &[]),
        // Another case in the same vault.
        id(OTHER, Role::Child, "[ילד]", "אלון", &[]),
        id(OTHER, Role::Teacher, "[גננת]", "שירה", &[]),
        id(OTHER, Role::Other, "[אדם_1]", "ברקוביץ יונתן", &[]),
    ]
}

/// Strings that must never leave the computer (normalized substring match – the strictest).
const PROTECTED: &[&str] = &[
    "נועם",
    "נועמי",
    "noam",
    "רותם",
    "עידו",
    "מאיה",
    "מיכל",
    "אבנר",
    "שטרן",
    "הילה",
    "הדקל",
    "ורדים",
    "000000018",
    "אלון",
    "ברקוביץ",
    "יונתן",
    "יובל",
    "דנה",
    "כהן-לוי",
    "גולדשטיין",
    "מזרחי",
    "שמעוני",
    "טננבאום",
    "ליבוביץ",
    "פרידמן",
    "שרעבי",
    CANARY,
];

struct Outcome {
    leaked: Vec<String>,
    cleared: bool,
}

fn ctx_for<'a>(
    ids: &'a [Identity],
    practitioner: &'a [String],
    case: &'a str,
) -> PrivacyContext<'a> {
    PrivacyContext {
        case_id: case,
        identities: ids,
        practitioner,
        allowlisted: &|_: &str| false,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    }
}

/// What dv-core keeps from a filter: every new name it hid becomes an identity of the case,
/// with the tag it was hidden under.
fn found(out: &FilterOutcome, known: &[Identity], case: &str) -> Vec<Identity> {
    let mut new: Vec<Identity> = Vec::new();
    for a in &out.auto_hidden {
        if !matches!(a.kind, AutoKind::Name | AutoKind::OtherCase) {
            continue;
        }
        if known.iter().any(|i| i.case_id == case && i.tag == a.tag) {
            continue;
        }
        if let Some(i) = new.iter_mut().find(|i| i.tag == a.tag) {
            if normalize(&i.value) != normalize(&a.token) && !i.aliases.contains(&a.token) {
                i.aliases.push(a.token.clone());
            }
            continue;
        }
        new.push(Identity {
            id: format!("{case}-auto-{}", a.tag),
            case_id: case.to_owned(),
            role: a.role,
            tag: a.tag.clone(),
            value: a.token.clone(),
            aliases: Vec::new(),
            source: dv_domain::IdentitySource::Auto,
            reason: a.reason.clone(),
        });
    }
    new
}

/// The whole path of an input: filter, keep the names it found, filter again with them (as
/// dv-core does), keep anything new, and send what the second pass produced.
///
/// Twice: with the fixed tags, and with the case's mask book, as dv-core sends (numbered tags,
/// round 25). Both must leak nothing and agree on whether the request goes out.
fn send(text: &str, case: &str) -> Outcome {
    let fixed = send_with(text, case, None);
    let book = MaskBook::new(Vec::new());
    let numbered = send_with(text, case, Some(&book));
    assert_eq!(
        fixed.cleared, numbered.cleared,
        "{text}: the mask book changed the verdict"
    );
    let mut leaked = fixed.leaked;
    leaked.extend(numbered.leaked);
    Outcome {
        leaked,
        cleared: fixed.cleared,
    }
}

fn send_with(text: &str, case: &str, book: Option<&MaskBook>) -> Outcome {
    let mut ids = identities();
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let with = |ids: &[Identity]| {
        let ctx = PrivacyContext {
            masks: book,
            ..ctx_for(ids, &practitioner, case)
        };
        filter(text, &ctx).unwrap()
    };
    let first = with(&ids);
    let more = found(&first, &ids, case);
    ids.extend(more);
    let second = with(&ids);
    let more = found(&second, &ids, case);
    ids.extend(more);
    gate_with(&second.tagged, &ids, case)
}

/// The gate's verdict on text that was already filtered, with the case's identities `ids`.
fn gate_with(tagged: &str, ids: &[Identity], case: &str) -> Outcome {
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let ctx = ctx_for(ids, &practitioner, case);
    let body = serde_json::json!({
        "model": "claude-opus-5",
        "system": "את עוזרת לכתיבת דוח אבחון. השתמשי בתגיות בדיוק כפי שהן.",
        "messages": [{"role": "user", "content": tagged}],
    });
    let case_tags: HashSet<String> = ids
        .iter()
        .filter(|i| i.case_id == case)
        .map(|i| i.tag.clone())
        .collect();
    let canaries = vec![CANARY.to_owned()];
    let req = GateRequest {
        body: &body,
        ctx: &ctx,
        case_tags: &case_tags,
        unresolved_suspects: 0,
        canaries: &canaries,
        max_bytes: 200_000,
    };
    match clear(&req) {
        Ok(payload) => {
            let sent = normalize(&String::from_utf8_lossy(payload.body()));
            let leaked = PROTECTED
                .iter()
                .filter(|p| sent.contains(&normalize(p)))
                .map(|p| (*p).to_owned())
                .collect();
            Outcome {
                leaked,
                cleared: true,
            }
        }
        Err(_) => Outcome {
            leaked: Vec::new(),
            cleared: false,
        },
    }
}

const PREFIXES: &[&str] = &[
    "", "ו", "ה", "ב", "ל", "מ", "ש", "כ", "וב", "ול", "ומ", "וש", "וכ", "שב", "של", "שמ", "כש",
    "וכש", "מה", "שה", "לכש", "ו-", "ב-", "ל-",
];

const TEMPLATES: &[&str] = &[
    "{} מתקשה במעברים בין פעילויות.",
    "הגננת סיפרה ש{} לא רצה לצאת לחצר.",
    "אמרתי {} שזה בסדר.",
    "\"{}\" אמרה הגננת בחיוך.",
    "({}) הגיע באיחור.",
    "{}, שוב ושוב, סירב לשתף.",
    "בשיחה עם {} עלה קושי.",
    "לדברי האם, {} אוהב דינוזאורים.",
    "…{}!",
    "הדוח של {}: תקין.",
];

/// Every declared name × every prefix × every context.
fn declared_corpus() -> Vec<String> {
    let names = [
        "נועם",
        "נועמי",
        "Noam",
        "NOAM",
        "noam",
        "נוֹעַם",
        "רותם",
        "עידו",
        "מאיה",
        "מיכל",
        "אבנר",
        "שטרן",
        "ד\"ר שטרן",
        "ד״ר אבנר שטרן",
        "הילה",
        "גן הדקל",
        "כפר ורדים",
        "נעם",
    ];
    let mut out = Vec::new();
    for name in names {
        let latin = name.chars().next().is_some_and(|c| c.is_ascii());
        for prefix in PREFIXES {
            let hyphen = prefix.ends_with('-');
            if latin && !prefix.is_empty() && !hyphen {
                continue;
            }
            for t in TEMPLATES {
                // Letters glued before the slot in the template ("ש{}") join the prefix; only
                // grammatical Hebrew prefix stacks are generated (see text::is_valid_prefix).
                let glued: String = t
                    .split("{}")
                    .next()
                    .unwrap_or("")
                    .chars()
                    .rev()
                    .take_while(|c| "והבלמשכ".contains(*c))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let combined = if hyphen {
                    glued.clone()
                } else {
                    format!("{glued}{prefix}")
                };
                if !combined.is_empty() && !is_valid_prefix(&combined) {
                    continue;
                }
                out.push(t.replace("{}", &format!("{prefix}{name}")));
            }
        }
    }
    out
}

#[test]
fn zero_leaks_for_declared_names_with_prefixes_spellings_and_scripts() {
    let corpus = declared_corpus();
    let mut leaks = Vec::new();
    for s in &corpus {
        let o = send(s, CASE);
        if !o.leaked.is_empty() {
            leaks.push(format!("{s} → {:?}", o.leaked));
        }
    }
    assert!(corpus.len() > 2000, "corpus too small: {}", corpus.len());
    assert!(
        leaks.is_empty(),
        "{} leaks out of {}:\n{}",
        leaks.len(),
        corpus.len(),
        leaks.join("\n")
    );
}

#[test]
fn zero_leaks_for_numbers_dates_and_contact_details() {
    // Phone and e-mail samples are assembled at runtime so the repository scanner never sees
    // a real-looking number or address in this file.
    let join = |parts: &[&str]| parts.concat();
    let owned = [
        "ת.ז. של הילד 000000018".to_owned(),
        "ת.ז. 000 000 018".to_owned(),
        "תעודת זהות 00000001-8".to_owned(),
        // Right after a period that is not a decimal point (D-126: was let through whole).
        "ת.ז.000000018".to_owned(),
        "הגיע עם האם.000000018 נבדק".to_owned(),
        join(&["נמסר.054", "7654321"]),
        join(&["טלפון האם 054-", "7654321"]),
        join(&["הטלפון 054", "7654321 או +972-54-", "765-4321"]),
        join(&["בבית: 03-", "1234567"]),
        join(&["מייל: ruthie.m", "@", "clinic.co.il"]),
        "ב-14.9 לא רצה לצאת לחצר".to_owned(),
        "ב14/9/2025 נבדק".to_owned(),
        "נבדק ב-3 במרץ 2025".to_owned(),
        "נולד בשנת 2020".to_owned(),
        "ה' בתשרי תשפ\"ו".to_owned(),
        "גרים ברחוב הדקלים 12".to_owned(),
        "עברו לקיבוץ גבעת ברנר".to_owned(),
        "אושפז בשניידר".to_owned(),
        "מספר תיק במכון 44-887766".to_owned(),
    ];
    let cases: Vec<&str> = owned.iter().map(String::as_str).collect();
    for s in cases {
        let o = send(s, CASE);
        assert!(o.leaked.is_empty(), "{s} leaked {:?}", o.leaked);
    }
}

#[test]
fn unknown_names_other_cases_and_near_misses_are_hidden_without_asking() {
    for s in [
        "ב-14.9 לא רצה לצאת לחצר כשיובל ביקש לשחק איתו.",
        "שיחק עם גל ועם טל בחצר.",
        "זוהר סיפרה שהוא מתקשה.",
        "עוד משתפת זוהר כי בקבוצה קטנה הוא מווסת יותר.",
        "אלון, הילד מהתיק השני, הוזכר.",
        "השם יונתן עלה בשיחה.",
        "נואם מתקשה", // near-miss spelling of the child's name
        "Yuval did not want to play.",
    ] {
        let ids = identities();
        let out = filter(s, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(!out.auto_hidden.is_empty(), "nothing hidden: {s}");
        let o = send(s, CASE);
        assert!(o.cleared, "should pass once hidden: {s}");
        assert!(o.leaked.is_empty(), "{s} leaked {:?}", o.leaked);
    }
}

#[test]
fn names_that_are_words_are_hidden_in_a_context_with_no_other_reading() {
    // D-049: a name that is also an everyday word ("שירה", "ים", "הודיה") is hidden only
    // where nothing else can be meant, and the same words stay words elsewhere.
    let ids = vec![
        id("c", Role::Child, "[ילד]", "ארטיום", &[]),
        id("c", Role::Teacher, "[גננת]", "סמאח", &[]),
    ];
    let ctx = ctx_for(&ids, &[], "c");
    for (s, name) in [
        ("הודיה מתקרבת ושואלת מה את מציירת.", "הודיה"),
        ("שירה אמרה לי השבוע שהוא תמיד מחליט לבד.", "שירה"),
        ("מרינה (המרפאה בעיסוק שלו) נתנה לנו מחזיק עיפרון.", "מרינה"),
        ("עם שירה ואביתר נוצר משחק משותף קצר.", "שירה"),
        ("בהפסקות משחק עם ים ומתן. היועצת מציינת.", "מתן"),
        ("בגן הוא משחק בעיקר עם מתן ועם ג'סיקה.", "מתן"),
        ("הגננות: סמאח ונוגה. טלפון של הגן.", "נוגה"),
        ("הגננת הקודמת שלו שולי שאלה עליו אתמול.", "שולי"),
        ("ביום ההולדת של ים התפוצץ בלון לידו.", " ים "),
        ("מר שמחה אוחיון, מנהל הלשכה", "אוחיון"),
        ("ארטיום נולד בחרקוב ועלה לארץ.", "חרקוב"),
    ] {
        let out = filter(s, &ctx).unwrap();
        assert!(!out.tagged.contains(name), "{name} leaked: {}", out.tagged);
    }
    for s in [
        "הוא אוהב שירה ומוזיקה. מתן הוראות כתובות. כל אדם שפוגש אותו.",
        "ישיבה קרובה למורה ומתן תזכורות שקטות. טיול לים. בשיעורים האלה הקשב טוב.",
        "בבית היא לא מפסיקה לדבר, שרה, מספרת סיפורים.",
        "נולדה בשבוע 35 ונולד ברכב בדרך לבית החולים.",
        "בשמחה רבה. תודה והודיה. ההורים אמרו שהוא שמח.",
    ] {
        let out = filter(s, &ctx).unwrap();
        assert_eq!(out.tagged, s, "an ordinary word was hidden");
    }
}

#[test]
fn ordinary_words_containing_a_name_are_confirmed_not_rewritten() {
    // A child called אלון, and the everyday word "שאלון" (questionnaire) = ש + אלון.
    let ids = vec![id("c", Role::Child, "[ילד]", "אלון", &[])];
    let never = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: "c",
        identities: &ids,
        practitioner: &[],
        allowlisted: &never,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let out = filter("במילוי השאלון ובשאלון ASRS, אלון ענה.", &ctx).unwrap();
    assert!(
        out.tagged.contains("השאלון") && out.tagged.contains("ובשאלון"),
        "{}",
        out.tagged
    );
    assert!(out.tagged.contains("[ילד] ענה"));

    // At the start of a sentence it may be the name: hidden, and listed so "להחזיר" can undo it.
    let bare = filter("שאלון ההורים הוחזר.", &ctx).unwrap();
    assert_eq!(bare.auto_hidden.len(), 1, "{:?}", bare.auto_hidden);
    assert_eq!(bare.auto_hidden[0].kind, AutoKind::DeclaredInWord);
    assert_eq!(bare.auto_hidden[0].token, "שאלון");
    assert!(bare.tagged.starts_with("ש[ילד]"), "{}", bare.tagged);

    let confirmed = |t: &str| t == normalize("שאלון");
    let ctx = PrivacyContext {
        case_id: "c",
        identities: &ids,
        practitioner: &[],
        allowlisted: &confirmed,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let again = filter("שאלון ההורים הוחזר.", &ctx).unwrap();
    assert!(again.auto_hidden.is_empty(), "{:?}", again.auto_hidden);
    assert!(again.tagged.starts_with("שאלון"), "{}", again.tagged);
    let body = serde_json::json!({"messages": [{"role": "user", "content": again.tagged}]});
    let tags: HashSet<String> = HashSet::from(["[ילד]".to_owned()]);
    let req = GateRequest {
        body: &body,
        ctx: &ctx,
        case_tags: &tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 10_000,
    };
    assert!(
        clear(&req).is_ok(),
        "confirmed ordinary word passes the gate"
    );

    // The other answer: here it is the name ("סיפרה שאלון" = that Alon).
    let is_name = |t: &str| t == normalize("שאלון");
    let ctx = PrivacyContext {
        case_id: "c",
        identities: &ids,
        practitioner: &[],
        allowlisted: &|_: &str| false,
        confirmed_names: &is_name,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let named = filter("הגננת סיפרה שאלון מתקשה במעברים.", &ctx).unwrap();
    assert!(named.auto_hidden.is_empty(), "{:?}", named.auto_hidden);
    assert!(named.tagged.contains("ש[ילד] מתקשה"), "{}", named.tagged);
}

#[test]
fn surnames_and_names_after_labels_are_hidden_without_asking() {
    // A declared first name is hidden; the family name next to it must not slip out.
    for text in [
        "שם הילד: נועם גולדשטיין, גיל 5:4.",
        "נועם מזרחי הגיע לגן בשמחה.",
        "רותם פרידמן-שרעבי סיפרה על הלילות.",
        "לכבוד משפחת שמעוני, מצורף הדוח.",
        "המטופלת: יעלה טננבאום",
        "בשיחה עם מיכל ליבוביץ, הגננת, עלה קושי במעברים.",
        "שם הילד: אלון ברקוביץ, גיל 5:4. הופנה על ידי רופאת הילדים.",
    ] {
        let o = send(text, CASE);
        assert!(
            !o.leaked.iter().any(|l| !l.is_empty()),
            "{text}: leaked {:?}",
            o.leaked
        );
        assert!(o.cleared, "{text}: should pass once hidden");
    }
}

#[test]
fn ordinary_sentences_around_names_are_not_flagged() {
    for text in [
        "נועם הגיע לגן בשמחה ושיחק עם חברים.",
        "מיכל סיפרה שהוא נרגע מהר יותר השבוע.",
        "שם המבחן: WPPSI-IV, גרסה עברית.",
        "הילד נבדק בגיל 5:4 והתקבל ציון 102.",
    ] {
        let o = send(text, CASE);
        assert!(
            o.cleared,
            "{text}: should pass once declared names are hidden"
        );
    }
}

/// Family names that are also everyday words ("שני", "גיל", "אלה", "מתן", "אור"): the word
/// passes only where the grammar rules out a name; everywhere else the name is hidden.
#[test]
fn declared_names_that_are_words_pass_only_as_words() {
    let case = "case-dual";
    let ids = vec![
        id(case, Role::Mother, "[אם]", "שני", &[]),
        id(case, Role::Father, "[אב]", "גיל", &[]),
        id(case, Role::Sister, "[אחות_1]", "אלה", &[]),
        id(case, Role::Brother, "[אח_1]", "מתן", &[]),
        id(case, Role::Sister, "[אחות_2]", "אור", &[]),
        id(case, Role::Brother, "[אח_2]", "ציון", &[]),
        id(case, Role::Sister, "[אחות_3]", "עדינה", &[]),
        id(case, Role::Other, "[אחר]", "פרידה", &[]),
        id(case, Role::Sister, "[אחות_4]", "בילי", &[]),
    ];
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let allow = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: case,
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &allow,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let case_tags: HashSet<String> = ids.iter().map(|i| i.tag.clone()).collect();
    // What the gate sends (normalized), or None when it blocks.
    let sent = |text: &str| {
        let out = filter(text, &ctx).unwrap();
        let body = serde_json::json!({
            "model": "claude-opus-5",
            "messages": [{"role": "user", "content": out.tagged}],
        });
        let req = GateRequest {
            body: &body,
            ctx: &ctx,
            case_tags: &case_tags,
            unresolved_suspects: 0,
            canaries: &[],
            max_bytes: 200_000,
        };
        clear(&req)
            .ok()
            .map(|p| normalize(&String::from_utf8_lossy(p.body())))
    };
    for (text, name) in [
        ("שני הגיעה לפגישה עם גיל.", "שני"),
        ("שני הבינה מהר.", "שני"),
        ("ילדים במשפחה: גיל 5, שני 3.", "גיל"),
        ("פגע בגיל ובאלה.", "גיל"),
        ("אלה לפעמים כועסת.", "אלה"),
        ("שני לעיתים קרובות מרגישה עייפה.", "שני"),
        ("מתן לקח לו את הכדור.", "מתן"),
        ("אור הקטנה ישנה בחדר של ההורים.", "אור"),
        ("בוקר טוב, אור.", "אור"),
        ("הוא כתב מכתב לאור הקטנה.", "אור"),
        ("ציון הגיע לפגישה עם אמא.", "ציון"),
        ("ציון גבוה מאחיו.", "ציון"),
        ("ציון מסתגל לגן החדש.", "ציון"),
        ("עדינה ישבה ליד השולחן.", "עדינה"),
        ("פרידה מהגן הגיעה לבקר.", "פרידה"),
        ("בילי שיחקה בחצר.", "בילי"),
    ] {
        if let Some(body) = sent(text) {
            let words: Vec<&str> = body.split(|c: char| !c.is_alphanumeric()).collect();
            assert!(
                !words.contains(&normalize(name).as_str()),
                "{text}: {name} left as a name"
            );
        }
    }
    for text in [
        "שני ההורים הגיעו לפגישה.",
        "אלה הדברים שהכי מעניינים אותו.",
        "ישב בגיל 7 חודשים, הלך בגיל שנה.",
        "ביחס לגיל הכרונולוגי ולגיל שלו.",
        "זקוק למתן זמן נוסף.",
        "ישן רק עם אור דולק.",
        "ולאור זאת ההמלצה עומדת בעינה.",
        "ציון 112 בסולם המילולי, ציון T של 64.",
        "ציון כולל: 31. ציון מסתגל כללי בטווח הממוצע.",
        "קשיים במוטוריקה עדינה.",
        "חרדת פרידה בבוקר, פרידה מההורה בכניסה לחדר.",
        "הגיע בלי התיק ושאל שאלות כלליות.",
    ] {
        assert!(
            sent(text).is_some(),
            "over-blocking an ordinary word: {text}"
        );
    }
}

/// A name declared in Hebrew and written in Arabic in a bilingual document ("روضة جسر"),
/// with vowel signs, the article or "and" glued on, never leaves as is.
#[test]
fn declared_names_in_arabic_script_never_leak() {
    let case = "case-arabic";
    let ids = vec![
        id(case, Role::Child, "[ילד]", "סמאח", &[]),
        id(case, Role::Father, "[אב]", "ג'מיל עבאסי", &[]),
        id(case, Role::Kindergarten, "[גן]", "ג'סר", &[]),
    ];
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let allow = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: case,
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &allow,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let case_tags: HashSet<String> = ids.iter().map(|i| i.tag.clone()).collect();
    let gate = |tagged: &str| {
        let body = serde_json::json!({
            "model": "claude-opus-5",
            "messages": [{"role": "user", "content": tagged}],
        });
        let req = GateRequest {
            body: &body,
            ctx: &ctx,
            case_tags: &case_tags,
            unresolved_suspects: 0,
            canaries: &[],
            max_bytes: 200_000,
        };
        clear(&req)
            .ok()
            .map(|p| normalize(&String::from_utf8_lossy(p.body())))
    };
    let protected = ["جسر", "سماح", "جميل", "عباسي"].map(normalize);
    for text in [
        "גן דו-לשוני \"ג'סר\" | روضة جسر",
        "الطفلة سَمَاح تلعب في الحديقة.",
        "والجسر قريب من البيت، وجميل عباسي يحضر كل يوم.",
        "رسالة من الأب: جميل عبّاسي",
    ] {
        let out = filter(text, &ctx).unwrap();
        if let Some(sent) = gate(&out.tagged) {
            for p in &protected {
                assert!(!sent.contains(p.as_str()), "{text}: {p} left in {sent}");
            }
        }
        // The gate alone refuses the raw text too.
        assert!(gate(text).is_none(), "gate let through: {text}");
    }
    let plain = "الطفل يلعب في الحديقة مع أصدقائه.";
    let out = filter(plain, &ctx).unwrap();
    assert!(
        gate(&out.tagged).is_some(),
        "over-blocking Arabic text without names"
    );
}

#[test]
fn canaries_always_block() {
    let o = send(&format!("{CANARY} היא מחרוזת מלכודת"), CASE);
    assert!(!o.cleared);
}

#[test]
fn clinical_text_without_identifiers_is_not_blocked() {
    for s in [
        "הילד מגיב היטב לתיווך ומפגין נחישות בביצוע משימות ביצועיות.",
        "במבחן WPPSI הציג תפקוד ממוצע (9) ובמטריצות תפקוד מעל הממוצע (13).",
        "ציון T כללי של 83 בשאלון ASRS, כאשר ציון של 60 ומעלה מעלה חשד.",
        "בגיל 2.5 החל טיפול קלינאית תקשורת.",
        "ילד מלא מרץ ושמחת חיים, מתקשה בהמתנה לתור.",
        "ציונו ב-ADOS-2 הינו 13, כאשר נקודת החתך היא 9.",
        "מתקשה במעברים; עם זאת, כשמכינים אותו מראש עם לוח תמונות הוא מסתדר.",
    ] {
        let o = send(s, CASE);
        assert!(o.cleared, "over-blocking clinical text: {s}");
    }
}

#[test]
fn tags_keep_prefixes_and_restore_puts_names_back() {
    let ids = identities();
    let allow = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: CASE,
        identities: &ids,
        practitioner: &[],
        allowlisted: &allow,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let out = filter("מיכל סיפרה שנועם עבר לכפר ורדים עם ד\"ר שטרן", &ctx).unwrap();
    // The title before part of a titled identity goes into its tag (D-131), so the name
    // comes back once.
    assert_eq!(out.tagged, "[גננת] סיפרה ש[ילד] עבר ל[יישוב] עם [רופא_1]");
    assert!(out.hidden.iter().any(|h| h.contains("הילד")));
    let case_ids: Vec<Identity> = ids.into_iter().filter(|i| i.case_id == CASE).collect();
    let back = dv_privacy::restore::restore(&out.tagged, &case_ids, None, &[]);
    assert_eq!(back, "מיכל סיפרה שנועם עבר לכפר ורדים עם ד\"ר אבנר שטרן");
    assert!(dv_privacy::restore::remaining_tags(&back).is_empty());
}

#[test]
fn a_tag_from_another_case_is_blocked_by_the_gate() {
    let ids = identities();
    let allow = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: CASE,
        identities: &ids,
        practitioner: &[],
        allowlisted: &allow,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let body = serde_json::json!({"messages": [{"role": "user", "content": "[אדם_1] הגיע"}]});
    let tags: HashSet<String> = ids
        .iter()
        .filter(|i| i.case_id == CASE)
        .map(|i| i.tag.clone())
        .collect();
    let req = GateRequest {
        body: &body,
        ctx: &ctx,
        case_tags: &tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 10_000,
    };
    assert!(clear(&req).is_err());
}

#[test]
fn the_metadata_leak_pattern_is_caught_once_metadata_names_are_declared() {
    // fake_case_metadata_leak.yaml: the Title property held another full name, the teacher's
    // name appeared once mid-paragraph. Ingest proposes metadata names as identities.
    let ids = vec![
        id("case-2", Role::Child, "[ילד]", "אלון", &[]),
        id("case-2", Role::Teacher, "[גננת]", "שירה", &[]),
        id("case-2", Role::Other, "[אדם_1]", "ברקוביץ יונתן", &[]),
    ];
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let allow = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: "case-2",
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &allow,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    let text =
        "אלון, בן 4:9, הופנה לאבחון ביוזמת הגננת. עוד משתפת שירה כי בקבוצה קטנה אלון מווסת יותר. \
                סיכום הערכה – ברקוביץ יונתן. כתבה: דנה כהן-לוי.";
    let out = filter(text, &ctx).unwrap();
    for leaked in ["אלון", "שירה", "ברקוביץ", "יונתן", "דנה", "כהנ"] {
        assert!(
            !normalize(&out.tagged).contains(leaked),
            "{leaked} in {}",
            out.tagged
        );
    }
    assert!(out.auto_hidden.is_empty(), "{:?}", out.auto_hidden);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    /// Random prefix + declared name + random clinical filler: must be replaced or blocked.
    #[test]
    fn random_contexts_never_leak(
        prefix in prop::sample::select(PREFIXES.to_vec()),
        name in prop::sample::select(vec!["נועם", "נועמי", "רותם", "עידו", "מאיה", "מיכל", "שטרן", "הילה"]),
        before in prop::collection::vec(prop::sample::select(vec!["הילד", "מתקשה", "בגן", "היום", "אמר", "עם", "ו", "כי", ",", "."]), 0..5),
        after in prop::collection::vec(prop::sample::select(vec!["סיפרה", "בחצר", "שוב", "מאוד", "!", "?", "(9)", "בגיל 3"]), 0..5),
    ) {
        let text = format!("{} {prefix}{name} {}", before.join(" "), after.join(" "));
        let o = send(&text, CASE);
        prop_assert!(o.leaked.is_empty(), "{text} leaked {:?}", o.leaked);
    }
}

fn noam_ctx<'a>(ids: &'a [Identity], practitioner: &'a [String]) -> PrivacyContext<'a> {
    PrivacyContext {
        case_id: CASE,
        identities: ids,
        practitioner,
        allowlisted: &|_: &str| false,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    }
}

/// D-022: a material is filtered once, whole, and cut into passages. Each passage must be
/// exactly the whole result cut at its edges, leak nothing when sent alone, and list every
/// item the whole text hid on its own.
#[test]
fn passages_cut_from_a_whole_filter_leak_nothing_and_keep_every_hidden_item() {
    let ids = identities();
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let ctx = noam_ctx(&ids, &practitioner);
    let mut lines = declared_corpus();
    lines.extend(
        [
            "שם הילד: נועם גולדשטיין, גיל 5:4.",
            "בשיחה עם מיכל ליבוביץ, הגננת, עלה קושי במעברים.",
            "בגן משחק בעיקר עם יובל",
            "ובחצר עם דנה.",
            "ת\"ז 000000018 הופיעה בטופס.",
            "נפגשנו ב-12.3.2026 ושוב אחרי שבועיים.",
            "רקע:",
            "הלך בגיל שנה, ללא סיבוכים.",
        ]
        .map(str::to_owned),
    );
    // Built at run time so the pre-commit scan does not flag a phone number in the source.
    lines.push(["טלפון האם: 052-", "1234567."].concat());
    // Paragraphs of different shapes: blank lines, wrapped lines, headings.
    let doc: String = lines
        .chunks(3)
        .map(|c| c.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let ranges = passage_ranges(&doc);
    assert!(ranges.len() > 100, "the corpus should make many passages");
    let (whole, parts) = filter_split(&doc, &ctx, &ranges).unwrap();
    let parts = parts.expect("line-based passages never cut a name");
    assert_eq!(parts.len(), ranges.len());

    // Exactly the whole outcome, cut at the edges.
    let mut rebuilt = String::new();
    let mut pos = 0;
    for (r, part) in ranges.iter().zip(&parts) {
        rebuilt.push_str(&doc[pos..r.start]);
        rebuilt.push_str(&part.tagged);
        pos = r.end;
    }
    rebuilt.push_str(&doc[pos..]);
    assert_eq!(rebuilt, whole.tagged);

    // Every item the whole text hid is listed in a passage that holds it.
    for a in &whole.auto_hidden {
        assert!(
            parts.iter().any(|p| p.auto_hidden.contains(a)),
            "hidden item lost when cutting: {}",
            a.token
        );
    }

    // Sent alone, with the names it found kept, no passage leaks.
    let mut kept = ids.clone();
    let more = found(&whole, &ids, CASE);
    kept.extend(more);
    let mut leaks = Vec::new();
    for part in &parts {
        let o = gate_with(&part.tagged, &kept, CASE);
        if o.cleared && !o.leaked.is_empty() {
            leaks.push(format!("{} -> {:?}", part.tagged, o.leaked));
        }
    }
    assert!(
        leaks.is_empty(),
        "{} leaks: {:#?}",
        leaks.len(),
        &leaks[..leaks.len().min(5)]
    );
}

#[test]
fn a_range_that_cuts_through_a_name_refuses_to_split() {
    let ids = identities();
    let practitioner = Vec::new();
    let ctx = noam_ctx(&ids, &practitioner);
    let text = "נועם הגיע לגן.";
    // "נו" | "עם הגיע לגן." – the edge falls inside the child's name.
    let (whole, parts) = filter_split(text, &ctx, &[0..4, 4..text.len()]).unwrap();
    assert!(parts.is_none(), "must fall back to the whole text");
    assert!(!whole.tagged.contains("נועם"));
    // A range that is not on a character edge is refused too, never sliced.
    let inside_a_letter = std::iter::once(0..1).collect::<Vec<_>>();
    let (_, parts) = filter_split(text, &ctx, &inside_a_letter).unwrap();
    assert!(parts.is_none());
}

/// Drafts are stored tagged and filtered again before each request. An institution word inside
/// a tag ("גן" in `[ילד_גן]`, `[גן]`) is not followed by a kindergarten's name: the next word
/// stays. (It used to be dropped: "[ילד_גן] הופנה לאבחון" went out as "[ילד_גן לאבחון".)
#[test]
fn filtering_tagged_text_again_keeps_every_word() {
    let mut ids = identities();
    ids.push(id(CASE, Role::OtherChild, "[ילד_גן]", "דנה", &[]));
    ids.push(id(CASE, Role::School, "[בית_ספר]", "בית ספר אופק", &[]));
    let practitioner: Vec<String> = Vec::new();
    let ctx = PrivacyContext {
        case_id: CASE,
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &|_: &str| false,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
        masks: None,
    };
    for tagged in [
        "[ילד_גן] הופנה לאבחון.",
        "[ילד] משחק עם [ילד_גן] בחצר.",
        "ב[גן] שלו יש שגרה קבועה.",
        "ב[בית_ספר] החדש הוא משתלב.",
    ] {
        let again = filter(tagged, &ctx).unwrap();
        assert_eq!(again.tagged, tagged);
        assert!(
            again.auto_hidden.is_empty(),
            "{tagged}: {:?}",
            again.auto_hidden
        );
    }
    // A kindergarten's name never takes part of a tag after it: "גן_1] מרץ" was read as
    // the first of March, and the gate refused it (D-130).
    for tagged in ["גן ע\"ש [טלפון_1] מרץ", "בגן של [מספר_זהות_1] מרץ"]
    {
        let again = filter(tagged, &ctx).unwrap();
        let tag = &tagged[tagged.find('[').unwrap()..=tagged.find(']').unwrap()];
        assert!(again.tagged.contains(tag), "{tagged} → {}", again.tagged);
        assert!(
            gate_with(&again.tagged, &ids, CASE).cleared,
            "{tagged} → {}",
            again.tagged
        );
    }
}

/// Stage 3: a new name is hidden under a tag that says who it is, keeps that tag in every
/// later text of the case once kept, and a job is generalized, never asked about.
#[test]
fn found_names_get_a_role_tag_and_keep_it_in_later_texts() {
    let mut ids = identities();
    let practitioner: Vec<String> = Vec::new();
    let text = "הגננת רינת סיפרה שנועם משחק עם סבתא שמחה. האם עובדת כמורה בבית ספר יסודי.";
    let out = filter(text, &ctx_for(&ids, &practitioner, CASE)).unwrap();
    let tag_of = |out: &FilterOutcome, token: &str| {
        out.auto_hidden
            .iter()
            .find(|a| a.token == token)
            .map(|a| (a.tag.clone(), a.role))
    };
    assert_eq!(
        tag_of(&out, "רינת"),
        Some(("[גננת_2]".to_owned(), Role::Teacher)),
        "{:?}",
        out.auto_hidden
    );
    assert_eq!(
        tag_of(&out, "שמחה"),
        Some(("[קרוב_משפחה_1]".to_owned(), Role::Relative))
    );
    assert!(
        out.tagged.ends_with("האם עובדת בתחום החינוך."),
        "{}",
        out.tagged
    );
    assert!(!out.tagged.contains("רינת") && !out.tagged.contains("שמחה"));
    let more = found(&out, &ids, CASE);
    ids.extend(more);

    // A later text: the kept name has the same tag, and is listed again for "להחזיר".
    let later = filter("רינת התקשרה שוב.", &ctx_for(&ids, &practitioner, CASE)).unwrap();
    assert!(later.tagged.starts_with("[גננת_2]"), "{}", later.tagged);
    assert_eq!(later.auto_hidden.len(), 1, "{:?}", later.auto_hidden);

    // A kept name that is also a word hides only where a name fits: "בשמחה" stays.
    let word = filter(
        "הגיע לגן בשמחה, וסבתא שמחה אספה אותו.",
        &ctx_for(&ids, &practitioner, CASE),
    )
    .unwrap();
    assert!(word.tagged.contains("בשמחה"), "{}", word.tagged);
    assert!(
        word.tagged.contains("סבתא [קרוב_משפחה_1]"),
        "{}",
        word.tagged
    );
}

#[test]
fn names_from_past_reports_are_hidden_and_refused_by_the_gate() {
    // A fabricated name the lexicon does not know, known only from a past report.
    let past = |t: &str| t == normalize("תמוזי") || t == normalize("גיל");
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let ids = vec![id("c1", Role::Child, "[ילד]", "אלון", &[])];
    let ctx = PrivacyContext {
        past_names: &past,
        ..ctx_for(&ids, &practitioner, "c1")
    };
    for text in ["אלון סיפר שתמוזי בא לבקר.", "הוא נתן את הכדור ולתמוזי."]
    {
        let out = filter(text, &ctx).unwrap();
        assert!(!out.tagged.contains("תמוזי"), "{}", out.tagged);
        assert!(
            out.auto_hidden.iter().any(|a| a.reason == "שם מדוח ישן"),
            "{:?}",
            out.auto_hidden
        );
    }
    // An everyday word is never a past name: "בגיל 3" stays.
    let out = filter("בגיל 3 הוא הלך.", &ctx).unwrap();
    assert!(out.tagged.contains("בגיל 3"), "{}", out.tagged);

    // The gate refuses one that reaches it unhidden.
    let body = serde_json::json!({"messages": [{"role": "user", "content": "תמוזי בא לבקר."}]});
    let case_tags: HashSet<String> = ["[ילד]".to_owned()].into();
    let req = GateRequest {
        body: &body,
        ctx: &ctx,
        case_tags: &case_tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 200_000,
    };
    let blocked = clear(&req).unwrap_err();
    assert!(
        blocked.reasons.iter().any(|r| r.code == "past_report_name"),
        "{:?}",
        blocked.reasons
    );
}

#[test]
fn ocr_misreads_of_a_declared_name_are_found_one_letter_away() {
    // Fabricated scan text: OCR read "אלון" as "אלוו" (ו for ן) and "אלוז".
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let ids = vec![id("c1", Role::Child, "[ילד]", "אלון", &[])];
    let ctx = ctx_for(&ids, &practitioner, "c1");
    let scan = "ואלוו הגיע לגן. אלוז שיחק בחול. אלון צייר. האלוף ניצח.";
    let found: Vec<(String, String)> = ocr_misreads(scan, &ctx)
        .into_iter()
        .map(|m| (m.written, m.tag))
        .collect();
    assert_eq!(
        found,
        vec![
            ("אלוו".to_owned(), "[ילד]".to_owned()),
            ("אלוז".to_owned(), "[ילד]".to_owned())
        ]
    );
    // Typed text keeps the stricter near-spelling rule: ז is not a weak letter.
    assert!(filter(scan, &ctx).unwrap().tagged.contains("אלוז"));

    // Kept as spellings of the name, they are hidden everywhere and refused by the gate.
    let ids = vec![id("c1", Role::Child, "[ילד]", "אלון", &["אלוו", "אלוז"])];
    let ctx = ctx_for(&ids, &practitioner, "c1");
    let out = filter(scan, &ctx).unwrap();
    assert!(
        !out.tagged.contains("אלוו") && !out.tagged.contains("אלוז"),
        "{}",
        out.tagged
    );
    assert!(out.tagged.contains("האלוף"), "{}", out.tagged);
    assert!(!gate_with("אלוז שיחק בחול.", &ids, "c1").cleared);
}

/// Names of other cases that are also everyday words (D-050): "גיל", "שירה", "אור", "שני".
fn word_name_identities() -> Vec<Identity> {
    let mut ids = identities();
    ids.extend([
        id("case-gil", Role::Child, "[ילד]", "גיל", &[]),
        id("case-gil", Role::Mother, "[אם]", "אור", &[]),
        id("case-gil", Role::Sister, "[אחות_1]", "שני", &[]),
        id("case-gil", Role::Teacher, "[גננת]", "נוגה", &[]),
        id("case-gil", Role::Other, "[אדם_1]", "עין", &[]),
    ]);
    ids
}

/// The app's flow on `text` with "not a name" answers `allow`: two filters keeping what they
/// found, then the gate on what the second let out. `None`: the gate refused it.
fn flow_with(text: &str, base: &[Identity], case: &str, allow: &[&str]) -> Option<String> {
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let allow: HashSet<String> = allow.iter().map(|a| normalize(a)).collect();
    let allowed = |t: &str| allow.contains(t);
    let mut ids = base.to_vec();
    let mut tagged = String::new();
    for _ in 0..2 {
        let ctx = PrivacyContext {
            allowlisted: &allowed,
            ..ctx_for(&ids, &practitioner, case)
        };
        let out = filter(text, &ctx).unwrap();
        let more = found(&out, &ids, case);
        ids.extend(more);
        tagged = out.tagged;
    }
    let ctx = PrivacyContext {
        allowlisted: &allowed,
        ..ctx_for(&ids, &practitioner, case)
    };
    let body = serde_json::json!({"messages": [{"role": "user", "content": tagged}]});
    let case_tags: HashSet<String> = ids
        .iter()
        .filter(|i| i.case_id == case)
        .map(|i| i.tag.clone())
        .collect();
    clear(&GateRequest {
        body: &body,
        ctx: &ctx,
        case_tags: &case_tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 200_000,
    })
    .ok()
    .map(|_| tagged)
}

/// Another case's name is hidden in a name's place, also where only the comma and a person
/// word after it say so, and stays the word it is elsewhere ("בגיל 3", "משימות אלה").
#[test]
fn another_cases_word_name_is_read_by_its_place() {
    let ids = word_name_identities();
    let hidden = [
        "אלון, הילד מהתיק השני, הוזכר.",
        "נוגה, הגננת, סיפרה שהוא שמח.",
        "גיל הגיע לגן באיחור.",
        "שיחקתי עם שני.",
        "אחותו אור עזרה לו.",
    ];
    for text in hidden {
        let tagged =
            flow_with(text, &ids, CASE, &[]).unwrap_or_else(|| panic!("dead end on «{text}»"));
        let name = ["אלון", "נוגה", "גיל", "שני", "אור"]
            .into_iter()
            .find(|n| text.contains(n))
            .unwrap();
        assert!(!tagged.contains(name), "«{text}» let out {name}: {tagged}");
    }
    let words = [
        ("הוא אוהב לשחק עם ילדים בגיל 3.", "בגיל"),
        ("משימות אלה חיזקו אותו.", "אלה"),
        ("הוא ישן רק לאור מנורה.", "לאור"),
    ];
    for (text, word) in words {
        let tagged =
            flow_with(text, &ids, CASE, &[]).unwrap_or_else(|| panic!("dead end on «{text}»"));
        assert!(tagged.contains(word), "«{text}» hid {word}: {tagged}");
    }
}

/// "להחזיר" on another case's name in this case: the word goes out from then on wherever it
/// is not in a name's place, and is still hidden where it is (it used to come back hidden
/// on the next send, so the button did nothing).
#[test]
fn restoring_another_cases_name_frees_the_word_only_outside_a_names_place() {
    let ids = word_name_identities();
    let free = flow_with("נועם אוהב שירה ומוזיקה.", &ids, CASE, &["שירה"]).unwrap();
    assert!(free.contains("שירה ומוזיקה"), "{free}");
    let named = flow_with("שירה סיפרה שנועם שמח.", &ids, CASE, &["שירה"]).unwrap();
    assert!(!named.contains("שירה"), "{named}");
    // Only in this case: elsewhere the same text still hides it.
    let other = flow_with("הוא אוהב שירה ומוזיקה.", &ids, "case-gil", &[]).unwrap();
    assert!(!other.contains("שירה"), "{other}");
}

/// Context that changes once names become tags ("נועם גיל" → "[ילד] גיל") never ends in a
/// refusal: the filter hides whatever the gate would refuse in its own output (D-050).
#[test]
fn a_name_next_to_a_tag_is_never_a_dead_end() {
    let ids = word_name_identities();
    for text in [
        // Found by the property test below with the fixed point turned off.
        "אבא וגיל נועם",
        "נועם גיל ונוגה שיחקו.",
        "נועם, גיל, שני ואור.",
        "אחות של נועם, נוגה עזרה.",
        "גיל נועם: 5 שנים.",
        "הגננת מיכל ושני הבנים.",
        "עם נועם ואור.",
    ] {
        assert!(
            flow_with(text, &ids, CASE, &[]).is_some(),
            "dead end on «{text}»"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    /// D-050: whatever the filter lets out, the gate clears. Random sentences of names (this
    /// case, other cases, the lexicon, words that are names), person words, verbs and marks.
    #[test]
    fn the_gate_never_refuses_what_the_filter_let_out(
        words in prop::collection::vec(prop::sample::select(vec![
            "נועם", "רותם", "מיכל", "גיל", "שירה", "אור", "שני", "נוגה", "עין", "אלה", "אלון",
            "יובל", "דנה", "בן", "חן", "הילד", "אחות", "אחותו", "הגננת", "אבא", "אמא", "סבא",
            "סיפרה", "הגיע", "עובד", "אוהב", "קשר", "ומוזיקה", "בגן", "הבנים", "מאוד",
            ",", ".", ":", "(", ")", "עם", "של", "את",
        ]), 1..10),
        prefixes in prop::collection::vec(prop::sample::select(vec!["", "", "", "ו", "ה", "ב", "ל", "ש", "מ"]), 10),
        allow in prop::sample::select(vec![vec![], vec!["שירה"], vec!["גיל", "אור"]]),
    ) {
        let text: Vec<String> = words
            .iter()
            .zip(&prefixes)
            .map(|(w, p)| if w.chars().all(char::is_alphabetic) { format!("{p}{w}") } else { (*w).to_owned() })
            .collect();
        let text = text.join(" ");
        let ids = word_name_identities();
        prop_assert!(flow_with(&text, &ids, CASE, &allow).is_some(), "dead end on «{text}»");
    }
}

/// A declared name typed with a space between its letters ("נ ו ע ם", "N o a m") is hidden
/// like the name, by the filter and by the gate.
#[test]
fn zero_leaks_for_names_typed_with_spaced_letters() {
    let spaced = |name: &str, gap: &str| {
        name.chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(gap)
    };
    let mut leaks = Vec::new();
    for name in ["נועם", "רותם", "מאיה", "Noam", "שטרן"] {
        for gap in [" ", "  ", "\u{00A0}"] {
            let typed = spaced(name, gap);
            for t in TEMPLATES {
                let text = t.replace("{}", &typed);
                let mut ids = identities();
                let practitioner = vec!["דנה כהן-לוי".to_owned()];
                let out = filter(&text, &ctx_for(&ids, &practitioner, CASE)).unwrap();
                ids.extend(found(&out, &ids, CASE));
                let joined: String = out.tagged.chars().filter(|c| !c.is_whitespace()).collect();
                if out.tagged.contains(&typed) || normalize(&joined).contains(&normalize(name)) {
                    leaks.push(format!("filter: {text} → {}", out.tagged));
                }
                // The gate on the raw text (a path that skipped the filter) refuses it.
                if gate_with(&text, &ids, CASE).cleared {
                    leaks.push(format!("gate: {text}"));
                }
            }
        }
    }
    assert!(
        leaks.is_empty(),
        "{} leaks:\n{}",
        leaks.len(),
        leaks.join("\n")
    );
}

/// A declared Latin name typed with Cyrillic or Greek look-alike letters is hidden too.
#[test]
fn zero_leaks_for_names_with_look_alike_letters() {
    let mut leaks = Vec::new();
    for typed in ["Nоam", "NОAM", "Νoam", "nοam", "Νοаm"] {
        for t in TEMPLATES {
            let text = t.replace("{}", typed);
            let ids = identities();
            let practitioner = vec!["דנה כהן-לוי".to_owned()];
            let out = filter(&text, &ctx_for(&ids, &practitioner, CASE)).unwrap();
            if out.tagged.contains(typed) || gate_with(&text, &ids, CASE).cleared {
                leaks.push(format!("{text} → {}", out.tagged));
            }
        }
    }
    assert!(
        leaks.is_empty(),
        "{} leaks:\n{}",
        leaks.len(),
        leaks.join("\n")
    );
}

/// Groups of digits joined by a maqaf or a Unicode hyphen ("000־000־018") went out whole and
/// passed the gate: the patterns took only "-" between groups. An ID glued by a hyphen to a
/// date ("000000018-3.9.2026") went out whole too, and only the gate stopped it. Checked digit by digit, since
/// the joiners keep the number from showing as one string. Found in review of D-126.
#[test]
fn numbers_joined_by_a_maqaf_or_a_unicode_hyphen_never_go_out() {
    let join = |parts: &[&str]| parts.concat();
    let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    for (text, secret) in [
        ("000־000־018".to_owned(), "000000018"),
        ("ת.ז. 000־000־018".to_owned(), "000000018"),
        ("ת.ז. 0000־0001־8".to_owned(), "000000018"),
        ("000\u{2011}000\u{2011}018".to_owned(), "000000018"),
        ("000\u{2010}000\u{2010}018 נמסר".to_owned(), "000000018"),
        ("0־0־0־0־0־0־0־1־8".to_owned(), "000000018"),
        (join(&["טלפון 054", "־7654321"]), "7654321"),
        (join(&["טלפון 054־765", "־4321"]), "7654321"),
        // A date glued to the end took the run, and the ID before it went out whole.
        ("ת.ז. 000000018-3.9.2026".to_owned(), "000000018"),
        ("000000018־12.3.2020 נבדק".to_owned(), "000000018"),
        ("נבדק 3.9.2026/000000018".to_owned(), "000000018"),
        ("נבדק 3.9.2026.000000018".to_owned(), "000000018"),
    ] {
        let ids = identities();
        let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(
            !digits(&out.tagged).contains(secret),
            "{text} → {}",
            out.tagged
        );
        let o = send(&text, CASE);
        assert!(o.leaked.is_empty(), "{text} leaked {:?}", o.leaked);
        // The gate refuses the number itself.
        assert!(
            !gate_with(&text, &ids, CASE).cleared,
            "{text} cleared by the gate"
        );
    }
}

/// Every separator a document or a keyboard may put between the groups of an ID or a phone:
/// the dashes, the minus, other scripts' hyphens, the spaces that do not break, and an
/// apostrophe, a geresh or a comma. All of them went out whole and the gate cleared them
/// (second review of D-126). The filter hides each, and the gate refuses each on its own.
#[test]
fn numbers_split_by_any_dash_or_space_never_go_out() {
    let join = |parts: &[&str]| parts.concat();
    let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    let ids = identities();
    let dashes = [
        '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}', '\u{2212}', '\u{FE58}', '\u{058A}',
        '\u{2043}', '\u{2E3A}', '\u{30FC}', '\u{FE63}', '\u{2E17}',
    ];
    let spaces = [
        '\u{00A0}', '\u{2009}', '\u{202F}', '\u{3000}', '\u{2007}', '\u{2002}', ' ',
    ];
    let marks = ['\'', '\u{05F3}', ',', ':', ';', '_'];
    let mut cases = Vec::new();
    for sep in dashes.iter().chain(&spaces).chain(&marks) {
        let s = sep.to_string();
        cases.push((join(&["ת.ז. 000", &s, "000", &s, "018 נמסר"]), "000000018"));
        cases.push((join(&["ת.ז. 0000", &s, "0001", &s, "8 נמסר"]), "000000018"));
        let single: Vec<String> = "000000018".chars().map(String::from).collect();
        cases.push((join(&["מספר ", &single.join(&s), " נמסר"]), "000000018"));
        cases.push((join(&["טלפון 054", &s, "765", &s, "4321"]), "7654321"));
    }
    for sep in dashes {
        // Without a label too: joined by a dash, nine digits are an ID.
        let s = sep.to_string();
        cases.push((join(&["000", &s, "000", &s, "018 נמסר"]), "000000018"));
    }
    // After an ID label, nine digits in groups of any size, or with a mark inside one
    // (third review of D-126).
    for text in [
        "ת.ז 00 00 00 01 8 נמסר",
        "ת.ז 000 000 01 8 נמסר",
        "ת\"ז: 0 00 000 018",
        "ת.ז 0.0.0.0.0.0.0.1.8",
        "ת.ז 0000\u{301}00018",
        // Fourth review of D-128: two or three marks between the groups, the label after
        // the number, and the labels "תז", "מ.ז" and "id".
        "000 - 000 - 018 נמסר",
        "000  000  018 נמסר",
        "000, 000, 018 נמסר",
        "ת.ז 000 - 000 - 018",
        "ת.ז 00  00  00  01  8",
        "ת.ז 000, 000, 018",
        "00 00 00 01 8 ת.ז",
        "000 000 01 8 (מספר זהות)",
        "תז 00 00 00 01 8",
        "מ.ז. 00 00 00 01 8",
        "id 00 00 00 01 8",
        "ID: 00 00 00 01 8",
    ] {
        cases.push((text.to_owned(), "000000018"));
    }
    // A phone with two or three marks between its groups (review of D-130).
    // Built here, so no phone is written out in the source (the pre-commit scan).
    let (a, b) = ("123", "4567");
    for text in [
        format!("טלפון 050 - {a} - {b}"),
        format!("050  {a}  {b}"),
        format!("052 - {a} - 45 - 67"),
        format!("נייד: 050 - {a}4 - 567"),
        format!("טל' 03 - {a} - {b}"),
        format!("+972 - 50 - {a} - {b}"),
        format!("(050) - {a} - {b}"),
    ] {
        cases.push((text, "1234567"));
    }
    // Not a date the date rule takes, so still the ID.
    cases.push(("ת.ז 0.0.0000 018".to_owned(), "000000018"));
    // The label after it, with a date after the label.
    cases.push(("000 000 01 8 ת.ז. 12.3.2020".to_owned(), "000000018"));
    for (text, secret) in cases {
        let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(
            !digits(&out.tagged).contains(secret),
            "{text:?} → {}",
            out.tagged
        );
        let o = send(&text, CASE);
        assert!(o.leaked.is_empty(), "{text:?} leaked {:?}", o.leaked);
        assert!(
            !gate_with(&text, &ids, CASE).cleared,
            "{text:?} cleared by the gate"
        );
    }
    // A row of scores typed with plain spaces or commas is still a row of scores.
    for text in [
        "ציונים: 112 104 106",
        "ציונים: 112,104,106",
        "טווח 100–115",
        "ציונים 100–112–125",
        "תזונה: 112 104 106",
        "ציונים 100 - 112 - 125",
        // A label after the row that is not its own (review of D-130).
        "ציונים 100 112 125. ת.ז לא נמסרה",
        "ציונים 100 112 125 (ת.ז בתיק)",
        "ציונים 08 10 12 14 9",
    ] {
        let out = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert_eq!(out.tagged, text);
    }
    // A label after a row of scores is the next number's, also once that number is a tag;
    // and a date and an hour after a label are a date and an hour (fourth review of D-128).
    for (text, out) in [
        (
            "ציונים 100 112 125, ת.ז 000000018",
            "ציונים 100 112 125, ת.ז [ת.ז.]",
        ),
        (
            "ת.ז בתיק. נבדק 12.3.2020 10:15",
            "ת.ז בתיק. נבדק לפני כ-7 שנים 10:15",
        ),
        (
            "ת.ז בתיק. נבדק 12.3.20 10:15",
            "ת.ז בתיק. נבדק לפני כ-7 שנים 10:15",
        ),
        (
            "ציונים 100 112 125, ת.ז של הילד 000000018",
            "ציונים 100 112 125, ת.ז של הילד [ת.ז.]",
        ),
    ] {
        let filtered = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert_eq!(filtered.tagged, out);
        assert!(gate_with(&filtered.tagged, &ids, CASE).cleared, "{out}");
    }
}

/// A declared name with a mark glued after it never goes out: "נועם' הלך" was read as an
/// abbreviation ("ספט'") and went out whole, cleared by the gate (review of D-130).
#[test]
fn a_declared_name_with_any_mark_after_it_never_goes_out() {
    let ids = identities();
    for name in ["נועם", "רותם", "מאיה", "עידו"] {
        for mark in [
            "'", "\u{05F3}", "\u{2019}", "\"", "\u{05F4}", "\u{201D}", ".", ",", "!", "?", ":",
            ";", ")", "-", "…",
        ] {
            for context in ["{} הלך הביתה", "אמרתי ל{} שזה בסדר", "{} 2020"] {
                let text = context.replace("{}", &format!("{name}{mark}"));
                let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
                assert!(
                    !normalize(&out.tagged).contains(&normalize(name)),
                    "{text:?} → {}",
                    out.tagged
                );
                let o = send(&text, CASE);
                assert!(o.leaked.is_empty(), "{text:?} leaked {:?}", o.leaked);
                assert!(
                    !gate_with(&text, &ids, CASE).cleared,
                    "{text:?} cleared by the gate"
                );
            }
        }
    }
    // A child named טל: "טל'" with a geresh is her name, except as the abbreviation of a
    // phone number right before one.
    let mut with_tal = identities();
    with_tal.push(id(CASE, Role::Brother, "[אח_1]", "טל", &[]));
    let tal = |text: &str| filter(text, &ctx_for(&with_tal, &[], CASE)).unwrap().tagged;
    assert_eq!(tal("טל' הלך הביתה"), "[אח_1]' הלך הביתה");
    // Built here, so no phone is written out in the source (the pre-commit scan).
    let (a, b) = ("123", "4567");
    for text in [
        format!("טל' 03-{a}{b}"),
        format!("טל' הגן: 08-{a}{b}"),
        format!("טל': 050-{a}{b}"),
    ] {
        assert!(tal(&text).starts_with("טל'"), "{text:?} → {}", tal(&text));
    }
}

/// A kept-words tag (D-126) stands for her words only in her own stored paragraph, which
/// dv-core restores whole before filtering. Planted anywhere else (an answer of the model, a
/// material, her chat) it is read as plain words: out of its place a kept word would lose the
/// rule that hid it ("עם [מילים_1]" would have sent the kindergarten's name with no
/// institution word before it). The gate refuses the tag itself, and the answer scan points
/// it out. Found in review of D-126.
#[test]
fn a_kept_words_tag_planted_anywhere_never_brings_her_words_out() {
    let ids = identities();
    let entry = |tag: &str, original: &str| dv_privacy::MaskEntry {
        tag: tag.to_owned(),
        kind: dv_privacy::KEPT_WORDS.to_owned(),
        original: original.to_owned(),
    };
    let book = MaskBook::new(vec![
        entry("[מילים_1]", " \"זרזירים\""),
        entry("[מילים_9]", "כמנהלת חשבונות במאפיית שיבולים"),
    ]);
    let ctx = PrivacyContext {
        masks: Some(&book),
        ..ctx_for(&ids, &[], CASE)
    };
    let case_tags: Vec<String> = ids
        .iter()
        .filter(|i| i.case_id == CASE)
        .map(|i| i.tag.clone())
        .collect();
    for text in [
        "הילד שמח עם [מילים_1] מאוד",
        "האם עובדת [מילים_9].",
        "[מילים_1][מילים_9]",
        "ב[מילים_9] ובגן[מילים_1]",
        "לגן [מילים_1] הגיע",
    ] {
        let out = filter(text, &ctx).unwrap();
        for word in ["זרזירים", "חשבונות", "שיבולים"] {
            assert!(!out.tagged.contains(word), "{text} → {}", out.tagged);
        }
        assert!(!out.tagged.contains("[מילים_"), "{text} → {}", out.tagged);
        let o = send(text, CASE);
        assert!(o.leaked.is_empty(), "{text} leaked {:?}", o.leaked);
        assert!(
            !gate_with(text, &ids, CASE).cleared,
            "{text} cleared by the gate"
        );
        let found = dv_privacy::restore::scan_model_output(text, &case_tags, &ids);
        assert!(
            found.iter().any(|s| s.token.starts_with("[מילים_")),
            "{text}: {found:?}"
        );
    }
}

/// Every way a month is named, for the date tests below (D-128).
const HE_MONTH_NAMES: &[&str] = &[
    "ינואר",
    "פברואר",
    "מרץ",
    "מרס",
    "אפריל",
    "מאי",
    "יוני",
    "יולי",
    "אוגוסט",
    "ספטמבר",
    "אוקטובר",
    "נובמבר",
    "דצמבר",
    "ינו'",
    "פבר'",
    "מארס",
    "אפר'",
    "אוג'",
    "ספט'",
    "אוק'",
    "אוקט'",
    "נוב'",
    "דצמ'",
];
const EN_MONTH_NAMES: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Sept",
    "Oct",
    "Nov",
    "Dec",
];

/// A month's name as a word of `text`, alone or with a Hebrew prefix ("במרץ", "לינואר").
fn month_name_in(text: &str) -> Option<String> {
    dv_privacy::text::tokenize(text).into_iter().find_map(|t| {
        let named = dv_privacy::text::prefix_splits(&t.norm)
            .iter()
            .any(|(_, w)| {
                HE_MONTH_NAMES.iter().any(|m| normalize(m) == *w)
                    || EN_MONTH_NAMES.iter().any(|m| normalize(m) == *w)
            });
        named.then_some(t.norm)
    })
}

/// The filter's outcome for one date phrase inside `context`: one generalized date, no month
/// name and no year left, and the gate clears it (no dead end).
fn check_one_date(context: &str, phrase: &str, year: &str) -> Result<(), String> {
    check_one_date_with(&identities(), context, phrase, year)?;
    let o = send(&context.replace("{}", phrase), CASE);
    if !o.leaked.is_empty() {
        return Err(format!("{phrase:?} leaked {:?}", o.leaked));
    }
    Ok(())
}

/// [`check_one_date`] with the case's people `ids` (a sister named like a month).
fn check_one_date_with(
    ids: &[Identity],
    context: &str,
    phrase: &str,
    year: &str,
) -> Result<(), String> {
    let text = context.replace("{}", phrase);
    let out = filter(&text, &ctx_for(ids, &[], CASE)).unwrap();
    if let Some(m) = month_name_in(&out.tagged) {
        return Err(format!(
            "{text:?} → {:?}: the month {m} was left",
            out.tagged
        ));
    }
    if out.tagged.contains(year) {
        return Err(format!("{text:?} → {:?}: the year was left", out.tagged));
    }
    if !gate_with(&out.tagged, ids, CASE).cleared {
        return Err(format!("{text:?} → {:?}: the gate refused it", out.tagged));
    }
    Ok(())
}

/// A date with a month name comes out as one generalized date, the way a numeric one does,
/// with nothing of it beside it. "March 12.3.2020" went out as "March לפני כ-7 שנים" and the
/// gate refused it: Einat could not send the paragraph and did not know why (D-128).
#[test]
fn dates_with_a_month_name_come_out_whole_and_generalized() {
    for (phrase, year) in [
        ("12 במרץ 2020", "2020"),
        ("ב-12 במרץ 2020", "2020"),
        ("במרץ 2020", "2020"),
        ("March 12.3.2020", "2020"),
        ("12.3.2020 March", "2020"),
        ("12 March 2020", "2020"),
        ("Mar. 2020", "2020"),
        ("מרץ 20", "20"),
        ("12 במרץ 20", "20"),
        ("12 במרץ, 2020", "2020"),
        ("12.3.2020 (מרץ)", "2020"),
        ("March 3, 2025", "2025"),
        ("March 3rd, 2025", "2025"),
        ("on 12th March 2020", "2020"),
        ("2020 March", "2020"),
        ("בשנת 2020 במרץ", "2020"),
        ("12/מרץ/2020", "2020"),
        ("12-במרץ-2020", "2020"),
        ("ינואר-2020", "2020"),
        ("Jan '20", "20"),
        ("מרץ ’20", "20"),
        ("SEPT 2021", "2021"),
        ("3 במרס 2025", "2025"),
        ("31 בפברואר 2020", "2020"),
        // Third review.
        ("12th of March 2020", "2020"),
        ("the 12th of March, 2020", "2020"),
        ("March the 12th, 2020", "2020"),
        ("י\"ב במרץ 2020", "2020"),
        ("ט\"ו בספטמבר 2021", "2021"),
        ("15 בתשרי 2020", "2020"),
        ("אדר ב' 2024", "2024"),
        ("בכסלו 2021", "2021"),
        ("ספט' 20", "20"),
        ("אוג' 2020", "2020"),
        ("אוקט' 2020", "2020"),
        ("פבר' 2020", "2020"),
        ("12 בספט' 2020", "2020"),
        ("מארס 2020", "2020"),
        ("ב-12.3 במרץ 2020", "2020"),
        ("2020-Mar-12", "2020"),
        ("2020 אפריל 85", "2020"),
        ("12 במרץ (2020)", "2020"),
        ("(12 במרץ) 2020", "2020"),
    ] {
        if let Err(e) = check_one_date("נבדק {} בבוקר", phrase, year) {
            panic!("{e}");
        }
    }
    // A month alone, in a word that is also an everyday one, stays ("energetically"). One
    // that is only a month ("בינואר") is hidden as before.
    let ids = identities();
    for text in ["נבדק במרץ בבוקר", "עבד במרץ רב", "ילד מלא מרץ ושמחה"]
    {
        assert_eq!(
            filter(text, &ctx_for(&ids, &[], CASE)).unwrap().tagged,
            text
        );
    }
    // A range keeps both its ends, each generalized.
    for text in ["בין מרץ 2020 למאי 2021", "12 במרץ – 15 באפריל 2020"] {
        let out = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(month_name_in(&out.tagged).is_none(), "{}", out.tagged);
        assert_eq!(out.tagged.matches("לפני").count(), 2, "{}", out.tagged);
    }
    // The exact generalization: the year written out wins, a short year is not the next
    // date's day, an age is not a year.
    for (text, out) in [
        ("ב-12.3 במרץ 2020", "ב-לפני כ-6 שנים"),
        ("2020-Mar-12", "לפני כ-7 שנים"),
        ("2020 אפריל 85", "לפני כ-6 שנים 85"),
    ] {
        assert_eq!(filter(text, &ctx_for(&ids, &[], CASE)).unwrap().tagged, out);
    }
    // "may" the verb.
    for text in ["She may 3 times a week refuse", "It may rain"] {
        assert_eq!(
            filter(text, &ctx_for(&ids, &[], CASE)).unwrap().tagged,
            text
        );
    }
    // A sister named like a month: her name is hers, and the date next to it is still one
    // generalized date that the gate clears (third review of D-128).
    let mut with_may = identities();
    with_may.push(id(CASE, Role::Sister, "[אחות_2]", "מאי", &[]));
    with_may.push(id(CASE, Role::Brother, "[אח_1]", "יוני", &[]));
    for phrase in [
        "12.5.2020 (מאי)",
        "2020 מאי",
        "מאי 2020 הגיעה",
        "ביוני 2015",
        "3.9.2020 יוני",
    ] {
        let year = if phrase.contains("2015") {
            "2015"
        } else {
            "2020"
        };
        if let Err(e) = check_one_date_with(&with_may, "נבדקה {} בבוקר", phrase, year) {
            panic!("{e}");
        }
    }
    // Fourth review of D-128: the day beside her name is hidden too, and a word glued to
    // her name ("ממאי", "from May") is her name, not a date.
    for text in [
        "נבדקה מאי 3, 2020 בבוקר",
        "נבדקה יוני 15 2020 בבוקר",
        "נבדקה מאי (3) 2020 בבוקר",
    ] {
        let out = filter(text, &ctx_for(&with_may, &[], CASE)).unwrap();
        // Nothing but the tags and the generalized year carries a digit.
        let mut rest = out.tagged.replace("לפני כ-6 שנים", "");
        while let (Some(a), Some(b)) = (rest.find('['), rest.find(']')) {
            if b < a {
                break;
            }
            rest.replace_range(a..=b, "");
        }
        assert!(
            !rest.contains(|c: char| c.is_ascii_digit()),
            "{text:?} → {}",
            out.tagged
        );
        assert!(
            gate_with(&out.tagged, &with_may, CASE).cleared,
            "{}",
            out.tagged
        );
    }
    for (text, out) in [
        ("קיבלנו מידע ממאי", "קיבלנו מידע מ[אחות_2]"),
        ("ביוני", "ב[אח_1]"),
    ] {
        assert_eq!(
            filter(text, &ctx_for(&with_may, &[], CASE)).unwrap().tagged,
            out
        );
    }
}

/// A Hebrew-calendar month with a day in digits, and no year or a short one, is a date:
/// "15 בתשרי" went out unchanged (fourth review of D-128). Bare, the month may be a name
/// ("סיון"), and it is not taken into the date next to it: she is asked about, or hidden as
/// a person, never lost in a date.
#[test]
fn hebrew_calendar_dates_with_a_day_in_digits_are_hidden_whole() {
    let ids = identities();
    for text in [
        "נבדק 15 בתשרי בבוקר",
        "נבדק 15 בטבת בבוקר",
        "נבדק 15 בתשרי 20 בבוקר",
        "נבדק בתשרי 20 בבוקר",
        "נבדק 3 ב-אדר 2024 בבוקר",
        "נבדק בסיון 2020",
    ] {
        let out = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(
            !out.tagged.contains(|c: char| c.is_ascii_digit()),
            "{text:?} → {}",
            out.tagged
        );
        assert!(out.tagged.contains("[תאריך]"), "{text:?} → {}", out.tagged);
        assert!(gate_with(&out.tagged, &ids, CASE).cleared, "{}", out.tagged);
        let o = send(text, CASE);
        assert!(o.leaked.is_empty() && o.cleared, "{text:?}");
    }
    for text in ["פגישה עם סיון 12.3.2020", "פגישה עם סיוון 12.3.2020"] {
        let out = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(
            out.hidden.iter().any(|h| h == "שם אחר"),
            "{text:?} → {}",
            out.tagged
        );
        assert!(out.tagged.contains("לפני כ-7 שנים"), "{}", out.tagged);
        let o = send(text, CASE);
        assert!(o.leaked.is_empty() && o.cleared, "{text:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    /// Month names × separators × orders × contexts (D-128): one generalized date, no month
    /// name and no year beside it, cleared by the gate, nothing leaked.
    #[test]
    fn every_month_name_with_any_separator_and_order_is_one_generalized_date(
        hebrew in any::<bool>(),
        month in 0usize..64,
        prefix in 0usize..4,
        dot in any::<bool>(),
        order in 0usize..6,
        sep in 0usize..6,
        day in 1u32..=28,
        year in 2015u32..=2025,
        short in any::<bool>(),
        context in 0usize..4,
    ) {
        let seps = [" ", "-", "/", ".", ", ", " - "];
        let s = seps[sep];
        let m = if hebrew {
            let name = HE_MONTH_NAMES[month % HE_MONTH_NAMES.len()];
            format!("{}{name}", ["", "ב", "ל", "ב-"][prefix])
        } else {
            let name = EN_MONTH_NAMES[month % EN_MONTH_NAMES.len()];
            let dot = if dot && name.len() <= 4 { "." } else { "" };
            format!("{name}{dot}")
        };
        let y = if short { format!("{:02}", year % 100) } else { year.to_string() };
        let d = day.to_string();
        let phrase = match order {
            0 => [d.as_str(), &m, &y].join(s),
            1 => [m.as_str(), &y].join(s),
            2 => [y.as_str(), &m].join(s),
            3 => [m.as_str(), &d, &y].join(s),
            4 => format!("{d}.{}.{y}{s}{m}", (month % 12) + 1),
            _ => format!("{m}{s}{d}.{}.{y}", (month % 12) + 1),
        };
        let contexts = ["נבדק {} בבוקר", "{}", "בתאריך {}, הגיע עם אמו.", "(נבדק {})"];
        // Every other case has a sister and a brother named like months.
        let r = if day % 2 == 0 {
            let mut ids = identities();
            ids.push(id(CASE, Role::Sister, "[אחות_2]", "מאי", &[]));
            ids.push(id(CASE, Role::Brother, "[אח_1]", "יוני", &[]));
            check_one_date_with(&ids, contexts[context], &phrase, &y)
        } else {
            check_one_date(contexts[context], &phrase, &y)
        };
        prop_assert!(r.is_ok(), "{}", r.unwrap_err());
        // No day of it left either, beside a sister's name too (fourth review of D-128).
        let ids = if day % 2 == 0 {
            let mut ids = identities();
            ids.push(id(CASE, Role::Sister, "[אחות_2]", "מאי", &[]));
            ids.push(id(CASE, Role::Brother, "[אח_1]", "יוני", &[]));
            ids
        } else {
            identities()
        };
        let text = contexts[context].replace("{}", &phrase);
        let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
        prop_assert!(!digit_left(&out.tagged), "{text:?} → {}", out.tagged);
    }

    /// A Hebrew-calendar month with "in", a day in digits and no year, a short one or a full
    /// one (fourth review of D-128): one hidden date, no digit and no month left.
    #[test]
    fn hebrew_calendar_months_with_a_day_are_one_hidden_date(
        month in 0usize..HEBREW_CAL_NAMES.len(),
        apart in any::<bool>(),
        day in 1u32..=30,
        year in 0usize..3,
        context in 0usize..3,
    ) {
        let m = HEBREW_CAL_NAMES[month];
        let y = ["", " 20", " 2021"][year];
        let phrase = format!("{day} {}{m}{y}", if apart { "ב-" } else { "ב" });
        let text = ["נבדק {} בבוקר", "{}", "(נבדק {})"][context].replace("{}", &phrase);
        let ids = identities();
        let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
        prop_assert!(!digit_left(&out.tagged), "{text:?} → {}", out.tagged);
        prop_assert!(!out.tagged.contains(m), "{text:?} → {}", out.tagged);
        prop_assert!(gate_with(&out.tagged, &ids, CASE).cleared, "{}", out.tagged);
        let o = send(&text, CASE);
        prop_assert!(o.leaked.is_empty() && o.cleared, "{text:?}");
    }

    /// An ID in groups of any size, with one to three marks between them, next to a label
    /// before or after it; or, with no label, in groups of three or four (fourth review of
    /// D-128). Never out, and the gate refuses it in the clear.
    #[test]
    fn an_id_in_groups_with_any_marks_and_a_label_never_goes_out(
        cuts in proptest::collection::vec(any::<bool>(), 8),
        marks in proptest::collection::vec((0usize..10, 1usize..=3), 8),
        label in 0usize..8,
        after in any::<bool>(),
        shape in 0usize..4,
    ) {
        let pool = [" ", "-", "\u{2013}", ",", ":", ";", "_", "\u{00A0}", "'", "."];
        let digits: Vec<char> = "000000018".chars().collect();
        let labels = ["ת.ז", "ת\"ז", "תז", "מ.ז.", "מספר זהות", "id", "ID", ""];
        let labelled = !labels[label].is_empty();
        // Where the groups end: anywhere with a label, else 3/4-3/4-1..3.
        let ends: Vec<bool> = if labelled {
            cuts
        } else {
            let sizes = [[3, 3, 3], [4, 4, 1], [3, 4, 2], [4, 3, 2]][shape];
            let mut ends = vec![false; 8];
            ends[sizes[0] - 1] = true;
            ends[sizes[0] + sizes[1] - 1] = true;
            ends
        };
        let mut number = String::new();
        for (i, d) in digits.iter().enumerate() {
            number.push(*d);
            if i < 8 && ends[i] {
                let (k, n) = marks[i];
                // Without a label, a period joins a date, not an ID.
                let k = if labelled { k } else { k % 9 };
                number.push_str(&pool[k].repeat(n));
            }
        }
        let text = match (labelled, after) {
            (false, _) => format!("נמסר {number} בתיק"),
            (true, false) => format!("{} {number} נמסר", labels[label]),
            (true, true) => format!("נמסר {number} ({})", labels[label]),
        };
        let ids = identities();
        let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
        let left: String = out.tagged.chars().filter(char::is_ascii_digit).collect();
        prop_assert!(!left.contains("000000018"), "{text:?} → {}", out.tagged);
        let o = send(&text, CASE);
        prop_assert!(o.leaked.is_empty(), "{text:?} leaked {:?}", o.leaked);
        prop_assert!(!gate_with(&text, &ids, CASE).cleared, "{text:?} cleared by the gate");
    }
}

/// The months of the Hebrew calendar, as she writes them.
const HEBREW_CAL_NAMES: &[&str] = &[
    "תשרי",
    "חשוון",
    "חשון",
    "כסלו",
    "טבת",
    "שבט",
    "אדר",
    "ניסן",
    "אייר",
    "סיוון",
    "סיון",
    "תמוז",
    "אב",
    "אלול",
];

/// A digit in the filtered text outside the tags and the generalized times ("לפני כ-6
/// שנים"): a piece of a date left beside it.
fn digit_left(tagged: &str) -> bool {
    let mut rest = tagged.to_owned();
    while let (Some(a), Some(b)) = (rest.find('['), rest.find(']')) {
        if b < a {
            break;
        }
        rest.replace_range(a..=b, "");
    }
    let mut out = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if out.ends_with("כ-") {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
        }
    }
    out.contains(|c: char| c.is_ascii_digit())
}

// ------------------------------------------------------------------ D-131: the review of 0.9.1

/// A bidirectional control (a direction mark, an embedding, an isolate) between two words
/// glued them into one token, so a declared name next to one passed the filter and the gate:
/// "נועם\u{200F}הלך" went out whole. Now the text is read both ways, and a name inside a word
/// ("נו\u{200E}עם") is still the name.
#[test]
fn a_direction_mark_between_a_name_and_the_next_word_never_lets_it_out() {
    let controls = [
        '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
        '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{061C}',
    ];
    let ids = identities();
    for z in controls {
        for (text, name) in [
            (format!("נועם{z}הלך לגן."), "נועם"),
            (format!("הלך{z}נועם לגן."), "נועם"),
            (format!("{z}נועם{z}הלך{z}לגן"), "נועם"),
            (format!("Noam{z}walked home."), "noam"),
            (format!("מיכל{z}הגננת סיפרה."), "מיכל"),
            (format!("גן{z}הדקל"), "הדקל"),
            (format!("כפר{z}ורדים"), "ורדים"),
            (format!("ד\"ר{z}שטרן"), "שטרן"),
            (format!("נו{z}עם הלך לגן."), "נועם"),
        ] {
            let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
            assert!(
                !normalize(&out.tagged).contains(&normalize(name)),
                "{text:?} → {:?}",
                out.tagged
            );
            let o = send(&text, CASE);
            assert!(o.leaked.is_empty(), "{text:?} leaked {:?}", o.leaked);
            assert!(o.cleared, "{text:?} is a dead end");
            assert!(
                !gate_with(&text, &ids, CASE).cleared,
                "{text:?} cleared raw by the gate"
            );
        }
    }
}

/// Characters that show nothing and were not read as marks: a variation selector (U+FE0F
/// comes with every emoji keyboard), a tag character, an interlinear mark, a Hangul filler.
/// Inside a name or a number each one split it, and it went out whole.
#[test]
fn more_invisible_characters_inside_a_name_or_a_number_never_let_it_out() {
    let invisible = [
        '\u{FE00}',
        '\u{FE0E}',
        '\u{FE0F}',
        '\u{180E}',
        '\u{061C}',
        '\u{E0001}',
        '\u{E0100}',
        '\u{FFF9}',
        '\u{FFFA}',
        '\u{FFFB}',
        '\u{1D173}',
        '\u{206A}',
        '\u{206F}',
        '\u{3164}',
        '\u{115F}',
        '\u{1160}',
        '\u{FFA0}',
    ];
    let ids = identities();
    let strip = |s: &str| -> String { s.chars().filter(|c| !invisible.contains(c)).collect() };
    for z in invisible {
        for (text, secret) in [
            (format!("נו{z}עם הלך לגן."), "נועם"),
            (format!("{z}נועם הלך."), "נועם"),
            (format!("נועם{z} הלך."), "נועם"),
            (format!("No{z}am walked."), "noam"),
            (format!("גן הד{z}קל"), "הדקל"),
            (format!("כפר ור{z}דים"), "ורדים"),
            (format!("ל{z}נועם יש אח."), "נועם"),
            (format!("ת.ז. 0000{z}00018"), "000000018"),
            (format!("000{z}000{z}018"), "000000018"),
        ] {
            let out = filter(&text, &ctx_for(&ids, &[], CASE)).unwrap();
            let kept = normalize(&strip(&out.tagged));
            assert!(
                !kept.contains(&normalize(secret)),
                "{text:?} → {:?}",
                out.tagged
            );
            let o = send(&text, CASE);
            assert!(o.leaked.is_empty(), "{text:?} leaked {:?}", o.leaked);
            assert!(
                !gate_with(&text, &ids, CASE).cleared,
                "{text:?} cleared raw by the gate"
            );
        }
    }
}

/// A declared name inside a longer word: a diminutive ("עידוש", "מאיהלה"), a glued digit or
/// Latin letter ("נועם1", "noam123"), the possessive typed in Hebrew ("נועם'ס"), a plural
/// ("שטרנים"), a typo ("ננועם"). Each went out whole. The whole word is hidden, under the
/// name's tag, and the gate refuses it raw. Everyday words that merely hold a name stay.
#[test]
fn a_declared_name_inside_a_longer_word_is_hidden_whole() {
    let ids = identities();
    for (text, name) in [
        ("עידוש לא רצה לצאת.", "עידו"),
        ("מיכלוש סיפרה.", "מיכל"),
        ("מאיהלה בכתה.", "מאיה"),
        ("נועמוש הגיע.", "נועם"),
        ("נועם'ס", "נועם"),
        ("הציור של נועם'ס.", "נועם"),
        ("נועם1 הגיע.", "נועם"),
        ("קובץ noam123.docx", "noam"),
        ("נועםabc", "נועם"),
        ("ננועם הגיע.", "נועם"),
        ("נועםם הגיע.", "נועם"),
        ("שטרנים", "שטרן"),
        ("השטרנים הגיעו.", "שטרן"),
        ("מיכל׳ל", "מיכל"),
        ("מתגוררים בכפרורדים.", "ורדים"),
        ("נועם%20מיכל", "מיכל"),
    ] {
        let out = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(
            !normalize(&out.tagged).contains(&normalize(name)),
            "{text:?} → {:?}",
            out.tagged
        );
        let o = send(text, CASE);
        assert!(o.leaked.is_empty(), "{text:?} leaked {:?}", o.leaked);
        assert!(o.cleared, "{text:?} is a dead end");
        assert!(
            !gate_with(text, &ids, CASE).cleared,
            "{text:?} cleared raw by the gate"
        );
    }
    // The diminutive is listed as a name inside a word, so "להחזיר" can keep it.
    let out = filter("עידוש לא רצה לצאת.", &ctx_for(&ids, &[], CASE)).unwrap();
    assert!(
        out.auto_hidden
            .iter()
            .any(|a| a.token == "עידוש" && a.kind == AutoKind::DeclaredInWord && a.tag == "[אב]"),
        "{:?}",
        out.auto_hidden
    );
    // Everyday words that hold a name are words ("עידוד" holds "עידו", "שאלון" holds the
    // other case's "אלון"), and a short name ("דנה", the practitioner) sits in many words.
    // A name that only starts a longer word ("קוגניטיבי" starts with a surname "קוגן") is
    // that word, and a name of three letters and a geresh ("ג'וד") is not held by "ג'ודו".
    let mut more = ids.clone();
    more.push(id(OTHER, Role::Father, "[אב]", "יוסי קוגן", &[]));
    more.push(id(OTHER, Role::Brother, "[אח_1]", "ג'וד", &[]));
    // A word the filter once took for a name ("Core", kept from the card) is held by a
    // longer word only after digits, not after a letter.
    let mut core = id(OTHER, Role::Other, "[אדם_2]", "Core", &[]);
    core.source = dv_domain::IdentitySource::Auto;
    more.push(core);
    for text in [
        "עידוד חיובי עוזר לו.",
        "השאלון הוחזר מלא.",
        "בשאלון ההורים דווח על קושי.",
        "עץ האלון בחצר.",
        "ההתקדמות נבדקה.",
        "פרופיל קוגניטיבי תקין, יכולות קוגניטיביות טובות.",
        "שחייה וג'ודו מתאימים לו.",
    ] {
        let out = filter(text, &ctx_for(&more, &[], CASE)).unwrap();
        assert_eq!(out.tagged, text, "an ordinary word was hidden");
        assert!(gate_with(text, &more, CASE).cleared, "{text:?} refused");
    }
    let out = filter("Core Language Score: 108", &ctx_for(&more, &[], CASE)).unwrap();
    assert!(
        out.tagged.ends_with(" Language Score: 108"),
        "{:?}",
        out.tagged
    );
    // An address whose local part holds a name is hidden whole, not the word alone with the
    // rest of the address left beside its tag.
    for text in [
        "כתבו אל noamk19@example.com בבוקר.",
        "המייל שלה rotemf@example.org נשמר.",
    ] {
        let out = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert!(
            !out.tagged.contains("example.") && !out.tagged.contains('@'),
            "{text:?} → {:?}",
            out.tagged
        );
        let o = send(text, CASE);
        assert!(
            o.leaked.is_empty() && o.cleared,
            "{text:?}: leaked {:?}, cleared {}",
            o.leaked,
            o.cleared
        );
    }
}

/// A note for something missing has one form. Anything else that begins with "[חסר" is a
/// tag the gate refuses: "[חסרנועם]" passed as a note, with the name inside it.
#[test]
fn only_a_well_formed_missing_note_passes_the_gate() {
    let ids = identities();
    for text in [
        "[חסרנועם]",
        "[חסר\u{200F}נועם]",
        "[חסר\u{200B}נועם]",
        "[חסר:נועם]",
        "[חסר: נועם]",
        "[חסר נועם]",
        "[חסר-נועם]",
        "[חסר: ]",
        "[חסר:]",
        "[חסרה: גיל]",
    ] {
        let o = gate_with(text, &ids, CASE);
        assert!(!o.cleared, "{text:?} cleared");
    }
    for text in ["[ילד] הלך [חסר: גיל ההליכה].", "[ילד] הלך [חסר]."] {
        assert!(gate_with(text, &ids, CASE).cleared, "{text:?} refused");
    }
    assert!(dv_privacy::is_missing_note("[חסר: גיל ההליכה]"));
    assert!(dv_privacy::is_missing_note("[חסר]"));
    assert!(!dv_privacy::is_missing_note("[חסרנועם]"));
    assert!(!dv_privacy::is_missing_note("[חסר: ]"));
}

/// After "ד\"ר" and a first name, the next word is a surname only when it is no everyday
/// word: "ד\"ר אבנר בדק את הילד" hid the verb as a second doctor.
#[test]
fn a_verb_after_a_titled_first_name_stays_a_word() {
    let ids = identities();
    let ctx = ctx_for(&ids, &[], CASE);
    for (text, word) in [
        ("ד\"ר אבנר בדק את הילד.", "בדק"),
        ("ד\"ר אבנר המליץ על טיפול.", "המליץ"),
        ("ד\"ר אבנר אמר שהכול תקין.", "אמר"),
        ("ד\"ר אבנר ציין קושי.", "ציין"),
    ] {
        let out = filter(text, &ctx).unwrap();
        assert!(out.tagged.contains(word), "{text:?} → {:?}", out.tagged);
        assert!(
            !out.tagged.contains("[רופא_2]"),
            "{text:?} → {:?}",
            out.tagged
        );
        assert!(!out.tagged.contains("אבנר"), "{text:?} → {:?}", out.tagged);
    }
    // A surname after the first name is still hidden.
    let out = filter("ד\"ר אבנר ברקוביץ בדק.", &ctx).unwrap();
    assert!(!out.tagged.contains("ברקוביץ"), "{:?}", out.tagged);
}

/// Part of a titled identity with its title before it ("ד\"ר שטרן" for "ד\"ר אבנר שטרן")
/// is one tag with the title, so the name comes back once: not "ד\"ר ד\"ר אבנר שטרן".
#[test]
fn a_titled_identity_restores_without_a_doubled_title() {
    let ids = identities();
    let case_ids: Vec<Identity> = ids.iter().filter(|i| i.case_id == CASE).cloned().collect();
    let ctx = ctx_for(&ids, &[], CASE);
    for (text, want) in [
        ("ד\"ר שטרן בדק.", "ד\"ר אבנר שטרן בדק."),
        ("ד״ר שטרן בדק.", "ד\"ר אבנר שטרן בדק."),
        ("דר' שטרן בדק.", "ד\"ר אבנר שטרן בדק."),
        ("ד\"ר אבנר שטרן בדק.", "ד\"ר אבנר שטרן בדק."),
        ("אבנר שטרן בדק.", "ד\"ר אבנר שטרן בדק."),
        ("הגיע אל ד\"ר שטרן.", "הגיע אל ד\"ר אבנר שטרן."),
    ] {
        let out = filter(text, &ctx).unwrap();
        let back = dv_privacy::restore::restore(&out.tagged, &case_ids, None, &[]);
        assert_eq!(back, want, "{text:?} → {:?}", out.tagged);
        assert!(
            gate_with(&out.tagged, &ids, CASE).cleared,
            "{:?}",
            out.tagged
        );
    }
}

/// A place of two consonants declared in Hebrew ("כפר כנא") is found in Arabic script too.
#[test]
fn a_short_place_name_declared_in_hebrew_is_hidden_in_arabic_script() {
    let ids = vec![
        id("ar", Role::Child, "[ילד]", "מוחמד", &[]),
        id("ar", Role::Town, "[יישוב]", "כפר כנא", &[]),
    ];
    let ctx = ctx_for(&ids, &[], "ar");
    // "كفر" is a generic place word, like "כפר", and stays; the name is hidden.
    for text in ["يسكن في كفر كنا.", "كفر كنا", "في كنا"] {
        let out = filter(text, &ctx).unwrap();
        assert!(!out.tagged.contains("كنا"), "{text:?} → {:?}", out.tagged);
        assert!(
            out.tagged.contains("[יישוב]"),
            "{text:?} → {:?}",
            out.tagged
        );
    }
    // Two consonants match a token of three letters or more only: "كن" is a word.
    let out = filter("كن هادئا.", &ctx).unwrap();
    assert_eq!(out.tagged, "كن هادئا.");
    // The generic word for a kindergarten or a town is never a name on its own.
    for text in ["ذهب إلى الروضة.", "في المدرسة كل يوم.", "قرية صغيرة."]
    {
        let out = filter(text, &ctx).unwrap();
        assert_eq!(out.tagged, text, "a generic Arabic word was hidden");
    }
}
