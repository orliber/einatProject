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

/// The half of the password slot kept on this computer. Windows DPAPI in the program; a
/// fake in tests (no Windows in tests).
pub trait ComputerSeal: Send + Sync {
    fn available(&self) -> bool;
    fn load(&self, dir: &Path) -> Result<Key32, VaultError>;
    fn create(&self, dir: &Path) -> Result<Key32, VaultError>;
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
        let bound = Vault::password_is_bound(&self.dir).unwrap_or(false);
        key_here(self.computer_seal.as_ref(), &self.dir, bound)
    }

    /// This computer's key for a new password slot: the one here, or a new one.
    pub(crate) fn new_password_key(&self) -> Option<Key32> {
        key_here(self.computer_seal.as_ref(), &self.dir, false)
    }

    /// Check the password on this computer without opening the vault again.
    pub(crate) fn verify_password_here(&self, password: &str) -> Result<(), VaultError> {
        let key = self.password_key_here();
        Vault::verify_password_here(&self.dir, password, key.as_ref())
    }

    /// The security page's line: bound (ok), nothing here to bind to (info), or not bound
    /// although it could be (warn; it binds at the next opening with the password).
    pub(crate) fn binding_state(&self) -> &'static str {
        match self.vault.as_ref() {
            Some(v) if v.password_bound() => "ok",
            _ if !self.computer_seal.available() => "info",
            _ => "warn",
        }
    }
}
