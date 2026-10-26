//! An encrypted copy of each backup in her own Google Drive (D-099). Off by default.
//!
//! What goes up is the `.vaultbak` file exactly as it was written to her drive and read back:
//! sealed with the vault's own keys, so it opens only with her password or her printed
//! recovery kit. It goes into the app's hidden folder (`appDataFolder`, scope `drive.appdata`),
//! which no other app and none of her other files can see.
//!
//! Rules, as for every other connection in this crate:
//! * Nothing else leaves: the file name is the vault's random id and the backup's time (both
//!   digits and letters only, no case, no name), the type is `application/octet-stream`, and
//!   the folder is `appDataFolder`. No other metadata, no description, no properties.
//! * Only [`crate::google::GOOGLE_HOSTS`], through [`crate::google::Http`]; the upload address
//!   Google hands back must be on `www.googleapis.com` at the upload path, and the address the
//!   body goes to is rebuilt here from its `upload_id` alone, or nothing is sent.
//! * Only three Drive calls: start an upload into `appDataFolder` and send its body, list the
//!   files of `appDataFolder`, delete a file there. Only files named as this module names them
//!   for this vault are ever deleted; the sign-in key files are never touched.

use std::time::Duration;

use serde_json::Value;
use zeroize::Zeroizing;

use crate::google::{
    delete_file, json, GoogleError, Http, Method, Reply, Request, Session, FILES_URL,
};

/// Start a resumable upload; the body follows to this address with the `upload_id` Google
/// answers with. The answer names the new file and its size, to check what arrived.
const UPLOAD_START: &str =
    "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id,size";
/// Google's upload ids are short; this leaves room and refuses anything longer.
const MAX_UPLOAD_ID: usize = 512;
const UPLOAD_HOST: &str = "www.googleapis.com";
const UPLOAD_PATH: &str = "/upload/drive/v3/files";
/// Copies kept in her Drive; older ones are deleted after each upload.
pub const KEEP: usize = 3;
/// Far larger than a vault of text can grow (as `dv_core::MAX_BACKUP_BYTES`).
pub const MAX_COPY_BYTES: usize = 1 << 30;
const NAME_PREFIX: &str = "diagnostic-vault-";
const NAME_MIDDLE: &str = "-backup-";
const NAME_SUFFIX: &str = ".vaultbak";

/// One copy in the hidden folder: its Drive id and when the backup was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveCopy {
    pub id: String,
    pub created_at: i64,
}

fn safe_vault_id(vault_id: &str) -> String {
    vault_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// The copy's name: the vault's random id and the backup's time in seconds. Nothing else.
#[must_use]
pub fn copy_name(vault_id: &str, created_at: i64) -> String {
    format!(
        "{NAME_PREFIX}{}{NAME_MIDDLE}{}{NAME_SUFFIX}",
        safe_vault_id(vault_id),
        created_at.max(0)
    )
}

/// The backup time in a name this module made for this vault, or `None` for any other file.
fn copy_time(name: &str, vault_id: &str) -> Option<i64> {
    let head = format!("{NAME_PREFIX}{}{NAME_MIDDLE}", safe_vault_id(vault_id));
    let digits = name.strip_prefix(&head)?.strip_suffix(NAME_SUFFIX)?;
    if digits.is_empty() || digits.len() > 19 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn safe_file_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Long enough for a slow line: two minutes and a second for each 50 KB, at most four hours.
fn upload_timeout(bytes: usize) -> Duration {
    let secs = 120 + u64::try_from(bytes / 50_000).unwrap_or(u64::MAX);
    Duration::from_secs(secs.min(4 * 60 * 60))
}

/// The address the body goes to: Google's answer held to its own host and path, with no
/// fragment and no key twice, and then rebuilt from [`UPLOAD_START`] and its `upload_id` alone
/// (letters, digits, `-` and `_`), so nothing else Google's answer carries is followed.
fn upload_address(location: Option<&str>) -> Result<String, GoogleError> {
    let refuse = || GoogleError::Protocol("unexpected upload address".to_owned());
    let url = reqwest::Url::parse(location.ok_or_else(refuse)?).map_err(|_| refuse())?;
    let place = crate::google::host_allowed(&url)
        && url.host_str() == Some(UPLOAD_HOST)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.path() == UPLOAD_PATH;
    if !place {
        return Err(refuse());
    }
    let mut keys = std::collections::HashSet::new();
    let mut id = None;
    for (k, v) in url.query_pairs() {
        if !keys.insert(k.clone().into_owned()) {
            return Err(refuse());
        }
        if k == "upload_id" {
            id = Some(v.into_owned());
        }
    }
    let id = id
        .filter(|v| {
            !v.is_empty()
                && v.len() <= MAX_UPLOAD_ID
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or_else(refuse)?;
    Ok(format!("{UPLOAD_START}&upload_id={id}"))
}

/// Upload one sealed backup file into the hidden folder. Returns the new file's id.
///
/// `still_wanted` is asked right before the body goes out: when it says no (the vault was
/// locked, or the copy turned off, meanwhile), nothing is sent and the answer is
/// [`GoogleError::Cancelled`]. The upload started at Google then lapses on its own.
pub fn upload_copy(
    http: &dyn Http,
    s: &Session,
    vault_id: &str,
    created_at: i64,
    sealed: Vec<u8>,
    still_wanted: &dyn Fn() -> bool,
) -> Result<String, GoogleError> {
    if sealed.is_empty() || sealed.len() > MAX_COPY_BYTES {
        return Err(GoogleError::Protocol("backup size".to_owned()));
    }
    let meta = serde_json::json!({
        "name": copy_name(vault_id, created_at),
        "parents": ["appDataFolder"],
        "mimeType": "application/octet-stream",
    });
    let started = http.send(Request {
        method: Method::Post,
        url: UPLOAD_START.to_owned(),
        bearer: Some(s.token.clone()),
        content_type: Some("application/json; charset=UTF-8".to_owned()),
        body: Zeroizing::new(meta.to_string().into_bytes()),
        timeout: None,
    })?;
    if !(200..300).contains(&started.status) {
        return Err(GoogleError::Protocol(format!("status {}", started.status)));
    }
    let address = upload_address(started.location.as_deref())?;
    if !still_wanted() {
        return Err(GoogleError::Cancelled);
    }
    let size = sealed.len();
    let Reply { status, body, .. } = http.send(Request {
        method: Method::Put,
        url: address,
        bearer: Some(s.token.clone()),
        content_type: Some("application/octet-stream".to_owned()),
        body: Zeroizing::new(sealed),
        timeout: Some(upload_timeout(size)),
    })?;
    let v = json(status, &body)?;
    // Drive gives the size as a decimal string; it must be what was sent.
    let arrived = match v.get("size") {
        Some(Value::String(n)) => n.parse::<u64>().ok(),
        Some(n) => n.as_u64(),
        None => None,
    };
    if arrived != u64::try_from(size).ok() {
        return Err(GoogleError::Protocol(
            "the copy arrived with another size".to_owned(),
        ));
    }
    v.get("id")
        .and_then(Value::as_str)
        .filter(|id| safe_file_id(id))
        .map(str::to_owned)
        .ok_or_else(|| GoogleError::Protocol("no file id".to_owned()))
}

/// This vault's copies in the hidden folder, newest first. Other files there are left out.
pub fn list_copies(
    http: &dyn Http,
    s: &Session,
    vault_id: &str,
) -> Result<Vec<DriveCopy>, GoogleError> {
    // The folder holds a few files (a key, a few copies); the names are matched here, not in
    // a Drive query, so the query carries nothing but the folder.
    let url = reqwest::Url::parse_with_params(
        FILES_URL,
        &[
            ("spaces", "appDataFolder"),
            ("q", "trashed = false"),
            ("fields", "files(id,name),nextPageToken"),
            ("pageSize", "1000"),
        ],
    )
    .map_err(|e| GoogleError::Protocol(e.to_string()))?;
    let Reply { status, body, .. } = http.send(Request {
        method: Method::Get,
        url: url.to_string(),
        bearer: Some(s.token.clone()),
        content_type: None,
        body: Zeroizing::new(Vec::new()),
        timeout: None,
    })?;
    let v = json(status, &body)?;
    // Fail closed: an answer without its list, or with a second page, is not "no copies"
    // (turning off would then say the copies were deleted while some stay).
    let files = v
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| GoogleError::Protocol("no file list".to_owned()))?;
    if v.get("nextPageToken").is_some_and(|t| !t.is_null()) {
        return Err(GoogleError::Protocol(
            "more than one page of files".to_owned(),
        ));
    }
    let mut copies: Vec<DriveCopy> = files
        .iter()
        .filter_map(|f| {
            let id = f.get("id").and_then(Value::as_str)?;
            let name = f.get("name").and_then(Value::as_str)?;
            let created_at = copy_time(name, vault_id)?;
            safe_file_id(id).then(|| DriveCopy {
                id: id.to_owned(),
                created_at,
            })
        })
        .collect();
    copies.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    Ok(copies)
}

/// Delete one copy (permanently, not to the Drive bin).
pub fn delete_copy(http: &dyn Http, s: &Session, file_id: &str) -> Result<(), GoogleError> {
    if !safe_file_id(file_id) {
        return Err(GoogleError::Protocol("file id".to_owned()));
    }
    delete_file(http, s, file_id)
}

/// Keep the newest `keep` copies of this vault (always `just_uploaded`; [`KEEP`] as a rule,
/// one after her password or kit changed), delete the rest. Returns how many were deleted.
pub fn keep_latest(
    http: &dyn Http,
    s: &Session,
    vault_id: &str,
    just_uploaded: &str,
    keep: usize,
) -> Result<usize, GoogleError> {
    let copies = list_copies(http, s, vault_id)?;
    let mut kept = usize::from(copies.iter().any(|c| c.id == just_uploaded));
    let mut deleted = 0;
    for c in copies {
        if c.id == just_uploaded {
            continue;
        }
        if kept < keep {
            kept += 1;
            continue;
        }
        delete_copy(http, s, &c.id)?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Delete every copy of this vault (turning the copy off). Returns how many were deleted.
pub fn delete_all_copies(
    http: &dyn Http,
    s: &Session,
    vault_id: &str,
) -> Result<usize, GoogleError> {
    let copies = list_copies(http, s, vault_id)?;
    for c in &copies {
        delete_copy(http, s, &c.id)?;
    }
    Ok(copies.len())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Method, URL, content type and body of one request.
    type Seen = (Method, String, Option<String>, Vec<u8>);

    /// A fake Drive: keeps files by id, answers the three calls, refuses any host off the list.
    #[derive(Default)]
    struct FakeDrive {
        files: Mutex<Vec<(String, String, Vec<u8>)>>,
        seen: Mutex<Vec<Seen>>,
        pending: Mutex<Option<String>>,
        next: Mutex<u32>,
        /// Hand back this upload address instead of the real-looking one.
        location: Option<String>,
        /// Answer the upload's start with this status instead of 200.
        start_status: Option<u16>,
        /// Answer the file list with this instead of the files.
        listing: Option<Value>,
        /// Say the copy arrived with this size instead of its own.
        size: Option<Value>,
    }

    impl Http for FakeDrive {
        fn send(&self, req: Request) -> Result<Reply, GoogleError> {
            let url = reqwest::Url::parse(&req.url).unwrap();
            assert!(crate::google::host_allowed(&url), "{}", req.url);
            self.seen.lock().unwrap().push((
                req.method,
                req.url.clone(),
                req.content_type.clone(),
                req.body.to_vec(),
            ));
            let path = url.path().to_owned();
            match (req.method, path.as_str()) {
                (Method::Post, UPLOAD_PATH) => {
                    let meta: Value = serde_json::from_slice(&req.body).unwrap();
                    *self.pending.lock().unwrap() = Some(meta["name"].as_str().unwrap().to_owned());
                    Ok(Reply::pointing_to(
                        self.start_status.unwrap_or(200),
                        self.location.as_deref().unwrap_or(
                            "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id%2Csize&upload_id=AB-c_1",
                        ),
                    ))
                }
                (Method::Put, UPLOAD_PATH) => {
                    let name = self.pending.lock().unwrap().take().unwrap();
                    let mut n = self.next.lock().unwrap();
                    *n += 1;
                    let id = format!("f{n}");
                    let size = self
                        .size
                        .clone()
                        .unwrap_or_else(|| Value::from(req.body.len().to_string()));
                    self.files
                        .lock()
                        .unwrap()
                        .push((id.clone(), name, req.body.to_vec()));
                    Ok(Reply::new(
                        200,
                        serde_json::json!({ "id": id, "size": size })
                            .to_string()
                            .into_bytes(),
                    ))
                }
                (Method::Get, "/drive/v3/files") => {
                    // The list asks for the hidden folder, and for nothing about the names.
                    let q: Vec<(String, String)> = url.query_pairs().into_owned().collect();
                    assert!(q.contains(&("spaces".to_owned(), "appDataFolder".to_owned())));
                    assert!(q.contains(&("q".to_owned(), "trashed = false".to_owned())));
                    if let Some(answer) = &self.listing {
                        return Ok(Reply::new(200, answer.to_string().into_bytes()));
                    }
                    let files: Vec<Value> = self
                        .files
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(id, name, _)| serde_json::json!({"id": id, "name": name}))
                        .collect();
                    Ok(Reply::new(
                        200,
                        serde_json::json!({ "files": files })
                            .to_string()
                            .into_bytes(),
                    ))
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

    fn session() -> Session {
        Session::for_tests("ya29.token")
    }

    #[test]
    fn the_copy_is_exactly_the_sealed_file_in_the_hidden_folder_with_an_opaque_name() {
        let drive = FakeDrive::default();
        let sealed: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        let id = upload_copy(
            &drive,
            &session(),
            "V1x",
            1_790_683_200,
            sealed.clone(),
            &|| true,
        )
        .unwrap();
        assert_eq!(id, "f1");
        let seen = drive.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        // The start: only the name, the folder and the type.
        assert_eq!(seen[0].0, Method::Post);
        assert_eq!(seen[0].1, UPLOAD_START);
        let meta: Value = serde_json::from_slice(&seen[0].3).unwrap();
        assert_eq!(
            meta,
            serde_json::json!({
                "name": "diagnostic-vault-V1x-backup-1790683200.vaultbak",
                "parents": ["appDataFolder"],
                "mimeType": "application/octet-stream",
            })
        );
        // The body: the sealed bytes, nothing added.
        assert_eq!(seen[1].0, Method::Put);
        assert_eq!(
            seen[1].1,
            format!("{UPLOAD_START}&upload_id=AB-c_1"),
            "rebuilt from the upload id alone"
        );
        assert_eq!(seen[1].3, sealed);
        assert_eq!(seen[1].2.as_deref(), Some("application/octet-stream"));
        assert_eq!(drive.files.lock().unwrap()[0].2, sealed);
    }

    #[test]
    fn only_the_three_latest_copies_of_this_vault_are_kept() {
        let drive = FakeDrive::default();
        let s = session();
        // A sign-in key file and another vault's copy share the folder; neither is touched.
        drive.files.lock().unwrap().extend([
            (
                "key1".to_owned(),
                "diagnostic-vault-V1.key".to_owned(),
                b"k".to_vec(),
            ),
            ("other".to_owned(), copy_name("V2", 10), b"theirs".to_vec()),
            (
                "odd".to_owned(),
                "diagnostic-vault-V1-backup-12x.vaultbak".to_owned(),
                Vec::new(),
            ),
        ]);
        let mut last = String::new();
        for t in 1..=5 {
            last = upload_copy(&drive, &s, "V1", 100 + t, vec![1, 2, 3], &|| true).unwrap();
            keep_latest(&drive, &s, "V1", &last, KEEP).unwrap();
        }
        let copies = list_copies(&drive, &s, "V1").unwrap();
        assert_eq!(
            copies.iter().map(|c| c.created_at).collect::<Vec<_>>(),
            [105, 104, 103]
        );
        assert_eq!(copies[0].id, last);
        let names: Vec<String> = drive
            .files
            .lock()
            .unwrap()
            .iter()
            .map(|f| f.0.clone())
            .collect();
        for keep in ["key1", "other", "odd"] {
            assert!(names.contains(&keep.to_owned()), "{keep} was deleted");
        }
        assert_eq!(delete_all_copies(&drive, &s, "V1").unwrap(), 3);
        assert_eq!(drive.files.lock().unwrap().len(), 3);
    }

    #[test]
    fn an_upload_address_off_the_list_is_refused_and_the_file_never_sent() {
        for bad in [
            "https://evil.example/upload/drive/v3/files?upload_id=A",
            "https://www.googleapis.com.evil.example/upload/drive/v3/files?upload_id=A",
            "http://www.googleapis.com/upload/drive/v3/files?upload_id=A",
            "https://oauth2.googleapis.com/upload/drive/v3/files?upload_id=A",
            "https://www.googleapis.com/drive/v3/files?upload_id=A",
            "https://www.googleapis.com/upload/drive/v3/files",
            "https://www.googleapis.com:8443/upload/drive/v3/files?upload_id=A",
            "https://u:@www.googleapis.com/upload/drive/v3/files?upload_id=A",
            "https://www.googleapis.com/upload/drive/v3/files?upload_id=A#x",
            "https://www.googleapis.com/upload/drive/v3/files?upload_id=A&upload_id=B",
            "https://www.googleapis.com/upload/drive/v3/files?upload_id=A&x=1&x=2",
            "https://www.googleapis.com/upload/drive/v3/files?upload_id=A%26uploadType%3Dmedia",
            "https://www.googleapis.com/upload/drive/v3/files?upload_id=a.b",
            "https://www.googleapis.com/upload/drive/v3/files?upload_id=",
            "not a url",
        ] {
            let drive = FakeDrive {
                location: Some(bad.to_owned()),
                ..FakeDrive::default()
            };
            let got = upload_copy(&drive, &session(), "V1", 1, vec![9; 10], &|| true);
            assert!(matches!(got, Err(GoogleError::Protocol(_))), "{bad}");
            let seen = drive.seen.lock().unwrap();
            assert_eq!(seen.len(), 1, "{bad}: the body went out");
        }
    }

    #[test]
    fn names_hold_only_the_vault_id_and_the_time() {
        assert_eq!(
            copy_name("x' or name != 'y", 5),
            "diagnostic-vault-xornamey-backup-5.vaultbak"
        );
        assert_eq!(copy_time(&copy_name("V1", 77), "V1"), Some(77));
        assert_eq!(copy_time(&copy_name("V1", 77), "V2"), None);
        assert_eq!(copy_time("diagnostic-vault-V1.key", "V1"), None);
        assert_eq!(
            copy_time("diagnostic-vault-V1-backup-.vaultbak", "V1"),
            None
        );
        assert_eq!(
            copy_time("diagnostic-vault-V1-backup--1.vaultbak", "V1"),
            None
        );
    }

    #[test]
    fn an_empty_or_huge_file_is_not_sent() {
        let drive = FakeDrive::default();
        assert!(upload_copy(&drive, &session(), "V1", 1, Vec::new(), &|| true).is_err());
        assert!(drive.seen.lock().unwrap().is_empty());
        assert!(upload_timeout(MAX_COPY_BYTES) <= Duration::from_secs(4 * 60 * 60));
        assert_eq!(upload_timeout(0), Duration::from_secs(120));
    }

    #[test]
    fn an_upload_id_longer_than_any_google_gives_is_refused() {
        let long = format!(
            "https://www.googleapis.com/upload/drive/v3/files?upload_id={}",
            "A".repeat(MAX_UPLOAD_ID + 1)
        );
        assert!(upload_address(Some(&long)).is_err());
        let ok = format!(
            "https://www.googleapis.com/upload/drive/v3/files?upload_id={}",
            "A".repeat(MAX_UPLOAD_ID)
        );
        assert!(upload_address(Some(&ok)).is_ok());
    }

    /// A redirect is never followed: an answer that is not a success sends nothing more.
    #[test]
    fn a_redirect_at_the_start_sends_no_body() {
        for status in [301, 302, 307, 308, 403] {
            let drive = FakeDrive {
                start_status: Some(status),
                ..FakeDrive::default()
            };
            assert!(upload_copy(&drive, &session(), "V1", 1, vec![9; 10], &|| true).is_err());
            assert_eq!(drive.seen.lock().unwrap().len(), 1, "{status}");
        }
    }

    /// Locked or turned off between the start and the body: the body never goes out.
    #[test]
    fn when_no_longer_wanted_the_body_is_not_sent() {
        let drive = FakeDrive::default();
        let got = upload_copy(&drive, &session(), "V1", 1, vec![9; 10], &|| false);
        assert_eq!(got.unwrap_err(), GoogleError::Cancelled);
        let seen = drive.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, Method::Post);
    }

    #[test]
    fn a_copy_that_arrived_with_another_size_does_not_count() {
        for size in [
            serde_json::json!("9"),
            serde_json::json!(null),
            serde_json::json!("x"),
        ] {
            let drive = FakeDrive {
                size: Some(size.clone()),
                ..FakeDrive::default()
            };
            assert!(
                upload_copy(&drive, &session(), "V1", 1, vec![9; 10], &|| true).is_err(),
                "{size}"
            );
        }
        let drive = FakeDrive {
            size: Some(serde_json::json!(10)),
            ..FakeDrive::default()
        };
        assert!(upload_copy(&drive, &session(), "V1", 1, vec![9; 10], &|| true).is_ok());
    }

    /// Fail closed: an answer without its list, or with a second page, is not "no copies".
    #[test]
    fn a_list_without_its_files_or_with_more_pages_is_an_error() {
        for answer in [
            serde_json::json!({}),
            serde_json::json!({ "files": "x" }),
            serde_json::json!({ "files": [], "nextPageToken": "p2" }),
        ] {
            let drive = FakeDrive {
                listing: Some(answer.clone()),
                ..FakeDrive::default()
            };
            assert!(list_copies(&drive, &session(), "V1").is_err(), "{answer}");
            assert!(
                delete_all_copies(&drive, &session(), "V1").is_err(),
                "{answer}"
            );
        }
        let drive = FakeDrive {
            listing: Some(serde_json::json!({ "files": [], "nextPageToken": null })),
            ..FakeDrive::default()
        };
        assert_eq!(list_copies(&drive, &session(), "V1").unwrap(), []);
    }

    /// Neither a request's printout nor an error's text carries a token.
    #[test]
    fn no_token_in_printouts_or_errors() {
        let req = Request {
            method: Method::Put,
            url: UPLOAD_START.to_owned(),
            bearer: Some(Zeroizing::new("ya29.secret".to_owned())),
            content_type: None,
            body: Zeroizing::new(b"1//refresh".to_vec()),
            timeout: None,
        };
        let printed = format!("{req:?} {:?}", session());
        assert!(
            !printed.contains("ya29") && !printed.contains("1//"),
            "{printed}"
        );
        let drive = FakeDrive {
            location: Some("https://evil.example/?token=ya29.token".to_owned()),
            ..FakeDrive::default()
        };
        let e = upload_copy(&drive, &session(), "V1", 1, vec![9; 10], &|| true).unwrap_err();
        let text = format!("{e} {e:?}");
        assert!(!text.contains("ya29"), "{text}");
    }

    #[test]
    fn a_strange_file_id_is_never_deleted() {
        let drive = FakeDrive::default();
        for id in ["", "a/../b", "x?y", "f 1"] {
            assert!(delete_copy(&drive, &session(), id).is_err());
        }
        assert!(drive.seen.lock().unwrap().is_empty());
    }
}
