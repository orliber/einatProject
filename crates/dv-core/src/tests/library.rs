//! D-023: the library, and what came after it in this file.

use super::*;

#[test]
fn the_sample_case_is_made_once_and_its_child_is_hidden_like_any_other() {
    let (_dir, mut core, case) = setup(None);
    assert!(!core.case_detail(&case).unwrap().sample);
    let sample = core.create_sample_case().unwrap();
    assert_eq!(core.create_sample_case().unwrap(), sample, "opened once");
    let detail = core.case_detail(&sample).unwrap();
    assert!(detail.sample);
    assert_eq!(detail.inputs.len(), 3);
    assert!(detail.meta.consent.is_some());
    let prepared = core
        .prepare_section(&sample, "background", "טיוטה")
        .unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);

    // Thrown away, a new one is made on request.
    core.delete_case(&sample).unwrap();
    let again = core.create_sample_case().unwrap();
    assert_ne!(again, sample);
    assert!(core.case_detail(&again).unwrap().sample);
}

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
fn a_moved_case_is_in_one_folder_and_a_name_is_not_taken_twice_side_by_side() {
    let (_dir, mut core, case) = setup(None);
    let a = core.create_folder(None, "גן הדקל").unwrap();
    let b = core.create_folder(None, "גן האלון").unwrap();
    core.move_case(&case, Some(&a.id)).unwrap();
    core.move_case(&case, Some(&b.id)).unwrap();
    // Moved, not copied: one case, in the last folder only.
    let cases = core.list_cases().unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].folder_id.as_deref(), Some(b.id.as_str()));
    // Into the bin and back: it returns to where it was, still once.
    core.delete_case(&case).unwrap();
    assert!(core.list_cases().unwrap().is_empty());
    core.restore_case(&case).unwrap();
    let cases = core.list_cases().unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].folder_id.as_deref(), Some(b.id.as_str()));

    // The same name side by side is refused, whatever the spaces or letter case.
    for name in ["גן הדקל", "  גן הדקל "] {
        assert!(matches!(
            core.create_folder(None, name),
            Err(CoreError::Refused(_))
        ));
    }
    assert!(matches!(
        core.rename_folder(&b.id, "גן הדקל"),
        Err(CoreError::Refused(_))
    ));
    // Its own name again is fine, and so is the same name one level down.
    core.rename_folder(&a.id, "גן הדקל").unwrap();
    let inner = core.create_folder(Some(&b.id), "גן הדקל").unwrap();
    // Moving it up next to its namesake is refused, and nothing moves.
    assert!(matches!(
        core.move_folder(&inner.id, None),
        Err(CoreError::Refused(_))
    ));
    assert_eq!(
        core.folders()
            .unwrap()
            .into_iter()
            .find(|f| f.id == inner.id)
            .unwrap()
            .parent_id
            .as_deref(),
        Some(b.id.as_str())
    );
    assert!(matches!(
        core.rename_folder("no-such-folder", "x"),
        Err(CoreError::Vault(VaultError::NotFound))
    ));
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
fn a_backup_is_made_by_itself_when_the_drive_is_there() {
    let (_dir, mut core, _case) = setup(None);
    // Never before she chose a folder herself.
    assert!(core.auto_backup().unwrap().is_none());
    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("שלי.vaultbak"))
        .unwrap();
    // Not due: nothing.
    assert!(core.auto_backup().unwrap().is_none());

    // Older auto backups in the folder, and one of hers with a similar name.
    for day in ["2026-01-01", "2026-02-01", "2026-03-01"] {
        std::fs::write(
            drive
                .path()
                .join(format!("גיבוי אוטומטי כספת האבחון {day}.vaultbak")),
            b"old",
        )
        .unwrap();
    }
    std::fs::write(drive.path().join("גיבוי אוטומטי כספת האבחון.txt"), b"hers").unwrap();

    // A new password makes it due; the drive is connected: a backup is made by itself.
    core.mark_secret_changed().unwrap();
    let done = core.auto_backup().unwrap().unwrap();
    assert!(done.path.contains("גיבוי אוטומטי כספת האבחון"));
    assert!(!core.backup_status().unwrap().due);
    let mut names: Vec<String> = std::fs::read_dir(drive.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    // The oldest automatic one went; hers stayed.
    assert_eq!(names.len(), 2 + crate::AUTO_KEEP, "{names:?}");
    assert!(!names.iter().any(|n| n.contains("2026-01-01")));
    assert!(names.contains(&"שלי.vaultbak".to_owned()));
    assert!(names.contains(&"גיבוי אוטומטי כספת האבחון.txt".to_owned()));

    // Turned off: nothing, even when due.
    core.mark_secret_changed().unwrap();
    assert!(!core.set_auto_backup(false).unwrap().auto);
    assert!(core.auto_backup().unwrap().is_none());
    assert!(core.set_auto_backup(true).unwrap().auto);

    // The drive is out: nothing, and no error.
    drop(drive);
    assert!(core.auto_backup().unwrap().is_none());

    // A locked vault: nothing.
    core.lock();
    assert!(core.auto_backup().unwrap().is_none());
}

#[test]
fn an_open_edit_survives_the_lock_inside_the_vault() {
    let (dir, mut core, case) = setup(None);
    let edit = |text: &str| crate::UnsavedEdit {
        case_id: case.clone(),
        place: "פסקה בסעיף רקע".into(),
        view: "report".into(),
        text: text.into(),
    };
    // Nothing kept: nothing offered.
    assert!(core.take_unsaved().unwrap().is_empty());
    // An edit saved (closed) before the lock is not kept.
    core.hold_unsaved(vec![edit("טיוטה")]);
    core.hold_unsaved(Vec::new());
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.take_unsaved().unwrap().is_empty());

    let text = "הילד הגיע לאבחון בליווי אמו, ושיתף פעולה לאורך כל המפגשים.";
    core.hold_unsaved(vec![edit(text)]);
    assert!(core.lock_with_computer());
    // Locked: a closing editor changes nothing.
    core.hold_unsaved(Vec::new());
    // Not on disk outside the vault.
    for entry in walkdir(dir.path()) {
        let bytes = std::fs::read(&entry).unwrap();
        assert!(
            !bytes.windows(text.len()).any(|w| w == text.as_bytes()),
            "{entry:?}"
        );
    }
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.take_unsaved().unwrap(), vec![edit(text)]);
    // Offered once.
    assert!(core.take_unsaved().unwrap().is_empty());
    // Whitespace is not an edit worth keeping.
    core.hold_unsaved(vec![edit("  \n")]);
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.take_unsaved().unwrap().is_empty());
    // An edit kept by an earlier version (no view) still comes back.
    let old: crate::UnsavedEdit =
        serde_json::from_str(r#"{"case_id":"c","place":"פסקה","text":"טקסט"}"#).unwrap();
    assert_eq!(old.view, "");
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
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
    let mut stranger_hidden = false;
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        match core.import_document(&case, &name, &bytes) {
            Ok(p) => {
                assert!(p.suspects.is_empty() && p.name_suggestions.is_empty());
                stranger_hidden |= p.auto_hidden.iter().any(|a| a.token.contains("עומרי"));
                core.add_input(&case, p.suggested_kind, &p.title, &p.body)
                    .unwrap();
            }
            Err(e) => assert!(name.contains("סרוק"), "{name}: {}", e.to_ui().message),
        }
    }
    assert!(
        stranger_hidden,
        "the friend the case does not list is hidden without asking"
    );

    // Nothing to answer: the request clears as it is.
    let prepared = core.prepare_sort(&case).unwrap();
    assert!(prepared.suspects.is_empty());
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

/// "להחזיר" must stick for every kind of hidden item: after one click on each, the same
/// request hides none of them again (an item that comes back looks like a broken button).
#[test]
fn every_restore_sticks_after_one_round() {
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
    assert!(first.auto_hidden.len() >= 5, "{:?}", first.auto_hidden);
    for a in &first.auto_hidden {
        core.restore_auto_hidden(&case, &a.token, &a.tag).unwrap();
    }
    let again = core.prepare_sort(&case).unwrap();
    let back: Vec<&str> = again.auto_hidden.iter().map(|a| a.token.as_str()).collect();
    assert!(back.is_empty(), "hidden again after restoring: {back:?}");
    assert!(again.approval_id.is_some(), "{:?}", again.blocked);
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

/// "עובדת" is generalized only when a job or a workplace follows it.
#[test]
fn work_words_are_generalized_only_before_a_job() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    core.add_input(
        &case,
        InputKind::FreeText,
        "הערה",
        "הגננת עובדת איתו על המעברים. מנהלת הגן הצטרפה לשיחה. האם עובדת כמנהלת חשבונות, והאב עובד בבנק.",
    )
    .unwrap();
    let prepared = core.prepare_sort(&case).unwrap();
    let asked: Vec<String> = prepared
        .auto_hidden
        .iter()
        .filter(|a| a.kind == dv_privacy::AutoKind::Indirect)
        .map(|a| a.token.clone())
        .collect();
    let out = outgoing(&prepared);
    assert!(out.contains("האם עובדת בתחום הכספים"), "{out}");
    assert!(out.contains("האב עובד בתחום הכספים"), "{out}");
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
    assert!(
        prepared.auto_hidden.is_empty(),
        "{:?}",
        prepared.auto_hidden
    );
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

/// An editor left open on an older wording does not silently write over a newer one: her text
/// is saved, the newer wording stays among the earlier versions, and the page is told.
#[test]
fn a_stale_editor_keeps_the_newer_wording_as_a_version_and_says_so() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "kindergarten", "נוסח ראשון.")
        .unwrap();
    let id = |core: &mut Core| {
        core.case_detail(&case)
            .unwrap()
            .sections
            .into_iter()
            .find(|s| s.key == "kindergarten")
            .unwrap()
            .paragraphs
            .remove(0)
            .id
    };
    let p = id(&mut core);
    // Two editors open on "נוסח ראשון.": the first saves, then the second one, still showing it.
    assert!(!core
        .edit_paragraph_opened(&case, &p, "נוסח שני.", Some("נוסח ראשון."))
        .unwrap());
    assert!(core
        .edit_paragraph_opened(&case, &p, "נוסח שלישי.", Some("נוסח ראשון."))
        .unwrap());
    let versions: Vec<String> = core
        .paragraph_versions(&case, &p)
        .unwrap()
        .into_iter()
        .map(|v| v.text)
        .collect();
    assert!(versions.contains(&"נוסח שני.".to_string()), "{versions:?}");
    // An editor that saw the current wording is not warned.
    assert!(!core
        .edit_paragraph_opened(&case, &p, "נוסח רביעי.", Some("נוסח שלישי."))
        .unwrap());
}

/// AI-6: a score written in the report is checked against the score table she entered, in
/// code, for every paragraph (hers too), each time the case is shown.
#[test]
fn written_scores_are_checked_against_the_score_table() {
    let (_dir, mut core, case) = setup(None);
    let sheet = dv_domain::ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![dv_domain::ScoreEntry {
            measure: "vci".into(),
            value: 95.0,
            note: String::new(),
        }],
        notes: String::new(),
    };
    core.save_scores(&case, None, &sheet).unwrap();
    core.add_own_paragraph(
        &case,
        "cognitive",
        "ההבנה המילולית (VCI) בטווח הממוצע (97).",
    )
    .unwrap();
    core.add_own_paragraph(
        &case,
        "cognitive",
        "ההבנה המילולית (VCI) בטווח הממוצע (95).",
    )
    .unwrap();
    let detail = core.case_detail(&case).unwrap();
    let paras = &detail
        .sections
        .iter()
        .find(|s| s.key == "cognitive")
        .unwrap()
        .paragraphs;
    assert!(
        paras[0]
            .warnings
            .iter()
            .any(|w| w.contains("97") && w.contains("95")),
        "{:?}",
        paras[0].warnings
    );
    assert!(paras[1].warnings.is_empty(), "{:?}", paras[1].warnings);
}

/// AI-7: "why did you write this?" shows the passages a paragraph was written from, with the
/// real names (on this computer only); a summary paragraph shows the approved sections.
#[test]
fn a_paragraph_shows_the_passages_it_was_written_from() {
    let (_dir, mut core, case) = setup(None);
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    let r = core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(!r.paragraphs.is_empty(), "{}", r.reply);
    let detail = core.case_detail(&case).unwrap();
    let para = detail
        .sections
        .iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs[0]
        .clone();
    let why = core.paragraph_sources(&case, &para.id).unwrap();
    assert!(!why.is_empty());
    assert!(
        why[0].text.contains("מעברים") && why[0].text.contains("אלון"),
        "{why:?}"
    );

    core.approve_section(&case, "kindergarten").unwrap();
    let s = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    core.send_section(&s.approval_id.unwrap()).unwrap();
    let summary = core.case_detail(&case).unwrap();
    let para = summary
        .sections
        .iter()
        .find(|s| s.key == "summary")
        .unwrap()
        .paragraphs[0]
        .clone();
    let why = core.paragraph_sources(&case, &para.id).unwrap();
    assert!(
        why.iter().any(|x| x.label.contains("סעיף מאושר")),
        "{why:?}"
    );
}

#[test]
fn nothing_goes_to_a_company_without_her_written_zero_retention_confirmation() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.set_model("gpt-5-1").unwrap();
    core.set_api_key("openai", "test-not-real", true).unwrap();
    // A key saved before the confirmation existed, or a confirmation taken back: refused.
    core.vault_mut()
        .unwrap()
        .set_setting(&format!("{ZDR_PREFIX}openai"), "")
        .unwrap();
    let id = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap()
        .approval_id
        .unwrap();
    let err = core.send_section(&id).unwrap_err();
    assert!(matches!(&err, CoreError::Needs(m, _) if m.contains("ZDR")));
    assert_eq!(
        err.to_ui().action,
        ErrorAction::settings("ai"),
        "fixed in Settings"
    );
    assert!(
        fake.sent.lock().unwrap().is_empty(),
        "nothing left the program"
    );

    // Confirmed with the key: it goes, through the same gate.
    core.set_api_key("openai", "test-not-real", true).unwrap();
    core.send_section(&id).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
    // Deleting the key takes the confirmation with it.
    core.set_api_key("openai", "", false).unwrap();
    assert_eq!(
        core.vault_ref()
            .unwrap()
            .setting("zdr/openai")
            .unwrap()
            .as_deref(),
        Some("")
    );
}

#[test]
fn letters_are_written_from_the_approved_recommendations_only() {
    let (_dir, mut core, case) = setup(None);
    // Nothing approved in the sections a letter is written from: nothing is sent.
    core.add_own_paragraph(&case, "referral", "ההורים של אלון פנו בשל קושי במעברים.")
        .unwrap();
    assert!(matches!(
        core.prepare_letter(&case, "school", ""),
        Err(CoreError::Refused(_))
    ));
    assert!(core.prepare_letter(&case, "neighbors", "").is_err());
    core.add_own_paragraph(
        &case,
        "recommendations",
        "מומלץ שאלון יקבל הכנה מראש למעברים בגן.",
    )
    .unwrap();
    core.add_own_paragraph(&case, "summary", "אלון ילד סקרן, עם קושי בוויסות רגשי.")
        .unwrap();

    // The school letter sees the recommendations, never the background or the summary.
    let p = core
        .prepare_letter(&case, "school", "לציין שיחה עם הגננת")
        .unwrap();
    let shown: Vec<String> = p
        .parts
        .iter()
        .map(|r| {
            let out: String = r.outgoing.iter().map(|s| s.text.as_str()).collect();
            format!("{} {out}", r.label)
        })
        .collect();
    let all = shown.join("\n");
    assert!(all.contains("הכנה מראש"), "{all}");
    assert!(
        !all.contains("פנו בשל קושי") && !all.contains("סקרן"),
        "{all}"
    );
    assert!(
        !all.contains("אלון"),
        "the name is hidden before it goes out: {all}"
    );
    core.send_section(&p.approval_id.unwrap()).unwrap();

    let letter = core.letter(&case, "school").unwrap();
    assert!(!letter.is_empty());
    assert!(core.letter(&case, "parents").unwrap().is_empty());
    // The letter never enters the report.
    assert!(core
        .case_detail(&case)
        .unwrap()
        .sections
        .iter()
        .all(|s| !s.key.starts_with("letter_")));
    assert!(matches!(
        core.export_letter(&case, "school", None),
        Err(CoreError::Refused(_))
    ));
    for para in &letter {
        core.approve_paragraph(&case, &para.id).unwrap();
    }
    let doc = read_docx_text(&core.export_letter(&case, "school", None).unwrap());
    assert!(
        doc.contains("מכתב לצוות החינוכי") && doc.contains("שם הילד"),
        "{doc}"
    );
    assert!(!doc.contains("סיבת הפניה"));
}

#[test]
fn locked_pdf_is_recognised_as_the_original() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "referral", "ההורים של אלון פנו בשל קושי במעברים.")
        .unwrap();
    let pdf = match core.export_pdf(&case, None) {
        Ok(pdf) => pdf,
        // No Hebrew font on this computer (a bare build machine): nothing to check.
        Err(CoreError::Refused(m)) if m.contains("גופן") => return,
        Err(e) => panic!("{e:?}"),
    };
    assert!(pdf.starts_with(b"%PDF-2.0"));
    assert!(!String::from_utf8_lossy(&pdf).contains("אלון"));
    assert!(core.check_original(&pdf).unwrap().is_some());
    let mut changed = pdf.clone();
    let last = changed.len() - 20;
    changed[last] ^= 1;
    assert!(core.check_original(&changed).unwrap().is_none());
    assert!(matches!(
        core.export_pdf(&case, Some("abc-אבג-12345")),
        Err(CoreError::Refused(_))
    ));
}

/// A word the program kept with the case by itself, refused by the gate: it gets "להחזיר" on
/// the card instead of leaving her with only "חזרה לעריכה". A name she typed stays refused.
#[test]
fn a_kept_word_the_gate_refuses_can_be_restored_from_the_card() {
    let ident = |value: &str, tag: &str, source: IdentitySource| dv_domain::Identity {
        id: tag.into(),
        case_id: "c1".into(),
        role: Role::Brother,
        tag: tag.into(),
        value: value.into(),
        aliases: vec![],
        source,
        reason: "אחרי 'אחות'".into(),
    };
    let data = PrivacyData {
        case_id: "c1".into(),
        identities: vec![
            ident("אח", "[אח_1]", IdentitySource::Auto),
            ident("יואב", "[אח_2]", IdentitySource::Manual),
        ],
        practitioner: vec![],
        allow: HashSet::new(),
        is_name: HashSet::new(),
        past: HashSet::new(),
        masks: dv_privacy::MaskBook::default(),
    };
    let reason = |detail: &str| dv_privacy::BlockReason {
        code: "identity".into(),
        message: "נמצא שם מוצהר בטקסט היוצא".into(),
        detail: Some(detail.into()),
    };
    let offered = refused_auto_names(&data, &[reason("אח"), reason("יואב")], &|_| {
        "2026-1".to_owned()
    });
    assert_eq!(offered.len(), 1, "{offered:?}");
    assert_eq!(
        (offered[0].token.as_str(), offered[0].tag.as_str()),
        ("אח", "[אח_1]")
    );
}

/// A word kept as a name in another case ("אח" from case 2) blocked every case: the card
/// offers "להחזיר", which takes it out of that case's names. A name typed there stays, and
/// the red line says which case it is in.
#[test]
fn a_word_kept_in_another_case_is_offered_and_restored_there() {
    let (_dir, mut core, case1) = setup(None);
    let case2 = core
        .create_case(
            CaseMeta {
                code: "TEST-0099".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Brother,
                value: "גלעד".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.vault_mut()
        .unwrap()
        .add_found_names(
            &case2,
            &[dv_domain::FoundName {
                value: "אח".into(),
                role: Role::Brother,
                source: IdentitySource::Auto,
                reason: "אחרי 'אחות'".into(),
            }],
        )
        .unwrap();
    let data = core.privacy_data(&case1).unwrap();
    let reason = |detail: &str| dv_privacy::BlockReason {
        code: "identity".into(),
        message: "נמצא שם מוצהר בטקסט היוצא".into(),
        detail: Some(detail.into()),
    };
    let offered = refused_auto_names(&data, &[reason("אח")], &|_| "TEST-0099".to_owned());
    assert_eq!(offered.len(), 1, "{offered:?}");
    assert_eq!(offered[0].tag, OTHER_CASE_TAG);
    assert!(offered[0].reason.contains("TEST-0099"));
    let mut reasons = vec![reason("גלעד")];
    locate_refused_names(&data, &mut reasons, &|_| "TEST-0099".to_owned());
    assert!(
        reasons[0].message.contains("TEST-0099"),
        "{}",
        reasons[0].message
    );

    core.restore_auto_hidden(&case1, "אח", OTHER_CASE_TAG)
        .unwrap();
    let left: Vec<String> = core
        .vault_ref()
        .unwrap()
        .identities(&case2)
        .unwrap()
        .into_iter()
        .map(|i| i.value)
        .collect();
    assert_eq!(
        left,
        vec!["גלעד".to_owned()],
        "only the kept word left case 2"
    );
}

#[test]
fn a_deleted_material_comes_back_whole_until_the_lock() {
    let (_dir, mut core, case) = setup(None);
    let input = core
        .add_input(
            &case,
            InputKind::SessionNote,
            "מפגש ראשון",
            "אלון בנה מגדל.",
        )
        .unwrap();
    core.vault_mut()
        .unwrap()
        .set_input_data(&case, &input.id, "{\"a\":1}")
        .unwrap();
    core.set_input_sections(&case, &input.id, &["cognitive".into()])
        .unwrap();
    let routing = core
        .vault_mut()
        .unwrap()
        .input_routing(&case, &input.id)
        .unwrap();
    let before = core.vault_mut().unwrap().inputs(&case).unwrap();

    core.delete_input(&case, &input.id).unwrap();
    assert!(core
        .vault_mut()
        .unwrap()
        .inputs(&case)
        .unwrap()
        .iter()
        .all(|i| i.id != input.id));
    // Only this material, in this case.
    assert!(core.restore_input(&case, "other").is_err());
    core.restore_input(&case, &input.id).unwrap();
    assert_eq!(core.vault_mut().unwrap().inputs(&case).unwrap(), before);
    assert_eq!(
        core.vault_mut()
            .unwrap()
            .input_routing(&case, &input.id)
            .unwrap(),
        routing
    );
    assert_eq!(
        core.vault_mut()
            .unwrap()
            .input_data(&case, &input.id)
            .unwrap()
            .as_deref(),
        Some("{\"a\":1}")
    );
    // Once.
    assert!(core.restore_input(&case, &input.id).is_err());

    // A lock forgets it: the deletion stands.
    core.delete_input(&case, &input.id).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.restore_input(&case, &input.id).is_err());
    assert!(core
        .vault_mut()
        .unwrap()
        .inputs(&case)
        .unwrap()
        .iter()
        .all(|i| i.id != input.id));
}
