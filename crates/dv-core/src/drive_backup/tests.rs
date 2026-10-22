//! The encrypted copy in Drive, against a fake Google (never the real one).

use super::*;
use dv_egress::google::{Method, Reply, Request, GOOGLE_HOSTS};
use std::path::Path;

const PASSWORD: &str = "סוס ירוק רץ בשדה 2026";

fn cfg() -> ClientConfig {
    ClientConfig {
        client_id: "client-1.apps.googleusercontent.com".to_owned(),
        client_secret: "public-secret".to_owned(),
    }
}

/// Method, URL and body of one request.
type Seen = (Method, String, Vec<u8>);

/// A fake Google: a token endpoint, the hidden folder, and a way to fail.
#[derive(Default)]
struct FakeGoogle {
    files: Mutex<Vec<(String, String, Vec<u8>)>>,
    seen: Mutex<Vec<Seen>>,
    pending: Mutex<Option<String>>,
    next: Mutex<u32>,
    fail: Option<GoogleError>,
    withdrawn: bool,
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
            .push((req.method, req.url.clone(), req.body.clone()));
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        let ok = |v: serde_json::Value| Ok(Reply::new(200, v.to_string().into_bytes()));
        match (req.method, path.as_str()) {
            (Method::Post, "/token") if self.withdrawn => {
                Ok(Reply::new(400, br#"{"error":"invalid_grant"}"#.to_vec()))
            }
            (Method::Post, "/token") => ok(serde_json::json!({"access_token": "ya29.short"})),
            (Method::Post, "/revoke") => Ok(Reply::new(200, Vec::new())),
            (Method::Post, "/upload/drive/v3/files") => {
                let meta: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                *self.pending.lock().unwrap() = Some(meta["name"].as_str().unwrap().to_owned());
                Ok(Reply {
                    status: 200,
                    body: Vec::new(),
                    location: Some(
                        "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&upload_id=U1"
                            .to_owned(),
                    ),
                })
            }
            (Method::Put, "/upload/drive/v3/files") => {
                let name = self.pending.lock().unwrap().take().unwrap();
                let mut n = self.next.lock().unwrap();
                *n += 1;
                let id = format!("f{n}");
                self.files
                    .lock()
                    .unwrap()
                    .push((id.clone(), name, req.body));
                ok(serde_json::json!({ "id": id }))
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

/// The half on "this computer" is never there: Google is not connected in these tests.
struct NoComputer;

impl ComputerKeys for NoComputer {
    fn available(&self) -> bool {
        true
    }
    fn exists(&self, _: &Path) -> bool {
        false
    }
    fn load(&self, _: &Path) -> Result<dv_vault::crypto::Key32, dv_vault::VaultError> {
        Err(dv_vault::VaultError::NotFound)
    }
    fn load_or_create(&self, _: &Path) -> Result<dv_vault::crypto::Key32, dv_vault::VaultError> {
        Err(dv_vault::VaultError::NotFound)
    }
    fn remove(&self, _: &Path) {}
}

fn vault() -> (tempfile::TempDir, Core) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    (dir, core)
}

/// A local backup to `dest`, as the shell runs it: begin, run, finish (and the upload, if any).
fn backup(core: &mut Core, dest: &Path) -> (crate::views::BackupDone, Option<DriveUpload>) {
    let task = core.begin_backup(dest).unwrap();
    let run = task.run();
    let (done, upload) = core.finish_backup_for_drive(run);
    (done.unwrap(), upload)
}

#[test]
fn off_by_default_nothing_goes_up() {
    let (_dir, mut core) = vault();
    let status = core.drive_backup_status(&NoComputer).unwrap();
    assert!(!status.on);
    assert_eq!(status.problem, None);
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    assert!(upload.is_none(), "an upload while off");
    // Turning off what was never on asks nothing of Google.
    assert!(core.drive_backup_turn_off().unwrap().is_none());
}

#[test]
fn turning_on_needs_google_connected_here() {
    let (_dir, mut core) = vault();
    let err = core.drive_backup_turn_on_check(&NoComputer).unwrap_err();
    assert!(
        matches!(&err, CoreError::Refused(t) if t.contains("כניסה עם גוגל")),
        "{err:?}"
    );
    assert!(!core.drive_backup_status(&NoComputer).unwrap().connected);
}

#[test]
fn when_on_exactly_the_sealed_backup_goes_up_and_nothing_else() {
    let (_dir, mut core) = vault();
    core.drive_backup_turn_on_finish("1//lasting").unwrap();
    assert!(core.drive_backup_status(&NoComputer).unwrap().on);
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

    let status = core.drive_backup_status(&NoComputer).unwrap();
    assert_eq!(status.last_at, Some(done.created_at));
    assert_eq!(status.problem, None);
    let log = core.activity(None).unwrap().entries;
    assert!(log.iter().any(|e| e.event == "drive_backup_uploaded"));
    assert!(log.iter().any(|e| e.event == "drive_backup_on" && e.warn));
}

#[test]
fn only_the_three_latest_copies_stay() {
    let (_dir, mut core) = vault();
    core.drive_backup_turn_on_finish("1//lasting").unwrap();
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
    let (_dir, mut core) = vault();
    core.drive_backup_turn_on_finish("1//lasting").unwrap();
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
    let status = core.drive_backup_status(&NoComputer).unwrap();
    assert!(status.on);
    assert_eq!(status.problem, Some(DriveProblem::Offline));
    assert_eq!(status.last_at, None);

    // A withdrawn permission asks to connect again; the next backup that goes up clears it.
    let (_, upload) = backup(&mut core, &out.path().join("b2"));
    let withdrawn = FakeGoogle {
        withdrawn: true,
        ..FakeGoogle::default()
    };
    core.finish_drive_upload(upload.unwrap().run(&withdrawn, &cfg()))
        .unwrap();
    assert_eq!(
        core.drive_backup_status(&NoComputer).unwrap().problem,
        Some(DriveProblem::Reconnect)
    );
    assert!(withdrawn.calls(Method::Put, "/upload/").is_empty());
    let (_, upload) = backup(&mut core, &out.path().join("b3"));
    core.finish_drive_upload(upload.unwrap().run(&FakeGoogle::default(), &cfg()))
        .unwrap();
    assert_eq!(core.drive_backup_status(&NoComputer).unwrap().problem, None);
}

#[test]
fn turning_off_forgets_the_permission_deletes_the_copies_and_withdraws_it() {
    let (_dir, mut core) = vault();
    core.drive_backup_turn_on_finish("1//lasting").unwrap();
    let google = FakeGoogle::default();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    core.finish_drive_upload(upload.unwrap().run(&google, &cfg()))
        .unwrap();
    assert_eq!(google.files.lock().unwrap().len(), 1);

    let cleanup = core.drive_backup_turn_off().unwrap().unwrap();
    assert!(!core.drive_backup_status(&NoComputer).unwrap().on);
    assert!(cleanup.run(&google, &cfg()));
    assert!(google.files.lock().unwrap().is_empty());
    let revoked = google.calls(Method::Post, "/revoke");
    assert_eq!(revoked.len(), 1);
    assert!(String::from_utf8(revoked[0].2.clone())
        .unwrap()
        .contains("token=1%2F%2Flasting"));
    // The next backup stays on her drive only.
    let (_, upload) = backup(&mut core, &out.path().join("b2"));
    assert!(upload.is_none());
    let log = core.activity(None).unwrap().entries;
    assert!(log.iter().any(|e| e.event == "drive_backup_off" && e.warn));
}

#[test]
fn an_upload_that_ends_after_turning_off_is_not_recorded() {
    let (_dir, mut core) = vault();
    core.drive_backup_turn_on_finish("1//lasting").unwrap();
    let out = tempfile::tempdir().unwrap();
    let (_, upload) = backup(&mut core, &out.path().join("b"));
    let up = upload.unwrap().run(&FakeGoogle::default(), &cfg());
    let _ = core.drive_backup_turn_off().unwrap();
    core.finish_drive_upload(up).unwrap();
    assert_eq!(core.drive_backup_status(&NoComputer).unwrap().last_at, None);
}
