//! F2: long work runs away from the core's lock. Each job is begun under the lock, runs with
//! no access to the core (other commands go on meanwhile), and lands back under the lock;
//! work begun before a lock never lands after it. Fabricated data only.

use std::sync::{Arc, Mutex};

use super::*;

/// The core behind its lock, as the shell holds it.
fn shared(core: Core) -> Arc<Mutex<Core>> {
    Arc::new(Mutex::new(core))
}

/// Run `job` on its own thread while this thread uses the core; the work in between proves
/// the core was free.
fn while_running<J: Send + 'static, D: Send + 'static>(
    core: &Arc<Mutex<Core>>,
    job: J,
    run: impl FnOnce(J) -> D + Send + 'static,
    meanwhile: impl FnOnce(&mut Core),
) -> D {
    let worker = std::thread::spawn(move || run(job));
    meanwhile(&mut core.lock().unwrap());
    worker.join().unwrap()
}

fn approved_case(core: &mut Core, case: &str) {
    let p = core.prepare_section(case, "kindergarten", "טיוטה").unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let para = core
        .case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs
        .remove(0);
    core.approve_paragraph(case, &para.id).unwrap();
}

#[test]
fn unlocking_derives_the_key_while_the_core_answers() {
    let (_dir, mut core, case) = setup(None);
    core.lock();
    let core = shared(core);
    let job = core.lock().unwrap().begin_unlock(PASSWORD).unwrap();
    let opened = while_running(&core, job, UnlockJob::run, |c| {
        // The status answers while the key is derived.
        assert!(!c.status().unlocked);
    });
    let mut c = core.lock().unwrap();
    assert!(c.finish_unlock(opened).unwrap().unlocked);
    assert!(c.case_detail(&case).is_ok());

    // Two clicks: the second waits for the first, so two openings never run side by side.
    c.lock();
    let first = c.begin_unlock(PASSWORD).unwrap();
    assert!(matches!(
        c.begin_unlock(PASSWORD),
        Err(CoreError::Needs(_, ErrorAction::Dismiss))
    ));
    assert!(matches!(
        c.begin_unlock_with_recovery("ABCD"),
        Err(CoreError::Needs(_, ErrorAction::Dismiss))
    ));
    assert!(c.finish_unlock(first.run()).unwrap().unlocked);
    assert!(c.case_detail(&case).is_ok());

    // An opening that never came back frees the place after a while, and when it does come
    // back after another opened the vault, its copy is dropped and the open vault stays.
    c.lock();
    let lost = c.begin_unlock(PASSWORD).unwrap();
    c.age_secret_work();
    let opened = c.begin_unlock(PASSWORD).unwrap().run();
    assert!(c.finish_unlock(opened).unwrap().unlocked);
    assert!(c.finish_unlock(lost.run()).unwrap().unlocked);
    assert!(c.case_detail(&case).is_ok());
    // A late one never frees the place of the newer check that took it.
    c.lock();
    let lost = c.begin_unlock(PASSWORD).unwrap();
    c.age_secret_work();
    let opened = c.begin_unlock(PASSWORD).unwrap();
    assert!(c.finish_unlock(lost.run()).unwrap().unlocked);
    assert!(matches!(
        c.begin_unlock(PASSWORD),
        Err(CoreError::Needs(_, ErrorAction::Dismiss))
    ));
    assert!(c.finish_unlock(opened.run()).unwrap().unlocked);

    // A wrong password counts toward the waiting time, as before, and guesses cannot be
    // sent side by side to slip past it: the second waits for the first's answer.
    c.lock();
    let wrong = c.begin_unlock("ניחוש ארוך אבל שגוי לגמרי").unwrap();
    assert!(matches!(
        c.begin_unlock("ניחוש שני, גם הוא שגוי"),
        Err(CoreError::Needs(_, ErrorAction::Dismiss))
    ));
    assert!(matches!(
        c.finish_unlock(wrong.run()),
        Err(CoreError::Vault(VaultError::WrongSecret))
    ));
    assert!(matches!(
        c.begin_unlock(PASSWORD),
        Err(CoreError::Backoff(_))
    ));
}

#[test]
fn a_new_password_is_made_while_the_core_works_and_lands_only_in_its_session() {
    let (_dir, core, case) = setup(None);
    let core = shared(core);
    let new_password = "סיסמה חדשה וארוכה מאוד לכספת";
    let job = core
        .lock()
        .unwrap()
        .begin_change_password(PASSWORD, false, new_password)
        .unwrap();
    let made = while_running(&core, job, SlotJob::run, |c| {
        c.add_input(&case, InputKind::Observation, "תצפית", "אלון שיחק בחצר.")
            .unwrap();
        // One password or kit at a time: two would each write the header.
        assert!(matches!(
            c.begin_new_recovery_kit(PASSWORD, false),
            Err(CoreError::Needs(_, ErrorAction::Dismiss))
        ));
    });
    let mut c = core.lock().unwrap();
    c.finish_change_password(made).unwrap();
    c.lock();
    assert!(c.unlock(PASSWORD).is_err());
    std::thread::sleep(std::time::Duration::from_millis(2100));
    c.unlock(new_password).unwrap();

    // A weak password is refused before any slow work.
    assert!(matches!(
        c.begin_change_password(new_password, false, "קצר"),
        Err(CoreError::Vault(VaultError::Policy(_)))
    ));

    // Made before a lock: dropped after it, and the password stays as it was.
    let job = c
        .begin_change_password(new_password, false, "עוד סיסמה חדשה וארוכה מאוד")
        .unwrap();
    c.lock();
    let made = job.run();
    c.unlock(new_password).unwrap();
    assert!(matches!(
        c.finish_change_password(made),
        Err(CoreError::Needs(_, ErrorAction::Unlock))
    ));
    c.lock();
    c.unlock(new_password).unwrap();

    // A new kit, the same way.
    let kit = c.begin_new_recovery_kit(new_password, false).unwrap().run();
    let kit = c.finish_new_recovery_kit(kit).unwrap();
    assert!(c.confirm_recovery_key(&kit.recovery_key).unwrap());
}

#[test]
fn a_backup_is_written_and_read_back_while_the_core_works() {
    let (_dir, core, case) = setup(None);
    let out = tempfile::tempdir().unwrap();
    let core = shared(core);
    let task = core
        .lock()
        .unwrap()
        .begin_backup(&out.path().join("גיבוי"))
        .unwrap();
    let run = while_running(&core, task, BackupTask::run, |c| {
        assert!(c.case_detail(&case).is_ok());
    });
    let mut c = core.lock().unwrap();
    let done = c.finish_backup(run).unwrap();
    assert!(done.path.ends_with(".vaultbak"));
    let status = c.backup_status().unwrap();
    assert!(!status.due, "the backup counts");

    // The drill on that file, with the key derivation off the lock too.
    let bytes = std::fs::read(&done.path).unwrap();
    c.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    let drill = c.begin_check_backup(PASSWORD).unwrap();
    let drilled = drill.run();
    let check = c.finish_check_backup(drilled).unwrap();
    assert_eq!(check.cases, 1);

    // A wrong password puts the file back, so another password can be tried.
    let bytes = std::fs::read(&done.path).unwrap();
    c.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    let drilled = c
        .begin_check_backup("ניחוש ארוך אבל שגוי לגמרי")
        .unwrap()
        .run();
    assert!(c.finish_check_backup(drilled).is_err());
    std::thread::sleep(std::time::Duration::from_millis(2100));
    assert!(
        c.check_staged_backup(PASSWORD).is_ok(),
        "the file was put back"
    );

    // Begun before a lock: the file may be on the drive, but it does not count.
    let task = c.begin_backup(&out.path().join("אחרי נעילה")).unwrap();
    c.lock();
    let run = task.run();
    c.unlock(PASSWORD).unwrap();
    assert!(c.finish_backup(run).is_err());
}

#[test]
fn reading_a_document_and_exporting_run_while_the_core_works() {
    let (_dir, core, case) = setup(None);
    let core = shared(core);
    let bytes = super::import_export::docx("אלון הגיע עם אמו. נצפה קושי במעברים.", "", "");
    let job = core
        .lock()
        .unwrap()
        .begin_import_document(&case, "סיכום.docx", bytes.clone())
        .unwrap();
    let read = while_running(&core, job, ReadJob::run, |c| {
        assert!(c.case_detail(&case).is_ok());
    });
    let preview = core
        .lock()
        .unwrap()
        .finish_import_document(&case, read)
        .unwrap();
    assert!(preview.body.contains("אלון הגיע"));

    let job = core
        .lock()
        .unwrap()
        .begin_import_style_source("דוח.docx", bytes)
        .unwrap();
    let read = job.run();
    let style = core
        .lock()
        .unwrap()
        .finish_import_style_source(read)
        .unwrap();
    assert!(!style.parts.is_empty());

    approved_case(&mut core.lock().unwrap(), &case);
    let job = core
        .lock()
        .unwrap()
        .begin_export_report(&case, Some("סיסמה ארוכה מאוד לקובץ"))
        .unwrap();
    let exported = while_running(&core, job, ExportJob::run, |c| {
        assert!(c.case_detail(&case).is_ok());
    });
    let mut c = core.lock().unwrap();
    let (file, check) = c.finish_export(exported).unwrap();
    assert!(!file.is_empty() && !check.file_name.contains("אלון"));
    assert!(c
        .vault_ref()
        .unwrap()
        .audit_entries(5)
        .unwrap()
        .iter()
        .any(|e| e.event == "export"));

    // A weak password fails in the job and records nothing.
    let job = c.begin_export_report(&case, Some("קצר")).unwrap();
    let before = c.vault_ref().unwrap().audit_seq();
    assert!(matches!(
        c.finish_export(job.run()),
        Err(CoreError::Refused(_))
    ));
    assert_eq!(c.vault_ref().unwrap().audit_seq(), before);

    // Begun before a lock: nothing lands after it.
    let job = c
        .begin_import_document(&case, "סיכום.docx", vec![])
        .unwrap();
    c.lock();
    let read = job.run();
    c.unlock(PASSWORD).unwrap();
    assert!(c.finish_import_document(&case, read).is_err());
}

#[test]
fn a_past_report_read_before_a_new_name_is_neutralized_with_it() {
    let (_dir, mut core, case) = setup(None);
    let bytes = super::import_export::docx("רקע: פרפר הגיע עם אמו לאבחון בגן.", "", "");
    let before = core.import_style_source("דוח.docx", &bytes).unwrap();
    let all: String = before.parts.iter().map(|p| p.text.clone()).collect();
    assert!(
        all.contains("פרפר"),
        "an everyday word, not yet a name: {all}"
    );
    let job = core.begin_import_style_source("דוח.docx", bytes).unwrap();
    let read = job.run();
    // A nickname added while the report was read: the staged parts hide it all the same.
    let mut ids = core.case_detail(&case).unwrap().identities;
    let mut inputs: Vec<IdentityInput> = ids
        .drain(..)
        .map(|i| IdentityInput {
            id: Some(i.id),
            role: i.role,
            value: i.value,
            aliases: i.aliases,
        })
        .collect();
    inputs.push(IdentityInput {
        id: None,
        role: Role::Child,
        value: "פרפר".into(),
        aliases: vec![],
    });
    core.set_identities(&case, inputs).unwrap();
    let preview = core.finish_import_style_source(read).unwrap();
    let all: String = preview.parts.iter().map(|p| p.text.clone()).collect();
    assert!(!all.contains("פרפר"), "{all}");
}

#[test]
fn a_backup_written_while_the_password_changed_still_asks_for_a_new_one() {
    let (_dir, mut c, _case) = setup(None);
    let out = tempfile::tempdir().unwrap();
    let new_password = "סיסמה חדשה וארוכה מאוד לכספת";
    // The backup's snapshot is taken, then the password changes before it lands.
    let task = c.begin_backup(&out.path().join("גיבוי")).unwrap();
    c.change_password(PASSWORD, false, new_password).unwrap();
    assert!(c.backup_status().unwrap().secret_changed);
    c.finish_backup(task.run()).unwrap();
    // It opens only with the old password, so the reminder stays.
    assert!(c.backup_status().unwrap().secret_changed);
    // A backup taken after the change clears it.
    let task = c.begin_backup(&out.path().join("גיבוי")).unwrap();
    c.finish_backup(task.run()).unwrap();
    assert!(!c.backup_status().unwrap().secret_changed);
}
