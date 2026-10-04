//! The writing-style profile (D-043): past reports as neutralized text, the profile versions,
//! and what was learned from Einat's edits. Nothing here belongs to a case, so everything is
//! sealed with the settings key. The contents are the caller's own JSON.

use rusqlite::{params, OptionalExtension};

use super::{aad, now, Vault};
use crate::audit::AuditEvent;
use crate::crypto::{open_string, random_id, seal_str};
use crate::VaultError;

/// A past report as kept: neutralized text and what the caller knows about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStyleSource {
    pub id: String,
    pub created_at: i64,
    pub json: String,
}

/// One version of the profile. `status`: `draft` | `active` | `retired`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStyleProfile {
    pub id: String,
    pub version: u32,
    pub status: String,
    pub created_at: i64,
    pub json: String,
}

const STATUSES: [&str; 3] = ["draft", "active", "retired"];

impl Vault {
    /// Create (`id` = None) or replace a past report. Returns its id.
    pub fn save_style_source(
        &mut self,
        id: Option<&str>,
        json: &str,
    ) -> Result<String, VaultError> {
        let (id, new) = match id {
            Some(id) => (id.to_owned(), false),
            None => (random_id()?, true),
        };
        let sealed = seal_str(
            &self.keys.settings,
            &aad("style_sources", "data", &id, ""),
            json,
        )?;
        if new {
            self.main.execute(
                "INSERT INTO style_sources (id, created_at, data_enc) VALUES (?1, ?2, ?3)",
                params![id, now(), sealed],
            )?;
            self.record(AuditEvent::StyleSourceAdded, None, &serde_json::json!({}))?;
        } else if self.main.execute(
            "UPDATE style_sources SET data_enc = ?1 WHERE id = ?2",
            params![sealed, id],
        )? == 0
        {
            return Err(VaultError::NotFound);
        }
        Ok(id)
    }

    /// Every past report, oldest first.
    pub fn style_sources(&self) -> Result<Vec<StoredStyleSource>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT id, created_at, data_enc FROM style_sources ORDER BY created_at, id",
        )?;
        let rows: Vec<(String, i64, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, created_at, sealed)| {
                let json = open_string(
                    &self.keys.settings,
                    &aad("style_sources", "data", &id, ""),
                    &sealed,
                )?;
                Ok(StoredStyleSource {
                    id,
                    created_at,
                    json,
                })
            })
            .collect()
    }

    pub fn style_source(&self, id: &str) -> Result<Option<StoredStyleSource>, VaultError> {
        Ok(self.style_sources()?.into_iter().find(|s| s.id == id))
    }

    /// Erase a past report, out of the write-ahead log too.
    pub fn delete_style_source(&mut self, id: &str) -> Result<(), VaultError> {
        if self
            .main
            .execute("DELETE FROM style_sources WHERE id = ?1", [id])?
            == 0
        {
            return Err(VaultError::NotFound);
        }
        self.main
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.record(AuditEvent::StyleSourceDeleted, None, &serde_json::json!({}))
    }

    /// A new profile version, numbered after the last one. Returns `(id, version)`.
    pub fn add_style_profile(
        &mut self,
        status: &str,
        json: &str,
    ) -> Result<(String, u32), VaultError> {
        if !STATUSES.contains(&status) {
            return Err(VaultError::Refused(format!("unknown status {status}")));
        }
        let id = random_id()?;
        let last: Option<u32> =
            self.main
                .query_row("SELECT MAX(version) FROM style_profiles", [], |r| r.get(0))?;
        let version = last.unwrap_or(0) + 1;
        let sealed = seal_str(
            &self.keys.settings,
            &aad("style_profiles", "data", &id, ""),
            json,
        )?;
        let tx = self.main.unchecked_transaction()?;
        if status == "active" {
            tx.execute(
                "UPDATE style_profiles SET status = 'retired' WHERE status = 'active'",
                [],
            )?;
        }
        tx.execute(
            "INSERT INTO style_profiles (id, version, status, created_at, data_enc)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, version, status, now(), sealed],
        )?;
        tx.commit()?;
        if status == "active" {
            self.record(
                AuditEvent::StyleProfileApproved,
                None,
                &serde_json::json!({ "version": version }),
            )?;
        }
        Ok((id, version))
    }

    /// Every profile version, newest first.
    pub fn style_profiles(&self) -> Result<Vec<StoredStyleProfile>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT id, version, status, created_at, data_enc FROM style_profiles
             ORDER BY version DESC",
        )?;
        let rows: Vec<(String, u32, String, i64, Vec<u8>)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, version, status, created_at, sealed)| {
                let json = open_string(
                    &self.keys.settings,
                    &aad("style_profiles", "data", &id, ""),
                    &sealed,
                )?;
                Ok(StoredStyleProfile {
                    id,
                    version,
                    status,
                    created_at,
                    json,
                })
            })
            .collect()
    }

    /// Erase draft versions (a draft that was replaced or thrown away). Active and retired
    /// versions stay: they are the history that can be brought back.
    pub fn delete_style_drafts(&mut self) -> Result<(), VaultError> {
        self.main
            .execute("DELETE FROM style_profiles WHERE status = 'draft'", [])?;
        self.main
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Erase every profile version and everything learned (when the last past report is
    /// deleted and Einat asks to start over).
    pub fn reset_style(&mut self) -> Result<(), VaultError> {
        let tx = self.main.unchecked_transaction()?;
        tx.execute("DELETE FROM style_profiles", [])?;
        tx.execute("DELETE FROM style_learning", [])?;
        tx.commit()?;
        self.main
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.record(
            AuditEvent::SettingsChanged,
            None,
            &serde_json::json!({ "key": "style_reset" }),
        )
    }

    /// What was learned from Einat's edits (one sealed document), or `None`.
    pub fn style_learning(&self) -> Result<Option<String>, VaultError> {
        let sealed: Option<Vec<u8>> = self
            .main
            .query_row(
                "SELECT data_enc FROM style_learning WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        sealed
            .map(|s| {
                open_string(
                    &self.keys.settings,
                    &aad("style_learning", "data", "1", ""),
                    &s,
                )
            })
            .transpose()
    }

    pub fn save_style_learning(&mut self, json: &str) -> Result<(), VaultError> {
        let sealed = seal_str(
            &self.keys.settings,
            &aad("style_learning", "data", "1", ""),
            json,
        )?;
        self.main.execute(
            "INSERT INTO style_learning (id, data_enc) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET data_enc = ?1",
            [sealed],
        )?;
        Ok(())
    }
}
