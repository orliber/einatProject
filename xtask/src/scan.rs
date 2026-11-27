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
    // The id of a calendar the app makes in her Google account (D-202) has this shape. It
    // is never a person's address.
    "group.calendar.google.com",
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
    // Photos and scans of a page are importable (OCR), so they may carry patient data too.
    "png",
    "webp",
    "tif",
    "tiff",
    "mp3",
    "m4a",
    "wav",
    "mp4",
];

/// Paths where an otherwise-forbidden extension is legitimate.
fn extension_allowed(path: &str, ext: &str) -> bool {
    (ext == "docx" && path.starts_with("templates/"))
        || (ext == "csv" && path.starts_with("knowledge/"))
        || (ext == "png"
            && (path.starts_with("apps/desktop/src-tauri/icons/")
                || path.starts_with("docs/design/")))
        // The help's pictures, taken from the preview's invented cases by
        // apps/desktop/e2e/help-shots.spec.ts (D-206). WebP only, and only there.
        || (ext == "webp" && path.starts_with("apps/desktop/src/help/shots/"))
        // Scans of an invented letter for the OCR tests (D-048).
        || (matches!(ext, "png" | "tif") && path.starts_with("crates/dv-ingest/tests/fixtures/"))
}

/// Generated or vendored files that are scanned for size only.
fn content_exempt(path: &str) -> bool {
    path.ends_with("Cargo.lock")
        || path.ends_with("pnpm-lock.yaml")
        || path.starts_with("apps/desktop/src-tauri/icons/")
        || path.starts_with("docs/design/")
        || path.starts_with("apps/desktop/src/help/shots/")
}

/// The ID numbers a text may hold: every run of 9 digits, every run of 8 (an ID that starts
/// with 0 is often written without it), and 7 or 8 digits with the check digit set apart by
/// one '-' or space ("NNNNNNNN-C"). Runs are found without consuming their neighbours, so a
/// number right after another one is tested too. Each comes back padded to nine digits.
fn id_candidates(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    // Maximal runs of ASCII digits: (start, end).
    let mut runs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i).is_some_and(u8::is_ascii_digit) {
            let start = i;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }
    let digits = |(a, b): (usize, usize)| text.get(a..b).unwrap_or_default();
    // An 8-digit run inside a version, a hash or a name ("v1.12345678", "a12345678b",
    // "tool_20250305") is not one; a 9-digit run is tested wherever it is, as before.
    let free = |(a, b): (usize, usize)| {
        let ok = |c: Option<&u8>| {
            c.is_none_or(|c| !c.is_ascii_alphanumeric() && !matches!(c, b'.' | b'_'))
        };
        ok(a.checked_sub(1).and_then(|k| bytes.get(k))) && ok(bytes.get(b))
    };
    let pad = |d: &str| format!("{d:0>9}");
    let mut out = Vec::new();
    for (k, &run) in runs.iter().enumerate() {
        let d = digits(run);
        match d.len() {
            9 => out.push(d.to_owned()),
            8 if free(run) => out.push(pad(d)),
            _ => {}
        }
        // "NNNNNNNN-C" / "NNNNNNNN C": a check digit set apart.
        if let Some(&next) = runs.get(k + 1) {
            let gap = text.get(run.1..next.0).unwrap_or_default();
            let joined = format!("{d}{}", digits(next));
            if matches!(gap, "-" | " ")
                && matches!(d.len(), 7 | 8)
                && next.1 - next.0 == 1
                && free((run.0, next.1))
            {
                out.push(pad(&joined));
            }
        }
    }
    out
}
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
    for id in id_candidates(text) {
        if israeli_id::is_valid(&id) && !FAKE_IDS.contains(&id.as_str()) {
            problems.push(format!(
                "{path}: valid Israeli ID number {}…",
                id.get(..3).unwrap_or_default()
            ));
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
        match std::str::from_utf8(bytes) {
            Ok(text) => problems.extend(scan_text(path, text)),
            // A picture or template that is allowed where it is (the OCR fixtures): not text.
            Err(_) if FORBIDDEN_EXTENSIONS.contains(&ext.as_str()) => {}
            Err(_) => match utf16(bytes) {
                // Saved as "Unicode" by Notepad: read as the text it is.
                Some(text) => problems.extend(scan_text(path, &text)),
                // Fail closed: anything else (ANSI Hebrew, an unknown binary) is scanned for
                // what it can hold in ASCII (digits and '@' are ASCII in Windows-1255) and is
                // refused as it is.
                None => {
                    problems.extend(scan_text(path, &String::from_utf8_lossy(bytes)));
                    problems.push(format!(
                        "{path}: not UTF-8 text (save it as UTF-8, or it is a binary file)"
                    ));
                }
            },
        }
    }
    problems
}

/// `bytes` as UTF-16 text with a byte order mark, or `None`.
fn utf16(bytes: &[u8]) -> Option<String> {
    let (le, body) = match bytes {
        [0xFF, 0xFE, rest @ ..] => (true, rest),
        [0xFE, 0xFF, rest @ ..] => (false, rest),
        _ => return None,
    };
    if body.len() % 2 != 0 {
        return None;
    }
    let units = body.chunks_exact(2).map(|p| match (le, p) {
        (true, [a, b]) => u16::from_le_bytes([*a, *b]),
        (false, [a, b]) => u16::from_be_bytes([*a, *b]),
        _ => 0,
    });
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .ok()
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

    /// DOMAIN-9: an ID written without its leading zero, or with the check digit set apart.
    #[test]
    fn blocks_ids_with_eight_digits_or_a_separate_check_digit() {
        // A valid ID that starts with 0, written as 8 digits or with its check digit apart.
        assert!(israeli_id::is_valid(&join(&["0123", "45674"])));
        assert_eq!(scan_text("a.md", &join(&["ת.ז. 1234", "5674"])).len(), 1);
        assert_eq!(scan_text("a.md", &join(&["ת.ז. 1234", "567-4"])).len(), 1);
        assert_eq!(scan_text("a.md", &join(&["ת.ז. 1234", "567 4"])).len(), 1);
        assert_eq!(scan_text("a.md", &join(&["id: 12345", "678-2"])).len(), 1);
        // A date, a version and a hash are not.
        assert!(scan_text("a.md", "2026-09-28, v1.94.1, 2026.11.27").is_empty());
        assert!(scan_text("a.md", &join(&["sha a1234", "5674b v0.1234", "5674"])).is_empty());
        assert!(scan_text("a.rs", &join(&["\"web_search_2025", "0305\""])).is_empty());
    }

    /// DOMAIN-10: a valid ID right after another 9-digit number is still tested.
    #[test]
    fn an_id_after_another_number_is_still_tested() {
        let real = join(&["12345", "6782"]);
        assert_eq!(scan_text("a.md", &format!("id 000000018,{real}")).len(), 1);
        assert_eq!(scan_text("a.md", &format!("000000018\n{real}")).len(), 1);
        assert_eq!(scan_text("a.md", &format!("{real}\n{real}")).len(), 2);
    }

    /// DOMAIN-8: a file that is not UTF-8 is read, and refused unless it is a picture that is
    /// allowed where it is.
    #[test]
    fn a_file_that_is_not_utf8_is_not_skipped() {
        let phone = join(&["טלפון 054-", "1234567"]);
        let mut utf16le = vec![0xFF, 0xFE];
        utf16le.extend(phone.encode_utf16().flat_map(u16::to_le_bytes));
        let problems = scan_file("notes.txt", &utf16le);
        assert_eq!(problems, vec!["notes.txt: Israeli phone number".to_owned()]);
        let mut utf16be = vec![0xFE, 0xFF];
        utf16be.extend(phone.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(scan_file("notes.txt", &utf16be).len(), 1);
        // Windows-1255: Hebrew letters are single bytes above 0x7F, digits are ASCII.
        let mut ansi = vec![0xE8, 0xEC, 0xF4, 0xE5, 0xEF, b' '];
        ansi.extend_from_slice(join(&["054-", "1234567"]).as_bytes());
        let problems = scan_file("notes.txt", &ansi);
        assert!(
            problems.contains(&"notes.txt: Israeli phone number".to_owned())
                && problems.iter().any(|p| p.contains("not UTF-8")),
            "{problems:?}"
        );
        // An unknown binary with a text extension is refused too.
        assert_eq!(scan_file("data.json", &[0x00, 0xC3, 0x28, 0xFF]).len(), 1);
        // The OCR fixtures and the icons stay allowed.
        assert!(scan_file("crates/dv-ingest/tests/fixtures/scan.png", &[0x89, 0xFF]).is_empty());
        assert!(scan_file("apps/desktop/src-tauri/icons/icon.ico", &[0x00, 0xFF]).is_empty());
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
        // D-206: a picture is allowed only as the help's own WebP.
        assert_eq!(scan_file("scan.webp", b"x").len(), 1);
        assert!(scan_file("apps/desktop/src/help/shots/cases.webp", b"x").is_empty());
        assert_eq!(
            scan_file("apps/desktop/src/help/shots/cases.png", b"x").len(),
            1
        );
        assert_eq!(
            scan_file("big.txt", &vec![b'a'; MAX_FILE_BYTES + 1]).len(),
            1
        );
    }
}
