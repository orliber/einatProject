//! A new version of the program (D-033): is there one, and fetch it verified.
//!
//! Nothing here touches the vault. The check and the download run without the core's lock, so
//! work continues while the file arrives. The installer is written next to the vault folder
//! (`updates/`), checked again just before it runs, and deleted on the next start.
//!
//! D-038: when a newer version is found it is fetched in the background (`prepare`), so the
//! click only restarts. Nothing is installed without that click.

use std::fs;
use std::path::{Path, PathBuf};

use dv_egress::update::{self, Fetch, GitHubFetch, UpdateError, UpdateInfo};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::CoreError;

/// The version running now.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const FOLDER: &str = "updates";

/// A newer version, shown to her before anything is downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateView {
    pub version: String,
    pub current: String,
    pub published: String,
    pub notes: Vec<String>,
    pub size_mb: u32,
    /// The installer runs by itself only on Windows; elsewhere she is told where to get it.
    pub can_install: bool,
}

/// Checked, ready to run.
#[derive(Debug)]
pub struct Downloaded {
    pub version: String,
    pub path: PathBuf,
    pub sha256: [u8; 32],
}

fn view(info: &UpdateInfo) -> UpdateView {
    UpdateView {
        version: info.version.clone(),
        current: CURRENT_VERSION.to_owned(),
        published: info.published.clone(),
        notes: info.notes.clone(),
        size_mb: u32::try_from(info.size.div_ceil(1024 * 1024)).unwrap_or(u32::MAX),
        can_install: cfg!(windows),
    }
}

fn key() -> Result<[u8; 32], CoreError> {
    update::pinned_key().ok_or(CoreError::Update(UpdateError::NotConfigured))
}

/// Is there a newer version? `None` when this is the newest.
pub fn check() -> Result<Option<UpdateView>, CoreError> {
    let fetch = GitHubFetch::new()?;
    check_with(&fetch, &key()?, CURRENT_VERSION)
}

pub fn check_with(
    fetch: &dyn Fetch,
    key: &[u8; 32],
    current: &str,
) -> Result<Option<UpdateView>, CoreError> {
    Ok(update::check(fetch, current, key)?.as_ref().map(view))
}

/// Fetch the newest installer. The notice is fetched and verified again (not trusted from
/// the earlier check), and the file is written only after its hash matched.
pub fn download(dir: &Path) -> Result<Downloaded, CoreError> {
    let fetch = GitHubFetch::new()?;
    download_with(&fetch, &key()?, CURRENT_VERSION, dir)
}

pub fn download_with(
    fetch: &dyn Fetch,
    key: &[u8; 32],
    current: &str,
    dir: &Path,
) -> Result<Downloaded, CoreError> {
    let info = update::check(fetch, current, key)?
        .ok_or_else(|| CoreError::Refused("זו כבר הגרסה החדשה ביותר.".to_owned()))?;
    write_installer(fetch, &info, dir)
}

/// In the background, before she is asked (D-038): fetch the newest installer, verified, so
/// the click only restarts. The notice is verified again here. A file fetched earlier for the
/// same version is kept when it is still the one the notice names; anything else is fetched
/// again. `None` (and nothing on disk) when there is no newer version.
pub fn prepare(dir: &Path, earlier: Option<Downloaded>) -> Result<Option<Downloaded>, CoreError> {
    let fetch = GitHubFetch::new()?;
    prepare_with(&fetch, &key()?, CURRENT_VERSION, dir, earlier)
}

pub fn prepare_with(
    fetch: &dyn Fetch,
    key: &[u8; 32],
    current: &str,
    dir: &Path,
    earlier: Option<Downloaded>,
) -> Result<Option<Downloaded>, CoreError> {
    let Some(info) = update::check(fetch, current, key)? else {
        clean(dir);
        return Ok(None);
    };
    if let Some(file) = earlier {
        if file.version == info.version && file.sha256 == info.sha256 && still_intact(&file) {
            return Ok(Some(file));
        }
    }
    write_installer(fetch, &info, dir).map(Some)
}

/// The installer of a verified notice, written only after its hash matched.
fn write_installer(
    fetch: &dyn Fetch,
    info: &UpdateInfo,
    dir: &Path,
) -> Result<Downloaded, CoreError> {
    let bytes = update::download(fetch, info)?;
    let folder = dir.join(FOLDER);
    fs::create_dir_all(&folder).map_err(|e| CoreError::Internal(e.to_string()))?;
    // Only digits and dots reach the file name (the version was parsed as x.y.z).
    let name = format!(
        "DiagnosticVault-Setup-{}.exe",
        info.version.trim_start_matches('v')
    );
    let path = folder.join(name);
    fs::write(&path, &bytes).map_err(|e| CoreError::Internal(e.to_string()))?;
    Ok(Downloaded {
        version: info.version.clone(),
        path,
        sha256: info.sha256,
    })
}

/// Just before the installer runs: the file on disk is still the one that was verified.
pub fn still_intact(file: &Downloaded) -> bool {
    fs::read(&file.path).is_ok_and(|b| update::sha256(&b) == file.sha256)
}

/// Installers from earlier updates are not kept.
pub fn clean(dir: &Path) {
    let _ = fs::remove_dir_all(dir.join(FOLDER));
}

impl From<UpdateError> for CoreError {
    fn from(e: UpdateError) -> Self {
        CoreError::Update(e)
    }
}

pub(crate) fn update_he(e: &UpdateError) -> String {
    match e {
        UpdateError::NotConfigured => "בגרסה הזו עדיין אין עדכונים אוטומטיים.".to_owned(),
        UpdateError::Offline | UpdateError::Server(_) => {
            "אין כרגע חיבור לשרת העדכונים. אפשר לנסות שוב אחר כך; התוכנה ממשיכה לעבוד כרגיל."
                .to_owned()
        }
        UpdateError::BadSignature | UpdateError::BadNotice(_) | UpdateError::HostRefused(_) => {
            "העדכון שהתקבל לא חתום כמו שצריך, ולכן לא הותקן. כדאי לספר לאור.".to_owned()
        }
        UpdateError::Tampered | UpdateError::TooLarge => {
            "הקובץ שהורד לא תואם את מה שאור פרסם, ולכן נמחק ולא הותקן. כדאי לספר לאור.".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
    use dv_egress::update::{signed_bytes, to_hex, PRODUCT};
    use std::collections::HashMap;

    struct Table(HashMap<String, Vec<u8>>);
    impl Fetch for Table {
        fn get(&self, url: &str, _max: u64) -> Result<Vec<u8>, UpdateError> {
            self.0.get(url).cloned().ok_or(UpdateError::Server(404))
        }
    }

    const BASE: &str = "https://github.com/orliber/einat-vault-releases/releases";

    fn server(installer: &[u8], served: &[u8]) -> (Table, [u8; 32]) {
        let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let k = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let url = format!("{BASE}/download/v0.3.0/DiagnosticVault-Setup.exe");
        let notice = serde_json::to_vec(&serde_json::json!({
            "product": PRODUCT, "version": "0.3.0", "published": "2026-10-01",
            "notes": ["עדכון לדוגמה"],
            "installer": {"url": url, "sha256": to_hex(&update::sha256(installer)), "size": installer.len()}
        }))
        .unwrap();
        let sig = to_hex(k.sign(&signed_bytes(&notice)).as_ref());
        let table = Table(HashMap::from([
            (format!("{BASE}/latest/download/latest.json"), notice),
            (
                format!("{BASE}/latest/download/latest.json.sig"),
                sig.into_bytes(),
            ),
            (url, served.to_vec()),
        ]));
        (table, k.public_key().as_ref().try_into().unwrap())
    }

    #[test]
    fn a_verified_installer_is_written_checked_again_and_cleaned_later() {
        let dir = tempfile::tempdir().unwrap();
        let (t, key) = server(b"MZ installer", b"MZ installer");
        let v = check_with(&t, &key, "0.2.9").unwrap().unwrap();
        assert_eq!((v.version.as_str(), v.size_mb), ("0.3.0", 1));
        assert!(check_with(&t, &key, "0.3.0").unwrap().is_none());

        let file = download_with(&t, &key, "0.2.9", dir.path()).unwrap();
        assert!(file
            .path
            .ends_with("updates/DiagnosticVault-Setup-0.3.0.exe"));
        assert!(still_intact(&file));
        fs::write(&file.path, b"MZ swapped").unwrap();
        assert!(
            !still_intact(&file),
            "a file changed after the check never runs"
        );
        clean(dir.path());
        assert!(!dir.path().join(FOLDER).exists());
    }

    #[test]
    fn a_prepared_installer_is_kept_while_it_is_still_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let (t, key) = server(b"MZ installer", b"MZ installer");
        let first = prepare_with(&t, &key, "0.2.9", dir.path(), None)
            .unwrap()
            .unwrap();
        assert_eq!(first.version, "0.3.0");
        let path = first.path.clone();

        // The same version again: the file on disk is reused, not fetched twice.
        let mut quiet = Table(t.0.clone());
        quiet
            .0
            .retain(|url, _| url.ends_with(".json") || url.ends_with(".sig"));
        let again = prepare_with(&quiet, &key, "0.2.9", dir.path(), Some(first))
            .unwrap()
            .unwrap();
        assert_eq!(again.path, path);

        // Changed on disk since: fetched again, and verified again.
        fs::write(&path, b"MZ swapped").unwrap();
        assert!(prepare_with(&quiet, &key, "0.2.9", dir.path(), Some(again)).is_err());
        let fresh = prepare_with(&t, &key, "0.2.9", dir.path(), None)
            .unwrap()
            .unwrap();
        assert!(still_intact(&fresh));

        // Already the newest: nothing prepared, nothing left on disk.
        assert!(prepare_with(&t, &key, "0.3.0", dir.path(), Some(fresh))
            .unwrap()
            .is_none());
        assert!(!dir.path().join(FOLDER).exists());
    }

    #[test]
    fn a_changed_installer_is_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let (t, key) = server(b"MZ installer", b"MZ evil");
        let err = download_with(&t, &key, "0.2.9", dir.path()).unwrap_err();
        assert_eq!(err.to_ui().code, "update");
        assert!(!dir.path().join(FOLDER).exists());
        let err = prepare_with(&t, &key, "0.2.9", dir.path(), None).unwrap_err();
        assert_eq!(err.to_ui().code, "update");
        assert!(!dir.path().join(FOLDER).exists());
    }
}
