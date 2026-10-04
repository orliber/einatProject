//! Windows Hello slot (D-012, D-043): the everyday way in, with the password as fallback.
//!
//! Windows Hello keeps a key pair for this vault, made in the computer's TPM when it has one,
//! that signs only after her PIN, face or fingerprint. The slot stores a random challenge; the
//! signature over it, stretched with HKDF, is the key that wraps the master key. Windows Hello
//! signatures are RSA PKCS#1 v1.5, so the same challenge always gives the same signature; that
//! is checked when the slot is made (two signatures must match), so a key that does not behave
//! this way is never used.
//!
//! The private key never leaves Windows, the signature is never stored, and the slot is
//! useless on another computer or Windows account (a restored backup falls back to the
//! password). No `unsafe`: the Windows Runtime calls are safe in the `windows` crate.

use zeroize::Zeroizing;

use crate::crypto::{hkdf, Key32};
use crate::VaultError;

/// The platform's Windows Hello, or a stand-in in tests.
pub trait HelloSigner {
    /// Windows Hello is set up on this computer (a PIN at least).
    fn available(&self) -> bool;
    /// Make (or replace) the key pair named `credential`. Asks her to confirm.
    fn create(&self, credential: &str) -> Result<(), VaultError>;
    /// Sign `challenge` with `credential`. Asks for her PIN, face or fingerprint.
    fn sign(&self, credential: &str, challenge: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError>;
    /// Remove the key pair. Missing is fine.
    fn delete(&self, credential: &str);
}

/// The name of the key pair, one per vault.
#[must_use]
pub fn credential_name(vault_id: &str) -> String {
    format!("il.diagnosticvault.{vault_id}")
}

/// The slot's key-encryption key from the signature.
pub fn derive_kek(signature: &[u8]) -> Result<Key32, VaultError> {
    if signature.len() < 128 {
        return Err(VaultError::Crypto("windows hello signature too short"));
    }
    hkdf(signature, "dv/slot/windows_hello/v1")
}

/// Why Windows Hello did not give a signature, kept apart from a wrong signature.
pub(crate) fn refused(what: &'static str) -> VaultError {
    VaultError::Refused(format!("windows hello: {what}"))
}

/// The real Windows Hello. Elsewhere it is never available.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsHello;

#[cfg(windows)]
impl HelloSigner for WindowsHello {
    fn available(&self) -> bool {
        windows::Security::Credentials::KeyCredentialManager::IsSupportedAsync()
            .and_then(|op| op.join())
            .unwrap_or(false)
    }

    fn create(&self, credential: &str) -> Result<(), VaultError> {
        use windows::Security::Credentials::{KeyCredentialCreationOption, KeyCredentialManager};
        let result = KeyCredentialManager::RequestCreateAsync(
            &windows::core::HSTRING::from(credential),
            KeyCredentialCreationOption::ReplaceExisting,
        )
        .and_then(|op| op.join())
        .map_err(|_| refused("create failed"))?;
        status(result.Status().map_err(|_| refused("create failed"))?)
    }

    fn sign(&self, credential: &str, challenge: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        use windows::Security::Credentials::KeyCredentialManager;
        use windows::Security::Cryptography::CryptographicBuffer;
        let opened = KeyCredentialManager::OpenAsync(&windows::core::HSTRING::from(credential))
            .and_then(|op| op.join())
            .map_err(|_| refused("open failed"))?;
        status(opened.Status().map_err(|_| refused("open failed"))?)?;
        let key = opened.Credential().map_err(|_| refused("open failed"))?;
        let data = CryptographicBuffer::CreateFromByteArray(challenge)
            .map_err(|_| refused("sign failed"))?;
        let signed = key
            .RequestSignAsync(&data)
            .and_then(|op| op.join())
            .map_err(|_| refused("sign failed"))?;
        status(signed.Status().map_err(|_| refused("sign failed"))?)?;
        let buffer = signed.Result().map_err(|_| refused("sign failed"))?;
        let mut bytes = windows::core::Array::<u8>::new();
        CryptographicBuffer::CopyToByteArray(&buffer, &mut bytes)
            .map_err(|_| refused("sign failed"))?;
        let out = Zeroizing::new(bytes.to_vec());
        // `Array` frees its memory without wiping it; wipe it first.
        zeroize::Zeroize::zeroize(&mut *bytes);
        Ok(out)
    }

    fn delete(&self, credential: &str) {
        let _ = windows::Security::Credentials::KeyCredentialManager::DeleteAsync(
            &windows::core::HSTRING::from(credential),
        )
        .and_then(|op| op.join());
    }
}

#[cfg(windows)]
fn status(s: windows::Security::Credentials::KeyCredentialStatus) -> Result<(), VaultError> {
    use windows::Security::Credentials::KeyCredentialStatus as S;
    if s == S::Success {
        Ok(())
    } else if s == S::UserCanceled {
        Err(refused("cancelled"))
    } else if s == S::NotFound {
        Err(refused("no key on this computer"))
    } else {
        Err(refused("not available"))
    }
}

#[cfg(not(windows))]
impl HelloSigner for WindowsHello {
    fn available(&self) -> bool {
        false
    }
    fn create(&self, _credential: &str) -> Result<(), VaultError> {
        Err(refused("only on Windows"))
    }
    fn sign(&self, _credential: &str, _challenge: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        Err(refused("only on Windows"))
    }
    fn delete(&self, _credential: &str) {}
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn short_signatures_are_refused_and_long_ones_stretched() {
        assert!(derive_kek(&[1u8; 64]).is_err());
        let a = derive_kek(&[1u8; 256]).unwrap();
        let b = derive_kek(&[2u8; 256]).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, derive_kek(&[1u8; 256]).unwrap());
    }

    /// Runs on the Windows CI machine, which has no Windows Hello: asking must answer
    /// "not available" without crashing, and signing must fail cleanly.
    #[test]
    fn asking_windows_hello_never_crashes() {
        let hello = WindowsHello;
        let _ = hello.available();
        // No key by this name exists anywhere, so nothing is asked and nothing opens.
        assert!(hello.sign("il.diagnosticvault.test-missing", b"x").is_err());
    }
}
