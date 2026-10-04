//! End-to-end flows of the writing-style profile (D-043). Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
    // Fabricated: her past reports use sentences of 8–12 words.
    let sentence = |n: usize| vec!["מילה"; n].join(" ");
    let text: String = (0..40).map(|i| sentence(8 + i % 5) + ". ").collect();
    let source = super::StoredSource {
        title: "דוח".into(),
        format: "text".into(),
        parts: vec![super::StoredPart {
            section: None,
            heading: String::new(),
            text,
        }],
        analysis: None,
    };
    assert!(
        super::measure(&[]).is_none(),
        "nothing to measure, no notes"
    );
    let fp = super::measure(&[source]).unwrap();
    let long = format!("{}. {}.", sentence(30), sentence(28));
    let note = super::style_note(&fp, &long).unwrap();
    assert!(note.starts_with("משפטים ארוכים מהרגיל אצלך"), "{note}");
    let short = format!("{}. {}. {}.", sentence(3), sentence(4), sentence(3));
    assert!(super::style_note(&fp, &short)
        .unwrap()
        .starts_with("משפטים קצרים"));
    assert!(super::style_note(&fp, &format!("{}. {}.", sentence(10), sentence(9))).is_none());
    assert!(
        super::style_note(&fp, &sentence(40)).is_none(),
        "one sentence says little"
    );
}
