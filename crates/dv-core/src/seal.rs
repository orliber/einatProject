//! A rollback seal (S6): the newest log entry number this computer has seen, kept in a small
//! file OUTSIDE the vault folder. The log inside the vault checks itself, but someone who puts
//! back an older copy of the whole vault folder also puts back an older log. The seal stays
//! behind, so an older vault shows up as "fewer entries than this computer already saw".
//! A deliberate restore from a backup looks the same, so the warning says so, and the seal is
//! then moved to the vault's number: it warns once, never twice for the same thing.

use std::path::{Path, PathBuf};

/// Where the seal lives: the user's local data folder, one file per vault folder.
pub(crate) fn default_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_STATE_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("DiagnosticVault"))
}

/// A short stable name for a vault folder (FNV-1a), so two vaults never share a seal.
fn file_name(vault_dir: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in vault_dir.to_string_lossy().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("seal-{h:016x}.txt")
}

/// Compare the vault's newest entry number with the seal, and move the seal forward (or to the
/// vault's number after a warning). Returns the warning text, if any. A seal that cannot be
/// read or written is not an error: this check only adds protection.
pub(crate) fn check(seal_dir: &Path, vault_dir: &Path, vault_seq: i64) -> Option<String> {
    let path = seal_dir.join(file_name(vault_dir));
    let seen: Option<i64> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok());
    let warning = match seen {
        Some(seen) if vault_seq < seen => Some(format!(
            "הכספת הזו ישנה מהעותק האחרון שהמחשב ראה ({vault_seq} רשומות ביומן במקום {seen}). אם שחזרת מגיבוי, זה צפוי. אחרת, מישהו החזיר גרסה ישנה."
        )),
        _ => None,
    };
    if warning.is_some() || seen.is_none_or(|s| vault_seq > s) {
        let _ = std::fs::create_dir_all(seal_dir);
        let _ = std::fs::write(&path, vault_seq.to_string());
    }
    warning
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_vault_warns_once_and_a_newer_one_moves_the_seal() {
        let seal = tempfile::tempdir().unwrap();
        let vault = Path::new("/some/vault");
        assert!(check(seal.path(), vault, 5).is_none());
        assert!(check(seal.path(), vault, 9).is_none());
        let w = check(seal.path(), vault, 4).unwrap();
        assert!(w.contains("4") && w.contains("9"));
        // Warned once; the seal now follows the restored vault.
        assert!(check(seal.path(), vault, 4).is_none());
        // Another vault folder has its own seal.
        assert!(check(seal.path(), Path::new("/other"), 1).is_none());
    }
}
