//! Is the computer itself locked (Win+L, the lock screen, switching user)? Asked every 15
//! seconds by the lock timer, so the vault locks with the computer (THREAT_MODEL T2).
//!
//! No native calls (`forbid(unsafe_code)`): Windows shows its lock screen in `LogonUI.exe`,
//! which runs only while the computer is locked; on a Mac, the session reports
//! `CGSSessionScreenIsLocked`. The timer acts only when the answer changes from "not locked"
//! to "locked" while the vault is open, so a machine where the check always answers "locked"
//! can never keep the vault from opening.

use std::process::{Command, Stdio};

/// `None` when this computer cannot tell.
pub fn computer_locked() -> Option<bool> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot")?;
        let tasklist = std::path::Path::new(&root).join(r"System32\tasklist.exe");
        let out = run(Command::new(tasklist).args([
            "/FI",
            "IMAGENAME eq LogonUI.exe",
            "/FO",
            "CSV",
            "/NH",
        ]))?;
        Some(out.to_lowercase().contains("logonui.exe"))
    }
    #[cfg(target_os = "macos")]
    {
        let out = run(Command::new("/usr/sbin/ioreg").args(["-n", "Root", "-d1"]))?;
        Some(out.contains("\"CGSSessionScreenIsLocked\"=Yes"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn run(cmd: &mut Command) -> Option<String> {
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    // The program has no console on Windows: without this a black window would flash.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Remembers the last answer, so only a change to "locked" counts.
#[derive(Debug, Default)]
pub struct LockWatch {
    was_locked: Option<bool>,
}

impl LockWatch {
    /// True when the computer has just been locked.
    pub fn just_locked(&mut self, now: Option<bool>) -> bool {
        let before = std::mem::replace(&mut self.was_locked, now);
        now == Some(true) && before == Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::LockWatch;

    #[test]
    fn only_a_change_to_locked_counts() {
        let mut w = LockWatch::default();
        // Unknown or "always locked" never locks the vault.
        assert!(!w.just_locked(Some(true)));
        assert!(!w.just_locked(Some(true)));
        assert!(!w.just_locked(None));
        assert!(!w.just_locked(Some(false)));
        assert!(w.just_locked(Some(true)));
        assert!(!w.just_locked(Some(true)));
        assert!(!w.just_locked(Some(false)));
        assert!(w.just_locked(Some(true)));
    }
}
