//! Settings: API keys, the chosen AI and its speed, lock time, the practitioner.

use super::*;

impl Core {
    /// Save (or, when empty, delete) the API key of one company (`anthropic` | `openai` |
    /// `gemini` | `mistral`). ChatGPT, Gemini and Mistral keys are taken only after she confirmed she read what
    /// that company keeps (D-040): their standard API terms are not Zero Data Retention.
    pub fn set_api_key(
        &mut self,
        provider: &str,
        key: &str,
        retention_ack: bool,
    ) -> Result<(), CoreError> {
        let provider = Provider::from_id(provider)
            .ok_or_else(|| CoreError::Refused("ספק לא מוכר.".to_owned()))?;
        let name = key_name(provider)
            .ok_or_else(|| CoreError::Refused("למודל המקומי אין מפתח.".to_owned()))?;
        let key = key.trim();
        if key.is_empty() {
            self.vault_mut()?.delete_secret(name)?;
        } else {
            if provider != Provider::Anthropic && !retention_ack {
                return Err(CoreError::Refused(format!(
                    "לפני שמירת מפתח של {} צריך לאשר שיש בכתב הסכם אפס שמירת מידע (ZDR) עם החברה.",
                    provider.display_name()
                )));
            }
            self.vault_mut()?.set_secret(name, key)?;
        }
        if provider != Provider::Anthropic {
            // Her written zero-retention confirmation for this company, dated; gone with the key.
            let value = if key.is_empty() {
                String::new()
            } else {
                crate::dates::unix_now().to_string()
            };
            self.vault_mut()?
                .set_setting(&format!("{ZDR_PREFIX}{}", provider.id()), &value)?;
        }
        Ok(())
    }

    pub fn set_model(&mut self, model: &str) -> Result<(), CoreError> {
        if !ALLOWED_MODELS.contains(&model) {
            return Err(CoreError::Refused("הדגם הזה לא ברשימה המותרת.".to_owned()));
        }
        self.vault_mut()?.set_setting(MODEL_KEY, model)?;
        Ok(())
    }

    pub fn set_lock_minutes(&mut self, minutes: u32) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(LOCK_KEY, &minutes.clamp(1, 60).to_string())?;
        Ok(())
    }

    pub fn set_practitioner(&mut self, names: Vec<String>) -> Result<(), CoreError> {
        let names = names
            .into_iter()
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .collect();
        self.vault_mut()?
            .set_practitioner(&dv_vault::Practitioner { names })?;
        Ok(())
    }

    // ------------------------------------------------------------ cases

    /// No API key for the chosen AI and no test transport: answers are built locally and
    /// nothing is sent.
    pub(crate) fn demo_mode(&mut self) -> Result<bool, CoreError> {
        let provider = self.provider();
        Ok(key_of(self.vault_ref()?, provider)?.is_none() && self.transport.is_none())
    }

    /// The model chosen in settings (Claude Opus by default).
    pub(crate) fn chosen_model(&self) -> String {
        self.vault
            .as_ref()
            .and_then(|v| v.setting(MODEL_KEY).ok().flatten())
            .filter(|m| ALLOWED_MODELS.contains(&m.as_str()))
            .unwrap_or_else(|| dv_ai::DEFAULT_MODEL.to_owned())
    }

    /// The company of the chosen model.
    #[must_use]
    pub fn provider(&self) -> Provider {
        provider_of(&self.chosen_model())
    }

    /// The name shown wherever the program speaks of the AI ("לכתוב עם Gemini").
    #[must_use]
    pub fn ai_name(&self) -> &'static str {
        self.provider().display_name()
    }

    pub(crate) fn model_config(&mut self) -> Result<ModelConfig, CoreError> {
        self.model_for(Task::Draft)
    }

    /// How long Claude may think, by task and by the speed chosen in settings. Sorting only
    /// classifies passages; drafting and consultation weigh more, and "thorough" asks for most.
    pub(crate) fn model_for(&mut self, task: Task) -> Result<ModelConfig, CoreError> {
        let v = self.vault_ref()?;
        let model = v
            .setting(MODEL_KEY)?
            .unwrap_or_else(|| dv_ai::DEFAULT_MODEL.to_owned());
        let speed = v
            .setting(SPEED_KEY)?
            .unwrap_or_else(|| DEFAULT_SPEED.to_owned());
        let effort = match (task, speed.as_str()) {
            (Task::Sort, _) | (_, "fast") => "low",
            (_, "thorough") => "high",
            _ => "medium",
        };
        Ok(ModelConfig {
            model,
            effort: effort.to_owned(),
        })
    }

    /// `fast` | `balanced` | `thorough`.
    pub fn set_speed(&mut self, speed: &str) -> Result<(), CoreError> {
        if !["fast", "balanced", "thorough"].contains(&speed) {
            return Err(CoreError::Refused("מהירות לא מוכרת.".to_owned()));
        }
        self.vault_mut()?.set_setting(SPEED_KEY, speed)?;
        Ok(())
    }
}
