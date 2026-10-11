//! Regression: SQLCipher registers `sqlcipher_export` (used by backups) only after SQLite has
//! marked itself initialized, so a database opened by another thread in that window lacked
//! the function and its backups failed with "no such function: sqlcipher_export". This test
//! is alone in its own process so the first database opens really are concurrent.
//! Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Barrier;

use dv_vault::{Argon2Params, Vault};

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

#[test]
fn vaults_opened_at_once_on_many_threads_can_all_write_backups() {
    const THREADS: usize = 12;
    let barrier = Barrier::new(THREADS);
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                let dir = tempfile::tempdir().unwrap();
                barrier.wait();
                let mut vault = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST)
                    .unwrap()
                    .vault;
                vault
                    .write_backup(&dir.path().join("b.vaultbak"))
                    .unwrap_or_else(|e| panic!("backup failed: {e}"));
            });
        }
    });
}
