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
fn usage_is_counted_and_the_monthly_ceiling_stops_sending() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let section = |core: &mut Core| {
        core.prepare_section(&case, "kindergarten", "טיוטה")
            .unwrap()
            .approval_id
            .unwrap()
    };
    let first = section(&mut core);
    core.send_section(&first).unwrap();
    let used = core.usage_summary().unwrap();
    assert_eq!(used.requests, 1);
    assert_eq!(used.cap_usd, None);

    // An answer that cost more than the ceiling: the next request is refused, and stays approved.
    *fake.answer.lock().unwrap() = None;
    let mut month = usage::Month::default();
    month.add(
        dv_ai::DEFAULT_MODEL,
        usage::Tokens {
            requests: 1,
            input: 3_000_000,
            ..usage::Tokens::default()
        },
    );
    core.vault_mut()
        .unwrap()
        .set_setting(
            &usage::this_month_key(),
            &serde_json::to_string(&month).unwrap(),
        )
        .unwrap();
    assert!(core.set_monthly_cap(Some(0)).is_err());
    core.set_monthly_cap(Some(10)).unwrap();
    let second = section(&mut core);
    let err = core.send_section(&second).unwrap_err();
    assert_eq!(err.to_ui().code, "refused");
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
    // Raising the ceiling lets the same approval go out.
    core.set_monthly_cap(None).unwrap();
    assert_eq!(core.usage_summary().unwrap().cap_usd, None);
    core.send_section(&second).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 2);
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
        core.prepare_consult(Some(&case), None, "שאלה"),
        Err(CoreError::ConsentMissing)
    ));
    // A general consultation (no case) does not need a case's consent.
    assert!(core
        .prepare_consult(None, None, "מה ההבדל בין WPPSI-IV ל-WISC-V?")
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
fn consultations_are_kept_continued_and_deleted() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let p = core
        .prepare_consult(
            Some(&case),
            None,
            "איך כדאי לנסח המלצה על ליווי רגשי לאלון?",
        )
        .unwrap();
    assert!(!p.hidden.is_empty());
    let first = core.send_consult(&p.approval_id.unwrap()).unwrap();
    assert!(!first.answer.is_empty());

    // After locking and unlocking, the conversation is still there, with names shown.
    core.lock();
    core.unlock(PASSWORD).unwrap();
    let list = core.consultations().unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].title.contains("אלון"));
    assert!(list[0].case_label.as_deref().unwrap().contains("אלון"));
    let view = core.consultation(&first.conversation_id).unwrap();
    assert_eq!(view.turns.len(), 2);
    assert!(view.turns[0].text.contains("אלון") && !view.turns[0].hidden.is_empty());

    // Continuing sends the earlier turns as they were sent: filtered.
    let p = core
        .prepare_consult(Some(&case), Some(&first.conversation_id), "ומה עם שירה?")
        .unwrap();
    let again = core.send_consult(&p.approval_id.unwrap()).unwrap();
    assert_eq!(again.conversation_id, first.conversation_id);
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    for name in ["אלון", "שירה"] {
        assert!(!sent.contains(name), "{name} left the machine");
    }
    assert_eq!(
        core.consultation(&first.conversation_id)
            .unwrap()
            .turns
            .len(),
        4
    );
    // A conversation stays with its case.
    assert!(matches!(
        core.prepare_consult(None, Some(&first.conversation_id), "שאלה"),
        Err(CoreError::Refused(_))
    ));

    core.delete_consultation(&first.conversation_id).unwrap();
    assert!(core.consultations().unwrap().is_empty());
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

    assert_eq!(check.score_tables, 0);
    // A score table entered in the case becomes an appendix table in the report.
    let sheet = dv_domain::ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![dv_domain::ScoreEntry {
            measure: "vci".into(),
            value: 112.0,
            note: String::new(),
        }],
        notes: String::new(),
    };
    core.save_scores(&case, None, &sheet).unwrap();
    assert_eq!(core.check_export(&case).unwrap().score_tables, 1);
    let bytes = core.export_report(&case, None).unwrap();
    let doc = read_docx_text(&bytes);
    assert!(
        doc.contains("נספח: טבלאות ציונים") && doc.contains("הבנה מילולית (VCI)"),
        "score table"
    );
    assert!(
        doc.contains("ממוצע גבוה") && doc.contains("79"),
        "range and percentile in the table"
    );
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
fn missing_information_marker_is_pointed_out_and_bracketed_words_are_plain() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(
        &case,
        "background",
        "ההריון תקין. [חסר: גיל ההליכה] לדברי [מחנכת] הוא משתלב.",
    )
    .unwrap();
    let check = core.check_export(&case).unwrap();
    assert!(check.blocking.is_empty(), "{:?}", check.blocking);
    assert_eq!(check.to_complete.len(), 1, "{:?}", check.to_complete);
    assert!(check.to_complete[0].contains("[חסר: גיל ההליכה]"));
    // The file goes out; the marker stays in it, the model's brackets become plain text.
    let bytes = core.export_report(&case, None).unwrap();
    let text = read_docx_text(&bytes);
    assert!(
        text.contains("[חסר: גיל ההליכה]") && text.contains("(מחנכת)"),
        "{text}"
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
fn work_in_the_window_keeps_the_vault_open_but_never_reopens_it() {
    let (_dir, mut core, _case) = setup(None);
    core.last_activity = Instant::now() - Duration::from_secs(9 * 60 + 30);
    let left = core.status().idle_lock_in.unwrap();
    assert!(left <= 30, "{left}");
    // Typing a paragraph counts as activity.
    core.touch();
    assert!(core.status().idle_lock_in.unwrap() > 9 * 60);
    assert!(!core.lock_if_idle());
    // Past the idle time, a late keystroke does not extend it.
    core.last_activity = Instant::now() - Duration::from_secs(11 * 60);
    core.touch();
    assert!(core.lock_if_idle());
    assert_eq!(core.status().idle_lock_in, None);
}

#[test]
fn the_report_file_name_never_carries_a_name() {
    let who = |value: &str, aliases: &[&str]| dv_domain::Identity {
        id: "i".into(),
        case_id: "c".into(),
        role: Role::Child,
        tag: "[ילד]".into(),
        value: value.into(),
        aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
    };
    let ids = [who("אלון כהן", &["Alon"])];
    let day = (2026, 10, 3);
    assert_eq!(
        report_file_name("TEST-0002", &ids, day),
        "דוח אבחון TEST-0002.docx"
    );
    assert_eq!(
        report_file_name("אלון-5", &ids, day),
        "דוח אבחון 2026-10-03.docx"
    );
    assert_eq!(
        report_file_name("ALON2026", &ids, day),
        "דוח אבחון 2026-10-03.docx"
    );
    assert_eq!(report_file_name("", &ids, day), "דוח אבחון 2026-10-03.docx");
}

#[test]
fn readiness_lists_what_is_missing_and_keeps_her_confirmations() {
    let (_dir, mut core, _case) = setup(None);
    let r = core.readiness().unwrap();
    assert!(!r.all_done);
    let item = |r: &Readiness, k: &str| r.items.iter().find(|i| i.key == k).unwrap().clone();
    assert!(!item(&r, "zdr").done);
    assert!(!item(&r, "backup").done);
    assert!(item(&r, "backup").checked_by_program);
    assert!(!item(&r, "api_key").done);
    let r = core.confirm_readiness("zdr", true).unwrap();
    assert!(item(&r, "zdr").done);
    assert!(item(&r, "zdr").confirmed_at.is_some());
    let r = core.confirm_readiness("zdr", false).unwrap();
    assert!(!item(&r, "zdr").done);
    // Items the program checks cannot be confirmed by hand.
    assert!(core.confirm_readiness("backup", true).is_err());
}

#[test]
fn the_vault_locks_with_the_computer() {
    let (_dir, mut core, _case) = setup(None);
    assert!(core.is_unlocked());
    assert!(core.lock_with_computer());
    assert!(!core.is_unlocked());
    assert!(!core.lock_with_computer());
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

#[test]
fn entered_scores_feed_the_section_with_ranges_from_the_table() {
    use dv_domain::{ScoreEntry, ScoreSheet};
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let mut sheet = ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![
            ScoreEntry {
                measure: "fsiq".into(),
                value: 104.0,
                note: String::new(),
            },
            ScoreEntry {
                measure: "vci".into(),
                value: 112.0,
                note: String::new(),
            },
            ScoreEntry {
                measure: "psi".into(),
                value: 88.0,
                note: "עבד לאט ובדייקנות".into(),
            },
            ScoreEntry {
                measure: "block_design".into(),
                value: 9.0,
                note: String::new(),
            },
        ],
        notes: "אלון שיתף פעולה לאורך כל ההעברה.".into(),
    };
    let saved = core.save_scores(&case, None, &sheet).unwrap();
    assert_eq!(saved.kind, InputKind::TestScores);
    assert_eq!(saved.title, "ציוני WPPSI-IV");
    assert!(
        saved.content.contains("ציון 112, אחוזון 79 – ממוצע גבוה"),
        "{}",
        saved.content
    );
    assert_eq!(
        core.score_sheet(&case, &saved.id).unwrap().as_ref(),
        Some(&sheet)
    );

    // Editing keeps one material and replaces its text.
    sheet.entries[1].value = 115.0;
    let edited = core.save_scores(&case, Some(&saved.id), &sheet).unwrap();
    assert_eq!(edited.id, saved.id);
    let detail = core.case_detail(&case).unwrap();
    let scores: Vec<_> = detail
        .inputs
        .iter()
        .filter(|i| i.kind == InputKind::TestScores)
        .collect();
    assert_eq!(scores.len(), 1);
    assert!(
        scores[0].content.contains("ציון 115, אחוזון 84"),
        "{}",
        scores[0].content
    );

    // The table text passes the filter as is: abbreviations and scores are not names or ids.
    let prepared = core.prepare_section(&case, "cognitive", "").unwrap();
    assert!(prepared.suspects.is_empty(), "{:?}", prepared.suspects);
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    core.send_section(&prepared.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().join("\n");
    assert!(
        sent.contains("ממוצע גבוה") && sent.contains("אחוזון 84"),
        "{sent}"
    );
    assert!(
        !sent.contains("אלון"),
        "the child's name is hidden in the notes too"
    );

    // Nonsense is refused before anything is stored.
    let bad = ScoreSheet {
        entries: vec![ScoreEntry {
            measure: "vci".into(),
            value: 12.0,
            note: String::new(),
        }],
        ..sheet
    };
    assert!(core.save_scores(&case, None, &bad).is_err());
    assert!(core
        .save_scores(
            &case,
            Some("missing"),
            &ScoreSheet {
                entries: Vec::new(),
                ..bad
            }
        )
        .is_err());
}

#[test]
fn every_instrument_sheet_passes_the_filter_without_questions() {
    use dv_domain::{ScoreEntry, ScoreSheet};
    let (_dir, mut core, case) = setup(None);
    for inst in Core::score_instruments() {
        let entries = inst
            .measures
            .iter()
            .map(|m| ScoreEntry {
                measure: m.key.clone(),
                value: ((m.min + m.max) / 2.0).round(),
                note: String::new(),
            })
            .collect();
        let sheet = ScoreSheet {
            instrument: inst.key.clone(),
            module: "2".into(),
            cutoff: Some(8.0),
            entries,
            notes: String::new(),
        };
        let text = dv_domain::format_sheet(&sheet).unwrap();
        let outcome = core.preview_filter(&case, &text).unwrap();
        assert_eq!(
            outcome.tagged, text,
            "{}: the table goes out unchanged",
            inst.key
        );
        assert!(
            outcome.suspects.is_empty(),
            "{}: {:?}",
            inst.key,
            outcome.suspects
        );
        assert!(
            outcome.hidden.is_empty(),
            "{}: {:?}",
            inst.key,
            outcome.hidden
        );
    }
}

#[test]
fn a_half_point_score_is_not_mistaken_for_a_date() {
    use dv_domain::{ScoreEntry, ScoreSheet};
    let (_dir, mut core, case) = setup(None);
    let sheet = ScoreSheet {
        instrument: "cars_2".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![ScoreEntry {
            measure: "total".into(),
            value: 29.5,
            note: String::new(),
        }],
        notes: String::new(),
    };
    let text = dv_domain::format_sheet(&sheet).unwrap();
    assert!(text.contains("ציון 29.5 – מעט או ללא תסמינים"), "{text}");
    let outcome = core.preview_filter(&case, &text).unwrap();
    assert_eq!(outcome.tagged, text);
}

/// Every first name the filter knows, including names that are also words ("גיל", "חיים").
fn lexicon_names() -> Vec<&'static str> {
    let names: Vec<&str> = [
        include_str!("../../dv-privacy/data/first_names.txt"),
        include_str!("../../dv-privacy/data/word_names.txt"),
    ]
    .into_iter()
    .flat_map(str::lines)
    .map(str::trim)
    .filter(|l| !l.is_empty() && !l.starts_with('#'))
    .collect();
    assert!(names.len() > 300);
    names
}

fn child_named(name: &str) -> [dv_domain::Identity; 1] {
    [dv_domain::Identity {
        id: "i".into(),
        case_id: "c".into(),
        role: Role::Child,
        tag: "[ילד]".into(),
        value: name.into(),
        aliases: vec![],
    }]
}

/// The rules, style, schema and the builder's own words go out with every request and never
/// reach the review screen, so they must not contain any child's name, even inside another word
/// ("שלישי" when the child is "ישי"): the gate would block that case for good.
#[test]
fn fixed_prompt_text_never_collides_with_a_childs_name() {
    let mut body: Vec<Value> = [
        dv_ai::prompts::DRAFTING_RULES,
        dv_ai::prompts::DEFAULT_STYLE,
        dv_ai::prompts::OUTPUT_RULES,
        dv_ai::prompts::CONSULT_RULES,
        dv_ai::prompts::RESEARCH_RULES,
        dv_ai::prompts::SORTING_RULES,
    ]
    .into_iter()
    .chain(
        ["expand", "shorten", "analyze", "recommend", "rephrase"]
            .into_iter()
            .filter_map(dv_ai::prompts::quick_action),
    )
    .map(|t| Value::String(t.to_owned()))
    .collect();
    // A whole section request as the builder writes it: every kind label, both grammatical
    // genders, the age line, the data frames, the history opener and the JSON schema.
    for gender in [GrammaticalGender::Male, GrammaticalGender::Female] {
        let sources = [
            InputKind::Intake,
            InputKind::PriorReport,
            InputKind::TestScores,
            InputKind::Professional,
            InputKind::Kindergarten,
            InputKind::Observation,
            InputKind::SessionNote,
            InputKind::FreeText,
        ]
        .into_iter()
        .map(|k| dv_ai::TaggedInput {
            input_id: "in".into(),
            kind_label: k.label_he().to_owned(),
            title_tagged: String::new(),
            content_tagged: String::new(),
        })
        .collect();
        let input = dv_ai::SectionInput {
            section_key: "cognitive".into(),
            section_title: String::new(),
            age: Some("5:4".into()),
            gender: Some(gender),
            sources,
            approved_context: vec![(String::new(), String::new())],
            current_draft: vec![String::new()],
            history: vec![dv_ai::TaggedTurn {
                role: "assistant".into(),
                text_tagged: String::new(),
            }],
            instruction_tagged: String::new(),
            style_profile: None,
        };
        let nonce = dv_ai::nonce_from(&[7; 16]);
        // Like the real gate: every string in the body, not the JSON numbers ("max_tokens").
        body.push(dv_ai::build_section_request(&dv_ai::ModelConfig::default(), &input, &nonce).0);
    }
    // The sorting request (D-022): every section key and description of the template, the
    // passage frames and the schema.
    let structure = ReportStructure::load_default().unwrap();
    let sort = dv_ai::SortInput {
        sections: sorting::sortable(&structure)
            .into_iter()
            .map(|s| dv_ai::SortSection {
                key: s.key.clone(),
                about: s.about.clone(),
            })
            .collect(),
        materials: vec![dv_ai::SortMaterial {
            input_id: "in".into(),
            kind_label: InputKind::Intake.label_he().to_owned(),
            title_tagged: String::new(),
            passages_tagged: vec![String::new(), String::new()],
        }],
    };
    let nonce = dv_ai::nonce_from(&[7; 16]);
    body.push(dv_ai::build_sort_request(
        &dv_ai::ModelConfig::default(),
        &sort,
        &nonce,
    ));
    let body = Value::Array(body);
    let tags: HashSet<String> = HashSet::from(["[ילד]".to_owned()]);
    let mut collisions = Vec::new();
    for name in lexicon_names() {
        let identities = child_named(name);
        let ctx = PrivacyContext {
            case_id: "c",
            identities: &identities,
            practitioner: &[],
            allowlisted: &|_| false,
            confirmed_names: &|_| false,
            today: (2026, 9, 28),
        };
        let req = GateRequest {
            body: &body,
            ctx: &ctx,
            case_tags: &tags,
            unresolved_suspects: 0,
            canaries: &[],
            max_bytes: MAX_REQUEST_BYTES,
        };
        if let Err(blocked) = clear(&req) {
            let found: Vec<_> = blocked
                .reasons
                .iter()
                .filter_map(|r| r.detail.clone())
                .collect();
            collisions.push(format!("{name}: {found:?}"));
        }
    }
    assert!(collisions.is_empty(), "{collisions:#?}");
}

/// Section titles and score tables are case text: they pass the filter and the review screen,
/// where a word like "שאלון" (child "אלון") is a question. What must never happen is a silent
/// rewrite: "חיים בבית" turning into "[ילד] בבית" for a child named "חיים".
#[test]
fn score_tables_and_section_titles_are_never_rewritten_silently() {
    use dv_domain::{ScoreEntry, ScoreSheet};
    let mut texts: Vec<String> = ReportStructure::load_default()
        .unwrap()
        .parts
        .into_iter()
        .flat_map(|p| std::iter::once(p.title).chain(p.sections.into_iter().map(|s| s.title)))
        .collect();
    for inst in dv_domain::instruments() {
        let entries = inst
            .measures
            .iter()
            .map(|m| ScoreEntry {
                measure: m.key.clone(),
                value: m.min,
                note: String::new(),
            })
            .collect();
        let sheet = ScoreSheet {
            instrument: inst.key.clone(),
            module: "2".into(),
            cutoff: Some(1.0),
            entries,
            notes: String::new(),
        };
        texts.push(dv_domain::format_sheet(&sheet).unwrap());
    }
    let mut rewritten = Vec::new();
    for name in lexicon_names() {
        let identities = child_named(name);
        let ctx = PrivacyContext {
            case_id: "c",
            identities: &identities,
            practitioner: &[],
            allowlisted: &|_| false,
            confirmed_names: &|_| false,
            today: (2026, 9, 28),
        };
        for text in &texts {
            let outcome = dv_privacy::filter(text, &ctx).unwrap();
            if !outcome.hidden.is_empty() {
                rewritten.push(format!("{name}: {:?}", outcome.hidden));
            }
        }
    }
    assert!(rewritten.is_empty(), "{rewritten:#?}");
}

#[test]
fn a_consent_needs_a_real_past_date_and_a_signer() {
    let (_dir, mut core, case) = setup(None);
    let mut meta = core.case_detail(&case).unwrap().meta;
    for (given_on, given_by) in [
        ("2026-02-30", "ההורים"),
        ("28.09.2026", "ההורים"),
        ("2999-01-01", "ההורים"),
        ("2026-09-01", " "),
    ] {
        meta.consent = Some(Consent {
            given_on: given_on.into(),
            form_version: "v1".into(),
            given_by: given_by.into(),
        });
        assert!(
            core.update_case(&case, meta.clone()).is_err(),
            "{given_on} / {given_by:?}"
        );
    }
    meta.consent = Some(consent());
    core.update_case(&case, meta).unwrap();
}

// ------------------------------------------------------------ D-022: sorting into sections

/// A third material with three passages: pregnancy, milestones, home.
fn add_home_intake(core: &mut Core, case: &str) -> String {
    core.add_input(
        case,
        InputKind::Intake,
        "אינטייק שני",
        "ההריון והלידה עברו ללא סיבוכים.\n\nהלך בגיל שנה ואמר מילים ראשונות בגיל שנה וחצי.\n\nבבית אלון אוהב לבנות מגדלים ממגנטים.",
    )
    .unwrap()
    .id
}

/// Claude's sorting for the three materials of the test case (S1 kindergarten talk, S2 first
/// intake, S3 the intake above). The first intake gets nothing: it stays on the table.
fn claude_sorting() -> Value {
    api_json(&json!({"sections": [
        {"section": "background", "passages": ["S3P1", "S3P2"]},
        {"section": "parents_view", "passages": ["S3P3"]},
        {"section": "kindergarten", "passages": ["S1P1"]},
        {"section": "summary", "passages": ["S3P1"]},
        {"section": "background", "passages": ["S9P1"]}
    ]}))
}

fn section_request(core: &mut Core, fake: &FakeTransport, case: &str, key: &str) -> String {
    *fake.answer.lock().unwrap() = None;
    let prepared = core.prepare_section(case, key, "טיוטה").unwrap();
    let approval = prepared.approval_id.expect("clears the gate");
    core.send_section(&approval).unwrap();
    fake.sent.lock().unwrap().last().unwrap().clone()
}

#[test]
fn sorting_goes_out_filtered_and_drafts_then_get_only_their_passages() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let home = add_home_intake(&mut core, &case);

    let prepared = core.prepare_sort(&case).unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    assert!(!prepared.demo_mode);
    assert_eq!(prepared.parts.len(), 3, "one review part per material");
    *fake.answer.lock().unwrap() = Some(claude_sorting());
    let result = core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    assert_eq!((result.sorted, result.unchanged, result.links), (2, 1, 3));
    // The derived section and the unknown passage are dropped, never guessed.
    assert_eq!(result.ignored, 2);
    assert!(!result.demo);

    let sent = fake.sent.lock().unwrap()[0].clone();
    assert!(sent.contains("id=\\\"S3P2\\\"") && sent.contains("[ילד]"));
    for name in ["אלון", "שירה", "רותם", "אלמוג"] {
        assert!(!sent.contains(name), "{name} left the machine");
    }

    let detail = core.case_detail(&case).unwrap();
    let r = detail.routing.iter().find(|r| r.input_id == home).unwrap();
    assert!(r.sorted && r.by_ai);
    assert_eq!(r.feeds, ["background", "parents_view"]);
    assert_eq!((r.passages, r.used_passages), (3, 3));
    // The first intake was not placed: it keeps the table.
    let first = &detail.routing[1];
    assert!(!first.sorted && !first.needs_sorting);
    assert_eq!(first.feeds, first.table);

    // Background: the two chosen passages of the new intake, and the whole first intake.
    let background = section_request(&mut core, &fake, &case, "background");
    assert!(background.contains("ההריון") && background.contains("מילים ראשונות"));
    assert!(
        !background.contains("מגדלים"),
        "a passage chosen for another section"
    );
    assert!(
        background.contains("נרדם באיחור"),
        "the unsorted intake still goes whole"
    );
    // Home: only the third passage.
    let home_view = section_request(&mut core, &fake, &case, "parents_view");
    assert!(home_view.contains("מגדלים") && !home_view.contains("ההריון"));
    // Everything sorted: nothing new to send.
    assert!(matches!(
        core.prepare_sort(&case),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn chosen_passages_are_shown_in_the_review_exactly_as_sent() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(claude_sorting());
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();

    let prepared = core.prepare_section(&case, "background", "טיוטה").unwrap();
    let part = prepared
        .parts
        .iter()
        .find(|p| p.label.contains("קטעים שנבחרו לסעיף"))
        .expect("the chosen passages are labelled");
    let shown: String = part.outgoing.iter().map(|s| s.text.as_str()).collect();
    assert!(shown.contains("ההריון") && shown.contains("מילים ראשונות"));
    assert!(!shown.contains("מגדלים"));
}

#[test]
fn einat_decides_last_and_an_edit_forgets_the_old_passages() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let home = add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(claude_sorting());
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();

    core.set_input_sections(&case, &home, &["parents_view".into(), "cognitive".into()])
        .unwrap();
    let r = |core: &mut Core| {
        core.case_detail(&case)
            .unwrap()
            .routing
            .into_iter()
            .find(|r| r.input_id == home)
            .unwrap()
    };
    let now = r(&mut core);
    assert_eq!(now.feeds, ["parents_view", "cognitive"]);
    assert_eq!(
        (now.added.as_slice(), now.removed.as_slice()),
        (
            &["cognitive".to_owned()][..],
            &["background".to_owned()][..]
        )
    );
    // Added by hand: the whole material goes there.
    let cognitive = section_request(&mut core, &fake, &case, "cognitive");
    assert!(cognitive.contains("ההריון") && cognitive.contains("מגדלים"));
    let background = section_request(&mut core, &fake, &case, "background");
    assert!(!background.contains("ההריון"));

    // Only sections written from materials can be chosen.
    assert!(matches!(
        core.set_input_sections(&case, &home, &["summary".into()]),
        Err(CoreError::NotFound(_))
    ));

    // A new text has new passages: the sorting is forgotten, her choices stay.
    core.update_input(&case, &home, "אינטייק שני", "ההורים מתארים ילד סקרן.")
        .unwrap();
    let after = r(&mut core);
    assert!(!after.sorted);
    assert_eq!(after.feeds, ["referral", "parents_view", "cognitive"]);
}

#[test]
fn a_sorting_that_arrives_after_an_edit_is_not_applied() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let home = add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    let out = core.begin_send(&prepared.approval_id.unwrap()).unwrap();
    // Einat edits while Claude reads (same number of passages, different text).
    core.update_input(
        &case,
        &home,
        "אינטייק שני",
        "ההריון היה במעקב.\n\nהלך בגיל שנה.\n\nבבית משחק לבד.",
    )
    .unwrap();
    let result = core
        .finish_sort(out, Ok((claude_sorting(), false)))
        .unwrap();
    assert_eq!(result.sorted, 1, "only the kindergarten talk");
    let detail = core.case_detail(&case).unwrap();
    assert!(
        !detail
            .routing
            .iter()
            .find(|r| r.input_id == home)
            .unwrap()
            .sorted
    );
}

#[test]
fn sorting_needs_consent_and_asks_about_unknown_names_first() {
    let (_dir, mut core, case) = setup(None);
    core.add_input(
        &case,
        InputKind::SessionNote,
        "מפגש",
        "בפינת הבנייה סיפר שהוא משחק עם יובל.",
    )
    .unwrap();
    let prepared = core.prepare_sort(&case).unwrap();
    assert!(
        prepared.approval_id.is_none(),
        "an open question blocks sending"
    );
    assert!(prepared.suspects.iter().any(|s| s.token.contains("יובל")));

    let mut meta = core.case_detail(&case).unwrap().meta;
    meta.consent = None;
    core.update_case(&case, meta).unwrap();
    assert!(matches!(
        core.prepare_sort(&case),
        Err(CoreError::ConsentMissing)
    ));
}

#[test]
fn demo_sorting_runs_locally_and_says_so() {
    let (_dir, mut core, case) = setup(None);
    let home = add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    assert!(prepared.demo_mode);
    let result = core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    assert!(result.demo && result.sorted >= 1);
    let detail = core.case_detail(&case).unwrap();
    let r = detail.routing.iter().find(|r| r.input_id == home).unwrap();
    assert!(r.sorted && !r.by_ai);
    assert!(r.feeds.contains(&"background".to_owned()));
    // The section counts follow the routing.
    let kg = detail
        .sections
        .iter()
        .find(|s| s.key == "kindergarten")
        .unwrap();
    assert!(kg.sortable && kg.source_count >= 1);
    assert!(
        !detail
            .sections
            .iter()
            .find(|s| s.key == "summary")
            .unwrap()
            .sortable
    );
}

#[test]
fn passages_far_apart_are_marked_with_a_gap_the_gate_accepts() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    // Passages 1 and 3, not 2: the draft request carries a gap mark between them.
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"sections": [
        {"section": "background", "passages": ["S3P1", "S3P3"]}
    ]})));
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    let prepared = core.prepare_section(&case, "background", "טיוטה").unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    let background = section_request(&mut core, &fake, &case, "background");
    assert!(background.contains("(…)") && !background.contains("מילים ראשונות"));
}

// ------------------------------------------------------------ D-023: library

#[test]
fn delete_goes_to_the_bin_and_erasing_needs_the_password() {
    let (_dir, mut core, case) = setup(None);
    core.delete_case(&case).unwrap();
    assert!(core.list_cases().unwrap().is_empty());
    assert_eq!(core.list_trash().unwrap().len(), 1);
    core.restore_case(&case).unwrap();
    assert_eq!(core.list_cases().unwrap().len(), 1);

    // Only from the bin, and only with the right password.
    assert!(matches!(
        core.purge_case(&case, PASSWORD),
        Err(CoreError::Refused(_))
    ));
    core.delete_case(&case).unwrap();
    assert!(matches!(
        core.purge_case(&case, "ניחוש ארוך אבל שגוי"),
        Err(CoreError::Vault(VaultError::WrongSecret))
    ));
    // A wrong attempt makes the next one wait, like at unlock.
    assert!(matches!(
        core.purge_case(&case, PASSWORD),
        Err(CoreError::Backoff(_))
    ));
    core.not_before = None;
    core.purge_case(&case, PASSWORD).unwrap();
    assert!(core.list_trash().unwrap().is_empty());
    assert!(matches!(
        core.case_detail(&case),
        Err(CoreError::Vault(VaultError::NotFound))
    ));
}

#[test]
fn a_case_left_in_the_bin_past_thirty_days_is_erased_at_unlock() {
    let (_dir, mut core, case) = setup(None);
    core.delete_case(&case).unwrap();
    core.purge_expired().unwrap();
    assert_eq!(core.list_trash().unwrap().len(), 1, "still within 30 days");
    // Pretend it was deleted 31 days ago.
    core.vault_mut()
        .unwrap()
        .set_deleted_at_for_tests(&case, 1)
        .unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.list_trash().unwrap().is_empty());
}

#[test]
fn folders_hold_cases_and_refuse_loops() {
    let (_dir, mut core, case) = setup(None);
    let parent = core.create_folder(None, "  אבחונים פרטיים  ").unwrap();
    assert_eq!(parent.name, "אבחונים פרטיים");
    let child = core.create_folder(Some(&parent.id), "2026").unwrap();
    core.move_case(&case, Some(&child.id)).unwrap();
    assert_eq!(
        core.list_cases().unwrap()[0].folder_id.as_deref(),
        Some(child.id.as_str())
    );
    assert!(matches!(
        core.create_folder(None, "   "),
        Err(CoreError::Refused(_))
    ));
    assert!(matches!(
        core.create_folder(None, &"א".repeat(61)),
        Err(CoreError::Refused(_))
    ));
    assert!(matches!(
        core.move_folder(&parent.id, Some(&child.id)),
        Err(CoreError::Refused(_))
    ));
    core.delete_folder(&parent.id).unwrap();
    // The subfolder and its case are still there, one level up.
    let folders = core.folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].parent_id, None);
    assert_eq!(
        core.list_cases().unwrap()[0].folder_id.as_deref(),
        Some(child.id.as_str())
    );
}

#[test]
fn a_name_seen_in_another_case_is_pointed_out() {
    let (_dir, mut core, case) = setup(None);
    // The test case has the child "אלון" and the teacher "שירה".
    let found = core
        .find_name_matches(None, &["שירה לוי".into(), "מאיה".into(), "די".into()])
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].typed.as_str(), found[0].role),
        ("שירה לוי", Role::Teacher)
    );
    assert_eq!(found[0].case_id, case);
    // Editing the same case does not match itself; a case in the bin still counts.
    assert!(core
        .find_name_matches(Some(&case), &["אלון".into()])
        .unwrap()
        .is_empty());
    core.delete_case(&case).unwrap();
    let found = core.find_name_matches(None, &["אלון".into()]).unwrap();
    assert!(found[0].trashed);
}

#[test]
fn the_vault_locks_after_the_computer_slept() {
    let (_dir, mut core, _case) = setup(None);
    let t0 = SystemTime::now();
    assert!(!core.tick(t0));
    assert!(!core.tick(t0 + Duration::from_secs(15)));
    assert!(core.status().unlocked);
    // Five minutes between two ticks: the timer did not run, the computer was asleep.
    assert!(core.tick(t0 + Duration::from_secs(15 + 300)));
    assert!(!core.status().unlocked);
}

#[test]
fn a_backup_is_due_until_made_and_restores_on_a_new_computer() {
    let (dir, mut core, case) = setup(None);
    let status = core.backup_status().unwrap();
    assert!(status.due && status.has_cases && status.last_at.is_none());

    // Next to the vault protects nothing: refused.
    assert!(matches!(
        core.write_backup(&dir.path().join("copy.vaultbak")),
        Err(CoreError::Refused(_))
    ));
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("גיבוי")).unwrap();
    assert!(done.path.ends_with("גיבוי.vaultbak"));
    let status = core.backup_status().unwrap();
    assert_eq!((status.due, status.days_since), (false, Some(0)));
    assert_eq!(
        core.backup_dir().unwrap().unwrap(),
        drive.path().canonicalize().unwrap()
    );
    let bytes = std::fs::read(&done.path).unwrap();

    // A new computer: no vault yet.
    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    assert!(matches!(
        other.restore_staged_backup(Some(PASSWORD), None),
        Err(CoreError::Refused(_))
    ));
    let staged = other
        .stage_backup(bytes.clone(), "גיבוי.vaultbak".into())
        .unwrap();
    assert_eq!(
        (staged.created_at, staged.same_vault),
        (done.created_at, None)
    );
    assert!(matches!(
        other.restore_staged_backup(Some("ניחוש ארוך אבל שגוי"), None),
        Err(CoreError::Vault(VaultError::WrongSecret))
    ));
    assert!(matches!(
        other.restore_staged_backup(Some(PASSWORD), None),
        Err(CoreError::Backoff(_))
    ));
    other.not_before = None;
    let status = other.restore_staged_backup(Some(PASSWORD), None).unwrap();
    assert!(status.unlocked && status.integrity_warning.is_none());
    let detail = other.case_detail(&case).unwrap();
    assert!(detail.identities.iter().any(|i| i.value == "אלון"));

    // Never over an existing vault.
    other.stage_backup(bytes, "גיבוי.vaultbak".into()).unwrap();
    assert!(matches!(
        other.restore_staged_backup(Some(PASSWORD), None),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn the_restore_drill_proves_the_file_and_the_password() {
    let (_dir, mut core, _case) = setup(None);
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b.vaultbak")).unwrap();
    let bytes = std::fs::read(&done.path).unwrap();

    assert!(matches!(
        core.stage_backup(b"not a backup".to_vec(), "x".into()),
        Err(CoreError::Refused(_))
    ));
    let staged = core
        .stage_backup(bytes.clone(), "b.vaultbak".into())
        .unwrap();
    assert_eq!(staged.same_vault, Some(true));
    assert!(matches!(
        core.check_staged_backup("ניחוש ארוך אבל שגוי"),
        Err(CoreError::Vault(VaultError::WrongSecret))
    ));
    core.not_before = None;
    let check = core.check_staged_backup(PASSWORD).unwrap();
    assert_eq!((check.cases, check.integrity_ok), (1, true));
    assert!(core.backup_status().unwrap().last_check_at.is_some());
    assert!(
        core.list_cases().unwrap().len() == 1,
        "the live vault is untouched"
    );

    // A backup of another vault is named as such, and never "checked".
    let (_other_dir, mut other, _) = setup(None);
    let other_drive = tempfile::tempdir().unwrap();
    let theirs = other
        .write_backup(&other_drive.path().join("o.vaultbak"))
        .unwrap();
    let staged = core
        .stage_backup(std::fs::read(&theirs.path).unwrap(), "o.vaultbak".into())
        .unwrap();
    assert_eq!(staged.same_vault, Some(false));
    assert!(matches!(
        core.check_staged_backup(PASSWORD),
        Err(CoreError::Refused(_))
    ));

    // Locking forgets the chosen file.
    core.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    core.lock();
    assert!(core.staged_backup.is_none());
}

#[test]
fn a_new_password_or_kit_asks_for_a_new_backup() {
    let (dir, mut core, _case) = setup(None);
    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("b.vaultbak")).unwrap();
    assert!(!core.backup_status().unwrap().due);

    let new_password = "חתול כחול ישן על הספה";
    assert!(matches!(
        core.change_password("ניחוש ארוך אבל שגוי", false, new_password),
        Err(CoreError::Vault(VaultError::WrongSecret))
    ));
    core.not_before = None;
    assert!(matches!(
        core.change_password(PASSWORD, false, "קצר"),
        Err(CoreError::Vault(VaultError::Policy(_)))
    ));
    core.change_password(PASSWORD, false, new_password).unwrap();
    let status = core.backup_status().unwrap();
    assert!(status.due && status.secret_changed);

    let kit = core.new_recovery_kit(new_password, false).unwrap();
    assert!(core.confirm_recovery_key(&kit.recovery_key).unwrap());
    // A backup made now clears it.
    core.write_backup(&drive.path().join("c.vaultbak")).unwrap();
    let status = core.backup_status().unwrap();
    assert!(!status.due && !status.secret_changed);

    // Forgotten password: in with the kit, then a new password from the kit.
    core.lock();
    core.unlock_with_recovery(&kit.recovery_key).unwrap();
    core.change_password(&kit.recovery_key, true, "שמש צהובה על הים הכחול")
        .unwrap();
    core.lock();
    assert!(core.unlock("שמש צהובה על הים הכחול").is_ok());
    drop(dir);
}

#[test]
fn the_activity_log_names_cases_only_here_and_reads_in_pages() {
    let (_dir, mut core, case) = setup(None);
    core.delete_case(&case).unwrap();
    let page = core.activity(None).unwrap();
    assert!(page.intact && page.reviewed_at.is_none());
    let trashed = page
        .entries
        .iter()
        .find(|e| e.event == "case_trashed")
        .unwrap();
    assert!(trashed.case.as_deref().unwrap().contains("אלון"));
    assert!(trashed.case.as_deref().unwrap().contains("בסל המחזור"));
    assert!(page.entries.iter().all(|e| !e.text.contains('_')));
    // The program's own bookkeeping is not shown.
    assert!(page.entries.iter().all(|e| e.text != "הגדרה עודכנה"));

    core.purge_case(&case, PASSWORD).unwrap();
    let page = core.activity(None).unwrap();
    let created = page
        .entries
        .iter()
        .find(|e| e.event == "case_created")
        .unwrap();
    assert_eq!(created.case.as_deref(), Some("תיק שנמחק"));

    core.mark_activity_reviewed().unwrap();
    let page = core.activity(None).unwrap();
    assert!(page.reviewed_at.is_some());
    assert_eq!(page.entries[0].event, "audit_reviewed");
    assert!(!page.more && page.last_seq.is_some());
}

#[test]
fn a_case_past_its_retention_date_is_pointed_out_and_never_erased() {
    let (_dir, mut core, case) = setup(None);
    assert!(core.retention_due().unwrap().is_empty());
    let detail = core.case_detail(&case).unwrap();
    // Age 5 at the assessment: kept until the child is 25.
    assert!(
        detail.retention_default.as_str() > "2045-01-01",
        "{}",
        detail.retention_default
    );

    let mut meta = detail.meta.clone();
    meta.retention_until = Some("2020-01-01".into());
    core.update_case(&case, meta).unwrap();
    let due = core.retention_due().unwrap();
    assert_eq!(due.len(), 1);
    assert!(due[0].label.contains("אלון") && !due[0].by_default);
    assert_eq!(core.list_cases().unwrap().len(), 1, "nothing is erased");

    core.keep_case_longer(&case, 1).unwrap();
    assert!(core.retention_due().unwrap().is_empty());
    assert!(
        core.case_detail(&case)
            .unwrap()
            .meta
            .retention_until
            .unwrap()
            .as_str()
            > "2027-01-01"
    );
}

#[test]
fn a_send_is_logged_even_when_the_reply_is_unusable() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"not": "a sorting"})));
    assert!(core.send_sort(&prepared.approval_id.unwrap()).is_err());
    // The filtered materials did leave the computer: the log says so.
    let page = core.activity(None).unwrap();
    assert!(page.entries.iter().any(|e| e.event == "send"));
}

#[test]
fn an_edited_material_that_was_placed_nowhere_is_offered_for_sorting_again() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(claude_sorting());
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    let detail = core.case_detail(&case).unwrap();
    let (input, routing) = (&detail.inputs[1], &detail.routing[1]);
    assert!(!routing.sorted && !routing.needs_sorting);
    core.update_input(
        &case,
        &input.id,
        &input.title,
        &format!("{}\nועוד שורה.", input.content),
    )
    .unwrap();
    let detail = core.case_detail(&case).unwrap();
    assert!(detail.routing[1].needs_sorting);
}

#[test]
fn a_lock_after_sleep_is_one_entry_with_its_reason() {
    let (_dir, mut core, _case) = setup(None);
    let t0 = SystemTime::now();
    core.tick(t0);
    assert!(core.tick(t0 + Duration::from_secs(400)));
    core.unlock(PASSWORD).unwrap();
    let page = core.activity(None).unwrap();
    let locks: Vec<_> = page.entries.iter().filter(|e| e.event == "lock").collect();
    assert_eq!(locks.len(), 1, "{locks:?}");
    assert!(locks[0].text.contains("שינה"));
}

#[test]
fn a_general_conversation_continues_and_earlier_turns_are_reviewed() {
    let (_dir, mut core, _case) = setup(None);
    let p = core
        .prepare_consult(None, None, "מה ההבדל בין WPPSI-IV ל-WISC-V?")
        .unwrap();
    let first = core.send_consult(&p.approval_id.unwrap()).unwrap();
    let p = core
        .prepare_consult(None, Some(&first.conversation_id), "ובגיל 6 בדיוק?")
        .unwrap();
    // The earlier question and answer are on the review screen, not only the new one.
    let labels: Vec<&str> = p.parts.iter().map(|x| x.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("שאלה קודמת")),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("תשובה קודמת")),
        "{labels:?}"
    );
    let again = core.send_consult(&p.approval_id.unwrap()).unwrap();
    assert_eq!(again.conversation_id, first.conversation_id);
}

#[test]
fn an_answer_to_a_deleted_conversation_is_kept_in_a_new_one() {
    let (_dir, mut core, case) = setup(None);
    let p = core
        .prepare_consult(Some(&case), None, "שאלה ראשונה")
        .unwrap();
    let first = core.send_consult(&p.approval_id.unwrap()).unwrap();
    let p = core
        .prepare_consult(Some(&case), Some(&first.conversation_id), "ועוד שאלה")
        .unwrap();
    // Deleted while the answer is on its way.
    core.delete_consultation(&first.conversation_id).unwrap();
    let answer = core.send_consult(&p.approval_id.unwrap()).unwrap();
    assert_ne!(answer.conversation_id, first.conversation_id);
    assert_eq!(
        core.consultation(&answer.conversation_id)
            .unwrap()
            .turns
            .len(),
        2
    );
    assert!(core
        .activity(None)
        .unwrap()
        .entries
        .iter()
        .any(|e| e.event == "consultation_deleted"));
}

/// The sample documents Or and Einat try the app with (`tests/samples/make_samples.py`) go
/// through the real import, filter and gate: nothing personal leaves, the name the case does
/// not list is asked about, and the scanned page is refused.
#[test]
fn the_sample_documents_leak_nothing() {
    let out = tempfile::tempdir().unwrap();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/samples/make_samples.py"
    );
    let python = if cfg!(windows) { "python" } else { "python3" };
    let status = std::process::Command::new(python)
        .arg(script)
        .arg(out.path())
        .stdout(std::process::Stdio::null())
        .status()
        .expect("python runs the sample generator");
    assert!(status.success());

    let fake = FakeTransport::default();
    let (_dir, mut core, _) = setup(Some(fake.clone()));
    let person = |role, value: &str| IdentityInput {
        id: None,
        role,
        value: value.into(),
        aliases: vec![],
    };
    let meta = CaseMeta {
        code: "TEST-0003".into(),
        age: Some(Age {
            years: 5,
            months: 8,
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
                    aliases: vec!["בדיוני".into()],
                    ..person(Role::Child, "אלון")
                },
                person(Role::Mother, "שירה"),
                person(Role::Father, "גיא"),
                person(Role::Sister, "נוגה"),
                person(Role::Teacher, "אורית"),
                person(Role::Kindergarten, "גן השקד"),
                person(Role::Town, "גבעת הרימון"),
                person(Role::Slp, "ליאת"),
                person(Role::Therapist, "דפנה"),
                person(Role::Doctor, "ד\"ר יואב רון"),
            ],
        )
        .unwrap();

    let follow = out.path().join("מעקב - שנה אחרי");
    let mut files: Vec<std::path::PathBuf> = [out.path(), follow.as_path()]
        .iter()
        .flat_map(|d| std::fs::read_dir(d).unwrap().flatten().map(|e| e.path()))
        .filter(|p| p.is_file() && !p.to_string_lossy().contains("קרא אותי"))
        .collect();
    files.sort();
    assert!(files.len() >= 10, "{files:?}");
    let mut stranger_asked = false;
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        match core.import_document(&case, &name, &bytes) {
            Ok(p) => {
                stranger_asked |= p.suspects.iter().any(|s| s.token.contains("עומרי"));
                core.add_input(&case, p.suggested_kind, &p.title, &p.body)
                    .unwrap();
            }
            Err(e) => assert!(name.contains("סרוק"), "{name}: {}", e.to_ui().message),
        }
    }
    assert!(
        stranger_asked,
        "the friend the case does not list is asked about"
    );

    // Einat's answers: the friend is hidden, "שאלון" here is "that Alon", the rest are words.
    let mut prepared = core.prepare_sort(&case).unwrap();
    let mut asked: Vec<String> = Vec::new();
    for _ in 0..5 {
        if prepared.suspects.is_empty() {
            break;
        }
        for s in &prepared.suspects {
            asked.push(s.token.clone());
            let decision = if s.token.contains("עומרי") {
                SuspectDecision::Hide {
                    role: Role::OtherChild,
                }
            } else if s.token == "שאלון" {
                SuspectDecision::IsName
            } else {
                SuspectDecision::NotAName
            };
            core.decide_suspect(&case, &s.token, decision).unwrap();
        }
        prepared = core.prepare_sort(&case).unwrap();
    }
    assert!(asked.len() <= 4, "few questions, each once: {asked:?}");
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    core.send_sort(&prepared.approval_id.expect("clears the gate"))
        .unwrap();

    let sent = fake.sent.lock().unwrap().join("\n");
    assert!(sent.contains("[ילד]"));
    let phone = ["050", "000", "0000"].join("-");
    let mail = ["shira.fake", "example.com"].join("@");
    for secret in [
        "אלון",
        "שירה",
        "גיא ",
        "נוגה",
        "אורית",
        "השקד",
        "הרימון",
        "ליאת",
        "דפנה",
        "יואב",
        "עומרי",
        "בדיוני",
        "000000026",
        &phone,
        &mail,
        "הזית 4",
        "לא לשלוח",
        "הפרעת קשב",
    ] {
        assert!(!sent.contains(secret), "{secret} left the machine");
    }
}

/// One draft at a time: a new request replaces the paragraphs not approved yet, a rewrite
/// stays in its place, and approving the draft approves what is waiting.
#[test]
fn a_new_draft_replaces_the_waiting_one_and_a_rewrite_stays_in_place() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let waiting = |core: &mut Core| -> Vec<(String, String)> {
        core.case_detail(&case)
            .unwrap()
            .sections
            .into_iter()
            .find(|s| s.key == "kindergarten")
            .unwrap()
            .paragraphs
            .into_iter()
            .filter(|p| p.status == DraftStatus::Proposed)
            .map(|p| (p.id, p.text))
            .collect()
    };
    let answer = |texts: &[&str]| {
        api_json(&json!({
            "reply": "הנה",
            "paragraphs": texts.iter().map(|t| json!({"text": t, "source_refs": ["S1"]})).collect::<Vec<_>>(),
            "questions": [], "missing": [], "contradictions": [],
        }))
    };
    let send = |core: &mut Core, replaces: Option<&str>| {
        let p = core
            .prepare_section_replacing(&case, "kindergarten", "טיוטה", replaces)
            .unwrap();
        core.send_section(&p.approval_id.unwrap()).unwrap();
    };

    *fake.answer.lock().unwrap() = Some(answer(&["פסקה א", "פסקה ב"]));
    send(&mut core, None);
    let first = waiting(&mut core);
    assert_eq!(first.len(), 2);
    core.approve_paragraph(&case, &first[0].0).unwrap();

    // A second draft replaces the one still waiting; the approved paragraph stays.
    *fake.answer.lock().unwrap() = Some(answer(&["פסקה ג", "פסקה ד", "פסקה ה"]));
    send(&mut core, None);
    let second = waiting(&mut core);
    assert_eq!(
        second.iter().map(|p| p.1.as_str()).collect::<Vec<_>>(),
        ["פסקה ג", "פסקה ד", "פסקה ה"]
    );

    // "ניסוח מחדש" rewrites one paragraph where it is.
    *fake.answer.lock().unwrap() = Some(answer(&["פסקה ד בניסוח אחר"]));
    send(&mut core, Some(&second[1].0));
    let third = waiting(&mut core);
    assert_eq!(
        third[1],
        (second[1].0.clone(), "פסקה ד בניסוח אחר".to_owned())
    );
    assert_eq!(third.len(), 3);

    assert_eq!(core.approve_section(&case, "kindergarten").unwrap(), 3);
    assert!(waiting(&mut core).is_empty());
    let section = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .unwrap();
    assert_eq!(
        section.paragraphs.len(),
        4,
        "1 + 3 approved, the replaced ones gone"
    );
}

/// A follow-up assessment: the names come along, the consent does not, and the indexes
/// entered in both are compared and can become a material.
#[test]
fn a_follow_up_takes_the_names_and_compares_the_scores() {
    let (_dir, mut core, before) = setup(None);
    let sheet = |inst: &str, entries: &[(&str, f64)]| dv_domain::ScoreSheet {
        instrument: inst.into(),
        module: String::new(),
        cutoff: None,
        entries: entries
            .iter()
            .map(|(m, v)| dv_domain::ScoreEntry {
                measure: (*m).into(),
                value: *v,
                note: String::new(),
            })
            .collect(),
        notes: String::new(),
    };
    core.save_scores(
        &before,
        None,
        &sheet("wppsi_iv", &[("fsiq", 98.0), ("wmi", 86.0)]),
    )
    .unwrap();

    let after = core.create_follow_up(&before).unwrap();
    let detail = core.case_detail(&after).unwrap();
    assert_eq!(detail.meta.follows.as_deref(), Some(before.as_str()));
    assert!(
        detail.meta.consent.is_none(),
        "a new assessment needs its own consent"
    );
    assert!(detail.meta.code.ends_with("מעקב"));
    assert!(detail.identities.iter().any(|i| i.value == "אלון"));

    assert!(core.follow_up(&after).unwrap().unwrap().rows.is_empty());
    assert!(core.add_comparison_material(&after).is_err());
    core.save_scores(
        &after,
        None,
        &sheet("wisc_v", &[("fsiq", 101.0), ("wmi", 97.0)]),
    )
    .unwrap();
    let view = core.follow_up(&after).unwrap().unwrap();
    assert_eq!(view.rows.len(), 2);
    assert!(view.rows.iter().any(|r| r.abbr == "WMI" && r.notable));
    let m = core.add_comparison_material(&after).unwrap();
    assert!(m.content.contains("86 ← 97"), "{}", m.content);
    // Saving again replaces it.
    core.add_comparison_material(&after).unwrap();
    assert_eq!(
        core.case_detail(&after)
            .unwrap()
            .inputs
            .iter()
            .filter(|i| i.title == "השוואה לאבחון הקודם")
            .count(),
        1
    );
    assert!(core.follow_up(&before).unwrap().is_none());
}

/// "זה לא שם, להשאיר" must stick for every kind of question: after one answer to each, the
/// same request asks nothing again (a question that comes back looks like a frozen screen).
#[test]
fn every_not_a_name_answer_sticks_after_one_round() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    for text in [
        // A spelling close to a declared name, behind a prefix ("ה" + "ריון" ~ "רון").
        "ההריון עבר בשלום. ד\"ר רון בדק אותו.",
        // A profession, flagged with the words after it.
        "האם עובדת כמנהלת חשבונות בבנק.",
        // A capitalized Latin word, a first name behind a prefix, a word-name in context.
        "He likes Lego. ולשירן אין קשר. הילד שיחק עם גל בחצר.",
        // After a label.
        "לכבוד הצוות החינוכי, שם: פלוני אלמוני",
    ] {
        core.add_input(&case, InputKind::FreeText, "הערה", text)
            .unwrap();
    }
    let first = core.prepare_sort(&case).unwrap();
    assert!(!first.suspects.is_empty());
    for s in &first.suspects {
        core.decide_suspect(&case, &s.token, SuspectDecision::NotAName)
            .unwrap();
    }
    let again = core.prepare_sort(&case).unwrap();
    let back: Vec<&str> = again.suspects.iter().map(|s| s.token.as_str()).collect();
    assert!(back.is_empty(), "asked again after answering: {back:?}");
}

/// A follow-up shares its names with the case before: sending from it must work, with this
/// case's tags, and nothing personal leaves.
#[test]
fn a_follow_up_sends_with_its_own_tags() {
    let fake = FakeTransport::default();
    let (_dir, mut core, before) = setup(Some(fake.clone()));
    let after = core.create_follow_up(&before).unwrap();
    let mut meta = core.case_detail(&after).unwrap().meta;
    meta.consent = Some(consent());
    meta.age = Some(Age {
        years: 6,
        months: 5,
    });
    core.update_case(&after, meta).unwrap();
    core.add_input(
        &after,
        InputKind::Kindergarten,
        "שיחה עם המורה",
        "שירה סיפרה כי אלון השתלב בכיתה ומשתתף בשיעורים.",
    )
    .unwrap();
    let prepared = core
        .prepare_section(&after, "kindergarten", "טיוטה")
        .unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    let approval = prepared.approval_id.expect("clears the gate");
    core.send_section(&approval).unwrap();
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    for name in ["אלון", "שירה"] {
        assert!(!sent.contains(name), "{name} left the machine");
    }
    assert!(sent.contains("[ילד]"), "this case's own tag");
}

/// "להסתיר" also settles every kind of question in one round, and the request then clears.
#[test]
fn every_hide_answer_sticks_after_one_round() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    core.add_input(
        &case,
        InputKind::FreeText,
        "הערה",
        "האם עובדת כמנהלת חשבונות בבנק. He likes Lego. ולשירן אין קשר. לכבוד הצוות, שם: פלוני אלמוני",
    )
    .unwrap();
    let first = core.prepare_sort(&case).unwrap();
    for s in &first.suspects {
        core.decide_suspect(
            &case,
            &s.token,
            SuspectDecision::Hide {
                role: s.suggested_role,
            },
        )
        .unwrap();
    }
    let again = core.prepare_sort(&case).unwrap();
    let back: Vec<&str> = again.suspects.iter().map(|s| s.token.as_str()).collect();
    assert!(back.is_empty(), "asked again after hiding: {back:?}");
    assert!(again.blocked.is_empty(), "{:?}", again.blocked);
    assert!(again.approval_id.is_some());
}

/// Words the model put in square brackets ("[מחנכת]") and she approved are not tags: the
/// summary, written from the approved sections, still goes out, the words filtered as text.
#[test]
fn bracketed_words_in_approved_text_do_not_block_the_summary() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_own_paragraph(
        &case,
        "kindergarten",
        "לדברי [מחנכת] בגן, אלון משתתף יותר בקבוצה קטנה ב[גן השקד].",
    )
    .unwrap();
    let prepared = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    core.send_section(&prepared.approval_id.expect("clears the gate"))
        .unwrap();
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(sent.contains("(מחנכת)") && !sent.contains("אלון"), "{sent}");
}

/// "עובדת" is asked about only when a job or a workplace follows it.
#[test]
fn work_words_are_asked_about_only_before_a_job() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    core.add_input(
        &case,
        InputKind::FreeText,
        "הערה",
        "הגננת עובדת איתו על המעברים. מנהלת הגן הצטרפה לשיחה. האם עובדת כמנהלת חשבונות, והאב עובד בבנק.",
    )
    .unwrap();
    let asked: Vec<String> = core
        .prepare_sort(&case)
        .unwrap()
        .suspects
        .into_iter()
        .filter(|s| s.kind == dv_privacy::SuspectKind::Indirect)
        .map(|s| s.token)
        .collect();
    assert!(
        asked.iter().any(|t| t.starts_with("עובדת כמנהלת")),
        "{asked:?}"
    );
    assert!(
        asked.iter().any(|t| t.starts_with("עובד בבנק")),
        "{asked:?}"
    );
    assert!(
        !asked
            .iter()
            .any(|t| t.contains("איתו") || t.contains("הגן")),
        "{asked:?}"
    );
}

/// A section title that holds the child's name ("שאלון הסתגלות" for "אלון") is template text:
/// the section and the summary built on it go out without a question, and the name does not.
#[test]
fn a_section_title_holding_the_childs_name_blocks_nothing() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let abas = core
        .add_input(
            &case,
            InputKind::FreeText,
            "הערה על ABAS",
            "ההורים מילאו את ABAS. התפקוד המעשי בטווח הממוצע.",
        )
        .unwrap()
        .id;
    core.set_input_sections(&case, &abas, &["adaptive".to_owned()])
        .unwrap();
    let prepared = core.prepare_section(&case, "adaptive", "טיוטה").unwrap();
    assert!(prepared.suspects.is_empty(), "{:?}", prepared.suspects);
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    core.send_section(&prepared.approval_id.unwrap()).unwrap();
    core.approve_section(&case, "adaptive").unwrap();

    let summary = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    assert!(summary.blocked.is_empty(), "{:?}", summary.blocked);
    core.send_section(&summary.approval_id.unwrap()).unwrap();
    for sent in fake.sent.lock().unwrap().iter() {
        assert!(
            !sent.contains("אלון"),
            "the child's name left inside a title"
        );
    }
}

/// Claude's new wording of an approved paragraph waits beside it: approving it takes the old
/// one's place, removing it leaves the old one as it was (D-032).
#[test]
fn a_new_wording_of_an_approved_paragraph_replaces_it_only_when_approved() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_own_paragraph(&case, "kindergarten", "פסקה מקורית.")
        .unwrap();
    let para = |core: &mut Core| {
        core.case_detail(&case)
            .unwrap()
            .sections
            .into_iter()
            .find(|s| s.key == "kindergarten")
            .unwrap()
            .paragraphs
    };
    let original = para(&mut core)[0].id.clone();
    let reword = |core: &mut Core, text: &str| {
        *fake.answer.lock().unwrap() = Some(api_json(&json!({
            "reply": "הנה", "paragraphs": [{"text": text, "source_refs": ["S1"]}],
            "questions": [], "missing": [], "contradictions": [],
        })));
        let p = core
            .prepare_section_replacing(&case, "kindergarten", "לקצר", Some(&original))
            .unwrap();
        core.send_section(&p.approval_id.unwrap()).unwrap();
    };

    reword(&mut core, "ניסוח ראשון.");
    let now = para(&mut core);
    assert_eq!(now.len(), 2);
    let new = now
        .iter()
        .find(|p| p.status == DraftStatus::Proposed)
        .unwrap();
    assert_eq!(new.replaces.as_deref(), Some(original.as_str()));
    // Removing it keeps the original.
    core.reject_paragraph(&case, &new.id.clone()).unwrap();
    assert_eq!(para(&mut core).len(), 1);

    reword(&mut core, "ניסוח שני.");
    let new = para(&mut core)
        .into_iter()
        .find(|p| p.status == DraftStatus::Proposed)
        .unwrap();
    core.approve_paragraph(&case, &new.id).unwrap();
    let after = para(&mut core);
    assert_eq!(after.len(), 1, "the new wording took the old one's place");
    assert_eq!(after[0].text, "ניסוח שני.");
}

/// Her own paragraph goes where she put it: first, between two paragraphs, or at the end,
/// filtered like any other text (D-036).
#[test]
fn her_own_paragraph_goes_where_she_put_it() {
    let (_dir, mut core, case) = setup(None);
    let texts = |core: &mut Core| -> Vec<String> {
        core.case_detail(&case)
            .unwrap()
            .sections
            .into_iter()
            .find(|s| s.key == "kindergarten")
            .unwrap()
            .paragraphs
            .into_iter()
            .map(|p| p.text)
            .collect()
    };
    let ids = |core: &mut Core| -> Vec<String> {
        core.case_detail(&case)
            .unwrap()
            .sections
            .into_iter()
            .find(|s| s.key == "kindergarten")
            .unwrap()
            .paragraphs
            .into_iter()
            .map(|p| p.id)
            .collect()
    };
    core.add_own_paragraph(&case, "kindergarten", "שתיים.")
        .unwrap();
    core.add_own_paragraph(&case, "kindergarten", "ארבע.")
        .unwrap();
    core.add_own_paragraph_at(&case, "kindergarten", "אחת.", Some(None))
        .unwrap();
    let after = ids(&mut core)[1].clone();
    core.add_own_paragraph_at(&case, "kindergarten", "שלוש.", Some(Some(&after)))
        .unwrap();
    let t = texts(&mut core);
    assert_eq!(t.len(), 4, "{t:?}");
    assert_eq!(t[0], "אחת.");
    assert_eq!(t[1], "שתיים.");
    assert_eq!(t[2], "שלוש.");
    assert_eq!(t[3], "ארבע.");
    // Emptied and saved: gone.
    let second = ids(&mut core)[1].clone();
    core.edit_paragraph(&case, &second, "  \n ").unwrap();
    assert_eq!(texts(&mut core), vec!["אחת.", "שלוש.", "ארבע."]);
}
