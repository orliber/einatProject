//! L3 and L4: who opened what, and what happened while she was away. Fabricated data only.

use super::*;

fn opened(core: &mut Core) -> usize {
    core.activity(None)
        .unwrap()
        .entries
        .iter()
        .filter(|e| e.event == "case_opened")
        .count()
}

#[test]
fn each_case_she_opens_is_in_the_log_once_a_session() {
    let (_dir, mut core, case) = setup(None);
    let before = opened(&mut core);
    core.case_detail(&case).unwrap();
    core.case_detail(&case).unwrap();
    assert_eq!(opened(&mut core), before + 1);
    let entry = core
        .activity(None)
        .unwrap()
        .entries
        .into_iter()
        .find(|e| e.event == "case_opened")
        .unwrap();
    assert_eq!(entry.text, "פתחת את התיק");
    assert!(entry.case.is_some(), "named on this computer");
    core.lock();
    core.unlock(PASSWORD).unwrap();
    core.case_detail(&case).unwrap();
    assert_eq!(opened(&mut core), before + 2);
}

#[test]
fn wrong_passwords_before_a_restart_are_told_at_the_next_opening_and_only_then() {
    let (dir, mut core, _case) = setup(None);
    core.lock();
    drop(core);
    // Two tries in two runs of the program (each run starts without the waiting time).
    for _ in 0..2 {
        let mut run = Core::for_tests(dir.path(), None);
        assert!(run.unlock("סיסמה לא נכונה בכלל").is_err());
    }
    let mut core = Core::for_tests(dir.path(), None);
    let status = core.unlock(PASSWORD).unwrap();
    let said = status.since_last.unwrap();
    assert!(said.contains("2 ניסיונות כניסה שנכשלו"), "{said}");
    let log = core.activity(None).unwrap();
    assert!(log
        .entries
        .iter()
        .any(|e| e.text.contains("2 ניסיונות כניסה שנכשלו")));
    // The next opening has nothing to say.
    core.lock();
    assert!(core.status().since_last.is_none());
    assert!(core.unlock(PASSWORD).unwrap().since_last.is_none());
}

#[test]
fn her_own_typo_just_before_she_gets_in_is_not_reported_as_someone_else() {
    // Review of stage 5, U5: one mistyped password, then the right one, in the same sitting.
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    core.lock();
    assert!(core.unlock("סיסמה לא נכונה בכלל").is_err());
    core.not_before = None;
    let status = core.unlock(PASSWORD).unwrap();
    assert!(status.since_last.is_none(), "{:?}", status.since_last);
}
