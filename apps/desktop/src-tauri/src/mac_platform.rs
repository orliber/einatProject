//! The macOS side of [`crate::win_platform::harden_process`] (D-101): what keeps case data
//! out of the disk on a Mac, and what bounds a document worker. No `unsafe`: the limits are
//! read and set through `rustix`.
//!
//! * Memory that stays in RAM (S-5): SQLCipher locks every page it allocates
//!   (`cipher_memory_security`, `mlock`). macOS caps what one process may lock by
//!   `vm.user_wire_limit` (`RLIMIT_MEMLOCK` is not what limits it there); the layer is shown
//!   as on only when both that and the process limit allow at least [`MIN_LOCK_MIB`].
//! * No crash dumps with case data (S-5): a core file holds the whole heap. Its limit is set
//!   to zero, soft and hard, at start, so nothing this process does later can raise it.
//!   macOS crash reports (ReportCrash) hold the threads' stacks and registers, not the heap.
//! * A document worker ([`contain_worker`]) ends with the app and within a CPU limit, and
//!   refuses to run with any descriptor of the app's besides its own stdin, stdout and stderr.

use std::time::Duration;

use rustix::process::{getppid, getrlimit, setrlimit, Resource, Rlimit};

use crate::win_platform::Hardening;

/// The least that must be lockable for the vault's pages: SQLCipher keeps a page cache of
/// about 2 MiB per database and three are open (as `QUOTA_STEPS_MIB` on Windows).
const MIN_LOCK_MIB: usize = 32;

/// Run first in every process (the app and each worker): no core file.
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
        // Not yet: the system's own cap is read in the app only ([`with_wire_limit`]).
        lock_quota_mib: lockable_mib.filter(|&m| m >= MIN_LOCK_MIB && no_core),
        // On a Mac: no core file can be written (there is no per-program opt-out of
        // ReportCrash, whose reports carry no heap).
        reports_excluded: no_core,
        reports_without_heap: no_core,
    }
}

/// In the app (not a worker, which may start nothing): the lockable amount is also capped by
/// the system's `vm.user_wire_limit`, read with `/usr/sbin/sysctl`. Unreadable is not enough.
pub fn with_wire_limit(mut hardening: Hardening) -> Hardening {
    let wire_mib = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "vm.user_wire_limit"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env_clear()
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .and_then(|bytes| usize::try_from(bytes / (1024 * 1024)).ok());
    hardening.lock_quota_mib = match (hardening.lock_quota_mib, wire_mib) {
        (Some(process), Some(system)) => Some(process.min(system)).filter(|&m| m >= MIN_LOCK_MIB),
        _ => None,
    };
    hardening
}

/// How often the worker checks that the app is still there.
const PARENT_CHECK: Duration = Duration::from_millis(500);

/// First thing in a document worker, inside the sandbox. `None` when it may run: then it ends
/// itself once the app is gone (the app crashed or was forced to quit: the Windows job
/// object's kill-on-close) and after `cpu_seconds` of CPU, a limit it cannot raise. `Some`
/// (the exit code) when it must not run: a descriptor of the app's beyond 0, 1 and 2 reached
/// it, which the sandbox would not stop it reading.
///
/// Every number below its descriptor limit is looked at. Its start (dv-sandbox) closes all
/// that are open and sets that limit to [`dv_sandbox::FD_LIMIT`]; a higher limit (a start
/// some other way) is refused, since a descriptor could then be beyond the look.
#[must_use]
pub fn contain_worker(cpu_seconds: Option<u64>) -> Option<i32> {
    let open = |fd: u64| std::fs::metadata(format!("/dev/fd/{fd}")).is_ok();
    let Some(limit) = getrlimit(Resource::Nofile)
        .current
        .filter(|&l| l <= dv_sandbox::FD_LIMIT)
    else {
        return Some(7);
    };
    // Its own stdin must show as open, or the look would prove nothing (fail-closed).
    if !open(0) || (3..limit).any(open) {
        return Some(7);
    }
    if let Some(seconds) = cpu_seconds {
        let limit = Rlimit {
            current: Some(seconds),
            maximum: Some(seconds),
        };
        if setrlimit(Resource::Cpu, limit).is_err() {
            return Some(8);
        }
    }
    // Already gone (adopted by launchd): nothing would ever change to end it.
    let Some(app) = getppid().filter(|p| !p.is_init()) else {
        return Some(9);
    };
    std::thread::spawn(move || loop {
        std::thread::sleep(PARENT_CHECK);
        if getppid() != Some(app) {
            std::process::exit(9);
        }
    });
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn no_core_file_and_room_to_lock_the_vault() {
        let report = with_wire_limit(harden());
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
