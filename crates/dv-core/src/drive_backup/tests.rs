//! The encrypted copy in Drive, against a fake Google (never the real one).

use super::*;
use dv_egress::google::{Method, Reply, Request, GOOGLE_HOSTS};
use dv_vault::crypto::Key32;
use dv_vault::recovery::google_account_tag;
use std::cell::RefCell;
use std::path::Path;

const PASSWORD: &str = "סוס ירוק רץ בשדה 2026";
const NEW_PASSWORD: &str = "ציפור כתומה שרה בבוקר";

fn cfg() -> ClientConfig {
    ClientConfig {
        client_id: "client-1.apps.googleusercontent.com".to_owned(),
        client_secret: "public-secret".to_owned(),
    }
}

/// Method, URL and body of one request.
type Seen = (Method, String, Vec<u8>);

/// A fake Google: a token endpoint, the hidden folder, and ways to fail.
#[derive(Default)]
struct FakeGoogle {
    files: Mutex<Vec<(String, String, Vec<u8>)>>,
    seen: Mutex<Vec<Seen>>,
    pending: Mutex<Option<String>>,
    next: Mutex<u32>,
    fail: Option<GoogleError>,
    withdrawn: bool,
    /// The revoke endpoint fails (Google down for that one call).
    revoke_fails: bool,
}

impl FakeGoogle {
    fn calls(&self, method: Method, path: &str) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, u, _)| {
                *m == method && reqwest_path(u).is_some_and(|p| p.starts_with(path))
            })
            .cloned()
            .collect()
    }
}

fn reqwest_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    GOOGLE_HOSTS
        .contains(&host)
        .then(|| format!("/{}", path.split('?').next().unwrap_or("")))
}

impl Http for FakeGoogle {
    fn send(&self, req: Request) -> Result<Reply, GoogleError> {
        let path =
            reqwest_path(&req.url).unwrap_or_else(|| panic!("a host off the list: {}", req.url));
        self.seen
            .lock()
            .unwrap()
            .push((req.method, req.url.clone(), req.body.to_vec()));
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        let ok = |v: serde_json::Value| Ok(Reply::new(200, v.to_string().into_bytes()));
        match (req.method, path.as_str()) {
            (Method::Post, "/token") if self.withdrawn => {
                Ok(Reply::new(400, br#"{"error":"invalid_grant"}"#.to_vec()))
            }
            (Method::Post, "/token") => ok(serde_json::json!({"access_token": "ya29.short"})),
            (Method::Post, "/revoke") if self.revoke_fails => Ok(Reply::new(503, Vec::new())),
            (Method::Post, "/revoke") => Ok(Reply::new(200, Vec::new())),
            (Method::Post, "/upload/drive/v3/files") => {
                let meta: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                *self.pending.lock().unwrap() = Some(meta["name"].as_str().unwrap().to_owned());
                Ok(Reply::pointing_to(
                    200,
                    "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&upload_id=U1",
                ))
            }
            (Method::Put, "/upload/drive/v3/files") => {
                let name = self.pending.lock().unwrap().take().unwrap();
                let mut n = self.next.lock().unwrap();
                *n += 1;
                let id = format!("f{n}");
                let size = req.body.len().to_string();
                self.files
                    .lock()
                    .unwrap()
                    .push((id.clone(), name, req.body.to_vec()));
                ok(serde_json::json!({ "id": id, "size": size }))
            }
            (Method::Get, "/drive/v3/files") => {
                let files: Vec<serde_json::Value> = self
                    .files
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(id, name, _)| serde_json::json!({"id": id, "name": name}))
                    .collect();
                ok(serde_json::json!({ "files": files }))
            }
            (Method::Delete, p) => {
                let id = p.trim_start_matches("/drive/v3/files/").to_owned();
                self.files.lock().unwrap().retain(|(i, _, _)| *i != id);
                Ok(Reply::new(204, Vec::new()))
            }
            other => panic!("unexpected call {other:?}"),
        }
    }
}

/// The half on "this computer", kept in memory instead of Windows DPAPI.
#[derive(Default)]
struct FakeComputer(RefCell<Option<[u8; 32]>>);

impl ComputerKeys for FakeComputer {
    fn available(&self) -> bool {
        true
    }
    fn exists(&self, _: &Path) -> bool {
        self.0.borrow().is_some()
    }
    fn load(&self, _: &Path) -> Result<Key32, dv_vault::VaultError> {
        self.0
            .borrow()
            .map(Key32::from_bytes)
            .ok_or(dv_vault::VaultError::NotFound)
    }
    fn load_or_create(&self, dir: &Path) -> Result<Key32, dv_vault::VaultError> {
        if let Ok(k) = self.load(dir) {
            return Ok(k);
        }
        let k = Key32::random()?;
        *self.0.borrow_mut() = Some(*k.as_bytes());
        Ok(k)
    }
    fn remove(&self, _: &Path) {
        *self.0.borrow_mut() = None;
    }
}

fn vault() -> (tempfile::TempDir, Core) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    (dir, core)
}

/// Google connected here (D-041) with account "acct-1". Returns the connected account's tag.
fn connect_google(core: &mut Core, computer: &FakeComputer) -> String {
    let drive = Key32::random().unwrap();
    core.google_turn_on_finish(computer, PASSWORD, &drive, "acct-1")
        .unwrap();
    let vault_id = core.vault_ref().unwrap().vault_id().to_owned();
    google_account_tag(&vault_id, "acct-1")
}

/// Google connected and the copy on, with the lasting permission "1//lasting".
fn turned_on() -> (tempfile::TempDir, Core, FakeComputer) {
    let (dir, mut core) = vault();
    let computer = FakeComputer::default();
    let tag = connect_google(&mut core, &computer);
    core.drive_backup_turn_on_finish(&computer, &tag, "1//lasting")
        .unwrap();
    (dir, core, computer)
}

/// A local backup to `dest`, as the shell runs it: begin, run, finish (and the upload, if any).
fn backup(core: &mut Core, dest: &Path) -> (crate::views::BackupDone, Option<DriveUpload>) {
    let task = core.begin_backup(dest).unwrap();
    let run = task.run();
    let (done, upload) = core.finish_backup_for_drive(run);
    (done.unwrap(), upload)
}

/// Turn off, run the cleanup against `google`, and record it, as the shell does.
fn turn_off(core: &mut Core, google: &FakeGoogle) -> DriveCleanupResult {
    match core.drive_backup_turn_off().unwrap() {
        Some(cleanup) => core
            .finish_drive_cleanup(cleanup.run(google, &cfg()))
            .unwrap(),
        None => DriveCleanupResult::Off,
    }
}

#[test]
fn off_by_default_nothing_goes_up() {
    let (_dir, mut core) = vault();
    let status = core.drive_backup_status(&FakeComputer::default()).unwrap();
    assert!(!status.on && !status.cleanup_pending);
    assert_eq!(status.problem, None);
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    assert!(upload.is_none(), "an upload while off");
    // Turning off what was never on asks nothing of Google.
    assert!(core.drive_backup_turn_off().unwrap().is_none());
    assert!(core.take_drive_cleanup().is_none());
}

#[test]
fn turning_on_needs_google_connected_here_with_the_same_account() {
    let (_dir, mut core) = vault();
    let computer = FakeComputer::default();
    let err = core.drive_backup_turn_on_check(&computer).unwrap_err();
    assert!(
        matches!(&err, CoreError::Refused(t) if t.contains("כניסה עם גוגל")),
        "{err:?}"
    );
    assert!(!core.drive_backup_status(&computer).unwrap().connected);

    // Checked again when the permission comes back from the browser: Google disconnected
    // meanwhile, or another account, keeps nothing (the shell then withdraws it).
    let tag = connect_google(&mut core, &computer);
    assert!(core
        .drive_backup_turn_on_finish(&computer, "another-account", "1//x")
        .is_err());
    assert!(core
        .drive_backup_turn_on_finish(&FakeComputer::default(), &tag, "1//x")
        .is_err());
    assert!(!core.drive_backup_on());
    core.drive_backup_turn_on_finish(&computer, &tag, "1//lasting")
        .unwrap();
    assert!(core.drive_backup_on());
    // A second turn-on while on keeps the permission it has.
    core.drive_backup_turn_on_finish(&computer, &tag, "1//second")
        .unwrap();
    assert_eq!(core.drive_token().unwrap().as_str(), "1//lasting");
}

#[test]
fn when_on_exactly_the_sealed_backup_goes_up_and_nothing_else() {
    let (_dir, mut core, computer) = turned_on();
    assert!(core.drive_backup_status(&computer).unwrap().on);
    let out = tempfile::tempdir().unwrap();
    let (done, upload) = backup(&mut core, &out.path().join("b"));
    let on_disk = std::fs::read(&done.path).unwrap();
    let google = FakeGoogle::default();
    let uploaded = upload.unwrap().run(&google, &cfg());
    assert!(uploaded.ok());
    core.finish_drive_upload(uploaded).unwrap();

    // The body that went up is the file on her drive, byte for byte.
    let puts = google.calls(Method::Put, "/upload/");
    assert_eq!(puts.len(), 1);
    assert_eq!(puts[0].2, on_disk);
    // The only other things sent: the token exchange, the upload's name, the folder list.
    let seen = google.seen.lock().unwrap().clone();
    let kinds: Vec<(Method, String)> = seen
        .iter()
        .map(|(m, u, _)| (*m, reqwest_path(u).unwrap()))
        .collect();
    assert_eq!(
        kinds,
        [
            (Method::Post, "/token".to_owned()),
            (Method::Post, "/upload/drive/v3/files".to_owned()),
            (Method::Put, "/upload/drive/v3/files".to_owned()),
            (Method::Get, "/drive/v3/files".to_owned()),
        ]
    );
    let meta: serde_json::Value = serde_json::from_slice(&seen[1].2).unwrap();
    let vault_id = core.vault_ref().unwrap().vault_id().to_owned();
    assert_eq!(
        meta,
        serde_json::json!({
            "name": drive::copy_name(&vault_id, done.created_at),
            "parents": ["appDataFolder"],
            "mimeType": "application/octet-stream",
        })
    );
    // The token exchange carries the lasting permission and the app's registration only.
    let token_body = String::from_utf8(seen[0].2.clone()).unwrap();
    assert!(token_body.contains("grant_type=refresh_token"));

    let status = core.drive_backup_status(&computer).unwrap();
    assert_eq!(status.last_at, Some(done.created_at));
    assert_eq!(status.problem, None);
    let log = core.activity(None).unwrap().entries;
    assert!(log.iter().any(|e| e.event == "drive_backup_uploaded"));
    assert!(log.iter().any(|e| e.event == "drive_backup_on" && e.warn));
}

#[test]
fn only_the_three_latest_copies_stay() {
    let (_dir, mut core, _) = turned_on();
    let google = FakeGoogle::default();
    // Her sign-in key file in the same folder is never touched.
    let vault_id = core.vault_ref().unwrap().vault_id().to_owned();
    google.files.lock().unwrap().push((
        "key".to_owned(),
        google::key_file_name(&vault_id),
        b"k".to_vec(),
    ));
    let out = tempfile::tempdir().unwrap();
    let mut last = Vec::new();
    for i in 0..5 {
        let (done, upload) = backup(&mut core, &out.path().join(format!("b{i}")));
        last = std::fs::read(&done.path).unwrap();
        let up = upload.unwrap().run(&google, &cfg());
        core.finish_drive_upload(up).unwrap();
    }
    let files = google.files.lock().unwrap().clone();
    let copies: Vec<_> = files.iter().filter(|f| f.0 != "key").collect();
    assert_eq!(copies.len(), drive::KEEP);
    assert!(copies.iter().any(|f| f.2 == last), "the newest is kept");
    assert!(files.iter().any(|f| f.0 == "key"));
}

#[test]
fn a_failed_upload_leaves_the_local_backup_and_says_so_quietly() {
    let (_dir, mut core, computer) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (done, upload) = backup(&mut core, &out.path().join("b"));
    let offline = FakeGoogle {
        fail: Some(GoogleError::Offline),
        ..FakeGoogle::default()
    };
    let up = upload.unwrap().run(&offline, &cfg());
    assert!(!up.ok());
    core.finish_drive_upload(up).unwrap();
    // The local backup counts and is on the drive, whole.
    assert!(std::fs::metadata(&done.path).unwrap().len() > 0);
    let backup_status = core.backup_status().unwrap();
    assert_eq!(backup_status.last_at, Some(done.created_at));
    let status = core.drive_backup_status(&computer).unwrap();
    assert!(status.on);
    assert_eq!(status.problem, Some(DriveProblem::Offline));
    assert_eq!(status.last_at, None);
    // In the activity log too, by kind only.
    let log = core.activity(None).unwrap().entries;
    assert!(log.iter().any(|e| e.event == "drive_backup_failed"));

    // A withdrawn permission asks to connect again; the next backup that goes up clears it.
    let (_, upload) = backup(&mut core, &out.path().join("b2"));
    let withdrawn = FakeGoogle {
        withdrawn: true,
        ..FakeGoogle::default()
    };
    core.finish_drive_upload(upload.unwrap().run(&withdrawn, &cfg()))
        .unwrap();
    assert_eq!(
        core.drive_backup_status(&computer).unwrap().problem,
        Some(DriveProblem::Reconnect)
    );
    assert!(withdrawn.calls(Method::Put, "/upload/").is_empty());
    let (_, upload) = backup(&mut core, &out.path().join("b3"));
    core.finish_drive_upload(upload.unwrap().run(&FakeGoogle::default(), &cfg()))
        .unwrap();
    assert_eq!(core.drive_backup_status(&computer).unwrap().problem, None);
}

/// The connection could not be set up at all: the switch says so instead of "on" forever.
#[test]
fn an_upload_that_could_not_start_is_noted() {
    let (_dir, mut core, computer) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    core.finish_drive_upload(upload.unwrap().not_started())
        .unwrap();
    assert_eq!(
        core.drive_backup_status(&computer).unwrap().problem,
        Some(DriveProblem::Failed)
    );
}

#[test]
fn turning_off_forgets_the_permission_deletes_the_copies_and_withdraws_it() {
    let (_dir, mut core, computer) = turned_on();
    let google = FakeGoogle::default();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    core.finish_drive_upload(upload.unwrap().run(&google, &cfg()))
        .unwrap();
    assert_eq!(google.files.lock().unwrap().len(), 1);

    assert_eq!(turn_off(&mut core, &google), DriveCleanupResult::Cleaned);
    let status = core.drive_backup_status(&computer).unwrap();
    assert!(!status.on && !status.cleanup_pending);
    assert!(google.files.lock().unwrap().is_empty());
    let revoked = google.calls(Method::Post, "/revoke");
    assert_eq!(revoked.len(), 1);
    assert!(String::from_utf8(revoked[0].2.clone())
        .unwrap()
        .contains("token=1%2F%2Flasting"));
    // The next backup stays on her drive only, and nothing more is owed at Google.
    let (_, upload) = backup(&mut core, &out.path().join("b2"));
    assert!(upload.is_none());
    assert!(core.take_drive_cleanup().is_none());
    assert_eq!(turn_off(&mut core, &google), DriveCleanupResult::Off);
    let log = core.activity(None).unwrap().entries;
    assert!(log.iter().any(|e| e.event == "drive_backup_off" && e.warn));
    assert!(log.iter().any(|e| e.event == "drive_backup_cleanup"));
}

/// Turned off without internet: no copy goes up any more, the deleting and withdrawing stay
/// owed (sealed), a second click says so instead of "deleted", and it is tried again until
/// Google confirms.
#[test]
fn a_cleanup_that_did_not_finish_is_tried_again_until_google_confirms() {
    let (_dir, mut core, computer) = turned_on();
    let google = FakeGoogle::default();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    core.finish_drive_upload(upload.unwrap().run(&google, &cfg()))
        .unwrap();

    let offline = FakeGoogle {
        fail: Some(GoogleError::Offline),
        ..FakeGoogle::default()
    };
    assert_eq!(turn_off(&mut core, &offline), DriveCleanupResult::Pending);
    let status = core.drive_backup_status(&computer).unwrap();
    assert!(!status.on && status.cleanup_pending);
    let (_, upload) = backup(&mut core, &out.path().join("b2"));
    assert!(upload.is_none(), "no copy goes up once off");
    // Asked again (disconnecting Google does), it tries again and says how it went; the
    // activity log has the one row, not one per try.
    assert_eq!(turn_off(&mut core, &offline), DriveCleanupResult::Pending);
    let rows = |core: &mut Core| {
        core.activity(None)
            .unwrap()
            .entries
            .iter()
            .filter(|e| e.event == "drive_backup_cleanup")
            .count()
    };
    assert_eq!(rows(&mut core), 1);

    // A copy deleted but the withdrawal failed: still owed.
    let no_revoke = FakeGoogle {
        revoke_fails: true,
        ..FakeGoogle::default()
    };
    *no_revoke.files.lock().unwrap() = google.files.lock().unwrap().clone();
    assert_eq!(turn_off(&mut core, &no_revoke), DriveCleanupResult::Pending);
    assert!(no_revoke.files.lock().unwrap().is_empty());

    // The timer hands it out after an unlock, not again right away.
    core.lock();
    core.unlock(PASSWORD).unwrap();
    let again = core.take_drive_cleanup().expect("owed after an unlock");
    assert!(core.take_drive_cleanup().is_none(), "not twice at once");
    assert_eq!(
        core.finish_drive_cleanup(again.run(&google, &cfg()))
            .unwrap(),
        DriveCleanupResult::Cleaned
    );
    assert!(google.files.lock().unwrap().is_empty());
    assert!(!core.drive_backup_status(&computer).unwrap().cleanup_pending);
    assert_eq!(rows(&mut core), 2);
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.take_drive_cleanup().is_none());
}

/// The timer hands out no cleanup while one is on its way; a run that ends after another
/// finished the same cleanup says how that one went, and records nothing again.
#[test]
fn cleanups_do_not_overlap() {
    let (_dir, mut core, _) = turned_on();
    let offline = FakeGoogle {
        fail: Some(GoogleError::Offline),
        ..FakeGoogle::default()
    };
    assert_eq!(turn_off(&mut core, &offline), DriveCleanupResult::Pending);
    core.drive_cleanup_at = None;
    let first = core.take_drive_cleanup().unwrap();
    core.drive_cleanup_at = None;
    assert!(core.take_drive_cleanup().is_none(), "one on its way");
    let second = core.drive_backup_turn_off().unwrap().unwrap();
    let google = FakeGoogle::default();
    assert_eq!(
        core.finish_drive_cleanup(first.run(&google, &cfg()))
            .unwrap(),
        DriveCleanupResult::Cleaned
    );
    // The second one finds the permission withdrawn already, and is told it was cleaned.
    let withdrawn = FakeGoogle {
        withdrawn: true,
        ..FakeGoogle::default()
    };
    assert_eq!(
        core.finish_drive_cleanup(second.run(&withdrawn, &cfg()))
            .unwrap(),
        DriveCleanupResult::Cleaned
    );
    let log = core.activity(None).unwrap().entries;
    assert!(!log
        .iter()
        .any(|e| e.event == "drive_backup_cleanup" && e.warn));
    core.drive_cleanup_at = None;
    assert!(core.take_drive_cleanup().is_none(), "nothing owed");
}

/// Turned off without internet, then a Google sign-in (connecting again, or a forgotten
/// password): its end must not withdraw the grant, or the owed cleanup could no longer delete
/// the copies. The cleanup then deletes them and withdraws the grant itself.
#[test]
fn a_sign_in_after_turning_off_offline_leaves_the_cleanup_its_grant() {
    let (_dir, mut core, _) = turned_on();
    let google = FakeGoogle::default();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    core.finish_drive_upload(upload.unwrap().run(&google, &cfg()))
        .unwrap();
    let offline = FakeGoogle {
        fail: Some(GoogleError::Offline),
        ..FakeGoogle::default()
    };
    assert_eq!(turn_off(&mut core, &offline), DriveCleanupResult::Pending);

    // The sign-in's tidy-up, as the shell runs it.
    let keep_grant = core.drive_keeps_grant();
    assert!(keep_grant);
    let session = google::session_from_refresh(&FakeGoogle::default(), &cfg(), "short").unwrap();
    crate::google::tidy_up(&google, session, &[], keep_grant);
    // A lasting permission that came without its refresh token, or was not kept: not
    // withdrawn either.
    let vault_id = core.vault_ref().unwrap().vault_id().to_owned();
    let no_refresh = google::session_from_refresh(&FakeGoogle::default(), &cfg(), "short").unwrap();
    assert!(lasting_permission(
        &google,
        no_refresh,
        &vault_id,
        &google_account_tag(&vault_id, ""),
        keep_grant
    )
    .is_err());
    withdraw_unkept(&google, "1//unkept", keep_grant);
    assert!(google.calls(Method::Post, "/revoke").is_empty());

    core.drive_cleanup_at = None;
    let owed = core.take_drive_cleanup().unwrap();
    assert_eq!(
        core.finish_drive_cleanup(owed.run(&google, &cfg()))
            .unwrap(),
        DriveCleanupResult::Cleaned
    );
    assert!(google.files.lock().unwrap().is_empty(), "copies stranded");
    assert_eq!(google.calls(Method::Post, "/revoke").len(), 1);
    // Nothing rests on the grant any more: the next sign-in's end withdraws its token.
    assert!(!core.drive_keeps_grant());
}

/// The permission was withdrawn at Google before the cleanup: the copies can no longer be
/// reached from here, and she is told so instead of "deleted".
#[test]
fn a_cleanup_after_the_permission_was_withdrawn_says_so() {
    let (_dir, mut core, computer) = turned_on();
    let withdrawn = FakeGoogle {
        withdrawn: true,
        ..FakeGoogle::default()
    };
    assert_eq!(
        turn_off(&mut core, &withdrawn),
        DriveCleanupResult::Withdrawn
    );
    assert!(withdrawn.calls(Method::Get, "/drive/").is_empty());
    assert!(!core.drive_backup_status(&computer).unwrap().cleanup_pending);
    let log = core.activity(None).unwrap().entries;
    assert!(log
        .iter()
        .any(|e| e.event == "drive_backup_cleanup" && e.warn));
}

/// Turned on again while a cleanup is owed: the cleanup must not withdraw the new permission
/// (the same grant at Google).
#[test]
fn turning_on_again_drops_the_cleanup_that_would_withdraw_it() {
    let (_dir, mut core, computer) = turned_on();
    let offline = FakeGoogle {
        fail: Some(GoogleError::Offline),
        ..FakeGoogle::default()
    };
    assert_eq!(turn_off(&mut core, &offline), DriveCleanupResult::Pending);
    // A retry was handed out and is on its way when she turns it on again.
    core.drive_cleanup_at = None;
    let running = core.take_drive_cleanup().unwrap();
    // What the turn-on check does (it also needs this version registered with Google, which
    // tests are not): the generation moves before her browser opens.
    core.drive_moved();
    let vault_id = core.vault_ref().unwrap().vault_id().to_owned();
    let tag = google_account_tag(&vault_id, "acct-1");
    core.drive_backup_turn_on_finish(&computer, &tag, "1//new")
        .unwrap();
    let google = FakeGoogle::default();
    assert_eq!(
        core.finish_drive_cleanup(running.run(&google, &cfg()))
            .unwrap(),
        DriveCleanupResult::Off
    );
    assert!(google.calls(Method::Post, "/revoke").is_empty());
    assert!(google.seen.lock().unwrap().is_empty(), "nothing sent");
    let status = core.drive_backup_status(&computer).unwrap();
    assert!(status.on && !status.cleanup_pending);
}

#[test]
fn an_upload_that_ends_after_turning_off_is_not_recorded() {
    let (_dir, mut core, computer) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    let up = upload.unwrap().run(&FakeGoogle::default(), &cfg());
    let _ = core.drive_backup_turn_off().unwrap();
    core.finish_drive_upload(up).unwrap();
    assert_eq!(core.drive_backup_status(&computer).unwrap().last_at, None);
}

/// An upload waiting for its turn when she turns the copy off never sends its body.
#[test]
fn an_upload_waiting_when_turned_off_sends_nothing() {
    let (_dir, mut core, _) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    let _ = core.drive_backup_turn_off().unwrap();
    let google = FakeGoogle::default();
    let up = upload.unwrap().run(&google, &cfg());
    assert!(!up.ok());
    assert!(google.seen.lock().unwrap().is_empty(), "nothing sent");
}

/// Locked while an upload waits: it stops before anything is sent, holds no permission
/// afterwards, and nothing is recorded.
#[test]
fn a_lock_stops_an_upload_before_its_body() {
    let (_dir, mut core, computer) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    core.lock();
    let google = FakeGoogle::default();
    let up = upload.unwrap().run(&google, &cfg());
    assert!(!up.ok());
    assert!(google.calls(Method::Put, "/upload/").is_empty());
    assert!(google.seen.lock().unwrap().is_empty());
    assert!(core.finish_drive_upload(up).is_err(), "locked");
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.drive_backup_status(&computer).unwrap().problem, None);
}

/// The blocker of the review: a backup taken while the copy was on carries its permission. A
/// restore of it must not turn the copy on again without her.
#[test]
fn a_restored_backup_never_turns_the_copy_on() {
    let (_dir, mut core, _) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (done, upload) = backup(&mut core, &out.path().join("b"));
    core.finish_drive_upload(upload.unwrap().run(&FakeGoogle::default(), &cfg()))
        .unwrap();
    // She turns it off afterwards; the backup still carries the permission.
    let _ = core.drive_backup_turn_off().unwrap();
    let bytes = std::fs::read(&done.path).unwrap();

    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    other.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    other.restore_staged_backup(Some(PASSWORD), None).unwrap();
    let status = other.drive_backup_status(&FakeComputer::default()).unwrap();
    assert!(!status.on, "turned on by a restore");
    assert_eq!((status.last_at, status.problem), (None, None));
    // Her copies from before may be in her Drive: said, not deleted (they may be her only
    // ones away from the lost computer).
    assert!(status.copies_left && !status.cleanup_pending);
    let (_, upload) = backup(&mut other, &out.path().join("after"));
    assert!(upload.is_none(), "a copy went up after a restore");
    let log = other.activity(None).unwrap().entries;
    assert!(log.iter().any(|e| e.event == "drive_backup_off" && e.warn));

    // The permission the backup carried is withdrawn at Google, tried until confirmed; the
    // copies are left alone.
    let google = FakeGoogle::default();
    google
        .files
        .lock()
        .unwrap()
        .push(("old".to_owned(), "x".to_owned(), b"c".to_vec()));
    let failing = FakeGoogle {
        revoke_fails: true,
        ..FakeGoogle::default()
    };
    let owed = other.take_drive_cleanup().expect("the withdrawal");
    assert_eq!(
        other
            .finish_drive_cleanup(owed.run(&failing, &cfg()))
            .unwrap(),
        DriveCleanupResult::Pending
    );
    other.drive_cleanup_at = None;
    let owed = other.take_drive_cleanup().expect("tried again");
    assert_eq!(
        other
            .finish_drive_cleanup(owed.run(&google, &cfg()))
            .unwrap(),
        DriveCleanupResult::Cleaned
    );
    let revoked = google.calls(Method::Post, "/revoke");
    assert_eq!(revoked.len(), 1);
    assert!(String::from_utf8(revoked[0].2.clone())
        .unwrap()
        .contains("token=1%2F%2Flasting"));
    assert_eq!(google.seen.lock().unwrap().len(), 1, "only the withdrawal");
    assert_eq!(google.files.lock().unwrap().len(), 1, "the copies stay");
    other.drive_cleanup_at = None;
    assert!(other.take_drive_cleanup().is_none());
    assert!(
        other
            .drive_backup_status(&FakeComputer::default())
            .unwrap()
            .copies_left
    );
}

/// When the restored vault cannot be made sure to have the copy off, the restore fails and
/// leaves nothing behind (she can try again).
#[test]
fn a_restore_that_cannot_forget_the_permission_leaves_nothing() {
    let (_dir, mut core, _) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (done, _) = backup(&mut core, &out.path().join("b"));
    let bytes = std::fs::read(&done.path).unwrap();
    let fresh = tempfile::tempdir().unwrap();
    let failed = Vault::restore_backup_then(
        &bytes,
        fresh.path(),
        dv_vault::Secret::Password(PASSWORD),
        |_| Err(dv_vault::VaultError::NotFound),
    );
    assert!(failed.is_err());
    assert_eq!(std::fs::read_dir(fresh.path()).unwrap().count(), 0);
}

/// After a restore, turning the copy on and off deletes the copies from before it.
#[test]
fn turning_on_and_off_after_a_restore_deletes_the_copies_left() {
    let (_dir, mut core, _) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (done, _) = backup(&mut core, &out.path().join("b"));
    let bytes = std::fs::read(&done.path).unwrap();
    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    other.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    other.restore_staged_backup(Some(PASSWORD), None).unwrap();
    let vault_id = other.vault_ref().unwrap().vault_id().to_owned();
    let google = FakeGoogle::default();
    google.files.lock().unwrap().push((
        "old".to_owned(),
        drive::copy_name(&vault_id, done.created_at),
        b"c".to_vec(),
    ));
    let computer = FakeComputer::default();
    let tag = connect_google(&mut other, &computer);
    other
        .drive_backup_turn_on_finish(&computer, &tag, "1//again")
        .unwrap();
    // The restored permission is not withdrawn after this: it is the same grant.
    other.drive_cleanup_at = None;
    assert!(other.take_drive_cleanup().is_none());
    assert_eq!(turn_off(&mut other, &google), DriveCleanupResult::Cleaned);
    assert!(google.files.lock().unwrap().is_empty());
    let status = other.drive_backup_status(&computer).unwrap();
    assert!(!status.copies_left && !status.cleanup_pending);
}

/// A password changed while a backup was written: that backup opens with the old password and
/// never goes up. After the change, the next copy is the only one kept.
#[test]
fn copies_under_an_old_password_do_not_go_up_and_are_deleted_after_the_change() {
    let (_dir, mut core, _) = turned_on();
    let google = FakeGoogle::default();
    let out = tempfile::tempdir().unwrap();
    for i in 0..2 {
        let (_, upload) = backup(&mut core, &out.path().join(format!("old{i}")));
        core.finish_drive_upload(upload.unwrap().run(&google, &cfg()))
            .unwrap();
    }
    assert_eq!(google.files.lock().unwrap().len(), 2);

    let task = core.begin_backup(&out.path().join("during")).unwrap();
    core.change_password(PASSWORD, false, NEW_PASSWORD).unwrap();
    let (done, upload) = core.finish_backup_for_drive(task.run());
    assert!(done.is_ok(), "the local backup still counts");
    assert!(upload.is_none(), "a backup under the old password went up");

    let (_, upload) = backup(&mut core, &out.path().join("new"));
    core.finish_drive_upload(upload.unwrap().run(&google, &cfg()))
        .unwrap();
    assert_eq!(google.files.lock().unwrap().len(), 1, "only the new one");
    // Done with the old ones: the next copies keep three again.
    for i in 0..3 {
        let (_, upload) = backup(&mut core, &out.path().join(format!("n{i}")));
        core.finish_drive_upload(upload.unwrap().run(&google, &cfg()))
            .unwrap();
    }
    assert_eq!(google.files.lock().unwrap().len(), drive::KEEP);
}

/// Disconnecting Google (D-041) also turns the copy off, with its cleanup.
#[test]
fn disconnecting_google_turns_the_copy_off() {
    let (_dir, mut core, computer) = turned_on();
    let cleanup = core
        .google_turn_off(&computer, PASSWORD)
        .unwrap()
        .expect("the copy's cleanup");
    assert!(!core.drive_backup_on());
    let google = FakeGoogle::default();
    assert_eq!(
        core.finish_drive_cleanup(cleanup.run(&google, &cfg()))
            .unwrap(),
        DriveCleanupResult::Cleaned
    );
    assert_eq!(google.calls(Method::Post, "/revoke").len(), 1);
}

/// While the copy is on, the end of a D-041 action does not revoke its short token: at Google
/// that would withdraw the copy's lasting permission too.
#[test]
fn the_sign_in_tidy_up_keeps_the_grant_while_the_copy_is_on() {
    let google = FakeGoogle::default();
    let session =
        || google::session_from_refresh(&FakeGoogle::default(), &cfg(), "1//lasting").unwrap();
    crate::google::tidy_up(&google, session(), &[], true);
    assert!(google.calls(Method::Post, "/revoke").is_empty());
    crate::google::tidy_up(&google, session(), &[], false);
    assert_eq!(google.calls(Method::Post, "/revoke").len(), 1);
}

/// One worker, and only the newest upload waits: each one holds a whole backup in memory.
#[test]
fn only_the_newest_upload_waits_for_the_worker() {
    let (_dir, mut core, _) = turned_on();
    let out = tempfile::tempdir().unwrap();
    let (_, first) = backup(&mut core, &out.path().join("a"));
    let (second_done, second) = backup(&mut core, &out.path().join("b"));
    assert!(queue_upload(first.unwrap()), "start the worker");
    assert!(!queue_upload(second.unwrap()), "one worker only");
    let google = FakeGoogle::default();
    let mut ran = 0;
    while let Some(up) = next_upload() {
        core.finish_drive_upload(up.run(&google, &cfg())).unwrap();
        ran += 1;
    }
    assert_eq!(ran, 1, "the older one was dropped");
    let files = google.files.lock().unwrap();
    assert_eq!(files[0].2, std::fs::read(&second_done.path).unwrap());
    drop(files);
    // The worker ended: the next one starts a new worker.
    let (_, third) = backup(&mut core, &out.path().join("c"));
    assert!(queue_upload(third.unwrap()));
    assert!(next_upload().is_some());
    assert!(next_upload().is_none());
}
