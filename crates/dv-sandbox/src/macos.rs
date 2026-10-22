//! The worker contained on a Mac (D-101): the system's own sandbox (`sandbox-exec`, the
//! Seatbelt profile language every Apple app runs under), with a profile that denies
//! everything by default and then allows only what a reader needs to start and run:
//!
//! * no network at all (nothing allows it), no writing anywhere but `/dev/null`, no other
//!   process (no fork, and exec only of the worker itself), no talking to system services;
//! * reading only the system's own libraries, the worker program's folder and the folders in
//!   [`Policy::read_dirs`]. Her home folder, the vault and every other file stay unreadable;
//!   only names and sizes can be looked up (dyld needs that to find its libraries).
//!
//! The process is started through `/usr/bin/sandbox-exec` by its full path, with the profile
//! on the command line (it holds paths only, never anything from a document). No `unsafe`,
//! no new dependency. A memory ceiling, as the Windows job object sets, has no working
//! equivalent on macOS (`RLIMIT_AS` is not enforced); the time limit still ends the worker.

use std::io;
use std::path::{Path, PathBuf};

use crate::Policy;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// System folders a program needs to start: the libraries, the dyld shared cache (in the
/// cryptex on macOS 13 and later), locale data and the time zone.
const SYSTEM_READ: &[&str] = &[
    "/System",
    "/usr/lib",
    "/usr/share",
    "/Library/Apple/System",
    "/Library/Preferences/Logging",
    "/private/var/db/dyld",
    "/private/var/db/timezone",
    "/System/Volumes/Preboot/Cryptexes",
];

/// The profile for `exe`, which may also read `policy.read_dirs`.
pub fn profile(exe: &Path, policy: &Policy) -> io::Result<String> {
    let exe = std::fs::canonicalize(exe)?;
    let mut read: Vec<PathBuf> = Vec::new();
    // The worker's own folder: the program and what ships beside it (the OCR engine's data).
    if let Some(dir) = exe.parent() {
        read.push(dir.to_path_buf());
    }
    for d in &policy.read_dirs {
        read.push(std::fs::canonicalize(d)?);
    }
    let mut p = String::from("(version 1)\n(deny default)\n");
    p.push_str(&format!(
        "(allow process-exec (literal {}))\n",
        quote(&exe)?
    ));
    p.push_str("(allow file-read-metadata)\n");
    let mut subpaths = String::new();
    for s in SYSTEM_READ {
        subpaths.push_str(&format!(" (subpath \"{s}\")"));
    }
    for d in &read {
        subpaths.push_str(&format!(" (subpath {})", quote(d)?));
    }
    p.push_str(&format!("(allow file-read*{subpaths})\n"));
    p.push_str(&format!("(allow file-map-executable{subpaths})\n"));
    p.push_str(
        "(allow file-read* (literal \"/dev/null\") (literal \"/dev/random\") (literal \"/dev/urandom\") (literal \"/private/etc/localtime\"))\n",
    );
    p.push_str("(allow file-write-data (literal \"/dev/null\"))\n");
    p.push_str("(allow sysctl-read)\n");
    p.push_str("(allow signal (target self))\n");
    Ok(p)
}

/// A path as a profile string. Paths with a quote, a backslash or a control character are
/// refused rather than escaped: none of the app's own paths has one, and a profile that does
/// not say exactly what was meant is worse than a reader that does not start (fail-closed).
fn quote(path: &Path) -> io::Result<String> {
    let s = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))?;
    if s.chars().any(|c| c == '"' || c == '\\' || c.is_control()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path cannot go in a sandbox profile",
        ));
    }
    Ok(format!("\"{s}\""))
}

/// Start `exe` inside the profile.
pub fn spawn(exe: &Path, args: &[&str], policy: &Policy) -> io::Result<std::process::Child> {
    let profile = profile(exe, policy)?;
    let exe = std::fs::canonicalize(exe)?;
    std::process::Command::new(SANDBOX_EXEC)
        .arg("-p")
        .arg(profile)
        .arg(exe)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .env_clear()
        .envs(policy.single_thread.then_some(("OMP_THREAD_LIMIT", "1")))
        .current_dir("/")
        .spawn()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_denies_by_default_and_names_only_its_folders() {
        let dir = tempfile::tempdir().unwrap();
        let p = profile(
            Path::new("/bin/cat"),
            &Policy {
                read_dirs: vec![dir.path().to_path_buf()],
                ..Policy::default()
            },
        )
        .unwrap();
        assert!(p.starts_with("(version 1)\n(deny default)\n"), "{p}");
        assert!(!p.contains("network"), "{p}");
        assert!(!p.contains("process-fork"), "{p}");
        assert!(!p.contains("mach-lookup"), "{p}");
        let real = std::fs::canonicalize(dir.path()).unwrap();
        assert!(
            p.contains(&format!("(subpath \"{}\")", real.display())),
            "{p}"
        );
    }

    #[test]
    fn a_path_that_would_change_the_profile_is_refused() {
        for bad in ["/tmp/a\"b", "/tmp/a\\b", "/tmp/a\nb"] {
            assert!(quote(Path::new(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(quote(Path::new("/tmp/a b")).unwrap(), "\"/tmp/a b\"");
    }
}
