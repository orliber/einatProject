//! Repository checks that keep the architecture honest.
//!
//! * `cargo xtask check-invariants` – network isolation, `unsafe` policy, UI and
//!   Tauri hardening (see `docs/ARCHITECTURE.md`).
//! * `cargo xtask scan --staged | --all` – blocks personal data, archives and
//!   binaries from entering the repository (pre-commit hook and CI).

mod invariants;
mod israeli_id;
mod scan;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

fn report(title: &str, problems: &[String]) -> ExitCode {
    let mut err = std::io::stderr().lock();
    if problems.is_empty() {
        let _ = writeln!(err, "{title}: ok");
        return ExitCode::SUCCESS;
    }
    let _ = writeln!(err, "{title}: {} problem(s)", problems.len());
    for p in problems {
        let _ = writeln!(err, "  - {p}");
    }
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    match args.first().map(String::as_str) {
        Some("check-invariants") => match invariants::check_all(&root) {
            Ok(problems) => report("invariants", &problems),
            Err(e) => report("invariants", &[format!("could not run: {e}")]),
        },
        Some("scan") => {
            let mode = match args.get(1).map(String::as_str) {
                Some("--staged") => scan::Mode::Staged,
                Some("--all") | None => scan::Mode::All,
                Some(other) => return report("scan", &[format!("unknown option {other}")]),
            };
            match scan::run(&root, mode) {
                Ok(problems) => report("scan", &problems),
                Err(e) => report("scan", &[format!("could not run: {e}")]),
            }
        }
        _ => report(
            "usage",
            &["cargo xtask check-invariants | cargo xtask scan [--staged|--all]".to_owned()],
        ),
    }
}
