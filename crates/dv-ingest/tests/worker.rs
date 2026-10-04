//! The process boundary: results, crashes and timeouts. Fabricated text only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use dv_ingest::worker::{self, WORKER_ARG};
use dv_ingest::{Format, IngestError, Outcome};

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_dv-ingest-worker"))
}

#[test]
fn text_round_trip() {
    let out = worker::run(
        exe(),
        "notes.txt",
        "תצפית בגן: הילד משחק לבד.".as_bytes(),
        worker::DEFAULT_TIMEOUT,
    )
    .unwrap();
    let Outcome::Text(out) = out else {
        panic!("text expected")
    };
    assert_eq!(out.format, Format::Text);
    assert_eq!(out.body, "תצפית בגן: הילד משחק לבד.");
}

#[test]
fn errors_cross_the_boundary() {
    let r = worker::run(exe(), "x.bin", &[0, 1, 2, 3], worker::DEFAULT_TIMEOUT);
    assert_eq!(r, Err(IngestError::Unsupported));
    assert_eq!(
        worker::run(exe(), "empty.txt", b"", worker::DEFAULT_TIMEOUT),
        Err(IngestError::Empty)
    );
}

#[test]
fn broken_pdf_does_not_reach_the_app() {
    let r = worker::run(exe(), "x.pdf", b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 9 0 R >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF", worker::DEFAULT_TIMEOUT);
    assert!(
        matches!(r, Err(IngestError::Corrupt(_) | IngestError::Scanned)),
        "{r:?}"
    );
}

#[test]
fn time_limit_kills_the_worker() {
    let big = "א".repeat(4 * 1024 * 1024);
    let r = worker::run(exe(), "big.txt", big.as_bytes(), Duration::ZERO);
    assert_eq!(r, Err(IngestError::Timeout));
}

#[test]
fn worker_arg_is_stable() {
    // The desktop binary checks this exact argument before starting anything else.
    assert_eq!(WORKER_ARG, "--dv-ingest-worker");
}
