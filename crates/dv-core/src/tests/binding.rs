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

/// A computer with nothing to keep a key in.
struct Nowhere;

impl ComputerSeal for Nowhere {
    fn available(&self) -> bool {
        false
    }
    fn load(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::NotFound)
    }
    fn create(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::NotFound)
    }
    fn create_next(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::NotFound)
    }
    fn load_next(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::NotFound)
    }
    fn promote_next(&self, _dir: &Path) -> Result<(), VaultError> {
        Err(VaultError::NotFound)
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
    // What opens it here now: the newer version, or the backup with the password (D-103).
    assert!(ui.message.contains("הגיבוי") && !ui.message.contains("ערכת השחזור"));
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
    // Made on a computer with nothing to seal to (the program before this round): stated, not
    // left to the computer the test runs on (a Mac's Keychain would bind it at once).
    let mut before = Core::for_tests(dir.path(), None)
        .with_computer_seal(Arc::new(Nowhere) as Arc<dyn ComputerSeal>);
    before.create_vault(PASSWORD).unwrap();
    assert_ne!(state(&mut before), "ok");
    before.lock();
    drop(before);

    let here = Arc::new(FakeSeal::default());
    let mut core = on(dir.path(), &here);
    let opened = core.unlock(PASSWORD).unwrap();
    assert!(opened.unlocked);
    assert_eq!(state(&mut core), "ok");
    // Said once, with what opens it after a reinstall of Windows (review of stage 5, S1):
    // the backup with the password, no longer the kit (D-103).
    let said = opened.since_last.unwrap();
    assert!(
        said.contains("רק במחשב הזה") && said.contains("גיבוי"),
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
fn a_retired_kit_stops_opening_and_the_security_page_asks_for_google_instead() {
    let dir = tempfile::tempdir().unwrap();
    let here = Arc::new(FakeSeal::default());
    let mut core = on(dir.path(), &here);
    let kit = core.create_vault(PASSWORD).unwrap().recovery_key;
    // No kit on the page any more (D-103): a forgotten password is Google's.
    let items = core.security_status().items;
    assert!(items.iter().all(|i| i.key != "recovery_kit"));
    let forgot = items.iter().find(|i| i.key == "forgot_password").unwrap();
    assert!(["warn", "info"].contains(&forgot.state.as_str()));
    assert!(core.confirm_recovery_key(&kit).unwrap());
    // Retiring: the kit she had no longer opens the vault, and nothing new is handed out.
    let job = core.begin_new_recovery_kit(PASSWORD, false).unwrap();
    core.finish_retire_recovery_kit(job.run()).unwrap();
    assert!(!core.confirm_recovery_key(&kit).unwrap());
    let log = core.activity(None).unwrap();
    assert!(log.entries.iter().any(|e| e.text == "ערכת השחזור הוחלפה"));
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

/// A Windows that answers late `times` times, then as `FakeSeal` (a cold start).
struct Slow(Arc<FakeSeal>, Mutex<u32>);

impl ComputerSeal for Slow {
    fn available(&self) -> bool {
        true
    }
    fn load(&self, dir: &Path) -> Result<Key32, VaultError> {
        let mut left = self.1.lock().unwrap();
        if *left > 0 {
            *left -= 1;
            return Err(VaultError::Busy);
        }
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
    fn promote_next(&self, dir: &Path) -> Result<(), VaultError> {
        self.0.promote_next(dir)
    }
}

#[test]
fn a_slow_windows_says_try_again_and_never_another_computer() {
    // A first PowerShell after the computer starts took over 20 s on a CI runner: a late
    // answer is asked once more, and if it is late again the opening says to try again.
    let dir = tempfile::tempdir().unwrap();
    let here = Arc::new(FakeSeal::default());
    on(dir.path(), &here).create_vault(PASSWORD).unwrap();
    let slow = |times| {
        Core::for_tests(dir.path(), None)
            .with_computer_seal(
                Arc::new(Slow(Arc::clone(&here), Mutex::new(times))) as Arc<dyn ComputerSeal>
            )
    };
    assert!(
        slow(1).unlock(PASSWORD).unwrap().unlocked,
        "late once: asked again"
    );
    let mut core = slow(2);
    let e = core.unlock(PASSWORD).unwrap_err();
    assert!(matches!(e, CoreError::Vault(VaultError::Busy)));
    let ui = e.to_ui();
    assert_eq!(ui.code, "busy");
    assert!(ui.message.contains("לנסות שוב") && !ui.message.contains("ערכת השחזור"));
    assert!(core.check_backoff().is_ok(), "not a guess: no waiting time");
    assert!(core.unlock(PASSWORD).unwrap().unlocked);
}

#[test]
fn a_slow_windows_while_open_is_not_a_wrong_password_nor_another_computer() {
    // Review of stage 5: the checks of her password while the vault is open and the
    // security page tell a late Windows apart too.
    let dir = tempfile::tempdir().unwrap();
    let here = Arc::new(FakeSeal::default());
    on(dir.path(), &here).create_vault(PASSWORD).unwrap();
    let slow = Arc::new(Slow(Arc::clone(&here), Mutex::new(0)));
    let mut core = Core::for_tests(dir.path(), None)
        .with_computer_seal(Arc::clone(&slow) as Arc<dyn ComputerSeal>);
    core.unlock(PASSWORD).unwrap();
    *slow.1.lock().unwrap() = 1;
    let item = core
        .security_status()
        .items
        .into_iter()
        .find(|i| i.key == "computer_bound")
        .unwrap();
    assert_eq!(
        (item.state.as_str(), item.detail.as_deref()),
        ("info", Some("busy"))
    );
    *slow.1.lock().unwrap() = 1;
    let e = core.google_turn_on_check(PASSWORD).unwrap_err();
    assert!(matches!(e, CoreError::Vault(VaultError::Busy)));
    assert!(core.check_backoff().is_ok(), "not a guess: no waiting time");
    assert!(core.google_turn_on_check(PASSWORD).is_ok());
}

/// A computer that could keep a key but does not now (a locked Keychain, a dialog she
/// cancelled): every key it is asked for is `Busy`.
struct Locked;

impl ComputerSeal for Locked {
    fn available(&self) -> bool {
        true
    }
    fn load(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::Busy)
    }
    fn create(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::Busy)
    }
    fn create_next(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::Busy)
    }
    fn load_next(&self, _dir: &Path) -> Result<Key32, VaultError> {
        Err(VaultError::Busy)
    }
    fn promote_next(&self, _dir: &Path) -> Result<(), VaultError> {
        Err(VaultError::Busy)
    }
}

#[test]
fn a_key_that_cannot_be_made_here_writes_no_unbound_password() {
    // Review of #136: on a computer that can bind, a new password or a new vault is never
    // written unbound; she is told to try again and nothing changes.
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None)
        .with_computer_seal(Arc::new(Locked) as Arc<dyn ComputerSeal>);
    let e = core.create_vault(PASSWORD).unwrap_err();
    assert!(matches!(e, CoreError::Vault(VaultError::Busy)), "{e:?}");
    assert!(!Vault::exists(dir.path()), "no vault was made");

    let here = Arc::new(FakeSeal::default());
    let kit = on(dir.path(), &here)
        .create_vault(PASSWORD)
        .unwrap()
        .recovery_key;
    let copy = tempfile::tempdir().unwrap();
    copy_folder(dir.path(), copy.path());
    let mut there = Core::for_tests(copy.path(), None)
        .with_computer_seal(Arc::new(Locked) as Arc<dyn ComputerSeal>);
    there.unlock_with_recovery(&kit).unwrap();
    let e = there
        .change_password(&kit, true, "another fabricated password 7")
        .unwrap_err();
    assert!(matches!(e, CoreError::Vault(VaultError::Busy)), "{e:?}");
    assert!(
        Vault::password_is_bound(copy.path()).unwrap(),
        "the slot was not rewritten"
    );
    assert!(
        there.check_backoff().is_ok(),
        "not a guess: no waiting time"
    );
}

#[test]
fn a_slow_keychain_at_a_new_password_or_kit_says_try_again_and_makes_no_key() {
    // Review of #136: a bound slot whose key here is late twice is `Busy` for a new password
    // and for a new kit, never "another computer", and no key is made or held aside.
    let dir = tempfile::tempdir().unwrap();
    let here = Arc::new(FakeSeal::default());
    on(dir.path(), &here).create_vault(PASSWORD).unwrap();
    let slow = Arc::new(Slow(Arc::clone(&here), Mutex::new(0)));
    let mut core = Core::for_tests(dir.path(), None)
        .with_computer_seal(Arc::clone(&slow) as Arc<dyn ComputerSeal>);
    core.unlock(PASSWORD).unwrap();
    let in_use = *here.load(dir.path()).unwrap().as_bytes();

    *slow.1.lock().unwrap() = 2;
    let e = core
        .change_password(PASSWORD, false, "another fabricated password 7")
        .unwrap_err();
    assert!(matches!(e, CoreError::Vault(VaultError::Busy)), "{e:?}");
    *slow.1.lock().unwrap() = 2;
    let e = core.new_recovery_kit(PASSWORD, false).unwrap_err();
    assert!(matches!(e, CoreError::Vault(VaultError::Busy)), "{e:?}");

    assert!(here.load_next(dir.path()).is_err(), "no key held aside");
    assert_eq!(*here.load(dir.path()).unwrap().as_bytes(), in_use);
    assert!(core.check_backoff().is_ok(), "not a guess: no waiting time");
    core.change_password(PASSWORD, false, "another fabricated password 7")
        .unwrap();
    core.lock();
    assert!(
        core.unlock("another fabricated password 7")
            .unwrap()
            .unlocked
    );
}
