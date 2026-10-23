//! The only place in the program that calls Windows directly (D-047). Everything here is a
//! plain Win32 call with fixed arguments; `cargo xtask check-invariants` refuses `unsafe`
//! anywhere else, and refuses an `unsafe` block here without a `// SAFETY:` line above it.
//!
//! * Memory that stays in RAM (S-5): SQLCipher locks every page it allocates
//!   (`cipher_memory_security`, `VirtualLock`), but Windows lets a process lock only a little
//!   over its minimum working set, about 200 KiB by default. Past that the pages were not
//!   locked and could be written to the page file (and SQLCipher's log of that failure is what
//!   crashed v0.2.0, D-038). The minimum is raised at start, so the vault's pages fit.
//! * No crash dumps with case data (S-5): a crash of this program is kept out of Windows Error
//!   Reporting, its reports carry no heap, and no "stopped working" window waits on screen.
//! * Is the computer locked (Win+L)? Asked of Windows every 15 seconds (THREAT_MODEL T2). It
//!   used to start `tasklist.exe` each time, and a program started every 15 seconds is a likely
//!   reason the text box she was typing in kept losing the keyboard, so it is a plain question now.
#![allow(unsafe_code)]

/// What [`harden_process`] managed to do, for the crash-free startup log in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hardening {
    /// The minimum working set now granted, in MiB (`None`: the call failed everywhere). On a
    /// Mac, how much may be locked, when that is enough and no core file can be written.
    pub lock_quota_mib: Option<usize>,
    /// The program is excluded from Windows Error Reporting for this user. On a Mac, where
    /// there is no such opt-out, it means no core file can be written.
    pub reports_excluded: bool,
    /// Error reports, if any are still made, carry no heap.
    pub reports_without_heap: bool,
}

/// Minimum working sets to try, largest first (MiB). SQLCipher keeps a page cache of about
/// 2 MiB per database and three are open; the rest is room for large documents. A machine
/// that refuses the larger sizes still gets the largest one it accepts.
#[cfg(windows)]
const QUOTA_STEPS_MIB: &[usize] = &[256, 128, 64, 32];

/// Run once, first thing in `main`, in the program and in each document worker.
#[must_use]
pub fn harden_process() -> Hardening {
    #[cfg(windows)]
    {
        windows_impl::harden()
    }
    #[cfg(target_os = "macos")]
    {
        crate::mac_platform::harden()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Hardening::default()
    }
}

/// Is this Windows session locked (Win+L, the lock screen, switching user)? `None` when
/// Windows cannot tell.
#[cfg(windows)]
#[must_use]
pub fn session_locked() -> Option<bool> {
    windows_impl::session_locked()
}

#[cfg(windows)]
mod windows_impl {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Diagnostics::Debug::{
        SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX,
    };
    use windows::Win32::System::ErrorReporting::{
        WerAddExcludedApplication, WerSetFlags, WER_FAULT_REPORTING_FLAG_NOHEAP,
    };
    use windows::Win32::System::Memory::{
        SetProcessWorkingSetSizeEx, QUOTA_LIMITS_HARDWS_MAX_DISABLE,
        QUOTA_LIMITS_HARDWS_MIN_DISABLE,
    };
    use windows::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
        WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
        WTS_SESSIONSTATE_UNLOCK,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    use super::{Hardening, QUOTA_STEPS_MIB};

    pub fn harden() -> Hardening {
        // SAFETY: SetErrorMode takes a flag set by value and only changes this process's
        // error-mode word; it touches no memory of ours.
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        }
        // SAFETY: WerSetFlags takes a flag set by value and applies to this process only.
        let reports_without_heap = unsafe { WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP) }.is_ok();
        let reports_excluded = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.file_name().map(HSTRING::from))
            .is_some_and(|name| {
                // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call;
                // Windows copies it into the user's WER settings and keeps no pointer.
                unsafe { WerAddExcludedApplication(PCWSTR(name.as_ptr()), false) }.is_ok()
            });
        Hardening {
            lock_quota_mib: raise_lock_quota(),
            reports_excluded,
            reports_without_heap,
        }
    }

    /// The session's lock state from `WTSQuerySessionInformationW` (level 1 of
    /// `WTSSessionInfoEx`). Anything other than "locked" or "unlocked" is `None`.
    pub fn session_locked() -> Option<bool> {
        let mut buffer = windows::core::PWSTR::null();
        let mut bytes = 0u32;
        // SAFETY: both out-pointers point at locals that outlive the call; on success Windows
        // allocates the buffer, which is freed below and not used after.
        let asked = unsafe {
            WTSQuerySessionInformationW(
                Some(WTS_CURRENT_SERVER_HANDLE),
                WTS_CURRENT_SESSION,
                WTSSessionInfoEx,
                &raw mut buffer,
                &raw mut bytes,
            )
        };
        if asked.is_err() || buffer.is_null() {
            return None;
        }
        let fits = usize::try_from(bytes).is_ok_and(|b| b >= std::mem::size_of::<WTSINFOEXW>());
        let flags = if fits {
            // SAFETY: Windows filled the buffer with a WTSINFOEXW of at least its size (checked
            // above); it is copied out once, before the buffer is freed.
            let info = unsafe { buffer.0.cast::<WTSINFOEXW>().read_unaligned() };
            if info.Level == 1 {
                // SAFETY: the level is 1, so this union member is the one Windows filled.
                Some(unsafe { info.Data.WTSInfoExLevel1.SessionFlags })
            } else {
                None
            }
        } else {
            None
        };
        // SAFETY: the buffer was allocated by WTSQuerySessionInformationW and is freed once.
        unsafe { WTSFreeMemory(buffer.0.cast()) };
        match u32::try_from(flags?).ok()? {
            WTS_SESSIONSTATE_LOCK => Some(true),
            WTS_SESSIONSTATE_UNLOCK => Some(false),
            _ => None,
        }
    }

    /// Soft limits (`*_DISABLE`): Windows may still trim the working set under memory
    /// pressure, but the minimum also sets how much may be locked, which is what we need.
    fn raise_lock_quota() -> Option<usize> {
        for &mib in QUOTA_STEPS_MIB {
            let min = mib * 1024 * 1024;
            let max = min * 4;
            // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing, and the
            // sizes and flags are passed by value.
            let ok = unsafe {
                SetProcessWorkingSetSizeEx(
                    GetCurrentProcess(),
                    min,
                    max,
                    QUOTA_LIMITS_HARDWS_MIN_DISABLE | QUOTA_LIMITS_HARDWS_MAX_DISABLE,
                )
            }
            .is_ok();
            if ok {
                return Some(mib);
            }
        }
        None
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used)]
    mod tests {
        use windows::Win32::System::Memory::{VirtualLock, VirtualUnlock};

        /// The test runs in an unlocked session, so Windows answers "not locked".
        #[test]
        fn this_session_is_not_locked() {
            assert_eq!(super::session_locked(), Some(false));
        }

        /// The vault's locked pages fit: with the default quota (about 200 KiB) locking
        /// 16 MiB fails; after hardening it succeeds.
        #[test]
        fn sixteen_mib_can_be_locked_after_hardening() {
            let report = super::harden();
            assert!(report.lock_quota_mib.is_some(), "{report:?}");
            assert!(report.reports_without_heap, "{report:?}");
            let buf = vec![7u8; 16 * 1024 * 1024];
            // SAFETY: `buf` is a live allocation of exactly this length for the whole block.
            unsafe {
                VirtualLock(buf.as_ptr().cast(), buf.len()).unwrap();
                VirtualUnlock(buf.as_ptr().cast(), buf.len()).unwrap();
            }
        }
    }
}
