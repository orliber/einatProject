//! Starts an untrusted worker process, contained (D-044).
//!
//! Documents arrive from outside: a parser bug in a hostile file can run code. That code runs in
//! a separate worker process, and on Windows (where the app is used) that process is contained:
//!
//! * an **AppContainer** with no capabilities: no network at all, and no access to the user's
//!   files, the vault, or anything else not granted to it. It is granted read and execute on the
//!   worker program and on the folders listed in [`Policy::read_dirs`] only.
//! * a **job object**: one process (it cannot start another), a memory ceiling, killed with
//!   the app, no clipboard or other desktop access, no error dialog on a crash.
//! * an environment of `SystemRoot` and `WINDIR` only, the system folder as working folder, no
//!   window, and nothing inherited but its own stdin and stdout.
//!
//! Elsewhere (development on Linux, the Mac build) the process is started the same way minus
//! the containment; [`CONTAINED`] says which.
//!
//! The Windows part is the one audited exception to `forbid(unsafe_code)` in the workspace: all
//! of it is in `windows.rs`, every `unsafe` block says why it is sound, and nothing else here
//! uses `unsafe`.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

/// Whether this platform contains the worker (AppContainer and job object).
pub const CONTAINED: bool = cfg!(windows);

/// What the contained process may have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// The memory ceiling, in megabytes. Over it, allocations fail and the process ends.
    pub memory_mb: u32,
    /// Folders the process may read (for example a language-data folder). Nothing else.
    pub read_dirs: Vec<PathBuf>,
    /// Tell an OpenMP program (the OCR engine) to use one thread: its busy-waiting threads
    /// otherwise starve each other and the app (`OMP_THREAD_LIMIT=1`, the only variable added).
    pub single_thread: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            memory_mb: 1024,
            read_dirs: Vec::new(),
            single_thread: false,
        }
    }
}

/// A running contained process. Dropping it kills the process.
pub struct Child {
    #[cfg(windows)]
    inner: windows::Contained,
    #[cfg(not(windows))]
    inner: std::process::Child,
}

impl std::fmt::Debug for Child {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Child")
            .field("contained", &CONTAINED)
            .finish_non_exhaustive()
    }
}

/// Start `exe` with `args`, contained by `policy`.
pub fn spawn(exe: &Path, args: &[&str], policy: &Policy) -> io::Result<Child> {
    #[cfg(windows)]
    {
        Ok(Child {
            inner: windows::spawn(exe, args, policy)?,
        })
    }
    #[cfg(not(windows))]
    {
        let inner = std::process::Command::new(exe)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .env_clear()
            .envs(policy.single_thread.then_some(("OMP_THREAD_LIMIT", "1")))
            .spawn()?;
        Ok(Child { inner })
    }
}

impl Child {
    /// The process's stdin, once.
    pub fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>> {
        #[cfg(windows)]
        let pipe = self.inner.stdin.take();
        #[cfg(not(windows))]
        let pipe = self.inner.stdin.take();
        pipe.map(|p| Box::new(p) as Box<dyn Write + Send>)
    }

    /// The process's stdout, once.
    pub fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
        #[cfg(windows)]
        let pipe = self.inner.stdout.take();
        #[cfg(not(windows))]
        let pipe = self.inner.stdout.take();
        pipe.map(|p| Box::new(p) as Box<dyn Read + Send>)
    }

    /// Wait up to `timeout`. `None` if it is still running.
    pub fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        #[cfg(windows)]
        {
            self.inner.wait_timeout(timeout)
        }
        #[cfg(not(windows))]
        {
            let deadline = std::time::Instant::now() + timeout;
            loop {
                if let Some(status) = self.inner.try_wait()? {
                    return Ok(Some(status));
                }
                if std::time::Instant::now() >= deadline {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }

    /// End it now.
    pub fn kill(&mut self) {
        #[cfg(windows)]
        self.inner.kill();
        #[cfg(not(windows))]
        {
            let _ = self.inner.kill();
            let _ = self.inner.wait();
        }
    }
}

#[cfg(not(windows))]
impl Drop for Child {
    fn drop(&mut self) {
        if matches!(self.inner.try_wait(), Ok(None)) {
            self.kill();
        }
    }
}
