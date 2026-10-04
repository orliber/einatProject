//! The Windows Hello slot (D-043) with a stand-in for Windows Hello. Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use dv_vault::hello::HelloSigner;
use dv_vault::{Argon2Params, Secret, Vault, VaultError};
use zeroize::Zeroizing;

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

/// Deterministic like Windows Hello's RSA PKCS#1 v1.5: the signature depends on the key pair
/// and the challenge only. Each `create` makes a new key pair.
#[derive(Default)]
struct FakeHello {
    keys: RefCell<HashMap<String, u8>>,
    next: Cell<u8>,
    cancel: Cell<bool>,
    unstable: Cell<bool>,
    calls: Cell<u32>,
}

impl HelloSigner for FakeHello {
    fn available(&self) -> bool {
        true
    }
    fn create(&self, credential: &str) -> Result<(), VaultError> {
        self.next.set(self.next.get() + 1);
        self.keys
            .borrow_mut()
            .insert(credential.to_owned(), self.next.get());
        Ok(())
    }
    fn sign(&self, credential: &str, challenge: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        self.calls.set(self.calls.get() + 1);
        if self.cancel.get() {
            return Err(VaultError::Refused("windows hello: cancelled".into()));
        }
        let key = *self
            .keys
            .borrow()
            .get(credential)
            .ok_or(VaultError::Refused("windows hello: no key".into()))?;
        let salt = if self.unstable.get() {
            u8::try_from(self.calls.get() % 251).unwrap()
        } else {
            0
        };
        Ok(Zeroizing::new(
            (0..256)
                .map(|i| challenge[i % challenge.len()] ^ key ^ salt ^ u8::try_from(i % 256).unwrap())
                .collect(),
        ))
    }
    fn delete(&self, credential: &str) {
        self.keys.borrow_mut().remove(credential);
    }
}

fn vault_with_hello() -> (tempfile::TempDir, FakeHello) {
    let dir = tempfile::tempdir().unwrap();
    let hello = FakeHello::default();
    let mut vault = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST)
        .unwrap()
        .vault;
    assert!(!Vault::offers_hello(dir.path()));
    vault
        .set_hello_slot(Secret::Password(PASSWORD), &hello)
        .unwrap();
    assert!(vault.has_hello_slot());
    vault.lock().unwrap();
    (dir, hello)
}

#[test]
fn hello_opens_the_vault_and_the_password_still_does() {
    let (dir, hello) = vault_with_hello();
    assert!(Vault::offers_hello(dir.path()));
    let vault = Vault::unlock_with_hello(dir.path(), &hello).unwrap();
    assert!(vault.integrity().header_ok);
    vault.lock().unwrap();
    Vault::unlock_with_password(dir.path(), PASSWORD)
        .unwrap()
        .lock()
        .unwrap();
}

#[test]
fn turning_hello_on_needs_the_password() {
    let dir = tempfile::tempdir().unwrap();
    let hello = FakeHello::default();
    let mut vault = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST)
        .unwrap()
        .vault;
    let err = vault
        .set_hello_slot(Secret::Password("סיסמה אחרת לגמרי"), &hello)
        .unwrap_err();
    assert!(matches!(err, VaultError::WrongSecret), "{err:?}");
    assert!(!vault.has_hello_slot());
}

#[test]
fn cancelled_or_missing_hello_never_opens_and_falls_back() {
    let (dir, hello) = vault_with_hello();
    hello.cancel.set(true);
    assert!(Vault::unlock_with_hello(dir.path(), &hello).is_err());
    hello.cancel.set(false);
    // Another computer: the key pair is not there.
    let elsewhere = FakeHello::default();
    assert!(Vault::unlock_with_hello(dir.path(), &elsewhere).is_err());
    Vault::unlock_with_password(dir.path(), PASSWORD)
        .unwrap()
        .lock()
        .unwrap();
}

#[test]
fn a_key_pair_that_signs_differently_each_time_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let hello = FakeHello::default();
    hello.unstable.set(true);
    let mut vault = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST)
        .unwrap()
        .vault;
    assert!(vault
        .set_hello_slot(Secret::Password(PASSWORD), &hello)
        .is_err());
    assert!(!vault.has_hello_slot());
    assert!(hello.keys.borrow().is_empty(), "the key pair is removed");
}

#[test]
fn turning_hello_off_removes_the_slot_and_the_key_pair() {
    let (dir, hello) = vault_with_hello();
    let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    vault.remove_hello_slot(&hello).unwrap();
    assert!(!vault.has_hello_slot());
    assert!(hello.keys.borrow().is_empty());
    vault.lock().unwrap();
    assert!(!Vault::offers_hello(dir.path()));
    assert!(Vault::unlock_with_hello(dir.path(), &hello).is_err());
}

#[test]
fn turning_hello_on_again_retires_the_old_key_pair() {
    let (dir, hello) = vault_with_hello();
    let header_before = std::fs::read(dir.path().join("vault.header")).unwrap();
    let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    vault
        .set_hello_slot(Secret::Password(PASSWORD), &hello)
        .unwrap();
    vault.lock().unwrap();
    Vault::unlock_with_hello(dir.path(), &hello)
        .unwrap()
        .lock()
        .unwrap();
    // An old header (say, from a backup) names the same key pair, now replaced: no way in.
    std::fs::write(dir.path().join("vault.header"), header_before).unwrap();
    assert!(Vault::unlock_with_hello(dir.path(), &hello).is_err());
}

#[test]
fn the_header_never_holds_the_signature_or_the_master_key() {
    let (dir, hello) = vault_with_hello();
    let header = std::fs::read_to_string(dir.path().join("vault.header")).unwrap();
    assert!(header.contains("windows_hello"));
    let challenge_hex = header
        .split("\"challenge\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    let challenge: Vec<u8> = (0..challenge_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&challenge_hex[i..i + 2], 16).unwrap())
        .collect();
    let credential = hello.keys.borrow().keys().next().unwrap().clone();
    let signature = hello.sign(&credential, &challenge).unwrap();
    let sig_hex: String = signature[..16].iter().map(|b| format!("{b:02x}")).collect();
    assert!(!header.contains(&sig_hex));
}
