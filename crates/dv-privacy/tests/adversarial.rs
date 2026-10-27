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
    assert_eq!(
        out.tagged,
        "[גננת] סיפרה ש[ילד] עבר ל[יישוב] עם ד\"ר [רופא_1]"
    );
    assert!(out.hidden.iter().any(|h| h.contains("הילד")));
    let case_ids: Vec<Identity> = ids.into_iter().filter(|i| i.case_id == CASE).collect();
    let back = dv_privacy::restore::restore(&out.tagged, &case_ids, None, &[]);
    assert_eq!(
        back,
        "מיכל סיפרה שנועם עבר לכפר ורדים עם ד\"ר ד\"ר אבנר שטרן"
    );
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
    ] {
        cases.push((text.to_owned(), "000000018"));
    }
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
    ] {
        let out = filter(text, &ctx_for(&ids, &[], CASE)).unwrap();
        assert_eq!(out.tagged, text);
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
