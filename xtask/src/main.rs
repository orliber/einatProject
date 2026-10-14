//! Repository checks that keep the architecture honest.
//!
//! * `cargo xtask check-invariants` – network isolation, `unsafe` policy, UI and
//!   Tauri hardening (see `docs/ARCHITECTURE.md`).
//! * `cargo xtask scan --staged | --all` – blocks personal data, archives and
//!   binaries from entering the repository (pre-commit hook and CI).
//! * `cargo xtask update-keygen [--next]` / `update-notice …` / `update-key-backup …` /
//!   `update-key-restore …` – signed updates, on the developer's
//!   computer only (D-033, `scripts/release.sh`).
//! * `cargo xtask gen-ipc` – the TypeScript of every command, from the shell (`ipc.rs`).
//! * `cargo xtask ci [--e2e] [--rust|--ui]` – the CI checks, on this computer, in one command.
//! * `cargo xtask check-parsers` / `sbom <out.json>` / `self-audit [--full]` – the supply
//!   chain (SEC6–SEC8, `supply.rs`).

mod ci;
mod invariants;
mod ipc;
mod israeli_id;
mod release;
mod scan;
mod supply;

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

fn done(title: &str, result: Result<String, String>) -> ExitCode {
    match result {
        Ok(msg) => {
            let _ = writeln!(std::io::stderr().lock(), "{title}: {msg}");
            ExitCode::SUCCESS
        }
        Err(e) => report(title, &[e]),
    }
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
        Some("check-parsers") => match supply::check_parsers(&root) {
            Ok(problems) => report("parser pins", &problems),
            Err(e) => report("parser pins", &[format!("could not run: {e}")]),
        },
        Some("gen-ipc") => done("gen-ipc", ipc::write(&root)),
        Some("sbom") => done("sbom", supply::sbom(&root, &args[1..])),
        Some("self-audit") => report("self-audit", &supply::self_audit(&root, &args[1..])),
        Some("ci") => report("ci", &ci::run_all(&root, &args[1..])),
        Some("update-keygen") => done("update-keygen", release::keygen(&root, &args[1..])),
        Some("update-key-backup") => done("update-key-backup", release::key_backup(&root, &args[1..])),
        Some("update-key-restore") => done("update-key-restore", release::key_restore(&root, &args[1..])),
        Some("update-notice") => done("update-notice", release::notice(&root, &args[1..])),
        _ => report(
            "usage",
            &["cargo xtask check-invariants | cargo xtask scan [--staged|--all] | cargo xtask ci [--e2e] [--rust|--ui]".to_owned()],
        ),
    }
}
