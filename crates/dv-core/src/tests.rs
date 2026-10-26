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

mod access;
mod binding;
mod flows;
mod guides;
mod history;
mod hostile;
mod import_export;
mod institute;
mod instruments;
mod large_case;
mod layers;
mod library;
mod masks;
mod off_lock;
mod own;
mod parents;
mod perf_profile;
mod reading;
mod report_check;
mod sample_flows;
mod sort_flows;
mod surfaces;
mod templates;
mod upgrade;
mod verify;

use import_export::read_docx_text;
use sort_flows::{add_home_intake, claude_sorting};

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

/// F6: every error offers one next step, and the one that fits it.
#[test]
fn every_error_has_a_next_step_that_fits_it() {
    let cases: Vec<(CoreError, ErrorAction)> = vec![
        (CoreError::Locked, ErrorAction::Unlock),
        (CoreError::ConsentMissing, ErrorAction::Consent),
        (
            CoreError::Backoff(8),
            ErrorAction::Retry { after_seconds: 8 },
        ),
        (
            CoreError::Egress(EgressError::NoApiKey),
            ErrorAction::settings("ai"),
        ),
        (
            CoreError::Egress(EgressError::Unauthorized),
            ErrorAction::settings("ai"),
        ),
        (
            CoreError::Egress(EgressError::Stopped),
            ErrorAction::settings("security"),
        ),
        (
            CoreError::Egress(EgressError::Offline),
            ErrorAction::retry(),
        ),
        (
            CoreError::Egress(EgressError::RateLimited),
            ErrorAction::Retry { after_seconds: 60 },
        ),
        (
            CoreError::Egress(EgressError::TooLarge),
            ErrorAction::Dismiss,
        ),
        // Only what passes by itself offers a retry; the rest would fail the same way.
        (
            CoreError::Egress(EgressError::Unavailable(529)),
            ErrorAction::retry(),
        ),
        (
            CoreError::Egress(EgressError::Rejected(408, "x".into())),
            ErrorAction::retry(),
        ),
        (
            CoreError::Egress(EgressError::Rejected(400, "x".into())),
            ErrorAction::Report,
        ),
        (
            CoreError::Egress(EgressError::Policy("x".into())),
            ErrorAction::Report,
        ),
        (CoreError::Egress(EgressError::Tls), ErrorAction::Report),
        // A certificate from an authority this version is not pinned to: an update (D-077).
        (
            CoreError::Egress(EgressError::Pinned),
            ErrorAction::settings("about"),
        ),
        (
            CoreError::Vault(VaultError::OtherComputer),
            ErrorAction::Dismiss,
        ),
        (
            CoreError::Egress(EgressError::Protocol("x".into())),
            ErrorAction::Report,
        ),
        // A spent approval: sending it again would do nothing.
        (
            CoreError::Ai(dv_ai::AiError::Format("x".into())),
            ErrorAction::Dismiss,
        ),
        (CoreError::Internal("x".into()), ErrorAction::Report),
        (
            CoreError::Vault(VaultError::Corrupt("x".into())),
            ErrorAction::Report,
        ),
        (
            CoreError::Vault(VaultError::WrongSecret),
            ErrorAction::Dismiss,
        ),
        (CoreError::Refused("x".into()), ErrorAction::Dismiss),
        (
            CoreError::Needs("x".into(), ErrorAction::retry()),
            ErrorAction::retry(),
        ),
        // D-121: what the service said, each to the place that fixes it.
        (
            CoreError::Egress(EgressError::Overloaded),
            ErrorAction::retry(),
        ),
        (
            CoreError::Egress(EgressError::Billing("x".into())),
            ErrorAction::settings("ai"),
        ),
        (
            CoreError::Egress(EgressError::Forbidden("x".into())),
            ErrorAction::settings("ai"),
        ),
        (
            CoreError::Egress(EgressError::ModelGone("x".into())),
            ErrorAction::settings("about"),
        ),
        (
            CoreError::Egress(EgressError::InvalidRequest("x".into())),
            ErrorAction::Report,
        ),
        // D-122: the breaker's own wait.
        (
            CoreError::Egress(EgressError::CircuitOpen(42)),
            ErrorAction::Retry { after_seconds: 42 },
        ),
    ];
    for (e, want) in cases {
        let ui = e.to_ui();
        assert_eq!(ui.action, want, "{e:?}");
        assert!(!ui.message.is_empty());
    }
}

/// D-121: every failure of the AI is told in Hebrew, naming the AI she chose; the service's
/// own English words (and the parser's) are only in the details she can copy.
#[test]
fn ai_failures_are_told_in_hebrew_with_the_service_words_only_in_the_details() {
    let server = "Your credit balance is too low to access the Anthropic API";
    let cases: Vec<(CoreError, &str)> = vec![
        (CoreError::Egress(EgressError::Overloaded), "עמוס"),
        (
            CoreError::Egress(EgressError::Unavailable(503)),
            "תקלה זמנית",
        ),
        (CoreError::Egress(EgressError::CircuitOpen(42)), "42 שניות"),
        (
            CoreError::Egress(EgressError::Billing(server.into())),
            "קרדיט",
        ),
        (
            CoreError::Egress(EgressError::Forbidden(server.into())),
            "הרשאה",
        ),
        (
            CoreError::Egress(EgressError::ModelGone(server.into())),
            "כבר לא זמין",
        ),
        (
            CoreError::Egress(EgressError::InvalidRequest(server.into())),
            "דחה את הבקשה",
        ),
        (
            CoreError::Egress(EgressError::Rejected(418, server.into())),
            "דחה את הבקשה",
        ),
        (
            CoreError::Egress(EgressError::Protocol(server.into())),
            "לא צפויה",
        ),
        (
            CoreError::Egress(EgressError::Policy(server.into())),
            "שום דבר לא נשלח",
        ),
        (
            CoreError::Ai(dv_ai::AiError::Format(
                "expected value at line 1 column 1".into(),
            )),
            "לא צפויה",
        ),
        (
            CoreError::Ai(dv_ai::AiError::Refused("refusal".into())),
            "סירב",
        ),
        (CoreError::Ai(dv_ai::AiError::Truncated), "נקטעה"),
    ];
    for (e, words) in cases {
        let ui = e.to_ui_for("ChatGPT");
        assert!(ui.message.contains(words), "{e:?}: {}", ui.message);
        // No English but the AI's name (and "API", in capitals).
        assert!(
            !ui.message
                .replace("ChatGPT", "")
                .chars()
                .any(|c| c.is_ascii_lowercase()),
            "{e:?}: {}",
            ui.message
        );
        assert!(
            !ui.message.contains("Claude") && !ui.message.contains("שגיאה בחיבור"),
            "{e:?}: {}",
            ui.message
        );
        assert!(!ui.details.is_empty(), "{e:?}: the details keep the cause");
    }
    let ui = CoreError::Egress(EgressError::Billing(server.into())).to_ui();
    assert!(
        ui.details.iter().any(|d| d.contains(server)),
        "{:?}",
        ui.details
    );
    assert!(ui.message.contains("Claude"));
}

#[test]
fn ping_reports_versions() {
    let p = ping();
    assert_eq!(p.ipc_version, IPC_VERSION);
    assert!(!p.core_version.is_empty());
}

#[test]
fn model_allow_lists_agree() {
    assert_eq!(dv_ai::ANTHROPIC_MODELS, dv_egress::ALLOWED_MODELS);
    assert_eq!(dv_ai::OPENAI_MODELS, dv_egress::OPENAI_MODELS);
    assert_eq!(dv_ai::GEMINI_MODELS, dv_egress::GEMINI_MODELS);
    assert_eq!(dv_ai::MISTRAL_MODELS, dv_egress::MISTRAL_MODELS);
    assert_eq!(dv_ai::LOCAL_MODELS, dv_egress::LOCAL_MODELS);
    assert!(dv_ai::ANTHROPIC_MODELS.contains(&dv_ai::DEFAULT_MODEL));
}

#[test]
fn the_chosen_ai_names_itself_keeps_its_own_key_and_still_goes_through_the_gate() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    assert_eq!(core.status().ai_name, "Claude");
    assert_eq!(core.status().provider, "anthropic");

    core.set_model("gemini-2-5-pro").unwrap();
    let st = core.status();
    assert_eq!(
        (st.provider.as_str(), st.ai_name.as_str()),
        ("gemini", "Gemini")
    );
    assert_eq!(core.ai_name(), "Gemini");
    assert!(CoreError::ConsentMissing
        .to_ui_for(core.ai_name())
        .message
        .contains("Gemini"));

    // A ChatGPT or Gemini key needs her confirmation of what the company keeps.
    assert!(matches!(
        core.set_api_key("gemini", "test-not-real", false),
        Err(CoreError::Refused(_))
    ));
    core.set_api_key("gemini", "test-not-real", true).unwrap();
    core.set_api_key("anthropic", "sk-ant-test-not-real", false)
        .unwrap();
    let mut keys = core.status().keys;
    keys.sort();
    assert_eq!(keys, vec!["anthropic", "gemini"]);
    assert!(core.set_api_key("deepseek", "x", true).is_err());
    core.set_api_key("gemini", "", false).unwrap();
    assert_eq!(core.status().keys, vec!["anthropic"]);

    // The same filtered request, with the chosen model in it: the gate cleared this body.
    let prepared = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה על הוויסות הרגשי")
        .unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    assert!(prepared.suspects.is_empty(), "{:?}", prepared.suspects);
    core.send_section(&prepared.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().last().cloned().unwrap();
    let body: Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(body["model"], "gemini-2-5-pro");
    assert!(
        !sent.contains("אלון"),
        "names are hidden whichever AI answers"
    );
    let log = core.activity(None).unwrap();
    assert!(
        log.entries.iter().any(|e| e.text.contains("נשלח ל-Gemini")),
        "the log names the AI that was used"
    );
}

#[test]
fn without_a_key_for_the_chosen_ai_it_is_demo_mode() {
    let (_dir, mut core, _case) = setup(None);
    core.set_api_key("anthropic", "sk-ant-test-not-real", false)
        .unwrap();
    assert!(!core.status().demo_mode);
    core.set_model("gpt-5-1").unwrap();
    assert!(core.status().demo_mode, "a Claude key is not a ChatGPT key");
    assert_eq!(core.status().ai_name, "ChatGPT");

    // The local model needs no key: never demo mode, and no key can be saved for it.
    core.set_model("local-gemma").unwrap();
    assert!(!core.status().demo_mode);
    assert_eq!(core.status().ai_name, "Ollama");
    assert!(core.set_api_key("local", "x", true).is_err());
    // Mistral is a company abroad like the others: its key needs the confirmation.
    assert!(core.set_api_key("mistral", "x", false).is_err());
    assert!(core.set_api_key("mistral", "x", true).is_ok());
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
    let ui = err.to_ui();
    assert_eq!(ui.code, "refused");
    // The message offers the raised ceiling, and the button raises it to exactly that.
    assert_eq!(ui.action, ErrorAction::RaiseCap { to_usd: 20 });
    assert!(ui.message.contains("$20"), "{}", ui.message);
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
    // Raising the ceiling lets the same approval go out, without preparing it again.
    core.set_monthly_cap(Some(20)).unwrap();
    core.send_section(&second).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 2);
    core.set_monthly_cap(None).unwrap();
    assert_eq!(core.usage_summary().unwrap().cap_usd, None);
}

#[test]
fn the_raised_ceiling_is_half_again_at_least_ten_in_fives() {
    assert_eq!(send::raised_cap(10), 20);
    assert_eq!(send::raised_cap(7), 20);
    assert_eq!(send::raised_cap(30), 45);
    assert_eq!(send::raised_cap(100), 150);
    // No overflow at the top: it stays at the largest whole five.
    assert_eq!(send::raised_cap(u32::MAX), u32::MAX);
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

/// Prompt caching: the second turn about the same section starts with the very same
/// materials block, marked for the cache, so the API serves it from there.
#[test]
fn a_second_turn_on_a_section_starts_with_the_same_cached_materials() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    for ask in ["תנסח פסקה על הוויסות הרגשי", "לקצר את הפסקה"] {
        let prepared = core.prepare_section(&case, "kindergarten", ask).unwrap();
        core.send_section(&prepared.approval_id.unwrap()).unwrap();
    }
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    let first_block = |b: &str| {
        let v: Value = serde_json::from_str(b).unwrap();
        v["messages"][0]["content"][0].clone()
    };
    let (a, b) = (first_block(&sent[0]), first_block(&sent[1]));
    assert_eq!(a["cache_control"]["type"], "ephemeral");
    assert_eq!(
        a, b,
        "the cached materials are the same bytes on the next turn"
    );
    assert!(sent[1].contains("לקצר"));
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

/// The review of stage 2: a request prepared before her consent was removed, or before a
/// person was added to the case, never goes out (also not as a retry): it was filtered with
/// what the case knew then.
#[test]
fn a_request_prepared_before_the_case_changed_does_not_go_out() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    let mut meta = core.case_detail(&case).unwrap().meta;
    meta.consent = None;
    core.update_case(&case, meta).unwrap();
    assert!(core.send_section(&p.approval_id.unwrap()).is_err());

    let mut meta = core.case_detail(&case).unwrap().meta;
    meta.consent = Some(consent());
    core.update_case(&case, meta).unwrap();
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    let mut people: Vec<IdentityInput> = core
        .case_detail(&case)
        .unwrap()
        .identities
        .into_iter()
        .map(|i| IdentityInput {
            id: Some(i.id),
            role: i.role,
            value: i.value,
            aliases: i.aliases,
        })
        .collect();
    people.push(IdentityInput {
        id: None,
        role: Role::Other,
        value: "בתיה".into(),
        aliases: vec![],
    });
    core.set_identities(&case, people).unwrap();
    assert!(matches!(
        core.send_section(&p.approval_id.unwrap()),
        Err(CoreError::NotFound(_))
    ));
    assert!(fake.sent.lock().unwrap().is_empty());
}

/// Everything the review screen shows as going out, joined.
fn outgoing(p: &Prepared) -> String {
    p.parts
        .iter()
        .flat_map(|part| part.outgoing.iter().map(|s| s.text.as_str()))
        .collect()
}

#[test]
fn a_name_confirmed_for_the_case_leaves_the_card_and_stays_hidden() {
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
    let item = prepared
        .auto_hidden
        .iter()
        .find(|a| a.token == "ורד")
        .expect("listed on the card")
        .clone();

    core.confirm_auto_hidden(&case, &item.token, &item.tag)
        .unwrap();
    let again = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(again.approval_id.is_some(), "{:?}", again.blocked);
    assert!(
        again.auto_hidden.iter().all(|a| a.tag != item.tag),
        "{:?}",
        again.auto_hidden
    );
    assert!(!outgoing(&again).contains("ורד"));
    let kept = core
        .case_detail(&case)
        .unwrap()
        .identities
        .into_iter()
        .find(|i| i.value == "ורד")
        .expect("still a name of the case");
    assert_eq!(kept.source, IdentitySource::Manual);

    // A close spelling becomes one of the name's forms.
    core.confirm_auto_hidden(&case, "ורדד", &item.tag).unwrap();
    let kept = core
        .case_detail(&case)
        .unwrap()
        .identities
        .into_iter()
        .find(|i| i.value == "ורד")
        .unwrap();
    assert!(kept.aliases.contains(&"ורדד".to_owned()));
    // Only a person of the case can be confirmed.
    assert!(matches!(
        core.confirm_auto_hidden(&case, "עובדת בתחום החינוך", "עובדת בתחום החינוך"),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn unknown_name_is_hidden_kept_and_restored_with_one_click() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "שיחה נוספת",
        "הסייעת ורד אמרה שהוא משחק לבד בחצר.",
    )
    .unwrap();
    // Kept with the case when the material is saved, with the role its context gives.
    let kept = core
        .case_detail(&case)
        .unwrap()
        .identities
        .into_iter()
        .find(|i| i.value == "ורד")
        .expect("kept");
    assert_eq!(kept.role, Role::Assistant);
    assert_eq!(kept.source, IdentitySource::Auto);
    assert!(kept.reason.contains("הסייעת"), "{}", kept.reason);

    let prepared = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    let id = prepared
        .approval_id
        .clone()
        .unwrap_or_else(|| panic!("{:?}", prepared.blocked));
    assert!(prepared.suspects.is_empty());
    let item = prepared
        .auto_hidden
        .iter()
        .find(|a| a.token == "ורד")
        .expect("listed on the card");
    assert_eq!(item.tag, kept.tag);
    assert!(outgoing(&prepared).contains(&format!("הסייעת {}", kept.tag)));
    core.send_section(&id).unwrap();
    assert!(!fake.sent.lock().unwrap()[0].contains("ורד"));

    // The role menu: a teacher, not an assistant.
    let ids = core.change_role(&case, &kept.tag, Role::Teacher).unwrap();
    let teacher = ids
        .iter()
        .find(|i| i.value == "ורד" && i.role == Role::Teacher);
    assert!(teacher.is_some(), "{ids:?}");

    // "להחזיר": the word goes out as written from now on, and is not found again.
    let tag = teacher.unwrap().tag.clone();
    core.restore_auto_hidden(&case, "ורד", &tag).unwrap();
    let again = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(
        again.auto_hidden.iter().all(|a| a.token != "ורד"),
        "{:?}",
        again.auto_hidden
    );
    assert!(outgoing(&again).contains("הסייעת ורד"));
    assert!(again.approval_id.is_some(), "{:?}", again.blocked);
}

#[test]
fn names_kept_earlier_stay_on_the_card_wherever_they_go_out() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "שיחה נוספת",
        "הדודה מירב אוספת אותו מהגן בימי שלישי.",
    )
    .unwrap();
    let kept = core
        .case_detail(&case)
        .unwrap()
        .identities
        .into_iter()
        .find(|i| i.value == "מירב")
        .expect("kept");
    assert_eq!(kept.source, IdentitySource::Auto);
    // Kept as a name of the case, so this build finds nothing new; the card still lists it.
    let prepared = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(prepared.approval_id.is_some(), "{:?}", prepared.blocked);
    assert!(outgoing(&prepared).contains(&kept.tag));
    let item = prepared
        .auto_hidden
        .iter()
        .find(|a| a.tag == kept.tag)
        .expect("listed on the card");
    assert_eq!(item.token, "מירב");
    assert_eq!(item.reason, kept.reason);
    // A name Einat entered herself is not on the card.
    assert!(!prepared.auto_hidden.iter().any(|a| a.token == "שירה"));
}

#[test]
fn a_scanned_upload_hides_a_misread_of_the_childs_name_but_a_typed_one_does_not() {
    let (_dir, mut core, case) = setup(None);
    // Fabricated OCR output: "אלון" read as "אלוז".
    let extracted = |ocr: bool| dv_ingest::Extracted {
        format: if ocr {
            dv_ingest::Format::Image
        } else {
            dv_ingest::Format::Text
        },
        body: "אלוז שיחק בחול עם הילדים.".to_owned(),
        margins: String::new(),
        metadata: Vec::new(),
        warnings: Vec::new(),
        pages: 1,
        ocr,
    };
    let typed = core
        .preview_extracted(&case, "שיחה.txt", extracted(false))
        .unwrap();
    assert!(
        !typed.auto_hidden.iter().any(|a| a.token == "אלוז"),
        "{:?}",
        typed.auto_hidden
    );
    let scan = core
        .preview_extracted(&case, "סריקה.png", extracted(true))
        .unwrap();
    let item = scan
        .auto_hidden
        .iter()
        .find(|a| a.token == "אלוז")
        .expect("on the card");
    assert!(item.uncertain);
    assert!(scan
        .preview
        .iter()
        .all(|s| !(s.text.contains("אלוז") && s.mark.is_none())));
}

#[test]
fn an_ocr_misread_of_the_childs_name_is_kept_as_its_spelling_and_can_come_back() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    // Fabricated scan text: "אלון" read as "אלוז".
    let scan = "אלוז שיחק בחול עם הילדים.";
    let found = core.learn_ocr_misreads(&case, scan).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].token, "אלוז");
    assert_eq!(found[0].kind, AutoKind::SimilarSpelling);
    let child = |core: &mut Core| {
        core.case_detail(&case)
            .unwrap()
            .identities
            .into_iter()
            .find(|i| i.value == "אלון")
            .expect("child")
    };
    let tag = child(&mut core).tag;
    assert_eq!(found[0].tag, tag);
    assert!(child(&mut core).aliases.contains(&"אלוז".to_owned()));
    // Typed text later in the case hides it too, under the child's tag.
    let out = core.preview_filter(&case, "אחר כך אלוז נרגע.").unwrap();
    assert!(
        !out.tagged.contains("אלוז") && out.tagged.contains(&tag),
        "{}",
        out.tagged
    );

    // "להחזיר" takes only that spelling off; the name itself stays hidden.
    core.restore_auto_hidden(&case, "אלוז", &tag).unwrap();
    assert!(child(&mut core).aliases.is_empty());
    let out = core.preview_filter(&case, "אלוז ואלון שיחקו.").unwrap();
    assert!(
        out.tagged.contains("אלוז") && !out.tagged.contains("אלון"),
        "{}",
        out.tagged
    );
}

#[test]
fn manual_edit_with_new_name_is_hidden_before_it_goes_out() {
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

    // Kept with the case when the edit is saved, so the stored draft holds only its tag.
    let kept = core
        .case_detail(&case)
        .unwrap()
        .identities
        .into_iter()
        .find(|i| i.value == "זהבה")
        .expect("kept");
    assert_eq!(kept.role, Role::Relative);
    let next = core
        .prepare_section(&case, "kindergarten", "שפר/י את הניסוח")
        .unwrap();
    assert!(next.approval_id.is_some(), "{:?}", next.blocked);
    let out = outgoing(&next);
    assert!(!out.contains("זהבה"), "{out}");
    assert!(out.contains(&format!("הסבתא {}", kept.tag)), "{out}");
}

#[test]
fn earlier_wording_of_a_paragraph_comes_back_with_names() {
    let (_dir, mut core, case) = setup(None);
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let para = |core: &mut Core| {
        core.case_detail(&case)
            .unwrap()
            .sections
            .into_iter()
            .find(|s| s.key == "kindergarten")
            .unwrap()
            .paragraphs
            .remove(0)
    };
    let first = para(&mut core);
    assert!(!first.has_versions);
    core.edit_paragraph(&case, &first.id, "אלון רגיש לרעש בגן.")
        .unwrap();
    let edited = para(&mut core);
    assert!(edited.has_versions);
    assert!(!edited.by_ai);

    let versions = core.paragraph_versions(&case, &first.id).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].text, first.text);
    assert!(versions[0].by_ai);

    core.restore_paragraph_version(&case, &first.id, &versions[0].id)
        .unwrap();
    let back = para(&mut core);
    assert_eq!(back.text, first.text);
    assert_eq!(back.status, DraftStatus::Approved);
    assert!(back.by_ai);
    // Her edit is now an earlier wording, names shown as she wrote them.
    let after = core.paragraph_versions(&case, &first.id).unwrap();
    assert_eq!(after[0].text, "אלון רגיש לרעש בגן.");
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

/// The summary in demo mode is written from the approved sections, not answered with "no
/// sources yet"; with nothing approved it says so before anything is prepared.
#[test]
fn the_summary_is_written_from_the_approved_sections_in_demo_mode() {
    let (_dir, mut core, case) = setup(None);
    let err = core
        .prepare_section(&case, "summary", "טיוטה לסיכום")
        .unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");

    core.add_own_paragraph(&case, "kindergarten", "אלון מגיב בעוצמה למעברים.")
        .unwrap();
    let p = core
        .prepare_section(&case, "summary", "טיוטה לסיכום")
        .unwrap();
    let r = core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(r.demo);
    assert_eq!(r.paragraphs.len(), 1, "{}", r.reply);
    assert!(r.paragraphs[0].text.contains("מעברים"));
    assert!(
        r.paragraphs[0].warnings.is_empty(),
        "{:?}",
        r.paragraphs[0].warnings
    );
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
    // A restart of the program does not shorten the wait (D-122).
    let mut fresh = Core::for_tests(dir.path(), None);
    assert!(matches!(fresh.unlock(PASSWORD), Err(CoreError::Backoff(_))));
    fresh.not_before = None;
    assert!(fresh.unlock(PASSWORD).unwrap().unlocked);
}

/// D-122: five wrong passwords, then the program closed and opened again: the wait is still
/// there, as long as it was (32 s), and never more than a minute.
#[test]
fn the_wait_after_wrong_passwords_survives_a_restart() {
    let (dir, mut core, _case) = setup(None);
    core.lock();
    for _ in 0..5 {
        core.not_before = None;
        assert!(core.unlock("סיסמה שגויה לגמרי כאן").is_err());
    }
    drop(core);
    let mut again = Core::for_tests(dir.path(), None);
    let Err(CoreError::Backoff(s)) = again.unlock(PASSWORD) else {
        panic!("a restart cleared the wait");
    };
    assert!((30..=33).contains(&s), "{s}s");
    // The sixth try in the new run waits as the sixth: the count went with the file.
    again.not_before = None;
    assert!(again.unlock("סיסמה שגויה לגמרי כאן").is_err());
    let Err(CoreError::Backoff(s)) = again.unlock(PASSWORD) else {
        panic!("no wait after the sixth try");
    };
    assert!((59..=61).contains(&s), "{s}s");
}

/// A wait far in the future (a clock moved back, a file from elsewhere) is held to a minute,
/// and a file the program did not write is the longest wait, not none (fail-closed, D-122).
#[test]
fn a_kept_wait_is_bounded_and_a_strange_file_waits_the_longest() {
    let (dir, mut core, _case) = setup(None);
    core.lock();
    drop(core);
    let file = dir.path().join("unlock.failures");
    let far = crate::dates::unix_now() + 10 * 365 * 24 * 3600;
    for (text, wait) in [
        (format!("3 {far}"), true),
        ("שלום".to_owned(), true),
        ("3 4 5".to_owned(), true),
        (String::new(), true),
        // The program before D-122 wrote the count alone: no wait to keep.
        ("3".to_owned(), false),
        (format!("3 {}", crate::dates::unix_now() - 5), false),
    ] {
        std::fs::write(&file, &text).unwrap();
        let mut run = Core::for_tests(dir.path(), None);
        match run.unlock(PASSWORD) {
            Err(CoreError::Backoff(s)) => assert!(wait && s <= 61, "{text:?}: {s}s"),
            Ok(status) => {
                assert!(!wait && status.unlocked, "{text:?}");
                run.lock();
            }
            Err(e) => panic!("{text:?}: {e:?}"),
        }
    }
}

#[test]
fn only_allowed_models() {
    let (_dir, mut core, _case) = setup(None);
    assert!(core.set_model("claude-sonnet-5").is_ok());
    assert!(core.set_model("gpt-5-mini").is_ok());
    assert!(core.set_model("gemini-2-5-flash").is_ok());
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

/// "Write the empty sections" writes only those: a section with a draft waiting or approved
/// paragraphs is not drafted again beside them.
#[test]
fn full_draft_leaves_written_sections_alone() {
    let (_dir, mut core, case) = setup(None);
    let all: Vec<String> = core
        .prepare_full_draft(&case)
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert!(all.len() >= 2, "{all:?}");
    let (approved, proposed) = (&all[0], &all[1]);
    core.add_own_paragraph(&case, approved, "פסקה שאושרה.")
        .unwrap();
    let p = core.prepare_section(&case, proposed, "טיוטה").unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();

    let again: Vec<String> = core
        .prepare_full_draft(&case)
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert!(
        !again.contains(approved) && !again.contains(proposed),
        "{again:?}"
    );
    assert_eq!(again.len(), all.len() - 2);
}

#[test]
fn ambiguous_word_is_hidden_and_restored_once_per_case() {
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
    let id = p
        .approval_id
        .clone()
        .unwrap_or_else(|| panic!("{:?}", p.blocked));
    assert!(p
        .auto_hidden
        .iter()
        .any(|a| a.kind == dv_privacy::AutoKind::DeclaredInWord && a.token == "שאלון"));
    core.send_section(&id).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(sent[0].contains("ש[ילד] נרגע") && !sent[0].contains("אלון"));

    // Another case with the same child name, where it is the ordinary word: "להחזיר" there
    // keeps "שאלון" as written in that case only.
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
    let item = p
        .auto_hidden
        .iter()
        .find(|a| a.token == "שאלון")
        .expect("hidden")
        .clone();
    core.restore_auto_hidden(&other, &item.token, &item.tag)
        .unwrap();
    let p = core
        .prepare_section(&other, "kindergarten", "טיוטה")
        .unwrap();
    assert!(p.approval_id.is_some(), "{:?}", p.blocked);
    assert!(outgoing(&p).contains("שאלון ההורים"), "{}", outgoing(&p));
    // The first case still hides it.
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    assert!(!outgoing(&p).contains("שאלון נרגע"));
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
    let out = core.begin_send(&id, SendKind::Section).unwrap();
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
    let failed_at = |core: &mut Core| {
        core.list_cases()
            .unwrap()
            .into_iter()
            .find(|c| c.id == case)
            .unwrap()
            .failed_at
    };
    assert!(
        failed_at(&mut core).is_some(),
        "the case list shows it got stuck"
    );

    let r = core.send_section(&id).unwrap();
    assert!(!r.paragraphs.is_empty());
    assert_eq!(
        failed_at(&mut core),
        None,
        "a send that works clears the mark"
    );
}

#[test]
fn erasing_a_case_forgets_its_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    let case = core
        .create_case(
            CaseMeta {
                code: "TEST-0005".into(),
                ..CaseMeta::default()
            },
            vec![],
        )
        .unwrap();
    core.note_send_outcome(&case, false);
    assert!(core.list_cases().unwrap()[0].failed_at.is_some());
    core.delete_case(&case).unwrap();
    core.purge_case(&case, PASSWORD).unwrap();
    assert!(core.failed_sends().unwrap().is_empty());
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
        source: IdentitySource::Manual,
        reason: String::new(),
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
fn a_request_goes_without_a_window_by_default_and_she_may_ask_to_see_each_one() {
    let (_dir, mut core, _case) = setup(None);
    let s = core.status();
    assert!(
        !s.review_always,
        "off by default (D-062, approved by Einat)"
    );
    assert_eq!(s.practitioner, vec!["ד\"ר רותם אלמוג".to_owned()]);
    core.set_review_always(true).unwrap();
    assert!(core.status().review_always);
    core.set_review_always(false).unwrap();
    assert!(!core.status().review_always);
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
    for inst in dv_domain::instruments() {
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
            outcome.auto_hidden.is_empty(),
            "{}: {:?}",
            inst.key,
            outcome.auto_hidden
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

fn child_named(name: &str) -> dv_domain::Identity {
    dv_domain::Identity {
        id: format!("i-{name}"),
        case_id: "c".into(),
        role: Role::Child,
        tag: "[ילד]".into(),
        value: name.into(),
        aliases: vec![],
        source: dv_domain::IdentitySource::Manual,
        reason: String::new(),
    }
}

/// What `found` finds with each lexicon name as the child's name, per name. The names go in a
/// hundred at a time: every declared name is matched on its own, so a hundred names that find
/// nothing clear each of them, and only a hundred that finds something is tried name by name.
/// One by one, the 2,300 names took over ten minutes in a debug build.
fn found_for_every_child_name(
    found: impl Fn(&[dv_domain::Identity]) -> Option<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in lexicon_names().chunks(100) {
        let together: Vec<_> = chunk.iter().map(|n| child_named(n)).collect();
        let Some(all) = found(&together) else {
            continue;
        };
        let before = out.len();
        for name in chunk {
            if let Some(one) = found(&[child_named(name)]) {
                out.push(format!("{name}: {one}"));
            }
        }
        // A case has several names, so what they find only together counts too.
        if out.len() == before {
            out.push(format!("{chunk:?}: {all}"));
        }
    }
    out
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
        dv_ai::prompts::VERIFY_RULES,
        dv_ai::prompts::LETTER_PARENTS,
        dv_ai::prompts::LETTER_SCHOOL,
        dv_ai::prompts::LETTER_TITLE_PARENTS,
        dv_ai::prompts::LETTER_TITLE_SCHOOL,
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
            guide_tagged: String::new(),
            points_only: false,
            sources_in_order: false,
        };
        let nonce = dv_ai::nonce_from(&[7; 16]);
        // Like the real gate: every string in the body, not the JSON numbers ("max_tokens").
        body.push(dv_ai::build_section_request(&dv_ai::ModelConfig::default(), &input, &nonce).0);
        // A summary: written from the approved sections only, with its own opening line.
        // With a guide (round 29): its heading and numbering are the builder's words. The
        // lines are hers (the institute's until she edits them) and go through the filter like
        // her request, so they are checked with the case text below.
        let institute = ReportStructure::for_template(Some(dv_domain::INSTITUTE_TEMPLATE)).unwrap();
        let derived = dv_ai::SectionInput {
            section_key: "summary".into(),
            sources: vec![],
            guide_tagged: "[ילד]\n[ילד]".into(),
            points_only: false,
            sources_in_order: false,
            ..input
        };
        // What the institute's sections are about goes out in its sorting request (D-066).
        for s in institute.sections() {
            body.push(Value::String(s.about.clone()));
        }
        body.push(dv_ai::build_section_request(&dv_ai::ModelConfig::default(), &derived, &nonce).0);
        // A section she writes herself, worded from her points (round 30): its own opening.
        let points = dv_ai::SectionInput {
            points_only: true,
            ..derived
        };
        body.push(dv_ai::build_section_request(&dv_ai::ModelConfig::default(), &points, &nonce).0);
        // Previous assessments, in the order fixed in code (round 31).
        body.push(Value::String(dv_ai::ORDER_LINE.to_owned()));
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
    // The check against the sources: its frames and schema.
    body.push(
        dv_ai::build_verify_request(
            &dv_ai::ModelConfig::default(),
            &dv_ai::VerifyInput {
                section_title: String::new(),
                sources: vec![dv_ai::TaggedInput {
                    input_id: "in".into(),
                    kind_label: InputKind::Intake.label_he().to_owned(),
                    title_tagged: String::new(),
                    content_tagged: String::new(),
                }],
                paragraphs: vec![String::new()],
            },
            &nonce,
        )
        .0,
    );
    // The style profile (D-043): both requests' rules, frames and schemas, and the words the
    // profile is framed with in every drafting request.
    for rules in dv_ai::style::FIXED_TEXTS
        .into_iter()
        .chain(dv_ai::INTERPRETATION_FIXED_TEXTS)
    {
        body.push(Value::String(rules.to_owned()));
    }
    // The interpretation of scores (round 33): every subtest's label and cases, as the builder
    // writes them, and the words an approved interpretation is framed with.
    let groups: Vec<dv_ai::InterpretationGroup> = dv_domain::instruments()
        .into_iter()
        .flat_map(|i| {
            i.measures
                .into_iter()
                .map(move |m| dv_ai::InterpretationGroup {
                    cases: dv_domain::interpretation_cases(&i.key, &m.key),
                    label: format!("{} · {}", i.name, m.name_he),
                    instrument: i.key.clone(),
                    measure: m.key,
                    sentences: vec![(1, String::new())],
                })
        })
        .collect();
    body.push(dv_ai::build_interpretation_request(
        &dv_ai::ModelConfig::default(),
        &groups,
        &nonce,
    ));
    body.push(Value::String(
        dv_ai::render_interpretations(
            &[dv_ai::Interpretation {
                id: "i".into(),
                instrument: "x".into(),
                measure: "y".into(),
                case: "-".into(),
                text: "-".into(),
                support: 1,
                approved: true,
                enabled: true,
                edited: false,
            }],
            &|_, _| Some("-".to_owned()),
        )
        .unwrap_or_default(),
    ));
    // The order of a section learned from her reports (K3) goes out as its guide: every
    // topic, at every length.
    for (section, topics) in dv_style::skeleton::TOPICS {
        let skeleton = dv_style::Skeleton {
            section: (*section).to_owned(),
            band: None,
            reports: 3,
            topics: topics
                .iter()
                .zip([10, 40, 100, 200].into_iter().cycle())
                .map(|((label, _), words)| dv_style::SkeletonTopic {
                    label: (*label).to_owned(),
                    share: 100,
                    words,
                })
                .collect(),
        };
        body.push(Value::String(skeleton.guide()));
    }
    let keys: Vec<String> = structure.sections().map(|s| s.key.clone()).collect();
    let excerpts: Vec<dv_ai::StyleExcerpt> = keys
        .iter()
        .map(|k| dv_ai::StyleExcerpt {
            section: k.clone(),
            text: String::new(),
        })
        .collect();
    // Every pattern of scores there can be (D-100): their names go out as fixed text too.
    let patterns: Vec<dv_domain::ScorePattern> = dv_domain::pattern_keys()
        .into_iter()
        .map(|key| dv_domain::ScorePattern {
            label: dv_domain::pattern_label(&key).unwrap_or_default(),
            key,
        })
        .collect();
    body.push(dv_ai::build_style_analysis_request(
        &dv_ai::ModelConfig::default(),
        &keys,
        &excerpts,
        &patterns,
        &nonce,
    ));
    body.push(dv_ai::build_style_synthesis_request(
        &dv_ai::ModelConfig::default(),
        &keys,
        &[vec![]],
        &nonce,
    ));
    let every_kind = [
        dv_ai::StyleKind::Rule,
        dv_ai::StyleKind::Phrase,
        dv_ai::StyleKind::Avoid,
        dv_ai::StyleKind::Template,
        dv_ai::StyleKind::Example,
        dv_ai::StyleKind::Argument,
        dv_ai::StyleKind::Integration,
        dv_ai::StyleKind::Criterion,
    ];
    let profile = dv_ai::StyleProfile {
        reports: 1,
        items: [None, Some("cognitive".to_owned())]
            .into_iter()
            .flat_map(|section| {
                every_kind.into_iter().map(move |kind| dv_ai::StyleItem {
                    id: "i".into(),
                    section: section.clone(),
                    kind,
                    text: "-".into(),
                    enabled: true,
                    origin: dv_ai::StyleOrigin::Reports,
                    support: 1,
                    pattern: None,
                })
            })
            .collect(),
    };
    body.push(Value::String(
        dv_ai::render_for_section(&profile, "cognitive", &|_| true, &[]).unwrap(),
    ));
    let body = Value::Array(body);
    let tags: HashSet<String> = HashSet::from(["[ילד]".to_owned()]);
    let collisions = found_for_every_child_name(|identities| {
        let ctx = PrivacyContext {
            case_id: "c",
            identities,
            practitioner: &[],
            allowlisted: &|_| false,
            confirmed_names: &|_| false,
            past_names: &|_| false,
            today: (2026, 9, 28),
            masks: None,
        };
        let req = GateRequest {
            body: &body,
            ctx: &ctx,
            case_tags: &tags,
            unresolved_suspects: 0,
            canaries: &[],
            max_bytes: MAX_REQUEST_BYTES,
        };
        clear(&req).err().map(|blocked| {
            let found: Vec<_> = blocked
                .reasons
                .iter()
                .filter_map(|r| r.detail.clone())
                .collect();
            format!("{found:?}")
        })
    });
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
        .chain(
            // The institute's titles and guide lines (D-066, round 29) are case text too.
            ReportStructure::for_template(Some(dv_domain::INSTITUTE_TEMPLATE))
                .unwrap()
                .parts
                .into_iter()
                .flat_map(|p| {
                    std::iter::once(p.title)
                        .chain(p.sections.into_iter().flat_map(|s| [s.title, s.guide]))
                }),
        )
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
    // A word that holds the name ("שאלון" for a child "אלון") or is spelled close to it may
    // be hidden, but only as an item on the card: "להחזיר" there must bring the text back
    // exactly. Nothing may change without being listed.
    let rewritten = found_for_every_child_name(|identities| {
        let filter_with = |text: &str, kept: &[String]| {
            let allow = |t: &str| kept.iter().any(|k| normalize(k) == t);
            let ctx = PrivacyContext {
                case_id: "c",
                identities,
                practitioner: &[],
                allowlisted: &allow,
                confirmed_names: &|_| false,
                past_names: &|_| false,
                today: (2026, 9, 28),
                masks: None,
            };
            dv_privacy::filter(text, &ctx).unwrap()
        };
        let changed: Vec<String> = texts
            .iter()
            .filter_map(|text| {
                let first = filter_with(text, &[]);
                if first.tagged == *text {
                    return None;
                }
                let listed: Vec<String> =
                    first.auto_hidden.iter().map(|a| a.token.clone()).collect();
                let restored = filter_with(text, &listed);
                (restored.tagged != *text).then(|| format!("{:?} {listed:?}", restored.hidden))
            })
            .collect();
        (!changed.is_empty()).then(|| format!("{changed:?}"))
    });
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
