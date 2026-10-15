//! The password slot bound to this computer (D-076): an invisible second factor.
//!
//! The password slot's key is the stretched password together with a random key sealed to her
//! Windows account on this computer (DPAPI). A copied vault folder cannot be guessed at on
//! another computer, not even with a graphics card; on this computer she types the password
//! as before. The printed kit opens the vault anywhere, and a backup opens with the password
//! alone (it is on a drive she keeps; a documented deviation).
//!
//! Where nothing can seal a key to the account (not Windows), the slot stays as it was.

use std::path::Path;

use dv_vault::computer_key::{self, Purpose};
use dv_vault::crypto::Key32;
use dv_vault::{Secret, Vault, VaultError};

use crate::Core;

/// Said once, at the opening that bound an older vault to this computer (review of stage 5,
/// S1): from now on the kit is what opens it after Windows is reinstalled or elsewhere.
pub(crate) const JUST_BOUND: &str = "מעכשיו הסיסמה פותחת את הכספת רק במחשב הזה ובחשבון ה-Windows הזה. במחשב אחר, אחרי התקנה מחדש של Windows או אחרי איפוס החשבון, נכנסים עם ערכת השחזור, ולכן כדאי לוודא שהיא אצלך: הגדרות ← סיסמה ואבטחה ← בדיקת ערכת השחזור.";

/// The half of the password slot kept on this computer. Windows DPAPI in the program; a
/// fake in tests (no Windows in tests).
pub trait ComputerSeal: Send + Sync {
    fn available(&self) -> bool;
    fn load(&self, dir: &Path) -> Result<Key32, VaultError>;
    fn create(&self, dir: &Path) -> Result<Key32, VaultError>;
    /// A new key held aside: the key in use stays until [`Self::promote_next`].
    fn create_next(&self, dir: &Path) -> Result<Key32, VaultError>;
    fn load_next(&self, dir: &Path) -> Result<Key32, VaultError>;
    fn promote_next(&self, dir: &Path) -> Result<(), VaultError>;
}

/// The real one: sealed to her Windows account, as `vault.computer` in the vault folder.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsSeal;

impl ComputerSeal for WindowsSeal {
    fn available(&self) -> bool {
        computer_key::available()
    }
    fn load(&self, dir: &Path) -> Result<Key32, VaultError> {
        computer_key::load_for(dir, Purpose::Password)
    }
    fn create(&self, dir: &Path) -> Result<Key32, VaultError> {
        computer_key::create_for(dir, Purpose::Password)
    }
    fn create_next(&self, dir: &Path) -> Result<Key32, VaultError> {
        computer_key::create_next_for(dir, Purpose::Password)
    }
    fn load_next(&self, dir: &Path) -> Result<Key32, VaultError> {
        computer_key::load_next_for(dir, Purpose::Password)
    }
    fn promote_next(&self, dir: &Path) -> Result<(), VaultError> {
        computer_key::promote_next_for(dir, Purpose::Password)
    }
}

/// This computer's key for the password slot. For a bound slot only the key that is here
/// (none, or one that does not unseal, means another computer: it is never replaced, so a
/// key that comes back with a restored Windows profile still opens). For a slot not bound
/// yet, the key that is here or a new one, so the next opening binds it.
pub(crate) fn key_here(seal: &dyn ComputerSeal, dir: &Path, bound: bool) -> Option<Key32> {
    if !seal.available() {
        return None;
    }
    if bound {
        seal.load(dir).ok()
    } else {
        seal.load(dir).or_else(|_| seal.create(dir)).ok()
    }
}

/// The same for an opening with the password, where a Windows that did not answer in time
/// is told apart from a key that is not here: it is asked once more, and then the opening
/// says to try again, never "another computer" (a cold start could otherwise send her to
/// the kit). For a slot not bound yet, a slow answer only leaves the binding for next time.
pub(crate) fn key_for_opening(
    seal: &dyn ComputerSeal,
    dir: &Path,
    bound: bool,
) -> Result<Option<Key32>, VaultError> {
    if !seal.available() {
        return Ok(None);
    }
    let loaded = match seal.load(dir) {
        Err(VaultError::Busy) => seal.load(dir),
        other => other,
    };
    match loaded {
        Ok(key) => Ok(Some(key)),
        Err(VaultError::Busy) if bound => Err(VaultError::Busy),
        Err(VaultError::Busy) => Ok(None),
        Err(_) if bound => Ok(None),
        Err(_) => Ok(seal.create(dir).ok()),
    }
}

/// Whether the password slot on disk is bound. A header that cannot be read now (an
/// antivirus holding it, a disk error) counts as bound, so the key here is only ever loaded
/// and never replaced on a guess (review of stage 5, S4).
pub(crate) fn bound_on_disk(dir: &Path) -> bool {
    Vault::password_is_bound(dir).unwrap_or(true)
}

/// This computer's key for a new password slot while the slot on disk may be bound to
/// another key: the key here when it opens, else a new one held aside, to be promoted with
/// [`ComputerSeal::promote_next`] once the new slot is written (review of stage 5, S5). The
/// key in use is never replaced before that, so a slot that was not written still opens.
pub(crate) fn key_for_new_slot(seal: &dyn ComputerSeal, dir: &Path) -> (Option<Key32>, bool) {
    if !seal.available() {
        return (None, false);
    }
    match seal.load(dir) {
        Ok(key) => (Some(key), false),
        Err(_) => (seal.create_next(dir).ok(), true),
    }
}

/// The password as a secret: with this computer's key when there is one.
pub(crate) fn password_secret<'a>(password: &'a str, key: Option<&'a Key32>) -> Secret<'a> {
    match key {
        Some(computer) => Secret::PasswordHere { password, computer },
        None => Secret::Password(password),
    }
}

impl Core {
    /// Tests: the half kept on this computer, without Windows.
    #[must_use]
    pub fn with_computer_seal(mut self, seal: std::sync::Arc<dyn ComputerSeal>) -> Self {
        self.computer_seal = seal;
        self
    }

    /// This computer's key for the vault's password slot as it is on disk now.
    pub(crate) fn password_key_here(&self) -> Option<Key32> {
        key_here(
            self.computer_seal.as_ref(),
            &self.dir,
            bound_on_disk(&self.dir),
        )
    }

    /// This computer's key for a new password slot, and whether it is held aside until the
    /// slot is written (then [`Self::promote_password_key`]).
    pub(crate) fn new_password_key(&self) -> (Option<Key32>, bool) {
        key_for_new_slot(self.computer_seal.as_ref(), &self.dir)
    }

    /// The key held aside for a slot that was just written becomes the one in use. If this
    /// fails, the next opening with the password still finds it (see the unlock job).
    pub(crate) fn promote_password_key(&self) {
        let _ = self.computer_seal.promote_next(&self.dir);
    }

    /// Check the password on this computer without opening the vault again.
    pub(crate) fn verify_password_here(&self, password: &str) -> Result<(), VaultError> {
        let key = self.password_key_here();
        Vault::verify_password_here(&self.dir, password, key.as_ref())
    }

    /// The security page's line: bound to this computer (ok), nothing here to bind to
    /// (info), not bound although it could be (warn; it binds at the next opening with the
    /// password), or bound to another computer (`elsewhere`: she came in with the kit, and
    /// her password does not open it here until she chooses one; review of stage 5, U2).
    pub(crate) fn binding_state(&self) -> &'static str {
        match self.vault.as_ref() {
            Some(v) if v.password_bound() => {
                if self.password_key_here().is_some() {
                    "ok"
                } else {
                    "elsewhere"
                }
            }
            _ if !self.computer_seal.available() => "info",
            _ => "warn",
        }
    }
}
