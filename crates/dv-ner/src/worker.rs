//! The model in its own isolated process (SEC2, D-044): the same binary started with
//! [`WORKER_ARG`] and the model folder. On Windows it runs contained like the document worker
//! (no network, none of her files but the model folder, a memory ceiling, ended with the app).
//! It loads the model once and then answers texts until its stdin closes, so the 3 seconds of
//! loading are paid once per unlock, not once per material.
//!
//! The pipes carry frames: a 4-byte little-endian length, then the bytes. In: one text
//! (UTF-8). Out: first one frame saying whether the model loaded, then one frame per text with
//! its people as JSON. A span the app did not ask for (out of range, not on a character
//! boundary) is a broken worker, never a hint.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{NerModel, NerSpan};

/// First argument that turns the app binary into the name model worker; the second is the
/// model folder.
pub const WORKER_ARG: &str = "--dv-ner-worker";
/// Ceiling for the worker: the model takes about 1.4 GB at its peak (D-053).
pub const WORKER_MEMORY_MB: u32 = 3072;
/// The longest text sent at once (a material is at most a few hundred KB of text).
const MAX_TEXT_BYTES: u32 = 8 * 1024 * 1024;
/// The longest answer: spans are small, a text has at most this many bytes of them.
const MAX_ANSWER_BYTES: u32 = 16 * 1024 * 1024;
/// Loading the model.
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// One person span, as it crosses the pipe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub start: usize,
    pub end: usize,
    pub score: f32,
}

#[derive(Debug, Serialize, Deserialize)]
enum Loaded {
    Ok,
    Failed(String),
}

fn read_frame(r: &mut impl Read, max: u32) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len);
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0; len as usize];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

fn write_frame(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

/// Worker side. Returns the process exit code.
#[must_use]
pub fn worker_main(dir: &Path) -> i32 {
    let mut out = io::stdout().lock();
    let model = NerModel::load(dir);
    let loaded = match &model {
        Ok(_) => Loaded::Ok,
        Err(e) => Loaded::Failed(e.to_string()),
    };
    let Ok(json) = serde_json::to_vec(&loaded) else {
        return 3;
    };
    if write_frame(&mut out, &json).is_err() {
        return 4;
    }
    let Ok(model) = model else { return 5 };
    let mut input = io::stdin().lock();
    loop {
        let text = match read_frame(&mut input, MAX_TEXT_BYTES) {
            Ok(Some(t)) => t,
            Ok(None) => return 0,
            Err(_) => return 2,
        };
        let Ok(text) = String::from_utf8(text) else {
            return 2;
        };
        let people: Vec<Person> = match model.people(&text) {
            Ok(p) => p
                .into_iter()
                .map(
                    |NerSpan {
                         start, end, score, ..
                     }| Person { start, end, score },
                )
                .collect(),
            Err(_) => return 6,
        };
        let Ok(json) = serde_json::to_vec(&people) else {
            return 3;
        };
        if write_frame(&mut out, &json).is_err() {
            return 4;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("could not start: {0}")]
    Start(String),
    #[error("the model did not load: {0}")]
    Load(String),
    #[error("no answer in time")]
    Timeout,
    #[error("the worker ended or answered wrongly: {0}")]
    Broken(String),
}

/// App side: a running worker with the model loaded. Dropping it ends the process.
pub struct NerWorker {
    child: dv_sandbox::Child,
    stdin: Box<dyn Write + Send>,
    answers: Receiver<io::Result<Vec<u8>>>,
}

impl std::fmt::Debug for NerWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NerWorker").finish_non_exhaustive()
    }
}

impl NerWorker {
    /// Start `exe` as the worker on the model in `dir`, contained, and wait until it loaded.
    pub fn start(exe: &Path, dir: &Path) -> Result<Self, WorkerError> {
        let dir_arg = dir
            .to_str()
            .ok_or_else(|| WorkerError::Start("model folder is not UTF-8".to_owned()))?;
        let policy = dv_sandbox::Policy {
            memory_mb: WORKER_MEMORY_MB,
            read_dirs: vec![dir.to_path_buf()],
            single_thread: false,
        };
        // Fail closed: a model that cannot be isolated is not started.
        let mut child = dv_sandbox::spawn(exe, &[WORKER_ARG, dir_arg], &policy)
            .map_err(|e| WorkerError::Start(e.to_string()))?;
        let (Some(stdin), Some(mut stdout)) = (child.take_stdin(), child.take_stdout()) else {
            child.kill();
            return Err(WorkerError::Start("pipes".to_owned()));
        };
        let (tx, answers) = mpsc::channel();
        std::thread::spawn(move || loop {
            let frame = read_frame(&mut stdout, MAX_ANSWER_BYTES).and_then(|f| {
                f.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "worker ended"))
            });
            let end = frame.is_err();
            if tx.send(frame).is_err() || end {
                return;
            }
        });
        let mut worker = Self {
            child,
            stdin,
            answers,
        };
        let first = worker.answer(LOAD_TIMEOUT)?;
        match serde_json::from_slice::<Loaded>(&first) {
            Ok(Loaded::Ok) => Ok(worker),
            Ok(Loaded::Failed(why)) => Err(WorkerError::Load(why)),
            Err(e) => Err(WorkerError::Broken(e.to_string())),
        }
    }

    fn answer(&mut self, timeout: Duration) -> Result<Vec<u8>, WorkerError> {
        match self.answers.recv_timeout(timeout) {
            Ok(Ok(frame)) => Ok(frame),
            Ok(Err(e)) => Err(WorkerError::Broken(e.to_string())),
            Err(RecvTimeoutError::Timeout) => {
                self.child.kill();
                Err(WorkerError::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(WorkerError::Broken("ended".to_owned())),
        }
    }

    /// The people in `text`. After an error the worker is not used again.
    pub fn people(&mut self, text: &str) -> Result<Vec<Person>, WorkerError> {
        if u32::try_from(text.len()).map_or(true, |n| n > MAX_TEXT_BYTES) {
            return Err(WorkerError::Broken("text too long".to_owned()));
        }
        write_frame(&mut self.stdin, text.as_bytes())
            .map_err(|e| WorkerError::Broken(e.to_string()))?;
        // About 8 seconds per thousand words on an ordinary CPU (D-053), with room to spare.
        let words = u64::try_from(text.split_whitespace().count()).unwrap_or(u64::MAX);
        let timeout = Duration::from_secs(30_u64.saturating_add(words.saturating_div(25)));
        let frame = self.answer(timeout)?;
        let people: Vec<Person> =
            serde_json::from_slice(&frame).map_err(|e| WorkerError::Broken(e.to_string()))?;
        if people.iter().any(|p| {
            p.start >= p.end
                || p.end > text.len()
                || !text.is_char_boundary(p.start)
                || !text.is_char_boundary(p.end)
                || !p.score.is_finite()
        }) {
            self.child.kill();
            return Err(WorkerError::Broken("span outside the text".to_owned()));
        }
        Ok(people)
    }
}

impl Drop for NerWorker {
    fn drop(&mut self) {
        self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn frames_round_trip_and_an_oversized_one_is_refused() {
        let mut buf = Vec::new();
        write_frame(&mut buf, "נוגה".as_bytes()).unwrap();
        let mut r = buf.as_slice();
        assert_eq!(read_frame(&mut r, 100).unwrap().unwrap(), "נוגה".as_bytes());
        assert!(read_frame(&mut r, 100).unwrap().is_none(), "clean end");
        assert!(read_frame(&mut buf.as_slice(), 3).is_err());
        assert!(read_frame(&mut &buf[..6], 100).is_err(), "cut short");
    }
}
