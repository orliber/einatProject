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
//! * Windows Hello (S-2): its prompt, started from a desktop program, can open behind the
//!   window. It is brought to the front.
#![allow(unsafe_code)]

/// What [`harden_process`] managed to do, for the crash-free startup log in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hardening {
    /// The minimum working set now granted, in MiB (`None`: the call failed everywhere).
    pub lock_quota_mib: Option<usize>,
    /// The program is excluded from Windows Error Reporting for this user.
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
    #[cfg(not(windows))]
    {
        Hardening::default()
    }
}

/// While Windows Hello is asking (for up to `for_ms`), bring its prompt in front of the
/// program's window as soon as it appears.
pub fn bring_hello_prompt_forward(for_ms: u64) {
    #[cfg(windows)]
    {
        std::thread::spawn(move || windows_impl::hello_forward(for_ms));
    }
    #[cfg(not(windows))]
    {
        let _ = for_ms;
    }
}

#[cfg(windows)]
mod windows_impl {
    use windows::core::{w, HSTRING, PCWSTR};
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
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};

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

    pub fn hello_forward(for_ms: u64) {
        let step = 100;
        for _ in 0..for_ms / step {
            // SAFETY: both strings are static NUL-terminated literals; a missing window is
            // an error value, not a fault.
            let found = unsafe { FindWindowW(w!("Credential Dialog Xaml Host"), PCWSTR::null()) };
            if let Ok(hwnd) = found {
                // SAFETY: `hwnd` was just returned by FindWindowW; a stale handle only makes
                // the call return false.
                let _ = unsafe { SetForegroundWindow(hwnd) };
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(step));
        }
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used)]
    mod tests {
        use windows::Win32::System::Memory::{VirtualLock, VirtualUnlock};

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
