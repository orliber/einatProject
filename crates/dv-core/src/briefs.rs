//! What she asks before a draft (D-140): the box above every drafting request keeps her last
//! few wishes, and a wish she chose to keep for a section. Both live in the vault, encrypted
//! like everything else, and go out only inside a request, through the filter and the gate.
//! A kept wish is not the section's guide: the guide's lines are checked paragraph by
//! paragraph, a wish ("short", "start from the kindergarten") is not.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Core, CoreError};

/// Settings key of the last wishes, newest first (JSON list).
const RECENT_KEY: &str = "briefs/recent";
/// Settings key of the wishes kept per section (JSON map).
const SECTIONS_KEY: &str = "briefs/sections";
/// How many recent wishes are offered again.
const RECENT_MAX: usize = 5;
/// The longest wish (characters).
const BRIEF_MAX: usize = 1_000;
/// The longest section key accepted (the templates' keys are short words).
const KEY_MAX: usize = 64;

/// Her drafting wishes: the recent ones, and the ones kept per section.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftBriefs {
    /// Newest first, at most five, no two alike.
    pub recent: Vec<String>,
    /// Section key → the wish sent with every draft of that section.
    pub sections: BTreeMap<String, String>,
}

/// A wish as she typed it: one kind of line break, trimmed, within the limit.
fn clean(text: &str) -> Result<String, CoreError> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim().to_owned();
    if text.chars().count() > BRIEF_MAX {
        return Err(CoreError::Refused(format!(
            "ההנחיה ארוכה מדי: עד {BRIEF_MAX} תווים."
        )));
    }
    Ok(text)
}

/// A stored list or map; anything unreadable is treated as empty, never as an error that
/// stops her from drafting.
fn parse<T: Default + for<'de> Deserialize<'de>>(stored: Option<String>) -> T {
    stored
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

impl Core {
    /// Her recent wishes and the ones kept per section.
    pub fn draft_briefs(&mut self) -> Result<DraftBriefs, CoreError> {
        let v = self.vault_ref()?;
        let mut recent: Vec<String> = parse(v.setting(RECENT_KEY)?);
        recent.truncate(RECENT_MAX);
        Ok(DraftBriefs {
            recent,
            sections: parse(v.setting(SECTIONS_KEY)?),
        })
    }

    /// A wish she sent for `section_key`: it becomes the newest recent one, and is kept for
    /// the section when `keep` (an empty wish with `keep` off forgets the section's).
    pub fn keep_draft_brief(
        &mut self,
        section_key: &str,
        text: &str,
        keep: bool,
    ) -> Result<DraftBriefs, CoreError> {
        let key = section_key.trim();
        if key.is_empty() || key.chars().count() > KEY_MAX || !crate::guides::known_section(key) {
            return Err(CoreError::NotFound("הסעיף".to_owned()));
        }
        let text = clean(text)?;
        let mut briefs = self.draft_briefs()?;
        if !text.is_empty() {
            briefs.recent.retain(|r| r != &text);
            briefs.recent.insert(0, text.clone());
            briefs.recent.truncate(RECENT_MAX);
        }
        if keep && !text.is_empty() {
            briefs.sections.insert(key.to_owned(), text);
        } else {
            briefs.sections.remove(key);
        }
        let recent = serde_json::to_string(&briefs.recent)
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let sections = serde_json::to_string(&briefs.sections)
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let v = self.vault_mut()?;
        v.set_setting(RECENT_KEY, &recent)?;
        v.set_setting(SECTIONS_KEY, &sections)?;
        Ok(briefs)
    }
}
