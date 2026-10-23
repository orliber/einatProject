//! The worker contained on a Mac (D-101): the system's own sandbox (Seatbelt, the kernel
//! sandbox every Apple app runs in, here through `sandbox-exec` and its profile language),
//! with a profile that denies everything by default and then allows only what a reader needs
//! to start and run:
//!
//! * no network at all (nothing allows it), no writing anywhere but `/dev/null`, no other
//!   process (no fork, and exec only of the worker itself), no talking to system services,
//!   and only the system values in [`SYSCTL_NAMES`] (none about other processes);
//! * reading only the system's own libraries, the worker program's folder and the folders in
//!   [`Policy::read_dirs`]. Her home folder, the vault and every other file stay unreadable;
//!   only names and sizes can be looked up (dyld needs that to find its libraries).
//!
//! The process is started through `/usr/bin/sandbox-exec` by its full path, with the profile
//! on the command line (it holds paths only, never anything from a document), after every
//! descriptor the app may have left open is closed ([`spawn`]). No `unsafe`,
//! no new dependency. A memory ceiling, as the Windows job object sets, has no working
//! equivalent on macOS (`RLIMIT_AS` is not enforced); the app's time limit still ends the
//! worker, and the worker ends itself with the app and within a CPU limit (the app's
//! `mac_platform::contain_worker`). Residual risk in THREAT_MODEL.

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

/// The system values a reader may look up: processors, memory and page sizes, and the
/// system's version (the C library, the memory allocator and the ONNX runtime read these).
const SYSCTL_NAMES: &[&str] = &[
    "hw.ncpu",
    "hw.activecpu",
    "hw.logicalcpu",
    "hw.logicalcpu_max",
    "hw.physicalcpu",
    "hw.physicalcpu_max",
    "hw.memsize",
    "hw.pagesize",
    "hw.cachelinesize",
    "hw.l1dcachesize",
    "hw.l2cachesize",
    "hw.cputype",
    "hw.cpusubtype",
    "hw.cpufamily",
    "hw.nperflevels",
    "kern.osversion",
    "kern.osrelease",
    "kern.ostype",
    "kern.osproductversion",
    "kern.osvariant_status",
    "kern.secure_kernel",
    "sysctl.proc_translated",
    "vm.pagesize",
    // dyld reads these two to find the system's libraries (the OS cryptex) and stops the
    // program before `main` without them (macOS 26).
    "kern.bootargs",
    "security.mac.lockdown_mode_state",
    // The C library's page size and platform version: Rust's start-up stops on a zero page
    // size.
    "hw.pagesize_compat",
    "kern.iossupportversion",
];
/// The same by prefix: processor features and per-core-type sizes.
const SYSCTL_PREFIXES: &[&str] = &["hw.optional.", "hw.perflevel", "machdep.cpu."];

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
    let mut code = String::new();
    for s in SYSTEM_READ {
        code.push_str(&format!(" (subpath \"{s}\")"));
    }
    if let Some(own) = read.first() {
        code.push_str(&format!(" (subpath {})", quote(own)?));
    }
    // Inside the app, the libraries it ships (the FIPS module is a dylib on a Mac) are in
    // Contents/Frameworks beside Contents/MacOS, signed with the app.
    if let Some(frameworks) = exe
        .parent()
        .and_then(Path::parent)
        .map(|contents| contents.join("Frameworks"))
        .filter(|f| f.is_dir())
    {
        code.push_str(&format!(
            " (subpath {})",
            quote(&std::fs::canonicalize(frameworks)?)?
        ));
    }
    let mut subpaths = code.clone();
    for d in read.iter().skip(1) {
        subpaths.push_str(&format!(" (subpath {})", quote(d)?));
    }
    p.push_str(&format!("(allow file-read*{subpaths})\n"));
    // Code is mapped only from the system and the program's own folder, never from the
    // folders a run reads (documents, the name model).
    p.push_str(&format!("(allow file-map-executable{code})\n"));
    // dyld opens the root folder itself (not what is in it) to find the shared cache.
    p.push_str("(allow file-read-data (literal \"/\"))\n");
    p.push_str(
        "(allow file-read* (literal \"/dev/null\") (literal \"/dev/random\") (literal \"/dev/urandom\") (literal \"/private/etc/localtime\"))\n",
    );
    p.push_str("(allow file-write-data (literal \"/dev/null\"))\n");
    // The data volume is also mounted under /System (her home, the vault): never readable,
    // whatever path reaches it. In this language the last matching rule wins.
    p.push_str("(deny file-read* file-map-executable (subpath \"/System/Volumes/Data\"))\n");
    // Only what a program reads about the machine to size itself, never about other
    // processes (their command lines could carry a name into the extracted text).
    p.push_str("(allow sysctl-read");
    for name in SYSCTL_NAMES {
        p.push_str(&format!(" (sysctl-name \"{name}\")"));
    }
    for prefix in SYSCTL_PREFIXES {
        p.push_str(&format!(" (sysctl-name-prefix \"{prefix}\")"));
    }
    p.push_str(")\n");
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

/// Closes every descriptor that is open from 3 up, whatever its number, before the next
/// program starts: the sandbox does not stop a program reading a descriptor it inherits, and
/// a library in the app may have left one open across exec. A listing it cannot read stops
/// the start (fail-closed). Then the most descriptors the next program may hold is set to
/// [`FD_LIMIT`], so the worker's own check of every number below it covers all it can have,
/// and its arguments are `exec`ed (shell builtins only, no other program).
pub const CLOSE_INHERITED: &str = "for f in /dev/fd/*; do n=${f##*/}; case $n in ''|*[!0-9]*) exit 70;; esac; [ \"$n\" -gt 2 ] && eval \"exec $n>&-\"; done; ulimit -n 1024 || exit 71; exec \"$@\"";

/// The most descriptors a worker may hold (`ulimit -n` in [`CLOSE_INHERITED`]).
pub const FD_LIMIT: u64 = 1024;

/// Start `exe` inside the profile: `/bin/bash` closes what the app left open, `/usr/bin/env
/// -i` drops the shell's own variables, and `sandbox-exec` applies the profile and becomes the
/// worker. Each `exec`s the next, so the process the app holds is the worker itself.
pub fn spawn(exe: &Path, args: &[&str], policy: &Policy) -> io::Result<std::process::Child> {
    let profile = profile(exe, policy)?;
    let exe = std::fs::canonicalize(exe)?;
    std::process::Command::new("/bin/bash")
        .args(["-c", CLOSE_INHERITED, "dv-sandbox", "/usr/bin/env", "-i"])
        .args(policy.single_thread.then_some("OMP_THREAD_LIMIT=1"))
        .args([SANDBOX_EXEC, "-p"])
        .arg(profile)
        .arg(exe)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .env_clear()
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
        assert!(!p.contains("(allow sysctl-read)"), "{p}");
        assert!(!p.contains("kern.proc"), "{p}");
        let real = std::fs::canonicalize(dir.path()).unwrap();
        assert!(
            p.contains(&format!("(subpath \"{}\")", real.display())),
            "{p}"
        );
    }

    /// The start closes the descriptors the app left open (here: 5 and 3000, kept across exec
    /// by bash), whatever their number; without it the next program could read them. It then
    /// holds no more than [`FD_LIMIT`].
    #[test]
    fn nothing_the_app_left_open_reaches_the_next_program() {
        let next = |script: &str| {
            let out = std::process::Command::new("/bin/bash")
                .args([
                    "-c",
                    "ulimit -n 4096 && exec 5</dev/null 3000</dev/null && exec /bin/bash -c \"$0\" dv-sandbox /bin/bash -c 'for n in 5 3000; do { : <&$n; } 2>/dev/null && echo open || echo closed; done; ulimit -n'",
                    script,
                ])
                .env_clear()
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        assert_eq!(next("exec \"$@\""), "open\nopen\n4096\n");
        assert_eq!(
            next(CLOSE_INHERITED),
            format!("closed\nclosed\n{FD_LIMIT}\n")
        );
    }

    /// An app's own Frameworks folder (Contents/Frameworks beside Contents/MacOS) is code the
    /// worker may load; a folder beside a program that is not in an app is not added.
    #[test]
    fn the_apps_own_frameworks_are_readable_code() {
        let app = tempfile::tempdir().unwrap();
        let contents = app.path().join("Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::create_dir_all(contents.join("Frameworks")).unwrap();
        let exe = contents.join("MacOS").join("app");
        std::fs::copy("/bin/cat", &exe).unwrap();
        let p = profile(&exe, &Policy::default()).unwrap();
        let frameworks = std::fs::canonicalize(contents.join("Frameworks")).unwrap();
        let rule = format!("(subpath \"{}\")", frameworks.display());
        let mapped = p
            .lines()
            .find(|l| l.starts_with("(allow file-map-executable"))
            .unwrap();
        assert!(mapped.contains(&rule), "{p}");

        let p = profile(Path::new("/bin/cat"), &Policy::default()).unwrap();
        assert!(!p.contains("Frameworks"), "{p}");
    }

    #[test]
    fn a_path_that_would_change_the_profile_is_refused() {
        for bad in ["/tmp/a\"b", "/tmp/a\\b", "/tmp/a\nb"] {
            assert!(quote(Path::new(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(quote(Path::new("/tmp/a b")).unwrap(), "\"/tmp/a b\"");
    }
}
