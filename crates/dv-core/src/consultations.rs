//! Saved consultations, like the conversation list in Claude: each one sealed in the vault
//! (with its case's key, so it is erased with the case), continued later with the same turns
//! that were sent before (filtered), and deletable one by one.

use serde::{Deserialize, Serialize};

use crate::dates::unix_now;
use crate::views::{ConsultTurnView, ConsultationSummary, ConsultationView};
use crate::{Core, CoreError};

/// One turn as kept: what was sent (tagged) and what Einat saw (with names).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredTurn {
    pub role: String,
    pub tagged: String,
    pub shown: String,
    #[serde(default)]
    pub hidden: Vec<String>,
    #[serde(default)]
    pub demo: bool,
    pub at: i64,
}

/// A question and its answer, to add to a conversation.
pub(crate) struct Exchange {
    pub question_tagged: String,
    pub question_shown: String,
    pub hidden: Vec<String>,
    pub answer_tagged: String,
    pub answer_shown: String,
    pub demo: bool,
}

/// Longest title in the list.
const TITLE_CHARS: usize = 60;

fn title_of(turns: &[StoredTurn]) -> String {
    let first = turns
        .iter()
        .find(|t| t.role == "user")
        .map_or("שיחה", |t| t.shown.as_str());
    let line = first.lines().next().unwrap_or("").trim();
    if line.chars().count() > TITLE_CHARS {
        format!("{}…", line.chars().take(TITLE_CHARS).collect::<String>())
    } else {
        line.to_owned()
    }
}

fn parse(json: &str) -> Result<Vec<StoredTurn>, CoreError> {
    serde_json::from_str(json).map_err(|e| CoreError::Internal(e.to_string()))
}

impl Core {
    /// A saved conversation's case and turns.
    pub(crate) fn stored_consultation(
        &mut self,
        id: &str,
    ) -> Result<(Option<String>, Vec<StoredTurn>), CoreError> {
        let (case_id, json) = self
            .vault_ref()?
            .consultation(id)?
            .ok_or_else(|| CoreError::NotFound("השיחה".to_owned()))?;
        Ok((case_id, parse(&json)?))
    }

    /// Add a question and its answer to a conversation (a new one when `id` is None).
    pub(crate) fn keep_consultation(
        &mut self,
        id: Option<&str>,
        case_id: Option<&str>,
        exchange: Exchange,
    ) -> Result<String, CoreError> {
        // The answer already came back (and was paid for): if the conversation was deleted, or
        // its case moved to the recycle bin meanwhile, it starts a new one instead of being lost.
        let (id, mut turns) = match id.map(|i| (i, self.stored_consultation(i))) {
            Some((i, Ok((_, turns)))) => (Some(i), turns),
            Some((_, Err(CoreError::NotFound(_)))) | None => (None, Vec::new()),
            Some((_, Err(e))) => return Err(e),
        };
        let at = unix_now();
        turns.push(StoredTurn {
            role: "user".to_owned(),
            tagged: exchange.question_tagged,
            shown: exchange.question_shown,
            hidden: exchange.hidden,
            demo: exchange.demo,
            at,
        });
        turns.push(StoredTurn {
            role: "assistant".to_owned(),
            tagged: exchange.answer_tagged,
            shown: exchange.answer_shown,
            hidden: Vec::new(),
            demo: exchange.demo,
            at,
        });
        let json = serde_json::to_string(&turns).map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(self.vault_mut()?.save_consultation(id, case_id, &json)?)
    }

    /// Every saved conversation, newest first.
    pub fn consultations(&mut self) -> Result<Vec<ConsultationSummary>, CoreError> {
        let v = self.vault_ref()?;
        let cases = v.list_cases()?;
        let mut out = Vec::new();
        for row in v.consultations()? {
            let turns = parse(&row.turns_json)?;
            let (id, case_id, updated_at) = (row.id, row.case_id, row.updated_at);
            let case_label = case_id.as_ref().and_then(|c| {
                cases
                    .iter()
                    .find(|k| &k.id == c)
                    .map(|k| match &k.child_name {
                        Some(name) => format!("{name} · {}", k.meta.code),
                        None => k.meta.code.clone(),
                    })
            });
            out.push(ConsultationSummary {
                id,
                case_id,
                case_label,
                title: title_of(&turns),
                updated_at,
                turns: u32::try_from(turns.len()).unwrap_or(u32::MAX),
            });
        }
        Ok(out)
    }

    pub fn consultation(&mut self, id: &str) -> Result<ConsultationView, CoreError> {
        let (case_id, turns) = self.stored_consultation(id)?;
        Ok(ConsultationView {
            id: id.to_owned(),
            case_id,
            turns: turns
                .into_iter()
                .map(|t| ConsultTurnView {
                    role: t.role,
                    text: t.shown,
                    hidden: t.hidden,
                    demo: t.demo,
                    at: t.at,
                })
                .collect(),
        })
    }

    pub fn delete_consultation(&mut self, id: &str) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.delete_consultation(id)?)
    }
}
