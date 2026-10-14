//! The uncontained start used in development (Linux, macOS): what the worker inherits, how it
//! is waited for and ended. The Windows containment has its own tests in `src/windows.rs`.

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
        "/bin/sh",
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
