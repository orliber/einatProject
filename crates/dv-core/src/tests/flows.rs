//! Whole flows through `Core` around the send gate, the session and what is kept: an approval
//! sends exactly the bytes the gate cleared and only once, a failure or a lock never sends
//! twice or brings an approval back, retention and the recycle bin erase only what they
//! should, a backup restores the same cases, settings persist and refuse nonsense, and the
//! activity log never names anyone. Fabricated data only; nothing leaves the machine.

use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

fn sha256_hex(bytes: &[u8]) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Fails the first `fail` calls (offline), then answers like the demo, or with `answer`.
/// Every attempt is recorded, failed or not.
#[derive(Clone, Default)]
struct Scripted {
    attempts: Arc<Mutex<Vec<String>>>,
    fail: Arc<AtomicU32>,
    answer: Arc<Mutex<Option<Value>>>,
}

impl Scripted {
    fn failing(times: u32) -> Self {
        let s = Self::default();
        s.fail.store(times, Ordering::SeqCst);
        s
    }

    fn attempts(&self) -> Vec<String> {
        self.attempts.lock().unwrap().clone()
    }
}

impl Transport for Scripted {
    fn send(&self, payload: &ClearedPayload) -> Result<Value, EgressError> {
        let body = String::from_utf8_lossy(payload.body()).into_owned();
        self.attempts.lock().unwrap().push(body.clone());
        if self
            .fail
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(EgressError::Offline);
        }
        if let Some(a) = self.answer.lock().unwrap().clone() {
            return Ok(a);
        }
        Ok(dv_ai::demo::respond(&serde_json::from_str(&body).unwrap()))
    }
}

fn scripted_setup(t: &Scripted) -> (tempfile::TempDir, Core, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), Some(Box::new(t.clone())));
    core.create_vault(PASSWORD).unwrap();
    let case = core
        .create_case(
            CaseMeta {
                code: "TEST-0700".into(),
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
        "שיחה עם הגננת",
        "אלון מתקשה במעברים בין פעילויות ונרגע כשמכינים אותו מראש.",
    )
    .unwrap();
    (dir, core, case)
}

fn approval(core: &mut Core, case: &str) -> String {
    let p = core
        .prepare_section(case, "kindergarten", "תנסח פסקה")
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    p.approval_id.expect("a cleared request has an approval")
}

fn sends_logged(core: &mut Core) -> usize {
    core.vault_ref()
        .unwrap()
        .audit_entries(10_000)
        .unwrap()
        .iter()
        .filter(|e| e.event == "send")
        .count()
}

#[test]
fn an_approval_sends_exactly_the_cleared_bytes_once_and_is_kept_under_their_hash() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let id = approval(&mut core, &case);
    core.send_section(&id).unwrap();

    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    // The approval id is the digest of the very bytes that left: nothing was rebuilt after
    // the review screen.
    assert_eq!(sha256_hex(sent[0].as_bytes()), id);
    let kept = core.vault_ref().unwrap().transmissions(&case).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].payload_sha256, id);
    assert_eq!(kept[0].payload_tagged, sent[0]);
    assert_eq!(kept[0].section_key, "kindergarten");

    // Used up: the same approval twice sends nothing more and stores nothing more.
    assert!(matches!(
        core.send_section(&id),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        core.begin_send(&id, SendKind::Section),
        Err(CoreError::NotFound(_))
    ));
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
    assert_eq!(
        core.vault_ref()
            .unwrap()
            .transmissions(&case)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(core.chat(&case, "kindergarten").unwrap().len(), 2);
    assert_eq!(sends_logged(&mut core), 1);
}

#[test]
fn nothing_goes_out_without_an_approval_the_gate_issued() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // Prepared but never approved: the payload waits in memory, and nothing has been sent.
    let id = approval(&mut core, &case);
    assert!(fake.sent.lock().unwrap().is_empty());

    // Ids the gate never issued: made up, the digest of a body nobody cleared, an id with a
    // character changed, and the empty string.
    let forged_body = json!({ "model": dv_ai::DEFAULT_MODEL, "messages": [] }).to_string();
    let mut flipped = id.clone().into_bytes();
    flipped[0] = if flipped[0] == b'0' { b'1' } else { b'0' };
    for bad in [
        "not-an-approval".to_owned(),
        sha256_hex(forged_body.as_bytes()),
        String::from_utf8(flipped).unwrap(),
        String::new(),
    ] {
        assert!(
            matches!(core.send_section(&bad), Err(CoreError::NotFound(_))),
            "{bad}"
        );
        assert!(matches!(
            core.send_consult(&bad),
            Err(CoreError::NotFound(_))
        ));
        assert!(matches!(core.send_sort(&bad), Err(CoreError::NotFound(_))));
    }
    assert!(fake.sent.lock().unwrap().is_empty());
    assert!(core
        .vault_ref()
        .unwrap()
        .transmissions(&case)
        .unwrap()
        .is_empty());
    assert_eq!(sends_logged(&mut core), 0);
    // The real approval is still there.
    core.send_section(&id).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
}

#[test]
fn an_edit_after_the_review_never_rides_on_the_earlier_approval() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let first = approval(&mut core, &case);
    let reviewed = core.vault_ref().unwrap().inputs(&case).unwrap()[0].clone();

    // She changes the material after seeing the review screen.
    let added = "בנוסף, מטפלת התקשורת ממליצה על לוח מעברים מצויר.";
    core.update_input(
        &case,
        &reviewed.id,
        &reviewed.title,
        &format!("{} {added}", reviewed.content),
    )
    .unwrap();

    // The earlier approval does not go (she may have taken a sentence out), and says why.
    let Err(CoreError::Refused(why)) = core.send_section(&first) else {
        panic!("a request prepared with the old text was sent");
    };
    assert!(why.contains("החומר השתנה"), "{why}");
    assert!(fake.sent.lock().unwrap().is_empty());

    // The new text needs a new review, with a new approval of its own.
    let second = approval(&mut core, &case);
    assert_ne!(second, first);
    core.send_section(&second).unwrap();
    let sent = fake.sent.lock().unwrap().last().cloned().unwrap();
    assert_eq!(sha256_hex(sent.as_bytes()), second);
    assert!(sent.contains("לוח מעברים"));
    assert!(!sent.contains("אלון"));
}

#[test]
fn every_failed_attempt_drafts_nothing_and_the_retry_is_drafted_once() {
    let t = Scripted::failing(2);
    let (_dir, mut core, case) = scripted_setup(&t);
    let id = approval(&mut core, &case);

    for attempt in 1..=2 {
        let err = core.send_section(&id).unwrap_err();
        assert!(
            matches!(err, CoreError::Egress(EgressError::Offline)),
            "{err:?}"
        );
        assert_eq!(t.attempts().len(), attempt);
        // A connection lost on the way may come after the bytes reached the service: the
        // attempt is in the history of what left (the review of stage 2), and no draft appears.
        assert_eq!(
            core.vault_ref()
                .unwrap()
                .transmissions(&case)
                .unwrap()
                .len(),
            attempt
        );
        assert!(core.chat(&case, "kindergarten").unwrap().is_empty());
        assert_eq!(sends_logged(&mut core), attempt);
        let detail = core.case_detail(&case).unwrap();
        let section = detail.sections.iter().find(|s| s.key == "kindergarten");
        assert!(section.is_none_or(|s| s.paragraphs.is_empty()));
    }

    let r = core.send_section(&id).unwrap();
    assert!(!r.paragraphs.is_empty());
    // Every attempt carried the same approved bytes.
    let attempts = t.attempts();
    assert_eq!(attempts.len(), 3);
    assert!(attempts.iter().all(|a| sha256_hex(a.as_bytes()) == id));
    assert_eq!(
        core.vault_ref()
            .unwrap()
            .transmissions(&case)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(core.chat(&case, "kindergarten").unwrap().len(), 2);
    assert_eq!(sends_logged(&mut core), 3);
    let drafted = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs
        .len();
    assert_eq!(
        drafted,
        r.paragraphs.len(),
        "one set of drafts, not one per attempt"
    );

    // And then it is used up.
    assert!(matches!(
        core.send_section(&id),
        Err(CoreError::NotFound(_))
    ));
    assert_eq!(t.attempts().len(), 3);
}

#[test]
fn an_answer_that_cannot_be_read_still_counts_as_sent_and_is_not_sent_again() {
    let t = Scripted::default();
    *t.answer.lock().unwrap() = Some(json!({
        "stop_reason": "end_turn",
        "content": [{ "type": "text", "text": "not json at all" }]
    }));
    let (_dir, mut core, case) = scripted_setup(&t);
    let id = approval(&mut core, &case);

    let err = core.send_section(&id).unwrap_err();
    assert!(matches!(err, CoreError::Ai(_)), "{err:?}");
    // The request left the machine: it is on record, even though the reply was useless.
    assert_eq!(
        core.vault_ref()
            .unwrap()
            .transmissions(&case)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(sends_logged(&mut core), 1);
    // A retry would send the same thing again behind her back: the approval is gone.
    assert!(matches!(
        core.send_section(&id),
        Err(CoreError::NotFound(_))
    ));
    assert_eq!(t.attempts().len(), 1);
    assert!(core.chat(&case, "kindergarten").unwrap().is_empty());
}

#[test]
fn a_lock_wipes_the_approvals_waiting_for_review() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let section = approval(&mut core, &case);
    let consult = core
        .prepare_consult(Some(&case), None, "איך מנסחים המלצה על לוח מעברים?")
        .unwrap()
        .approval_id
        .unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(matches!(
        core.send_section(&section),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        core.send_consult(&consult),
        Err(CoreError::NotFound(_))
    ));
    assert!(fake.sent.lock().unwrap().is_empty());
}

#[test]
fn every_command_refuses_while_the_vault_is_locked() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let input = core.vault_ref().unwrap().inputs(&case).unwrap()[0]
        .id
        .clone();
    let drive = tempfile::tempdir().unwrap();
    core.lock();

    type Command<'a> = (
        &'a str,
        Box<dyn Fn(&mut Core) -> Result<(), CoreError> + 'a>,
    );
    let c = case.as_str();
    let i = input.as_str();
    let commands: Vec<Command<'_>> = vec![
        ("list_cases", Box::new(|k| k.list_cases().map(drop))),
        ("case_detail", Box::new(move |k| k.case_detail(c).map(drop))),
        (
            "create_case",
            Box::new(|k| k.create_case(CaseMeta::default(), vec![]).map(drop)),
        ),
        (
            "update_case",
            Box::new(move |k| k.update_case(c, CaseMeta::default())),
        ),
        (
            "set_identities",
            Box::new(move |k| k.set_identities(c, vec![]).map(drop)),
        ),
        (
            "add_input",
            Box::new(move |k| k.add_input(c, InputKind::Intake, "t", "x").map(drop)),
        ),
        (
            "update_input",
            Box::new(move |k| k.update_input(c, i, "t", "x")),
        ),
        ("delete_input", Box::new(move |k| k.delete_input(c, i))),
        (
            "prepare_section",
            Box::new(move |k| k.prepare_section(c, "kindergarten", "x").map(drop)),
        ),
        (
            "prepare_full_draft",
            Box::new(move |k| k.prepare_full_draft(c).map(drop)),
        ),
        (
            "prepare_consult",
            Box::new(move |k| k.prepare_consult(Some(c), None, "x").map(drop)),
        ),
        (
            "prepare_sort",
            Box::new(move |k| k.prepare_sort(c).map(drop)),
        ),
        (
            "prepare_verify",
            Box::new(move |k| k.prepare_verify(c, "kindergarten").map(drop)),
        ),
        (
            "chat",
            Box::new(move |k| k.chat(c, "kindergarten").map(drop)),
        ),
        (
            "approve_section",
            Box::new(move |k| k.approve_section(c, "kindergarten").map(drop)),
        ),
        (
            "add_own_paragraph",
            Box::new(move |k| k.add_own_paragraph(c, "kindergarten", "x").map(drop)),
        ),
        (
            "check_export",
            Box::new(move |k| k.check_export(c).map(drop)),
        ),
        (
            "export_report",
            Box::new(move |k| k.export_report(c, None).map(drop)),
        ),
        ("delete_case", Box::new(move |k| k.delete_case(c))),
        ("restore_case", Box::new(move |k| k.restore_case(c))),
        ("list_trash", Box::new(|k| k.list_trash().map(drop))),
        ("folders", Box::new(|k| k.folders().map(drop))),
        (
            "create_folder",
            Box::new(|k| k.create_folder(None, "תיקייה").map(drop)),
        ),
        ("consultations", Box::new(|k| k.consultations().map(drop))),
        (
            "sent_history",
            Box::new(|k| k.sent_history(None, None, "", None).map(drop)),
        ),
        ("sent_count", Box::new(|k| k.sent_count(0).map(drop))),
        (
            "sent_text",
            Box::new(move |k| k.sent_text(c, "x").map(drop)),
        ),
        ("activity", Box::new(|k| k.activity(None).map(drop))),
        ("readiness", Box::new(|k| k.readiness().map(drop))),
        (
            "confirm_readiness",
            Box::new(|k| k.confirm_readiness("zdr", true).map(drop)),
        ),
        ("retention_due", Box::new(|k| k.retention_due().map(drop))),
        (
            "keep_case_longer",
            Box::new(move |k| k.keep_case_longer(c, 1)),
        ),
        ("backup_status", Box::new(|k| k.backup_status().map(drop))),
        (
            "write_backup",
            Box::new(|k| k.write_backup(&drive.path().join("b")).map(drop)),
        ),
        ("usage_summary", Box::new(|k| k.usage_summary().map(drop))),
        ("set_monthly_cap", Box::new(|k| k.set_monthly_cap(Some(5)))),
        ("set_model", Box::new(|k| k.set_model(dv_ai::DEFAULT_MODEL))),
        ("set_speed", Box::new(|k| k.set_speed("fast"))),
        ("set_lock_minutes", Box::new(|k| k.set_lock_minutes(5))),
        (
            "set_practitioner",
            Box::new(|k| k.set_practitioner(vec!["x".into()])),
        ),
        (
            "set_api_key",
            Box::new(|k| k.set_api_key("anthropic", "sk-ant-test-not-real", false)),
        ),
        (
            "set_screen_protection",
            Box::new(|k| k.set_screen_protection(false)),
        ),
        ("set_verify_stage", Box::new(|k| k.set_verify_stage(true))),
        ("take_unsaved", Box::new(|k| k.take_unsaved().map(drop))),
        (
            "confirm_recovery_key",
            Box::new(|k| k.confirm_recovery_key("x").map(drop)),
        ),
        (
            "change_password",
            Box::new(|k| k.change_password(PASSWORD, false, "סיסמה חדשה וארוכה מאוד")),
        ),
        (
            "report_settings",
            Box::new(|k| k.report_settings().map(drop)),
        ),
    ];
    for (name, command) in &commands {
        let r = command(&mut core);
        assert!(matches!(r, Err(CoreError::Locked)), "{name}: {r:?}");
        assert!(!core.is_unlocked(), "{name} opened the vault");
    }
    assert!(fake.sent.lock().unwrap().is_empty());

    // Nothing was written while locked: the case is as it was.
    core.unlock(PASSWORD).unwrap();
    let detail = core.case_detail(&case).unwrap();
    assert_eq!(detail.inputs.len(), 2);
    assert_eq!(detail.identities.len(), 2);
    assert_eq!(core.list_cases().unwrap().len(), 1);
    assert!(core.folders().unwrap().is_empty());
}

#[test]
fn an_idle_lock_reached_inside_a_command_refuses_it_and_locks() {
    let (_dir, mut core, case) = setup(None);
    core.set_lock_minutes(1).unwrap();
    core.last_activity = Instant::now() - Duration::from_secs(61);
    // The first command after the idle time is refused rather than run on a stale session.
    assert!(matches!(
        core.add_input(&case, InputKind::Intake, "t", "טקסט"),
        Err(CoreError::Locked)
    ));
    assert!(!core.is_unlocked());
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.case_detail(&case).unwrap().inputs.len(), 2);
}

// ------------------------------------------------------------ retention and the bin

fn plain_case(core: &mut Core, code: &str, retention_until: Option<&str>) -> String {
    let id = core
        .create_case(
            CaseMeta {
                code: code.into(),
                retention_until: retention_until.map(str::to_owned),
                ..CaseMeta::default()
            },
            vec![],
        )
        .unwrap();
    core.add_input(
        &id,
        InputKind::Intake,
        "אינטייק",
        "ההורים מתארים שינה לא סדירה.",
    )
    .unwrap();
    id
}

#[test]
fn retention_points_out_only_cases_past_their_date_and_erases_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    let past = plain_case(&mut core, "TEST-0711", Some("2020-01-01"));
    let older = plain_case(&mut core, "TEST-0712", Some("2019-06-30"));
    let future = plain_case(&mut core, "TEST-0713", Some("2099-01-01"));
    let by_default = plain_case(&mut core, "TEST-0714", None);
    let binned = plain_case(&mut core, "TEST-0715", Some("2020-01-01"));
    core.delete_case(&binned).unwrap();
    // A date that is not a date is refused, not taken as "today" or "never".
    assert!(matches!(
        core.create_case(
            CaseMeta {
                code: "TEST-0716".into(),
                retention_until: Some("31/12/2020".into()),
                ..CaseMeta::default()
            },
            vec![],
        ),
        Err(CoreError::Refused(_))
    ));

    let due = core.retention_due().unwrap();
    let ids: Vec<&str> = due.iter().map(|d| d.case_id.as_str()).collect();
    assert_eq!(ids, vec![older.as_str(), past.as_str()], "oldest first");
    assert!(due.iter().all(|d| !d.by_default));
    assert!(!ids.contains(&future.as_str()) && !ids.contains(&by_default.as_str()));

    // Pointing out is all it does: every case and every material is still there.
    assert_eq!(core.list_cases().unwrap().len(), 4);
    assert_eq!(core.list_trash().unwrap().len(), 1);
    for c in [&past, &older, &future, &by_default] {
        assert_eq!(core.case_detail(c).unwrap().inputs.len(), 1);
    }
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.list_cases().unwrap().len(), 4, "not at unlock either");

    // "Keep it longer" moves the date from today, within one to ten years.
    core.keep_case_longer(&past, 0).unwrap();
    core.keep_case_longer(&older, 200).unwrap();
    let (y, m, d) = today();
    let until = |core: &mut Core, c: &str| core.case_detail(c).unwrap().meta.retention_until;
    let in_years = |n: i32| dates::iso(dates::add_years((y, m, d), n));
    assert_eq!(until(&mut core, &past), Some(in_years(1)));
    assert_eq!(until(&mut core, &older), Some(in_years(10)));
    assert!(core.retention_due().unwrap().is_empty());
}

#[test]
fn the_bin_erases_only_what_waited_past_thirty_days_and_is_in_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    let stale = plain_case(&mut core, "TEST-0721", None);
    let fresh = plain_case(&mut core, "TEST-0722", None);
    // At work, with its retention date long past: retention never erases on its own.
    let kept = plain_case(&mut core, "TEST-0723", Some("2020-01-01"));
    core.delete_case(&stale).unwrap();
    core.delete_case(&fresh).unwrap();
    let edge = (dates::unix_now() - (TRASH_DAYS * dv_domain::dates::DAY_SECS)) + 3_600;
    core.vault_mut()
        .unwrap()
        .set_deleted_at_for_tests(&stale, 1)
        .unwrap();
    core.vault_mut()
        .unwrap()
        .set_deleted_at_for_tests(&fresh, edge)
        .unwrap();
    let ids = |core: &mut Core| -> Vec<String> {
        let mut ids: Vec<String> = core
            .list_trash()
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        ids.sort();
        ids
    };

    let mut both = vec![stale.clone(), fresh.clone()];
    both.sort();

    // No backup yet: the clock alone erases nothing, however long it waited.
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(ids(&mut core), both);
    // A backup written before the deletion does not hold the case as deleted either: a
    // backup "from the past" (its time set back) counts for nothing.
    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("b")).unwrap();
    core.vault_mut()
        .unwrap()
        .set_state("backup_last_at", "0")
        .unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(ids(&mut core), both);

    // A backup written since the deletion: what waited past thirty days goes.
    core.write_backup(&drive.path().join("b")).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(
        ids(&mut core),
        vec![fresh.clone()],
        "an hour short of thirty days stays"
    );
    assert!(matches!(
        core.case_detail(&stale),
        Err(CoreError::Vault(VaultError::NotFound))
    ));
    let at_work: Vec<String> = core
        .list_cases()
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(at_work, vec![kept.clone()]);
    assert_eq!(core.case_detail(&kept).unwrap().inputs.len(), 1);
    // The bin's case can still come back whole.
    core.restore_case(&fresh).unwrap();
    assert_eq!(core.case_detail(&fresh).unwrap().inputs.len(), 1);
}

// ------------------------------------------------------------ backup and restore

#[test]
fn a_backup_restores_the_same_cases_settings_and_bin() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // A case with drafts approved and an edit of her own, a second case in a folder, and
    // one in the bin long enough to be erased at a normal unlock.
    let id = approval(&mut core, &case);
    core.send_section(&id).unwrap();
    core.approve_section(&case, "kindergarten").unwrap();
    core.add_own_paragraph(
        &case,
        "kindergarten",
        "אלון נעזר בתמונות כדי לעבור בין פעילויות.",
    )
    .unwrap();
    let folder = core.create_folder(None, "אבחונים 2026").unwrap();
    let second = plain_case(&mut core, "TEST-0731", Some("2030-05-05"));
    core.move_case(&second, Some(&folder.id)).unwrap();
    let binned = plain_case(&mut core, "TEST-0732", None);
    core.delete_case(&binned).unwrap();
    core.vault_mut()
        .unwrap()
        .set_deleted_at_for_tests(&binned, 1)
        .unwrap();
    core.set_model("claude-sonnet-5").unwrap();
    core.set_speed("thorough").unwrap();
    core.set_lock_minutes(25).unwrap();
    core.set_monthly_cap(Some(40)).unwrap();

    let before_cases = core.list_cases().unwrap();
    let before: Vec<CaseDetail> = [&case, &second]
        .iter()
        .map(|c| core.case_detail(c).unwrap())
        .collect();
    let before_trash = core.list_trash().unwrap();
    let before_folders = core.folders().unwrap();
    let before_chat = core.chat(&case, "kindergarten").unwrap();
    let before_status = core.status();

    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b")).unwrap();
    let bytes = std::fs::read(&done.path).unwrap();

    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    other.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    other.restore_staged_backup(Some(PASSWORD), None).unwrap();

    assert_eq!(other.list_cases().unwrap(), before_cases);
    for d in &before {
        assert_eq!(&other.case_detail(&d.id).unwrap(), d);
    }
    assert_eq!(other.chat(&case, "kindergarten").unwrap(), before_chat);
    assert_eq!(other.folders().unwrap(), before_folders);
    // A restore is how a mistaken deletion is undone: the bin comes back as it was.
    assert_eq!(other.list_trash().unwrap(), before_trash);
    let after = other.status();
    assert_eq!(after.model, before_status.model);
    assert_eq!(after.speed, before_status.speed);
    assert_eq!(after.lock_minutes, 25);
    assert_eq!(after.practitioner, before_status.practitioner);
    assert_eq!(other.usage_summary().unwrap().cap_usd, Some(40));
    // The restored vault opens with the same recovery and the same password afterwards.
    other.lock();
    other.unlock(PASSWORD).unwrap();
    assert_eq!(other.list_cases().unwrap().len(), before_cases.len());
}

#[test]
fn a_failed_restore_leaves_no_vault_behind_and_keeps_the_file_for_another_try() {
    let (_dir, mut core, case) = setup(None);
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b")).unwrap();
    let mut bytes = std::fs::read(&done.path).unwrap();

    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    // Not a backup at all.
    assert!(matches!(
        other.stage_backup(b"PK\x03\x04 not a vault".to_vec(), "x.vaultbak".into()),
        Err(CoreError::Refused(_))
    ));
    // A wrong recovery kit: nothing is left in the folder, and the file stays chosen.
    other
        .stage_backup(bytes.clone(), "b.vaultbak".into())
        .unwrap();
    assert!(other
        .restore_staged_backup(None, Some("AAAA-BBBB"))
        .is_err());
    assert!(!Vault::exists(fresh.path()));
    other.not_before = None;
    // While it is restored, nothing else creates or opens a vault in that folder (a failed
    // restore removes what it wrote, and would take the other with it).
    let job = other.begin_restore(Some(PASSWORD), None).unwrap();
    assert!(matches!(
        other.create_vault("סיסמה אחרת וארוכה מאוד"),
        Err(CoreError::Needs(_, ErrorAction::Dismiss))
    ));
    assert!(matches!(
        other.begin_unlock(PASSWORD),
        Err(CoreError::Needs(_, ErrorAction::Dismiss))
    ));
    other.finish_restore(job.run()).unwrap();
    assert!(other.case_detail(&case).is_ok());

    // A damaged file (one byte flipped near the end) does not restore anything.
    let last = bytes.len() - 10;
    bytes[last] ^= 0x55;
    let broken = tempfile::tempdir().unwrap();
    let mut third = Core::for_tests(broken.path(), None);
    if third.stage_backup(bytes, "b.vaultbak".into()).is_ok() {
        assert!(third.restore_staged_backup(Some(PASSWORD), None).is_err());
    }
    assert!(!third.is_unlocked());
    assert!(!Vault::exists(broken.path()));
}

/// D-124: a restore cut short after the databases and before the header. A new vault is not
/// made over them (its header would sit next to her data and never open); restoring again is.
#[test]
fn a_new_vault_is_not_made_over_databases_left_without_a_header() {
    let (_dir, mut core, case) = setup(None);
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b")).unwrap();
    let bytes = std::fs::read(&done.path).unwrap();

    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    other
        .stage_backup(bytes.clone(), "b.vaultbak".into())
        .unwrap();
    other.restore_staged_backup(Some(PASSWORD), None).unwrap();
    other.lock();
    std::fs::remove_file(fresh.path().join("vault.header")).unwrap();
    let main = std::fs::read(fresh.path().join("main.db")).unwrap();

    let mut again = Core::for_tests(fresh.path(), None);
    assert!(!again.status().vault_exists);
    match again.create_vault("סיסמה אחרת וארוכה מאוד") {
        Err(CoreError::Refused(why)) => assert!(why.contains("לשחזר שוב מהגיבוי")),
        other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
    }
    assert!(
        !fresh.path().join("vault.header").exists(),
        "no header was written"
    );
    assert_eq!(std::fs::read(fresh.path().join("main.db")).unwrap(), main);
    again.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    again.restore_staged_backup(Some(PASSWORD), None).unwrap();
    assert!(again.case_detail(&case).is_ok());
}

// ------------------------------------------------------------ readiness

/// This version has no Google sign-in (not Windows, or built without the registration).
struct NoGoogle;

impl crate::google::ComputerKeys for NoGoogle {
    fn available(&self) -> bool {
        false
    }
    fn exists(&self, _: &Path) -> bool {
        false
    }
    fn load(&self, _: &Path) -> Result<dv_vault::crypto::Key32, VaultError> {
        Err(VaultError::NotFound)
    }
    fn create_next(&self, _: &Path) -> Result<dv_vault::crypto::Key32, VaultError> {
        Err(VaultError::NotFound)
    }
    fn load_next(&self, _: &Path) -> Result<dv_vault::crypto::Key32, VaultError> {
        Err(VaultError::NotFound)
    }
    fn promote_next(&self, _: &Path) -> Result<(), VaultError> {
        Err(VaultError::NotFound)
    }
    fn remove(&self, _: &Path) {}
}

#[test]
fn the_safety_net_and_readiness_agree_on_what_a_recent_backup_is() {
    let (_dir, mut core, _case) = setup(None);
    let backup_done = |core: &mut Core| {
        core.readiness()
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.key == "backup")
            .unwrap()
            .done
    };
    let net = core.safety_net(&NoGoogle).unwrap();
    assert_eq!(net.google.state, crate::NetState::Unavailable);
    assert_eq!(net.backup.text, "אין עדיין גיבוי");
    assert_eq!(net.backup.state, crate::NetState::Missing);
    assert!(!net.ok && !backup_done(&mut core));

    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("b")).unwrap();
    let net = core.safety_net(&NoGoogle).unwrap();
    assert_eq!(net.backup.text, "גיבוי מהיום");
    assert_eq!(net.backup.state, crate::NetState::Ok);
    assert!(backup_done(&mut core));
    assert!(!net.ok, "without Google sign-in the net is never whole");

    // Eight days ago: a reminder is due, so neither counts it (it was two weeks before D-107).
    let eight_days_ago = crate::dates::unix_now() - 8 * dv_domain::dates::DAY_SECS;
    core.vault_mut()
        .unwrap()
        .set_setting("backup_last_at", &eight_days_ago.to_string())
        .unwrap();
    let net = core.safety_net(&NoGoogle).unwrap();
    assert_eq!(net.backup.text, "הגיבוי האחרון לפני 8 ימים");
    assert_eq!(net.backup.state, crate::NetState::Missing);
    assert!(!backup_done(&mut core));

    core.write_backup(&drive.path().join("c")).unwrap();
    core.change_password(PASSWORD, false, "חתול כחול ישן על הספה")
        .unwrap();
    let net = core.safety_net(&NoGoogle).unwrap();
    assert_eq!(net.backup.text, "הסיסמה הוחלפה אחרי הגיבוי האחרון");
    assert_eq!(net.backup.state, crate::NetState::Missing);
    assert!(!backup_done(&mut core));
}

#[test]
fn readiness_follows_what_the_program_checks_and_keeps_what_she_confirmed() {
    let (_dir, mut core, _case) = setup(None);
    let done = |core: &mut Core, key: &str| {
        core.readiness()
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.key == key)
            .unwrap()
            .done
    };
    let keys: Vec<String> = core
        .readiness()
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.key)
        .collect();
    assert_eq!(
        keys,
        [
            "zdr",
            "consent_form",
            "legal",
            "score_tables",
            "api_key",
            "backup",
            "disk"
        ]
    );

    // A key for the AI she chose; the local model needs none; another company's key is not it.
    assert!(!done(&mut core, "api_key"));
    core.set_api_key("anthropic", "sk-ant-test-not-real", false)
        .unwrap();
    assert!(done(&mut core, "api_key"));
    core.set_model("gpt-5-1").unwrap();
    assert!(
        !done(&mut core, "api_key"),
        "a Claude key is not a ChatGPT key"
    );
    core.set_model("local-gemma").unwrap();
    assert!(done(&mut core, "api_key"));

    // A backup counts until the password changes.
    assert!(!done(&mut core, "backup"));
    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("b")).unwrap();
    assert!(done(&mut core, "backup"));
    core.change_password(PASSWORD, false, "חתול כחול ישן על הספה")
        .unwrap();
    assert!(!done(&mut core, "backup"));

    // Her confirmations survive the lock; unknown items are refused, not stored.
    for key in ["consent_form", "legal"] {
        core.confirm_readiness(key, true).unwrap();
    }
    for bad in [
        "disk",
        "api_key",
        "score_tables",
        "",
        "zdr ",
        "ZDR",
        "ready/zdr",
    ] {
        assert!(core.confirm_readiness(bad, true).is_err(), "{bad:?}");
    }
    core.lock();
    core.unlock("חתול כחול ישן על הספה").unwrap();
    assert!(done(&mut core, "consent_form") && done(&mut core, "legal"));
    assert!(!done(&mut core, "zdr"));
    assert!(!core.readiness().unwrap().all_done);
}

// ------------------------------------------------------------ unsaved typing

#[test]
fn every_open_editor_is_kept_at_the_lock_not_only_the_last_one() {
    let (_dir, mut core, case) = setup(None);
    let edit = |case_id: &str, place: &str, text: &str| crate::UnsavedEdit {
        case_id: case_id.into(),
        place: place.into(),
        view: "report".into(),
        text: text.into(),
    };
    // Two editors in two cases: a lock by hand keeps both, the latest last.
    let both = vec![
        edit(&case, "פסקה בסעיף רקע", "טקסט בתיק הראשון"),
        edit("other-case", "רישום מפגש", "טקסט בתיק השני"),
    ];
    core.hold_unsaved(both.clone());
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.take_unsaved().unwrap(), both);
    assert!(core.take_unsaved().unwrap().is_empty());

    // More editors than the cap: the latest are kept.
    let many: Vec<_> = (0..12)
        .map(|i| edit(&case, &format!("פסקה {i}"), &format!("טקסט {i}")))
        .collect();
    core.hold_unsaved(many.clone());
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(
        core.take_unsaved().unwrap(),
        many[12 - crate::unsaved::MAX_EDITS..].to_vec()
    );

    // A vault locked by an earlier version kept one edit, not a list: it still comes back.
    core.vault_mut()
        .unwrap()
        .set_state(
            crate::unsaved::UNSAVED_KEY,
            r#"{"case_id":"c","place":"פסקה","text":"טקסט ישן"}"#,
        )
        .unwrap();
    let old = core.take_unsaved().unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].text, "טקסט ישן");
}

#[test]
fn the_latest_text_of_an_editor_is_kept_and_a_long_one_is_cut_not_lost() {
    let (_dir, mut core, case) = setup(None);
    let edit = |text: String| crate::UnsavedEdit {
        case_id: case.clone(),
        place: "שאלה ל-AI".into(),
        view: "consult".into(),
        text,
    };
    core.hold_unsaved(vec![edit("גרסה ראשונה".into())]);
    core.hold_unsaved(vec![edit("גרסה שנייה".into())]);
    // The computer slept: the tick locks, and what was typed is kept for the next entry.
    let now = SystemTime::now();
    assert!(!core.tick(now));
    assert!(core.tick(now + Duration::from_secs(10 * 60)));
    core.unlock(PASSWORD).unwrap();
    assert_eq!(
        core.take_unsaved().unwrap(),
        vec![edit("גרסה שנייה".into())]
    );

    let long: String = "א".repeat(60_000);
    core.hold_unsaved(vec![edit(long)]);
    core.lock();
    core.unlock(PASSWORD).unwrap();
    let back = core.take_unsaved().unwrap().pop().unwrap();
    assert_eq!(back.text.chars().count(), 50_000);
    assert!(back.text.chars().all(|c| c == 'א'));

    // A locked session takes nothing new, so nothing is offered after the next entry.
    core.lock();
    core.hold_unsaved(vec![edit("הוקלד אחרי הנעילה".into())]);
    core.unlock(PASSWORD).unwrap();
    assert!(core.take_unsaved().unwrap().is_empty());
}

// ------------------------------------------------------------ rollback seal

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

#[test]
fn an_older_copy_of_the_vault_put_back_is_pointed_out_once() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let seal = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(&vault, None);
    core.seal_dir = Some(seal.path().to_path_buf());
    core.create_vault(PASSWORD).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.status().integrity_warning, None);
    core.lock();
    let old = dir.path().join("old");
    copy_dir(&vault, &old);

    // More work: the log grows, and the seal outside the vault moves with it.
    core.unlock(PASSWORD).unwrap();
    plain_case(&mut core, "TEST-0741", None);
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.status().integrity_warning, None);
    core.lock();

    // Someone puts the older folder back.
    std::fs::remove_dir_all(&vault).unwrap();
    copy_dir(&old, &vault);
    core.unlock(PASSWORD).unwrap();
    assert!(
        core.list_cases().unwrap().is_empty(),
        "it really is the older vault"
    );
    let warning = core
        .status()
        .integrity_warning
        .expect("the older copy is pointed out");
    assert!(warning.contains("ישנה"), "{warning}");
    let security = core.security_status();
    let log = security
        .items
        .iter()
        .find(|i| i.key == "log_integrity")
        .unwrap();
    assert_eq!(log.state, "warn");
    // Said once: the seal now follows this vault.
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.status().integrity_warning, None);
}

/// A header changed from outside: she is told in Hebrew, at every opening, also after a new
/// backup (it is never signed again, D-104), and the activity log says it in Hebrew too.
#[test]
fn a_changed_header_is_said_in_her_words_and_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    core.lock();
    let path = dir.path().join("vault.header");
    let mut header: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    header["created_at"] = json!(1);
    std::fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();

    let in_hebrew = |w: &str| {
        assert!(!w.bytes().any(|b| b.is_ascii_alphabetic()), "{w}");
        assert!(w.contains("שונה מחוץ לתוכנה"), "{w}");
    };
    core.unlock(PASSWORD).unwrap();
    let warning = core
        .status()
        .integrity_warning
        .expect("the change is pointed out");
    in_hebrew(&warning);
    assert!(warning.ends_with("כדאי לספר לאור."), "{warning}");
    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("b")).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    in_hebrew(
        &core
            .status()
            .integrity_warning
            .expect("still, after a new backup"),
    );
    let page = core.activity(None).unwrap();
    let said = page
        .entries
        .iter()
        .find(|e| e.event == "integrity_warning")
        .expect("in the activity log");
    in_hebrew(&said.text);
}

// ------------------------------------------------------------ the activity log

/// A made-up name, different on every run, so no fixed list can hide a leak.
fn runtime_name() -> String {
    const LETTERS: &[char] = &[
        'ב', 'ג', 'ד', 'ז', 'ח', 'ט', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ', 'ק', 'ר', 'ש', 'ת',
    ];
    let bytes = dv_vault::crypto::random_array::<8>().unwrap();
    bytes
        .iter()
        .map(|b| LETTERS[usize::from(*b) % LETTERS.len()])
        .collect()
}

#[test]
fn the_activity_log_never_carries_a_name_whatever_the_name_is() {
    let fake = FakeTransport::default();
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), Some(Box::new(fake.clone())));
    core.create_vault(PASSWORD).unwrap();
    let child = runtime_name();
    let mother = runtime_name();
    let nickname = runtime_name();
    let person = |role, value: &str, aliases: Vec<String>| IdentityInput {
        id: None,
        role,
        value: value.into(),
        aliases,
    };
    let case = core
        .create_case(
            CaseMeta {
                code: "TEST-0750".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![person(Role::Child, &child, vec![])],
        )
        .unwrap();
    core.set_identities(
        &case,
        vec![
            person(Role::Child, &child, vec![nickname.clone()]),
            person(Role::Mother, &mother, vec![]),
        ],
    )
    .unwrap();
    let input = core
        .add_input(
            &case,
            InputKind::Kindergarten,
            &format!("שיחה עם {mother}"),
            &format!("{mother} מספרת ש{child} (שקוראים לו {nickname}) נרדם מאוחר."),
        )
        .unwrap();
    core.update_input(
        &case,
        &input.id,
        &input.title,
        &format!("{mother} מספרת ש{child} נרדם מאוחר ומתעורר בלילה."),
    )
    .unwrap();
    let id = core
        .prepare_section(&case, "kindergarten", &format!("כתוב על השינה של {child}"))
        .unwrap()
        .approval_id
        .unwrap();
    core.send_section(&id).unwrap();
    core.approve_section(&case, "kindergarten").unwrap();
    core.add_own_paragraph(&case, "kindergarten", &format!("{child} שיתף פעולה."))
        .unwrap();
    let ask = core
        .prepare_consult(Some(&case), None, &format!("מה לשאול את {mother}?"))
        .unwrap()
        .approval_id
        .unwrap();
    core.send_consult(&ask).unwrap();
    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("b")).unwrap();
    core.delete_case(&case).unwrap();
    core.restore_case(&case).unwrap();
    core.lock();
    assert!(core.unlock("ניחוש ארוך אבל שגוי").is_err());
    core.not_before = None;
    core.unlock(PASSWORD).unwrap();

    let secrets = [&child, &mother, &nickname];
    let clean = |surface: &str, text: &str| {
        for s in secrets {
            assert!(
                !text.contains(s.as_str()),
                "a name reached {surface}: {text}"
            );
        }
    };
    let page = core.activity(None).unwrap();
    assert!(page.entries.len() >= 8, "{}", page.entries.len());
    for e in &page.entries {
        clean("an activity text", &e.text);
        clean("an activity event", &e.event);
    }
    // The raw log, as stored: the case is a keyed reference, the details carry no text.
    for e in core.vault_ref().unwrap().audit_entries(10_000).unwrap() {
        clean(
            "the raw log",
            &format!("{} {:?} {}", e.event, e.case_ref, e.meta),
        );
    }
    for body in fake.sent.lock().unwrap().iter() {
        clean("a request", body);
    }
}

// ------------------------------------------------------------ settings

#[test]
fn settings_persist_across_the_lock_and_nonsense_is_refused_unchanged() {
    let (_dir, mut core, _case) = setup(None);
    core.set_model("claude-sonnet-5").unwrap();
    core.set_speed("fast").unwrap();
    core.set_monthly_cap(Some(25)).unwrap();
    core.set_screen_protection(false).unwrap();
    core.set_practitioner(vec![
        "  ד\"ר רותם אלמוג ".into(),
        "   ".into(),
        String::new(),
    ])
    .unwrap();

    // Refused, and nothing changes.
    for bad in [
        "",
        "claude",
        "claude-sonnet-5 ",
        "CLAUDE-SONNET-5",
        "gpt-3.5-turbo",
    ] {
        assert!(
            matches!(core.set_model(bad), Err(CoreError::Refused(_))),
            "{bad:?}"
        );
    }
    for bad in ["", "Fast", "slow", "balanced\n"] {
        assert!(
            matches!(core.set_speed(bad), Err(CoreError::Refused(_))),
            "{bad:?}"
        );
    }
    assert!(matches!(
        core.set_monthly_cap(Some(0)),
        Err(CoreError::Refused(_))
    ));
    for bad in ["", "deepseek", "Anthropic", "local"] {
        assert!(core.set_api_key(bad, "x", true).is_err(), "{bad:?}");
    }
    assert!(core.status().keys.is_empty());

    // Lock time is kept within one hour and one minute.
    core.set_lock_minutes(0).unwrap();
    assert_eq!(core.status().lock_minutes, 1);
    core.set_lock_minutes(10_000).unwrap();
    assert_eq!(core.status().lock_minutes, 60);
    core.set_lock_minutes(15).unwrap();

    core.lock();
    core.unlock(PASSWORD).unwrap();
    let s = core.status();
    assert_eq!(s.model, "claude-sonnet-5");
    assert_eq!(s.speed, "fast");
    assert_eq!(s.lock_minutes, 15);
    assert!(!s.screen_protection);
    assert_eq!(s.practitioner, vec!["ד\"ר רותם אלמוג".to_owned()]);
    assert_eq!(core.usage_summary().unwrap().cap_usd, Some(25));
    // The idle lock follows the minutes she chose.
    core.last_activity = Instant::now() - Duration::from_secs(14 * 60);
    assert!(!core.lock_if_idle());
    core.last_activity = Instant::now() - Duration::from_secs(16 * 60);
    assert!(core.lock_if_idle());
}

#[test]
fn the_requests_use_the_chosen_model_and_speed() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    for (model, speed, effort) in [
        ("claude-sonnet-5", "fast", "low"),
        ("claude-opus-5", "thorough", "high"),
        ("claude-opus-4-8", "balanced", "medium"),
    ] {
        core.set_model(model).unwrap();
        core.set_speed(speed).unwrap();
        let id = approval(&mut core, &case);
        core.send_section(&id).unwrap();
        let body: Value =
            serde_json::from_str(&fake.sent.lock().unwrap().last().cloned().unwrap()).unwrap();
        assert_eq!(body["model"], model);
        assert!(
            body.to_string()
                .contains(&format!("\"effort\":\"{effort}\"")),
            "{body}"
        );
    }
}

/// Small jobs think little, whatever the speed chosen for drafting (D-137).
#[test]
fn small_jobs_think_little_and_sections_at_the_chosen_speed() {
    let mut core = Core::for_tests(tempfile::tempdir().unwrap().keep().as_path(), None);
    core.create_vault(PASSWORD).unwrap();
    for (speed, section, light) in [
        ("fast", "low", "low"),
        ("balanced", "medium", "low"),
        ("thorough", "high", "medium"),
    ] {
        core.set_speed(speed).unwrap();
        assert_eq!(
            core.model_for(Task::Draft).unwrap().effort,
            section,
            "{speed}"
        );
        assert_eq!(
            core.model_for(Task::Light).unwrap().effort,
            light,
            "{speed}"
        );
        assert_eq!(core.model_for(Task::Sort).unwrap().effort, "low", "{speed}");
    }
}

// ------------------------------------------------------------ regressions

#[test]
fn a_model_kept_from_before_it_left_the_allow_list_is_not_put_in_requests() {
    // The status showed the default model while the request still named the stored one,
    // which the egress allow-list then refused on every send.
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.vault_mut()
        .unwrap()
        .set_setting(MODEL_KEY, "claude-retired-1")
        .unwrap();
    assert_eq!(core.status().model, dv_ai::DEFAULT_MODEL);
    let id = approval(&mut core, &case);
    core.send_section(&id).unwrap();
    let body: Value =
        serde_json::from_str(&fake.sent.lock().unwrap().last().cloned().unwrap()).unwrap();
    assert_eq!(body["model"], dv_ai::DEFAULT_MODEL);
    assert!(dv_egress::ALLOWED_MODELS.contains(&dv_ai::DEFAULT_MODEL));
}

#[test]
fn the_wait_after_wrong_secrets_doubles_up_to_a_minute_and_counts_down_exactly() {
    // The lock screen counts the wait down from the seconds in `Backoff` (7651f2b), so
    // they must be the real wait: 2, 4, 8 … seconds, never more than a minute.
    let (_dir, mut core, _case) = setup(None);
    core.lock();
    for (attempt, expected) in [
        (1u32, 2u64),
        (2, 4),
        (3, 8),
        (4, 16),
        (5, 32),
        (6, 60),
        (7, 60),
    ] {
        core.not_before = None;
        let wrong = if attempt % 2 == 0 {
            core.unlock_with_recovery("AAAA-BBBB-CCCC").unwrap_err()
        } else {
            core.unlock("ניחוש ארוך אבל שגוי").unwrap_err()
        };
        assert!(matches!(wrong, CoreError::Vault(_)), "{wrong:?}");
        let Err(CoreError::Backoff(s)) = core.unlock(PASSWORD) else {
            panic!("attempt {attempt}: the right password must wait too");
        };
        assert!(
            (expected..=expected + 1).contains(&s),
            "attempt {attempt}: {s}s"
        );
        assert!(!core.status().unlocked);
    }
    // The right secret after the wait clears the count: the next mistake waits 2 seconds again.
    core.not_before = None;
    core.unlock(PASSWORD).unwrap();
    core.lock();
    assert!(core.unlock("ניחוש ארוך אבל שגוי").is_err());
    let Err(CoreError::Backoff(s)) = core.unlock(PASSWORD) else {
        panic!("a wait after the first mistake");
    };
    assert!(s <= 3, "{s}");
}

#[test]
fn only_the_material_deleted_last_can_be_put_back() {
    let (_dir, mut core, case) = setup(None);
    let inputs = core.vault_ref().unwrap().inputs(&case).unwrap();
    let (first, second) = (inputs[0].id.clone(), inputs[1].id.clone());
    core.delete_input(&case, &first).unwrap();
    core.delete_input(&case, &second).unwrap();
    // Deleting something that is not there does not drop what is held.
    core.delete_input(&case, "no-such-material").unwrap_or(());
    assert!(core.restore_input(&case, &first).is_err());
    core.restore_input(&case, &second).unwrap();
    let left: Vec<String> = core
        .vault_ref()
        .unwrap()
        .inputs(&case)
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(left, vec![second]);
}

#[test]
fn the_emergency_stop_works_while_locked_and_shows_on_the_security_screen() {
    let (_dir, mut core, _case) = setup(None);
    core.lock();
    let stopped = core.set_send_stopped(true).unwrap();
    let send = |s: &SecurityStatus| {
        s.items
            .iter()
            .find(|i| i.key == "send")
            .unwrap()
            .state
            .clone()
    };
    let on = (stopped.send_stopped, send(&stopped));
    let resumed = core.set_send_stopped(false).unwrap();
    assert_eq!(on, (true, "warn".to_owned()));
    assert!(!resumed.send_stopped);
    assert_eq!(send(&resumed), "ok");
    assert!(
        !core.is_unlocked(),
        "stopping never needs, or opens, the vault"
    );
}

/// An answer that comes back after the vault was locked and opened again, or after its case
/// went to the recycle bin, is not written: it lands only in the session and the case it was
/// approved in. What left the computer is in the history either way.
#[test]
fn an_answer_lands_only_in_the_session_and_case_it_was_approved_in() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let logged_before = sends_logged(&mut core);

    // Locked and opened again while the answer was on its way (the computer slept and woke).
    let id = approval(&mut core, &case);
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    let response = out.transmit();
    assert!(response.is_ok());
    core.lock();
    core.unlock(PASSWORD).unwrap();
    let err = core.finish_section(out, response).unwrap_err();
    assert!(
        matches!(err, CoreError::Needs(_, ErrorAction::Unlock)),
        "{err:?}"
    );
    assert!(
        core.chat(&case, "kindergarten").unwrap().is_empty(),
        "nothing stored"
    );
    assert_eq!(sends_logged(&mut core), logged_before + 1, "but it left");

    // The case went to the recycle bin while the answer was on its way.
    let id = approval(&mut core, &case);
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    let response = out.transmit();
    core.delete_case(&case).unwrap();
    assert!(matches!(
        core.finish_section(out, response),
        Err(CoreError::Refused(_))
    ));
    core.restore_case(&case).unwrap();
    assert!(core.chat(&case, "kindergarten").unwrap().is_empty());
    assert_eq!(sends_logged(&mut core), logged_before + 2);

    // Without a lock or a deletion in between, it lands.
    let id = approval(&mut core, &case);
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    let response = out.transmit();
    core.finish_section(out, response).unwrap();
    assert!(!core.chat(&case, "kindergarten").unwrap().is_empty());
}

#[test]
fn a_lock_while_a_request_is_on_its_way_does_not_bring_its_approval_back() {
    // A send that failed after the vault locked put its approval back in memory, so it
    // outlived the lock and could be sent again after the next unlock.
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let offline = || Err(CoreError::Egress(EgressError::Offline));

    // Locked while it was out, and still locked when the failure arrives.
    let id = approval(&mut core, &case);
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    core.lock();
    assert!(core.finish_section(out, offline()).is_err());
    core.unlock(PASSWORD).unwrap();
    assert!(matches!(
        core.send_section(&id),
        Err(CoreError::NotFound(_))
    ));

    // Locked and opened again before the failure arrives (the computer slept and woke).
    let id = approval(&mut core, &case);
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.finish_section(out, offline()).is_err());
    assert!(matches!(
        core.send_section(&id),
        Err(CoreError::NotFound(_))
    ));

    // The same for a consultation.
    let ask = core
        .prepare_consult(Some(&case), None, "איך מנסחים המלצה?")
        .unwrap()
        .approval_id
        .unwrap();
    let out = core.begin_send(&ask, SendKind::Consult).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.finish_consult(out, offline()).is_err());
    assert!(matches!(
        core.send_consult(&ask),
        Err(CoreError::NotFound(_))
    ));
    assert!(fake.sent.lock().unwrap().is_empty());

    // Without a lock in between, a failure still keeps the approval for a retry.
    let id = approval(&mut core, &case);
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    assert!(core.finish_section(out, offline()).is_err());
    core.send_section(&id).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
}

#[test]
fn an_approval_for_one_kind_of_request_is_not_sent_by_the_command_for_another() {
    let t = Scripted::default();
    let (_dir, mut core, case) = scripted_setup(&t);
    let id = approval(&mut core, &case);

    // A section approval handed to every other command: refused before anything leaves.
    assert!(matches!(
        core.send_consult(&id),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(core.send_sort(&id), Err(CoreError::NotFound(_))));
    assert!(matches!(core.send_verify(&id), Err(CoreError::NotFound(_))));
    assert!(matches!(
        core.send_style_profile(&id),
        Err(CoreError::NotFound(_))
    ));
    for kind in [
        SendKind::Consult,
        SendKind::Sort,
        SendKind::Verify,
        SendKind::StyleAnalysis,
        SendKind::StyleSynthesis,
    ] {
        assert!(matches!(
            core.begin_send(&id, kind),
            Err(CoreError::NotFound(_))
        ));
    }
    assert!(t.attempts().is_empty(), "nothing may reach the network");
    assert_eq!(sends_logged(&mut core), 0);

    // The approval still waits for its own command, which sends it once and records it.
    core.send_section(&id).unwrap();
    assert_eq!(t.attempts().len(), 1);
    assert_eq!(sends_logged(&mut core), 1);
    assert_eq!(
        core.vault_ref()
            .unwrap()
            .transmissions(&case)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn approving_every_clean_draft_takes_only_what_the_core_still_sees_clean_and_once() {
    // D-139: the screen's list may be older than the case; the core decides again, and a
    // second press finds nothing left to approve.
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let id = approval(&mut core, &case);
    core.send_section(&id).unwrap();
    let proposed: Vec<ParagraphView> = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .flat_map(|s| s.paragraphs)
        .filter(|p| p.status == DraftStatus::Proposed)
        .collect();
    assert!(!proposed.is_empty());
    let clean = proposed
        .iter()
        .filter(|p| p.warnings.is_empty() && p.replaces.is_none())
        .count();
    let mut ids: Vec<String> = proposed.iter().map(|p| p.id.clone()).collect();
    ids.push("no-such-draft".to_owned());
    let n = core.approve_clean_paragraphs(&case, &ids).unwrap();
    assert_eq!(n as usize, clean);
    assert_eq!(core.approve_clean_paragraphs(&case, &ids).unwrap(), 0);
    // What had a note is still waiting for her.
    let still: usize = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .flat_map(|s| s.paragraphs)
        .filter(|p| p.status == DraftStatus::Proposed)
        .count();
    assert_eq!(still, proposed.len() - clean);
}
