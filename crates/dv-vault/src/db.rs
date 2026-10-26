//! SQLCipher databases (whole-file encryption) with versioned migrations.

use std::path::Path;
use std::sync::Once;

use rusqlite::Connection;
use zeroize::Zeroizing;

use crate::crypto::{hex, Key32};
use crate::VaultError;

/// SQLCipher registers `sqlcipher_export` (which backups use) from SQLite's extra-init hook,
/// after SQLite has already marked itself initialized. A database opened on another thread in
/// that window never gets the function, and its backups fail with "no such function:
/// sqlcipher_export". The process's first connection is therefore opened alone.
static SQLCIPHER_READY: Once = Once::new();

/// Open (or create) an encrypted database with a raw 256-bit key (no SQLCipher KDF).
pub fn open_encrypted(path: &Path, key: &Key32) -> Result<Connection, VaultError> {
    SQLCIPHER_READY.call_once(|| {
        // Opening any connection runs SQLite's (and SQLCipher's) initialization to the end.
        drop(Connection::open_in_memory());
    });
    let conn = Connection::open(path)?;
    quiet_sqlcipher_log(&conn)?;
    let key_hex = Zeroizing::new(hex(key.as_bytes()));
    let statement = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", key_hex.as_str()));
    conn.execute_batch(&statement)?;
    conn.execute_batch(
        "PRAGMA cipher_memory_security = ON;
         PRAGMA secure_delete = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = ON;",
    )?;
    // Reading the schema fails if the key is wrong or the file is not ours.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map_err(|_| VaultError::WrongSecret)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Ok(conn)
}

/// SQLCipher logs warnings to stderr by default. On Windows, with `cipher_memory_security`,
/// every allocation is locked in RAM (`VirtualLock`); once the process's lock quota is used
/// up, the failure is logged, the log line is converted to UTF-16 in memory allocated through
/// the same locking allocator, that lock fails and is logged too, and so on until the stack
/// overflows (sqlcipher/sqlcipher#619). The program vanished on Windows the moment a vault was
/// created or opened. Logging off breaks the loop; memory security stays on, so freed memory
/// is still wiped. The program has no console, so nothing that was ever seen is lost.
/// Global in SQLCipher, and set before anything else on every connection, so it is in place
/// before memory security is first turned on.
fn quiet_sqlcipher_log(conn: &Connection) -> Result<(), VaultError> {
    let level: String = conn.query_row("PRAGMA cipher_log_level = NONE", [], |r| r.get(0))?;
    if level != "NONE" {
        return Err(VaultError::Crypto("sqlcipher log level"));
    }
    Ok(())
}

/// Apply migrations in order. Each entry is one schema version; never edit a released one.
pub fn migrate(conn: &Connection, migrations: &[&str]) -> Result<(), VaultError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)")?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;
    for (index, sql) in migrations.iter().enumerate() {
        let version = i64::try_from(index + 1).unwrap_or(i64::MAX);
        if version > current {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [version],
            )?;
            tx.commit()?;
        }
    }
    Ok(())
}

pub const MAIN_MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE cases (
        id TEXT PRIMARY KEY,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        wrapped_case_key BLOB NOT NULL,
        meta_enc BLOB NOT NULL
     );
     CREATE TABLE inputs (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        title_enc BLOB NOT NULL,
        content_enc BLOB NOT NULL
     );
     CREATE TABLE messages (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        section_key TEXT NOT NULL,
        role TEXT NOT NULL,
        demo INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        text_tagged_enc BLOB NOT NULL,
        hidden_enc BLOB NOT NULL
     );
     CREATE TABLE drafts (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        section_key TEXT NOT NULL,
        position INTEGER NOT NULL,
        status TEXT NOT NULL,
        author TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        approved_at INTEGER,
        text_tagged_enc BLOB NOT NULL,
        source_refs_enc BLOB NOT NULL
     );
     CREATE TABLE transmissions (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        section_key TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        payload_sha256 TEXT NOT NULL,
        model TEXT NOT NULL,
        payload_tagged_enc BLOB NOT NULL
     );
     CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE secrets (key TEXT PRIMARY KEY, value_enc BLOB NOT NULL);
     CREATE INDEX inputs_case ON inputs(case_id);
     CREATE INDEX messages_case ON messages(case_id, section_key);
     CREATE INDEX drafts_case ON drafts(case_id, section_key);",
    // v2: structured data behind a material (a score sheet), sealed like the rest of the row.
    "ALTER TABLE inputs ADD COLUMN data_enc BLOB;",
    // v3: which report sections a material feeds (D-022), sealed like the rest of the row.
    "ALTER TABLE inputs ADD COLUMN routing_enc BLOB;",
    // v4: folders (names sealed with the settings key) and the recycle bin.
    "CREATE TABLE folders (
        id TEXT PRIMARY KEY,
        parent_id TEXT REFERENCES folders(id),
        created_at INTEGER NOT NULL,
        name_enc BLOB NOT NULL
     );
     ALTER TABLE cases ADD COLUMN folder_id TEXT;
     ALTER TABLE cases ADD COLUMN deleted_at INTEGER;",
    // v5: saved consultations. The turns are sealed with the case key (general ones with the
    // settings key), so a case's conversations are erased with the case.
    "CREATE TABLE consultations (
        id TEXT PRIMARY KEY,
        case_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        turns_enc BLOB NOT NULL
     );
     CREATE INDEX consultations_updated ON consultations(updated_at);",
    // v6: Claude's new wording of a paragraph she approved waits beside it; approving it
    // retires the old one (D-032).
    "ALTER TABLE drafts ADD COLUMN replaces TEXT;",
    // v7: the writing-style profile (D-043). Past reports are kept only as neutralized text;
    // nothing here belongs to a case, so all of it is sealed with the settings key.
    "CREATE TABLE style_sources (
        id TEXT PRIMARY KEY,
        created_at INTEGER NOT NULL,
        data_enc BLOB NOT NULL
     );
     CREATE TABLE style_profiles (
        id TEXT PRIMARY KEY,
        version INTEGER NOT NULL,
        status TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        data_enc BLOB NOT NULL
     );
     CREATE TABLE style_learning (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        data_enc BLOB NOT NULL
     );",
    // v8: earlier wordings of a paragraph, kept when it is edited or reworded in place, so
    // she can see them and bring one back (D-046). Sealed with the case key like the draft.
    "CREATE TABLE draft_versions (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        draft_id TEXT NOT NULL,
        saved_at INTEGER NOT NULL,
        author TEXT NOT NULL,
        text_tagged_enc BLOB NOT NULL
     );
     CREATE INDEX draft_versions_draft ON draft_versions(case_id, draft_id);",
    // v9: the name model's person spans for a material (R4), so it reads each text once, not
    // once per unlock. Keyed by a keyed hash of the model revision and the text; offsets only.
    "CREATE TABLE name_hints (
        key TEXT PRIMARY KEY,
        case_id TEXT NOT NULL,
        hints TEXT NOT NULL
     );
     CREATE INDEX name_hints_case ON name_hints(case_id);",
    // v10: the case's mask book (round 25): what each numbered tag of a number, a date, an
    // address or a place hid, so the answer comes back as she wrote it. The words are sealed
    // with the case key; the tag and its kind name nothing.
    "CREATE TABLE masks (
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        tag TEXT NOT NULL,
        kind TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        original_enc BLOB NOT NULL,
        PRIMARY KEY (case_id, tag)
     );",
    // v11: "what left the computer" is read per case; an index instead of a full scan as the
    // history grows (deep review 3.5). How long the text itself is kept waits for the legal
    // review (P-07), so nothing is deleted.
    "CREATE INDEX transmissions_case ON transmissions(case_id, created_at);",
];

pub const IDENTITY_MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE identities (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL,
        role TEXT NOT NULL,
        tag TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        value_enc BLOB NOT NULL,
        aliases_enc BLOB NOT NULL
     );
     CREATE TABLE decisions (
        id TEXT PRIMARY KEY,
        case_id TEXT,
        token_hmac TEXT NOT NULL,
        decision TEXT NOT NULL,
        tag TEXT,
        created_at INTEGER NOT NULL
     );
     CREATE TABLE practitioner (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        value_enc BLOB NOT NULL
     );
     CREATE INDEX identities_case ON identities(case_id);
     CREATE UNIQUE INDEX decisions_token ON decisions(COALESCE(case_id, ''), token_hmac);",
    // v2: names the filter hid on its own (`auto`) or took from a file's properties
    // (`metadata`), with the reason shown next to them.
    "ALTER TABLE identities ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
     ALTER TABLE identities ADD COLUMN reason TEXT NOT NULL DEFAULT '';",
];

pub const AUDIT_MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE audit (
        seq INTEGER PRIMARY KEY,
        ts INTEGER NOT NULL,
        event TEXT NOT NULL,
        case_ref TEXT,
        meta TEXT NOT NULL,
        prev_mac TEXT NOT NULL,
        mac TEXT NOT NULL
     );",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// R7: a version that cannot open the vault is replaced by the one before it. That only
    /// works while every migration adds and never changes or drops, so the older code still
    /// finds every table and column it reads.
    #[test]
    fn migrations_only_add_so_the_version_before_still_opens_the_vault() {
        for list in [MAIN_MIGRATIONS, IDENTITY_MIGRATIONS, AUDIT_MIGRATIONS] {
            for sql in list {
                for stmt in sql
                    .split(';')
                    .map(str::trim)
                    .filter(|s: &&str| !s.is_empty())
                {
                    let upper = stmt.to_uppercase();
                    let adds = upper.starts_with("CREATE TABLE")
                        || upper.starts_with("CREATE INDEX")
                        || upper.starts_with("CREATE UNIQUE INDEX")
                        || (upper.starts_with("ALTER TABLE") && upper.contains(" ADD COLUMN "));
                    assert!(adds, "a migration that does not only add: {stmt}");
                }
            }
        }
    }

    /// Every migration's SHA-256 (first 16 hex digits), in order. A released one never
    /// changes: a vault that already ran it would not run the new text (F7). A new migration
    /// goes at the end of its list, and its hash here, in the same change.
    const PINNED: &[(&str, &[&str])] = &[
        (
            "main",
            &[
                "c7185f7c0b2c3359",
                "0875175f7b3fe0fa",
                "c69d16e6c5753077",
                "5af9a553384f7c52",
                "811bb7b6e88a80d7",
                "3f439fe33652efeb",
                "f2a949fd4a472698",
                "c450369d36db7b57",
                "319cc0d373ae4523",
                "942e02ced273dca6",
                "860fa1cbe9c35c6b",
            ],
        ),
        ("identity", &["98e79c9e2d976424", "59aa5787b56ebc78"]),
        ("audit", &["922ed57e6a2659e3"]),
    ];

    #[test]
    fn a_migration_once_written_never_changes() {
        let now: Vec<(&str, Vec<String>)> = [
            ("main", MAIN_MIGRATIONS),
            ("identity", IDENTITY_MIGRATIONS),
            ("audit", AUDIT_MIGRATIONS),
        ]
        .into_iter()
        .map(|(name, list)| {
            let hashes = list
                .iter()
                .map(|sql| {
                    crate::crypto::sha256_hex(sql.as_bytes())
                        .chars()
                        .take(16)
                        .collect()
                })
                .collect();
            (name, hashes)
        })
        .collect();
        for ((name, hashes), (pinned_name, pinned)) in now.iter().zip(PINNED) {
            assert_eq!(name, pinned_name);
            for (i, (h, p)) in hashes.iter().zip(pinned.iter()).enumerate() {
                assert_eq!(
                    h,
                    p,
                    "{name} migration {} changed after it was written: add a new one at the end",
                    i + 1
                );
            }
            assert_eq!(
                hashes.len(),
                pinned.len(),
                "{name}: pin every migration here ({hashes:?})"
            );
        }
    }

    #[test]
    fn a_vault_from_a_newer_version_opens_and_keeps_what_it_added() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let conn = open_encrypted(&dir.path().join("t.db"), &Key32::from_bytes([4; 32]))
            .unwrap_or_else(|_| unreachable!());
        migrate(&conn, MAIN_MIGRATIONS).unwrap_or_else(|_| unreachable!());
        let later = i64::try_from(MAIN_MIGRATIONS.len() + 1).unwrap_or(i64::MAX);
        conn.execute_batch("CREATE TABLE from_the_future (x INTEGER);")
            .unwrap_or_else(|_| unreachable!());
        conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [later])
            .unwrap_or_else(|_| unreachable!());
        assert!(migrate(&conn, MAIN_MIGRATIONS).is_ok());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM from_the_future", [], |r| r.get(0))
            .unwrap_or(-1);
        assert_eq!(n, 0, "the newer table is still there");
    }

    /// The Windows crash (D-038) needs SQLCipher's log to be off before memory security is on.
    /// Windows CI proves the crash is gone; this keeps the line from being dropped elsewhere.
    #[test]
    fn sqlcipher_logging_is_off_with_memory_security_on() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let conn = open_encrypted(&dir.path().join("t.db"), &Key32::from_bytes([3; 32]))
            .unwrap_or_else(|_| unreachable!());
        let level: String = conn
            .query_row("PRAGMA cipher_log_level", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(level, "NONE");
        let security: String = conn
            .query_row("PRAGMA cipher_memory_security", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(security, "1");
    }
}
