//! A paragraph she was editing when the vault locked (idle, sleep, or the computer locked).
//!
//! While she edits, the page hands the text to the core, which keeps it in memory only. When
//! the vault locks, the text is written into the vault (encrypted) just before it closes, and
//! the next time she enters it is offered back once. Nothing goes to disk outside the vault:
//! the page itself is reloaded on lock and remembers nothing.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Core, CoreError};

/// Settings key of the edit kept at lock. Empty = none.
pub(crate) const UNSAVED_KEY: &str = "unsaved_edit";
/// Far longer than a paragraph; a longer text is cut, not refused.
const MAX_CHARS: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnsavedEdit {
    pub case_id: String,
    /// Where it was being written, as the page names it ("פסקה בסעיף רקע").
    pub place: String,
    pub text: String,
}

impl Core {
    /// The edit open right now, or `None` once it is saved or closed. While the vault is locked
    /// this does nothing (the page may report a closed editor as it is being replaced).
    pub fn hold_unsaved(&mut self, edit: Option<UnsavedEdit>) {
        if self.vault.is_none() {
            return;
        }
        self.unsaved = edit.filter(|e| !e.text.trim().is_empty()).map(|mut e| {
            if e.text.chars().count() > MAX_CHARS {
                e.text = e.text.chars().take(MAX_CHARS).collect();
            }
            e
        });
    }

    /// Before the vault closes: keep the open edit inside it.
    pub(crate) fn keep_unsaved(&mut self) {
        let (Some(edit), Some(v)) = (self.unsaved.take(), self.vault.as_mut()) else {
            return;
        };
        if let Ok(json) = serde_json::to_string(&edit) {
            let _ = v.set_setting(UNSAVED_KEY, &json);
        }
    }

    /// After entering: the edit kept at the last lock, offered once.
    pub fn take_unsaved(&mut self) -> Result<Option<UnsavedEdit>, CoreError> {
        let v = self.vault_mut()?;
        let Some(stored) = v.setting(UNSAVED_KEY)?.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        v.set_setting(UNSAVED_KEY, "")?;
        Ok(serde_json::from_str(&stored).ok())
    }
}
