//! Forgot the password: sign in with Google (D-041).
//!
//! The Google slot opens only with two keys together: one in her own Google Drive (the app's
//! hidden folder) and one sealed to her Windows account on this computer. So a broken-into
//! Google account, even with a stolen backup, reads nothing, and the developer holds nothing.
//!
//! The browser sign-in can take minutes, so it never runs under the core's lock. Each action
//! is split: a quick check under the lock, the sign-in and Drive calls outside it (in the app
//! shell, through [`sign_in`]), and the vault change under the lock again. The Google token
//! lives in memory for one action and is revoked at its end.

use std::sync::atomic::{AtomicBool, Ordering};

use dv_egress::google::{self, ClientConfig, GoogleError, Http};
pub use dv_egress::google::{GoogleHttp, Session, AUTH_URL};
use dv_vault::crypto::Key32;
use dv_vault::recovery::{drive_key_text, google_account_tag, parse_drive_key};
use dv_vault::{computer_key, Secret, Vault, VaultError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::{Argon2Params, Core, CoreError};

/// What the lock screen and the settings may offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GoogleStatus {
    /// This version was built with a Google registration, on Windows.
    pub available: bool,
    /// Turned on, and the half kept on this computer is here: "forgot password" works here.
    pub on: bool,
    /// The vault has a Google slot but this computer lacks its half (a restored backup on a
    /// new computer): turning it on again is needed.
    pub needs_setup_here: bool,
}

/// The half of the Google slot kept on this computer. Windows DPAPI in the program; a fake
/// in tests (no Windows, no real Google in tests).
pub trait ComputerKeys {
    fn available(&self) -> bool;
    fn exists(&self, dir: &std::path::Path) -> bool;
    fn load(&self, dir: &std::path::Path) -> Result<Key32, VaultError>;
    fn load_or_create(&self, dir: &std::path::Path) -> Result<Key32, VaultError>;
    fn remove(&self, dir: &std::path::Path);
}

/// The real one: sealed to her Windows account.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsAccount;

impl ComputerKeys for WindowsAccount {
    fn available(&self) -> bool {
        computer_key::available()
    }
    fn exists(&self, dir: &std::path::Path) -> bool {
        computer_key::exists(dir)
    }
    fn load(&self, dir: &std::path::Path) -> Result<Key32, VaultError> {
        computer_key::load(dir)
    }
    fn load_or_create(&self, dir: &std::path::Path) -> Result<Key32, VaultError> {
        // Keep the key that is there when it still opens: a failed switch never strands
        // the slot that already works.
        computer_key::load(dir).or_else(|_| computer_key::create(dir))
    }
    fn remove(&self, dir: &std::path::Path) {
        computer_key::remove(dir);
    }
}

/// One sign-in at a time, and a way to stop it from the screen.
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Stop a sign-in that is waiting for the browser.
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn refused(text: &str) -> CoreError {
    CoreError::Refused(text.to_owned())
}

/// What she reads when Google fails. Never the server's own words.
fn from_google(e: &GoogleError) -> CoreError {
    refused(match e {
        GoogleError::NotConfigured => "הכניסה עם גוגל עוד לא הוגדרה בגרסה הזו.",
        GoogleError::Cancelled => "הכניסה לגוגל בוטלה. אפשר לנסות שוב.",
        GoogleError::TimedOut => "הכניסה לגוגל לא הסתיימה בזמן. אפשר לנסות שוב.",
        GoogleError::NoKey => "בחשבון הגוגל הזה אין מפתח לכספת הזו. כדאי לבדוק שנבחר החשבון הנכון.",
        GoogleError::Offline => "אין חיבור לגוגל. צריך אינטרנט כדי להשתמש בכניסה עם גוגל.",
        GoogleError::Tls => "החיבור המאובטח לגוגל נכשל. ייתכן שמשהו במחשב מיירט תעבורה מוצפנת.",
        GoogleError::Protocol(_) => "גוגל ענתה תשובה לא צפויה. אפשר לנסות שוב מאוחר יותר.",
    })
}

/// The app's Google registration, or a clear refusal.
pub fn config() -> Result<ClientConfig, CoreError> {
    google::configured().ok_or_else(|| from_google(&GoogleError::NotConfigured))
}

/// Sign in in her browser and come back with a session. Runs outside the core's lock.
/// `open` opens the address in her default browser.
pub fn sign_in(
    open: &dyn Fn(&str) -> Result<(), CoreError>,
) -> Result<(GoogleHttp, Session), CoreError> {
    let cfg = config()?;
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(refused(
            "כבר פתוח חלון כניסה לגוגל. אפשר לסיים אותו או לבטל.",
        ));
    }
    CANCEL.store(false, Ordering::SeqCst);
    let result = (|| {
        let http = GoogleHttp::new().map_err(|e| from_google(&e))?;
        let pending = google::begin(&cfg).map_err(|e| from_google(&e))?;
        open(&pending.url)?;
        let code = google::wait(&pending, &CANCEL, google::SIGN_IN_TIMEOUT)
            .map_err(|e| from_google(&e))?;
        let session = google::finish(&http, &cfg, &pending, &code).map_err(|e| from_google(&e))?;
        Ok((http, session))
    })();
    BUSY.store(false, Ordering::SeqCst);
    result
}

/// The Drive side of turning it on: a new random key in the hidden folder. Returns the key
/// and the ids of older key files for this vault (deleted once the new slot is written).
pub fn put_new_key(
    http: &dyn Http,
    session: &Session,
    vault_id: &str,
) -> Result<(Key32, String, Vec<String>), CoreError> {
    let older = google::find_keys(http, session, vault_id).map_err(|e| from_google(&e))?;
    let key = Key32::random()?;
    let id = google::put_key(http, session, vault_id, &drive_key_text(&key))
        .map_err(|e| from_google(&e))?;
    Ok((key, id, older))
}

/// The Drive side of "forgot password": this vault's keys from the hidden folder.
pub fn read_keys(
    http: &dyn Http,
    session: &Session,
    vault_id: &str,
) -> Result<Vec<Key32>, CoreError> {
    let ids = google::find_keys(http, session, vault_id).map_err(|e| from_google(&e))?;
    let mut keys = Vec::new();
    for id in ids {
        let text: Zeroizing<String> =
            google::read_key(http, session, &id).map_err(|e| from_google(&e))?;
        if let Ok(k) = parse_drive_key(&text) {
            keys.push(k);
        }
    }
    if keys.is_empty() {
        return Err(from_google(&GoogleError::NoKey));
    }
    Ok(keys)
}

/// Best effort: delete key files that no slot uses any more, then end the session.
pub fn tidy_up(http: &dyn Http, session: Session, stale: &[String]) {
    for id in stale {
        let _ = google::delete_key(http, &session, id);
    }
    google::revoke(http, session);
}

impl Core {
    /// What to offer. Readable while locked (from the header only).
    pub fn google_status(&self, keys: &dyn ComputerKeys) -> GoogleStatus {
        let available = keys.available() && google::configured().is_some();
        let slot = Vault::google_slot(&self.dir)
            .map(|(_, tag)| tag.is_some())
            .unwrap_or(false);
        let here = keys.exists(&self.dir);
        GoogleStatus {
            available,
            on: available && slot && here,
            needs_setup_here: available && slot && !here,
        }
    }

    /// Turning it on, step 1 (under the lock): check the password before her browser opens,
    /// and return the vault id the Drive file is named after.
    pub fn google_turn_on_check(&mut self, password: &str) -> Result<String, CoreError> {
        self.check_backoff()?;
        let vault = self.vault_ref()?;
        let id = vault.vault_id().to_owned();
        if let Err(e) = Vault::verify_password(&self.dir, password) {
            self.count_failure(&e);
            return Err(e.into());
        }
        Ok(id)
    }

    /// Turning it on, step 3 (under the lock, after the key is in her Drive): write the slot.
    pub fn google_turn_on_finish(
        &mut self,
        keys: &dyn ComputerKeys,
        password: &str,
        drive: &Key32,
        google_account_id: &str,
    ) -> Result<(), CoreError> {
        self.check_backoff()?;
        let dir = self.dir.clone();
        let computer = keys.load_or_create(&dir)?;
        let vault = self.vault_mut()?;
        let tag = google_account_tag(vault.vault_id(), google_account_id);
        let result = vault.set_google_slot(Secret::Password(password), drive, &computer, &tag);
        if let Err(e) = result {
            self.count_failure(&e);
            return Err(e.into());
        }
        Ok(())
    }

    /// Turn it off. Needs no Google: without the half on this computer the key in her Drive
    /// opens nothing, not even old backups (they never carry that half).
    pub fn google_turn_off(
        &mut self,
        keys: &dyn ComputerKeys,
        password: &str,
    ) -> Result<(), CoreError> {
        self.check_backoff()?;
        let dir = self.dir.clone();
        let result = self
            .vault_mut()?
            .remove_google_slot(Secret::Password(password));
        if let Err(e) = result {
            self.count_failure(&e);
            return Err(e.into());
        }
        keys.remove(&dir);
        Ok(())
    }

    /// "Forgot password", step 1 (under the lock, while locked): the new password is
    /// acceptable, and this computer can open the Google slot. Returns the vault id and the
    /// tag of the connected account.
    pub fn google_recover_check(
        &mut self,
        keys: &dyn ComputerKeys,
        new_password: &str,
    ) -> Result<(String, String), CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        dv_vault::password::check_policy(new_password)
            .map_err(|p| CoreError::Vault(VaultError::Policy(p)))?;
        let (vault_id, tag) = Vault::google_slot(&self.dir)?;
        match tag {
            Some(tag) if keys.exists(&self.dir) => Ok((vault_id, tag)),
            _ => Err(refused(
                "הכניסה עם גוגל לא מופעלת במחשב הזה. אפשר להיכנס עם ערכת השחזור המודפסת.",
            )),
        }
    }

    /// "Forgot password", step 3 (under the lock): open with the keys read from her Drive
    /// and the half on this computer, then set the new password at once.
    pub fn google_recover_finish(
        &mut self,
        keys: &dyn ComputerKeys,
        drive_keys: &[Key32],
        new_password: &str,
    ) -> Result<crate::AppStatus, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        let computer = keys.load(&self.dir)?;
        let mut opened = Err(VaultError::WrongSecret);
        let mut used = None;
        for drive in drive_keys {
            opened = Vault::unlock_with_google(&self.dir, drive, &computer);
            if opened.is_ok() {
                used = Some(drive);
                break;
            }
        }
        self.after_unlock(opened, true)?;
        let Some(drive) = used else {
            return Err(CoreError::Internal("opened without a key".to_owned()));
        };
        let params = self.argon.unwrap_or_else(Argon2Params::calibrate);
        self.vault_mut()?.rekey_password(
            Secret::Google {
                drive,
                computer: &computer,
            },
            new_password,
            params,
        )?;
        self.mark_secret_changed()?;
        Ok(self.status())
    }
}

/// The account she signed in with is the one the vault knows.
#[must_use]
pub fn same_account(vault_id: &str, tag: &str, session: &Session) -> bool {
    google_account_tag(vault_id, &session.account_id) == tag
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;

    const PASSWORD: &str = "סוס ירוק רץ בשדה 2026";
    const NEW_PASSWORD: &str = "ציפור כתומה שרה בבוקר";

    /// The half on "this computer", kept in memory instead of Windows DPAPI.
    #[derive(Default)]
    struct FakeComputer(RefCell<Option<[u8; 32]>>);

    impl ComputerKeys for FakeComputer {
        fn available(&self) -> bool {
            true
        }
        fn exists(&self, _: &Path) -> bool {
            self.0.borrow().is_some()
        }
        fn load(&self, _: &Path) -> Result<Key32, VaultError> {
            self.0
                .borrow()
                .map(Key32::from_bytes)
                .ok_or(VaultError::NotFound)
        }
        fn load_or_create(&self, dir: &Path) -> Result<Key32, VaultError> {
            if let Ok(k) = self.load(dir) {
                return Ok(k);
            }
            let k = Key32::random()?;
            *self.0.borrow_mut() = Some(*k.as_bytes());
            Ok(k)
        }
        fn remove(&self, _: &Path) {
            *self.0.borrow_mut() = None;
        }
    }

    #[test]
    fn forgot_password_with_google_opens_and_sets_a_new_password() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = Core::for_tests(dir.path(), None);
        core.create_vault(PASSWORD).unwrap();
        let computer = FakeComputer::default();

        // Wrong password: refused before any browser opens.
        assert!(core.google_turn_on_check("לא הסיסמה הנכונה בכלל").is_err());
        core.not_before = None;
        let vault_id = core.google_turn_on_check(PASSWORD).unwrap();
        let drive = Key32::random().unwrap();
        core.google_turn_on_finish(&computer, PASSWORD, &drive, "acct-1")
            .unwrap();
        core.lock();

        // Locked: the tag of the connected account is known, and the new password is
        // checked before the browser opens.
        assert!(core.google_recover_check(&computer, "קצר").is_err());
        let (id, tag) = core.google_recover_check(&computer, NEW_PASSWORD).unwrap();
        assert_eq!(id, vault_id);
        assert_eq!(tag, google_account_tag(&vault_id, "acct-1"));

        // A key that is not this vault's is a failed attempt.
        assert!(core
            .google_recover_finish(&computer, &[Key32::random().unwrap()], NEW_PASSWORD)
            .is_err());
        assert!(!core.status().unlocked);
        core.not_before = None;

        let status = core
            .google_recover_finish(
                &computer,
                &[
                    Key32::random().unwrap(),
                    Key32::from_bytes(*drive.as_bytes()),
                ],
                NEW_PASSWORD,
            )
            .unwrap();
        assert!(status.unlocked);
        core.lock();
        assert!(core.unlock(PASSWORD).is_err());
        core.not_before = None;
        assert!(core.unlock(NEW_PASSWORD).unwrap().unlocked);

        // Off: the half on this computer is gone, and "forgot password" is not offered.
        core.google_turn_off(&computer, NEW_PASSWORD).unwrap();
        assert!(!computer.exists(dir.path()));
        core.lock();
        assert!(core.google_recover_check(&computer, NEW_PASSWORD).is_err());
    }

    #[test]
    fn without_the_half_on_this_computer_google_alone_is_not_offered() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = Core::for_tests(dir.path(), None);
        core.create_vault(PASSWORD).unwrap();
        let computer = FakeComputer::default();
        let drive = Key32::random().unwrap();
        core.google_turn_on_finish(&computer, PASSWORD, &drive, "acct-1")
            .unwrap();
        core.lock();
        // Another computer (a restored backup): the header has the slot, the half is missing.
        let elsewhere = FakeComputer::default();
        assert!(core.google_recover_check(&elsewhere, NEW_PASSWORD).is_err());
        assert!(core
            .google_recover_finish(&elsewhere, &[drive], NEW_PASSWORD)
            .is_err());
    }
}
