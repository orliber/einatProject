//! Sending past reports for analysis, and the analyses for the profile (D-043).

use super::*;

impl Core {
    /// A gate of its own: the past reports' context, and only role tags.
    pub(super) fn gate_style(
        &mut self,
        body: &Value,
        kind: PendingKind,
        prepared: &mut Prepared,
    ) -> Result<(), CoreError> {
        let suspects = prepared.suspects.len();
        let result = self.with_style_ctx(|ctx, _| {
            let tags = style_tags();
            Ok(clear(&GateRequest {
                body,
                ctx,
                case_tags: &tags,
                unresolved_suspects: suspects,
                canaries: &[],
                max_bytes: MAX_REQUEST_BYTES,
            }))
        })?;
        match result {
            Ok(payload) => {
                let id = payload.sha256().to_owned();
                prepared.approval_id = Some(id.clone());
                self.pending.insert(id, Pending { payload, kind });
            }
            Err(blocked) => {
                let codes: Vec<String> = blocked.reasons.iter().map(|r| r.code.clone()).collect();
                self.vault_mut()?.record(
                    AuditEvent::Blocked,
                    None,
                    &serde_json::json!({ "codes": codes, "style": true }),
                )?;
                prepared.blocked = blocked.reasons;
            }
        }
        Ok(())
    }

    /// Step 1 for one past report: its excerpts, neutralized again now (a name declared since
    /// the upload is hidden too), on the review screen, through the gate.
    pub fn prepare_style_analysis(&mut self, source_id: &str) -> Result<Prepared, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let keys = section_keys(&structure);
        let model = self.model_for(Task::Draft)?;
        let demo_mode = self.demo_mode()?;
        let stored = self
            .vault_ref()?
            .style_source(source_id)?
            .ok_or_else(|| CoreError::NotFound("הדוח".to_owned()))?;
        let source: StoredSource = parse(&stored.json)?;
        let (excerpts, review) = self.with_style_ctx(|ctx, _| {
            let mut review = Review::default();
            let mut excerpts = Vec::new();
            let mut budget = MAX_EXCERPT_CHARS;
            for p in &source.parts {
                if budget == 0 {
                    break;
                }
                let (text, _, _) = neutralize(&p.text, ctx, None)?;
                let text: String = text.chars().take(budget).collect();
                budget = budget.saturating_sub(text.chars().count());
                let out = filter(&text, ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
                review.add(format!("{} · {}", source.title, p.heading), &out);
                excerpts.push(StyleExcerpt {
                    section: p
                        .section
                        .clone()
                        .filter(|k| keys.contains(k))
                        .unwrap_or_else(|| dv_ai::style::GENERAL.to_owned()),
                    text: out.tagged,
                });
            }
            Ok((excerpts, review))
        })?;
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let body = dv_ai::build_style_analysis_request(&model, &keys, &excerpts, &nonce);
        let mut prepared = review.into_prepared(demo_mode);
        self.gate_style(
            &body,
            PendingKind::StyleAnalysis {
                source_id: source_id.to_owned(),
            },
            &mut prepared,
        )?;
        Ok(prepared)
    }

    pub fn send_style_analysis(
        &mut self,
        approval_id: &str,
    ) -> Result<StyleAnalysisResult, CoreError> {
        let out = self.begin_send(approval_id, crate::SendKind::StyleAnalysis)?;
        let response = out.transmit();
        self.finish_style_analysis(out, response)
    }

    /// Keep what passes every local check: shape (dv-ai), the filter, and no run of
    /// [`PROFILE_NGRAM`] words shared with any past report. Returns the kept items and how
    /// many were dropped here.
    pub(super) fn keep_items(
        &mut self,
        items: Vec<RawStyleItem>,
    ) -> Result<(Vec<RawStyleItem>, u32), CoreError> {
        let sources: Vec<StoredSource> = self.stored_sources()?.into_iter().map(|s| s.2).collect();
        let index = source_index(&sources, PROFILE_NGRAM);
        self.with_style_ctx(|ctx, _| {
            let before = items.len();
            let kept: Vec<RawStyleItem> = items
                .into_iter()
                .filter(|i| {
                    clean(&i.text, ctx) && shared_run(&i.text, PROFILE_NGRAM, &index).is_none()
                })
                .collect();
            let dropped = count(before - kept.len());
            Ok((kept, dropped))
        })
    }

    pub fn finish_style_analysis(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<StyleAnalysisResult, CoreError> {
        // Counted toward the month now too (it was not before the stages shared `receive`).
        let crate::stage::Received {
            kind,
            response,
            demo,
            ..
        } = self.receive(out, response, crate::stage::Stage::StyleAnalysis)?;
        let PendingKind::StyleAnalysis { source_id } = kind else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let (items, shape_dropped) =
            dv_ai::parse_style_analysis(&response, &section_keys(&structure))?;
        let (kept, privacy_dropped) = self.keep_items(items)?;
        let dropped = count(shape_dropped) + privacy_dropped;
        // The report may have been deleted while Claude answered: then there is nothing to keep.
        let Some(stored) = self.vault_ref()?.style_source(&source_id)? else {
            return Err(CoreError::NotFound("הדוח נמחק בינתיים.".to_owned()));
        };
        let mut source: StoredSource = parse(&stored.json)?;
        source.analysis = Some(StoredAnalysis {
            items: kept.iter().map(SavedItem::from).collect(),
            dropped,
            demo,
            at: unix_now(),
        });
        self.vault_mut()?
            .save_style_source(Some(&source_id), &json(&source)?)?;
        Ok(StyleAnalysisResult {
            source_id,
            kept: count(kept.len()),
            dropped,
            demo,
        })
    }

    /// Step 2: the analyses only (no report text), on the review screen, through the gate.
    pub fn prepare_style_profile(&mut self) -> Result<Prepared, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let keys = section_keys(&structure);
        let model = self.model_for(Task::Draft)?;
        let demo_mode = self.demo_mode()?;
        let analyzed: Vec<(String, Vec<RawStyleItem>)> = self
            .stored_sources()?
            .into_iter()
            .filter_map(|(_, _, s)| {
                s.analysis
                    .map(|a| (s.title, a.items.iter().map(RawStyleItem::from).collect()))
            })
            .filter(|(_, items): &(String, Vec<RawStyleItem>)| !items.is_empty())
            .collect();
        if analyzed.is_empty() {
            return Err(CoreError::Refused(
                "קודם צריך לנתח לפחות דוח אחד.".to_owned(),
            ));
        }
        let review = self.with_style_ctx(|ctx, _| {
            let mut review = Review::default();
            for (title, items) in &analyzed {
                let lines: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
                let out = filter(&lines.join("\n"), ctx)
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                review.add(format!("ניתוח הסגנון · {title}"), &out);
            }
            Ok(review)
        })?;
        let analyses: Vec<Vec<RawStyleItem>> = analyzed.into_iter().map(|(_, i)| i).collect();
        let reports = count(analyses.len());
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let body = dv_ai::build_style_synthesis_request(&model, &keys, &analyses, &nonce);
        let mut prepared = review.into_prepared(demo_mode);
        self.gate_style(
            &body,
            PendingKind::StyleSynthesis { reports },
            &mut prepared,
        )?;
        Ok(prepared)
    }

    pub fn send_style_profile(&mut self, approval_id: &str) -> Result<StyleProfileView, CoreError> {
        let out = self.begin_send(approval_id, crate::SendKind::StyleSynthesis)?;
        let response = out.transmit();
        self.finish_style_profile(out, response)
    }

    /// The new profile is a draft: nothing is used until she approves it. Her own rules and
    /// the ones she accepted from her edits carry over from the approved profile.
    pub fn finish_style_profile(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<StyleProfileView, CoreError> {
        let crate::stage::Received {
            kind,
            response,
            demo,
            ..
        } = self.receive(out, response, crate::stage::Stage::StyleSynthesis)?;
        let PendingKind::StyleSynthesis { reports } = kind else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let (items, shape_dropped) =
            dv_ai::parse_style_synthesis(&response, &section_keys(&structure))?;
        let (kept, privacy_dropped) = self.keep_items(items)?;
        let mut profile = StyleProfile {
            items: Vec::new(),
            reports,
        };
        for i in kept {
            profile.items.push(StyleItem {
                id: dv_vault::crypto::random_id()?,
                section: i.section,
                kind: i.kind,
                text: i.text,
                enabled: true,
                origin: StyleOrigin::Reports,
                support: i.support,
            });
        }
        if let Some(active) = self.active_profile()? {
            profile.items.extend(
                active
                    .profile
                    .items
                    .into_iter()
                    .filter(|i| i.origin != StyleOrigin::Reports),
            );
        }
        let stored = StoredProfile {
            profile,
            dropped: count(shape_dropped) + privacy_dropped,
            demo,
        };
        self.store_draft(&stored)
    }
}
