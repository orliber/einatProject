//! The clinic (beta): visits, notes, payments, charges, tasks and settings, their history,
//! and the patient cards. Nothing here belongs to a case, so everything is sealed with the
//! settings key. The vault knows nothing of the clinic's types: the contents are the
//! caller's own JSON. Only `kind`, `day` and `patient_id` stay in the clear, to index by.
//!
//! The clear columns that say what a row is are bound into its seal (D-177): a record's
//! `kind` and `patient_id`, a history row's `record_id`, `patient_id` and `action`. A row
//! whose column was changed in the file does not open. The lists ([`Vault::clinic_kind`],
//! [`Vault::clinic_range`], [`Vault::clinic_patients`]) leave such a row out, and
//! [`Vault::clinic_unreadable`] counts them (D-178); reading one record, one patient's
//! records or a history fails instead.

use dv_domain::InputKind;
use rusqlite::{params, Connection, OptionalExtension};

use super::{aad, enum_str, now, Vault};
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

const KINDS: [&str; 7] = [
    "visit", "note", "payment", "charge", "task", "settings", "tally",
];
const ACTIONS: [&str; 4] = ["created", "changed", "voided", "deleted"];
/// What may be removed one record at a time. A payment and a charge are voided, never removed
/// (D-172), and the settings record is only replaced: those go only with their patient.
const REMOVABLE: [&str; 3] = ["visit", "note", "task"];
/// The one settings record has a fixed id.
const SETTINGS_ID: &str = "settings";
/// The anonymous tally of the cards that were erased (D-179): one record with a fixed id,
/// written only by [`Vault::erase_clinic_patient`], in the transaction that removes the
/// records it stands for.
const TALLY: &str = "tally";
const TALLY_ID: &str = "tally";

type RecordRow = (String, String, Option<String>, i64, i64, Vec<u8>);
type HistoryRow = (String, Option<String>, i64, String, Vec<u8>);
/// A record to write: its id (`None`: a new one), its patient, its day and its JSON.
pub type ClinicPut = (Option<String>, Option<String>, i64, String);

/// What a record's seal is bound to besides its id: its kind and its patient (D-177).
fn record_aad(id: &str, kind: &str, patient_id: Option<&str>) -> Vec<u8> {
    aad(
        "clinic_records",
        "data",
        id,
        &format!("{kind}|{}", patient_id.unwrap_or_default()),
    )
}

/// What a history row's seal is bound to besides its id: the record it belongs to, the
/// patient and what was done (D-177).
fn history_aad(id: &str, record_id: &str, patient_id: Option<&str>, action: &str) -> Vec<u8> {
    aad(
        "clinic_history",
        "data",
        id,
        &format!("{record_id}|{}|{action}", patient_id.unwrap_or_default()),
    )
}

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
    let sealed = seal_str(key, &history_aad(&id, record_id, patient_id, action), json)?;
    conn.execute(
        "INSERT INTO clinic_history (id, record_id, patient_id, saved_at, action, data_enc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, record_id, patient_id, now(), action, sealed],
    )?;
    Ok(())
}

fn open_record(key: &Key32, row: RecordRow) -> Result<StoredClinicRecord, VaultError> {
    let (id, kind, patient_id, day, updated_at, sealed) = row;
    let json = open_string(key, &record_aad(&id, &kind, patient_id.as_deref()), &sealed)?;
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

/// Writes one record inside a transaction, with its history row when asked. Returns its id.
fn put_row(
    tx: &Connection,
    key: &Key32,
    kind: &str,
    row: &ClinicPut,
    history: Option<&str>,
) -> Result<String, VaultError> {
    let (id, patient_id, day, json) = row;
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
    let sealed = seal_str(key, &record_aad(&record_id, kind, patient_id), json)?;
    tx.execute(
        "INSERT INTO clinic_records (id, kind, patient_id, day, updated_at, data_enc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET patient_id = ?3, day = ?4, updated_at = ?5,
            data_enc = ?6",
        params![record_id, kind, patient_id, day, now(), sealed],
    )?;
    if let Some(action) = history {
        history_row(tx, key, &record_id, patient_id, action, json)?;
    }
    Ok(record_id)
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
    /// same transaction. The tally of erased cards is never written from here.
    pub fn clinic_put_many(
        &mut self,
        kind: &str,
        rows: &[ClinicPut],
        history: Option<&str>,
    ) -> Result<Vec<String>, VaultError> {
        check_kind(kind)?;
        check_action(history)?;
        if kind == TALLY {
            return Err(VaultError::Refused(
                "the clinic tally is written only when a patient is erased".to_owned(),
            ));
        }
        let key = &self.keys.settings;
        let tx = self.main.unchecked_transaction()?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            ids.push(put_row(&tx, key, kind, row, history)?);
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Records of several kinds in ONE transaction (D-198: the visits of an assessment
    /// pathway and the charge of its package): all written or none. Each group follows the
    /// rules of [`Vault::clinic_put_many`]; the settings record and the tally are not
    /// written from here.
    pub fn clinic_put_groups(
        &mut self,
        groups: &[(&str, &[ClinicPut])],
        history: Option<&str>,
    ) -> Result<Vec<Vec<String>>, VaultError> {
        check_action(history)?;
        for (kind, _) in groups {
            check_kind(kind)?;
            if *kind == TALLY || *kind == "settings" {
                return Err(VaultError::Refused(
                    "the clinic tally and the settings are not written in a group".to_owned(),
                ));
            }
        }
        let key = &self.keys.settings;
        let tx = self.main.unchecked_transaction()?;
        let mut all = Vec::with_capacity(groups.len());
        for (kind, rows) in groups {
            let mut ids = Vec::with_capacity(rows.len());
            for row in *rows {
                ids.push(put_row(&tx, key, kind, row, history)?);
            }
            all.push(ids);
        }
        tx.commit()?;
        Ok(all)
    }

    /// A session note's formal record goes into a case as a material, and the note is saved
    /// with the material's id, in ONE transaction (D-177): either both are written or
    /// neither, so asking again after a failure never adds the material twice. `input_id` is
    /// the caller's new random id for the material; `note` is the note as it should be kept
    /// (its JSON already names `input_id`), written with a `changed` history row.
    pub fn clinic_note_into_case(
        &mut self,
        case_id: &str,
        input_id: &str,
        kind: InputKind,
        title: &str,
        content: &str,
        note: &ClinicPut,
    ) -> Result<(), VaultError> {
        if note.0.is_none() {
            return Err(VaultError::NotFound);
        }
        let case_key = self.case_key(case_id)?;
        let key = &self.keys.settings;
        let tx = self.main.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO inputs (id, case_id, kind, created_at, title_enc, content_enc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                input_id,
                case_id,
                enum_str(&kind)?,
                now(),
                seal_str(&case_key, &aad("inputs", "title", input_id, case_id), title)?,
                seal_str(
                    &case_key,
                    &aad("inputs", "content", input_id, case_id),
                    content
                )?
            ],
        )?;
        tx.execute(
            "UPDATE cases SET updated_at = ?1 WHERE id = ?2",
            params![now(), case_id],
        )?;
        put_row(&tx, key, "note", note, Some("changed"))?;
        tx.commit()?;
        Ok(())
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

    fn clinic_rows(
        &self,
        sql: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<RecordRow>, VaultError> {
        let mut stmt = self.main.prepare(sql)?;
        let rows: Vec<RecordRow> = stmt
            .query_map(args, record_row)?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Every row asked for, or an error: for a read that must not go on with a row missing.
    fn clinic_query(
        &self,
        sql: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<StoredClinicRecord>, VaultError> {
        self.clinic_rows(sql, args)?
            .into_iter()
            .map(|r| open_record(&self.keys.settings, r))
            .collect()
    }

    /// The rows that open; one that does not is left out (and counted by
    /// [`Vault::clinic_unreadable`]), so one bad row does not take a whole screen with it.
    fn clinic_list(
        &self,
        sql: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<StoredClinicRecord>, VaultError> {
        Ok(self
            .clinic_rows(sql, args)?
            .into_iter()
            .filter_map(|r| open_record(&self.keys.settings, r).ok())
            .collect())
    }

    /// How many clinic rows (records and patient cards) do not open: a row damaged on disk,
    /// or one whose clear columns were changed (D-177, D-178). A number only.
    pub fn clinic_unreadable(&self) -> Result<u32, VaultError> {
        let key = &self.keys.settings;
        let records = self
            .clinic_rows(&format!("SELECT {RECORD_COLUMNS} FROM clinic_records"), &[])?
            .into_iter()
            .filter(|r| open_record(key, r.clone()).is_err())
            .count();
        let mut stmt = self
            .identity
            .prepare("SELECT id, data_enc FROM clinic_patients")?;
        let cards: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let cards = cards
            .into_iter()
            .filter(|(id, sealed)| {
                open_string(key, &aad("clinic_patients", "data", id, ""), sealed).is_err()
            })
            .count();
        Ok(u32::try_from(records.saturating_add(cards)).unwrap_or(u32::MAX))
    }

    /// Every readable record of a kind, by day then id.
    pub fn clinic_kind(&self, kind: &str) -> Result<Vec<StoredClinicRecord>, VaultError> {
        self.clinic_list(
            &format!(
                "SELECT {RECORD_COLUMNS} FROM clinic_records WHERE kind = ?1 ORDER BY day, id"
            ),
            &[&kind],
        )
    }

    /// The readable records of a kind from `from_day` to `to_day`, both included.
    pub fn clinic_range(
        &self,
        kind: &str,
        from_day: i64,
        to_day: i64,
    ) -> Result<Vec<StoredClinicRecord>, VaultError> {
        self.clinic_list(
            &format!(
                "SELECT {RECORD_COLUMNS} FROM clinic_records
                 WHERE kind = ?1 AND day >= ?2 AND day <= ?3 ORDER BY day, id"
            ),
            &[&kind, &from_day, &to_day],
        )
    }

    /// Every record of a kind from `from_day` to `to_day`, both included, or an error when
    /// one of them does not open: for a file that leaves the vault (the income export,
    /// D-194), which is never made with a row missing.
    pub fn clinic_range_strict(
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

    /// Every record of a patient, of every kind, by day then id. One that does not open is
    /// an error: what is decided from a patient's records (an erase, what the family owes)
    /// is never decided with a row missing.
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

    /// Several records in one transaction: all erased or none. A missing one is `NotFound`,
    /// a payment, a charge or the settings record is refused, and nothing is erased.
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
            if !REMOVABLE.contains(&row.1.as_str()) {
                return Err(VaultError::Refused(format!(
                    "a clinic {} is never removed",
                    row.1
                )));
            }
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
            "SELECT id, patient_id, saved_at, action, data_enc FROM clinic_history
             WHERE record_id = ?1 ORDER BY saved_at, rowid",
        )?;
        let rows: Vec<HistoryRow> = stmt
            .query_map([record_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, patient_id, saved_at, action, sealed)| {
                let json = open_string(
                    &self.keys.settings,
                    &history_aad(&id, record_id, patient_id.as_deref(), &action),
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

    /// Every patient card that opens as `(id, json)`, oldest first. One that does not is left
    /// out and counted by [`Vault::clinic_unreadable`].
    pub fn clinic_patients(&self) -> Result<Vec<(String, String)>, VaultError> {
        let mut stmt = self
            .identity
            .prepare("SELECT id, data_enc FROM clinic_patients ORDER BY created_at, id")?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, sealed)| {
                let json = open_string(
                    &self.keys.settings,
                    &aad("clinic_patients", "data", &id, ""),
                    &sealed,
                )
                .ok()?;
                Some((id, json))
            })
            .collect())
    }

    /// How many cards that were erased had been opened in each month, as the caller's own
    /// JSON (D-179); `None`: no card was erased yet.
    pub fn clinic_erased_cards(&self) -> Result<Option<String>, VaultError> {
        let sealed: Option<Vec<u8>> = self
            .identity
            .query_row("SELECT data_enc FROM clinic_erased WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        sealed
            .map(|s| {
                open_string(
                    &self.keys.settings,
                    &aad("clinic_erased", "data", "1", ""),
                    &s,
                )
            })
            .transpose()
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

    /// What the vault knows of a card beside its sealed JSON: when it was last written
    /// (`updated_at`, seconds) and whether an erase already counted it among the erased cards
    /// (D-179). `None`: no such card.
    pub fn clinic_patient_state(&self, id: &str) -> Result<Option<(i64, bool)>, VaultError> {
        Ok(self
            .identity
            .query_row(
                "SELECT updated_at, erase_counted FROM clinic_patients WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? != 0)),
            )
            .optional()?)
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

    /// Erase a patient: the card, every record of theirs and the history of those records,
    /// out of both write-ahead logs. One audit event with the number of records (a number
    /// only).
    ///
    /// What the erased records counted for in the summaries is not lost (D-179): `tally` is
    /// the whole anonymous tally of the main database with these records folded in, and
    /// `cards` the count of erased cards by the month they were opened in; both are the
    /// caller's JSON and hold no id, name or text. Each is written in the same transaction
    /// as the rows it stands for: the tally with the records (main database), the count
    /// with the card (identity database). The two databases cannot share one transaction;
    /// if the second fails, the card is left with no records, counts for itself as before,
    /// and asking again finishes the erase without counting anything twice. So that asking
    /// again still counts the card once, a `cards` given marks the card (`erase_counted`, see
    /// [`Vault::clinic_patient_state`]) before its records go: the caller passes `cards` again
    /// for a card so marked, even when nothing is left in it.
    ///
    /// Only history that belongs to the records being erased goes: the history of this
    /// patient's records, and the rows left by this patient's records that were removed
    /// earlier. A row of a record that now belongs to another card is never touched.
    pub fn erase_clinic_patient(
        &mut self,
        id: &str,
        tally: Option<&str>,
        cards: Option<&str>,
    ) -> Result<(), VaultError> {
        let known: Option<String> = self
            .identity
            .query_row("SELECT id FROM clinic_patients WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if known.is_none() {
            return Err(VaultError::NotFound);
        }
        let key = &self.keys.settings;
        if cards.is_some() {
            self.identity.execute(
                "UPDATE clinic_patients SET erase_counted = 1 WHERE id = ?1",
                [id],
            )?;
        }
        // The records first: if the card's removal fails, the erase can be asked again.
        let tx = self.main.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM clinic_history
             WHERE record_id IN (SELECT id FROM clinic_records WHERE patient_id = ?1)
                OR (patient_id = ?1 AND record_id NOT IN (SELECT id FROM clinic_records))",
            [id],
        )?;
        let records = tx.execute("DELETE FROM clinic_records WHERE patient_id = ?1", [id])?;
        if let Some(json) = tally {
            let sealed = seal_str(key, &record_aad(TALLY_ID, TALLY, None), json)?;
            tx.execute(
                "INSERT INTO clinic_records (id, kind, patient_id, day, updated_at, data_enc)
                 VALUES (?1, ?2, NULL, 0, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET updated_at = ?3, data_enc = ?4
                 WHERE kind = ?2",
                params![TALLY_ID, TALLY, now(), sealed],
            )?;
        }
        tx.commit()?;
        let tx = self.identity.unchecked_transaction()?;
        tx.execute("DELETE FROM clinic_patients WHERE id = ?1", [id])?;
        if let Some(json) = cards {
            let sealed = seal_str(key, &aad("clinic_erased", "data", "1", ""), json)?;
            tx.execute(
                "INSERT INTO clinic_erased (id, updated_at, data_enc) VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET updated_at = ?1, data_enc = ?2",
                params![now(), sealed],
            )?;
        }
        tx.commit()?;
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
