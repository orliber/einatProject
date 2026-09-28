//! End-to-end flows through `Core`. Fabricated data only; nothing leaves the machine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use dv_domain::{
    Age, CaseMeta, Consent, DraftStatus, GrammaticalGender, IdentityInput, InputKind, Role,
};
use dv_egress::{EgressError, Transport};
use dv_privacy::ClearedPayload;
use serde_json::{json, Value};

use super::*;

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

/// Records every payload and answers like the Messages API.
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
        let parsed: Value = serde_json::from_str(&body).unwrap();
        Ok(dv_ai::demo::respond(&parsed))
    }
}

fn api_json(v: &Value) -> Value {
    json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": v.to_string() }] })
}

fn consent() -> Consent {
    Consent {
        given_on: "2026-09-01".into(),
        form_version: "v1".into(),
        given_by: "שני ההורים".into(),
    }
}

fn setup(transport: Option<FakeTransport>) -> (tempfile::TempDir, Core, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(
        dir.path(),
        transport.map(|t| Box::new(t) as Box<dyn Transport>),
    );
    let created = core.create_vault(PASSWORD).unwrap();
    assert!(!created.recovery_key.is_empty());
    core.set_practitioner(vec!["ד\"ר רותם אלמוג".into()])
        .unwrap();
    let meta = CaseMeta {
        code: "TEST-0002".into(),
        age: Some(Age {
            years: 5,
            months: 4,
        }),
        child_gender: Some(GrammaticalGender::Male),
        consent: Some(consent()),
        ..CaseMeta::default()
    };
    let case = core
        .create_case(
            meta,
            vec![
                IdentityInput {
                    id: None,
                    role: Role::Child,
                    value: "אלון".into(),
                    aliases: vec![],
                },
                IdentityInput {
                    id: None,
                    role: Role::Teacher,
                    value: "שירה".into(),
                    aliases: vec![],
                },
            ],
        )
        .unwrap();
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "שיחה עם הגננת",
        "שירה סיפרה כי אלון מתקשה במעברים בין פעילויות. בבוקר הוא נפרד בבכי.",
    )
    .unwrap();
    core.add_input(
        &case,
        InputKind::Intake,
        "אינטייק עם ההורים",
        "ההורים מתארים כי אלון נרדם באיחור ומתעורר פעמיים בלילה.",
    )
    .unwrap();
    (dir, core, case)
}

#[test]
fn ping_reports_versions() {
    let p = ping();
    assert_eq!(p.ipc_version, IPC_VERSION);
    assert!(!p.core_version.is_empty());
}

#[test]
fn model_allow_lists_agree() {
    assert_eq!(dv_ai::ALLOWED_MODELS, dv_egress::ALLOWED_MODELS);
    assert!(dv_ai::ALLOWED_MODELS.contains(&dv_ai::DEFAULT_MODEL));
}

#[test]
fn full_section_flow_sends_only_tags_and_stores_tagged() {
    let fake = FakeTransport::default();
    let (dir, mut core, case) = setup(Some(fake.clone()));

    let prepared = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה על הוויסות הרגשי")
        .unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    assert!(prepared.suspects.is_empty(), "{:?}", prepared.suspects);
    assert!(!prepared.hidden.is_empty());
    let approval = prepared.approval_id.clone().expect("approved payload");

    let result = core.send_section(&approval).unwrap();
    assert!(!result.paragraphs.is_empty());

    // What left the machine: tags, never names.
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    for name in ["אלון", "שירה", "רותם", "אלמוג"] {
        assert!(!sent[0].contains(name), "{name} left the machine");
    }
    assert!(sent[0].contains("[ילד]"));

    // Shown to the psychologist with names restored; stored tagged.
    let detail = core.case_detail(&case).unwrap();
    let section = detail
        .sections
        .iter()
        .find(|s| s.key == "kindergarten")
        .unwrap();
    assert!(!section.paragraphs.is_empty());
    assert!(section
        .paragraphs
        .iter()
        .all(|p| p.status == DraftStatus::Proposed && p.by_ai));
    assert!(section.paragraphs.iter().any(|p| p.text.contains("אלון")));
    assert!(section.paragraphs.iter().all(|p| !p.sources.is_empty()));

    // The same approval cannot be used twice.
    assert!(matches!(
        core.send_section(&approval),
        Err(CoreError::NotFound(_))
    ));

    // On disk nothing is readable.
    core.lock();
    for entry in std::fs::read_dir(dir.path()).unwrap().flatten() {
        let bytes = std::fs::read(entry.path()).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("אלון") && !text.contains("מתקשה"),
            "{:?}",
            entry.path()
        );
    }
}

#[test]
fn demo_mode_without_key_or_transport() {
    let (_dir, mut core, case) = setup(None);
    assert!(core.status().demo_mode);
    let prepared = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(prepared.demo_mode);
    let result = core.send_section(&prepared.approval_id.unwrap()).unwrap();
    assert!(result.demo);
    let chat = core.chat(&case, "kindergarten").unwrap();
    assert_eq!(chat.len(), 2);
    assert!(chat.iter().all(|m| m.demo));
}

#[test]
fn no_consent_no_sending() {
    let (_dir, mut core, case) = setup(None);
    let mut meta = core.case_detail(&case).unwrap().meta;
    meta.consent = None;
    core.update_case(&case, meta).unwrap();
    assert!(matches!(
        core.prepare_section(&case, "kindergarten", "טיוטה"),
        Err(CoreError::ConsentMissing)
    ));
    assert!(matches!(
        core.prepare_consult(Some(&case), "שאלה"),
        Err(CoreError::ConsentMissing)
    ));
    // A general consultation (no case) does not need a case's consent.
    assert!(core
        .prepare_consult(None, "מה ההבדל בין WPPSI-IV ל-WISC-V?")
        .unwrap()
        .approval_id
        .is_some());
}

#[test]
fn unknown_name_blocks_until_decided() {
    let (_dir, mut core, case) = setup(None);
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "שיחה נוספת",
        "הסייעת ורד אמרה שהוא משחק לבד בחצר.",
    )
    .unwrap();
    let prepared = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(prepared.approval_id.is_none());
    let suspect = prepared
        .suspects
        .iter()
        .find(|s| s.token.contains("ורד"))
        .expect("suspect");

    core.decide_suspect(
        &case,
        &suspect.token,
        SuspectDecision::Hide {
            role: Role::Assistant,
        },
    )
    .unwrap();
    let again = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(again.suspects.is_empty(), "{:?}", again.suspects);
    assert!(again.approval_id.is_some());
}

#[test]
fn manual_edit_with_new_name_goes_through_review() {
    let (_dir, mut core, case) = setup(None);
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let para = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs
        .remove(0);
    core.edit_paragraph(&case, &para.id, "לפי הסבתא זהבה, אלון רגיש לרעש.")
        .unwrap();

    let next = core
        .prepare_section(&case, "kindergarten", "שפר/י את הניסוח")
        .unwrap();
    assert!(
        next.approval_id.is_none(),
        "an unreviewed name in a manual edit must stop the send"
    );
    assert!(next.suspects.iter().any(|s| s.token.contains("זהבה")));
}

#[test]
fn model_answer_with_unknown_source_and_number_is_flagged() {
    let fake = FakeTransport::default();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה טיוטה.",
        "paragraphs": [
            { "text": "[ילד] מתקשה במעברים בין פעילויות.", "source_refs": ["S1"] },
            { "text": "[ילד] קיבל ציון 85 במבחן.", "source_refs": ["S1"] },
            { "text": "[ילד] אוהב לצייר.", "source_refs": ["S9"] }
        ],
        "questions": [], "missing": [], "contradictions": []
    })));
    let (_dir, mut core, case) = setup(Some(fake));
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    let r = core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(
        r.paragraphs[0].warnings.is_empty(),
        "{:?}",
        r.paragraphs[0].warnings
    );
    assert!(!r.paragraphs[1].warnings.is_empty(), "85 is not in S1");
    assert!(!r.paragraphs[2].warnings.is_empty(), "S9 does not exist");
    assert!(r.paragraphs[0].text.contains("אלון"));
}

#[test]
fn approved_sections_feed_derived_sections() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "kindergarten", "אלון מגיב בעוצמה למעברים.")
        .unwrap();
    let p = core
        .prepare_section(&case, "summary", "טיוטה לסיכום")
        .unwrap();
    assert!(p.parts.iter().any(|part| part.label.contains("סעיף מאושר")));
    assert!(
        p.blocked.is_empty() && p.suspects.is_empty(),
        "{:?} {:?}",
        p.blocked,
        p.suspects
    );
    assert!(p.approval_id.is_some());
}

#[test]
fn consultation_keeps_history_until_lock() {
    let (_dir, mut core, case) = setup(None);
    let p = core
        .prepare_consult(Some(&case), "איך כדאי לנסח המלצה על ליווי רגשי לאלון?")
        .unwrap();
    assert!(!p.hidden.is_empty());
    let r = core.send_consult(&p.approval_id.unwrap()).unwrap();
    assert!(r.demo && !r.answer.is_empty());
    core.lock();
    assert!(matches!(core.list_cases(), Err(CoreError::Locked)));
}

#[test]
fn unlock_backoff_after_wrong_password() {
    let (dir, mut core, _case) = setup(None);
    core.lock();
    assert!(core.unlock("סיסמה שגויה לגמרי כאן").is_err());
    assert!(matches!(core.unlock(PASSWORD), Err(CoreError::Backoff(_))));
    let mut fresh = Core::for_tests(dir.path(), None);
    assert!(fresh.unlock(PASSWORD).unwrap().unlocked);
}

#[test]
fn only_allowed_models() {
    let (_dir, mut core, _case) = setup(None);
    assert!(core.set_model("claude-sonnet-5").is_ok());
    assert!(matches!(
        core.set_model("some-other-model"),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn full_draft_prepares_every_section_with_material() {
    let (_dir, mut core, case) = setup(None);
    let all = core.prepare_full_draft(&case).unwrap();
    assert!(!all.is_empty());
    for (key, p) in &all {
        assert!(
            p.approval_id.is_some(),
            "{key}: {:?} {:?}",
            p.blocked,
            p.suspects
        );
    }
}

#[test]
fn ambiguous_word_is_decided_once_per_case() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "עדכון",
        "הסייעת אמרה שאלון נרגע מהר יותר השבוע.",
    )
    .unwrap();
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(p.approval_id.is_none());
    let s = p
        .suspects
        .iter()
        .find(|s| s.kind == dv_privacy::SuspectKind::AmbiguousWord)
        .expect("ambiguous");

    core.decide_suspect(&case, &s.token, SuspectDecision::IsName)
        .unwrap();
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    let id = p
        .approval_id
        .clone()
        .unwrap_or_else(|| panic!("{:?} {:?}", p.blocked, p.suspects));
    core.send_section(&id).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(sent[0].contains("ש[ילד] נרגע") && !sent[0].contains("אלון"));

    // "Keep as is" instead, in another case with the same child name: that case asks again.
    let other = core
        .create_case(
            CaseMeta {
                code: "TEST-0003".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "אלון".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.add_input(
        &other,
        InputKind::Kindergarten,
        "שאלון",
        "שאלון ההורים הוחזר מלא.",
    )
    .unwrap();
    let p = core
        .prepare_section(&other, "kindergarten", "טיוטה")
        .unwrap();
    assert!(p.approval_id.is_none());
    core.decide_suspect(&other, "שאלון", SuspectDecision::NotAName)
        .unwrap();
    let p = core
        .prepare_section(&other, "kindergarten", "טיוטה")
        .unwrap();
    assert!(p.approval_id.is_some(), "{:?} {:?}", p.blocked, p.suspects);
}

fn docx(document_body: &str, header: &str, creator: &str) -> Vec<u8> {
    use std::io::Write;
    let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let p = |t: &str| format!("<w:p><w:r><w:t xml:space=\"preserve\">{t}</w:t></w:r></w:p>");
    let parts = [
        ("word/document.xml", format!("<w:document {w}><w:body>{}</w:body></w:document>", p(document_body))),
        ("word/header1.xml", format!("<w:hdr {w}>{}</w:hdr>", p(header))),
        ("docProps/core.xml", format!("<cp:coreProperties xmlns:cp=\"c\" xmlns:dc=\"d\"><dc:creator>{creator}</dc:creator></cp:coreProperties>")),
    ];
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    for (name, content) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    buf.into_inner()
}

#[test]
fn import_shows_body_hides_names_and_suggests_names_from_margins() {
    let (_dir, mut core, case) = setup(None);
    let bytes = docx(
        "סיכום ביקור: אלון הגיע עם אמו. בבדיקה נצפה קושי במעברים.",
        "מרכז בדוי · לידי ד\"ר רונית",
        "יעל בדויה",
    );
    let p = core
        .import_document(&case, "סיכום ביקור.docx", &bytes)
        .unwrap();
    assert_eq!(p.format, "docx");
    assert_eq!(p.suggested_kind, InputKind::PriorReport);
    assert!(
        p.body.contains("אלון הגיע"),
        "the stored body keeps the real text"
    );
    assert!(!p.body.contains("מרכז בדוי"), "the header is not imported");
    assert!(p.left_out.iter().any(|l| l.contains("מרכז בדוי")));
    assert!(
        p.preview
            .iter()
            .any(|s| s.mark.is_some() && s.text.contains("אלון")),
        "{:?}",
        p.preview
    );
    let values: Vec<&str> = p
        .name_suggestions
        .iter()
        .map(|s| s.value.as_str())
        .collect();
    assert!(values.contains(&"יעל בדויה"), "{values:?}");
    assert!(values.iter().any(|v| v.contains("רונית")), "{values:?}");
    assert!(p.warnings.iter().any(|w| w.contains("הכותרת")));

    // Confirming stores the (reviewed) body as case material.
    core.add_input(&case, p.suggested_kind, &p.title, &p.body)
        .unwrap();
    assert!(core
        .case_detail(&case)
        .unwrap()
        .inputs
        .iter()
        .any(|i| i.title == "סיכום ביקור"));
}

#[test]
fn import_refuses_what_it_cannot_read() {
    let (_dir, mut core, case) = setup(None);
    let err = core
        .import_document(
            &case,
            "old.doc",
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0],
        )
        .unwrap_err();
    assert!(
        err.to_ui().message.contains(".docx"),
        "{}",
        err.to_ui().message
    );
}

fn read_docx_text(bytes: &[u8]) -> String {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

#[test]
fn export_needs_approved_paragraphs_and_restores_names() {
    let (_dir, mut core, case) = setup(None);
    let check = core.check_export(&case).unwrap();
    assert!(!check.blocking.is_empty(), "nothing approved yet");
    assert!(matches!(
        core.export_report(&case, None),
        Err(CoreError::Refused(_))
    ));

    // A demo draft for the kindergarten section, then approve it.
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let para = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs
        .remove(0);
    core.approve_paragraph(&case, &para.id).unwrap();
    core.add_own_paragraph(&case, "referral", "ההורים של אלון פנו בשל קושי במעברים.")
        .unwrap();

    let check = core.check_export(&case).unwrap();
    assert!(check.blocking.is_empty(), "{:?}", check.blocking);
    assert_eq!(check.included_sections, 2);
    assert!(
        !check.file_name.contains("אלון"),
        "no child name in the file name: {}",
        check.file_name
    );
    assert!(check.file_name.contains("TEST-0002"));

    let bytes = core.export_report(&case, None).unwrap();
    let doc = read_docx_text(&bytes);
    assert!(doc.contains("ההורים של אלון פנו"), "names restored");
    assert!(doc.contains("שם הילד"), "info line");
    assert!(!doc.contains("[ילד]") && !doc.contains("[גננת]"));

    // Protected export: a Compound File, not a readable zip.
    let protected = core.export_report(&case, Some("סיסמה-לקובץ-1")).unwrap();
    assert!(protected.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]));
    assert!(matches!(
        core.export_report(&case, Some("קצר")),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn missing_information_marker_blocks_export() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "background", "ההריון תקין. [חסר: גיל ההליכה]")
        .unwrap();
    let check = core.check_export(&case).unwrap();
    assert!(
        check.blocking.iter().any(|b| b.contains("חסר")),
        "{:?}",
        check.blocking
    );
}

/// Offline for the first call, then answers like the demo.
#[derive(Default)]
struct OfflineOnce {
    calls: std::sync::atomic::AtomicU32,
}

impl Transport for OfflineOnce {
    fn send(&self, payload: &ClearedPayload) -> Result<Value, EgressError> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err(EgressError::Offline);
        }
        Ok(dv_ai::demo::respond(
            &serde_json::from_slice(payload.body()).unwrap(),
        ))
    }
}

#[test]
fn failed_send_keeps_the_approval_for_a_retry() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), Some(Box::new(OfflineOnce::default())));
    core.create_vault(PASSWORD).unwrap();
    let case = core
        .create_case(
            CaseMeta {
                code: "TEST-0004".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "אלון".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "שיחה",
        "אלון נרגע מהר יותר השבוע.",
    )
    .unwrap();
    let id = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap()
        .approval_id
        .unwrap();

    // The shell's path: take it out, send without holding the core, then finish.
    let out = core.begin_send(&id).unwrap();
    let response = out.transmit();
    let err = core.finish_section(out, response).unwrap_err();
    assert!(
        err.to_ui().message.contains("אין חיבור"),
        "{}",
        err.to_ui().message
    );
    assert!(
        core.chat(&case, "kindergarten").unwrap().is_empty(),
        "nothing stored"
    );

    let r = core.send_section(&id).unwrap();
    assert!(!r.paragraphs.is_empty());
}

#[test]
fn idle_session_locks_itself() {
    let (_dir, mut core, _case) = setup(None);
    assert!(!core.lock_if_idle());
    core.last_activity = Instant::now() - Duration::from_secs(11 * 60);
    assert!(core.lock_if_idle());
    assert!(!core.status().unlocked);
}

#[test]
fn review_screen_is_always_shown_in_the_first_weeks() {
    let (_dir, mut core, _case) = setup(None);
    let s = core.status();
    assert!(!s.review_only_suspect && !s.review_choice_available);
    assert!(matches!(
        core.set_review_only_suspect(true),
        Err(CoreError::Refused(_))
    ));
    assert!(core.set_review_only_suspect(false).is_ok());
    assert_eq!(s.practitioner, vec!["ד\"ר רותם אלמוג".to_owned()]);
}
