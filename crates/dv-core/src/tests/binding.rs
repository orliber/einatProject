//! D-076: the password opens the vault only on the computer it is bound to, with nothing new
//! for her to do. "This computer" is a fake seal holding one random key per computer (no
//! Windows in tests); "another computer" is another seal. Fabricated data only.

use std::path::Path;

use dv_vault::crypto::Key32;

use super::*;

/// One computer's sealed key, kept in memory; a computer that "lost" its key has none. The
/// second is a key held aside until it is promoted.
#[derive(Default)]
struct FakeSeal(Mutex<Option<[u8; 32]>>, Mutex<Option<[u8; 32]>>);

impl ComputerSeal for FakeSeal {
    fn available(&self) -> bool {
        true
    }
    fn load(&self, _dir: &Path) -> Result<Key32, VaultError> {
        self.0
            .lock()
            .unwrap()
            .map(Key32::from_bytes)
            .ok_or(VaultError::NotFound)
    }
    fn create(&self, _dir: &Path) -> Result<Key32, VaultError> {
        let key = Key32::random()?;
        *self.0.lock().unwrap() = Some(*key.as_bytes());
        Ok(key)
    }
    fn create_next(&self, _dir: &Path) -> Result<Key32, VaultError> {
        let key = Key32::random()?;
        *self.1.lock().unwrap() = Some(*key.as_bytes());
        Ok(key)
    }
    fn load_next(&self, _dir: &Path) -> Result<Key32, VaultError> {
        self.1
            .lock()
            .unwrap()
            .map(Key32::from_bytes)
            .ok_or(VaultError::NotFound)
    }
    fn promote_next(&self, _dir: &Path) -> Result<(), VaultError> {
        let next = self.1.lock().unwrap().take().ok_or(VaultError::NotFound)?;
        *self.0.lock().unwrap() = Some(next);
        Ok(())
    }
}

fn on(dir: &Path, seal: &Arc<FakeSeal>) -> Core {
    Core::for_tests(dir, None).with_computer_seal(Arc::clone(seal) as Arc<dyn ComputerSeal>)
}

fn state(core: &mut Core) -> String {
    core.security_status()
        .items
        .into_iter()
        .find(|i| i.key == "computer_bound")
        .unwrap()
        .state
}

fn copy_folder(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn a_new_vault_is_bound_to_this_computer_and_she_types_only_her_password() {
    let dir = tempfile::tempdir().unwrap();
    let here = Arc::new(FakeSeal::default());
    let mut core = on(dir.path(), &here);
    let kit = core.create_vault(PASSWORD).unwrap().recovery_key;
    assert_eq!(state(&mut core), "ok");
    core.lock();
    assert!(core.unlock(PASSWORD).unwrap().unlocked);

    // The same folder on another computer: the password does not open it, and the message
    // says what does. A wrong guess there is not even tried.
    let copy = tempfile::tempdir().unwrap();
    core.lock();
    copy_folder(dir.path(), copy.path());
    let elsewhere = Arc::new(FakeSeal::default());
    let mut there = on(copy.path(), &elsewhere);
    let e = there.unlock(PASSWORD).unwrap_err();
    assert!(matches!(e, CoreError::Vault(VaultError::OtherComputer)));
    let ui = e.to_ui();
    assert_eq!(ui.code, "other_computer");
    assert!(ui.message.contains("ערכת השחזור"));
    assert!(
        there.unlock(PASSWORD).is_err() && there.check_backoff().is_ok(),
        "not a guess: no waiting time"
    );

    // The kit opens it there, and a new password chosen there binds to that computer.
    there.unlock_with_recovery(&kit).unwrap();
    // Bound, but to the first computer: the page says the password does not open it here yet
    // (review of stage 5, U2).
    assert_eq!(state(&mut there), "warn");
    there.change_password(&kit, true, PASSWORD).unwrap();
    assert_eq!(state(&mut there), "ok");
    there.lock();
    assert!(there.unlock(PASSWORD).unwrap().unlocked);
    // ...and the first computer's folder still opens with the password there.
    assert!(core.unlock(PASSWORD).unwrap().unlocked);
}

#[test]
fn a_vault_from_before_is_bound_at_its_next_opening_with_no_screen() {
    let dir = tempfile::tempdir().unwrap();
    // Made on a computer with nothing to seal to (the program before this round).
    let mut before = Core::for_tests(dir.path(), None);
    before.create_vault(PASSWORD).unwrap();
    assert_ne!(state(&mut before), "ok");
    before.lock();
    drop(before);

    let here = Arc::new(FakeSeal::default());
    let mut core = on(dir.path(), &here);
    let opened = core.unlock(PASSWORD).unwrap();
    assert!(opened.unlocked);
    assert_eq!(state(&mut core), "ok");
    // Said once, with what opens it after a reinstall of Windows (review of stage 5, S1).
    let said = opened.since_last.unwrap();
    assert!(
        said.contains("רק במחשב הזה") && said.contains("ערכת השחזור"),
        "{said}"
    );
    core.lock();
    assert!(core.unlock(PASSWORD).unwrap().since_last.is_none());
    let log = core.activity(None).unwrap();
    assert!(
        log.entries.iter().any(|e| e.text.contains("נקשרה למחשב")),
        "the log says so"
    );
    core.lock();
    let elsewhere = Arc::new(FakeSeal::default());
    assert!(matches!(
        on(dir.path(), &elsewhere).unlock(PASSWORD),
        Err(CoreError::Vault(
            VaultError::OtherComputer | VaultError::WrongSecret
        ))
    ));
}

#[test]
fn a_backup_restores_on_a_new_computer_with_the_password_and_binds_there() {
    let dir = tempfile::tempdir().unwrap();
    let here = Arc::new(FakeSeal::default());
    let mut core = on(dir.path(), &here);
    core.create_vault(PASSWORD).unwrap();
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b")).unwrap();
    let bytes = std::fs::read(&done.path).unwrap();
    // The drill on this computer opens it with the password alone.
    core.stage_backup(bytes.clone(), "b.vaultbak".into())
        .unwrap();
    assert!(core.check_staged_backup(PASSWORD).unwrap().integrity_ok);

    let fresh = tempfile::tempdir().unwrap();
    let new_computer = Arc::new(FakeSeal::default());
    let mut other = on(fresh.path(), &new_computer);
    other.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    other.restore_staged_backup(Some(PASSWORD), None).unwrap();
    assert_eq!(state(&mut other), "ok", "bound to the new computer at once");
    other.lock();
    assert!(other.unlock(PASSWORD).unwrap().unlocked);
    other.lock();
    // The restored folder does not open with the first computer's key.
    assert!(on(fresh.path(), &here).unlock(PASSWORD).is_err());
}

#[test]
fn actions_that_ask_for_the_password_take_it_on_the_bound_computer() {
    let (_dir, mut core, case) = {
        let dir = tempfile::tempdir().unwrap();
        let here = Arc::new(FakeSeal::default());
        let mut core = on(dir.path(), &here);
        core.create_vault(PASSWORD).unwrap();
        let case = core
            .create_case(
                CaseMeta {
                    code: "TEST-0003".into(),
                    ..CaseMeta::default()
                },
                vec![],
            )
            .unwrap();
        (dir, core, case)
    };
    core.delete_case(&case).unwrap();
    core.purge_case(&case, PASSWORD).unwrap();
    assert!(core.list_trash().unwrap().is_empty());
}

#[test]
fn the_printed_kit_is_checked_in_settings_and_the_security_page_says_when() {
    let dir = tempfile::tempdir().unwrap();
    let here = Arc::new(FakeSeal::default());
    let mut core = on(dir.path(), &here);
    let kit = core.create_vault(PASSWORD).unwrap().recovery_key;
    let kit_item = |core: &mut Core| {
        core.security_status()
            .items
            .into_iter()
            .find(|i| i.key == "recovery_kit")
            .unwrap()
    };
    let before = kit_item(&mut core);
    assert_eq!((before.state.as_str(), before.detail), ("warn", None));
    assert!(!core.confirm_recovery_key("AAAA-BBBB-CCCC").unwrap());
    assert_eq!(kit_item(&mut core).state, "warn");
    assert!(core.confirm_recovery_key(&kit).unwrap());
    let after = kit_item(&mut core);
    assert_eq!(
        (after.state.as_str(), after.detail.as_deref()),
        ("ok", Some("0"))
    );
    // An internal date, not a line of its own in her log.
    let log = core.activity(None).unwrap();
    assert!(log.entries.iter().all(|e| e.text != "הגדרה עודכנה"));
}

/// A computer whose key held aside is never promoted (the program closed right after the new
/// slot was written).
struct NoPromote(Arc<FakeSeal>);

impl ComputerSeal for NoPromote {
    fn available(&self) -> bool {
        true
    }
    fn load(&self, dir: &Path) -> Result<Key32, VaultError> {
        self.0.load(dir)
    }
    fn create(&self, dir: &Path) -> Result<Key32, VaultError> {
        self.0.create(dir)
    }
    fn create_next(&self, dir: &Path) -> Result<Key32, VaultError> {
        self.0.create_next(dir)
    }
    fn load_next(&self, dir: &Path) -> Result<Key32, VaultError> {
        self.0.load_next(dir)
    }
    fn promote_next(&self, _dir: &Path) -> Result<(), VaultError> {
        Err(VaultError::NotFound)
    }
}

#[test]
fn a_new_key_waits_aside_until_its_slot_is_written_and_still_opens_after_a_crash() {
    // Review of stage 5, S5: the key in use is never replaced before the new slot is written.
    let dir = tempfile::tempdir().unwrap();
    let first = Arc::new(FakeSeal::default());
    let mut core = on(dir.path(), &first);
    let kit = core.create_vault(PASSWORD).unwrap().recovery_key;
    core.lock();
    // On another computer the kit opens it; the new password's key is held aside there, and
    // the program "closes" before it is promoted.
    let copy = tempfile::tempdir().unwrap();
    copy_folder(dir.path(), copy.path());
    let second = Arc::new(FakeSeal::default());
    let mut there = Core::for_tests(copy.path(), None)
        .with_computer_seal(Arc::new(NoPromote(Arc::clone(&second))) as Arc<dyn ComputerSeal>);
    there.unlock_with_recovery(&kit).unwrap();
    there.change_password(&kit, true, PASSWORD).unwrap();
    assert!(second.load(copy.path()).is_err(), "nothing replaced yet");
    there.lock();
    drop(there);
    // The next opening finds the key held aside, opens, and keeps it as the one in use.
    let mut again = on(copy.path(), &second);
    assert!(again.unlock(PASSWORD).unwrap().unlocked);
    assert!(second.load(copy.path()).is_ok() && second.load_next(copy.path()).is_err());
    again.lock();
    assert!(again.unlock(PASSWORD).unwrap().unlocked);
}

#[test]
fn a_header_that_cannot_be_read_never_makes_a_new_key() {
    // Review of stage 5, S4: unreadable counts as bound, so the key here is only loaded.
    let dir = tempfile::tempdir().unwrap();
    assert!(crate::binding::bound_on_disk(dir.path()));
    let seal = FakeSeal::default();
    assert!(
        crate::binding::key_here(&seal, dir.path(), crate::binding::bound_on_disk(dir.path()))
            .is_none()
    );
    assert!(seal.load(dir.path()).is_err(), "no key was made");
}
