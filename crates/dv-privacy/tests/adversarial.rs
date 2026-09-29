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
use dv_privacy::{clear, filter, filter_split, GateRequest, PrivacyContext};
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

fn send(text: &str, case: &str) -> Outcome {
    let ids = identities();
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let allow = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: case,
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &allow,
        confirmed_names: &|_: &str| false,
        today: TODAY,
    };
    let filtered = filter(text, &ctx).unwrap();
    gate_tagged(&filtered.tagged, filtered.suspects.len(), case)
}

/// The gate's verdict on text that was already filtered (a passage cut from a whole filter).
fn gate_tagged(tagged: &str, unresolved: usize, case: &str) -> Outcome {
    let ids = identities();
    let practitioner = vec!["דנה כהן-לוי".to_owned()];
    let allow = |_: &str| false;
    let ctx = PrivacyContext {
        case_id: case,
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &allow,
        confirmed_names: &|_: &str| false,
        today: TODAY,
    };
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
        unresolved_suspects: unresolved,
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
fn unknown_names_other_cases_and_near_misses_block_until_decided() {
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
        let o = send(s, CASE);
        assert!(!o.cleared, "should block until decided: {s}");
        assert!(o.leaked.is_empty());
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
        today: TODAY,
    };
    let out = filter("במילוי השאלון ובשאלון ASRS, אלון ענה.", &ctx).unwrap();
    assert!(
        out.tagged.contains("השאלון") && out.tagged.contains("ובשאלון"),
        "{}",
        out.tagged
    );
    assert!(out.tagged.contains("[ילד] ענה"));

    let bare = filter("שאלון ההורים הוחזר.", &ctx).unwrap();
    assert_eq!(bare.suspects.len(), 1, "{:?}", bare.suspects);
    assert!(bare.tagged.starts_with("שאלון"), "not rewritten silently");

    let confirmed = |t: &str| t == normalize("שאלון");
    let ctx = PrivacyContext {
        case_id: "c",
        identities: &ids,
        practitioner: &[],
        allowlisted: &confirmed,
        confirmed_names: &|_: &str| false,
        today: TODAY,
    };
    let again = filter("שאלון ההורים הוחזר.", &ctx).unwrap();
    assert!(again.suspects.is_empty());
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
        today: TODAY,
    };
    let named = filter("הגננת סיפרה שאלון מתקשה במעברים.", &ctx).unwrap();
    assert!(named.suspects.is_empty(), "{:?}", named.suspects);
    assert!(named.tagged.contains("ש[ילד] מתקשה"), "{}", named.tagged);
}

#[test]
fn surnames_and_names_after_labels_block_until_decided() {
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
        assert!(
            !o.cleared,
            "{text}: must block until the psychologist decides"
        );
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
    assert!(out.suspects.is_empty(), "{:?}", out.suspects);
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
        today: TODAY,
    }
}

/// D-022: a material is filtered once, whole, and cut into passages. Each passage must be
/// exactly the whole result cut at its edges, leak nothing when sent alone, and keep every
/// question the whole text asks inside it.
#[test]
fn passages_cut_from_a_whole_filter_leak_nothing_and_keep_every_question() {
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

    // Every question of the whole text is asked in the passage that holds it.
    for s in &whole.suspects {
        assert!(
            parts.iter().any(|p| p.suspects.contains(s)),
            "question lost when cutting: {}",
            s.token
        );
    }

    // Sent alone, no passage leaks.
    let mut leaks = Vec::new();
    for part in &parts {
        let o = gate_tagged(&part.tagged, part.suspects.len(), CASE);
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
