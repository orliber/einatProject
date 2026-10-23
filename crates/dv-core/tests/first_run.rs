//! The first run on a real machine, as the program does it: the strongest Argon2 cost this
//! computer picks (up to 1 GiB), the vault created, the program closed, and the vault opened
//! again with the password and with the recovery kit. Slow, so it runs on purpose only
//! (`cargo test --release -p dv-core --test first_run -- --ignored`), on each desktop OS in CI.
//! It exists because the vault was never opened on real Windows before a release.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::SystemTime;

use dv_core::Core;

const PASSWORD: &str = "סוס ירוק רץ בשדה 2026";

#[test]
#[ignore = "slow: the real Argon2 cost; run with --ignored"]
fn the_vault_opens_again_after_the_program_closes() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    std::fs::create_dir_all(&vault).unwrap();

    let recovery = {
        let mut core = Core::new(&vault);
        let created = core.create_vault(PASSWORD).unwrap();
        assert!(core.confirm_recovery_key(&created.recovery_key).unwrap());
        assert!(core.status().unlocked);
        // On Windows and macOS the password slot is bound to this computer from the start
        // (D-076, D-101); a DPAPI or Keychain that failed quietly would leave it unbound
        // (review of stage 5, S3).
        assert_eq!(
            dv_vault::Vault::password_is_bound(&vault).unwrap(),
            dv_vault::computer_key::available(),
            "bound to this computer exactly where a key can be kept for the account"
        );
        core.list_cases().unwrap();
        core.lock();
        created.recovery_key
    };

    // A new start of the program: the same folder, the same password.
    let mut core = Core::new(&vault);
    assert!(core.status().vault_exists);
    let status = core.unlock(PASSWORD).unwrap();
    assert!(status.unlocked);
    core.list_cases().unwrap();
    core.folders().unwrap();
    core.backup_status().unwrap();
    assert!(!core.tick(SystemTime::now()));
    core.lock();

    let mut core = Core::new(&vault);
    assert!(core.unlock_with_recovery(&recovery).unwrap().unlocked);
    core.lock();
}

/// R6: opening, as she waits for it: the Argon2 cost this computer picked, the vault, the
/// list of cases. The cost is picked so the key takes about a second on the computer that
/// created the vault (`Argon2Params::calibrate`); the whole opening must stay under 3 seconds.
#[test]
#[ignore = "slow: the real Argon2 cost; run with --ignored"]
fn opening_the_vault_takes_under_three_seconds() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    {
        let mut core = Core::new(&vault);
        core.create_vault(PASSWORD).unwrap();
        core.lock();
    }
    let mut core = Core::new(&vault);
    let started = Instant::now();
    assert!(core.unlock(PASSWORD).unwrap().unlocked);
    core.list_cases().unwrap();
    let took = started.elapsed();
    let _ = std::io::Write::write_fmt(
        &mut std::io::stderr(),
        format_args!("R6: opening took {took:?}\n"),
    );
    assert!(took < Duration::from_secs(3), "opening took {took:?}");
}
