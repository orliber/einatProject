//! `cargo xtask ci`: the checks of `.github/workflows/ci.yml` that run on Linux, on this
//! computer, in one command (the Actions minutes are not always there). Every step runs even
//! after one fails, and the summary lists what failed. A tool that is missing is a failure,
//! not a skip: a check that did not run did not pass.

use std::path::Path;
use std::process::Command;

struct Step {
    title: &'static str,
    dir: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
}

const RUST: &[Step] = &[
    Step {
        title: "cargo fmt",
        dir: "",
        program: "cargo",
        args: &["fmt", "--all", "--check"],
        env: &[],
    },
    Step {
        title: "cargo clippy",
        dir: "",
        program: "cargo",
        args: &["clippy", "--all-targets", "--", "-D", "warnings"],
        env: &[],
    },
    Step {
        title: "cargo test",
        dir: "",
        program: "cargo",
        args: &["test"],
        env: &[],
    },
    Step {
        title: "IPC types are up to date",
        dir: "",
        program: "git",
        args: &[
            "diff",
            "--exit-code",
            "--",
            "apps/desktop/src/ipc/generated",
        ],
        env: &[],
    },
    Step {
        title: "cargo deny",
        dir: "",
        program: "cargo",
        args: &["deny", "check"],
        env: &[],
    },
];

const UI: &[Step] = &[
    Step {
        title: "pnpm install",
        dir: "apps/desktop",
        program: "pnpm",
        args: &["install", "--frozen-lockfile"],
        env: &[],
    },
    Step {
        title: "tsc",
        dir: "apps/desktop",
        program: "pnpm",
        args: &["typecheck"],
        env: &[],
    },
    Step {
        title: "eslint",
        dir: "apps/desktop",
        program: "pnpm",
        args: &["lint"],
        env: &[],
    },
    Step {
        title: "vitest",
        dir: "apps/desktop",
        program: "pnpm",
        args: &["test"],
        env: &[],
    },
    Step {
        title: "UI build",
        dir: "apps/desktop",
        program: "pnpm",
        args: &["build"],
        env: &[],
    },
];

/// The whole app, end to end (`tests/e2e/run.py`): Linux with Xvfb and tauri-driver.
const E2E: &[Step] = &[
    Step {
        title: "app build (debug)",
        dir: "apps/desktop",
        program: "pnpm",
        args: &["tauri", "build", "--debug", "--no-bundle"],
        env: &[],
    },
    Step {
        title: "end to end",
        dir: "",
        program: "python3",
        args: &["tests/e2e/run.py"],
        env: &[],
    },
];

fn run(root: &Path, step: &Step) -> Result<(), String> {
    eprintln_step(step.title);
    let status = Command::new(step.program)
        .args(step.args)
        .envs(step.env.iter().copied())
        .current_dir(root.join(step.dir))
        .status()
        .map_err(|e| format!("{}: could not start `{}` ({e})", step.title, step.program))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{}: failed ({status})", step.title))
    }
}

fn eprintln_step(title: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "== {title}");
}

/// Runs the steps; returns what failed. `--e2e` adds the end-to-end run, `--ui` / `--rust`
/// run only one side.
pub fn run_all(root: &Path, args: &[String]) -> Vec<String> {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let mut steps: Vec<&Step> = Vec::new();
    if !has("--ui") {
        steps.extend(RUST);
    }
    if !has("--rust") {
        steps.extend(UI);
    }
    if has("--e2e") {
        steps.extend(E2E);
    }
    let mut problems: Vec<String> = steps
        .into_iter()
        .filter_map(|s| run(root, s).err())
        .collect();
    if !has("--ui") {
        eprintln_step("invariants");
        match crate::invariants::check_all(root) {
            Ok(p) => problems.extend(p.into_iter().map(|p| format!("invariants: {p}"))),
            Err(e) => problems.push(format!("invariants: could not run: {e}")),
        }
        eprintln_step("parser pins");
        match crate::supply::check_parsers(root) {
            Ok(p) => problems.extend(p.into_iter().map(|p| format!("parser pins: {p}"))),
            Err(e) => problems.push(format!("parser pins: could not run: {e}")),
        }
        eprintln_step("scan");
        match crate::scan::run(root, crate::scan::Mode::All) {
            Ok(p) => problems.extend(p.into_iter().map(|p| format!("scan: {p}"))),
            Err(e) => problems.push(format!("scan: could not run: {e}")),
        }
    }
    problems
}
