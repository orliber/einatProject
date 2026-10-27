//! Which version of the program opened the vault last (D-104).
//!
//! The vault keeps the newest version that ever opened it, twice:
//!
//! * in the header (`app_version`), readable while the vault is locked, so an older program
//!   says "install the new version" before a password is typed or a computer key is asked
//!   for. It is a version number and nothing of hers, and it is signed with the rest of the
//!   header (checked once the vault opens);
//! * sealed inside the main database, which an older program reads after the password opens
//!   the vault and before it writes anything: the header copy can be edited while the vault is
//!   locked, this one cannot.
//!
//! A program older than either refuses the vault and writes nothing to it. A newer program
//! records itself once everything it does at opening has succeeded, so a version that could
//! not open the vault never shuts out the one before (the way back of D-057, R7).

use crate::VaultError;

/// The version of the program this is built into (the workspace version is the app's).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `major.minor.patch`, digits only (as in the signed update notice, D-033).
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().split('.');
    let mut part = || -> Option<u64> {
        let p = it.next()?;
        if p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()
    };
    let parts = (part()?, part()?, part()?);
    it.next().is_none().then_some(parts)
}

/// True when a vault that `recorded` opened must not be opened by `current`: `recorded` is a
/// newer version, or not one this program can read (fail closed).
#[must_use]
pub fn is_newer(recorded: &str, current: &str) -> bool {
    match (parse(recorded), parse(current)) {
        (Some(r), Some(c)) => r > c,
        _ => true,
    }
}

/// True when `current` should be recorded over `recorded` (none yet, or an older one).
pub(crate) fn should_record(recorded: Option<&str>, current: &str) -> bool {
    match (recorded.map(parse), parse(current)) {
        (None, Some(_)) => true,
        (Some(Some(r)), Some(c)) => c > r,
        _ => false,
    }
}

/// The refusal for a vault `recorded` opened, when that is newer than `current`.
pub(crate) fn refuse_newer(recorded: Option<&str>, current: &str) -> Result<(), VaultError> {
    match recorded {
        Some(r) if is_newer(r, current) => Err(VaultError::NewerVersion(shown(r))),
        _ => Ok(()),
    }
}

/// The version as it may be shown to her: only a version this program can read (the header
/// is not authenticated while the vault is locked). Empty when it is not one.
fn shown(recorded: &str) -> String {
    parse(recorded).map_or_else(String::new, |(a, b, c)| format!("{a}.{b}.{c}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_version_is_one_the_check_reads() {
        assert!(parse(APP_VERSION).is_some(), "{APP_VERSION}");
        assert!(!is_newer(APP_VERSION, APP_VERSION));
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.8.2", "0.8.1"));
        assert!(!is_newer("0.8.1", "0.8.1"));
        assert!(!is_newer("0.7.0", "0.8.1"));
        assert!(!is_newer("0.9.0", "0.10.0"));
    }

    #[test]
    fn a_version_it_cannot_read_counts_as_newer() {
        for odd in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.x",
            "1.2.3-beta",
            " .1.2",
            "-1.0.0",
            "1.2.+3",
            "9999999999.0.0",
        ] {
            assert!(is_newer(odd, "0.8.1"), "{odd:?}");
            assert!(refuse_newer(Some(odd), "0.8.1").is_err(), "{odd:?}");
        }
    }

    #[test]
    fn only_a_newer_version_is_recorded() {
        assert!(should_record(None, "0.9.0"));
        assert!(should_record(Some("0.8.1"), "0.9.0"));
        assert!(!should_record(Some("0.9.0"), "0.9.0"));
        assert!(!should_record(Some("0.10.0"), "0.9.0"));
        assert!(!should_record(Some("junk"), "0.9.0"));
    }

    #[test]
    fn the_refusal_names_only_a_readable_version() {
        match refuse_newer(Some("0.12.3"), "0.9.0") {
            Err(VaultError::NewerVersion(v)) => assert_eq!(v, "0.12.3"),
            other => panic!("{other:?}"),
        }
        match refuse_newer(Some("<b>9</b>"), "0.9.0") {
            Err(VaultError::NewerVersion(v)) => assert_eq!(v, ""),
            other => panic!("{other:?}"),
        }
        assert!(refuse_newer(None, "0.9.0").is_ok());
        assert!(refuse_newer(Some("0.9.0"), "0.9.0").is_ok());
    }
}
