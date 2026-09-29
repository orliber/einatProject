//! Keeps patient data, archives and opaque binaries out of the repository.
//!
//! Runs as the pre-commit hook (`--staged`, reading the staged blobs) and in CI (`--all`).

use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

use crate::israeli_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Staged,
    All,
}

/// Known-fake ID numbers used by the fixtures (valid check digit, obviously synthetic).
const FAKE_IDS: &[&str] = &["000000018", "000000026"];

/// Top-level "domains" that are really file extensions (`icon@2x.png`).
const FILE_EXTENSION_TLDS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "ico", "icns", "webp", "json", "js", "ts", "tsx", "css",
    "rs", "md", "toml", "yaml", "yml", "html", "txt", "woff", "woff2",
];

/// Mail domains that may appear in the repository.
const ALLOWED_MAIL_DOMAINS: &[&str] = &[
    "example.com",
    "example.org",
    "anthropic.com",
    "users.noreply.github.com",
];

const MAX_FILE_BYTES: usize = 1_000_000;

const FORBIDDEN_EXTENSIONS: &[&str] = &[
    "zip",
    "7z",
    "rar",
    "tar",
    "gz",
    "tgz",
    "bz2",
    "xz",
    "db",
    "sqlite",
    "sqlite3",
    "db-wal",
    "db-journal",
    "vaultbak",
    "key",
    "pem",
    "p12",
    "pfx",
    "doc",
    "docx",
    "rtf",
    "odt",
    "pdf",
    "xls",
    "xlsx",
    "csv",
    "msg",
    "eml",
    "jpg",
    "jpeg",
    "heic",
    "mp3",
    "m4a",
    "wav",
    "mp4",
];

/// Paths where an otherwise-forbidden extension is legitimate.
fn extension_allowed(path: &str, ext: &str) -> bool {
    (ext == "docx" && path.starts_with("templates/"))
        || (ext == "csv" && path.starts_with("knowledge/"))
}

/// Generated or vendored files that are scanned for size only.
fn content_exempt(path: &str) -> bool {
    path.ends_with("Cargo.lock")
        || path.ends_with("pnpm-lock.yaml")
        || path.starts_with("apps/desktop/src-tauri/icons/")
        || path.starts_with("docs/design/")
}

static ID_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?:^|[^\d])(\d{9})(?:[^\d]|$)").ok());
static PHONE_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?:\+972[-\s]?|(?:^|[^\d])0)(?:5\d|[2-4]|[89]|7\d)[-\s]?\d{3}[-\s]?\d{4}(?:[^\d]|$)",
    )
    .ok()
});
static MAIL_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@([A-Za-z0-9.-]+\.[A-Za-z]{2,})").ok());

/// Problems found in one file's text.
pub fn scan_text(path: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(re) = ID_RE.as_ref() {
        for cap in re.captures_iter(text) {
            let id = &cap[1];
            if israeli_id::is_valid(id) && !FAKE_IDS.contains(&id) {
                problems.push(format!("{path}: valid Israeli ID number {}…", &id[..3]));
            }
        }
    }
    if let Some(re) = PHONE_RE.as_ref() {
        if re.is_match(text) {
            problems.push(format!("{path}: Israeli phone number"));
        }
    }
    if let Some(re) = MAIL_RE.as_ref() {
        for cap in re.captures_iter(text) {
            let domain = cap[1].to_ascii_lowercase();
            let tld = domain.rsplit('.').next().unwrap_or_default();
            if FILE_EXTENSION_TLDS.contains(&tld) {
                continue;
            }
            if !ALLOWED_MAIL_DOMAINS
                .iter()
                .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
            {
                problems.push(format!("{path}: e-mail address at {domain}"));
            }
        }
    }
    problems
}

/// Problems with the file itself (type and size), independent of its text.
pub fn scan_file(path: &str, bytes: &[u8]) -> Vec<String> {
    let mut problems = Vec::new();
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if FORBIDDEN_EXTENSIONS.contains(&ext.as_str()) && !extension_allowed(path, &ext) {
        problems.push(format!(
            "{path}: .{ext} files may carry patient data and are not allowed"
        ));
    }
    if bytes.len() > MAX_FILE_BYTES && !content_exempt(path) {
        problems.push(format!("{path}: larger than {MAX_FILE_BYTES} bytes"));
    }
    if path.starts_with("vault/") || path.contains("/vault/") || path.contains("real_data") {
        problems.push(format!("{path}: path is reserved for real data"));
    }
    if !content_exempt(path) {
        if let Ok(text) = std::str::from_utf8(bytes) {
            problems.extend(scan_text(path, text));
        }
    }
    problems
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(out.stdout)
}

pub fn run(root: &Path, mode: Mode) -> Result<Vec<String>, String> {
    let listing = match mode {
        Mode::Staged => git(
            root,
            &[
                "diff",
                "--cached",
                "--name-only",
                "--diff-filter=ACMR",
                "-z",
            ],
        )?,
        Mode::All => git(root, &["ls-files", "-z"])?,
    };
    let mut problems = Vec::new();
    for raw in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8_lossy(raw).into_owned();
        let bytes = match mode {
            Mode::Staged => git(root, &["show", &format!(":{path}")])?,
            Mode::All => match std::fs::read(root.join(&path)) {
                Ok(b) => b,
                Err(_) => continue, // deleted in the working tree
            },
        };
        problems.extend(scan_file(&path, &bytes));
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test data is assembled at runtime so this file does not trip the scanner itself.
    fn join(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn blocks_real_looking_ids_but_allows_known_fakes() {
        assert!(scan_text("a.md", "ת.ז. 000000018").is_empty());
        // Valid check digit.
        assert_eq!(scan_text("a.md", &join(&["id: 12345", "6782"])).len(), 1);
        assert!(scan_text("a.md", &join(&["id: 12345", "6789"])).is_empty());
        assert!(scan_text("a.md", &join(&["hash 1234567", "890123"])).is_empty());
    }

    #[test]
    fn blocks_israeli_phone_numbers() {
        for phone in [
            join(&["054-", "1234567"]),
            join(&["054", "1234567"]),
            join(&["+972-54-", "123-4567"]),
            join(&["03-", "1234567"]),
        ] {
            assert_eq!(
                scan_text("a.md", &format!("טלפון {phone}")).len(),
                1,
                "{phone}"
            );
        }
        assert!(scan_text("a.md", "version 1.94.1 and 2026-09-28").is_empty());
    }

    #[test]
    fn blocks_personal_mail_but_allows_project_domains() {
        assert_eq!(
            scan_text("a.md", &join(&["someone", "@", "gmail.com"])).len(),
            1
        );
        assert!(scan_text("a.md", "noreply@anthropic.com user@example.com").is_empty());
        assert!(scan_text("a.json", "\"icons/128x128@2x.png\"").is_empty());
    }

    #[test]
    fn blocks_archives_documents_and_vault_paths() {
        assert_eq!(scan_file("report.docx", b"x").len(), 1);
        assert!(scan_file("templates/report.docx", b"x").is_empty());
        assert_eq!(scan_file("kit.zip", b"x").len(), 1);
        assert_eq!(scan_file("vault/main.db", b"x").len(), 2);
        assert_eq!(
            scan_file("big.txt", &vec![b'a'; MAX_FILE_BYTES + 1]).len(),
            1
        );
    }
}
