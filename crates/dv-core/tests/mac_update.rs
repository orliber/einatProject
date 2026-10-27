//! L3: a Mac update, version to version, as on her Mac (CI job `macos-update`).
//!
//! The same file runs in two trees: copied into the source of the version before (the one she
//! has), it creates a fictional vault with a password, bound to this computer's Keychain, and
//! then installs the new disk image with that version's own update code, exactly as the
//! installed app would on "להפעיל מחדש ולעדכן"; in the tree under test it opens the same vault
//! with the same password. So it uses only what every version since the Mac one (0.8.0) has.
//!
//! Each step is an ignored test, run on purpose by name with its inputs in the environment:
//!
//! * `notice` (tree under test only): `DV_UPDATE_NOTICE`, `DV_UPDATE_NOTICE_SIG` (the published
//!   `latest.json` and its signature, as fetched), `DV_UPDATE_OUT`: the notice is checked with
//!   the program's own code and the keys pinned in it, and the Mac entry written out
//! * `create`: `DV_UPDATE_VAULT` (an empty folder)
//! * `install`: `DV_UPDATE_DMG`, `DV_UPDATE_VERSION`, `DV_UPDATE_SHA256`, `DV_UPDATE_CDHASH`,
//!   `DV_UPDATE_BUNDLE` (the installed app), `DV_UPDATE_WORK` (the app's own data folder)
//! * `open`: `DV_UPDATE_VAULT`, and `DV_UPDATE_VERSION` (the version under test)
//!
//! Fabricated data only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use dv_core::Core;

const PASSWORD: &str = "סוס ירוק רץ בשדה 2026";
const PRACTITIONER: &str = "ד\"ר נועה בדויה";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

fn hex<const N: usize>(name: &str) -> [u8; N] {
    let text = env(name);
    let digits: Vec<u8> = text
        .trim()
        .chars()
        .map(|c| {
            c.to_digit(16)
                .and_then(|d| u8::try_from(d).ok())
                .unwrap_or_else(|| panic!("{name}: not hex: {text}"))
        })
        .collect();
    assert_eq!(digits.len(), N * 2, "{name}: {text}");
    let mut out = [0u8; N];
    for (b, pair) in out.iter_mut().zip(digits.chunks(2)) {
        *b = (pair[0] << 4) | pair[1];
    }
    out
}

/// The published notice of the version before, checked as an installed copy checks it: signed
/// with a pinned key, for this product, with a Mac entry. Its version, disk image address,
/// SHA-256 and app signature go to `DV_UPDATE_OUT`, one `name=value` a line, for the job.
#[test]
#[ignore = "a step of the Mac update test in CI; run by name with its environment"]
fn notice() {
    use dv_egress::update::{check, pinned_keys, to_hex, Fetch, UpdateError, PLATFORM_ENTRY};
    struct Fetched {
        notice: Vec<u8>,
        sig: Vec<u8>,
    }
    impl Fetch for Fetched {
        fn get(&self, url: &str, _max: u64) -> Result<Vec<u8>, UpdateError> {
            Ok(if url.ends_with(".sig") {
                self.sig.clone()
            } else {
                self.notice.clone()
            })
        }
    }
    assert_eq!(PLATFORM_ENTRY, "macos", "the Mac entry is read on a Mac");
    let fetched = Fetched {
        notice: std::fs::read(env("DV_UPDATE_NOTICE")).unwrap(),
        sig: std::fs::read(env("DV_UPDATE_NOTICE_SIG")).unwrap(),
    };
    let keys = pinned_keys();
    assert!(!keys.is_empty(), "no update key is pinned in this build");
    let info = check(&fetched, "0.0.0", &keys)
        .unwrap_or_else(|e| panic!("the published notice does not verify: {e:?}"))
        .expect("the published notice has no Mac entry");
    let cdhash = info.cdhash.expect("the Mac entry names no app signature");
    std::fs::write(
        env("DV_UPDATE_OUT"),
        format!(
            "version={}\nurl={}\nsha256={}\nsize={}\ncdhash={}\n",
            info.version,
            info.installer_url,
            to_hex(&info.sha256),
            info.size,
            to_hex(&cdhash)
        ),
    )
    .unwrap();
}

/// The version before: a new vault, as at her first run (the real Argon2 cost, the password
/// bound to this computer), with something of hers in it.
#[test]
#[ignore = "a step of the Mac update test in CI; run by name with its environment"]
fn create() {
    let vault = PathBuf::from(env("DV_UPDATE_VAULT"));
    std::fs::create_dir_all(&vault).unwrap();
    let mut core = Core::new(&vault);
    core.create_vault(PASSWORD).unwrap();
    core.set_practitioner(vec![PRACTITIONER.into()]).unwrap();
    assert!(
        dv_vault::Vault::password_is_bound(&vault).unwrap(),
        "the password is bound to this computer's Keychain"
    );
    core.lock();
    // Closed and opened again in the same version, as before any update.
    let mut core = Core::new(&vault);
    assert!(core.unlock(PASSWORD).unwrap().unlocked);
    core.lock();
}

/// The version before installs the new one: its own update code, with the disk image and the
/// signed notice's hash and app signature, over the installed app.
#[test]
#[ignore = "a step of the Mac update test in CI; run by name with its environment"]
fn install() {
    let file = dv_core::update::Downloaded {
        version: env("DV_UPDATE_VERSION"),
        path: PathBuf::from(env("DV_UPDATE_DMG")),
        sha256: hex::<32>("DV_UPDATE_SHA256"),
        cdhash: Some(hex::<20>("DV_UPDATE_CDHASH")),
    };
    assert!(
        dv_core::update::still_intact(&file),
        "the disk image changed"
    );
    let bundle = PathBuf::from(env("DV_UPDATE_BUNDLE"));
    let work = PathBuf::from(env("DV_UPDATE_WORK"));
    std::fs::create_dir_all(&work).unwrap();
    if let Err(e) = dv_core::update::install_macos(&file, &bundle, &work) {
        panic!("the update was not installed: {e:?}");
    }
}

/// The version under test opens the vault the version before made, with the same password and
/// the same key in the Keychain, with everything in it.
#[test]
#[ignore = "a step of the Mac update test in CI; run by name with its environment"]
fn open() {
    let vault = PathBuf::from(env("DV_UPDATE_VAULT"));
    let mut core = Core::new(&vault);
    let status = core.status();
    assert!(status.vault_exists && !status.unlocked, "{status:?}");
    let status = core
        .unlock(PASSWORD)
        .unwrap_or_else(|e| panic!("the same password does not open it: {:?}", e.to_ui()));
    assert!(status.unlocked);
    assert!(
        status.integrity_warning.is_none(),
        "{:?}",
        status.integrity_warning
    );
    assert_eq!(status.practitioner, vec![PRACTITIONER.to_owned()]);
    assert!(dv_vault::Vault::password_is_bound(&vault).unwrap());
    core.list_cases().unwrap();
    core.lock();
    // The vault now names the version under test as the newest that opened it (D-104).
    let header: serde_json::Value =
        serde_json::from_slice(&std::fs::read(vault.join("vault.header")).unwrap()).unwrap();
    assert_eq!(
        header["app_version"].as_str(),
        Some(env("DV_UPDATE_VERSION").as_str()),
        "{header}"
    );
    // And it opens again.
    let mut core = Core::new(&vault);
    assert!(core.unlock(PASSWORD).unwrap().unlocked);
    core.lock();
}
