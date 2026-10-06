//! The isolated worker process.
//!
//! The app starts itself with [`WORKER_ARG`], writes the file name and bytes to the worker's
//! stdin, and reads one JSON result from its stdout. The worker never opens the vault, gets
//! no environment (no proxy settings, no paths), and is killed when the time limit passes. On
//! Windows it also runs contained (`dv-sandbox`, D-044): no network, none of her files, no
//! clipboard, one process, a memory ceiling.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use crate::{IngestError, Outcome, MAX_INPUT_BYTES};

/// First argument that turns the app binary into a worker.
pub const WORKER_ARG: &str = "--dv-ingest-worker";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);
/// Text, or a scan's page images (base64): up to about 215 MB for the largest scan allowed.
const MAX_OUTPUT_BYTES: u64 = 224 * 1024 * 1024;

type WorkerResult = Result<Outcome, IngestError>;

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
    let result: WorkerResult = crate::read(&input[newline + 1..], &name);
    let Ok(json) = serde_json::to_vec(&result) else {
        return 3;
    };
    let mut out = std::io::stdout().lock();
    if out.write_all(&json).and_then(|()| out.flush()).is_err() {
        return 4;
    }
    0
}

/// A running isolated process; dropping it ends the process.
pub use dv_sandbox::Child as IsolatedChild;
/// What an isolated process may have (memory ceiling, folders it may read). See D-044.
pub use dv_sandbox::Policy as Isolation;
/// Whether this platform contains the process (Windows: AppContainer and job object).
pub use dv_sandbox::CONTAINED;

/// The worker's ceiling: far above what a text extraction of a 50 MB file needs.
const WORKER_MEMORY_MB: u32 = 1024;

/// Start an untrusted helper process isolated: on Windows in an AppContainer with no network
/// and no access to her files, inside a job object (one process, a memory ceiling, no clipboard,
/// ended with the app); everywhere with an empty environment and no window. The one way the app
/// starts a document reader (the worker, and later the OCR engine).
pub fn spawn_isolated(
    exe: &Path,
    args: &[&str],
    policy: &Isolation,
) -> Result<IsolatedChild, IngestError> {
    // Fail closed: a reader that cannot be isolated is not started.
    dv_sandbox::spawn(exe, args, policy).map_err(|e| IngestError::Worker(e.to_string()))
}

/// App side: run one document through a fresh worker. A scan comes back as page images, which
/// the app reads with the OCR engine ([`crate::import`] does both).
pub fn run(exe: &Path, file_name: &str, bytes: &[u8], timeout: Duration) -> WorkerResult {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(IngestError::TooLarge);
    }
    let policy = Isolation {
        memory_mb: WORKER_MEMORY_MB,
        read_dirs: Vec::new(),
        single_thread: false,
    };
    let mut child = spawn_isolated(exe, &[WORKER_ARG], &policy)?;
    let (Some(mut stdin), Some(mut stdout)) = (child.take_stdin(), child.take_stdout()) else {
        child.kill();
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

    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            child.kill();
            return Err(IngestError::Timeout);
        }
        Err(e) => {
            child.kill();
            return Err(IngestError::Worker(e.to_string()));
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
