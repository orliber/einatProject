//! The clinic (beta): visits, notes, payments, charges, tasks and settings, their history,
//! and the patient cards. Nothing here belongs to a case, so everything is sealed with the
//! settings key. The vault knows nothing of the clinic's types: the contents are the
//! caller's own JSON. Only `kind`, `day` and `patient_id` stay in the clear, to index by.

use rusqlite::{params, Connection, OptionalExtension};

use super::{aad, now, Vault};
use crate::audit::AuditEvent;
use crate::crypto::{open_string, random_id, seal_str, Key32};
use crate::VaultError;

/// One clinic record as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredClinicRecord {
    pub id: String,
    pub kind: String,
    pub patient_id: Option<String>,
    pub day: i64,
    pub updated_at: i64,
    pub json: String,
}

/// One earlier state of a record, with what was done to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredClinicHistory {
    pub saved_at: i64,
    pub action: String,
    pub json: String,
}

const KINDS: [&str; 6] = ["visit", "note", "payment", "charge", "task", "settings"];
const ACTIONS: [&str; 4] = ["created", "changed", "voided", "deleted"];
/// The one settings record has a fixed id.
const SETTINGS_ID: &str = "settings";

type RecordRow = (String, String, Option<String>, i64, i64, Vec<u8>);

fn check_kind(kind: &str) -> Result<(), VaultError> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(VaultError::Refused(format!("unknown clinic kind {kind}")))
    }
}

fn check_action(history: Option<&str>) -> Result<(), VaultError> {
    match history {
        Some(action) if !ACTIONS.contains(&action) => Err(VaultError::Refused(format!(
            "unknown clinic history action {action}"
        ))),
        _ => Ok(()),
    }
}

fn history_row(
    conn: &Connection,
    key: &Key32,
    record_id: &str,
    patient_id: Option<&str>,
    action: &str,
    json: &str,
) -> Result<(), VaultError> {
    let id = random_id()?;
    let sealed = seal_str(key, &aad("clinic_history", "data", &id, ""), json)?;
    conn.execute(
        "INSERT INTO clinic_history (id, record_id, patient_id, saved_at, action, data_enc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, record_id, patient_id, now(), action, sealed],
    )?;
    Ok(())
}

fn open_record(key: &Key32, row: RecordRow) -> Result<StoredClinicRecord, VaultError> {
    let (id, kind, patient_id, day, updated_at, sealed) = row;
    let json = open_string(key, &aad("clinic_records", "data", &id, ""), &sealed)?;
    Ok(StoredClinicRecord {
        id,
        kind,
        patient_id,
        day,
        updated_at,
        json,
    })
}

const RECORD_COLUMNS: &str = "id, kind, patient_id, day, updated_at, data_enc";

fn record_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RecordRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
    ))
}

impl Vault {
    /// Create (`id` = None) or replace a record. Returns its id. See [`Vault::clinic_put_many`].
    pub fn clinic_put(
        &mut self,
        kind: &str,
        id: Option<&str>,
        patient_id: Option<&str>,
        day: i64,
        json: &str,
        history: Option<&str>,
    ) -> Result<String, VaultError> {
        let row = (
            id.map(str::to_owned),
            patient_id.map(str::to_owned),
            day,
            json.to_owned(),
        );
        let mut ids = self.clinic_put_many(kind, &[row], history)?;
        ids.pop().ok_or(VaultError::NotFound)
    }

    /// Several records of one kind in ONE transaction: all written or none (a repeating
    /// series). A row with no id is new; a given id replaces that record (it must exist, and
    /// be of this kind), except the settings record, which is created on first save.
    /// With `history` = Some(action), each row's JSON is also written to the history, in the
    /// same transaction.
    pub fn clinic_put_many(
        &mut self,
        kind: &str,
        rows: &[(Option<String>, Option<String>, i64, String)],
        history: Option<&str>,
    ) -> Result<Vec<String>, VaultError> {
        check_kind(kind)?;
        check_action(history)?;
        let key = &self.keys.settings;
        let tx = self.main.unchecked_transaction()?;
        let mut ids = Vec::with_capacity(rows.len());
        for (id, patient_id, day, json) in rows {
            let patient_id = patient_id.as_deref();
            let existing: Option<String> = match id {
                Some(id) => tx
                    .query_row("SELECT kind FROM clinic_records WHERE id = ?1", [id], |r| {
                        r.get(0)
                    })
                    .optional()?,
                None => None,
            };
            let record_id = match (id, existing) {
                (Some(id), Some(old_kind)) => {
                    if old_kind != kind {
                        return Err(VaultError::Refused(
                            "a clinic record cannot change its kind".to_owned(),
                        ));
                    }
                    id.clone()
                }
                (Some(id), None) => {
                    if kind != "settings" || id != SETTINGS_ID {
                        return Err(VaultError::NotFound);
                    }
                    id.clone()
                }
                (None, _) if kind == "settings" => {
                    // The settings record is one fixed row: saving it again replaces it.
                    let present: Option<String> = tx
                        .query_row(
                            "SELECT kind FROM clinic_records WHERE id = ?1",
                            [SETTINGS_ID],
                            |r| r.get(0),
                        )
                        .optional()?;
                    if present.is_some_and(|k| k != "settings") {
                        return Err(VaultError::Refused(
                            "a clinic record cannot change its kind".to_owned(),
                        ));
                    }
                    SETTINGS_ID.to_owned()
                }
                (None, _) => random_id()?,
            };
            let sealed = seal_str(key, &aad("clinic_records", "data", &record_id, ""), json)?;
            tx.execute(
                "INSERT INTO clinic_records (id, kind, patient_id, day, updated_at, data_enc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET patient_id = ?3, day = ?4, updated_at = ?5,
                    data_enc = ?6",
                params![record_id, kind, patient_id, day, now(), sealed],
            )?;
            if let Some(action) = history {
                history_row(&tx, key, &record_id, patient_id, action, json)?;
            }
            ids.push(record_id);
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn clinic_get(&self, id: &str) -> Result<Option<StoredClinicRecord>, VaultError> {
        let row = self
            .main
            .query_row(
                &format!("SELECT {RECORD_COLUMNS} FROM clinic_records WHERE id = ?1"),
                [id],
                record_row,
            )
            .optional()?;
        row.map(|r| open_record(&self.keys.settings, r)).transpose()
    }

    fn clinic_query(
        &self,
        sql: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<StoredClinicRecord>, VaultError> {
        let mut stmt = self.main.prepare(sql)?;
        let rows: Vec<RecordRow> = stmt
            .query_map(args, record_row)?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|r| open_record(&self.keys.settings, r))
            .collect()
    }

    /// Every record of a kind, by day then id.
    pub fn clinic_kind(&self, kind: &str) -> Result<Vec<StoredClinicRecord>, VaultError> {
        self.clinic_query(
            &format!(
                "SELECT {RECORD_COLUMNS} FROM clinic_records WHERE kind = ?1 ORDER BY day, id"
            ),
            &[&kind],
        )
    }

    /// The records of a kind from `from_day` to `to_day`, both included.
    pub fn clinic_range(
        &self,
        kind: &str,
        from_day: i64,
        to_day: i64,
    ) -> Result<Vec<StoredClinicRecord>, VaultError> {
        self.clinic_query(
            &format!(
                "SELECT {RECORD_COLUMNS} FROM clinic_records
                 WHERE kind = ?1 AND day >= ?2 AND day <= ?3 ORDER BY day, id"
            ),
            &[&kind, &from_day, &to_day],
        )
    }

    /// Every record of a patient, of every kind, by day then id.
    pub fn clinic_of_patient(
        &self,
        patient_id: &str,
    ) -> Result<Vec<StoredClinicRecord>, VaultError> {
        self.clinic_query(
            &format!(
                "SELECT {RECORD_COLUMNS} FROM clinic_records WHERE patient_id = ?1
                 ORDER BY day, id"
            ),
            &[&patient_id],
        )
    }

    /// Erase a record, out of the write-ahead log too. With `history`, a `deleted` row holding
    /// its last JSON is written in the same transaction.
    pub fn clinic_delete(&mut self, id: &str, history: bool) -> Result<(), VaultError> {
        self.clinic_delete_many(&[id.to_owned()], history)
    }

    /// Several records in one transaction: all erased or none. A missing one is `NotFound`
    /// and nothing is erased.
    pub fn clinic_delete_many(&mut self, ids: &[String], history: bool) -> Result<(), VaultError> {
        let key = &self.keys.settings;
        let tx = self.main.unchecked_transaction()?;
        for id in ids {
            let row = tx
                .query_row(
                    &format!("SELECT {RECORD_COLUMNS} FROM clinic_records WHERE id = ?1"),
                    [id],
                    record_row,
                )
                .optional()?
                .ok_or(VaultError::NotFound)?;
            if history {
                let rec = open_record(key, row)?;
                history_row(
                    &tx,
                    key,
                    &rec.id,
                    rec.patient_id.as_deref(),
                    "deleted",
                    &rec.json,
                )?;
            }
            tx.execute("DELETE FROM clinic_records WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        self.main
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// A record's earlier states, oldest first.
    pub fn clinic_history(&self, record_id: &str) -> Result<Vec<StoredClinicHistory>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT id, saved_at, action, data_enc FROM clinic_history
             WHERE record_id = ?1 ORDER BY saved_at, rowid",
        )?;
        let rows: Vec<(String, i64, String, Vec<u8>)> = stmt
            .query_map([record_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, saved_at, action, sealed)| {
                let json = open_string(
                    &self.keys.settings,
                    &aad("clinic_history", "data", &id, ""),
                    &sealed,
                )?;
                Ok(StoredClinicHistory {
                    saved_at,
                    action,
                    json,
                })
            })
            .collect()
    }

    /// For each record that has history, how many rows it has.
    pub fn clinic_history_counts(&self) -> Result<Vec<(String, u32)>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT record_id, COUNT(*) FROM clinic_history GROUP BY record_id ORDER BY record_id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<(String, u32)>, _>>()?;
        Ok(rows)
    }

    /// Every patient card as `(id, json)`, oldest first.
    pub fn clinic_patients(&self) -> Result<Vec<(String, String)>, VaultError> {
        let mut stmt = self
            .identity
            .prepare("SELECT id, data_enc FROM clinic_patients ORDER BY created_at, id")?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, sealed)| {
                let json = open_string(
                    &self.keys.settings,
                    &aad("clinic_patients", "data", &id, ""),
                    &sealed,
                )?;
                Ok((id, json))
            })
            .collect()
    }

    pub fn clinic_patient(&self, id: &str) -> Result<Option<String>, VaultError> {
        let sealed: Option<Vec<u8>> = self
            .identity
            .query_row(
                "SELECT data_enc FROM clinic_patients WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        sealed
            .map(|s| {
                open_string(
                    &self.keys.settings,
                    &aad("clinic_patients", "data", id, ""),
                    &s,
                )
            })
            .transpose()
    }

    /// Create (`id` = None) or replace a patient card. Returns its id.
    pub fn save_clinic_patient(
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
            &aad("clinic_patients", "data", &id, ""),
            json,
        )?;
        let t = now();
        if new {
            self.identity.execute(
                "INSERT INTO clinic_patients (id, created_at, updated_at, data_enc)
                 VALUES (?1, ?2, ?2, ?3)",
                params![id, t, sealed],
            )?;
            self.record(AuditEvent::ClinicPatientAdded, None, &serde_json::json!({}))?;
        } else if self.identity.execute(
            "UPDATE clinic_patients SET updated_at = ?1, data_enc = ?2 WHERE id = ?3",
            params![t, sealed, id],
        )? == 0
        {
            return Err(VaultError::NotFound);
        }
        Ok(id)
    }

    /// Erase a patient: the card, every record and every history row of theirs, out of both
    /// write-ahead logs. One audit event with the number of records (a number only).
    pub fn erase_clinic_patient(&mut self, id: &str) -> Result<(), VaultError> {
        let known: Option<String> = self
            .identity
            .query_row("SELECT id FROM clinic_patients WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if known.is_none() {
            return Err(VaultError::NotFound);
        }
        // The records first: if the card's removal fails, the erase can be asked again.
        let tx = self.main.unchecked_transaction()?;
        let records = tx.execute("DELETE FROM clinic_records WHERE patient_id = ?1", [id])?;
        tx.execute("DELETE FROM clinic_history WHERE patient_id = ?1", [id])?;
        tx.commit()?;
        self.identity
            .execute("DELETE FROM clinic_patients WHERE id = ?1", [id])?;
        self.main
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.identity
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.record(
            AuditEvent::ClinicPatientErased,
            None,
            &serde_json::json!({ "records": records }),
        )
    }
}

#[cfg(test)]
#[path = "clinic_tests.rs"]
mod tests;
