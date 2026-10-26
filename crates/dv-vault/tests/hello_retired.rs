//! Windows Hello was taken out (D-054): a vault that had its slot keeps opening with the
//! password, and the slot leaves the header. Fabricated data only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use dv_vault::{Argon2Params, Vault};

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

#[test]
fn a_vault_with_an_old_hello_slot_opens_with_the_password_and_loses_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    Vault::create(dir.path(), PASSWORD, Argon2Params::TEST)
        .unwrap()
        .vault
        .lock()
        .unwrap();
    // A header as an older version wrote it, with Windows Hello turned on. The slot is added
    // without the master key, so the header's seal no longer matches, as after any outside edit.
    let path = dir.path().join("vault.header");
    let mut header: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    header["slots"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "kind": "windows_hello",
            "credential": "il.diagnosticvault.test",
            "challenge": "00",
            "wrapped_mk": "00"
        }));
    std::fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();

    let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(vault.has_hello_slot());
    vault.remove_hello_slot().unwrap();
    assert!(!vault.has_hello_slot());
    vault.lock().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("windows_hello"));
    // Edited from outside here, so it stays flagged: a changed header is never sealed again
    // (D-104, review of #153). A header an older version really signed stays sealed
    // (`store::version_tests::a_retired_hello_slot_leaves_a_signed_header_signed`).
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(
        !vault.integrity().header_ok,
        "an outside edit was sealed again"
    );
}
