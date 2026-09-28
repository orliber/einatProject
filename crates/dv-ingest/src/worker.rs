//! The isolated worker process.
//!
//! The app starts itself with [`WORKER_ARG`], writes the file name and bytes to the worker's
//! stdin, and reads one JSON result from its stdout. The worker never opens the vault, gets
//! no environment (no proxy settings, no paths), and is killed when the time limit passes.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{Extracted, IngestError, MAX_INPUT_BYTES};

/// First argument that turns the app binary into a worker.
pub const WORKER_ARG: &str = "--dv-ingest-worker";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

type WorkerResult = Result<Extracted, IngestError>;

/// Worker side: stdin → extract → JSON on stdout. Returns the process exit code.
#[must_use]
pub fn worker_main() -> i32 {
    let mut input = Vec::new();
    let cap = u64::try_from(MAX_INPUT_BYTES).unwrap_or(u64::MAX) + 1024;
    if std::io::stdin()
        .lock()
        .take(cap)
        .read_to_end(&mut input)
        .is_err()
    {
        return 2;
    }
    let Some(newline) = input.iter().position(|b| *b == b'\n') else {
        return 2;
    };
    let name = String::from_utf8_lossy(&input[..newline]).into_owned();
    let result: WorkerResult = crate::extract(&input[newline + 1..], &name);
    let Ok(json) = serde_json::to_vec(&result) else {
        return 3;
    };
    let mut out = std::io::stdout().lock();
    if out.write_all(&json).and_then(|()| out.flush()).is_err() {
        return 4;
    }
    0
}

/// App side: run one document through a fresh worker.
pub fn run(exe: &Path, file_name: &str, bytes: &[u8], timeout: Duration) -> WorkerResult {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(IngestError::TooLarge);
    }
    let mut cmd = Command::new(exe);
    cmd.arg(WORKER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_clear();
    for keep in ["SYSTEMROOT", "WINDIR"] {
        if let Some(v) = std::env::var_os(keep) {
            cmd.env(keep, v);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| IngestError::Worker(e.to_string()))?;
    let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return Err(IngestError::Worker("pipes".to_owned()));
    };

    let mut payload = file_name.replace(['\n', '\r'], " ").into_bytes();
    payload.push(b'\n');
    payload.extend_from_slice(bytes);
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&payload);
    });
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        (&mut stdout)
            .take(MAX_OUTPUT_BYTES)
            .read_to_end(&mut buf)
            .map(|_| buf)
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(IngestError::Timeout);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                let _ = child.kill();
                return Err(IngestError::Worker(e.to_string()));
            }
        }
    };
    let _ = writer.join();
    let output = reader
        .join()
        .map_err(|_| IngestError::Worker("reader".to_owned()))?
        .map_err(|e| IngestError::Worker(e.to_string()))?;
    if !status.success() {
        // A parser crash on a hostile or broken file ends here, not in the app.
        return Err(IngestError::Corrupt(format!("worker exited: {status}")));
    }
    serde_json::from_slice::<WorkerResult>(&output)
        .map_err(|e| IngestError::Worker(e.to_string()))?
}
