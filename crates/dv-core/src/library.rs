//! The case library: folders like a file explorer, the recycle bin, and the check for a name
//! that already appears in another case (D-023). Everything here stays on the computer.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dv_domain::{CaseSummary, Folder};
use dv_privacy::text::normalize;
use dv_vault::{Vault, VaultError};

use crate::views::NameMatch;
use crate::{Core, CoreError};

/// A deleted case waits this long in the recycle bin before it is erased for good.
pub const TRASH_DAYS: i64 = 30;
const FOLDER_NAME_MAX: usize = 60;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn folder_name(name: &str) -> Result<String, CoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::Refused("צריך לתת שם לתיקייה.".to_owned()));
    }
    if name.chars().count() > FOLDER_NAME_MAX {
        return Err(CoreError::Refused(format!(
            "שם התיקייה ארוך מדי (עד {FOLDER_NAME_MAX} תווים)."
        )));
    }
    Ok(name.to_owned())
}

/// Words of a name, normalized; short words ("בן", "די") are too common to mean anything.
fn name_words(value: &str) -> Vec<String> {
    normalize(value)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_owned)
        .collect()
}

impl Core {
    pub fn list_trash(&mut self) -> Result<Vec<CaseSummary>, CoreError> {
        Ok(self.vault_ref()?.list_trash()?)
    }

    /// Delete = into the recycle bin. The case can be restored for 30 days.
    pub fn delete_case(&mut self, case_id: &str) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.trash_case(case_id)?)
    }

    pub fn restore_case(&mut self, case_id: &str) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.restore_case(case_id)?)
    }

    /// Erase a case from the recycle bin now. Irreversible, so the password is asked again,
    /// with the same waiting time after wrong attempts as at unlock.
    pub fn purge_case(&mut self, case_id: &str, password: &str) -> Result<(), CoreError> {
        self.check_backoff()?;
        if !self
            .vault_ref()?
            .list_trash()?
            .iter()
            .any(|c| c.id == case_id)
        {
            return Err(CoreError::Refused(
                "אפשר למחוק לצמיתות רק תיק שנמצא בסל המחזור.".to_owned(),
            ));
        }
        if let Err(e) = Vault::verify_password(&self.dir, password) {
            if matches!(e, VaultError::WrongSecret) {
                self.failed_unlocks += 1;
                let wait = 2u64.saturating_pow(self.failed_unlocks.min(6)).min(60);
                self.not_before = Some(Instant::now() + Duration::from_secs(wait));
            }
            return Err(e.into());
        }
        self.failed_unlocks = 0;
        Ok(self.vault_mut()?.delete_case(case_id)?)
    }

    /// Erase what has waited in the recycle bin longer than [`TRASH_DAYS`]. Runs at unlock.
    pub(crate) fn purge_expired(&mut self) -> Result<(), CoreError> {
        let cutoff = unix_now() - TRASH_DAYS * 86_400;
        let v = self.vault_mut()?;
        for id in v.trashed_before(cutoff)? {
            v.delete_case(&id)?;
        }
        Ok(())
    }

    pub fn folders(&mut self) -> Result<Vec<Folder>, CoreError> {
        Ok(self.vault_ref()?.folders()?)
    }

    pub fn create_folder(
        &mut self,
        parent_id: Option<&str>,
        name: &str,
    ) -> Result<Folder, CoreError> {
        let name = folder_name(name)?;
        Ok(self.vault_mut()?.create_folder(parent_id, &name)?)
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) -> Result<(), CoreError> {
        let name = folder_name(name)?;
        Ok(self.vault_mut()?.rename_folder(id, &name)?)
    }

    pub fn move_folder(&mut self, id: &str, parent_id: Option<&str>) -> Result<(), CoreError> {
        match self.vault_mut()?.move_folder(id, parent_id) {
            Err(VaultError::Refused(_)) => Err(CoreError::Refused(
                "אי אפשר להעביר תיקייה לתוך עצמה או לתוך תיקייה שבתוכה.".to_owned(),
            )),
            other => Ok(other?),
        }
    }

    /// Delete a folder; what is inside moves one level up. Nothing inside is deleted.
    pub fn delete_folder(&mut self, id: &str) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.delete_folder(id)?)
    }

    pub fn move_case(&mut self, case_id: &str, folder_id: Option<&str>) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.move_case(case_id, folder_id)?)
    }

    /// Names typed for a new (or edited) case that already appear in another case: a sibling,
    /// a family seen before, or the same child twice. Compares whole words, spelling-normalized.
    pub fn find_name_matches(
        &mut self,
        case_id: Option<&str>,
        names: &[String],
    ) -> Result<Vec<NameMatch>, CoreError> {
        let typed: Vec<(String, Vec<String>)> = names
            .iter()
            .map(|n| (n.trim().to_owned(), name_words(n)))
            .filter(|(_, w)| !w.is_empty())
            .collect();
        if typed.is_empty() {
            return Ok(Vec::new());
        }
        let v = self.vault_ref()?;
        let live = v.list_cases()?;
        let trash = v.list_trash()?;
        let mut out = Vec::new();
        for (summary, trashed) in live
            .iter()
            .map(|c| (c, false))
            .chain(trash.iter().map(|c| (c, true)))
        {
            if Some(summary.id.as_str()) == case_id {
                continue;
            }
            for identity in v.identities(&summary.id)? {
                let known: Vec<String> = std::iter::once(identity.value.as_str())
                    .chain(identity.aliases.iter().map(String::as_str))
                    .flat_map(name_words)
                    .collect();
                for (name, words) in &typed {
                    if words.iter().any(|w| known.contains(w))
                        && !out.iter().any(|m: &NameMatch| {
                            m.case_id == summary.id && &m.typed == name && m.value == identity.value
                        })
                    {
                        out.push(NameMatch {
                            typed: name.clone(),
                            case_id: summary.id.clone(),
                            case_code: summary.meta.code.clone(),
                            child_name: summary.child_name.clone(),
                            role: identity.role,
                            value: identity.value.clone(),
                            trashed,
                        });
                    }
                }
            }
        }
        Ok(out)
    }
}
