//! Free consultation with the AI, optionally about a case.

use super::*;

impl Core {
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
        let left_out = crate::send::had_left_out(&out);
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
            left_out,
            conversation_id,
        })
    }

    // ------------------------------------------------------------ Word report
}
