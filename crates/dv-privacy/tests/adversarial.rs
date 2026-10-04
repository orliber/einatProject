//! Adversarial, canary and property tests. Merge condition: ZERO leaks.
//!
//! A leak = the gate cleared a request whose bytes still contain a protected string.
//! Blocking is never a leak (it costs the psychologist a click, not a child's privacy).
//! All data is fabricated (tests/fixtures/fake_case_noam.yaml, fake_case_metadata_leak.yaml).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use std::collections::HashSet;

use dv_domain::{passage_ranges, Identity, Role};
use dv_privacy::text::{is_valid_prefix, normalize};
use dv_privacy::{
    clear, filter, filter_split, AutoKind, FilterOutcome, GateRequest, PrivacyContext,
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
fn send(text: &str, case: &str) -> Outcome {
    let mut ids = identities();
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let first = filter(text, &ctx_for(&ids, &practitioner, case)).unwrap();
    let more = found(&first, &ids, case);
    ids.extend(more);
    let second = filter(text, &ctx_for(&ids, &practitioner, case)).unwrap();
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
    };
    let out = filter("מיכל סיפרה שנועם עבר לכפר ורדים עם ד\"ר שטרן", &ctx).unwrap();
    assert_eq!(
        out.tagged,
        "[גננת] סיפרה ש[ילד] עבר ל[יישוב] עם ד\"ר [רופא_1]"
    );
    assert!(out.hidden.iter().any(|h| h.contains("הילד")));
    let case_ids: Vec<Identity> = ids.into_iter().filter(|i| i.case_id == CASE).collect();
    let back = dv_privacy::restore::restore(&out.tagged, &case_ids, None);
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
