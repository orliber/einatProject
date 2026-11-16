//! The clinic's storage: round trips, ordering, the sealing, the all-or-nothing writes, the
//! erase of a patient, and survival of lock and backup. Invented data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::{Secret, Vault};
use crate::password::Argon2Params;
use crate::VaultError;

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

fn new_vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST)
        .unwrap()
        .vault;
    (dir, vault)
}

fn put(v: &mut Vault, kind: &str, patient: Option<&str>, day: i64, json: &str) -> String {
    v.clinic_put(kind, None, patient, day, json, None).unwrap()
}

#[test]
fn a_record_round_trips_and_can_be_replaced() {
    let (_d, mut v) = new_vault();
    let id = v
        .clinic_put("visit", None, Some("p1"), 20000, r#"{"x":"דנה"}"#, None)
        .unwrap();
    let got = v.clinic_get(&id).unwrap().unwrap();
    assert_eq!(got.kind, "visit");
    assert_eq!(got.patient_id.as_deref(), Some("p1"));
    assert_eq!(got.day, 20000);
    assert_eq!(got.json, r#"{"x":"דנה"}"#);
    let again = v
        .clinic_put(
            "visit",
            Some(&id),
            Some("p1"),
            20001,
            r#"{"x":"שני"}"#,
            None,
        )
        .unwrap();
    assert_eq!(again, id);
    let got = v.clinic_get(&id).unwrap().unwrap();
    assert_eq!((got.day, got.json.as_str()), (20001, r#"{"x":"שני"}"#));
    assert_eq!(v.clinic_kind("visit").unwrap().len(), 1);
    assert!(v.clinic_get("nope").unwrap().is_none());
}

#[test]
fn an_unknown_id_is_not_found_except_for_the_settings_record() {
    let (_d, mut v) = new_vault();
    let r = v.clinic_put("visit", Some("ghost"), None, 1, "{}", None);
    assert!(matches!(r, Err(VaultError::NotFound)));
    assert!(v.clinic_get("ghost").unwrap().is_none());
    let id = v
        .clinic_put("settings", Some("settings"), None, 0, r#"{"a":1}"#, None)
        .unwrap();
    assert_eq!(id, "settings");
    let id = v
        .clinic_put("settings", None, None, 0, r#"{"a":2}"#, None)
        .unwrap();
    assert_eq!(id, "settings");
    assert_eq!(v.clinic_kind("settings").unwrap().len(), 1);
    assert_eq!(
        v.clinic_get("settings").unwrap().unwrap().json,
        r#"{"a":2}"#
    );
    let other = v.clinic_put("settings", Some("other"), None, 0, "{}", None);
    assert!(matches!(other, Err(VaultError::NotFound)));
}

#[test]
fn kind_and_action_allowlists_and_kind_is_fixed() {
    let (_d, mut v) = new_vault();
    assert!(matches!(
        v.clinic_put("secret", None, None, 1, "{}", None),
        Err(VaultError::Refused(_))
    ));
    assert!(matches!(
        v.clinic_put("visit", None, None, 1, "{}", Some("edited")),
        Err(VaultError::Refused(_))
    ));
    assert!(v.clinic_kind("visit").unwrap().is_empty());
    for kind in ["visit", "note", "payment", "charge", "task"] {
        for action in ["created", "changed", "voided", "deleted"] {
            v.clinic_put(kind, None, None, 1, "{}", Some(action))
                .unwrap();
        }
    }
    let id = put(&mut v, "visit", None, 1, "{}");
    assert!(matches!(
        v.clinic_put("note", Some(&id), None, 1, "{}", None),
        Err(VaultError::Refused(_))
    ));
    assert_eq!(v.clinic_get(&id).unwrap().unwrap().kind, "visit");
}

#[test]
fn records_come_back_ordered_and_ranges_include_both_ends() {
    let (_d, mut v) = new_vault();
    let c = put(&mut v, "visit", Some("p1"), 30, "{}");
    let a = put(&mut v, "visit", Some("p1"), 10, "{}");
    let b = put(&mut v, "visit", Some("p2"), 20, "{}");
    let n = put(&mut v, "note", Some("p1"), 15, "{}");
    let days: Vec<i64> = v
        .clinic_kind("visit")
        .unwrap()
        .iter()
        .map(|r| r.day)
        .collect();
    assert_eq!(days, [10, 20, 30]);
    let range: Vec<String> = v
        .clinic_range("visit", 10, 20)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(range, [a.clone(), b]);
    assert_eq!(v.clinic_range("visit", 11, 29).unwrap().len(), 1);
    assert!(v.clinic_range("visit", 31, 40).unwrap().is_empty());
    let mine: Vec<String> = v
        .clinic_of_patient("p1")
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(mine, [a, n, c]);
    // Same day: by id.
    let x = put(&mut v, "task", None, 5, "{}");
    let y = put(&mut v, "task", None, 5, "{}");
    let mut want = [x, y];
    want.sort();
    let got: Vec<String> = v
        .clinic_kind("task")
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(got, want);
}

#[test]
fn the_sealed_cell_is_bound_to_its_row_and_hides_the_text() {
    let (_d, mut v) = new_vault();
    let a = put(
        &mut v,
        "note",
        Some("p1"),
        1,
        r#"{"text":"סימן-סודי-בדוי"}"#,
    );
    let b = put(&mut v, "note", Some("p1"), 2, "{}");
    let raw: Vec<u8> = v
        .main
        .query_row(
            "SELECT data_enc FROM clinic_records WHERE id = ?1",
            [&a],
            |r| r.get(0),
        )
        .unwrap();
    let needle = "סימן-סודי-בדוי".as_bytes();
    assert!(!raw.windows(needle.len()).any(|w| w == needle));
    v.main
        .execute(
            "UPDATE clinic_records SET data_enc = (SELECT data_enc FROM clinic_records WHERE id = ?1)
             WHERE id = ?2",
            [&a, &b],
        )
        .unwrap();
    assert!(v.clinic_get(&b).is_err());
    assert!(v.clinic_get(&a).unwrap().is_some());
}

#[test]
fn a_history_cell_and_a_patient_cell_are_bound_to_their_rows_too() {
    let (_d, mut v) = new_vault();
    let r = v
        .clinic_put(
            "payment",
            None,
            Some("p1"),
            1,
            r#"{"a":1}"#,
            Some("created"),
        )
        .unwrap();
    v.clinic_put(
        "payment",
        Some(&r),
        Some("p1"),
        1,
        r#"{"a":2}"#,
        Some("changed"),
    )
    .unwrap();
    v.main
        .execute(
            "UPDATE clinic_history SET data_enc = (SELECT data_enc FROM clinic_history
             WHERE action = 'created')
             WHERE action = 'changed'",
            [],
        )
        .unwrap();
    assert!(v.clinic_history(&r).is_err());
    let p1 = v.save_clinic_patient(None, r#"{"n":"אורי"}"#).unwrap();
    let p2 = v.save_clinic_patient(None, r#"{"n":"נועה"}"#).unwrap();
    v.identity
        .execute(
            "UPDATE clinic_patients SET data_enc = (SELECT data_enc FROM clinic_patients
             WHERE id = ?1) WHERE id = ?2",
            [&p1, &p2],
        )
        .unwrap();
    assert!(v.clinic_patient(&p2).is_err());
}

#[test]
fn put_many_is_all_or_nothing() {
    let (_d, mut v) = new_vault();
    let rows = vec![
        (None, Some("p1".to_owned()), 1, "{}".to_owned()),
        (None, Some("p1".to_owned()), 2, "{}".to_owned()),
        (
            Some("ghost".to_owned()),
            Some("p1".to_owned()),
            3,
            "{}".to_owned(),
        ),
    ];
    let r = v.clinic_put_many("visit", &rows, Some("created"));
    assert!(matches!(r, Err(VaultError::NotFound)));
    assert!(v.clinic_kind("visit").unwrap().is_empty());
    assert!(v.clinic_history_counts().unwrap().is_empty());
    let ok = v
        .clinic_put_many("visit", &rows[..2], Some("created"))
        .unwrap();
    assert_eq!(ok.len(), 2);
    assert_eq!(v.clinic_kind("visit").unwrap().len(), 2);
    assert_eq!(v.clinic_history_counts().unwrap().len(), 2);
}

#[test]
fn history_is_written_with_the_record_ordered_and_counted() {
    let (_d, mut v) = new_vault();
    let id = v
        .clinic_put("visit", None, Some("p1"), 1, r#"{"v":1}"#, Some("created"))
        .unwrap();
    v.clinic_put(
        "visit",
        Some(&id),
        Some("p1"),
        1,
        r#"{"v":2}"#,
        Some("changed"),
    )
    .unwrap();
    v.clinic_put(
        "visit",
        Some(&id),
        Some("p1"),
        1,
        r#"{"v":3}"#,
        Some("voided"),
    )
    .unwrap();
    let other = put(&mut v, "visit", Some("p1"), 2, "{}");
    let h = v.clinic_history(&id).unwrap();
    let seen: Vec<(&str, &str)> = h
        .iter()
        .map(|x| (x.action.as_str(), x.json.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("created", r#"{"v":1}"#),
            ("changed", r#"{"v":2}"#),
            ("voided", r#"{"v":3}"#)
        ]
    );
    assert!(v.clinic_history(&other).unwrap().is_empty());
    assert_eq!(v.clinic_history_counts().unwrap(), [(id, 3)]);
}

#[test]
fn delete_with_and_without_history() {
    let (_d, mut v) = new_vault();
    let a = v
        .clinic_put("visit", None, Some("p1"), 1, r#"{"last":true}"#, None)
        .unwrap();
    let b = put(&mut v, "visit", Some("p1"), 2, "{}");
    v.clinic_delete(&a, true).unwrap();
    assert!(v.clinic_get(&a).unwrap().is_none());
    let h = v.clinic_history(&a).unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(
        (h[0].action.as_str(), h[0].json.as_str()),
        ("deleted", r#"{"last":true}"#)
    );
    v.clinic_delete(&b, false).unwrap();
    assert!(v.clinic_history(&b).unwrap().is_empty());
    assert!(matches!(
        v.clinic_delete(&b, true),
        Err(VaultError::NotFound)
    ));
}

#[test]
fn delete_many_is_one_transaction() {
    let (_d, mut v) = new_vault();
    let a = put(&mut v, "visit", None, 1, "{}");
    let b = put(&mut v, "visit", None, 2, "{}");
    let r = v.clinic_delete_many(&[a.clone(), "ghost".to_owned()], true);
    assert!(matches!(r, Err(VaultError::NotFound)));
    assert!(v.clinic_get(&a).unwrap().is_some());
    assert!(v.clinic_history(&a).unwrap().is_empty());
    v.clinic_delete_many(&[a.clone(), b.clone()], true).unwrap();
    assert!(v.clinic_kind("visit").unwrap().is_empty());
    assert_eq!(v.clinic_history_counts().unwrap().len(), 2);
}

#[test]
fn patient_cards_round_trip_in_order() {
    let (_d, mut v) = new_vault();
    let a = v.save_clinic_patient(None, r#"{"n":"גל"}"#).unwrap();
    let b = v.save_clinic_patient(None, r#"{"n":"רון"}"#).unwrap();
    assert_eq!(
        v.clinic_patient(&a).unwrap().as_deref(),
        Some(r#"{"n":"גל"}"#)
    );
    v.save_clinic_patient(Some(&a), r#"{"n":"גל ב"}"#).unwrap();
    assert_eq!(
        v.clinic_patient(&a).unwrap().as_deref(),
        Some(r#"{"n":"גל ב"}"#)
    );
    assert!(v.clinic_patient("ghost").unwrap().is_none());
    assert!(matches!(
        v.save_clinic_patient(Some("ghost"), "{}"),
        Err(VaultError::NotFound)
    ));
    let ids: Vec<String> = v
        .clinic_patients()
        .unwrap()
        .into_iter()
        .map(|p| p.0)
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&a) && ids.contains(&b));
    let raw: Vec<u8> = v
        .identity
        .query_row(
            "SELECT data_enc FROM clinic_patients WHERE id = ?1",
            [&b],
            |r| r.get(0),
        )
        .unwrap();
    let needle = "רון".as_bytes();
    assert!(!raw.windows(needle.len()).any(|w| w == needle));
    assert!(v.audit_intact().unwrap());
}

fn audit_rows(v: &Vault, event: &str) -> Vec<String> {
    let mut stmt = v
        .audit
        .prepare("SELECT meta FROM audit WHERE event = ?1 ORDER BY seq")
        .unwrap();
    stmt.query_map([event], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn erasing_a_patient_removes_theirs_and_nothing_else() {
    let (_d, mut v) = new_vault();
    let p1 = v.save_clinic_patient(None, r#"{"n":"אורי"}"#).unwrap();
    let p2 = v.save_clinic_patient(None, r#"{"n":"נועה"}"#).unwrap();
    for day in 1..=2 {
        v.clinic_put("visit", None, Some(&p1), day, "{}", Some("created"))
            .unwrap();
    }
    v.clinic_put("note", None, Some(&p1), 3, "{}", None)
        .unwrap();
    let theirs = v
        .clinic_put("payment", None, Some(&p2), 4, "{}", Some("created"))
        .unwrap();
    let block = put(&mut v, "visit", None, 5, "{}");
    // A record of p1 deleted earlier leaves history that the erase must take too.
    let gone = v
        .clinic_put("visit", None, Some(&p1), 6, "{}", None)
        .unwrap();
    v.clinic_delete(&gone, true).unwrap();
    v.erase_clinic_patient(&p1, None, None).unwrap();
    assert!(v.clinic_of_patient(&p1).unwrap().is_empty());
    assert!(v.clinic_patient(&p1).unwrap().is_none());
    assert!(v.clinic_patient(&p2).unwrap().is_some());
    assert_eq!(v.clinic_of_patient(&p2).unwrap().len(), 1);
    assert!(v.clinic_get(&block).unwrap().is_some());
    assert_eq!(v.clinic_history(&theirs).unwrap().len(), 1);
    let left: i64 = v
        .main
        .query_row(
            "SELECT COUNT(*) FROM clinic_history WHERE patient_id = ?1",
            [&p1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0);
    assert_eq!(
        audit_rows(&v, "clinic_patient_erased"),
        [r#"{"records":3}"#]
    );
    assert_eq!(audit_rows(&v, "clinic_patient_added").len(), 2);
    assert!(audit_rows(&v, "clinic_patient_added")
        .iter()
        .all(|m| m == "{}"));
    assert!(matches!(
        v.erase_clinic_patient(&p1, None, None),
        Err(VaultError::NotFound)
    ));
    assert!(v.audit_intact().unwrap());
}

#[test]
fn clinic_data_survives_lock_and_unlock() {
    let (dir, mut v) = new_vault();
    let p = v.save_clinic_patient(None, r#"{"n":"מיכל"}"#).unwrap();
    let id = v
        .clinic_put("visit", None, Some(&p), 7, r#"{"a":1}"#, Some("created"))
        .unwrap();
    v.lock().unwrap();
    let v = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert_eq!(v.clinic_get(&id).unwrap().unwrap().json, r#"{"a":1}"#);
    assert_eq!(v.clinic_history(&id).unwrap().len(), 1);
    assert_eq!(
        v.clinic_patient(&p).unwrap().as_deref(),
        Some(r#"{"n":"מיכל"}"#)
    );
    assert!(v.audit_intact().unwrap());
}

#[test]
fn clinic_data_survives_a_backup_and_restore() {
    let (dir, mut v) = new_vault();
    let p = v.save_clinic_patient(None, r#"{"n":"תמר"}"#).unwrap();
    let id = v
        .clinic_put(
            "payment",
            None,
            Some(&p),
            9,
            r#"{"sum":100}"#,
            Some("created"),
        )
        .unwrap();
    let out = dir.path().join("b.vaultbak");
    v.write_backup(&out).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let target = tempfile::tempdir().unwrap();
    let (r, _) = Vault::restore_backup(&bytes, target.path(), Secret::Password(PASSWORD)).unwrap();
    assert_eq!(r.clinic_get(&id).unwrap().unwrap().json, r#"{"sum":100}"#);
    assert_eq!(r.clinic_history(&id).unwrap().len(), 1);
    assert_eq!(
        r.clinic_patient(&p).unwrap().as_deref(),
        Some(r#"{"n":"תמר"}"#)
    );
    assert!(r.audit_intact().unwrap());
}

#[test]
fn a_payment_a_charge_and_the_settings_are_never_removed_one_by_one() {
    let (_d, mut v) = new_vault();
    let pay = put(&mut v, "payment", Some("p1"), 1, "{}");
    let charge = put(&mut v, "charge", Some("p1"), 1, "{}");
    let settings = put(&mut v, "settings", None, 0, "{}");
    let visit = put(&mut v, "visit", Some("p1"), 1, "{}");
    for id in [&pay, &charge, &settings] {
        assert!(matches!(
            v.clinic_delete(id, true),
            Err(VaultError::Refused(_))
        ));
        assert!(v.clinic_get(id).unwrap().is_some());
    }
    // One of them in a list keeps the whole list.
    let r = v.clinic_delete_many(&[visit.clone(), pay.clone()], true);
    assert!(matches!(r, Err(VaultError::Refused(_))));
    assert!(v.clinic_get(&visit).unwrap().is_some());
    assert!(v.clinic_history_counts().unwrap().is_empty());
    for kind in ["visit", "note", "task"] {
        let id = put(&mut v, kind, None, 1, "{}");
        v.clinic_delete(&id, false).unwrap();
        assert!(v.clinic_get(&id).unwrap().is_none());
    }
}

#[test]
fn the_same_id_twice_in_one_delete_removes_nothing() {
    let (_d, mut v) = new_vault();
    let a = put(&mut v, "visit", None, 1, "{}");
    let r = v.clinic_delete_many(&[a.clone(), a.clone()], true);
    assert!(matches!(r, Err(VaultError::NotFound)));
    assert!(v.clinic_get(&a).unwrap().is_some());
    assert!(v.clinic_history(&a).unwrap().is_empty());
}

#[test]
fn erasing_takes_the_history_a_record_had_under_another_card() {
    let (_d, mut v) = new_vault();
    let p1 = v.save_clinic_patient(None, r#"{"n":"אורי"}"#).unwrap();
    let p2 = v.save_clinic_patient(None, r#"{"n":"נועה"}"#).unwrap();
    // Written for p2 by mistake, then moved to p1: its first history row carries p2.
    let moved = v
        .clinic_put("payment", None, Some(&p2), 1, r#"{"a":1}"#, Some("created"))
        .unwrap();
    v.clinic_put(
        "payment",
        Some(&moved),
        Some(&p1),
        1,
        r#"{"a":2}"#,
        Some("changed"),
    )
    .unwrap();
    let stays = v
        .clinic_put("payment", None, Some(&p2), 2, "{}", Some("created"))
        .unwrap();
    v.erase_clinic_patient(&p1, None, None).unwrap();
    assert!(v.clinic_get(&moved).unwrap().is_none());
    assert!(v.clinic_history(&moved).unwrap().is_empty());
    assert_eq!(v.clinic_history(&stays).unwrap().len(), 1);
    assert_eq!(v.clinic_of_patient(&p2).unwrap().len(), 1);
    let rows: i64 = v
        .main
        .query_row("SELECT COUNT(*) FROM clinic_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn a_replaced_record_keeps_its_kind_and_moves_in_the_indexes() {
    let (_d, mut v) = new_vault();
    let id = put(&mut v, "task", Some("p1"), 10, r#"{"t":1}"#);
    v.clinic_put("task", Some(&id), None, 0, r#"{"t":2}"#, None)
        .unwrap();
    let got = v.clinic_get(&id).unwrap().unwrap();
    assert_eq!(
        (got.kind.as_str(), got.patient_id, got.day),
        ("task", None, 0)
    );
    assert!(v.clinic_of_patient("p1").unwrap().is_empty());
    assert!(v.clinic_range("task", 10, 10).unwrap().is_empty());
    assert_eq!(v.clinic_range("task", 0, 0).unwrap().len(), 1);
    // No history was asked for: none was written.
    assert!(v.clinic_history_counts().unwrap().is_empty());
}

#[test]
fn an_empty_list_writes_and_removes_nothing() {
    let (_d, mut v) = new_vault();
    assert!(v
        .clinic_put_many("visit", &[], Some("created"))
        .unwrap()
        .is_empty());
    v.clinic_delete_many(&[], true).unwrap();
    assert!(v.clinic_kind("visit").unwrap().is_empty());
    assert!(v.clinic_history_counts().unwrap().is_empty());
}

#[test]
fn nothing_of_the_clinic_is_readable_in_the_files() {
    let (dir, mut v) = new_vault();
    let p = v
        .save_clinic_patient(None, r#"{"n":"שם-בדוי-לבדיקה"}"#)
        .unwrap();
    let id = v
        .clinic_put(
            "note",
            None,
            Some(&p),
            3,
            r#"{"text":"תוכן-בדוי-לבדיקה"}"#,
            Some("created"),
        )
        .unwrap();
    v.clinic_delete(&id, true).unwrap();
    v.lock().unwrap();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        for needle in ["שם-בדוי-לבדיקה", "תוכן-בדוי-לבדיקה"] {
            let needle = needle.as_bytes();
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "{path:?}"
            );
        }
    }
}

// ------------------------------------------------------------------ the review of the beta

fn set_column(v: &Vault, table: &str, column: &str, value: &str, id: &str) {
    let changed = v
        .main
        .execute(
            &format!("UPDATE {table} SET {column} = ?1 WHERE id = ?2"),
            [value, id],
        )
        .unwrap();
    assert_eq!(changed, 1);
}

/// D-177: what a row is (its kind, whose it is) is part of its seal. Someone who can write
/// the database file cannot turn a payment into a task, or hand it to another card.
#[test]
fn a_record_whose_kind_or_patient_was_changed_in_the_file_does_not_open() {
    for (column, value) in [("kind", "task"), ("patient_id", "p2")] {
        let (_d, mut v) = new_vault();
        let id = v
            .clinic_put("payment", None, Some("p1"), 5, r#"{"a":1}"#, None)
            .unwrap();
        let other = put(&mut v, "payment", Some("p1"), 6, r#"{"a":2}"#);
        assert_eq!(v.clinic_unreadable().unwrap(), 0);
        set_column(&v, "clinic_records", column, value, &id);
        assert!(v.clinic_get(&id).is_err(), "{column}");
        // One patient's records are all read or none: nothing is decided with a row missing.
        let patient = if column == "kind" { "p1" } else { "p2" };
        assert!(v.clinic_of_patient(patient).is_err(), "{column}");
        // The lists leave the row out, and it is counted.
        let listed = |v: &Vault, kind: &str| -> Vec<String> {
            v.clinic_kind(kind)
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect()
        };
        assert_eq!(
            listed(&v, "payment"),
            std::slice::from_ref(&other),
            "{column}"
        );
        assert!(listed(&v, "task").is_empty());
        assert!(v
            .clinic_range("payment", 0, 10)
            .unwrap()
            .iter()
            .all(|r| r.id != id));
        assert_eq!(v.clinic_unreadable().unwrap(), 1);
        // The row itself is still there, untouched.
        let rows: i64 = v
            .main
            .query_row("SELECT COUNT(*) FROM clinic_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2);
    }
}

#[test]
fn a_patient_column_set_where_there_was_none_does_not_open() {
    let (_d, mut v) = new_vault();
    let block = put(&mut v, "visit", None, 5, "{}");
    set_column(&v, "clinic_records", "patient_id", "p1", &block);
    assert!(v.clinic_get(&block).is_err());
}

#[test]
fn a_history_row_moved_to_another_record_patient_or_action_does_not_open() {
    for (column, value) in [
        ("record_id", "other"),
        ("patient_id", "p2"),
        ("action", "created"),
    ] {
        let (_d, mut v) = new_vault();
        let id = v
            .clinic_put("payment", None, Some("p1"), 5, "{}", Some("voided"))
            .unwrap();
        let other = v
            .clinic_put("payment", None, Some("p1"), 5, "{}", Some("created"))
            .unwrap();
        assert_eq!(v.clinic_history(&id).unwrap().len(), 1);
        let value = if column == "record_id" {
            other.as_str()
        } else {
            value
        };
        let changed = v
            .main
            .execute(
                &format!("UPDATE clinic_history SET {column} = ?1 WHERE record_id = ?2"),
                [value, id.as_str()],
            )
            .unwrap();
        assert_eq!(changed, 1);
        let read = if column == "record_id" { &other } else { &id };
        assert!(v.clinic_history(read).is_err(), "{column}");
    }
}

#[test]
fn a_card_that_does_not_open_is_left_out_and_counted() {
    let (_d, mut v) = new_vault();
    let a = v.save_clinic_patient(None, r#"{"n":"אורי"}"#).unwrap();
    let b = v.save_clinic_patient(None, r#"{"n":"נועה"}"#).unwrap();
    // The sealed contents of one card put under the other: the seal is bound to the id.
    let sealed: Vec<u8> = v
        .identity
        .query_row(
            "SELECT data_enc FROM clinic_patients WHERE id = ?1",
            [&a],
            |r| r.get(0),
        )
        .unwrap();
    v.identity
        .execute(
            "UPDATE clinic_patients SET data_enc = ?1 WHERE id = ?2",
            rusqlite::params![sealed, b],
        )
        .unwrap();
    let ids: Vec<String> = v
        .clinic_patients()
        .unwrap()
        .into_iter()
        .map(|p| p.0)
        .collect();
    assert_eq!(ids, [a]);
    assert!(v.clinic_patient(&b).is_err());
    assert_eq!(v.clinic_unreadable().unwrap(), 1);
}

/// The reviewer's case: a payment written for one card and moved to another keeps the first
/// card's id on its older history rows. Erasing the first card leaves all of it.
#[test]
fn erasing_a_card_leaves_the_history_of_a_record_that_now_belongs_to_another() {
    let (_d, mut v) = new_vault();
    let p1 = v.save_clinic_patient(None, r#"{"n":"אורי"}"#).unwrap();
    let p2 = v.save_clinic_patient(None, r#"{"n":"נועה"}"#).unwrap();
    let moved = v
        .clinic_put("payment", None, Some(&p1), 1, r#"{"a":1}"#, Some("created"))
        .unwrap();
    v.clinic_put(
        "payment",
        Some(&moved),
        Some(&p2),
        1,
        r#"{"a":2}"#,
        Some("changed"),
    )
    .unwrap();
    let own = v
        .clinic_put("visit", None, Some(&p1), 2, "{}", Some("created"))
        .unwrap();
    // A visit of p1 removed earlier: its rows are p1's and go with the card.
    let gone = v
        .clinic_put("visit", None, Some(&p1), 3, "{}", Some("created"))
        .unwrap();
    v.clinic_delete(&gone, true).unwrap();
    v.erase_clinic_patient(&p1, None, None).unwrap();
    let history = v.clinic_history(&moved).unwrap();
    assert_eq!(
        history.iter().map(|h| h.json.as_str()).collect::<Vec<_>>(),
        [r#"{"a":1}"#, r#"{"a":2}"#]
    );
    assert_eq!(v.clinic_get(&moved).unwrap().unwrap().json, r#"{"a":2}"#);
    assert!(v.clinic_history(&own).unwrap().is_empty());
    assert!(v.clinic_history(&gone).unwrap().is_empty());
    let rows: i64 = v
        .main
        .query_row("SELECT COUNT(*) FROM clinic_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 2);
}

/// D-179: what the erased records counted for is written with their removal, and the count
/// of erased cards with the card's.
#[test]
fn an_erase_writes_the_tallies_it_is_given_and_they_are_sealed() {
    let (_d, mut v) = new_vault();
    assert!(v.clinic_get("tally").unwrap().is_none());
    assert!(v.clinic_erased_cards().unwrap().is_none());
    let p1 = v.save_clinic_patient(None, r#"{"n":"אורי"}"#).unwrap();
    let p2 = v.save_clinic_patient(None, r#"{"n":"נועה"}"#).unwrap();
    put(&mut v, "payment", Some(&p1), 4, "{}");
    v.erase_clinic_patient(
        &p1,
        Some(r#"{"months":{"סכום-בדוי":1}}"#),
        Some(r#"{"m":1}"#),
    )
    .unwrap();
    let tally = v.clinic_get("tally").unwrap().unwrap();
    assert_eq!(
        (tally.kind.as_str(), tally.patient_id.clone(), tally.day),
        ("tally", None, 0)
    );
    assert_eq!(tally.json, r#"{"months":{"סכום-בדוי":1}}"#);
    assert_eq!(
        v.clinic_erased_cards().unwrap().as_deref(),
        Some(r#"{"m":1}"#)
    );
    // A second erase replaces both with what it is given; none given, both stay.
    put(&mut v, "payment", Some(&p2), 4, "{}");
    v.erase_clinic_patient(&p2, Some(r#"{"months":{}}"#), Some(r#"{"m":2}"#))
        .unwrap();
    assert_eq!(
        v.clinic_get("tally").unwrap().unwrap().json,
        r#"{"months":{}}"#
    );
    assert_eq!(
        v.clinic_erased_cards().unwrap().as_deref(),
        Some(r#"{"m":2}"#)
    );
    let p3 = v.save_clinic_patient(None, r#"{"n":"גיל"}"#).unwrap();
    v.erase_clinic_patient(&p3, None, None).unwrap();
    assert_eq!(
        v.clinic_erased_cards().unwrap().as_deref(),
        Some(r#"{"m":2}"#)
    );
    assert_eq!(v.clinic_kind("tally").unwrap().len(), 1);
    // Sealed like every row: nothing of it is readable in the tables.
    let needle = "סכום-בדוי".as_bytes();
    let raw: Vec<Vec<u8>> = {
        let mut stmt = v
            .main
            .prepare("SELECT data_enc FROM clinic_records")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(raw
        .iter()
        .all(|bytes| !bytes.windows(needle.len()).any(|w| w == needle)));
    // The log says how many records went, and nothing of the tallies.
    assert_eq!(
        audit_rows(&v, "clinic_patient_erased"),
        [r#"{"records":1}"#, r#"{"records":1}"#, r#"{"records":0}"#]
    );
    assert!(v.audit_intact().unwrap());
}

#[test]
fn the_tally_is_written_only_by_an_erase_and_never_removed() {
    let (_d, mut v) = new_vault();
    for id in [None, Some("tally")] {
        let r = v.clinic_put("tally", id, None, 0, "{}", None);
        assert!(matches!(r, Err(VaultError::Refused(_))), "{id:?}");
    }
    let p = v.save_clinic_patient(None, "{}").unwrap();
    v.erase_clinic_patient(&p, Some("{}"), Some("{}")).unwrap();
    assert!(matches!(
        v.clinic_delete("tally", false),
        Err(VaultError::Refused(_))
    ));
    assert!(v.clinic_get("tally").unwrap().is_some());
}

/// D-177: the material and the note that points to it are one write.
#[test]
fn a_note_goes_into_a_case_with_its_material_or_not_at_all() {
    use dv_domain::{CaseMeta, InputKind};
    let (_d, mut v) = new_vault();
    let case = v.create_case(&CaseMeta::default()).unwrap();
    let note = v
        .clinic_put("note", None, Some("p1"), 3, r#"{"v":1}"#, Some("created"))
        .unwrap();
    // A note that is not there: nothing is written, the material neither.
    let ghost = (
        Some("ghost".to_owned()),
        Some("p1".to_owned()),
        3,
        "{}".to_owned(),
    );
    let r = v.clinic_note_into_case(
        &case,
        "in-1",
        InputKind::SessionNote,
        "כותרת",
        "תוכן",
        &ghost,
    );
    assert!(matches!(r, Err(VaultError::NotFound)));
    assert!(v.inputs(&case).unwrap().is_empty());
    // A record of another kind under that id: refused, and again nothing.
    let visit = put(&mut v, "visit", Some("p1"), 3, "{}");
    let wrong = (Some(visit), Some("p1".to_owned()), 3, "{}".to_owned());
    let r = v.clinic_note_into_case(
        &case,
        "in-1",
        InputKind::SessionNote,
        "כותרת",
        "תוכן",
        &wrong,
    );
    assert!(matches!(r, Err(VaultError::Refused(_))));
    assert!(v.inputs(&case).unwrap().is_empty());
    assert_eq!(v.clinic_get(&note).unwrap().unwrap().json, r#"{"v":1}"#);
    // The real one: both, and the note's history says it changed.
    let row = (
        Some(note.clone()),
        Some("p1".to_owned()),
        3,
        r#"{"v":2}"#.to_owned(),
    );
    v.clinic_note_into_case(&case, "in-1", InputKind::Intake, "כותרת", "תוכן", &row)
        .unwrap();
    let inputs = v.inputs(&case).unwrap();
    assert_eq!(inputs.len(), 1);
    let input = inputs.first().unwrap();
    assert_eq!(
        (
            input.id.as_str(),
            input.kind,
            input.title.as_str(),
            input.content.as_str()
        ),
        ("in-1", InputKind::Intake, "כותרת", "תוכן")
    );
    assert_eq!(v.clinic_get(&note).unwrap().unwrap().json, r#"{"v":2}"#);
    let history = v.clinic_history(&note).unwrap();
    assert_eq!(
        history
            .iter()
            .map(|h| h.action.as_str())
            .collect::<Vec<_>>(),
        ["created", "changed"]
    );
    // The same material id twice: the second is refused whole, the note stays as it was.
    let again = (
        Some(note.clone()),
        Some("p1".to_owned()),
        3,
        r#"{"v":3}"#.to_owned(),
    );
    assert!(v
        .clinic_note_into_case(&case, "in-1", InputKind::Intake, "כותרת", "תוכן", &again)
        .is_err());
    assert_eq!(v.clinic_get(&note).unwrap().unwrap().json, r#"{"v":2}"#);
    assert_eq!(v.inputs(&case).unwrap().len(), 1);
}
