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
/// Every released version, for going back one when a new version cannot open the vault (R7).
/// The only page besides Google's sign-in the program ever opens in her browser.
pub const RELEASES_PAGE: &str = "https://github.com/orliber/einat-vault-releases/releases";
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
    /// The new version installs by itself on Windows and macOS (D-101); elsewhere she is told
    /// where to get it.
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
        can_install: cfg!(any(windows, target_os = "macos")),
    }
}

/// The file a version is written as: the setup program on Windows, the disk image on a Mac.
/// Only digits and dots reach the name (the version was parsed as x.y.z).
fn installer_name(version: &str) -> String {
    let v = version.trim_start_matches('v');
    if cfg!(target_os = "macos") {
        format!("DiagnosticVault-{v}.dmg")
    } else {
        format!("DiagnosticVault-Setup-{v}.exe")
    }
}

/// The pinned keys: the one in use and, once there is one, the next (SEC5).
fn keys() -> Result<Vec<[u8; 32]>, CoreError> {
    let keys = update::pinned_keys();
    if keys.is_empty() {
        return Err(CoreError::Update(UpdateError::NotConfigured));
    }
    Ok(keys)
}

/// Is there a newer version? `None` when this is the newest.
pub fn check() -> Result<Option<UpdateView>, CoreError> {
    let fetch = GitHubFetch::new()?;
    check_with(&fetch, &keys()?, CURRENT_VERSION)
}

pub fn check_with(
    fetch: &dyn Fetch,
    keys: &[[u8; 32]],
    current: &str,
) -> Result<Option<UpdateView>, CoreError> {
    Ok(update::check(fetch, current, keys)?.as_ref().map(view))
}

/// Fetch the newest installer. The notice is fetched and verified again (not trusted from
/// the earlier check), and the file is written only after its hash matched.
pub fn download(dir: &Path) -> Result<Downloaded, CoreError> {
    let fetch = GitHubFetch::new()?;
    download_with(&fetch, &keys()?, CURRENT_VERSION, dir)
}

pub fn download_with(
    fetch: &dyn Fetch,
    keys: &[[u8; 32]],
    current: &str,
    dir: &Path,
) -> Result<Downloaded, CoreError> {
    let info = update::check(fetch, current, keys)?
        .ok_or_else(|| CoreError::Refused("זו כבר הגרסה החדשה ביותר.".to_owned()))?;
    write_installer(fetch, &info, dir)
}

/// In the background, before she is asked (D-038): fetch the newest installer, verified, so
/// the click only restarts. The notice is verified again here. A file fetched earlier for the
/// same version is kept when it is still the one the notice names; anything else is fetched
/// again. `None` (and nothing on disk) when there is no newer version.
pub fn prepare(dir: &Path, earlier: Option<Downloaded>) -> Result<Option<Downloaded>, CoreError> {
    let fetch = GitHubFetch::new()?;
    prepare_with(&fetch, &keys()?, CURRENT_VERSION, dir, earlier)
}

pub fn prepare_with(
    fetch: &dyn Fetch,
    keys: &[[u8; 32]],
    current: &str,
    dir: &Path,
    earlier: Option<Downloaded>,
) -> Result<Option<Downloaded>, CoreError> {
    let Some(info) = update::check(fetch, current, keys)? else {
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
    let path = folder.join(installer_name(&info.version));
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

/// The app bundle the running program is in (`…/כספת.app/Contents/MacOS/<program>`).
#[must_use]
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    let ok = exe.parent()?.file_name()? == "MacOS"
        && exe.parent()?.parent()?.file_name()? == "Contents"
        && bundle.extension()? == "app";
    ok.then(|| bundle.to_path_buf())
}

/// On a Mac (D-101): the new version from the verified disk image takes the place of the app
/// bundle `bundle`, working in a private folder made under `work` (the app's own data
/// folder, outside the vault folder; never the system's temporary folder). Every program is
/// called by its full path, with arguments, no shell. The vault folder is never touched.
pub fn install_macos(file: &Downloaded, bundle: &Path, work: &Path) -> Result<(), CoreError> {
    install_macos_with(file, bundle, &SystemTools, work)
}

/// The programs the Mac install runs; a fake in tests.
pub(crate) trait MacTools {
    /// Run `program` with `args`; whether it succeeded.
    fn run(&self, program: &str, args: &[&std::ffi::OsStr]) -> bool;
}

struct SystemTools;

impl MacTools for SystemTools {
    fn run(&self, program: &str, args: &[&std::ffi::OsStr]) -> bool {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// The install, in order:
/// 1. the image is read once, its SHA-256 checked against the signed one, and those same
///    bytes written into a new folder only she can open (`0700`, under `work`):
///    what is mounted is exactly what was checked, not a file another program could swap;
/// 2. that copy is mounted read-only and hidden, in the same private folder;
/// 3. its one `.app` is copied beside the old bundle (`ditto`, which keeps the signature),
///    and the copy's signature is checked whole (`codesign --verify --deep --strict`);
/// 4. only then are the two swapped, so any failure before leaves the old version in place.
pub(crate) fn install_macos_with(
    file: &Downloaded,
    bundle: &Path,
    tools: &dyn MacTools,
    work: &Path,
) -> Result<(), CoreError> {
    let fail = |what: &str| CoreError::Internal(format!("mac update: {what}"));
    let name = bundle.file_name().ok_or_else(|| fail("bundle name"))?;
    let parent = bundle.parent().ok_or_else(|| fail("bundle folder"))?;
    let bytes = fs::read(&file.path).map_err(|_| fail("the disk image could not be read"))?;
    if update::sha256(&bytes) != file.sha256 {
        return Err(fail("the disk image changed after it was checked"));
    }
    let private = private_folder(work).map_err(|_| fail("a private folder"))?;
    let image = private.join("update.dmg");
    let mount = private.join("mount");
    let staged = parent.join(format!("{}.new", name.to_string_lossy()));
    let copied = (|| {
        fs::write(&image, &bytes).map_err(|_| fail("the private copy"))?;
        drop(bytes);
        fs::create_dir(&mount).map_err(|_| fail("mount point"))?;
        let attached = tools.run(
            "/usr/bin/hdiutil",
            &[
                "attach".as_ref(),
                "-nobrowse".as_ref(),
                "-readonly".as_ref(),
                "-noautoopen".as_ref(),
                "-mountpoint".as_ref(),
                mount.as_os_str(),
                image.as_os_str(),
            ],
        );
        if !attached {
            return Err(fail("/usr/bin/hdiutil"));
        }
        let found = (|| {
            let apps: Vec<PathBuf> = fs::read_dir(&mount)
                .map_err(|_| fail("disk image"))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "app") && !p.is_symlink())
                .collect();
            let [app] = apps.as_slice() else {
                return Err(fail("the disk image does not hold one app"));
            };
            let _ = fs::remove_dir_all(&staged);
            if tools.run("/usr/bin/ditto", &[app.as_os_str(), staged.as_os_str()]) {
                Ok(())
            } else {
                Err(fail("/usr/bin/ditto"))
            }
        })();
        let _ = tools.run(
            "/usr/bin/hdiutil",
            &["detach".as_ref(), mount.as_os_str(), "-force".as_ref()],
        );
        found?;
        let whole = tools.run(
            "/usr/bin/codesign",
            &[
                "--verify".as_ref(),
                "--deep".as_ref(),
                "--strict".as_ref(),
                staged.as_os_str(),
            ],
        );
        if whole {
            Ok(())
        } else {
            Err(fail("the new version's signature does not hold"))
        }
    })();
    let _ = fs::remove_dir_all(&private);
    if let Err(e) = copied {
        let _ = fs::remove_dir_all(&staged);
        return Err(e);
    }
    let old = parent.join(format!("{}.old", name.to_string_lossy()));
    let _ = fs::remove_dir_all(&old);
    fs::rename(bundle, &old).map_err(|_| {
        let _ = fs::remove_dir_all(&staged);
        fail("the old version could not be moved")
    })?;
    if fs::rename(&staged, bundle).is_err() {
        let _ = fs::rename(&old, bundle);
        let _ = fs::remove_dir_all(&staged);
        return Err(fail("the new version could not be put in place"));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// A new folder under `work` that only this account can open. Never one that already exists:
/// a folder put there in advance makes the install fail, not use it.
fn private_folder(work: &Path) -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = work.join(format!("dv-update-{}-{nanos}", std::process::id()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir)?;
    Ok(dir)
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

    #[test]
    fn the_releases_page_is_the_releases_repository() {
        assert_eq!(
            RELEASES_PAGE,
            format!("https://github.com/{}/releases", update::RELEASES_REPO)
        );
    }
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
            "installer": {"url": url, "sha256": to_hex(&update::sha256(installer)), "size": installer.len()},
            "macos": {"url": url, "sha256": to_hex(&update::sha256(installer)), "size": installer.len()}
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
        let v = check_with(&t, &[key], "0.2.9").unwrap().unwrap();
        assert_eq!((v.version.as_str(), v.size_mb), ("0.3.0", 1));
        assert!(check_with(&t, &[key], "0.3.0").unwrap().is_none());

        let file = download_with(&t, &[key], "0.2.9", dir.path()).unwrap();
        assert!(file
            .path
            .ends_with(format!("updates/{}", installer_name("0.3.0"))));
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
        let first = prepare_with(&t, &[key], "0.2.9", dir.path(), None)
            .unwrap()
            .unwrap();
        assert_eq!(first.version, "0.3.0");
        let path = first.path.clone();

        // The same version again: the file on disk is reused, not fetched twice.
        let mut quiet = Table(t.0.clone());
        quiet
            .0
            .retain(|url, _| url.ends_with(".json") || url.ends_with(".sig"));
        let again = prepare_with(&quiet, &[key], "0.2.9", dir.path(), Some(first))
            .unwrap()
            .unwrap();
        assert_eq!(again.path, path);

        // Changed on disk since: fetched again, and verified again.
        fs::write(&path, b"MZ swapped").unwrap();
        assert!(prepare_with(&quiet, &[key], "0.2.9", dir.path(), Some(again)).is_err());
        let fresh = prepare_with(&t, &[key], "0.2.9", dir.path(), None)
            .unwrap()
            .unwrap();
        assert!(still_intact(&fresh));

        // Already the newest: nothing prepared, nothing left on disk.
        assert!(prepare_with(&t, &[key], "0.3.0", dir.path(), Some(fresh))
            .unwrap()
            .is_none());
        assert!(!dir.path().join(FOLDER).exists());
    }

    #[test]
    fn the_app_bundle_is_found_only_where_a_mac_puts_it() {
        assert_eq!(
            bundle_of(Path::new("/Applications/V.app/Contents/MacOS/dv-desktop")),
            Some(PathBuf::from("/Applications/V.app"))
        );
        for not in [
            "/usr/local/bin/dv-desktop",
            "/Applications/V/Contents/MacOS/dv-desktop",
            "/Applications/V.app/Contents/Resources/dv-desktop",
            "dv-desktop",
        ] {
            assert_eq!(bundle_of(Path::new(not)), None, "{not}");
        }
    }

    /// The Mac's programs, played on folders: `attach` puts `apps` in the mount point,
    /// `ditto` copies, `codesign` answers `signed`. Records what ran.
    struct FakeMac {
        apps: Vec<&'static str>,
        signed: bool,
        ran: std::cell::RefCell<Vec<String>>,
    }

    impl MacTools for FakeMac {
        fn run(&self, program: &str, args: &[&std::ffi::OsStr]) -> bool {
            self.ran.borrow_mut().push(program.to_owned());
            let arg = |i: usize| PathBuf::from(args[i]);
            match (program, args.first().and_then(|a| a.to_str())) {
                ("/usr/bin/hdiutil", Some("attach")) => {
                    let image = arg(6);
                    assert_eq!(fs::read(&image).unwrap(), b"disk image");
                    let mount = arg(5);
                    assert!(mount.starts_with(image.parent().unwrap()));
                    for app in &self.apps {
                        fs::create_dir_all(mount.join(app).join("Contents")).unwrap();
                        fs::write(mount.join(app).join("Contents/new"), b"new").unwrap();
                    }
                    true
                }
                ("/usr/bin/hdiutil", Some("detach")) => true,
                ("/usr/bin/ditto", _) => {
                    let (from, to) = (arg(0), arg(1));
                    fs::create_dir_all(to.join("Contents")).unwrap();
                    fs::copy(from.join("Contents/new"), to.join("Contents/new")).is_ok()
                }
                ("/usr/bin/codesign", _) => self.signed,
                _ => false,
            }
        }
    }

    fn mac_case(
        apps: Vec<&'static str>,
        signed: bool,
    ) -> (tempfile::TempDir, Downloaded, PathBuf, FakeMac) {
        let dir = tempfile::tempdir().unwrap();
        let (t, key) = server(b"disk image", b"disk image");
        let file = download_with(&t, &[key], "0.2.9", dir.path()).unwrap();
        let bundle = dir.path().join("Applications").join("V.app");
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(bundle.join("Contents/old"), b"old").unwrap();
        fs::create_dir_all(dir.path().join("tmp")).unwrap();
        let tools = FakeMac {
            apps,
            signed,
            ran: std::cell::RefCell::new(Vec::new()),
        };
        (dir, file, bundle, tools)
    }

    #[test]
    fn on_a_mac_the_new_version_takes_the_old_ones_place_and_nothing_is_left() {
        let (dir, file, bundle, tools) = mac_case(vec!["V.app"], true);
        install_macos_with(&file, &bundle, &tools, &dir.path().join("tmp")).unwrap();
        assert!(bundle.join("Contents/new").is_file() && !bundle.join("Contents/old").exists());
        let apps = dir.path().join("Applications");
        assert_eq!(
            fs::read_dir(&apps).unwrap().count(),
            1,
            "no .new or .old left"
        );
        assert_eq!(
            fs::read_dir(dir.path().join("tmp")).unwrap().count(),
            0,
            "private copy gone"
        );
        assert!(tools.ran.borrow().iter().any(|p| p == "/usr/bin/codesign"));
    }

    #[test]
    fn on_a_mac_a_broken_signature_or_a_strange_image_leaves_the_old_version() {
        for (apps, signed) in [
            (vec!["V.app"], false),
            (vec!["V.app", "W.app"], true),
            (vec![], true),
        ] {
            let (dir, file, bundle, tools) = mac_case(apps.clone(), signed);
            assert!(install_macos_with(&file, &bundle, &tools, &dir.path().join("tmp")).is_err());
            assert!(bundle.join("Contents/old").is_file(), "{apps:?} {signed}");
            let apps_dir = dir.path().join("Applications");
            assert_eq!(fs::read_dir(&apps_dir).unwrap().count(), 1, "no .new left");
            assert_eq!(fs::read_dir(dir.path().join("tmp")).unwrap().count(), 0);
        }
    }

    #[test]
    fn a_disk_image_changed_on_disk_is_never_installed() {
        let dir = tempfile::tempdir().unwrap();
        let (t, key) = server(b"disk image", b"disk image");
        let file = download_with(&t, &[key], "0.2.9", dir.path()).unwrap();
        fs::write(&file.path, b"swapped").unwrap();
        let bundle = dir.path().join("V.app");
        fs::create_dir_all(&bundle).unwrap();
        let tools = FakeMac {
            apps: vec!["V.app"],
            signed: true,
            ran: std::cell::RefCell::new(Vec::new()),
        };
        assert!(install_macos_with(&file, &bundle, &tools, dir.path()).is_err());
        assert!(bundle.is_dir(), "the version in place stays");
        assert!(tools.ran.borrow().is_empty(), "nothing was mounted");
    }

    #[test]
    fn a_changed_installer_is_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let (t, key) = server(b"MZ installer", b"MZ evil");
        let err = download_with(&t, &[key], "0.2.9", dir.path()).unwrap_err();
        assert_eq!(err.to_ui().code, "update");
        assert!(!dir.path().join(FOLDER).exists());
        let err = prepare_with(&t, &[key], "0.2.9", dir.path(), None).unwrap_err();
        assert_eq!(err.to_ui().code, "update");
        assert!(!dir.path().join(FOLDER).exists());
    }
}
