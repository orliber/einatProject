//! Changing a key slot without holding the vault for the slow part (F2).
//!
//! A password change derives two keys with Argon2id (the proof, then the new password), and a
//! new recovery kit derives one when the proof is the password. Each takes a second or more.
//! The work runs on a copy of the public header, away from the open vault, and makes a
//! [`NewSlot`]; the vault then only checks it and swaps it in. A `NewSlot` exists only after a
//! valid proof (its fields are private), and applies only to the vault and the very slot it
//! was made from: when the slot changed in between, nothing is changed.

use zeroize::Zeroizing;

use super::{bound_slot, master_key, random_array, Secret, Vault};
use crate::audit::AuditEvent;
use crate::crypto::{hex, Key32};
use crate::header::{wrap_mk, KeySlot, VaultHeader};
use crate::password::{self, Argon2Params};
use crate::{recovery, VaultError};

/// Which slot a [`NewSlot`] replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    Password,
    Recovery,
}

/// A replacement key slot, made from a valid proof away from the open vault.
pub struct NewSlot {
    vault_id: String,
    kind: SlotKind,
    /// The wrapped key of the slot it replaces, as it was when the proof was checked.
    replaces: String,
    slot: KeySlot,
    /// For a password slot bound to this computer: the unbound one, kept for the backups.
    backup: Option<KeySlot>,
}

impl std::fmt::Debug for NewSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewSlot")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

fn wrapped_of(header: &VaultHeader, kind: SlotKind) -> Option<&str> {
    header.slots.iter().find_map(|s| match (s, kind) {
        (KeySlot::Password { wrapped_mk, .. }, SlotKind::Password)
        | (KeySlot::Recovery { wrapped_mk }, SlotKind::Recovery) => Some(wrapped_mk.as_str()),
        _ => None,
    })
}

fn existing(header: &VaultHeader, kind: SlotKind) -> Result<String, VaultError> {
    wrapped_of(header, kind)
        .map(str::to_owned)
        .ok_or_else(|| VaultError::Corrupt(format!("no {kind:?} slot").to_lowercase()))
}

impl NewSlot {
    /// A new password slot. `current` is the password, or the recovery kit when it was
    /// forgotten. Checks the new password's policy first, before any slow work.
    pub fn password(
        header: &VaultHeader,
        current: Secret<'_>,
        new_password: &str,
        params: Argon2Params,
    ) -> Result<Self, VaultError> {
        Self::password_here(header, current, new_password, params, None)
    }

    /// A new password slot, bound to this computer when `computer` is the key sealed here to
    /// her Windows account (D-076).
    pub fn password_here(
        header: &VaultHeader,
        current: Secret<'_>,
        new_password: &str,
        params: Argon2Params,
        computer: Option<&Key32>,
    ) -> Result<Self, VaultError> {
        password::check_policy(new_password).map_err(VaultError::Policy)?;
        let replaces = existing(header, SlotKind::Password)?;
        let mk = master_key(header, current)?;
        let salt = random_array::<32>()?;
        let stretched = password::stretch(new_password, &salt, params)?;
        let unbound = KeySlot::Password {
            salt: hex(&salt),
            params,
            wrapped_mk: wrap_mk(
                &password::kek_of(&stretched)?,
                "password",
                &header.vault_id,
                &mk,
            )?,
            bound: false,
        };
        let (slot, backup) = match computer {
            Some(c) => (
                bound_slot(&stretched, &salt, params, c, &header.vault_id, &mk)?,
                Some(unbound),
            ),
            None => (unbound, None),
        };
        Ok(Self {
            vault_id: header.vault_id.clone(),
            kind: SlotKind::Password,
            replaces,
            slot,
            backup,
        })
    }

    /// A new recovery slot and the kit to print. The old kit stops opening the vault once the
    /// slot is applied (not old backups).
    pub fn recovery(
        header: &VaultHeader,
        current: Secret<'_>,
    ) -> Result<(Self, Zeroizing<String>), VaultError> {
        let replaces = existing(header, SlotKind::Recovery)?;
        let mk = master_key(header, current)?;
        let recovery_key = Key32::random()?;
        let kek = recovery::derive_kek(&recovery_key)?;
        let slot = Self {
            vault_id: header.vault_id.clone(),
            kind: SlotKind::Recovery,
            replaces,
            slot: KeySlot::Recovery {
                wrapped_mk: wrap_mk(&kek, "recovery", &header.vault_id, &mk)?,
            },
            backup: None,
        };
        Ok((slot, recovery::format(&recovery_key)))
    }
}

impl Vault {
    /// A copy of the public header (wrapped keys and KDF parameters, no secret), for work
    /// away from the open vault.
    #[must_use]
    pub fn header_copy(&self) -> VaultHeader {
        self.header.clone()
    }

    /// Swap in a slot made by [`NewSlot`]: re-signed, written, and recorded in the log.
    /// Refused when it was made for another vault, or the slot changed since.
    pub fn apply_slot(&mut self, new: NewSlot) -> Result<(), VaultError> {
        if new.vault_id != self.header.vault_id
            || wrapped_of(&self.header, new.kind) != Some(new.replaces.as_str())
        {
            return Err(VaultError::Integrity(
                "the key slot changed while the new one was being made",
            ));
        }
        if new.kind == SlotKind::Password {
            // The slot the backups carry follows the new password (first, so a backup is
            // never written with a password slot that is gone).
            match &new.backup {
                Some(unbound) => self.keep_backup_slot(unbound)?,
                None => self.drop_backup_slot()?,
            }
        }
        for slot in &mut self.header.slots {
            let same = matches!(
                (&*slot, new.kind),
                (KeySlot::Password { .. }, SlotKind::Password)
                    | (KeySlot::Recovery { .. }, SlotKind::Recovery)
            );
            if same {
                *slot = new.slot.clone();
            }
        }
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)?;
        let event = match new.kind {
            SlotKind::Password => AuditEvent::PasswordChanged,
            SlotKind::Recovery => AuditEvent::RecoveryKeyRotated,
        };
        self.record(event, None, &serde_json::json!({}))
    }
}
