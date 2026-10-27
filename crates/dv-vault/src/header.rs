//! `vault.header`: key slots (LUKS-style), KDF parameters and the audit anchor.
//!
//! Each slot wraps the same master key (MK) under a different key-encryption key. The
//! whole header is authenticated with an HMAC derived from MK, verified after unlock.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::crypto::{hex, hmac_sha256, hmac_verify, open, seal, unhex, Key32};
use crate::password::Argon2Params;
use crate::{version, VaultError};

pub const HEADER_FILE: &str = "vault.header";
/// The header format this version writes. 2 (D-104): the vault records the newest version
/// that opened it. Programs from before D-104 read only 1, so they refuse a vault a version
/// with D-104 opened (as "damaged", before writing anything), instead of opening it and
/// working on what a newer version wrote. Format 1 is still read, and becomes 2 the first time
/// the vault opens here.
pub const FORMAT: u32 = 2;

/// One way to unlock the vault. New unlock methods (Windows Hello, Touch ID, FIDO2)
/// are new variants; existing vaults keep working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeySlot {
    Password {
        salt: String,
        params: Argon2Params,
        wrapped_mk: String,
        /// Bound to this computer (D-076): the key-encryption key is the stretched password
        /// together with a key sealed to her Windows account. Left out of the file when false,
        /// so a header from before reads and signs exactly as it did.
        #[serde(default, skip_serializing_if = "is_false")]
        bound: bool,
    },
    Recovery {
        wrapped_mk: String,
    },
    /// Windows Hello (D-047), taken out of the program in D-054. Still read, so a vault that
    /// had it keeps opening with the password; removed from the header on the first unlock.
    WindowsHello {
        credential: String,
        challenge: String,
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

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
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
    /// The newest version of the program that opened the vault (D-104). Left out of the file
    /// until one records itself, so a header from before reads and signs exactly as it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
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
    /// The password slot is bound to a computer (D-076).
    #[must_use]
    pub fn password_bound(&self) -> bool {
        self.slots
            .iter()
            .any(|s| matches!(s, KeySlot::Password { bound: true, .. }))
    }

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

    /// A header from its file bytes (also as kept inside a backup). A header a newer version
    /// of the program recorded itself in is refused here (D-104), before anything else is
    /// read from it, so even a format this version does not know says "a newer version".
    pub fn parse(bytes: &[u8]) -> Result<Self, VaultError> {
        Self::parse_for(bytes, version::APP_VERSION)
    }

    /// [`VaultHeader::parse`] by the program at version `current`.
    pub(crate) fn parse_for(bytes: &[u8], current: &str) -> Result<Self, VaultError> {
        /// Only the version, from a header of any shape.
        #[derive(Deserialize)]
        struct Opened {
            #[serde(default)]
            app_version: Option<serde_json::Value>,
        }
        if let Ok(opened) = serde_json::from_slice::<Opened>(bytes) {
            match opened.app_version {
                None | Some(serde_json::Value::Null) => {}
                Some(serde_json::Value::String(v)) => version::refuse_newer(Some(&v), current)?,
                // Not a version this program writes: fail closed.
                Some(_) => return Err(VaultError::NewerVersion(String::new())),
            }
        }
        let header: Self =
            serde_json::from_slice(bytes).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        if !(1..=FORMAT).contains(&header.format) {
            return Err(VaultError::Corrupt(format!(
                "unsupported vault format {}",
                header.format
            )));
        }
        Ok(header)
    }

    /// Atomic replace: write a temp file, flush to disk, rename over the old header. Each
    /// write has its own temp file, so two writes never fill the same one and rename half of
    /// one and half of the other into place; the last rename wins whole.
    pub fn write(&self, dir: &Path) -> Result<(), VaultError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!("{HEADER_FILE}.{}.{n}.tmp", std::process::id()));
        let json =
            serde_json::to_vec_pretty(self).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        let written = (|| -> Result<(), VaultError> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
            fs::rename(&tmp, dir.join(HEADER_FILE))?;
            Ok(())
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }
}
