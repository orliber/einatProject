//! The copy of the clinic's calendar to Google Calendar, against a fake Google (never the
//! real one). Every family and name here is invented.

use super::*;
use dv_domain::clinic::{
    ClinicReply, ClinicRequest, Contact, ContactRole, Patient, PatientStatus, Scope, Visit,
    VisitKind, VisitStatus,
};
use dv_egress::google::{Method, Reply, Request, GOOGLE_HOSTS};
use std::collections::BTreeMap;

const PASSWORD: &str = "סוס ירוק רץ בשדה 2026";
const CAL: &str = "c_0a1b2c@group.calendar.google.com";
const CHILD: &str = "נועה בדויה";
const PARENT: &str = "דנה בדויה";
/// The emergency stop is process-wide: the test that turns it on holds this for writing, the
/// others for reading.
static STOP_FOR_TESTS: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Her Google account's id, as the sign-in gives it (invented).
const ACCOUNT: &str = "acct-1";

fn cfg() -> ClientConfig {
    ClientConfig {
        client_id: "client-1.apps.googleusercontent.com".to_owned(),
        client_secret: "public-secret".to_owned(),
    }
}

/// A fake Google: the token endpoint, one calendar and its events, and ways to fail.
#[derive(Default)]
struct FakeGoogle {
    calendar: Mutex<Option<String>>,
    /// id → (deleted, summary, start, end)
    events: Mutex<BTreeMap<String, (bool, String, String, String)>>,
    seen: Mutex<Vec<(Method, String, String)>>,
    withdrawn: bool,
    offline: bool,
}

impl FakeGoogle {
    fn calls(&self, method: Method, part: &str) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, u, _)| *m == method && u.contains(part))
            .count()
    }

    fn live(&self) -> Vec<(String, String, String)> {
        self.events
            .lock()
            .unwrap()
            .values()
            .filter(|(deleted, ..)| !deleted)
            .map(|(_, summary, start, end)| (summary.clone(), start.clone(), end.clone()))
            .collect()
    }
}

impl Http for FakeGoogle {
    fn send(&self, req: Request) -> Result<Reply, GoogleError> {
        let rest = req.url.strip_prefix("https://").unwrap();
        let (host, path) = rest.split_once('/').unwrap();
        assert!(
            GOOGLE_HOSTS.contains(&host),
            "a host off the list: {}",
            req.url
        );
        let path = format!("/{}", path.split('?').next().unwrap());
        let body = String::from_utf8(req.body.to_vec()).unwrap();
        self.seen
            .lock()
            .unwrap()
            .push((req.method, req.url.clone(), body.clone()));
        if self.offline {
            return Err(GoogleError::Offline);
        }
        let ok = |v: serde_json::Value| Ok(Reply::new(200, v.to_string().into_bytes()));
        let cal_path = format!("/calendar/v3/calendars/{CAL}");
        match (req.method, path.as_str()) {
            (Method::Post, "/token") if self.withdrawn => {
                Ok(Reply::new(400, br#"{"error":"invalid_grant"}"#.to_vec()))
            }
            (Method::Post, "/token") => ok(serde_json::json!({"access_token": "ya29.short"})),
            (Method::Post, "/revoke") => Ok(Reply::new(200, Vec::new())),
            (Method::Get, "/drive/v3/files") => ok(serde_json::json!({ "files": [] })),
            (Method::Post, "/calendar/v3/calendars") => {
                *self.calendar.lock().unwrap() = Some(CAL.to_owned());
                ok(serde_json::json!({ "id": CAL }))
            }
            (Method::Get, p) if p == cal_path => {
                if self.calendar.lock().unwrap().is_some() {
                    ok(serde_json::json!({ "id": CAL }))
                } else {
                    Ok(Reply::new(404, Vec::new()))
                }
            }
            (Method::Delete, p) if p == cal_path => {
                *self.calendar.lock().unwrap() = None;
                self.events.lock().unwrap().clear();
                Ok(Reply::new(204, Vec::new()))
            }
            (Method::Get, p) if p.ends_with("/events") => {
                let items: Vec<serde_json::Value> = self
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(id, (deleted, summary, start, end))| {
                        serde_json::json!({
                            "id": id,
                            "status": if *deleted { "cancelled" } else { "confirmed" },
                            "summary": summary,
                            "start": { "dateTime": format!("{start}+02:00") },
                            "end": { "dateTime": format!("{end}+02:00") },
                        })
                    })
                    .collect();
                ok(serde_json::json!({ "items": items }))
            }
            (Method::Post | Method::Put, p) if p.contains("/events") => {
                let b: serde_json::Value = serde_json::from_str(&body).unwrap();
                let id = b["id"].as_str().unwrap().to_owned();
                self.events.lock().unwrap().insert(
                    id,
                    (
                        false,
                        b["summary"].as_str().unwrap().to_owned(),
                        b["start"]["dateTime"].as_str().unwrap().to_owned(),
                        b["end"]["dateTime"].as_str().unwrap().to_owned(),
                    ),
                );
                ok(serde_json::json!({}))
            }
            (Method::Delete, p) if p.contains("/events/") => {
                let id = p.rsplit('/').next().unwrap();
                if let Some(e) = self.events.lock().unwrap().get_mut(id) {
                    e.0 = true;
                }
                Ok(Reply::new(204, Vec::new()))
            }
            other => panic!("unexpected call {other:?}"),
        }
    }
}

fn day(offset: i64) -> String {
    rules::add_days(&iso(today()), offset).unwrap()
}

fn vault() -> (tempfile::TempDir, Core) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    (dir, core)
}

fn turned_on() -> (tempfile::TempDir, Core) {
    let (dir, mut core) = vault();
    assert!(
        !core.calendar_turn_on_check().unwrap(),
        "nothing holds the grant yet"
    );
    core.calendar_turn_on_finish("1//lasting", ACCOUNT).unwrap();
    (dir, core)
}

fn patient(core: &mut Core) -> String {
    let patient = Patient {
        name: CHILD.to_owned(),
        contacts: vec![Contact {
            role: ContactRole::Mother,
            name: PARENT.to_owned(),
            phone: "050-0000000".to_owned(),
            email: String::new(),
        }],
        status: PatientStatus::Active,
        created_on: day(-30),
        ..Patient::default()
    };
    match core.clinic(ClinicRequest::SavePatient { patient }).unwrap() {
        ClinicReply::Patient(p) => p.id,
        other => panic!("{other:?}"),
    }
}

fn save(core: &mut Core, visit: Visit) -> Visit {
    let request = ClinicRequest::SaveVisit {
        visit,
        repeat: None,
        scope: Scope::One,
    };
    match core.clinic(request).unwrap() {
        ClinicReply::Saved(mut s) => s.visits.remove(0).visit,
        other => panic!("{other:?}"),
    }
}

fn visit(patient_id: &str, offset: i64, start: u16) -> Visit {
    Visit {
        patient_id: Some(patient_id.to_owned()),
        kind: VisitKind::Assessment,
        title: format!("אבחון של {CHILD}"),
        date: day(offset),
        start,
        minutes: 90,
        place: "רחוב בדוי 3".to_owned(),
        note: "להביא את השאלון של נועה".to_owned(),
        ..Visit::default()
    }
}

/// One round now, as the shell runs it.
fn round(core: &mut Core, google: &FakeGoogle) {
    let job = core.calendar_sync_now().unwrap();
    let done = job.run(google, &cfg());
    core.finish_calendar_sync(done).unwrap();
}

#[test]
fn off_by_default_nothing_goes() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = vault();
    let pid = patient(&mut core);
    save(&mut core, visit(&pid, 1, 600));
    let status = core.calendar_status().unwrap();
    assert!(!status.on && !status.cleanup_pending && status.problem.is_none());
    assert!(core.take_calendar_sync().is_none());
    assert!(core.calendar_sync_now().is_err());
    assert!(core.take_calendar_cleanup().is_none());
}

#[test]
fn only_the_time_and_one_word_reach_google() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let pid = patient(&mut core);
    let a = save(&mut core, visit(&pid, 1, 9 * 60));
    save(
        &mut core,
        Visit {
            patient_id: None,
            kind: VisitKind::Block,
            title: "כתיבת הדוח של נועה".to_owned(),
            ..visit(&pid, 2, 13 * 60)
        },
    );
    let google = FakeGoogle::default();
    round(&mut core, &google);

    let mut live = google.live();
    live.sort();
    assert_eq!(
        live,
        vec![
            (
                "לא פנויה".to_owned(),
                format!("{}T13:00:00", day(2)),
                format!("{}T14:30:00", day(2))
            ),
            (
                "פגישה".to_owned(),
                format!("{}T09:00:00", day(1)),
                format!("{}T10:30:00", day(1))
            ),
        ]
    );
    // Nothing of the card, the visit or the vault's ids anywhere in what went.
    for (_, url, body) in google.seen.lock().unwrap().iter() {
        for secret in [
            CHILD,
            "נועה",
            PARENT,
            "רחוב בדוי",
            "050-0000000",
            a.id.as_str(),
            pid.as_str(),
            "אבחון",
        ] {
            assert!(
                !url.contains(secret) && !body.contains(secret),
                "{secret} in {url} {body}"
            );
        }
    }
    let status = core.calendar_status().unwrap();
    assert!(status.on && status.last_at.is_some() && status.problem.is_none());
    assert_eq!(status.detail, CalendarDetail::TimesOnly);
}

#[test]
fn the_copy_follows_moves_cancellations_and_deletions() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let pid = patient(&mut core);
    let google = FakeGoogle::default();
    let a = save(&mut core, visit(&pid, 1, 9 * 60));
    let b = save(&mut core, visit(&pid, 3, 9 * 60));
    round(&mut core, &google);
    assert_eq!(google.live().len(), 2);

    // Nothing changed: a round writes nothing.
    let writes = |g: &FakeGoogle| {
        g.calls(Method::Post, "/events")
            + g.calls(Method::Put, "/events")
            + g.calls(Method::Delete, "/events/")
    };
    let before = writes(&google);
    round(&mut core, &google);
    assert_eq!(writes(&google), before);

    save(
        &mut core,
        Visit {
            start: 11 * 60,
            ..a.clone()
        },
    );
    save(
        &mut core,
        Visit {
            status: VisitStatus::Cancelled,
            status_note: "חולה".to_owned(),
            ..b
        },
    );
    round(&mut core, &google);
    assert_eq!(
        google.live(),
        vec![(
            "פגישה".to_owned(),
            format!("{}T11:00:00", day(1)),
            format!("{}T12:30:00", day(1))
        )]
    );

    // She deleted the app's calendar in Google Calendar: the next round makes it again.
    *google.calendar.lock().unwrap() = None;
    google.events.lock().unwrap().clear();
    round(&mut core, &google);
    assert_eq!(google.live().len(), 1);
    let made = google
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(m, u, _)| *m == Method::Post && u.ends_with("/calendar/v3/calendars"))
        .count();
    assert_eq!(made, 2);
}

#[test]
fn a_change_asks_for_a_round_soon_and_rounds_never_overlap() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let first = core.take_calendar_sync().expect("due after turning on");
    assert!(core.take_calendar_sync().is_none(), "one at a time");
    assert!(
        core.calendar_sync_now().is_err(),
        "one at a time, also by hand"
    );
    let google = FakeGoogle::default();
    let done = first.run(&google, &cfg());
    core.finish_calendar_sync(done).unwrap();
    assert!(
        core.take_calendar_sync().is_none(),
        "nothing changed, and it just ran"
    );
    let pid = patient(&mut core);
    assert!(core.take_calendar_sync().is_none(), "a card is not a visit");
    save(&mut core, visit(&pid, 1, 600));
    assert!(core.calendar.changed);
}

#[test]
fn a_lock_or_turning_off_stops_a_round_before_it_writes() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let pid = patient(&mut core);
    save(&mut core, visit(&pid, 1, 600));
    let google = FakeGoogle::default();

    let job = core.calendar_sync_now().unwrap();
    core.lock();
    let done = job.run(&google, &cfg());
    assert!(google.seen.lock().unwrap().is_empty());
    assert!(core.finish_calendar_sync(done).is_err());

    core.unlock(PASSWORD).unwrap();
    let job = core.calendar_sync_now().unwrap();
    let cleanup = core.calendar_turn_off().unwrap();
    let done = job.run(&google, &cfg());
    assert!(google.seen.lock().unwrap().is_empty());
    core.finish_calendar_sync(done).unwrap();
    assert!(!core.calendar_status().unwrap().on);
    let cleanup = cleanup.expect("the permission is owed at Google");
    assert!(cleanup.calendars.is_empty(), "no calendar was made");
}

#[test]
fn turning_off_deletes_the_calendar_and_withdraws_the_permission() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let pid = patient(&mut core);
    save(&mut core, visit(&pid, 1, 600));
    let google = FakeGoogle::default();
    round(&mut core, &google);
    assert!(google.calendar.lock().unwrap().is_some());

    let cleanup = core.calendar_turn_off().unwrap().expect("owed");
    assert!(!core.calendar_on());
    assert!(core.calendar_status().unwrap().cleanup_pending);
    let result = core
        .finish_calendar_cleanup(cleanup.run(&google, &cfg()))
        .unwrap();
    assert_eq!(result, CalendarOff::Cleaned);
    assert!(google.calendar.lock().unwrap().is_none());
    assert_eq!(google.calls(Method::Post, "/revoke"), 1);
    let status = core.calendar_status().unwrap();
    assert!(!status.on && !status.cleanup_pending);
    assert!(core.take_calendar_cleanup().is_none());
}

#[test]
fn a_calendar_made_before_a_lock_is_used_again_and_deleted_at_turning_off() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let pid = patient(&mut core);
    save(&mut core, visit(&pid, 1, 600));
    let google = FakeGoogle::default();
    // The round makes the calendar, then the vault locks before its outcome is written.
    let job = core.calendar_sync_now().unwrap();
    let done = job.run(&google, &cfg());
    core.lock();
    assert!(core.finish_calendar_sync(done).is_err());
    core.unlock(PASSWORD).unwrap();
    // The next round uses that calendar: not a second one.
    round(&mut core, &google);
    let made = |g: &FakeGoogle| {
        g.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, u, _)| *m == Method::Post && u.ends_with("/calendar/v3/calendars"))
            .count()
    };
    assert_eq!(made(&google), 1);

    // Turned off right after such a lock: the calendar is in the cleanup owed (kept in the
    // vault, so also after a restart).
    let (_dir2, mut core) = turned_on();
    let google = FakeGoogle::default();
    let job = core.calendar_sync_now().unwrap();
    let done = job.run(&google, &cfg());
    core.lock();
    drop(done);
    core.unlock(PASSWORD).unwrap();
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    assert_eq!(cleanup.calendars, vec![CAL.to_owned()]);
    core.finish_calendar_cleanup(cleanup.run(&google, &cfg()))
        .unwrap();
    assert!(google.calendar.lock().unwrap().is_none());
}

#[test]
fn a_sign_in_begun_meanwhile_keeps_the_grant_until_the_next_try() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let google = FakeGoogle::default();
    round(&mut core, &google);
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    // She begins turning it on again before the cleanup reaches Google.
    assert!(core.calendar_turn_on_check().is_ok());
    assert_eq!(
        core.finish_calendar_cleanup(cleanup.run(&google, &cfg()))
            .unwrap(),
        CalendarOff::Pending
    );
    assert!(google.calendar.lock().unwrap().is_none(), "deleted anyway");
    assert_eq!(google.calls(Method::Post, "/revoke"), 0);
    // The sign-in ended without a permission: the next try withdraws it.
    core.lock();
    core.unlock(PASSWORD).unwrap();
    let again = core.take_calendar_cleanup().unwrap();
    assert_eq!(
        core.finish_calendar_cleanup(again.run(&google, &cfg()))
            .unwrap(),
        CalendarOff::Cleaned
    );
    assert_eq!(google.calls(Method::Post, "/revoke"), 1);
}

#[test]
fn the_drive_copy_on_another_account_does_not_keep_the_calendars_grant() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let google = FakeGoogle::default();
    round(&mut core, &google);
    let vault_id = core.vault_ref().unwrap().vault_id().to_owned();
    let v = core.vault_mut().unwrap();
    v.set_secret("drive_backup_token", "1//drive").unwrap();
    v.set_state(
        "drive_backup_account",
        &google_account_tag(&vault_id, "acct-2"),
    )
    .unwrap();
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    core.finish_calendar_cleanup(cleanup.run(&google, &cfg()))
        .unwrap();
    assert_eq!(
        google.calls(Method::Post, "/revoke"),
        1,
        "another account is another grant"
    );
    // A cleanup owed on one account keeps its place from a permission of another.
    let (_dir2, mut core) = turned_on();
    let offline = FakeGoogle {
        offline: true,
        ..FakeGoogle::default()
    };
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    core.finish_calendar_cleanup(cleanup.run(&offline, &cfg()))
        .unwrap();
    core.calendar_turn_on_check().unwrap();
    assert!(core.calendar_turn_on_finish("1//other", "acct-2").is_err());
    assert!(!core.calendar_on());
    core.calendar_turn_on_finish("1//same", ACCOUNT).unwrap();
    assert!(core.calendar_on());
}

#[test]
fn a_cleanup_offline_stays_owed_and_is_tried_again() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let google = FakeGoogle::default();
    round(&mut core, &google);
    let offline = FakeGoogle {
        offline: true,
        ..FakeGoogle::default()
    };
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    assert_eq!(
        core.finish_calendar_cleanup(cleanup.run(&offline, &cfg()))
            .unwrap(),
        CalendarOff::Pending
    );
    assert!(
        core.take_calendar_cleanup().is_none(),
        "waits before trying again"
    );
    core.lock();
    core.unlock(PASSWORD).unwrap();
    let again = core
        .take_calendar_cleanup()
        .expect("tried again after an unlock");
    assert!(core.take_calendar_cleanup().is_none(), "never two at once");
    assert_eq!(
        core.finish_calendar_cleanup(again.run(&google, &cfg()))
            .unwrap(),
        CalendarOff::Cleaned
    );
    assert!(google.calendar.lock().unwrap().is_none());
}

#[test]
fn the_drive_copy_keeps_the_grant_and_the_calendar_keeps_it_for_the_drive_copy() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    assert!(
        core.drive_keeps_grant(),
        "a sign-in's end leaves the calendar's grant alone"
    );
    let google = FakeGoogle::default();
    round(&mut core, &google);
    // The Drive copy on (its permission sealed, as the Drive tests set it).
    core.vault_mut()
        .unwrap()
        .set_secret("drive_backup_token", "1//drive")
        .unwrap();
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    assert_eq!(
        core.finish_calendar_cleanup(cleanup.run(&google, &cfg()))
            .unwrap(),
        CalendarOff::Cleaned
    );
    assert!(google.calendar.lock().unwrap().is_none());
    assert_eq!(
        google.calls(Method::Post, "/revoke"),
        0,
        "the Drive copy rests on it"
    );
}

#[test]
fn turning_the_drive_copy_off_leaves_the_grant_to_the_calendar() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    core.vault_mut()
        .unwrap()
        .set_secret("drive_backup_token", "1//drive")
        .unwrap();
    let google = FakeGoogle::default();
    let cleanup = core.drive_backup_turn_off().unwrap().expect("owed");
    let result = core
        .finish_drive_cleanup(cleanup.run(&google, &cfg()))
        .unwrap();
    assert_eq!(result, crate::drive_backup::DriveCleanupResult::Cleaned);
    assert_eq!(
        google.calls(Method::Post, "/revoke"),
        0,
        "the calendar rests on it"
    );
    // The calendar, turned off last, withdraws it.
    round(&mut core, &google);
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    core.finish_calendar_cleanup(cleanup.run(&google, &cfg()))
        .unwrap();
    assert_eq!(google.calls(Method::Post, "/revoke"), 1);
}

#[test]
fn a_withdrawn_permission_asks_to_connect_again() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let withdrawn = FakeGoogle {
        withdrawn: true,
        ..FakeGoogle::default()
    };
    round(&mut core, &withdrawn);
    let status = core.calendar_status().unwrap();
    assert_eq!(status.problem, Some(CalendarProblem::Reconnect));
    assert!(
        core.take_calendar_sync().is_none(),
        "not tried again by itself"
    );
    assert!(
        core.calendar_turn_on_check().is_ok(),
        "connecting again is allowed"
    );
    core.calendar_turn_on_finish("1//new", ACCOUNT).unwrap();
    assert_eq!(core.calendar_status().unwrap().problem, None);
    let cleanup = core.calendar_turn_off().unwrap().unwrap();
    assert_eq!(
        core.finish_calendar_cleanup(cleanup.run(&withdrawn, &cfg()))
            .unwrap(),
        CalendarOff::Withdrawn
    );
    assert_eq!(
        core.calendar_status().unwrap().last_off,
        Some(CalendarOff::Withdrawn)
    );
}

#[test]
fn the_emergency_stop_sends_nothing() {
    let _only = STOP_FOR_TESTS
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let google = FakeGoogle::default();
    let job = core.calendar_sync_now().unwrap();
    dv_egress::set_stopped(true);
    let done = job.run(&google, &cfg());
    dv_egress::set_stopped(false);
    assert!(google.seen.lock().unwrap().is_empty());
    core.finish_calendar_sync(done).unwrap();
    assert_eq!(
        core.calendar_status().unwrap().problem,
        Some(CalendarProblem::Paused)
    );
}

#[test]
fn a_restore_never_turns_it_on() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let (_dir, mut core) = turned_on();
    let google = FakeGoogle::default();
    round(&mut core, &google);
    forget_on_restore(core.vault_mut().unwrap()).unwrap();
    let status = core.calendar_status().unwrap();
    assert!(!status.on && status.cleanup_pending);
    let owed = core
        .take_calendar_cleanup()
        .expect("the calendar is deleted");
    assert_eq!(
        core.finish_calendar_cleanup(owed.run(&google, &cfg()))
            .unwrap(),
        CalendarOff::Cleaned
    );
    assert!(google.calendar.lock().unwrap().is_none());
}

#[test]
fn the_rules_and_the_wire_agree_on_every_word() {
    let _calm = STOP_FOR_TESTS
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    assert_eq!(rules::CALENDAR_NAME, cal::CALENDAR_NAME);
    assert_eq!(rules::CALENDAR_TIME_ZONE, cal::TIME_ZONE);
    for title in [rules::TITLE_VISIT, rules::TITLE_BLOCK] {
        assert!(cal::TITLES.contains(&title));
    }
    let key = Key32::random().unwrap();
    let id = event_id(&key, "visit-1");
    assert!(cal::event_id_ok(&id));
    assert_ne!(id, event_id(&Key32::random().unwrap(), "visit-1"), "keyed");
    assert_eq!(id, event_id(&key, "visit-1"), "stable");
}
