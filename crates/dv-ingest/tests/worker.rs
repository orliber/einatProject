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

/// A one-page PDF built in memory with a correct cross-reference table.
fn pdf_page(resources: &str, content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources {resources} /Contents 4 0 R >>"),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

#[test]
fn a_crash_inside_the_pdf_library_ends_in_the_worker_not_the_app() {
    // The page draws an image its resources do not hold. The PDF library (a dependency)
    // panics on this rather than returning an error; in the app that panic must stay in the
    // worker process and come back as a refusal.
    let bytes = pdf_page("<< >>", "q 595 0 0 842 0 0 cm /Im1 Do Q");
    let in_process =
        std::panic::catch_unwind(|| dv_ingest::read(&bytes, "x.pdf")).map_err(|_| "panic");
    let r = worker::run(exe(), "x.pdf", &bytes, worker::DEFAULT_TIMEOUT);
    match in_process {
        Err(_) => assert!(
            matches!(&r, Err(IngestError::Corrupt(m)) if m.contains("worker exited")),
            "{r:?}"
        ),
        Ok(local) => assert_eq!(r, local, "the worker answers as the reader does"),
    }
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn hostile_files_are_refused_across_the_boundary() {
    use std::io::Write;
    // A zip bomb: refused inside the worker, under its memory ceiling.
    let huge = "א".repeat(25 * 1024 * 1024);
    let doc = format!(
        "<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>{huge}</w:t></w:r></w:p></w:body></w:document>"
    );
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(doc.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    assert_eq!(
        worker::run(
            exe(),
            "bomb.docx",
            &buf.into_inner(),
            worker::DEFAULT_TIMEOUT
        ),
        Err(IngestError::TooLarge)
    );
    // Nesting deep enough to overflow a recursive parser's stack.
    let mut rtf = br"{\rtf1 ".to_vec();
    rtf.extend(std::iter::repeat_n(b'{', 100_000));
    assert!(matches!(
        worker::run(exe(), "deep.rtf", &rtf, worker::DEFAULT_TIMEOUT),
        Err(IngestError::Corrupt(_))
    ));
    // Too large to send at all: refused before a worker starts.
    let big = vec![b'a'; dv_ingest::MAX_INPUT_BYTES + 1];
    assert_eq!(
        worker::run(exe(), "big.txt", &big, worker::DEFAULT_TIMEOUT),
        Err(IngestError::TooLarge)
    );
}

#[test]
fn a_file_name_cannot_inject_into_the_worker_protocol() {
    // The name and the bytes travel on one stream, split at the first line break. A name with
    // line breaks must not move that split (and so change what is read).
    let r = worker::run(
        exe(),
        "notes\n%PDF-1.7\r\n.txt",
        "תצפית בדויה.".as_bytes(),
        worker::DEFAULT_TIMEOUT,
    )
    .unwrap();
    let Outcome::Text(out) = r else {
        panic!("text expected")
    };
    assert_eq!(out.body, "תצפית בדויה.");
}
