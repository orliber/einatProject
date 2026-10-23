//! The start on Linux (uncontained, development) and macOS (the system sandbox, D-101): what
//! the worker inherits, how it is waited for and ended, and on a Mac what the sandbox keeps
//! from it. The Windows containment has its own tests in `src/windows.rs`.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use dv_sandbox::{spawn, Policy};

fn output_of(exe: &str, args: &[&str], policy: &Policy) -> String {
    let mut child = spawn(Path::new(exe), args, policy).unwrap();
    let mut out = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    let status = child.wait_timeout(Duration::from_secs(10)).unwrap();
    assert!(status.is_some_and(|s| s.success()), "{status:?}");
    out
}

#[test]
fn the_worker_inherits_no_environment() {
    // Nothing of the app's environment (paths, tokens, the user's home) reaches the worker.
    assert_eq!(output_of("/usr/bin/env", &[], &Policy::default()), "");
    // The one variable that may be added, and only when asked for.
    let single = Policy {
        single_thread: true,
        ..Policy::default()
    };
    assert_eq!(
        output_of("/usr/bin/env", &[], &single),
        "OMP_THREAD_LIMIT=1\n"
    );
}

#[test]
fn the_worker_writes_nothing_to_the_app_but_its_stdout() {
    // stderr goes nowhere: a parser's error output (which may quote the document) never
    // reaches the app's console or logs.
    let out = output_of(
        "/bin/bash",
        &["-c", "echo quoted-document-text >&2; echo ok"],
        &Policy::default(),
    );
    assert_eq!(out, "ok\n");
}

#[test]
fn stdin_and_stdout_carry_the_bytes_once() {
    let mut child = spawn(Path::new("/bin/cat"), &[], &Policy::default()).unwrap();
    let mut stdin = child.take_stdin().unwrap();
    let mut stdout = child.take_stdout().unwrap();
    assert!(child.take_stdin().is_none() && child.take_stdout().is_none());
    let payload: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
    let writer = {
        let payload = payload.clone();
        std::thread::spawn(move || {
            stdin.write_all(&payload).unwrap();
        })
    };
    let mut back = Vec::new();
    stdout.read_to_end(&mut back).unwrap();
    writer.join().unwrap();
    assert_eq!(back, payload);
}

#[test]
fn a_running_worker_times_out_and_dropping_it_ends_it() {
    let mut child = spawn(Path::new("/bin/cat"), &[], &Policy::default()).unwrap();
    let mut stdout = child.take_stdout().unwrap();
    // `cat` waits on its open stdin forever: the wait gives up after the time limit.
    let started = Instant::now();
    assert!(child
        .wait_timeout(Duration::from_millis(100))
        .unwrap()
        .is_none());
    assert!(started.elapsed() >= Duration::from_millis(100));
    // Dropping the handle kills it: its stdout reaches the end at once.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf.len());
    });
    drop(child);
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(0));
}

#[test]
fn kill_ends_it_and_the_status_says_it_failed() {
    let mut child = spawn(Path::new("/bin/sleep"), &["30"], &Policy::default()).unwrap();
    child.kill();
    let status = child.wait_timeout(Duration::from_secs(5)).unwrap();
    assert!(status.is_some_and(|s| !s.success()), "{status:?}");
    // Killing twice is harmless.
    child.kill();
}

#[test]
fn a_missing_program_is_an_error_not_a_panic() {
    let r = spawn(
        Path::new("/nonexistent/dv-ingest-worker"),
        &[],
        &Policy::default(),
    );
    assert!(r.is_err());
}

/// On a Mac the worker is contained: each of these works unsandboxed and fails here.
#[cfg(target_os = "macos")]
mod contained_on_a_mac {
    use super::*;
    use std::net::TcpListener;

    fn bash(script: &str, policy: &Policy) -> String {
        output_of("/bin/bash", &["-c", script], policy)
    }

    #[test]
    fn no_network_not_even_this_computer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let out = bash(
            // Builtins and redirections only: no second process, so only the network can fail.
            &format!(
                ": 3<>/dev/tcp/127.0.0.1/{port} 2>/dev/null && echo connected || echo blocked"
            ),
            &Policy::default(),
        );
        assert_eq!(out, "blocked\n");
    }

    #[test]
    fn her_files_are_unreadable_unless_granted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "fabricated case note\n").unwrap();
        // `read` is a builtin: the file is opened by the worker itself.
        let script = format!(
            "read -r line < '{}' 2>/dev/null && printf '%s' \"$line\" || echo denied",
            file.display()
        );
        assert_eq!(bash(&script, &Policy::default()), "denied\n");
        let granted = Policy {
            read_dirs: vec![dir.path().to_path_buf()],
            ..Policy::default()
        };
        assert_eq!(bash(&script, &granted), "fabricated case note");
        // Nor through the data volume's mount under /System.
        let real = std::fs::canonicalize(&file).unwrap();
        let script = format!(
            "read -r line < '/System/Volumes/Data{}' 2>/dev/null && printf '%s' \"$line\" || echo denied",
            real.display()
        );
        assert_eq!(bash(&script, &Policy::default()), "denied\n");
    }

    #[test]
    fn nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.txt");
        let granted = Policy {
            read_dirs: vec![dir.path().to_path_buf()],
            ..Policy::default()
        };
        let script = format!(
            "echo x > '{}' 2>/dev/null && echo wrote || echo denied",
            file.display()
        );
        assert_eq!(bash(&script, &granted), "denied\n");
        assert!(!file.exists());
    }

    /// The worker cannot start another program (a parser bug cannot hand off to a shell).
    #[test]
    fn no_second_process() {
        // A refused fork ends the shell itself: nothing is printed and it fails.
        let mut child = spawn(
            Path::new("/bin/bash"),
            &["-c", "/bin/echo started"],
            &Policy::default(),
        )
        .unwrap();
        let mut out = String::new();
        child
            .take_stdout()
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        let status = child.wait_timeout(Duration::from_secs(10)).unwrap();
        assert!(status.is_some_and(|s| !s.success()), "{status:?}");
        assert_eq!(out, "");
        // Nor replace itself with another program (`exec` is a builtin; with `execfail` a
        // refused exec returns instead of ending the shell).
        let out = bash(
            "shopt -s execfail; exec /bin/echo started 2>/dev/null; echo denied",
            &Policy::default(),
        );
        assert_eq!(out, "denied\n");
    }
}
