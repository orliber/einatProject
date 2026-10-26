//! Text she was typing when the vault locked (idle, sleep, or the computer locked): a paragraph,
//! a session note or other material, or a question to the AI.
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
/// Editors open at once (several cases, a paragraph and a note). More than this keeps the latest.
pub(crate) const MAX_EDITS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnsavedEdit {
    pub case_id: String,
    /// Where it was being written, as the page names it ("פסקה בסעיף רקע").
    pub place: String,
    /// The case view to open for it ("report", "materials", "consult"). Empty in edits kept by
    /// earlier versions, which were always paragraphs.
    #[serde(default)]
    pub view: String,
    pub text: String,
}

impl Core {
    /// The editors open right now, the latest last; empty once all are saved or closed. While
    /// the vault is locked this does nothing (the page may report a closed editor as it is
    /// being replaced).
    pub fn hold_unsaved(&mut self, edits: Vec<UnsavedEdit>) {
        if self.vault.is_none() {
            return;
        }
        let mut kept: Vec<UnsavedEdit> = edits
            .into_iter()
            .filter(|e| !e.text.trim().is_empty())
            .map(|mut e| {
                if e.text.chars().count() > MAX_CHARS {
                    e.text = e.text.chars().take(MAX_CHARS).collect();
                }
                e
            })
            .collect();
        if kept.len() > MAX_EDITS {
            kept.drain(..kept.len() - MAX_EDITS);
        }
        self.unsaved = kept;
    }

    /// Before the vault closes: keep every open edit inside it.
    pub(crate) fn keep_unsaved(&mut self) {
        let edits = std::mem::take(&mut self.unsaved);
        let Some(v) = self.vault.as_mut() else {
            return;
        };
        if edits.is_empty() {
            return;
        }
        if let Ok(json) = serde_json::to_string(&edits) {
            let _ = v.set_state(UNSAVED_KEY, &json);
        }
    }

    /// After entering: the edits kept at the last lock, offered once. A vault kept by an
    /// earlier version holds one edit, not a list.
    pub fn take_unsaved(&mut self) -> Result<Vec<UnsavedEdit>, CoreError> {
        let v = self.vault_mut()?;
        let Some(stored) = v.setting(UNSAVED_KEY)?.filter(|s| !s.is_empty()) else {
            return Ok(Vec::new());
        };
        v.set_state(UNSAVED_KEY, "")?;
        if let Ok(list) = serde_json::from_str::<Vec<UnsavedEdit>>(&stored) {
            return Ok(list);
        }
        Ok(serde_json::from_str::<UnsavedEdit>(&stored)
            .into_iter()
            .collect())
    }
}
