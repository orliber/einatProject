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
    pub(crate) fn gate(
        &mut self,
        data: &PrivacyData,
        body: &Value,
        mut kind: PendingKind,
        prepared: &mut Prepared,
        may_learn: bool,
    ) -> Result<bool, CoreError> {
        let found = found_names(data, &prepared.auto_hidden);
        if may_learn && !data.case_id.is_empty() && !found.is_empty() {
            self.vault_mut()?.add_found_names(&data.case_id, &found)?;
            return Ok(true);
        }
        let suspects = prepared.suspects.len();
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
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
            masks: None,
        };
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
        let Answered {
            result,
            fixed,
            cuts,
            mut codes,
        } = answer_refusals(body, &ctx, &case_tags, suspects, &data.masks);
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
        Ok(false)
    }

    /// Build the request for one section and everything the review screen needs.
    pub fn prepare_section(
        &mut self,
        case_id: &str,
        section_key: &str,
        instruction: &str,
    ) -> Result<Prepared, CoreError> {
        self.prepare_section_replacing(case_id, section_key, instruction, None)
    }

    /// A section request whose answer rewrites one proposed paragraph (`replaces`), or, with
    /// `None`, becomes the section's draft in place of the paragraphs not approved yet: one
    /// draft at a time, never a pile of versions.
    pub fn prepare_section_replacing(
        &mut self,
        case_id: &str,
        section_key: &str,
        instruction: &str,
        replaces: Option<&str>,
    ) -> Result<Prepared, CoreError> {
        // Only a paragraph of this section in this case can be rewritten (SEC9).
        if let Some(id) = replaces {
            let ours = self
                .vault_ref()?
                .drafts(case_id, section_key)?
                .iter()
                .any(|d| d.id == id);
            if !ours {
                return Err(CoreError::NotFound("הפסקה".to_owned()));
            }
        }
        self.prepare_section_once(case_id, section_key, instruction, replaces, false, false)
    }

    /// "לנסח מהנקודות שלי" (round 30): a section she writes herself, worded from the points
    /// she typed, with the section's guide and her style. No material, score or other section
    /// goes with it.
    pub fn prepare_from_points(
        &mut self,
        case_id: &str,
        section_key: &str,
        points: &str,
    ) -> Result<Prepared, CoreError> {
        if points.trim().is_empty() {
            return Err(CoreError::Refused(
                "כותבים קודם את הנקודות, ואז מנסחים מהן.".to_owned(),
            ));
        }
        self.prepare_section_once(case_id, section_key, points, None, false, true)
    }

    pub(crate) fn prepare_section_once(
        &mut self,
        case_id: &str,
        section_key: &str,
        instruction: &str,
        replaces: Option<&str>,
        learned: bool,
        points: bool,
    ) -> Result<Prepared, CoreError> {
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let letter = Letter::of(section_key);
        let section = match letter {
            Some(l) => dv_domain::ReportSection {
                key: section_key.to_owned(),
                title: l.title().to_owned(),
                inputs: Vec::new(),
                about: String::new(),
                mode: dv_domain::SectionMode::Ai,
                guide: String::new(),
            },
            None => structure
                .section(section_key)
                .ok_or_else(|| CoreError::NotFound(section_key.to_owned()))?
                .clone(),
        };
        // Her own sections (round 30): nothing is sent for them but her points, on request.
        let own = section.mode == dv_domain::SectionMode::Own;
        if own && !points {
            return Err(CoreError::Refused(
                "את כותבת את הסעיף הזה בעצמך, ולכן שום חומר לא נשלח בשבילו. אפשר לכתוב פסקה, או לנסח מהנקודות שלך."
                    .to_owned(),
            ));
        }
        if points && !own {
            return Err(CoreError::Refused(
                "ניסוח מהנקודות שלך הוא לסעיפים שאת כותבת בעצמך.".to_owned(),
            ));
        }
        let model = self.model_config()?;
        let data = self.privacy_data(case_id)?;
        let hints = self.case_hints(case_id, &[])?;
        let demo_mode = self.demo_mode()?;

        let (input, review, sources) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            let meta = v.case_meta(case_id)?;
            if meta.consent.is_none() {
                return Err(CoreError::ConsentMissing);
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
            let run = |t: &str| {
                hints
                    .filter(t, &ctx)
                    .map_err(|e| CoreError::Internal(e.to_string()))
            };
            let fixed_title = |sec: &dv_domain::ReportSection| model_title(sec, &ctx);
            let mut review = Review::default();
            let derived = DERIVED_SECTIONS.contains(&section_key) || letter.is_some();
            let (tagged_sources, source_rows) = if derived || points {
                (Vec::new(), Vec::new())
            } else {
                collect_sources(
                    v,
                    &structure,
                    &ctx,
                    &hints,
                    case_id,
                    section_key,
                    &mut review,
                )?
            };
            let mut approved_context = Vec::new();
            if derived && !points {
                // A letter gets only the sections it is written from (EX-4): the school letter
                // only the recommendations, never the background or the diagnoses.
                let wanted = |key: &str| match letter {
                    Some(l) => l.written_from().contains(&key),
                    None => key != section_key && !DERIVED_SECTIONS.contains(&key),
                };
                // The recommendations she left out of the letters (round 44); the rest go in.
                let marked = |key: &str, id: &str| match (&letter, &meta.letter_left_out) {
                    (Some(_), Some(ids)) if key == "recommendations" => {
                        !ids.iter().any(|i| i == id)
                    }
                    _ => true,
                };
                for s in structure.sections().filter(|s| wanted(&s.key)) {
                    let text: Vec<String> = v
                        .drafts(case_id, &s.key)?
                        .into_iter()
                        .filter(|d| d.status == DraftStatus::Approved && marked(&s.key, &d.id))
                        .map(|d| d.text_tagged)
                        .collect();
                    if !text.is_empty() {
                        let out = run(&text.join("\n"))?;
                        review.add(format!("סעיף מאושר · {}", s.title), &out);
                        approved_context.push((fixed_title(s)?, out.tagged));
                    }
                }
            }
            // Nothing approved yet: there is nothing to write it from, so nothing is sent.
            if letter.is_some() && approved_context.is_empty() {
                return Err(CoreError::Refused(
                    "המכתב נכתב מתוך ההמלצות שאישרת בדוח, ועוד אין המלצות מאושרות. מאשרים קודם את סעיף ההמלצות.".to_owned(),
                ));
            }
            if derived && !points && approved_context.is_empty() {
                return Err(CoreError::Refused(
                    "הסעיף הזה נכתב מתוך הסעיפים שכבר אישרת, ועוד לא אישרת אף סעיף. מאשרים קודם את הטיוטות בסעיפים האחרים, ואז חוזרים לכאן."
                        .to_owned(),
                ));
            }
            // The current draft may contain manual edits, so it goes through review too.
            let mut current = Vec::new();
            for d in v
                .drafts(case_id, section_key)?
                .into_iter()
                .filter(|_| !points)
                .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
            {
                let out = run(&d.text_tagged)?;
                review.add("הטיוטה הנוכחית".to_owned(), &out);
                current.push(out.tagged);
            }
            // The conversation so far is not shown again, but what the filter hides in it is
            // kept like everything else, so its tags belong to the case.
            let mut history: Vec<TaggedTurn> = Vec::new();
            for m in v
                .messages(case_id, section_key)?
                .into_iter()
                .filter(|_| !points)
            {
                let o = run(&m.text_tagged)?;
                review.note_hidden(&o);
                history.push(TaggedTurn {
                    role: if m.role == ChatRole::User {
                        "user".to_owned()
                    } else {
                        "assistant".to_owned()
                    },
                    text_tagged: o.tagged,
                });
            }
            let instr = run(instruction)?;
            review.add(
                if points {
                    "הנקודות שלך"
                } else {
                    "הבקשה שלך"
                }
                .to_owned(),
                &instr,
            );
            // Her approved style profile (D-043), as it goes out with this case's names hidden.
            // The items that fit this section's material best go out (D-052).
            let query: String = tagged_sources
                .iter()
                .map(|s| s.content_tagged.as_str())
                .chain([instr.tagged.as_str()])
                .collect::<Vec<_>>()
                .join("\n");
            let mut style_profile = style::style_for_request(v, &ctx, section_key, &query)?;
            // How she interprets the scores of this case (round 37), in a section written from
            // the scores: the approved interpretations that fit each score.
            if section.inputs.contains(&InputKind::TestScores) && !points {
                let sheets = score_sheets(v, case_id, &v.inputs(case_id)?)?;
                if let Some(text) = style::interpretations_for_request(v, &sheets)? {
                    style_profile = Some(match style_profile {
                        Some(p) => format!("{p}\n{text}"),
                        None => text,
                    });
                }
            }
            if let Some(text) = &style_profile {
                review.add_context("פרופיל הסגנון שלך".to_owned(), &run(text)?);
            }
            // How she wants the section written (round 29), filtered like her request: the
            // case's own lines may name someone.
            let guide_text = if letter.is_some() {
                String::new()
            } else {
                guides::guide_for(v, &meta, &section)?.text()
            };
            let guide_tagged = if guide_text.is_empty() {
                String::new()
            } else {
                let out = run(&guide_text)?;
                review.add("איך לכתוב את הסעיף".to_owned(), &out);
                out.tagged
            };
            // A first draft of the whole section is checked against the guide's lines, when it
            // has two or more; a rewrite of one paragraph or a turn on a draft is not.
            let lines = dv_ai::guide_lines(&guide_tagged);
            let guide_lines: Vec<String> =
                if current.is_empty() && replaces.is_none() && lines.len() >= 2 {
                    lines.into_iter().map(str::to_owned).collect()
                } else {
                    Vec::new()
                };
            // The DSM-5 severity she set (round 43) goes with the DSM section as fixed words,
            // after the guide, so the model writes her levels and no other; while she has
            // set none, that it writes none (review of stage 6, R3).
            let severity = (section.key == "dsm" && !points && letter.is_none())
                .then(|| dv_domain::severity_request(meta.dsm_severity.as_ref()));
            let guide_tagged = match severity {
                Some(text) => {
                    let out = run(&text)?;
                    review.add("רמת החומרה שקבעת".to_owned(), &out);
                    if guide_tagged.is_empty() {
                        out.tagged
                    } else {
                        format!("{guide_tagged}\n{}", out.tagged)
                    }
                }
                None => guide_tagged,
            };
            let input = SectionInput {
                section_key: section.key.clone(),
                section_title: fixed_title(&section)?,
                age: meta.age.map(dv_domain::Age::display),
                gender: meta.child_gender,
                sources: tagged_sources,
                approved_context,
                current_draft: current,
                history,
                instruction_tagged: instr.tagged.clone(),
                style_profile,
                guide_tagged,
                points_only: points,
                sources_in_order: section_key == sorting::PRIOR_SECTION && !points,
            };
            // From her points, the numbers in the answer are checked against the points.
            let approved = if points {
                vec![instr.tagged.clone()]
            } else {
                input
                    .approved_context
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect::<Vec<_>>()
            };
            let sources = (
                section.key.clone(),
                instr.tagged,
                instr.hidden.clone(),
                source_rows,
                approved,
                guide_lines,
            );
            (input, review, sources)
        };

        let nonce = match self
            .section_nonces
            .get(&(case_id.to_owned(), input.section_key.clone()))
        {
            Some(n) => n.clone(),
            None => {
                let n = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
                self.section_nonces
                    .insert((case_id.to_owned(), input.section_key.clone()), n.clone());
                n
            }
        };
        let (body, _refs) = dv_ai::build_section_request(&model, &input, &nonce);
        let mut prepared = review.into_case_prepared(demo_mode, &data);
        let (key, instruction_tagged, instr_hidden, rows, approved, guide) = sources;
        let kind = PendingKind::Section {
            case_id: case_id.to_owned(),
            section_key: key,
            instruction_tagged,
            hidden: instr_hidden,
            sources: rows,
            approved,
            replaces: replaces.map(str::to_owned),
            guide,
            from_points: points,
        };
        if self.gate(&data, &body, kind, &mut prepared, !learned)? {
            return self.prepare_section_once(
                case_id,
                section_key,
                instruction,
                replaces,
                true,
                points,
            );
        }
        Ok(prepared)
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
            .ok_or_else(|| CoreError::NotFound("האישור פג. יש להכין את הבקשה מחדש.".to_owned()))?;
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

    /// Prepare every section that has material and nothing written yet (the "write the empty
    /// sections" button). A section with a draft or approved paragraphs is left as it is:
    /// writing it again would put a second draft beside what she already approved.
    pub fn prepare_full_draft(
        &mut self,
        case_id: &str,
    ) -> Result<Vec<(String, Prepared)>, CoreError> {
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let fed: HashSet<String> = {
            let v = self.vault_ref()?;
            let inputs = v.inputs(case_id)?;
            Core::material_routing(v, &structure, case_id, &inputs)?
                .into_iter()
                .flat_map(|r| r.feeds)
                .collect()
        };
        let mut out = Vec::new();
        for s in structure
            .sections()
            .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()))
            .filter(|s| s.mode == dv_domain::SectionMode::Ai)
        {
            let written = self
                .vault_ref()?
                .drafts(case_id, &s.key)?
                .iter()
                .any(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved));
            if fed.contains(&s.key) && !written {
                let p = self.prepare_section(
                    case_id,
                    &s.key,
                    "כתוב/י טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.",
                )?;
                out.push((s.key.clone(), p));
            }
        }
        Ok(out)
    }
}

/// The sources a section is written from, filtered: for each material the passages routed to
/// this section (D-022). Returns the tagged inputs and, per `S#`, the material's id and
/// tagged text. Each is also added to the review screen.
pub(crate) type SectionSources = (Vec<TaggedInput>, Vec<(String, String, String)>);

pub(crate) fn collect_sources(
    v: &Vault,
    structure: &ReportStructure,
    ctx: &PrivacyContext<'_>,
    hints: &NameHints,
    case_id: &str,
    section_key: &str,
    review: &mut Review,
) -> Result<SectionSources, CoreError> {
    let run = |t: &str| {
        hints
            .filter(t, ctx)
            .map_err(|e| CoreError::Internal(e.to_string()))
    };
    let mut tagged_sources = Vec::new();
    let mut source_rows = Vec::new();
    let mut inputs = v.inputs(case_id)?;
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
    for inp in inputs {
        // D-022: the table, the sorting, and Einat's choice decide what goes here.
        let table = sorting::table_for(structure, inp.kind);
        let count = passage_ranges(&inp.content).len();
        let Some(feed) = sorting::routing_of(v, case_id, &inp.id)?.feed(section_key, &table, count)
        else {
            continue;
        };
        let sid = format!("S{}", tagged_sources.len() + 1);
        let title = run(&inp.title)?;
        // G4: a long material sent whole goes with the passages that bear on the section.
        let section_title = structure
            .sections()
            .find(|s| s.key == section_key)
            .map_or("", |s| s.title.as_str());
        let cut = match feed {
            dv_domain::Feed::Whole if inp.kind != InputKind::TestScores => {
                sorting::trimmed(&inp.content, section_key, section_title)
            }
            _ => None,
        };
        let (feed, kind) = match (feed, cut) {
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
            (f @ dv_domain::Feed::Passages(_), _) => {
                (f, format!("{} · קטעים שנבחרו לסעיף", inp.kind.label_he()))
            }
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
        source_rows.push((sid, inp.id, content.tagged));
    }
    Ok((tagged_sources, source_rows))
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
