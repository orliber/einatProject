//! Free consultation with the AI, optionally about a case.

use super::*;

impl Core {
    /// Free consultation, optionally about a case (then approved sections go along, filtered).
    /// `conversation_id`: a saved conversation to continue (its earlier turns go along, as
    /// they were sent: filtered), or `None` for a new one.
    pub fn prepare_consult(
        &mut self,
        case_id: Option<&str>,
        conversation_id: Option<&str>,
        message: &str,
    ) -> Result<Prepared, CoreError> {
        self.prepare_consult_once(case_id, conversation_id, message, false)
    }

    pub(crate) fn prepare_consult_once(
        &mut self,
        case_id: Option<&str>,
        conversation_id: Option<&str>,
        message: &str,
        learned: bool,
    ) -> Result<Prepared, CoreError> {
        // Earlier turns, as Einat saw them: filtered again now, with today's names and decisions,
        // and shown on the review screen like everything else that leaves the computer.
        let earlier = match conversation_id {
            Some(id) => {
                let (case_of, turns) = self.stored_consultation(id)?;
                if case_of.as_deref() != case_id {
                    return Err(CoreError::Refused(
                        "השיחה הזו שייכת לתיק אחר. אפשר לפתוח שיחה חדשה.".to_owned(),
                    ));
                }
                turns
            }
            None => Vec::new(),
        };
        let model = self.model_for(Task::Consult)?;
        let key = case_id.unwrap_or("").to_owned();
        let data = self.privacy_data(&key)?;
        let hints = self.case_hints(&key, &[message])?;
        let demo_mode = self.demo_mode()?;
        let (input, review) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if let Some(c) = case_id {
                if v.case_meta(c)?.consent.is_none() {
                    return Err(CoreError::ConsentMissing);
                }
            }
            let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
            let is_name = |t: &str| data.is_name.contains(&v.token_hmac(t));
            let past = |t: &str| !data.past.is_empty() && data.past.contains(&v.token_hmac(t));
            let ctx = PrivacyContext {
                case_id: &data.case_id,
                identities: &data.identities,
                practitioner: &data.practitioner,
                allowlisted: &allow,
                confirmed_names: &is_name,
                past_names: &past,
                today: today(),
                masks: Some(&data.masks),
            };
            let msg = hints
                .filter(message, &ctx)
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            let mut review = Review::default();
            let mut history = Vec::new();
            for (n, t) in earlier.iter().enumerate() {
                let label = |who: &str| format!("{who} ({})", n / 2 + 1);
                if t.role == "user" {
                    // Einat's own words: filtered again, with today's names and decisions.
                    let out =
                        filter(&t.shown, &ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
                    review.add(label("שאלה קודמת"), &out);
                    history.push(TaggedTurn {
                        role: t.role.clone(),
                        text_tagged: out.tagged,
                    });
                } else {
                    // Claude's own answer goes back as it came (it only ever saw tags); shown
                    // on the review screen, without name questions about Claude's own words.
                    let out =
                        filter(&t.tagged, &ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
                    review.add_context(label("תשובה קודמת"), &out);
                    history.push(TaggedTurn {
                        role: t.role.clone(),
                        text_tagged: t.tagged.clone(),
                    });
                }
            }
            review.add("השאלה שלך".to_owned(), &msg);
            let mut case_context = None;
            if let Some(c) = case_id {
                let structure = ReportStructure::load_default()
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                let mut lines = Vec::new();
                for s in structure.sections() {
                    for d in v
                        .drafts(c, &s.key)?
                        .into_iter()
                        .filter(|d| d.status == DraftStatus::Approved)
                    {
                        lines.push(format!("{}: {}", model_title(s, &ctx)?, d.text_tagged));
                    }
                }
                if !lines.is_empty() {
                    let out = filter(&lines.join("\n"), &ctx)
                        .map_err(|e| CoreError::Internal(e.to_string()))?;
                    review.add("סיכום התיק (סעיפים מאושרים)".to_owned(), &out);
                    case_context = Some(out.tagged);
                }
            }
            let input = dv_ai::ConsultInput {
                history,
                message_tagged: msg.tagged.clone(),
                case_context_tagged: case_context,
            };
            (input, review)
        };
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let body = dv_ai::build_consult_request(&model, &input, &nonce);
        let mut prepared = review.into_case_prepared(demo_mode, &data);
        let kind = PendingKind::Consult {
            case_id: case_id.map(str::to_owned),
            conversation_id: conversation_id.map(str::to_owned),
            message_tagged: input.message_tagged,
            message_shown: message.to_owned(),
            hidden: prepared.hidden.clone(),
            masks: data.masks.entries(),
        };
        if self.gate(&data, &body, kind, &mut prepared, !learned)? {
            return self.prepare_consult_once(case_id, conversation_id, message, true);
        }
        Ok(prepared)
    }

    pub fn send_consult(&mut self, approval_id: &str) -> Result<ConsultResult, CoreError> {
        let out = self.begin_send(approval_id, SendKind::Consult)?;
        let response = out.transmit();
        self.finish_consult(out, response)
    }

    /// Check and keep the answer to a consultation (history in memory only).
    pub fn finish_consult(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<ConsultResult, CoreError> {
        let crate::stage::Received {
            kind,
            response,
            demo,
            ..
        } = self.receive(out, response, crate::stage::Stage::Consult)?;
        let PendingKind::Consult {
            case_id,
            conversation_id,
            message_tagged,
            message_shown,
            hidden,
            masks,
        } = kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let answer = dv_ai::parse_consult(&response)?;
        // The numbers, dates and places of the question come back as she wrote them, with a
        // case or without one (its mask book lives only in this request then).
        let identities = match &case_id {
            Some(c) => self.vault_ref()?.identities(c)?,
            None => Vec::new(),
        };
        let shown = restore(&answer, &identities, None, &masks);
        let conversation_id = self.keep_consultation(
            conversation_id.as_deref(),
            case_id.as_deref(),
            consultations::Exchange {
                question_tagged: message_tagged,
                question_shown: message_shown,
                hidden,
                answer_tagged: answer,
                answer_shown: shown.clone(),
                demo,
            },
        )?;
        Ok(ConsultResult {
            answer: shown,
            demo,
            conversation_id,
        })
    }

    // ------------------------------------------------------------ Word report
}
