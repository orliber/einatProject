//! Building a request (through the privacy gate), sending it, and storing the answer.

use super::*;
use dv_privacy::{Mark, Segment};

impl Core {
    /// Clear the request, or keep the names the filter found first. `Ok(true)`: new names
    /// were kept with the case and the caller builds the request again, so every text of it
    /// carries their tags (a name found in two texts gets one tag, and Claude's answer can
    /// be restored). `may_learn` is false on that second build: anything new then blocks.
    ///
    /// A refusal the fix loop can answer (a name, a pattern, a foreign tag the filter never
    /// saw) is answered here, up to three times: the refused words are hidden in the outgoing
    /// copy and the copy goes back to the same gate (round 25). Anything else stays refused.
    ///
    /// The requests themselves go through the same two steps apart, the filter away from the
    /// core's lock (`clear_request`, then `land`; `prepare.rs`); tests call them in a row here.
    #[cfg(test)]
    pub(crate) fn gate(
        &mut self,
        data: &PrivacyData,
        body: &Value,
        kind: PendingKind,
        prepared: &mut Prepared,
        may_learn: bool,
    ) -> Result<bool, CoreError> {
        let cleared = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            clear_request(data, &data.lookups(v), body, prepared, may_learn)
        };
        match cleared {
            Cleared::Learn(found) => {
                self.vault_mut()?.add_found_names(&data.case_id, &found)?;
                Ok(true)
            }
            Cleared::Answered(answered) => {
                self.land(data, answered, kind, prepared)?;
                Ok(false)
            }
        }
    }

    /// The gate's verdict kept with the case: the fixes in the log and on the card, the
    /// numbered tags with the case, and only a cleared payload as an approval to send.
    pub(crate) fn land(
        &mut self,
        data: &PrivacyData,
        answered: Answered,
        mut kind: PendingKind,
        prepared: &mut Prepared,
    ) -> Result<(), CoreError> {
        let Answered {
            result,
            fixed,
            cuts,
            mut codes,
        } = answered;
        if (!fixed.is_empty() || !cuts.is_empty()) && result.is_ok() {
            codes.sort();
            codes.dedup();
            let case_opt = (!data.case_id.is_empty()).then_some(data.case_id.as_str());
            self.vault_mut()?.record(
                AuditEvent::Blocked,
                case_opt,
                &serde_json::json!({ "codes": codes, "fixed": true }),
            )?;
            if !fixed.is_empty() {
                note_fixed(prepared, &fixed);
            }
            if !cuts.is_empty() {
                note_cut(prepared, &cuts);
            }
        }
        match result {
            Ok(payload) => {
                // The numbered tags this request gave keep their words with the case, so the
                // answer, and every later request of the case, reads them the same way.
                if !data.case_id.is_empty() {
                    self.vault_mut()?
                        .add_masks(&data.case_id, &data.masks.added())?;
                }
                // Without a case the book lives in the request; it holds the fixes too.
                if let PendingKind::Consult { masks, .. } = &mut kind {
                    *masks = data.masks.entries();
                }
                let id = payload.sha256().to_owned();
                prepared.approval_id = Some(id.clone());
                self.pending.insert(id, Pending { payload, kind });
            }
            Err(blocked) => {
                let codes: Vec<String> = blocked.reasons.iter().map(|r| r.code.clone()).collect();
                let case_opt = (!data.case_id.is_empty()).then_some(data.case_id.as_str());
                self.vault_mut()?.record(
                    AuditEvent::Blocked,
                    case_opt,
                    &serde_json::json!({ "codes": codes }),
                )?;
                let mut reasons = blocked.reasons;
                let v = self.vault_ref()?;
                let code_of = |case: &str| v.case_meta(case).map(|m| m.code).unwrap_or_default();
                for a in refused_auto_names(data, &reasons, &code_of) {
                    if !prepared
                        .auto_hidden
                        .iter()
                        .any(|x| x.tag == a.tag && normalize(&x.token) == normalize(&a.token))
                    {
                        prepared.auto_hidden.push(a);
                    }
                }
                locate_refused_names(data, &mut reasons, &code_of);
                prepared.blocked = reasons;
            }
        }
        Ok(())
    }

    /// Take an approved request out of the core so it can be sent without holding the
    /// session (the app stays responsive while Claude answers).
    /// `kind` is what the calling command sends; an approval for anything else is refused
    /// here, before any byte leaves, and stays waiting for its own command.
    pub fn begin_send(&mut self, approval_id: &str, kind: SendKind) -> Result<Outgoing, CoreError> {
        let pending = self
            .pending
            .get(approval_id)
            .filter(|p| p.kind.send_kind() == kind)
            .ok_or_else(|| match self.withdrawn.get(approval_id) {
                Some(why) => CoreError::Refused((*why).to_owned()),
                None => CoreError::NotFound("האישור פג. יש להכין את הבקשה מחדש.".to_owned()),
            })?;
        let model = serde_json::from_slice::<Value>(pending.payload.body())
            .ok()
            .and_then(|b| b.get("model")?.as_str().map(str::to_owned))
            .unwrap_or_default();
        // Consent is checked again at the send itself, also for a request kept for a retry or
        // past the monthly ceiling: consent removed since it was prepared stops it.
        if let Some(case_id) = pending.kind.case().map(str::to_owned) {
            if self.vault_ref()?.case_meta(&case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
        }
        let provider = provider_of(&model);
        let api_key = key_of(self.vault_ref()?, provider)?;
        // ChatGPT, Gemini and Mistral keep what they receive unless she has a written zero-retention
        // agreement with them; without her confirmation of one, nothing goes to them (D-040).
        if api_key.is_some()
            && matches!(
                provider,
                Provider::OpenAi | Provider::Gemini | Provider::Mistral
            )
        {
            let confirmed = self
                .vault_ref()?
                .setting(&format!("{ZDR_PREFIX}{}", provider.id()))?
                .is_some_and(|s| s.parse::<i64>().is_ok());
            if !confirmed {
                return Err(CoreError::Needs(
                    format!(
                        "לא נשלח: אין אישור על הסכם אפס שמירת מידע (ZDR) עם {}. אפשר לאשר בהגדרות, או לבחור Claude או מודל מקומי.",
                        provider.display_name()
                    ),
                    ErrorAction::settings("ai"),
                ));
            }
        }
        // Demo mode costs nothing; anything else stops at the monthly ceiling she set.
        if api_key.is_some() || self.transport.is_some() {
            self.refuse_over_cap()?;
        }
        let pending = self
            .pending
            .remove(approval_id)
            .ok_or_else(|| CoreError::NotFound("האישור פג. יש להכין את הבקשה מחדש.".to_owned()))?;
        Ok(Outgoing {
            pending,
            model,
            api_key,
            transport: self.transport.clone(),
            session: self.session,
        })
    }

    pub(crate) fn refuse_over_cap(&mut self) -> Result<(), CoreError> {
        let v = self.vault_ref()?;
        let Some(cap) = v
            .setting(usage::CAP_KEY)?
            .and_then(|s| s.parse::<u32>().ok())
        else {
            return Ok(());
        };
        let month = usage::Month::parse(v.setting(&usage::this_month_key())?.as_deref());
        if month.reached(cap) {
            let to_usd = raised_cap(cap);
            return Err(CoreError::Needs(
                format!(
                    "הגעת לתקרת ההוצאה החודשית שקבעת (${cap}). אפשר להעלות אותה ל-${to_usd} ולהמשיך, והבקשה נשלחת כמו שהוכנה."
                ),
                ErrorAction::RaiseCap { to_usd },
            ));
        }
        Ok(())
    }

    /// Count the tokens of an answer toward this month (numbers only). Never fails a send:
    /// the answer has arrived and is worth more than the count.
    pub(crate) fn note_usage(&mut self, payload: &ClearedPayload, answer: &Value, demo: bool) {
        if demo {
            return;
        }
        let model = serde_json::from_slice::<Value>(payload.body())
            .ok()
            .and_then(|b| b.get("model")?.as_str().map(str::to_owned))
            .unwrap_or_default();
        let key = usage::this_month_key();
        let Ok(v) = self.vault_mut() else { return };
        let mut month = usage::Month::parse(v.setting(&key).ok().flatten().as_deref());
        month.add(&model, usage::Tokens::from_answer(answer));
        if let Ok(json) = serde_json::to_string(&month) {
            let _ = v.set_state(&key, &json);
        }
    }

    /// This month's use of the AI and the ceiling, for settings.
    pub fn usage_summary(&mut self) -> Result<UsageSummary, CoreError> {
        let v = self.vault_mut()?;
        let key = usage::this_month_key();
        let cap = v
            .setting(usage::CAP_KEY)?
            .and_then(|s| s.parse::<u32>().ok());
        Ok(usage::Month::parse(v.setting(&key)?.as_deref()).view(&key, cap))
    }

    /// The monthly ceiling in dollars; `None` removes it.
    pub fn set_monthly_cap(&mut self, cap_usd: Option<u32>) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        match cap_usd {
            Some(0) => {
                return Err(CoreError::Refused(
                    "תקרה של 0 תעצור כל שליחה. כדי לבטל את התקרה, משאירים את השדה ריק.".to_owned(),
                ))
            }
            Some(cap) => v.set_setting(usage::CAP_KEY, &cap.to_string())?,
            // Empty = no ceiling (it does not parse as a number).
            None => v.set_setting(usage::CAP_KEY, "")?,
        }
        Ok(())
    }

    /// A failed send is not a send: the approval stays valid for a retry. Not when the vault
    /// locked meanwhile: a lock wipes every approval, including one that was on its way out.
    pub(crate) fn restore_pending(&mut self, out: Outgoing) {
        if self.vault.is_none() || out.session != self.session {
            return;
        }
        self.pending
            .insert(out.pending.payload.sha256().to_owned(), out.pending);
    }

    /// Send exactly what was approved, check the answer, and store it (tagged).
    pub fn send_section(&mut self, approval_id: &str) -> Result<SectionResult, CoreError> {
        let out = self.begin_send(approval_id, SendKind::Section)?;
        let response = out.transmit();
        self.finish_section(out, response)
    }

    /// Check and store the answer to a section request. A failure marks the case as stuck
    /// in the case list, and the next success for the case clears it.
    pub fn finish_section(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<SectionResult, CoreError> {
        let case_id = out.pending.kind.failing_case().map(str::to_owned);
        let result = self.store_section(out, response);
        if let Some(case_id) = case_id {
            self.note_send_outcome(&case_id, result.is_ok());
        }
        result
    }

    fn store_section(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<SectionResult, CoreError> {
        // A line the last check left out (D-082) is named under the draft.
        let left_out = had_left_out(&out);
        let crate::stage::Received {
            kind,
            response,
            demo,
            model,
        } = self.receive(out, response, crate::stage::Stage::Section)?;
        let PendingKind::Section {
            case_id,
            section_key,
            instruction_tagged,
            hidden,
            sources,
            approved,
            replaces,
            guide,
            from_points,
        } = kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let ai = provider_of(&model).display_name();
        let refs: Vec<(String, String)> = sources
            .iter()
            .map(|(sid, _, text)| (sid.clone(), text.clone()))
            .collect();
        let mut reply = if from_points {
            dv_ai::parse_points_section(&response, &approved.concat())?
        } else if DERIVED_SECTIONS.contains(&section_key.as_str()) {
            dv_ai::parse_derived_section(&response, &approved)?
        } else {
            dv_ai::parse_section(&response, &refs)?
        };
        let v = self.vault_mut()?;
        let identities = v.identities(&case_id)?;
        let masks = v.masks(&case_id)?;
        let case_tags: Vec<String> = identities.iter().map(|i| i.tag.clone()).collect();
        let everyone = v.all_identities()?;
        let inputs = v.inputs(&case_id)?;
        let sheets = score_sheets(v, &case_id, &inputs)?;
        for p in &mut reply.paragraphs {
            p.warnings.extend(dv_domain::check_scores(&p.text, &sheets));
            for s in scan_model_output(&p.text, &case_tags, &everyone) {
                p.warnings
                    .push(format!("{}: {}", s.message.replace("Claude", ai), s.token));
            }
        }
        // D-043: a paragraph that repeats a past report word for word.
        let texts: Vec<&str> = reply.paragraphs.iter().map(|p| p.text.as_str()).collect();
        let overlaps = style::overlap_warning(v, &texts)?;
        let interpretations = style::InterpretationCheck::new(v, &sheets)?;
        for (p, w) in reply.paragraphs.iter_mut().zip(overlaps) {
            p.warnings.extend(w);
            p.warnings.extend(interpretations.notes(&p.text));
        }
        v.add_message(
            &case_id,
            &section_key,
            ChatRole::User,
            &instruction_tagged,
            &hidden,
            demo,
        )?;
        v.add_message(
            &case_id,
            &section_key,
            ChatRole::Assistant,
            &reply.reply,
            &[],
            demo,
        )?;
        let input_ids = |p: &dv_ai::ProposedParagraph| -> Vec<String> {
            p.source_refs
                .iter()
                .filter_map(|r| {
                    sources
                        .iter()
                        .find(|(sid, _, _)| sid == r)
                        .map(|(_, id, _)| id.clone())
                })
                .collect()
        };
        // A rewrite of one paragraph stays in its place; if it was approved or removed
        // meanwhile, the answer is added as a new proposal instead of touching it.
        let rewritten = match (&replaces, reply.paragraphs.is_empty()) {
            (Some(id), false) => {
                let text: Vec<&str> = reply.paragraphs.iter().map(|p| p.text.as_str()).collect();
                let mut refs: Vec<String> = reply.paragraphs.iter().flat_map(&input_ids).collect();
                refs.dedup();
                let text = text.join("\n\n");
                // A proposal is rewritten in place; an approved paragraph gets a new wording
                // beside it, which replaces it only when she approves (D-032).
                v.rewrite_proposal(&case_id, id, &text, &refs)?
                    || v.propose_rewording(&case_id, id, &text, &refs)?
            }
            _ => false,
        };
        if !rewritten {
            if replaces.is_none() && !reply.paragraphs.is_empty() {
                v.supersede_proposals(&case_id, &section_key)?;
            }
            for p in &reply.paragraphs {
                v.add_draft(&case_id, &section_key, &p.text, Author::Ai, &input_ids(p))?;
            }
        }

        // The guide's lines no paragraph answers (round 29), worked out here from the numbers
        // the paragraphs give. Not in a demo answer, which follows no guide.
        let unanswered: Vec<&String> = if demo || reply.paragraphs.is_empty() {
            Vec::new()
        } else {
            guide
                .iter()
                .zip(1u32..)
                .filter(|(_, n)| !reply.paragraphs.iter().any(|p| p.guide == *n))
                .map(|(l, _)| l)
                .collect()
        };
        let practitioner = v.practitioner()?.names.first().cloned();
        let show = |t: &str| restore(t, &identities, practitioner.as_deref(), &masks);
        Ok(SectionResult {
            reply: show(&reply.reply),
            paragraphs: reply
                .paragraphs
                .into_iter()
                .map(|mut p| {
                    p.text = show(&p.text);
                    p
                })
                .collect(),
            questions: reply.questions.iter().map(|q| show(q)).collect(),
            missing: unanswered
                .iter()
                .map(|l| format!("חסרה פסקה לפי ההנחיה: {}", show(l)))
                .chain(reply.missing.iter().map(|q| show(q)))
                .chain(left_out.then(|| LEFT_OUT_NOTE.to_owned()))
                .collect(),
            contradictions: reply.contradictions.iter().map(|q| show(q)).collect(),
            demo,
        })
    }
}

/// The sources a section is written from, filtered: for each material the passages routed to
/// this section (D-022). Returns the tagged inputs and, per `S#`, the material's id and
/// tagged text. Each is also added to the review screen.
pub(crate) type SectionSources = (Vec<TaggedInput>, Vec<(String, String, String)>);

/// A material that goes to a section, as the vault holds it: read under the core's lock and
/// filtered away from it (D-109). Its text is wiped when it is dropped.
pub(crate) struct SourceRead {
    id: String,
    kind: InputKind,
    title: String,
    content: String,
    /// The passages routed to the section, or the whole material.
    feed: dv_domain::Feed,
}

impl Drop for SourceRead {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.title);
        zeroize::Zeroize::zeroize(&mut self.content);
    }
}

/// The materials a section is written from, in the order they go out (D-022): the table, the
/// sorting, and Einat's choice decide what goes here.
pub(crate) fn read_sources(
    v: &Vault,
    structure: &ReportStructure,
    case_id: &str,
    section_key: &str,
) -> Result<Vec<SourceRead>, CoreError> {
    // A score table of an instrument she does not give stays in the computer (D-098).
    let mut inputs = crate::instruments::request_inputs(v, case_id)?;
    if section_key == sorting::PRIOR_SECTION {
        // Round 31: the institute first, then the community, newest first, fixed here.
        let keys = inputs
            .iter()
            .map(|i| sorting::prior_key(v, case_id, i))
            .collect::<Result<Vec<_>, _>>()?;
        inputs = dv_domain::record_order(&keys)
            .into_iter()
            .filter_map(|k| inputs.get(k).cloned())
            .collect();
    }
    let mut out = Vec::new();
    for inp in inputs {
        // D-022: the table, the sorting, and Einat's choice decide what goes here.
        let table = sorting::table_for(structure, inp.kind);
        let count = passage_ranges(&inp.content).len();
        let Some(feed) = sorting::routing_of(v, case_id, &inp.id)?.feed(section_key, &table, count)
        else {
            continue;
        };
        out.push(SourceRead {
            id: inp.id,
            kind: inp.kind,
            title: inp.title,
            content: inp.content,
            feed,
        });
    }
    Ok(out)
}

/// The materials read by [`read_sources`], filtered: for each its title and the passages
/// routed to the section. Returns the tagged inputs and, per `S#`, the material's id and
/// tagged text. Each is also added to the review screen. `stop` is asked before each one: a
/// job the vault's lock cancelled ends there.
pub(crate) fn filter_sources(
    read: &[SourceRead],
    section_key: &str,
    section_title: &str,
    ctx: &PrivacyContext<'_>,
    hints: &NameHints,
    review: &mut Review,
    stop: &dyn Fn() -> Result<(), CoreError>,
) -> Result<SectionSources, CoreError> {
    let run = |t: &str| {
        hints
            .filter(t, ctx)
            .map_err(|e| CoreError::Internal(e.to_string()))
    };
    let mut tagged_sources = Vec::new();
    let mut source_rows = Vec::new();
    for inp in read {
        stop()?;
        let sid = format!("S{}", tagged_sources.len() + 1);
        let title = run(&inp.title)?;
        // G4: a long material sent whole goes with the passages that bear on the section.
        let cut = match inp.feed {
            dv_domain::Feed::Whole if inp.kind != InputKind::TestScores => {
                sorting::trimmed(&inp.content, section_key, section_title)
            }
            _ => None,
        };
        let (feed, kind) = match (&inp.feed, cut) {
            (dv_domain::Feed::Whole, Some((chosen, total))) => (
                dv_domain::Feed::Passages(chosen.clone()),
                format!(
                    "{} · {} מתוך {total} קטעים, אלה שנוגעים לסעיף (השאר נשאר במחשב)",
                    inp.kind.label_he(),
                    chosen.len()
                ),
            ),
            (dv_domain::Feed::Whole, None) => {
                (dv_domain::Feed::Whole, inp.kind.label_he().to_owned())
            }
            (f @ dv_domain::Feed::Passages(_), _) => (
                f.clone(),
                format!("{} · קטעים שנבחרו לסעיף", inp.kind.label_he()),
            ),
        };
        let content = sorting::section_text(&inp.content, &feed, ctx, hints)?;
        // A title with nothing to hide is shown as the source's label, not as a part.
        if title.original_segments.iter().any(|s| s.mark.is_some()) {
            review.add(format!("{sid} · {kind} · כותרת"), &title);
            review.add(format!("{sid} · {kind}"), &content);
        } else {
            review.add(format!("{sid} · {kind} · {}", title.tagged), &content);
        }
        tagged_sources.push(TaggedInput {
            input_id: inp.id.clone(),
            kind_label: inp.kind.label_he().to_owned(),
            title_tagged: title.tagged,
            content_tagged: content.tagged.clone(),
        });
        source_rows.push((sid, inp.id.clone(), content.tagged));
    }
    Ok((tagged_sources, source_rows))
}

/// The gate's side of a request, which needs no vault: the names the filter found first
/// (`Learn`: kept with the case, and the request is built again), else the fix loop's verdict.
pub(crate) enum Cleared {
    Learn(Vec<FoundName>),
    Answered(Answered),
}

/// The checks of [`Core::gate`] that read nothing but the request and the case's privacy data,
/// so they can run away from the core's lock (D-109). Nothing is kept here: [`Core::land`]
/// keeps the verdict under the lock.
pub(crate) fn clear_request<A, N, P>(
    data: &PrivacyData,
    lookups: &CaseLookups<'_, A, N, P>,
    body: &Value,
    prepared: &Prepared,
    may_learn: bool,
) -> Cleared
where
    A: Fn(&str) -> bool,
    N: Fn(&str) -> bool,
    P: Fn(&str) -> bool,
{
    let found = found_names(data, &prepared.auto_hidden);
    if may_learn && !data.case_id.is_empty() && !found.is_empty() {
        return Cleared::Learn(found);
    }
    let suspects = prepared.suspects.len();
    let mut case_tags: HashSet<String> = data
        .identities
        .iter()
        .filter(|i| i.case_id == data.case_id)
        .map(|i| i.tag.clone())
        .collect();
    // A consultation without a case has nowhere to keep names: the tags the filter gave
    // them are this request's own.
    if data.case_id.is_empty() {
        case_tags.extend(
            prepared
                .auto_hidden
                .iter()
                .filter(|a| matches!(a.kind, AutoKind::Name | AutoKind::OtherCase))
                .map(|a| a.tag.clone()),
        );
    }
    Cleared::Answered(answer_refusals(
        body,
        &lookups.context(),
        &case_tags,
        suspects,
        &data.masks,
    ))
}

/// What the fix loop made of a request: the gate's last verdict, what was hidden or cut on
/// the way, and the codes it answered.
pub(crate) struct Answered {
    pub(crate) result: Result<ClearedPayload, dv_privacy::Blocked>,
    pub(crate) fixed: Vec<dv_privacy::FixedItem>,
    pub(crate) cuts: Vec<dv_ai::Cut>,
    pub(crate) codes: Vec<String>,
}

/// The gate on `body`, and when it refuses only what can be answered in the outgoing copy
/// (a name, a pattern, a foreign tag, the size), that answer and the same gate again, up to
/// three times (round 25). The gate is not changed; a fix only takes words out.
pub(crate) fn answer_refusals(
    body: &Value,
    ctx: &PrivacyContext<'_>,
    case_tags: &HashSet<String>,
    suspects: usize,
    book: &dv_privacy::MaskBook,
) -> Answered {
    fn request<'a>(
        body: &'a Value,
        ctx: &'a PrivacyContext<'a>,
        case_tags: &'a HashSet<String>,
        suspects: usize,
    ) -> GateRequest<'a> {
        GateRequest {
            body,
            ctx,
            case_tags,
            unresolved_suspects: suspects,
            canaries: &[],
            max_bytes: MAX_REQUEST_BYTES,
        }
    }
    let mut body = body.clone();
    let mut result = clear(&request(&body, ctx, case_tags, suspects));
    let mut fixed: Vec<dv_privacy::FixedItem> = Vec::new();
    let mut cuts: Vec<dv_ai::Cut> = Vec::new();
    let mut answered: Vec<String> = Vec::new();
    for _ in 0..3 {
        let Err(blocked) = &result else { break };
        let codes: Vec<&str> = blocked.reasons.iter().map(|r| r.code.as_str()).collect();
        if !codes
            .iter()
            .all(|c| *c == "too_large" || dv_privacy::FIXABLE_CODES.contains(c))
        {
            break;
        }
        answered.extend(codes.iter().map(|c| (*c).to_owned()));
        if codes.iter().any(|c| dv_privacy::FIXABLE_CODES.contains(c)) {
            let Some((next, items)) =
                dv_privacy::fix_refused(&request(&body, ctx, case_tags, suspects), book)
            else {
                break;
            };
            fixed.extend(items);
            body = next;
        }
        // Too large: the longest materials are cut at a line, the rest stays home.
        if codes.contains(&"too_large") {
            let Some((next, shortened)) = dv_ai::shrink_to_fit(&body, MAX_REQUEST_BYTES) else {
                break;
            };
            for c in shortened {
                match cuts.iter_mut().find(|x| x.label == c.label) {
                    Some(x) => x.kept = c.kept,
                    None => cuts.push(c),
                }
            }
            body = next;
        }
        result = clear(&request(&body, ctx, case_tags, suspects));
    }
    // The last step (D-082, Einat chose it): when hiding the words was not enough, every line
    // the gate still refuses on its own is left out whole, and the copy goes to the same gate
    // once more. Only for refusals a fix answers; a canary or an error stays refused.
    if let Err(blocked) = &result {
        let codes: Vec<&str> = blocked.reasons.iter().map(|r| r.code.as_str()).collect();
        if !codes.is_empty() && codes.iter().all(|c| dv_privacy::FIXABLE_CODES.contains(c)) {
            if let Some((next, items)) =
                dv_privacy::drop_refused_lines(&request(&body, ctx, case_tags, suspects))
            {
                answered.extend(codes.iter().map(|c| (*c).to_owned()));
                fixed.extend(items);
                body = next;
                result = clear(&request(&body, ctx, case_tags, suspects));
            }
        }
    }
    Answered {
        result,
        fixed,
        cuts,
        codes: answered,
    }
}

/// The ceiling offered when the month reaches it: half again, at least $10 more, in whole
/// fives, so one click is enough for the rest of a busy month and never a blank cheque.
pub(crate) fn raised_cap(cap: u32) -> u32 {
    let more = (cap / 2).max(10);
    cap.saturating_add(more).div_ceil(5).saturating_mul(5)
}

/// What was cut for size shows with the request, like what was hidden: which material, and
/// how much of it went.
pub(crate) fn note_cut(prepared: &mut Prepared, cuts: &[dv_ai::Cut]) {
    let lines: Vec<String> = cuts
        .iter()
        .map(|c| {
            let percent = c.kept.saturating_mul(100) / c.total.max(1);
            format!("{}: נשלחו כ-{percent}% מהטקסט, ההמשך נשאר במחשב", c.label)
        })
        .collect();
    let text = lines.join("\n");
    let plain = || {
        vec![Segment {
            text: text.clone(),
            mark: None,
            label: None,
        }]
    };
    prepared.parts.push(ReviewPart {
        label: "קוצר, כי הבקשה הייתה ארוכה מדי".to_owned(),
        original: plain(),
        outgoing: plain(),
    });
}

/// What the fix loop hid shows with the request (the review in advanced mode, and the history
/// of what left the computer): the words, what went out instead, and their kinds.
pub(crate) fn note_fixed(prepared: &mut Prepared, fixed: &[dv_privacy::FixedItem]) {
    let mut original = Vec::new();
    let mut outgoing = Vec::new();
    for (i, f) in fixed.iter().enumerate() {
        if i > 0 {
            let sep = Segment {
                text: ", ".to_owned(),
                mark: None,
                label: None,
            };
            original.push(sep.clone());
            outgoing.push(sep);
        }
        original.push(Segment {
            text: f.original.clone(),
            mark: Some(Mark::Replaced),
            label: Some(f.label.clone()),
        });
        outgoing.push(Segment {
            text: f.tag.clone(),
            mark: Some(Mark::Tag),
            label: Some(f.label.clone()),
        });
        if !prepared.hidden.contains(&f.label) {
            prepared.hidden.push(f.label.clone());
        }
    }
    prepared.parts.push(ReviewPart {
        label: "הוסתר בבדיקה האחרונה לפני השליחה".to_owned(),
        original,
        outgoing,
    });
}

impl Core {
    /// The case's requests waiting to go (prepared, kept for a retry or past the ceiling) are
    /// dropped when its people or details change: each was filtered with what the case knew
    /// then, so a name added since would go out untagged (the review of stage 2).
    pub(crate) fn forget_pending(&mut self, case_id: &str) {
        self.pending
            .retain(|_, p| p.kind.case() != Some(case_id) && p.payload.case_id() != case_id);
        // A request still being filtered lands as begun again (D-109).
        self.pending_epoch = self.pending_epoch.wrapping_add(1);
    }
}

/// Said where a request went out with a line the last check left out (D-082): under a
/// draft, after a sorting, a check against the sources or a consultation (review of stage 6,
/// R4), so a paragraph marked unsupported or a material left where it was has its reason.
pub(crate) const LEFT_OUT_NOTE: &str = "קטע מהחומרים לא נשלח, כי הבדיקה לא הייתה בטוחה שאין בו שם. אם הוא חשוב לסעיף, אפשר להוסיף אותו בעצמך.";

/// Whether the request went out with a line left out (D-082).
pub(crate) fn had_left_out(out: &Outgoing) -> bool {
    out.pending
        .payload
        .body()
        .windows(dv_privacy::LEFT_OUT.len())
        .any(|w| w == dv_privacy::LEFT_OUT.as_bytes())
}
