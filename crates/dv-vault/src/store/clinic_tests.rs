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
        .clinic_put("charge", None, Some(&p1), 6, "{}", None)
        .unwrap();
    v.clinic_delete(&gone, true).unwrap();
    v.erase_clinic_patient(&p1).unwrap();
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
        v.erase_clinic_patient(&p1),
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
