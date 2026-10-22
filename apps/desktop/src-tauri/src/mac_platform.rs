//! The macOS side of [`crate::win_platform::harden_process`] (D-101): what keeps case data
//! out of the disk on a Mac. No `unsafe`: the limits are read and set through `rustix`.
//!
//! * Memory that stays in RAM (S-5): SQLCipher locks every page it allocates
//!   (`cipher_memory_security`, `mlock`). macOS lets a process lock as much as its
//!   `RLIMIT_MEMLOCK` allows; the layer is shown as on only when that is at least
//!   [`MIN_LOCK_MIB`] (unlimited by default).
//! * No crash dumps with case data (S-5): a core file holds the whole heap. Its limit is set
//!   to zero, soft and hard, at start, so nothing this process does later can raise it.
//!   macOS crash reports (ReportCrash) hold the threads' stacks and registers, not the heap.

use rustix::process::{getrlimit, setrlimit, Resource, Rlimit};

use crate::win_platform::Hardening;

/// The least that must be lockable for the vault's pages: SQLCipher keeps a page cache of
/// about 2 MiB per database and three are open (as `QUOTA_STEPS_MIB` on Windows).
const MIN_LOCK_MIB: usize = 32;

pub fn harden() -> Hardening {
    let no_core = setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )
    .is_ok()
        && getrlimit(Resource::Core).current == Some(0);
    let lockable_mib = match getrlimit(Resource::Memlock).current {
        None => Some(usize::MAX),
        Some(bytes) => usize::try_from(bytes / (1024 * 1024)).ok(),
    };
    Hardening {
        // Shown as on only when both hold: pages can be locked, and no core file is written.
        lock_quota_mib: lockable_mib.filter(|&m| m >= MIN_LOCK_MIB && no_core),
        reports_excluded: no_core,
        reports_without_heap: no_core,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn no_core_file_and_room_to_lock_the_vault() {
        let report = harden();
        assert!(report.reports_without_heap, "{report:?}");
        assert!(report.lock_quota_mib.is_some(), "{report:?}");
        assert_eq!(getrlimit(Resource::Core).maximum, Some(0));
        // Once lowered, the limit cannot be raised again by this process.
        assert!(setrlimit(
            Resource::Core,
            Rlimit {
                current: Some(1 << 20),
                maximum: Some(1 << 20),
            },
        )
        .is_err());
    }
}
