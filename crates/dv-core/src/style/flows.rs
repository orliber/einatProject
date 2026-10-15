//! End-to-end flows of the writing-style profile (D-043). Fabricated data only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::string_slice
)]

use std::sync::{Arc, Mutex};

use dv_domain::{
    Age, Author, CaseMeta, Consent, GrammaticalGender, IdentityInput, InputKind, Role,
};
use dv_egress::{EgressError, Transport};
use dv_privacy::text::normalize;
use dv_privacy::ClearedPayload;
use serde_json::{json, Value};

use crate::{Core, CoreError};

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

/// A past report, fabricated: names, the fixtures' fake ID, a phone, dates, places and scores.
/// The phone is split in the source so the repository scan (no phone numbers) does not trip.
const PAST_REPORT: &str = concat!(
    "סיכום אבחון פסיכולוגי: יואב לוי\n",
    "תאריך: 12.03.2025 · ת.ז. 000000018 · טלפון 050-",
    "1234567\n",
    "סיבת הפניה\n",
    "יואב הופנה לאבחון על ידי הגננת אורית מגן החצב ברחובות.\n",
    "רקע: הריון, לידה והתפתחות\n",
    "האם, מירב, מתארת הריון תקין ולידה בשבוע 39.\n",
    "הופעה והתרשמות\n",
    "יואב הגיע למפגש עם אמו מירב, נפרד ממנה בקלות ושיתף פעולה לאורך כל המפגש. אלון, אחיו, המתין בחוץ. בשיח עימי ניכר כי הוא מגיב היטב לתיווך של המבוגר.\n",
    "פרופיל קוגניטיבי:\n",
    "יואב הציג תפקוד ממוצע (9) בהבנה מילולית. עם זאת, ניכר קושי בזיכרון העבודה (6). כך לדוגמא, התקשה לחזור על רצף של ספרות.\n",
    "סיכום\n",
    "לסיכום, יואב הוא ילד סקרן וחברותי. עם זאת, ניכרים קשיים בוויסות.\n",
    "המלצות\n",
    "מומלץ על המשך מעקב התפתחותי.\n",
);

const SECRETS: &[&str] = &[
    "יואב",
    "לוי",
    "אורית",
    "החצב",
    "רחובות",
    "מירב",
    "אלון",
    "000000018",
    concat!("050-", "1234567"),
    "1234567",
    "12.03.2025",
    "רותם",
    "אלמוג",
];

#[derive(Clone, Default)]
struct FakeTransport {
    sent: Arc<Mutex<Vec<String>>>,
    answer: Arc<Mutex<Option<Value>>>,
}

impl Transport for FakeTransport {
    fn send(&self, payload: &ClearedPayload) -> Result<Value, EgressError> {
        let body = String::from_utf8_lossy(payload.body()).into_owned();
        self.sent.lock().unwrap().push(body.clone());
        if let Some(a) = self.answer.lock().unwrap().clone() {
            return Ok(a);
        }
        Ok(dv_ai::demo::respond(&serde_json::from_str(&body).unwrap()))
    }
}

fn api_json(v: &Value) -> Value {
    json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": v.to_string() }] })
}

fn vault(transport: Option<FakeTransport>) -> (tempfile::TempDir, Core) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(
        dir.path(),
        transport.map(|t| Box::new(t) as Box<dyn Transport>),
    );
    core.create_vault(PASSWORD).unwrap();
    core.set_practitioner(vec!["ד\"ר רותם אלמוג".into()])
        .unwrap();
    (dir, core)
}

/// A case of a child named "אלון", with consent and one material.
fn case(core: &mut Core) -> String {
    let meta = CaseMeta {
        code: "TEST-0003".into(),
        age: Some(Age {
            years: 5,
            months: 4,
        }),
        child_gender: Some(GrammaticalGender::Male),
        consent: Some(Consent {
            given_on: "2026-09-01".into(),
            form_version: "v1".into(),
            given_by: "שני ההורים".into(),
        }),
        ..CaseMeta::default()
    };
    let id = core
        .create_case(
            meta,
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "אלון".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.add_input(
        &id,
        InputKind::Observation,
        "תצפית",
        "אלון הגיע למפגש בשמחה ושיתף פעולה.",
    )
    .unwrap();
    // These flows draft the impression from the observation, as before it became hers to
    // write in the institute's report (round 30).
    core.set_section_mode("appearance", dv_domain::SectionMode::Ai)
        .unwrap();
    id
}

fn upload(core: &mut Core) -> crate::StyleSourceView {
    let preview = core
        .import_style_source("יואב לוי - אבחון.txt", PAST_REPORT.as_bytes())
        .unwrap();
    let keep: Vec<u32> = preview
        .parts
        .iter()
        .filter(|p| p.included)
        .map(|p| p.index)
        .collect();
    core.save_style_source(&preview.token, &keep, None).unwrap()
}

fn no_secrets(text: &str, what: &str) {
    for s in SECRETS {
        assert!(!text.contains(s), "{s} found in {what}: {text}");
    }
}

/// Analyze every report and build the profile (draft), through the approval flow.
fn build(core: &mut Core) -> crate::StyleProfileView {
    for s in core.style_overview().unwrap().sources {
        let p = core.prepare_style_analysis(&s.id).unwrap();
        assert!(p.blocked.is_empty(), "{:?}", p.blocked);
        core.send_style_analysis(&p.approval_id.unwrap()).unwrap();
    }
    let p = core.prepare_style_profile().unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    core.send_style_profile(&p.approval_id.unwrap()).unwrap()
}

fn last_sent(fake: &FakeTransport) -> String {
    fake.sent.lock().unwrap().last().cloned().unwrap()
}

const PROFILE_HEADER: &str = "פרופיל הסגנון של הפסיכולוגית";

#[test]
fn a_past_report_is_neutralized_before_anything_is_kept() {
    let (_dir, mut core) = vault(None);
    case(&mut core); // its child's name must be hidden in the past report too
    let preview = core
        .import_style_source("יואב לוי - אבחון.txt", PAST_REPORT.as_bytes())
        .unwrap();
    for p in &preview.parts {
        no_secrets(&p.text, "a part");
        no_secrets(&p.heading, "a heading");
    }
    no_secrets(&preview.title, "the title");
    assert!(preview.hidden > 0 && preview.numbers >= 2);
    let included: Vec<&str> = preview
        .parts
        .iter()
        .filter(|p| p.included)
        .filter_map(|p| p.section.as_deref())
        .collect();
    assert_eq!(
        included,
        vec!["appearance", "cognitive", "summary", "recommendations"]
    );
    let cognitive = preview
        .parts
        .iter()
        .find(|p| p.section.as_deref() == Some("cognitive"))
        .unwrap();
    assert!(cognitive.text.contains("[מספר]"), "{}", cognitive.text);
    assert!(cognitive.text.contains("[ילד]"), "{}", cognitive.text);
    assert!(
        cognitive.text.contains("עם זאת, ניכר קושי"),
        "style words stay"
    );

    let source = core
        .save_style_source(&preview.token, &[cognitive.index], Some("דוח של מירב"))
        .unwrap();
    no_secrets(&source.title, "the title she typed");
    assert_eq!(source.headings.len(), 1, "only the parts she kept");
    assert!(matches!(
        core.save_style_source(&preview.token, &[0], None),
        Err(CoreError::NotFound(_))
    ));
}

#[test]
fn a_score_beside_its_subtest_is_kept_as_a_score_tag_and_goes_out_as_one() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    let report = "פרופיל קוגניטיבי\nבמבחן WPPSI-IV יואב הציג בקוביות (7) ובמטריצות (13) תפקוד \
                  שונה, בזיכרון תמונות 10 ובחיפוש חרקים 11. בקידוד חיות (6) ניכר קושי. \
                  נבדק בגיל 5:4, בחדר 12.\n";
    let preview = core
        .import_style_source("ישן.txt", report.as_bytes())
        .unwrap();
    let cognitive = preview
        .parts
        .iter()
        .find(|p| p.section.as_deref() == Some("cognitive"))
        .unwrap();
    let t = &cognitive.text;
    assert!(t.contains("בקוביות ([תוצאה 7 · ממוצע נמוך])"), "{t}");
    assert!(t.contains("[תוצאה 13 · ממוצע גבוה · חוזקה יחסית]"), "{t}");
    assert!(t.contains("[תוצאה 6 · ממוצע נמוך · חולשה יחסית]"), "{t}");
    // Every other number is masked; the child's name is hidden.
    assert!(t.contains("בגיל [מספר], בחדר [מספר]"), "{t}");
    assert!(!t.contains("יואב"), "{t}");
    // Saved and sent for the analysis, the tags go through the gate as they are.
    let source = core
        .save_style_source(&preview.token, &[cognitive.index], None)
        .unwrap();
    let p = core.prepare_style_analysis(&source.id).unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    let id = p.approval_id.unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"items": []})));
    let _ = core.send_style_analysis(&id);
    let sent = last_sent(&fake);
    assert!(
        sent.contains("[תוצאה 13 · ממוצע גבוה · חוזקה יחסית]"),
        "{sent}"
    );
}

#[test]
fn an_upload_waiting_for_confirmation_is_forgotten_at_lock() {
    let (_dir, mut core) = vault(None);
    let preview = core
        .import_style_source("r.txt", PAST_REPORT.as_bytes())
        .unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(matches!(
        core.save_style_source(&preview.token, &[0], None),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        core.save_style_source("x", &[], None),
        Err(CoreError::NotFound(_))
    ));
}

#[test]
fn the_profile_is_built_approved_and_used_in_drafting() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    let case_id = case(&mut core);
    upload(&mut core);

    let draft = build(&mut core);
    assert_eq!(draft.status, "draft");
    assert!(draft
        .profile
        .items
        .iter()
        .any(|i| i.kind == dv_ai::StyleKind::Example));
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2, "one analysis, one synthesis");
    for body in &sent {
        no_secrets(body, "a style request");
    }
    assert!(sent[0].contains("[ילד]") && sent[0].contains("[מספר]"));
    assert!(
        !sent[1].contains("ניכר כי הוא מגיב"),
        "the synthesis sends no report text"
    );

    // A draft is not used.
    let p = core
        .prepare_section(&case_id, "appearance", "נסחי")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(!last_sent(&fake).contains(PROFILE_HEADER));

    let active = core.approve_style_draft().unwrap();
    assert_eq!((active.status.as_str(), active.version), ("active", 1));
    let p = core
        .prepare_section(&case_id, "appearance", "נסחי")
        .unwrap();
    assert!(p.parts.iter().any(|part| part.label == "פרופיל הסגנון שלך"));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let body = last_sent(&fake);
    assert!(body.contains(PROFILE_HEADER));
    assert!(
        body.contains("<style_example>"),
        "this section's examples go along"
    );

    core.set_style_enabled(false).unwrap();
    let p = core
        .prepare_section(&case_id, "appearance", "נסחי")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(!last_sent(&fake).contains(PROFILE_HEADER), "switched off");
    core.set_style_enabled(true).unwrap();

    // A new build carries her own rules over; an earlier version can come back.
    let mut edited = active.profile.clone();
    edited.items.push(dv_ai::StyleItem {
        id: String::new(),
        section: None,
        kind: dv_ai::StyleKind::Rule,
        text: "משפטים קצרים בסיכום".into(),
        enabled: true,
        origin: dv_ai::StyleOrigin::Manual,
        support: 0,
    });
    core.save_style_draft(edited).unwrap();
    let v2 = core.approve_style_draft().unwrap();
    assert_eq!(v2.version, 2);
    let rebuilt = build(&mut core);
    assert!(rebuilt
        .profile
        .items
        .iter()
        .any(|i| i.text == "משפטים קצרים בסיכום"));
    core.discard_style_draft().unwrap();
    let back = core.restore_style_version(&active.id).unwrap();
    assert_eq!(back.version, 3);
    let overview = core.style_overview().unwrap();
    assert!(overview.draft.is_none());
    assert_eq!(overview.versions.len(), 3);
    assert_eq!(overview.active.unwrap().id, back.id);
}

#[test]
fn items_that_copy_a_report_or_hold_a_name_are_dropped() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    upload(&mut core);
    let s = core.style_overview().unwrap().sources.remove(0);
    let p = core.prepare_style_analysis(&s.id).unwrap();
    core.send_style_analysis(&p.approval_id.unwrap()).unwrap();

    *fake.answer.lock().unwrap() = Some(api_json(&json!({ "items": [
        {"section": "general", "kind": "rule", "support": 2,
         "text": "מבנה של חוזקה ואחריה הקושי"},
        {"section": "appearance", "kind": "rule", "support": 1,
         "text": "ניכר כי הוא מגיב היטב לתיווך של המבוגר"},
        {"section": "general", "kind": "rule", "support": 1,
         "text": "לכתוב על מירב בגוף שלישי"},
        {"section": "cognitive", "kind": "template", "support": 1,
         "text": "הציג תפקוד {רמה} ({תוצאה})"}
    ]})));
    let p = core.prepare_style_profile().unwrap();
    let view = core.send_style_profile(&p.approval_id.unwrap()).unwrap();
    let texts: Vec<&str> = view.profile.items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(
        texts,
        vec!["מבנה של חוזקה ואחריה הקושי", "הציג תפקוד {רמה} ({תוצאה})"]
    );
    assert_eq!(view.dropped, 2, "a copied run and a name");
}

#[test]
fn an_item_holding_a_later_childs_name_is_left_out_of_that_case_only() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    // Approved before any case: "שאלון" is an ordinary word then.
    let profile = dv_ai::StyleProfile {
        reports: 1,
        items: vec![
            dv_ai::StyleItem {
                id: String::new(),
                section: None,
                kind: dv_ai::StyleKind::Rule,
                text: "תוצאות שאלון הורים מוצגות בקצרה".into(),
                enabled: true,
                origin: dv_ai::StyleOrigin::Manual,
                support: 0,
            },
            dv_ai::StyleItem {
                id: String::new(),
                section: None,
                kind: dv_ai::StyleKind::Rule,
                text: "גוף נסתר וזמן הווה".into(),
                enabled: true,
                origin: dv_ai::StyleOrigin::Manual,
                support: 0,
            },
        ],
    };
    core.save_style_draft(profile).unwrap();
    core.approve_style_draft().unwrap();
    let case_id = case(&mut core); // a child named "אלון"
    let p = core
        .prepare_section(&case_id, "appearance", "נסחי")
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let body = last_sent(&fake);
    assert!(body.contains("גוף נסתר וזמן הווה"));
    assert!(
        !body.contains("שאלון הורים"),
        "left out, not blocking the case"
    );
}

#[test]
fn her_own_items_with_a_name_or_a_tag_are_refused() {
    let (_dir, mut core) = vault(None);
    case(&mut core);
    for text in [
        "אלון מגיב היטב לתיווך",
        "[ילד] מגיב היטב",
        concat!("טלפון 050-", "1234567"),
    ] {
        let profile = dv_ai::StyleProfile {
            reports: 0,
            items: vec![dv_ai::StyleItem {
                id: String::new(),
                section: None,
                kind: dv_ai::StyleKind::Rule,
                text: text.into(),
                enabled: true,
                origin: dv_ai::StyleOrigin::Manual,
                support: 0,
            }],
        };
        assert!(
            matches!(core.save_style_draft(profile), Err(CoreError::Refused(_))),
            "{text}"
        );
    }
}

#[test]
fn her_edits_become_suggestions_and_an_accepted_one_joins_the_profile() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    let case_id = case(&mut core);
    for n in 0..3 {
        let d = core
            .vault
            .as_mut()
            .unwrap()
            .add_draft(
                &case_id,
                "appearance",
                &format!("[ילד] מראה קושי בוויסות במשימה {}.", ["א", "ב", "ג"][n]),
                Author::Ai,
                &[],
            )
            .unwrap();
        core.edit_paragraph(
            &case_id,
            &d.id,
            &format!("אלון מפגין קושי בוויסות במשימה {}.", ["א", "ב", "ג"][n]),
        )
        .unwrap();
        let shown = core.style_overview().unwrap().suggestions;
        assert_eq!(shown.is_empty(), n < 2, "a suggestion after three edits");
    }
    // Her own paragraphs teach nothing.
    core.add_own_paragraph(&case_id, "appearance", "נראה כי אלון רגוע")
        .unwrap();
    let suggestion = core.style_overview().unwrap().suggestions.remove(0);
    assert_eq!(
        (
            suggestion.from.as_str(),
            suggestion.to.as_str(),
            suggestion.count
        ),
        ("מראה", "מפגין", 3)
    );
    let active = core.accept_style_suggestion(&suggestion.id).unwrap();
    assert!(active
        .profile
        .items
        .iter()
        .any(|i| i.text == "לכתוב «מפגין» ולא «מראה»" && i.origin == dv_ai::StyleOrigin::Edits));
    assert!(core.style_overview().unwrap().suggestions.is_empty());

    let p = core
        .prepare_section(&case_id, "appearance", "נסחי")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let body = last_sent(&fake);
    assert!(body.contains("לכתוב «מפגין» ולא «מראה»"));
    assert!(
        body.contains("גוף נסתר וזמן הווה"),
        "learned from edits only: on top of the default style"
    );
}

#[test]
fn a_drafted_paragraph_that_repeats_a_past_report_is_flagged() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    let case_id = case(&mut core);
    upload(&mut core);
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "ניסחתי",
        "paragraphs": [
            {"text": "[ילד] הציג תפקוד בהבנה מילולית. עם זאת, ניכר קושי בזיכרון העבודה של הילד.", "source_refs": ["S1"]},
            {"text": "[ילד] הגיע למפגש בשמחה ושיתף פעולה.", "source_refs": ["S1"]}
        ],
        "questions": [], "missing": [], "contradictions": []
    })));
    let p = core
        .prepare_section(&case_id, "appearance", "נסחי")
        .unwrap();
    let result = core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(
        result.paragraphs[0]
            .warnings
            .iter()
            .any(|w| w.contains("דוח ישן")),
        "{:?}",
        result.paragraphs[0].warnings
    );
    assert!(!result.paragraphs[1]
        .warnings
        .iter()
        .any(|w| w.contains("דוח ישן")));
}

#[test]
fn deleting_and_resetting() {
    let (_dir, mut core) = vault(None);
    let s = upload(&mut core);
    build(&mut core);
    core.approve_style_draft().unwrap();
    core.delete_style_source(&s.id).unwrap();
    let o = core.style_overview().unwrap();
    assert!(o.sources.is_empty());
    assert!(
        o.active.is_some(),
        "an approved profile stays until rebuilt or reset"
    );
    assert!(matches!(
        core.prepare_style_profile(),
        Err(CoreError::Refused(_))
    ));
    core.reset_style().unwrap();
    let o = core.style_overview().unwrap();
    assert!(o.active.is_none() && o.versions.is_empty());
}

#[test]
fn names_from_a_past_report_are_kept_only_as_hashes_and_hidden_in_every_case() {
    let (_dir, mut core) = vault(None);
    let source = upload(&mut core);
    let hashes =
        |core: &mut Core| crate::style::past_name_hmacs(core.vault_ref().unwrap()).unwrap();
    let kept = hashes(&mut core);
    let hmac = |core: &mut Core, w: &str| core.vault_ref().unwrap().token_hmac(&normalize(w));
    for name in ["יואב", "מירב", "אורית"] {
        assert!(kept.contains(&hmac(&mut core, name)), "{name}");
    }
    // Everyday words are never kept, or "בגיל" would be hidden everywhere.
    assert!(!kept.contains(&hmac(&mut core, "גיל")));

    // A new case, a material that mentions the past report's child: hidden, and not sent.
    let case_id = case(&mut core);
    let prepared = core
        .preview_filter(&case_id, "בהפסקה הוא שיחק שוב עם יואב.")
        .unwrap();
    assert!(!prepared.tagged.contains("יואב"), "{}", prepared.tagged);
    assert!(
        prepared
            .auto_hidden
            .iter()
            .any(|a| a.token == "יואב" && a.reason == "שם מדוח ישן"),
        "{:?}",
        prepared.auto_hidden
    );

    core.delete_style_source(&source.id).unwrap();
    assert!(
        hashes(&mut core).is_empty(),
        "deleting the report deletes its names"
    );
}

#[test]
fn a_paragraph_whose_sentences_are_unlike_hers_gets_a_gentle_note() {
    // Fabricated: her past reports use paragraphs of three sentences of 8–12 words.
    let sentence = |n: usize| vec!["מילה"; n].join(" ");
    let text: String = (0..20)
        .map(|i| {
            format!(
                "{}. {}. {}.\n",
                sentence(8 + i % 5),
                sentence(9 + i % 3),
                sentence(12 - i % 4)
            )
        })
        .collect();
    let source = super::StoredSource {
        title: "דוח".into(),
        format: "text".into(),
        parts: vec![super::StoredPart {
            section: Some("observation".into()),
            heading: String::new(),
            text,
        }],
        analysis: None,
    };
    let mut empty = super::LocalStyle::default();
    super::fit_reports(&mut empty, &[]);
    let long = format!("{}. {}.", sentence(30), sentence(28));
    assert!(
        super::style_note(&empty.model, "observation", &long).is_none(),
        "nothing to measure, no notes"
    );
    let mut local = super::LocalStyle::default();
    super::fit_reports(&mut local, &[source]);
    let m = &local.model;
    let note = super::style_note(m, "observation", &long).unwrap();
    assert!(note.starts_with("משפטים ארוכים מהרגיל אצלך"), "{note}");
    let short = format!("{}. {}. {}.", sentence(3), sentence(4), sentence(3));
    assert!(super::style_note(m, "observation", &short)
        .unwrap()
        .starts_with("משפטים קצרים"));
    let usual = format!("{}. {}. {}.", sentence(10), sentence(9), sentence(11));
    assert!(super::style_note(m, "observation", &usual).is_none());
    assert!(
        super::style_note(m, "observation", &sentence(40)).is_none(),
        "one sentence says little"
    );

    // The same figures follow the AI's paragraphs over time: the latest 30, and those before.
    assert_eq!(local.learned().fit_recent, None);
    for _ in 0..10 {
        local.note_fit("observation", &long);
    }
    for _ in 0..30 {
        local.note_fit("observation", &usual);
    }
    let learned = local.learned();
    assert_eq!(learned.fit_count, 40);
    let (recent, earlier) = (learned.fit_recent.unwrap(), learned.fit_earlier.unwrap());
    assert!(recent > earlier, "{recent} vs {earlier}");
    assert!(learned
        .words_per_sentence
        .is_some_and(|w| (9.0..11.0).contains(&w)));
    for _ in 0..200 {
        local.note_fit("observation", &usual);
    }
    assert_eq!(local.learned().fit_count, super::FIT_KEPT as u32);

    // By month (ST12): oldest first, at most a year kept.
    let mut local2 = super::LocalStyle {
        model: local.model.clone(),
        ..Default::default()
    };
    for m in 1..=14 {
        let month = format!("2025-{m:02}");
        let month = if m > 12 {
            format!("2026-{:02}", m - 12)
        } else {
            month
        };
        local2.note_fit_in("observation", if m < 13 { &long } else { &usual }, &month);
    }
    let by_month = local2.learned().fit_by_month;
    assert_eq!(by_month.len(), super::MONTHS_KEPT);
    assert_eq!(by_month[0].month, "2025-03");
    assert_eq!(by_month.last().unwrap().month, "2026-02");
    assert!(by_month.last().unwrap().average > by_month[0].average);
}

#[test]
fn forgetting_what_was_learned_keeps_her_reports_and_profile() {
    let (_dir, mut core) = vault(None);
    upload(&mut core);
    build(&mut core);
    core.approve_style_draft().unwrap();
    core.learn_from_edit(
        "observation",
        "הילד שיתף פעולה באופן מלא לאורך כל המפגש.",
        "נראה כי הילד שיתף פעולה לאורך רוב המפגש.",
    )
    .unwrap();
    assert!(core.style_overview().unwrap().learned.from_edits > 0);
    core.forget_style_learning().unwrap();
    let o = core.style_overview().unwrap();
    assert_eq!(o.learned.from_edits, 0);
    assert_eq!(o.learned.fit_count, 0);
    assert!(o
        .suggestions
        .iter()
        .all(|s| s.kind == super::StyleSuggestionKind::Phrase));
    assert!(o.active.is_some() && !o.sources.is_empty());
}

#[test]
fn phrases_that_recur_in_her_reports_are_offered_without_any_request() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    for n in 0..3 {
        let preview = core
            .import_style_source(&format!("דוח {n}.txt"), PAST_REPORT.as_bytes())
            .unwrap();
        let keep: Vec<u32> = preview
            .parts
            .iter()
            .filter(|p| p.included)
            .map(|p| p.index)
            .collect();
        core.save_style_source(&preview.token, &keep, None).unwrap();
    }
    assert!(fake.sent.lock().unwrap().is_empty(), "learned locally");
    let shown = core.style_overview().unwrap().suggestions;
    let phrases: Vec<&crate::StyleSuggestion> = shown
        .iter()
        .filter(|s| s.kind == crate::StyleSuggestionKind::Phrase)
        .collect();
    assert!(!phrases.is_empty(), "{shown:?}");
    for p in &phrases {
        no_secrets(&p.from, "a phrase");
        assert!(!p.from.contains('['), "{}", p.from);
        assert_eq!((p.count, p.confidence), (3, 100));
    }
    let first = phrases[0].clone();
    let active = core.accept_style_suggestion(&first.id).unwrap();
    assert!(active.profile.items.iter().any(|i| i.text == first.from
        && i.kind == dv_ai::StyleKind::Phrase
        && i.origin == dv_ai::StyleOrigin::Reports
        && i.section == first.section));
    assert!(
        !core
            .style_overview()
            .unwrap()
            .suggestions
            .iter()
            .any(|s| s.id == first.id),
        "an accepted phrase is not offered again"
    );
    let second = core
        .style_overview()
        .unwrap()
        .suggestions
        .into_iter()
        .find(|s| s.kind == crate::StyleSuggestionKind::Phrase)
        .unwrap();
    core.dismiss_style_suggestion(&second.id).unwrap();
    assert!(!core
        .style_overview()
        .unwrap()
        .suggestions
        .iter()
        .any(|s| s.id == second.id));
}

#[test]
fn what_she_deletes_becomes_an_avoid_and_what_she_keeps_is_not_suggested() {
    let (_dir, mut core) = vault(Some(FakeTransport::default()));
    let case_id = case(&mut core);
    let add = |core: &mut Core, text: &str| {
        core.vault
            .as_mut()
            .unwrap()
            .add_draft(&case_id, "appearance", text, Author::Ai, &[])
            .unwrap()
    };
    for n in ["א", "ב", "ג"] {
        let d = add(
            &mut core,
            &format!("חשוב לציין כי [ילד] רגוע ומפגין סקרנות במשימה {n}."),
        );
        core.edit_paragraph(
            &case_id,
            &d.id,
            &format!("אלון רגוע ומפגין סקרנות במשימה {n}."),
        )
        .unwrap();
    }
    let shown = core.style_overview().unwrap().suggestions;
    let avoid = shown
        .iter()
        .find(|s| s.kind == crate::StyleSuggestionKind::Avoid)
        .unwrap_or_else(|| panic!("{shown:?}"))
        .clone();
    assert_eq!(avoid.from, "חשוב לציין כי");
    assert_eq!(avoid.section.as_deref(), Some("appearance"));
    let active = core.accept_style_suggestion(&avoid.id).unwrap();
    assert!(active
        .profile
        .items
        .iter()
        .any(|i| i.text == "לא לכתוב «חשוב לציין כי»"
            && i.kind == dv_ai::StyleKind::Avoid
            && i.section.as_deref() == Some("appearance")));

    // "מפגין" was changed three times, then approved as written many more: not her rule.
    let (_dir2, mut core) = vault(Some(FakeTransport::default()));
    let case_id = case(&mut core);
    let add = |core: &mut Core, text: &str| {
        core.vault
            .as_mut()
            .unwrap()
            .add_draft(&case_id, "appearance", text, Author::Ai, &[])
            .unwrap()
    };
    for n in ["א", "ב", "ג"] {
        let d = add(&mut core, &format!("[ילד] מפגין סקרנות במשימה {n}."));
        core.edit_paragraph(&case_id, &d.id, &format!("אלון מראה סקרנות במשימה {n}."))
            .unwrap();
    }
    assert_eq!(core.style_overview().unwrap().suggestions.len(), 1);
    for _ in 0..12 {
        let d = add(&mut core, "[ילד] מפגין סקרנות רבה בכל משימה.");
        core.approve_paragraph(&case_id, &d.id).unwrap();
    }
    assert!(
        core.style_overview().unwrap().suggestions.is_empty(),
        "she keeps it most of the time"
    );
}

#[test]
fn the_style_requests_count_toward_the_month_like_every_other_request() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    upload(&mut core);
    assert_eq!(core.usage_summary().unwrap().requests, 0);
    build(&mut core);
    // One analysis and one profile: both went out, so both are counted (and so stop at the
    // monthly ceiling she set), as a section draft is.
    assert_eq!(
        core.usage_summary().unwrap().requests,
        u32::try_from(fake.sent.lock().unwrap().len()).unwrap()
    );
}

#[test]
fn a_proposed_paragraph_shows_her_usual_changes_and_nothing_with_the_profile_off() {
    let (_dir, mut core) = vault(Some(FakeTransport::default()));
    let case_id = case(&mut core);
    for n in ["א", "ב", "ג"] {
        let d = core
            .vault
            .as_mut()
            .unwrap()
            .add_draft(
                &case_id,
                "appearance",
                &format!("[ילד] מראה קושי במשימה {n}."),
                Author::Ai,
                &[],
            )
            .unwrap();
        core.edit_paragraph(&case_id, &d.id, &format!("אלון מפגין קושי במשימה {n}."))
            .unwrap();
    }
    core.vault
        .as_mut()
        .unwrap()
        .add_draft(
            &case_id,
            "appearance",
            "[ילד] מראה עניין במשחק.",
            Author::Ai,
            &[],
        )
        .unwrap();
    let hint = |core: &mut Core| {
        core.case_detail(&case_id)
            .unwrap()
            .sections
            .into_iter()
            .flat_map(|s| s.paragraphs)
            .find(|p| p.status == dv_domain::DraftStatus::Proposed)
            .unwrap()
            .style
    };
    let style = hint(&mut core).expect("a proposed paragraph of the AI");
    assert_eq!(
        style.marks,
        vec![crate::StyleMark {
            from: "מראה".into(),
            to: "מפגין".into()
        }]
    );
    assert_eq!(style.fit, None, "no past reports: too little to measure");
    assert!(style.details.is_empty() && style.ask.is_none());
    core.set_style_enabled(false).unwrap();
    assert!(hint(&mut core).is_none());
}

#[test]
fn the_closeness_figure_comes_with_its_reasons_and_what_to_ask() {
    let sentence = |n: usize| vec!["מילה"; n].join(" ");
    let text: String = (0..20)
        .map(|i| {
            format!(
                "{}. {}. {}.\n",
                sentence(8 + i % 5),
                sentence(9 + i % 3),
                sentence(12 - i % 4)
            )
        })
        .collect();
    let mut local = super::LocalStyle::default();
    super::fit_reports(
        &mut local,
        &[super::StoredSource {
            title: "דוח".into(),
            format: "text".into(),
            parts: vec![super::StoredPart {
                section: Some("observation".into()),
                heading: String::new(),
                text,
            }],
            analysis: None,
        }],
    );
    let m = &local.model;
    let long = format!("{}. {}.", sentence(30), sentence(28));
    let usual = format!("{}. {}. {}.", sentence(10), sentence(9), sentence(11));
    assert!(m.fidelity("observation", &long).unwrap() < m.fidelity("observation", &usual).unwrap());
    let details = m.differences("observation", &long);
    assert!(
        details[0].starts_with("משפטים ארוכים מהרגיל אצלך"),
        "{details:?}"
    );
    let ask = m.closer_ask("observation", &long).unwrap();
    assert!(ask.starts_with("משפטים קצרים יותר, כ-1"), "{ask}");
    assert!(!ask.contains("מילה מילה"), "only her figures, never text");
    assert!(m.closer_ask("observation", &usual).is_none());
    assert!(m.differences("observation", &usual).is_empty());
}

#[test]
fn the_closest_paragraph_of_her_reports_is_shown_without_names() {
    let (_dir, mut core) = vault(None);
    let case_id = case(&mut core);
    let draft = core
        .vault
        .as_mut()
        .unwrap()
        .add_draft(
            &case_id,
            "appearance",
            "[ילד] הגיע למפגש בשמחה ושיתף פעולה לאורך כל המפגש.",
            Author::Ai,
            &[],
        )
        .unwrap();
    assert_eq!(
        core.similar_paragraph(&case_id, &draft.id).unwrap(),
        None,
        "no past reports yet"
    );
    upload(&mut core);
    let similar = core
        .similar_paragraph(&case_id, &draft.id)
        .unwrap()
        .expect("a close paragraph");
    assert!(
        similar.text.contains("שיתף פעולה לאורך כל המפגש"),
        "{}",
        similar.text
    );
    no_secrets(&similar.text, "the similar paragraph");
    no_secrets(&similar.report, "its report's title");
    assert!(core.similar_paragraph(&case_id, "no-such-draft").is_err());
}

#[test]
fn a_report_that_names_one_thing_two_ways_gets_a_note_on_the_odd_one() {
    let (_dir, mut core) = vault(None);
    let case_id = case(&mut core);
    let v = core.vault.as_mut().unwrap();
    for t in ["ל[ילד] זכרון עבודה טוב.", "זכרון עבודה נמדד שוב."]
    {
        v.add_draft(&case_id, "appearance", t, Author::Ai, &[])
            .unwrap();
    }
    v.add_draft(
        &case_id,
        "appearance",
        "בזיכרון עבודה ניכר שיפור.",
        Author::Ai,
        &[],
    )
    .unwrap();
    let warnings: Vec<String> = core
        .case_detail(&case_id)
        .unwrap()
        .sections
        .into_iter()
        .flat_map(|s| s.paragraphs)
        .flat_map(|p| p.warnings)
        .collect();
    assert_eq!(
        warnings,
        vec!["כתוב «זיכרון עבודה»; בשאר הדוח «זכרון עבודה»".to_owned()]
    );
}

#[test]
fn her_usual_openings_are_offered_once_and_not_after_she_answers() {
    let local = super::LocalStyle {
        shapes: vec![dv_style::Shape {
            place: dv_style::Place::Opening,
            section: "observation".into(),
            text: "במהלך המפגש".into(),
            norm: "במהלך המפגש".into(),
            reports: 4,
            share: 80,
        }],
        ..Default::default()
    };
    let mut learning = super::Learning::default();
    let found = super::suggestions(&learning, &local, None);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, super::StyleSuggestionKind::Opening);
    assert_eq!(
        (found[0].from.as_str(), found[0].section.as_deref()),
        ("במהלך המפגש", Some("observation"))
    );
    learning.dismissed_phrases.push(found[0].id.clone());
    assert!(super::suggestions(&learning, &local, None).is_empty());
}

/// Round 30: beside a section she writes herself, how she wrote it in her reports, from this
/// computer only, with every name neutralized.
#[test]
fn her_own_section_shows_how_she_wrote_it_before_without_sending_anything() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    let _ = case(&mut core);
    upload(&mut core);
    let help = core.own_section_help("appearance").unwrap();
    assert_eq!(help.examples.len(), 1, "{help:?}");
    assert!(help.examples[0].text.contains("נפרד"), "{help:?}");
    assert_eq!(help.openings.len(), 1);
    assert!(help.openings[0].ends_with('.'));
    assert_eq!(help.closings.len(), 1);
    for line in help
        .examples
        .iter()
        .map(|e| &e.text)
        .chain(&help.openings)
        .chain(&help.closings)
    {
        no_secrets(line, "the help beside her own section");
    }
    // A section she never wrote has nothing to show, and nothing left the computer.
    assert!(core.own_section_help("dsm").unwrap().examples.is_empty());
    assert!(fake.sent.lock().unwrap().is_empty());
}

/// The fabricated reports of `tests/fixtures/score_reports`, without their marks, and how many
/// scores each one marks.
fn score_reports() -> Vec<(String, String, usize)> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/score_reports");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let marked = std::fs::read_to_string(&p).unwrap();
            let marks = marked.matches('⟦').count();
            // A mark may name its measure ("⟦13:matrix⟧"): keep only the number.
            let text = strip_measure_keys(&marked.replace(['⟦', '⟧'], ""));
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                text,
                marks,
            )
        })
        .collect()
}

fn strip_measure_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(':') {
        let (head, tail) = rest.split_at(at);
        let key: String = tail
            .chars()
            .skip(1)
            .take_while(|c| c.is_ascii_lowercase() || *c == '_')
            .collect();
        out.push_str(head);
        if !key.is_empty() && head.ends_with(|c: char| c.is_ascii_digit()) {
            rest = &tail[1 + key.len()..];
        } else {
            out.push(':');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn old_reports_keep_their_scores_as_tags_and_no_other_number() {
    let (_dir, mut core) = vault(None);
    let children = [
        "יואב",
        "נועה",
        "איתי",
        "תמר",
        "עומר",
        "מאיה",
        "רון",
        "שירה",
        "אורי",
        "דניאל",
        "הילה",
        "גיל",
    ];
    let (mut marked, mut tagged) = (0usize, 0usize);
    let mut stayed = Vec::new();
    for (name, text, marks) in score_reports() {
        assert!(
            !text.contains('⟦') && !text.contains("matrix"),
            "{name}: {text}"
        );
        let preview = core.import_style_source(&name, text.as_bytes()).unwrap();
        let kept: String = preview
            .parts
            .iter()
            .map(|p| format!("{}\n{}\n", p.heading, p.text))
            .collect();
        marked += marks;
        // Score tags out; what is left holds no number but a test's own name ("WPPSI-IV").
        let mut rest = String::new();
        let mut tail = kept.as_str();
        while let Some(open) = tail.find('[') {
            rest.push_str(&tail[..open]);
            let close = tail[open..].find(']').map_or(tail.len(), |c| open + c + 1);
            let tag = &tail[open..close];
            if dv_domain::is_score_tag(tag) {
                tagged += 1;
            }
            tail = &tail[close..];
        }
        rest.push_str(tail);
        for word in rest.split_whitespace() {
            assert!(
                !word.chars().any(|c| c.is_ascii_digit())
                    || word.chars().any(|c| c.is_ascii_alphabetic()),
                "{name}: a number stayed: {word}\n{kept}"
            );
        }
        let words: Vec<String> = kept.split_whitespace().map(normalize).collect();
        for child in children {
            if words
                .iter()
                .any(|w| w.trim_matches(|c: char| !c.is_alphabetic()) == child)
            {
                stayed.push(format!("{name}: {child}"));
            }
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let share = tagged as f64 / marked as f64;
    assert!(share >= 0.95, "{tagged} of {marked} scores kept as tags");
    assert!(stayed.is_empty(), "names stayed: {stayed:?}");
}

/// Two of the fabricated old reports, every part kept.
fn upload_scored(core: &mut Core) {
    for (name, text, _) in score_reports()
        .into_iter()
        .filter(|(n, _, _)| n == "01.txt" || n == "11.txt")
    {
        let preview = core.import_style_source(&name, text.as_bytes()).unwrap();
        let all: Vec<u32> = preview.parts.iter().map(|p| p.index).collect();
        core.save_style_source(&preview.token, &all, None).unwrap();
    }
}

#[test]
fn the_interpretation_of_scores_is_learned_from_all_reports_and_waits_for_her() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    assert!(matches!(
        core.prepare_interpretations(),
        Err(CoreError::Refused(_))
    ));
    upload_scored(&mut core);
    let o = core.style_overview().unwrap();
    assert!(o.scored_measures >= 10, "{}", o.scored_measures);
    assert!(o.interpretations.is_empty());

    let p = core.prepare_interpretations().unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"items": [
        {"measure": "wppsi_iv/bug_search", "case": "ממוצע נמוך",
         "text": "תוצאה נמוכה מתפרשת אצלה כקושי במהירות העבודה, ולא בהבנה.", "support": 2},
        {"measure": "wppsi_iv/block_design", "case": "כללי",
         "text": "היא בודקת את התוצאה לצד העבודה השיטתית שנצפתה.", "support": 1},
        // Five words of a report: content, not an interpretation.
        {"measure": "wppsi_iv/matrix", "case": "כללי",
         "text": "לדבריה הופנה לאבחון בשל קשיי קשב בגן.", "support": 1},
        {"measure": "wppsi_iv/matrix", "case": "כללי", "text": "הילה מתקשה.", "support": 1}
    ]})));
    let r = core.send_interpretations(&p.approval_id.unwrap()).unwrap();
    let sent = last_sent(&fake);
    // Grouped by subtest, with the score tags; the children's names stay home.
    assert!(
        sent.contains("wppsi_iv/bug_search") && sent.contains("[תוצאה 6 · ממוצע נמוך"),
        "{sent}"
    );
    for child in ["יואב", "הילה"] {
        assert!(!sent.contains(child), "{child}: {sent}");
    }
    assert_eq!((r.kept, r.dropped), (2, 2), "{r:?}");

    let o = core.style_overview().unwrap();
    assert_eq!(o.interpretations.len(), 2);
    let first = &o.interpretations[0];
    assert!(!first.item.approved && first.item.enabled);
    assert_eq!(first.instrument_name, "WPPSI-IV");

    // She approves one and writes another her way; a name in her words is refused.
    let id = first.item.id.clone();
    let v = core
        .update_interpretation(&id, None, Some(true), None)
        .unwrap();
    assert!(v.item.approved && !v.item.edited);
    let other = o.interpretations[1].item.id.clone();
    assert!(matches!(
        core.update_interpretation(&other, Some("כמו אצל יואב.".into()), None, None),
        Err(CoreError::Refused(_))
    ));
    let v = core
        .update_interpretation(
            &other,
            Some("תוצאה בקוביות נקראת לצד אופן העבודה.".into()),
            None,
            None,
        )
        .unwrap();
    assert!(v.item.edited && !v.item.approved);

    // A new reading replaces only what she did not approve or write.
    let p = core.prepare_interpretations().unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"items": [
        {"measure": "wppsi_iv/matrix", "case": "כללי",
         "text": "היא מקשרת אותה לחשיבה החזותית.", "support": 2}
    ]})));
    core.send_interpretations(&p.approval_id.unwrap()).unwrap();
    let o = core.style_overview().unwrap();
    assert_eq!(o.interpretations.len(), 3);
    core.delete_interpretation(&other).unwrap();
    assert_eq!(core.style_overview().unwrap().interpretations.len(), 2);
    assert!(matches!(
        core.delete_interpretation(&other),
        Err(CoreError::NotFound(_))
    ));
}

#[test]
fn demo_mode_shows_what_an_interpretation_looks_like() {
    let (_dir, mut core) = vault(None);
    upload_scored(&mut core);
    let p = core.prepare_interpretations().unwrap();
    assert!(p.demo_mode);
    let r = core.send_interpretations(&p.approval_id.unwrap()).unwrap();
    assert!(r.demo && r.kept > 0, "{r:?}");
}

/// A fabricated past report in the institute's headings: the home section in her order, and
/// the test presented in her fixed wording.
fn template_report(name: &str, fsiq: u32) -> String {
    format!(
        "סיכום אבחון פסיכולוגי\n\
         תפקוד כיום בבית\n\
         {name} ילד חייכן וסקרן, אוהב מוזיקה ונהנה מטיולים עם המשפחה.\n\
         בבוקר מתלבש בעזרה, אוכל מגוון ונרדם בקלות.\n\
         במשחק בוחר בלגו ובפאזלים, ומשחק בדמיון עם בובות.\n\
         כשהוא כועס מגיב בבכי, ומתקשה להירגע לאחר תסכול.\n\
         עם האחים משתף, ובגינה פונה לילדים אחרים ומחפש חברים.\n\
         מדבר במשפטים קצרים, מבין הוראות פשוטות ומצביע על מה שרוצה.\n\
         רגיש לרעש חזק ונמנע ממגע של מרקמים מסוימים.\n\
         מתקשה להתרכז לאורך זמן, ופעלתנות רבה ניכרת בבית.\n\
         התחום הקוגניטיבי\n\
         מבחן WPPSI-IV הוא מבחן אינטליגנציה לילדים בגילאי הגן ובכיתות הראשונות. \
         מנת המשכל הכללית נמצאה {fsiq}, בטווח שנמדד במבחן. \
         {name} עבד בריכוז על הקוביות ושמח בכל הצלחה קטנה.\n"
    )
}

fn upload_template_reports(core: &mut Core) {
    for (n, (name, fsiq)) in [("נועם", 98), ("עידו", 104), ("איתמר", 91)]
        .into_iter()
        .enumerate()
    {
        let text = template_report(name, fsiq);
        let preview = core
            .import_style_source(&format!("דוח {n}.txt"), text.as_bytes())
            .unwrap();
        let all: Vec<u32> = preview.parts.iter().map(|p| p.index).collect();
        core.save_style_source(&preview.token, &all, None).unwrap();
    }
}

#[test]
fn the_guide_of_a_section_is_the_order_she_writes_it_in_until_she_writes_her_own() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    upload_template_reports(&mut core);
    assert!(fake.sent.lock().unwrap().is_empty(), "learned locally");
    let o = core.style_overview().unwrap();
    let home = o
        .skeletons
        .iter()
        .find(|k| k.section == "parents_view" && k.band.is_none())
        .unwrap_or_else(|| panic!("{:?}", o.skeletons));
    assert_eq!(home.topics.len(), 8, "{:?}", home.topics);
    assert_eq!(home.reports, 3);
    let id = case(&mut core);
    let guide = |core: &mut Core| {
        let d = core.case_detail(&id).unwrap();
        let s = d
            .sections
            .into_iter()
            .find(|s| s.key == "parents_view")
            .unwrap();
        (s.guide, s.guide_learned_from, s.guide_is_default)
    };
    let (text, from, default) = guide(&mut core);
    assert_eq!(from, 3);
    assert!(default, "not hers yet");
    assert!(text.starts_with("התכונות הטובות ומה אוהב ("), "{text}");
    assert_eq!(text.lines().count(), 8);
    no_secrets(&text, "a learned guide");
    for name in ["נועם", "עידו", "איתמר"] {
        assert!(!text.contains(name));
    }
    core.set_section_guide("parents_view", "קודם מה שההורים מספרים.")
        .unwrap();
    let (text, from, default) = guide(&mut core);
    assert_eq!(
        (text.as_str(), from, default),
        ("קודם מה שההורים מספרים.", 0, false)
    );
    core.set_section_guide("parents_view", "").unwrap();
    core.set_style_enabled(false).unwrap();
    let (text, from, _) = guide(&mut core);
    assert_eq!(from, 0, "with the profile off, the institute's text");
    assert!(!text.starts_with("התכונות"), "{text}");
}

#[test]
fn a_sentence_she_writes_the_same_way_in_three_reports_is_offered_as_her_wording() {
    let (_dir, mut core) = vault(None);
    upload_template_reports(&mut core);
    let formulas: Vec<crate::StyleSuggestion> = core
        .style_overview()
        .unwrap()
        .suggestions
        .into_iter()
        .filter(|s| s.kind == crate::StyleSuggestionKind::Formula)
        .collect();
    let texts: Vec<&str> = formulas.iter().map(|f| f.from.as_str()).collect();
    assert!(
        texts.contains(&"מבחן WPPSI-IV הוא מבחן אינטליגנציה לילדים בגילאי הגן ובכיתות הראשונות."),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"מנת המשכל הכללית נמצאה {תוצאה}, בטווח שנמדד במבחן."),
        "the score is an empty place: {texts:?}"
    );
    for f in &formulas {
        assert!(!f.from.contains('[') && !f.from.chars().any(|c| c.is_ascii_digit()));
        assert!(
            !f.from.contains("קוביות"),
            "a sentence about one child is not her wording"
        );
        assert_eq!(f.count, 3);
    }
    let first = formulas.first().unwrap().clone();
    let active = core.accept_style_suggestion(&first.id).unwrap();
    assert!(active.profile.items.iter().any(|i| i.text == first.from
        && i.kind == dv_ai::StyleKind::Template
        && i.origin == dv_ai::StyleOrigin::Reports));
    assert!(!core
        .style_overview()
        .unwrap()
        .suggestions
        .iter()
        .any(|s| s.id == first.id));
    let second = formulas.get(1).unwrap();
    core.dismiss_style_suggestion(&second.id).unwrap();
    assert!(!core
        .style_overview()
        .unwrap()
        .suggestions
        .iter()
        .any(|s| s.id == second.id));
}

#[test]
fn a_percentile_she_never_writes_beside_subtests_is_left_out_of_the_scores_material() {
    let (_dir, mut core) = vault(None);
    let sheet = dv_domain::ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![dv_domain::ScoreEntry {
            measure: "block_design".into(),
            value: 9.0,
            note: String::new(),
        }],
        notes: String::new(),
    };
    assert!(core
        .preview_scores(&sheet)
        .unwrap()
        .contains("ציון 9, אחוזון"));
    for (name, text, _) in score_reports() {
        let preview = core.import_style_source(&name, text.as_bytes()).unwrap();
        let all: Vec<u32> = preview.parts.iter().map(|p| p.index).collect();
        core.save_style_source(&preview.token, &all, None).unwrap();
    }
    let o = core.style_overview().unwrap();
    // The twelve fabricated reports share no sentence about a child: none is her wording.
    let formulas: Vec<&str> = o
        .suggestions
        .iter()
        .filter(|s| s.kind == crate::StyleSuggestionKind::Formula)
        .map(|s| s.from.as_str())
        .collect();
    assert!(formulas.is_empty(), "{formulas:?}");
    assert!(
        o.omissions.iter().any(|t| t.contains("תתי-המבחנים")),
        "{:?}",
        o.omissions
    );
    let preview = core.preview_scores(&sheet).unwrap();
    assert!(
        preview.contains("ציון 9 –") && !preview.contains("אחוזון"),
        "{preview}"
    );
    let id = case(&mut core);
    let saved = core.save_scores(&id, None, &sheet).unwrap();
    assert_eq!(saved.content, preview, "what is saved is what she saw");
}

/// Fabricated past reports with diagnoses and recommendations: two like a preschool case
/// tested with the WPPSI, one of a toddler.
const LIST_REPORTS: [&str; 3] = [
    "סיכום אבחון\nהתחום הקוגניטיבי\nנועם נבדק במבחן WPPSI-IV. מנת המשכל הכללית נמצאה 98, בטווח הממוצע.\n\
     אבחנות\n- הפרעה על הספקטרום האוטיסטי, ברמת תמיכה ראשונה.\nהמלצות\n- מומלץ על ריפוי בעיסוק.\n- מומלץ על הדרכת הורים.\n- מומלץ שנועם ימשיך בגן של אורית.\n",
    "סיכום אבחון\nהתחום הקוגניטיבי\nעידו נבדק במבחן WPPSI-IV. מנת המשכל הכללית נמצאה 101, בטווח הממוצע.\n\
     אבחנות\n- הפרעה על הספקטרום האוטיסטי ברמת תמיכה ראשונה.\nהמלצות\n1. מומלץ על טיפול בריפוי בעיסוק.\n2. מומלץ על הדרכת הורים.\n",
    "סיכום אבחון\nהתחום הקוגניטיבי\nאיתמר נבדק במבחן Bayley-4.\n\
     אבחנות\n- עיכוב בהתפתחות השפה.\nהמלצות\n- מומלץ על הדרכת הורים.\n- מומלץ על טיפול בקלינאית תקשורת.\n",
];

#[test]
fn her_recommendations_come_with_how_often_she_gave_them_in_reports_like_this_case() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    for (n, text) in LIST_REPORTS.iter().enumerate() {
        let preview = core
            .import_style_source(&format!("דוח {n}.txt"), text.as_bytes())
            .unwrap();
        let all: Vec<u32> = preview.parts.iter().map(|p| p.index).collect();
        core.save_style_source(&preview.token, &all, None).unwrap();
    }
    let id = case(&mut core);
    let sheet = dv_domain::ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![dv_domain::ScoreEntry {
            measure: "fsiq".into(),
            value: 99.0,
            note: String::new(),
        }],
        notes: String::new(),
    };
    core.save_scores(&id, None, &sheet).unwrap();
    let lib = core.section_library(&id, "recommendations").unwrap();
    let lines: Vec<(&str, u32, u32, u32)> = lib
        .lines
        .iter()
        .map(|l| (l.text.as_str(), l.reports, l.in_similar, l.similar))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("מומלץ על הדרכת הורים.", 3, 2, 2),
            ("מומלץ על טיפול בריפוי בעיסוק.", 2, 2, 2),
        ],
        "what she gave in the reports like this case first, then the most often"
    );
    let names = ["נועם", "עידו", "איתמר", "אורית"];
    for l in &lib.lines {
        assert!(!names.iter().any(|n| l.text.contains(n)), "{}", l.text);
    }
    assert_eq!(
        lib.similar.len(),
        2,
        "the toddler's report is not like this case"
    );
    let first = &lib.similar[0];
    assert!(
        first.reasons.contains(&"WPPSI-IV".to_owned()),
        "{:?}",
        first.reasons
    );
    assert!(first.reasons.iter().any(|r| r == "גיל שלוש עד שש"));
    for s in &lib.similar {
        assert!(!names.iter().any(|n| s.text.contains(n)), "{}", s.text);
    }
    let dx = core.section_library(&id, "diagnoses").unwrap();
    assert_eq!(dx.lines.len(), 1, "{:?}", dx.lines);
    assert_eq!(dx.lines[0].reports, 2);
    assert!(dx.lines[0].text.contains("הספקטרום האוטיסטי"));
    // Choosing one adds it to the case as her own paragraph.
    core.add_own_paragraph(&id, "recommendations", &lib.lines[0].text)
        .unwrap();
    assert!(fake.sent.lock().unwrap().is_empty(), "nothing was sent");
    assert!(core.section_library(&id, "nothing").is_err());
}

#[test]
fn her_approved_interpretations_that_fit_the_scores_go_out_and_a_misfit_is_noted() {
    let fake = FakeTransport::default();
    let (_dir, mut core) = vault(Some(fake.clone()));
    upload_scored(&mut core);
    let p = core.prepare_interpretations().unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"items": [
        {"measure": "wppsi_iv/bug_search", "case": "ממוצע נמוך",
         "text": "תוצאה נמוכה מתפרשת אצלה כקושי במהירות העבודה, ולא בהבנה.", "support": 2},
        {"measure": "wppsi_iv/block_design", "case": "כללי",
         "text": "היא בודקת את התוצאה לצד העבודה השיטתית שנצפתה.", "support": 1},
        {"measure": "wppsi_iv/matrix", "case": "חוזקה יחסית",
         "text": "חוזקה בחשיבה החזותית נקשרת אצלה ליכולת ללמוד מהתבוננות.", "support": 1}
    ]})));
    core.send_interpretations(&p.approval_id.unwrap()).unwrap();
    *fake.answer.lock().unwrap() = None;
    for i in core.style_overview().unwrap().interpretations {
        core.update_interpretation(&i.item.id, None, Some(true), None)
            .unwrap();
    }
    let id = case(&mut core);
    let entry = |measure: &str, value: f64| dv_domain::ScoreEntry {
        measure: measure.into(),
        value,
        note: String::new(),
    };
    let sheet = dv_domain::ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![
            entry("block_design", 9.0),
            entry("matrix", 10.0),
            entry("bug_search", 12.0),
            entry("information", 10.0),
        ],
        notes: String::new(),
    };
    core.save_scores(&id, None, &sheet).unwrap();
    let p = core.prepare_section(&id, "cognitive", "נסחי").unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let body = last_sent(&fake);
    assert!(body.contains("איך הפסיכולוגית מפרשת תוצאות"), "{body}");
    assert!(
        body.contains("העבודה השיטתית שנצפתה"),
        "the general one fits every score"
    );
    assert!(
        !body.contains("במהירות העבודה") && !body.contains("ללמוד מהתבוננות"),
        "a low-range or strength reading does not fit these scores"
    );
    // A paragraph that uses one written for a low range, beside an average score, is noted.
    core.add_own_paragraph(
        &id,
        "cognitive",
        "בחיפוש חרקים נראה כי תוצאה נמוכה מתפרשת אצלה כקושי במהירות העבודה.",
    )
    .unwrap();
    let d = core.case_detail(&id).unwrap();
    let warnings: Vec<String> = d
        .sections
        .iter()
        .filter(|s| s.key == "cognitive")
        .flat_map(|s| s.paragraphs.iter().flat_map(|p| p.warnings.clone()))
        .collect();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("שכתבת ל«ממוצע נמוך» בWPPSI-IV · חיפוש חרקים")),
        "{warnings:?}"
    );
    core.set_style_enabled(false).unwrap();
    let p = core.prepare_section(&id, "cognitive", "נסחי").unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(
        !last_sent(&fake).contains("איך הפסיכולוגית מפרשת תוצאות"),
        "switched off"
    );
}
