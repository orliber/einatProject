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
        // The same fix loop as a case request (round 26, Z8): what the gate refuses is hidden
        // in the outgoing copy, with a book of this request only (her past reports have no
        // case to keep it in), and the copy goes back to the same gate.
        let book = dv_privacy::MaskBook::default();
        let answered = self.with_style_ctx(|ctx, _| {
            let tags = style_tags();
            Ok(crate::send::answer_refusals(
                body, ctx, &tags, suspects, &book,
            ))
        })?;
        let crate::send::Answered {
            result,
            fixed,
            cuts,
            mut codes,
        } = answered;
        if (!fixed.is_empty() || !cuts.is_empty()) && result.is_ok() {
            codes.sort();
            codes.dedup();
            self.vault_mut()?.record(
                AuditEvent::Blocked,
                None,
                &serde_json::json!({ "codes": codes, "style": true, "fixed": true }),
            )?;
            if !fixed.is_empty() {
                crate::send::note_fixed(prepared, &fixed);
            }
            if !cuts.is_empty() {
                crate::send::note_cut(prepared, &cuts);
            }
        }
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
        let model = self.model_for(Task::Light)?;
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
        // The patterns of the report's scores, from its tags (D-100): worked out here, so the
        // model only describes how she reads each one.
        let text: String = source
            .parts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let patterns = dv_domain::report_patterns(&dv_domain::find_tagged_scores(&text));
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let body = dv_ai::build_style_analysis_request(&model, &keys, &excerpts, &patterns, &nonce);
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
        let (ids, sources): (Vec<String>, Vec<StoredSource>) = self
            .stored_sources()?
            .into_iter()
            .map(|s| (s.0, s.2))
            .unzip();
        let children = super::child_groups(self.vault_ref()?, &ids)?;
        let index = source_index(&sources, &children, PROFILE_NGRAM);
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

    /// The new profile is a draft: nothing is used until she approves it. Her decisions carry
    /// over from the profile in use and from a draft she was going through (C1, D-166): what
    /// she wrote, reworded, accepted or learned from her edits stays as it is; a newly learned
    /// item like one she switched off comes switched off; one like an item she deleted, or
    /// like the earlier wording of one she reworded, is left out. Support is at most the
    /// number of reports analysed.
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
        let decided = self.decided_profiles()?;
        let learning = self.learning()?;
        let local = local_style(self.vault_ref()?)?;
        let mut new_items = Vec::new();
        for i in kept {
            new_items.push(StyleItem {
                id: dv_vault::crypto::random_id()?,
                section: i.section,
                kind: i.kind,
                text: i.text,
                enabled: true,
                origin: StyleOrigin::Reports,
                support: i.support.min(reports.max(1)),
                pattern: i.pattern,
            });
        }
        let mut stored = carry_decisions(new_items, reports, &decided, &|item| {
            accepted_earlier(item, &learning, &local)
        });
        stored.dropped = count(shape_dropped) + privacy_dropped;
        stored.demo = demo;
        self.store_draft(&stored)
    }

    /// Where her decisions are: the profile in use, then a draft she was going through (its
    /// items, by id, over the ones in use).
    fn decided_profiles(&mut self) -> Result<Vec<StoredProfile>, CoreError> {
        let profiles = self.vault_ref()?.style_profiles()?;
        let mut out = Vec::new();
        for status in ["active", "draft"] {
            if let Some(p) = profiles.iter().find(|p| p.status == status) {
                out.push(parse::<StoredProfile>(&p.json)?);
            }
        }
        Ok(out)
    }
}

/// A new profile of `new_items` (learned now from `reports` reports), with her decisions in
/// `decided` (oldest first) carried over (C1). `accepted_earlier`: an item of an older profile
/// she accepted from what was found on her computer, stored then as learned from her reports.
pub(super) fn carry_decisions(
    new_items: Vec<StyleItem>,
    reports: u32,
    decided: &[StoredProfile],
    accepted_earlier: &dyn Fn(&StyleItem) -> bool,
) -> StoredProfile {
    // Every item she decided on, the latest version of each.
    let mut previous: Vec<StyleItem> = Vec::new();
    let mut originals: BTreeMap<String, String> = BTreeMap::new();
    let mut removed: Vec<RemovedItem> = Vec::new();
    for p in decided {
        for item in &p.profile.items {
            previous.retain(|x| x.id != item.id);
            previous.push(item.clone());
        }
        originals.extend(p.originals.clone());
        for r in &p.removed {
            if !removed.contains(r) {
                removed.push(r.clone());
            }
        }
    }
    let extra = removed.len().saturating_sub(MAX_REMOVED);
    removed.drain(..extra);
    // Hers: kept whatever the new build says.
    let hers: Vec<StyleItem> = previous
        .iter()
        .cloned()
        .map(|mut i| {
            if accepted_earlier(&i) {
                i.origin = StyleOrigin::Accepted;
            }
            i
        })
        .filter(|i| i.origin != StyleOrigin::Reports)
        .collect();
    let switched_off: Vec<&StyleItem> = previous
        .iter()
        .filter(|i| i.origin == StyleOrigin::Reports && !i.enabled)
        .collect();
    let like = |i: &StyleItem, kind, section: Option<&str>, text: &str| {
        same_item(i.kind, i.section.as_deref(), &i.text, kind, section, text)
    };
    let mut items = Vec::new();
    for mut i in new_items {
        let gone = removed
            .iter()
            .any(|r| like(&i, r.kind, r.section.as_deref(), &r.text));
        // Her version stands for it: the same item, reworded by her or already hers.
        let in_hers = hers.iter().any(|h| {
            like(&i, h.kind, h.section.as_deref(), &h.text)
                || originals
                    .get(&h.id)
                    .is_some_and(|o| like(&i, h.kind, h.section.as_deref(), o))
        });
        if gone || in_hers {
            continue;
        }
        if switched_off
            .iter()
            .any(|d| like(&i, d.kind, d.section.as_deref(), &d.text))
        {
            i.enabled = false;
        }
        items.push(i);
    }
    originals.retain(|id, _| hers.iter().any(|h| h.id == *id));
    items.extend(hers);
    let mut stored = StoredProfile::new(StyleProfile { items, reports });
    stored.originals = originals;
    stored.removed = removed;
    stored
}
