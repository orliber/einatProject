//! Tests that need to reach under the vault's API, into the rows inside the encrypted
//! databases: what someone who got hold of the database keys (but not the case keys or the
//! settings key) could do by moving sealed cells around. Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use dv_domain::{CaseMeta, IdentityInput, InputKind, Role};
use rusqlite::{params, Connection};

use super::{Practitioner, Vault};
use crate::password::Argon2Params;
use crate::VaultError;

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

/// `version ‖ nonce(12) ‖ tag(16)` at least.
const MIN_SEALED: usize = 1 + 12 + 16;

fn new_vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST)
        .unwrap()
        .vault;
    (dir, vault)
}

fn case_with_input(vault: &mut Vault, code: &str) -> (String, String) {
    let case = vault
        .create_case(&CaseMeta {
            code: code.into(),
            ..CaseMeta::default()
        })
        .unwrap();
    vault
        .set_identities(
            &case,
            &[IdentityInput {
                id: None,
                role: Role::Child,
                value: format!("ילד בדוי {code}"),
                aliases: vec![],
            }],
        )
        .unwrap();
    let input = vault
        .add_input(&case, InputKind::Kindergarten, "כותרת בדויה", "תוכן בדוי")
        .unwrap();
    (case, input.id)
}

fn blob(conn: &Connection, sql: &str, id: &str) -> Vec<u8> {
    conn.query_row(sql, [id], |r| r.get(0)).unwrap()
}

/// The AAD binds every sealed cell to its table, column, row and case: copying a cell into
/// another column, another row or another case makes the read fail; it never shows the
/// moved text in the wrong place.
#[test]
fn sealed_cells_cannot_be_moved_between_columns_rows_or_cases() {
    let (_dir, mut vault) = new_vault();
    let (a, input_a) = case_with_input(&mut vault, "TEST-A");
    let (b, input_b) = case_with_input(&mut vault, "TEST-B");
    let second_a = vault
        .add_input(&a, InputKind::Kindergarten, "עוד כותרת", "עוד תוכן")
        .unwrap()
        .id;
    let title = blob(
        &vault.main,
        "SELECT title_enc FROM inputs WHERE id = ?1",
        &input_a,
    );
    let content = blob(
        &vault.main,
        "SELECT content_enc FROM inputs WHERE id = ?1",
        &input_a,
    );
    let restore = |v: &Vault| {
        v.main
            .execute(
                "UPDATE inputs SET title_enc = ?1, content_enc = ?2 WHERE id = ?3",
                params![title, content, input_a],
            )
            .unwrap();
    };

    // Another column of the same row.
    vault
        .main
        .execute(
            "UPDATE inputs SET title_enc = content_enc WHERE id = ?1",
            [&input_a],
        )
        .unwrap();
    assert!(matches!(vault.inputs(&a), Err(VaultError::Authentication)));
    restore(&vault);

    // Another row of the same case (same case key).
    vault
        .main
        .execute(
            "UPDATE inputs SET content_enc = (SELECT content_enc FROM inputs WHERE id = ?1) WHERE id = ?2",
            [&second_a, &input_a],
        )
        .unwrap();
    assert!(matches!(vault.inputs(&a), Err(VaultError::Authentication)));
    restore(&vault);

    // Another case's row.
    vault
        .main
        .execute(
            "UPDATE inputs SET content_enc = (SELECT content_enc FROM inputs WHERE id = ?1) WHERE id = ?2",
            [&input_b, &input_a],
        )
        .unwrap();
    assert!(matches!(vault.inputs(&a), Err(VaultError::Authentication)));
    restore(&vault);
    assert_eq!(
        vault.inputs(&a).unwrap().len(),
        2,
        "restored rows open again"
    );

    // A whole row re-filed under another case.
    vault
        .main
        .execute(
            "UPDATE inputs SET case_id = ?1 WHERE id = ?2",
            [&b, &input_a],
        )
        .unwrap();
    assert!(vault.inputs(&b).is_err());
    vault
        .main
        .execute(
            "UPDATE inputs SET case_id = ?1 WHERE id = ?2",
            [&a, &input_a],
        )
        .unwrap();

    // A name re-filed under another case (identities live in their own database).
    let identity_a: String = vault
        .identity
        .query_row("SELECT id FROM identities WHERE case_id = ?1", [&a], |r| {
            r.get(0)
        })
        .unwrap();
    vault
        .identity
        .execute(
            "UPDATE identities SET case_id = ?1 WHERE id = ?2",
            [&b, &identity_a],
        )
        .unwrap();
    assert!(vault.identities(&b).is_err());
}

/// A case key is wrapped for its own case id: case B given case A's wrapped key (and sealed
/// metadata) does not open, so one case's key can never be used to read another's rows.
#[test]
fn a_wrapped_case_key_cannot_be_given_to_another_case() {
    let (_dir, mut vault) = new_vault();
    let (a, _) = case_with_input(&mut vault, "TEST-A");
    let (b, _) = case_with_input(&mut vault, "TEST-B");
    vault
        .main
        .execute(
            "UPDATE cases SET wrapped_case_key = (SELECT wrapped_case_key FROM cases WHERE id = ?1),
                              meta_enc = (SELECT meta_enc FROM cases WHERE id = ?1) WHERE id = ?2",
            [&a, &b],
        )
        .unwrap();
    assert!(matches!(
        vault.case_meta(&b),
        Err(VaultError::Authentication)
    ));
    assert!(vault.inputs(&b).is_err());
    assert_eq!(vault.case_meta(&a).unwrap().code, "TEST-A");
}

/// Settings-key cells share one key, so only the AAD tells them apart: a secret moved under
/// another name, into the practitioner row, or into a folder name, is refused.
#[test]
fn cells_under_the_settings_key_are_told_apart_by_their_aad() {
    let (_dir, mut vault) = new_vault();
    vault.set_secret("api_key", "sk-בדוי-1").unwrap();
    vault.set_secret("other_key", "sk-בדוי-2").unwrap();
    vault
        .set_practitioner(&Practitioner {
            names: vec!["מטפלת בדויה".into()],
        })
        .unwrap();
    let folder = vault.create_folder(None, "תיקייה בדויה").unwrap();
    let secret = blob(
        &vault.main,
        "SELECT value_enc FROM secrets WHERE key = ?1",
        "api_key",
    );

    vault
        .main
        .execute(
            "UPDATE secrets SET value_enc = ?1 WHERE key = 'other_key'",
            [&secret],
        )
        .unwrap();
    assert!(matches!(
        vault.secret("other_key"),
        Err(VaultError::Authentication)
    ));
    vault
        .identity
        .execute(
            "UPDATE practitioner SET value_enc = ?1 WHERE id = 1",
            [&secret],
        )
        .unwrap();
    assert!(matches!(
        vault.practitioner(),
        Err(VaultError::Authentication)
    ));
    vault
        .main
        .execute(
            "UPDATE folders SET name_enc = ?1 WHERE id = ?2",
            params![secret, folder.id],
        )
        .unwrap();
    assert!(matches!(vault.folders(), Err(VaultError::Authentication)));
    assert_eq!(
        vault.secret("api_key").unwrap().unwrap().as_str(),
        "sk-בדוי-1"
    );
}

/// Writing the same value again and again never reuses a nonce (GCM's one hard rule), and
/// every rewrite produces a different cell, so equal values cannot be spotted by comparing
/// cells.
#[test]
fn rewriting_the_same_value_never_reuses_a_nonce() {
    let (_dir, mut vault) = new_vault();
    let (case, input) = case_with_input(&mut vault, "TEST-N");
    let mut nonces = HashSet::new();
    let mut cells = HashSet::new();
    for _ in 0..100 {
        vault.set_secret("api_key", "אותו ערך").unwrap();
        vault
            .update_input(&case, &input, "אותו ערך", "אותו ערך")
            .unwrap();
        let mut sealed = vec![blob(
            &vault.main,
            "SELECT value_enc FROM secrets WHERE key = ?1",
            "api_key",
        )];
        sealed.push(blob(
            &vault.main,
            "SELECT title_enc FROM inputs WHERE id = ?1",
            &input,
        ));
        sealed.push(blob(
            &vault.main,
            "SELECT content_enc FROM inputs WHERE id = ?1",
            &input,
        ));
        for cell in sealed {
            assert!(nonces.insert(cell[1..13].to_vec()), "nonce reused");
            assert!(cells.insert(cell), "identical cell written twice");
        }
    }
}

/// Every BLOB in every table of the main and identity databases is a sealed envelope: no
/// column stores bytes outside a seal (a guard for columns added in later migrations).
#[test]
fn every_stored_blob_is_a_sealed_envelope() {
    let (_dir, mut vault) = new_vault();
    let (case, input) = case_with_input(&mut vault, "TEST-E");
    vault.set_input_data(&case, &input, "{}").unwrap();
    vault.set_input_routing(&case, &input, "[]").unwrap();
    vault
        .add_message(&case, "s", dv_domain::ChatRole::User, "טקסט", &[], false)
        .unwrap();
    let draft = vault
        .add_draft(&case, "s", "טיוטה", dv_domain::Author::Ai, &[])
        .unwrap();
    vault.edit_draft(&case, &draft.id, "טיוטה מתוקנת").unwrap();
    vault
        .add_transmission(&case, "s", "00", "model", "מטען")
        .unwrap();
    vault.save_consultation(None, Some(&case), "[]").unwrap();
    vault.create_folder(None, "תיקייה").unwrap();
    vault.set_secret("api_key", "x").unwrap();
    vault
        .set_practitioner(&Practitioner {
            names: vec!["שם".into()],
        })
        .unwrap();
    vault.save_style_source(None, "{}").unwrap();
    vault.add_style_profile("draft", "{}").unwrap();

    let mut checked = 0;
    for conn in [&vault.main, &vault.identity] {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for table in tables {
            let blob_columns: Vec<String> = conn
                .prepare(&format!(
                    "SELECT name, type FROM pragma_table_info('{table}')"
                ))
                .unwrap()
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .into_iter()
                .filter(|(_, t)| t.eq_ignore_ascii_case("BLOB"))
                .map(|(n, _)| n)
                .collect();
            for column in blob_columns {
                let values: Vec<Option<Vec<u8>>> = conn
                    .prepare(&format!("SELECT {column} FROM {table}"))
                    .unwrap()
                    .query_map([], |r| r.get(0))
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap();
                for v in values.into_iter().flatten() {
                    assert!(
                        v.len() >= MIN_SEALED && v[0] == 1,
                        "{table}.{column} holds an unsealed value"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 20, "only {checked} cells checked");
}
