//! `vault.header`: key slots (LUKS-style), KDF parameters and the audit anchor.
//!
//! Each slot wraps the same master key (MK) under a different key-encryption key. The
//! whole header is authenticated with an HMAC derived from MK, verified after unlock.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::crypto::{hex, hmac_sha256, hmac_verify, open, seal, unhex, Key32};
use crate::password::Argon2Params;
use crate::VaultError;

pub const HEADER_FILE: &str = "vault.header";
pub const FORMAT: u32 = 1;

/// One way to unlock the vault. New unlock methods (Windows Hello, Touch ID, FIDO2)
/// are new variants; existing vaults keep working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeySlot {
    Password {
        salt: String,
        params: Argon2Params,
        wrapped_mk: String,
    },
    Recovery {
        wrapped_mk: String,
    },
    /// Forgot the password: sign in with her Google account (D-041). The slot opens only with
    /// two keys together: a random key in her own Google Drive (the app's hidden folder) and
    /// a random key sealed to her Windows account on this computer (DPAPI). `account` is a
    /// hash of the Google account id, so another account is told apart before anything is
    /// tried.
    Google {
        account: String,
        wrapped_mk: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditAnchor {
    pub seq: i64,
    pub mac: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultHeader {
    pub format: u32,
    pub vault_id: String,
    pub created_at: i64,
    pub slots: Vec<KeySlot>,
    pub audit_anchor: Option<AuditAnchor>,
    /// HMAC over the header with this field set to `None`.
    pub mac: Option<String>,
}

fn slot_aad(kind: &str, vault_id: &str) -> Vec<u8> {
    format!("dv/slot/{kind}/v1|{vault_id}").into_bytes()
}

pub fn wrap_mk(kek: &Key32, kind: &str, vault_id: &str, mk: &Key32) -> Result<String, VaultError> {
    Ok(hex(&seal(kek, &slot_aad(kind, vault_id), mk.as_bytes())?))
}

pub fn unwrap_mk(
    kek: &Key32,
    kind: &str,
    vault_id: &str,
    wrapped: &str,
) -> Result<Key32, VaultError> {
    let plain = open(kek, &slot_aad(kind, vault_id), &unhex(wrapped)?)
        .map_err(|_| VaultError::WrongSecret)?;
    let bytes: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::Corrupt("master key length".to_owned()))?;
    Ok(Key32::from_bytes(bytes))
}

impl VaultHeader {
    fn mac_input(&self) -> Result<Vec<u8>, VaultError> {
        let unsigned = Self {
            mac: None,
            ..self.clone()
        };
        serde_json::to_vec(&unsigned).map_err(|e| VaultError::Corrupt(e.to_string()))
    }

    pub fn sign(&mut self, header_key: &Key32) -> Result<(), VaultError> {
        self.mac = Some(hex(&hmac_sha256(header_key, &self.mac_input()?)));
        Ok(())
    }

    pub fn verify(&self, header_key: &Key32) -> Result<(), VaultError> {
        let mac = self
            .mac
            .as_deref()
            .ok_or(VaultError::Integrity("header is not signed"))?;
        if hmac_verify(header_key, &self.mac_input()?, &unhex(mac)?) {
            Ok(())
        } else {
            Err(VaultError::Integrity("vault header was modified"))
        }
    }

    pub fn read(dir: &Path) -> Result<Self, VaultError> {
        Self::parse(&fs::read(dir.join(HEADER_FILE))?)
    }

    /// A header from its file bytes (also as kept inside a backup).
    pub fn parse(bytes: &[u8]) -> Result<Self, VaultError> {
        let header: Self =
            serde_json::from_slice(bytes).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        if header.format != FORMAT {
            return Err(VaultError::Corrupt(format!(
                "unsupported vault format {}",
                header.format
            )));
        }
        Ok(header)
    }

    /// Atomic replace: write a temp file, flush to disk, rename over the old header.
    pub fn write(&self, dir: &Path) -> Result<(), VaultError> {
        let tmp = dir.join(format!("{HEADER_FILE}.tmp"));
        let json =
            serde_json::to_vec_pretty(self).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, dir.join(HEADER_FILE))?;
        Ok(())
    }
}
