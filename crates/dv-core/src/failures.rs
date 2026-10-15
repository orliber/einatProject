//! Which cases got stuck: a send for the case that failed (network, key, an answer that did
//! not pass the checks) and has not succeeded since. The case list shows it as "נכשל".
//! Only case ids and times are kept, never what failed or why.

use super::*;

const FAILED_KEY: &str = "failed_sends";

impl Core {
    pub(crate) fn failed_sends(&mut self) -> Result<HashMap<String, i64>, CoreError> {
        Ok(self
            .vault_ref()?
            .setting(FAILED_KEY)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }

    fn set_failed_sends(&mut self, failed: &HashMap<String, i64>) -> Result<(), CoreError> {
        let json = serde_json::to_string(failed).map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(self.vault_mut()?.set_state(FAILED_KEY, &json)?)
    }

    /// Note how a send for a case ended. Never fails the send itself: the answer (or the
    /// error she needs to see) matters more than the mark.
    pub(crate) fn note_send_outcome(&mut self, case_id: &str, ok: bool) {
        let _ = self.mark_case(case_id, (!ok).then(dates::unix_now));
    }

    /// Forget a case that was erased from the vault.
    pub(crate) fn forget_failure(&mut self, case_id: &str) -> Result<(), CoreError> {
        self.mark_case(case_id, None)
    }

    fn mark_case(&mut self, case_id: &str, failed_at: Option<i64>) -> Result<(), CoreError> {
        if case_id.is_empty() {
            return Ok(());
        }
        let mut failed = self.failed_sends()?;
        let changed = match failed_at {
            Some(at) => failed.insert(case_id.to_owned(), at) != Some(at),
            None => failed.remove(case_id).is_some(),
        };
        if changed {
            self.set_failed_sends(&failed)?;
        }
        Ok(())
    }

    /// Put the failure marks on the rows of a case list.
    pub(crate) fn with_failures(
        &mut self,
        mut cases: Vec<CaseSummary>,
    ) -> Result<Vec<CaseSummary>, CoreError> {
        let failed = self.failed_sends()?;
        for c in &mut cases {
            c.failed_at = failed.get(&c.id).copied();
        }
        Ok(cases)
    }
}

impl PendingKind {
    /// The case a request belongs to, when its failure should mark the case.
    pub(crate) fn failing_case(&self) -> Option<&str> {
        match self {
            PendingKind::Section { case_id, .. } | PendingKind::Sort { case_id, .. } => {
                Some(case_id)
            }
            _ => None,
        }
    }
}
