//! `cargo xtask speed`: the speed checkers, in one command.
//!
//! * The core's main actions on a vault of 300 invented cases, each against its time budget
//!   (`crates/dv-core/tests/speed.rs`), in a release build, with the table printed.
//! * The size of the built page (`apps/desktop/dist/assets`), when it was built
//!   (`pnpm build`): the scripts and styles the window loads, against a budget.
//!
//! Local only: nothing is sent anywhere.

use std::io::Write;
use std::path::Path;
use std::process::Command;

/// A line of the report, on the same stream as every other xtask report.
macro_rules! say {
    ($($arg:tt)*) => {{
        let _ = writeln!(std::io::stderr().lock(), $($arg)*);
    }};
}

/// Scripts the window loads, all files together. The page is local, so this is about the
/// time the webview takes to parse it at start, not a download.
const SCRIPT_BUDGET_BYTES: u64 = 1_200_000;
/// Stylesheets, all files together.
const STYLE_BUDGET_BYTES: u64 = 200_000;

pub fn run(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let status = Command::new("cargo")
        .current_dir(root)
        .args([
            "test",
            "--release",
            "-p",
            "dv-core",
            "--test",
            "speed",
            "--",
            "--ignored",
            "--nocapture",
        ])
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(_) => {
            problems.push("an action is over its time budget (see the table above)".to_owned())
        }
        Err(e) => problems.push(format!("could not run the speed test: {e}")),
    }
    match bundle(&root.join("apps/desktop/dist/assets")) {
        Some((scripts, styles)) => {
            say!("\npage size (apps/desktop/dist/assets)");
            say!("{:<12} {:>12} {:>12}", "", "bytes", "budget");
            say!("{:<12} {scripts:>12} {SCRIPT_BUDGET_BYTES:>12}", "scripts");
            say!("{:<12} {styles:>12} {STYLE_BUDGET_BYTES:>12}", "styles");
            problems.extend(over_budget(scripts, styles));
        }
        None => say!("\npage size: not built (run `pnpm build` in apps/desktop to include it)"),
    }
    problems
}

/// What is over its budget, in words.
pub fn over_budget(scripts: u64, styles: u64) -> Vec<String> {
    let mut problems = Vec::new();
    if scripts > SCRIPT_BUDGET_BYTES {
        problems.push(format!(
            "the page's scripts are {scripts} bytes, over the budget of {SCRIPT_BUDGET_BYTES}"
        ));
    }
    if styles > STYLE_BUDGET_BYTES {
        problems.push(format!(
            "the page's styles are {styles} bytes, over the budget of {STYLE_BUDGET_BYTES}"
        ));
    }
    problems
}

/// The bytes of the scripts and of the styles in the built page; `None` when it is not built.
fn bundle(assets: &Path) -> Option<(u64, u64)> {
    let entries = std::fs::read_dir(assets).ok()?;
    let (mut scripts, mut styles) = (0, 0);
    for entry in entries.flatten() {
        let path = entry.path();
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        match path.extension().and_then(|e| e.to_str()) {
            Some("js") => scripts += len,
            Some("css") => styles += len,
            _ => {}
        }
    }
    Some((scripts, styles))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_inside_its_budgets_passes_and_one_over_is_named() {
        assert!(over_budget(SCRIPT_BUDGET_BYTES, STYLE_BUDGET_BYTES).is_empty());
        let over = over_budget(SCRIPT_BUDGET_BYTES + 1, STYLE_BUDGET_BYTES + 1);
        assert_eq!(over.len(), 2);
        assert!(over[0].contains("scripts") && over[1].contains("styles"));
    }
}
