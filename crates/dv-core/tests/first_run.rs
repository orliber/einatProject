//! The first run on a real machine, as the program does it: the strongest Argon2 cost this
//! computer picks (up to 1 GiB), the vault created, the program closed, and the vault opened
//! again with the password and with the recovery kit. Slow, so it runs on purpose only
//! (`cargo test --release -p dv-core --test first_run -- --ignored`), on each desktop OS in CI.
//! It exists because the vault was never opened on real Windows before a release.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
