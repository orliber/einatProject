//! TEMPORARY probe: which step of opening an encrypted database recurses without end on
//! Windows (STATUS_STACK_OVERFLOW). Each test is run alone in CI, in its own process.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rusqlite::Connection;

fn work(conn: &Connection) {
    conn.execute_batch("CREATE TABLE t (x TEXT); INSERT INTO t VALUES ('a');")
        .unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

const KEY: &str =
    "PRAGMA key = \"x'0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20'\";";

#[test]
fn p1_plain_in_memory() {
    work(&Connection::open_in_memory().unwrap());
}

#[test]
fn p2_plain_file() {
    let dir = tempfile::tempdir().unwrap();
    work(&Connection::open(dir.path().join("a.db")).unwrap());
}

#[test]
fn p3_keyed_file() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("a.db")).unwrap();
    conn.execute_batch(KEY).unwrap();
    work(&conn);
}

#[test]
fn p4_keyed_file_memory_security() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("a.db")).unwrap();
    conn.execute_batch(KEY).unwrap();
    conn.execute_batch("PRAGMA cipher_memory_security = ON;")
        .unwrap();
    work(&conn);
}

#[test]
fn p5_keyed_file_secure_delete_temp_memory() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("a.db")).unwrap();
    conn.execute_batch(KEY).unwrap();
    conn.execute_batch(
        "PRAGMA secure_delete = ON; PRAGMA temp_store = MEMORY; PRAGMA foreign_keys = ON;",
    )
    .unwrap();
    work(&conn);
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    work_more(&conn);
}

fn work_more(conn: &Connection) {
    conn.execute_batch("INSERT INTO t VALUES ('b');").unwrap();
}

#[test]
fn p6_aws_lc_then_keyed_file() {
    let mut b = [0u8; 32];
    aws_lc_rs::rand::fill(&mut b).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("a.db")).unwrap();
    conn.execute_batch(KEY).unwrap();
    work(&conn);
}

#[test]
fn p7_all_open_pragmas_like_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("a.db")).unwrap();
    conn.execute_batch(KEY).unwrap();
    conn.execute_batch(
        "PRAGMA cipher_memory_security = ON; PRAGMA secure_delete = ON; PRAGMA temp_store = MEMORY; PRAGMA foreign_keys = ON;",
    )
    .unwrap();
    let _: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    work(&conn);
}

#[test]
fn p8_vault_create() {
    let dir = tempfile::tempdir().unwrap();
    dv_vault::Vault::create(
        dir.path(),
        "סוס ירוק רץ בשדה 2026",
        dv_vault::Argon2Params::TEST,
    )
    .unwrap();
}
