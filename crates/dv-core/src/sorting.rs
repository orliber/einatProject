//! Which materials feed which section (D-022): the fixed table, the sorting by Claude (or
//! locally in demo mode), and Einat's own choice.
//!
//! Sorting goes out like any other request: filtered, through the review screen and the gate.
//! Claude answers with passage ids only. Afterwards a section draft sends only the passages
//! chosen for it, cut from a filter of the whole material, so every rule saw the full context.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use dv_ai::{SortInput, SortMaterial, SortSection};
use dv_domain::{
    passage_ranges, Feed, InputKind, ReportSection, ReportStructure, Routing, Suggestion,
};
use dv_privacy::{filter, filter_split, FilterOutcome, PrivacyContext};
use dv_vault::{AuditEvent, Vault, VaultError};
use serde_json::Value;

use crate::views::{MaterialRouting, Prepared, SortResult};
use crate::{today, Core, CoreError, Outgoing, Pending, PendingKind, Review, DERIVED_SECTIONS};

/// Sections that are written from materials (not from other sections, not the signature).
pub(crate) fn sortable(structure: &ReportStructure) -> Vec<&ReportSection> {
    structure
        .sections()
        .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()) && !s.inputs.is_empty())
        .collect()
}

/// The fixed table: the sortable sections a kind of material feeds by default.
pub(crate) fn table_for(structure: &ReportStructure, kind: InputKind) -> Vec<String> {
    sortable(structure)
        .into_iter()
        .filter(|s| s.inputs.contains(&kind))
        .map(|s| s.key.clone())
        .collect()
}

pub(crate) fn routing_of(v: &Vault, case_id: &str, input_id: &str) -> Result<Routing, CoreError> {
    Ok(v.input_routing(case_id, input_id)?
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|e| CoreError::Internal(e.to_string()))?
        .unwrap_or_default())
}

fn fingerprint(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// What goes from one material to one section, filtered with the whole material as context.
/// `None` when nothing goes. Falls back to the whole material when the passages cannot be cut
/// safely (a name across an edge).
pub(crate) fn section_text(
    content: &str,
    feed: &Feed,
    ctx: &PrivacyContext<'_>,
) -> Result<FilterOutcome, CoreError> {
    let internal = |e: dv_privacy::PrivacyError| CoreError::Internal(e.to_string());
    let Feed::Passages(chosen) = feed else {
        return filter(content, ctx).map_err(internal);
    };
    let ranges = passage_ranges(content);
    let (whole, parts) = filter_split(content, ctx, &ranges).map_err(internal)?;
    let Some(parts) = parts else {
        return Ok(whole);
    };
    let mut picked = Vec::new();
    let mut separators = Vec::new();
    let mut last = 0u32;
    for &n in chosen {
        let Some(part) = usize::try_from(n)
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| parts.get(i))
        else {
            continue;
        };
        if !picked.is_empty() {
            // Mark a gap so Claude does not read two distant passages as one text.
            separators.push(if n == last + 1 {
                "\n\n"
            } else {
                "\n\n(…)\n\n"
            });
        }
        picked.push(part);
        last = n;
    }
    if picked.is_empty() {
        return Ok(whole);
    }
    Ok(FilterOutcome::join(&picked, &separators))
}

impl Core {
    /// Where each material of a case goes, for the materials screen.
    pub(crate) fn material_routing(
        v: &Vault,
        structure: &ReportStructure,
        case_id: &str,
        inputs: &[dv_domain::CaseInput],
    ) -> Result<Vec<MaterialRouting>, CoreError> {
        let order: Vec<String> = sortable(structure).iter().map(|s| s.key.clone()).collect();
        inputs
            .iter()
            .map(|inp| {
                let routing = routing_of(v, case_id, &inp.id)?;
                let table = table_for(structure, inp.kind);
                let count = passage_ranges(&inp.content).len();
                let feeds = routing.feeds(&order, &table, count);
                let sorted = routing.is_sorted(count);
                let suggested = if sorted {
                    routing.base(&table, count)
                } else {
                    Vec::new()
                };
                let mut used: Vec<u32> = Vec::new();
                if sorted {
                    if let Some(s) = &routing.suggestion {
                        for sec in &s.sections {
                            if feeds.contains(&sec.section) {
                                used.extend(&sec.passages);
                            }
                        }
                    }
                    used.sort_unstable();
                    used.dedup();
                }
                Ok(MaterialRouting {
                    input_id: inp.id.clone(),
                    passages: u32::try_from(count).unwrap_or(u32::MAX),
                    used_passages: if sorted {
                        u32::try_from(used.len()).unwrap_or(u32::MAX)
                    } else {
                        u32::try_from(count).unwrap_or(u32::MAX)
                    },
                    by_ai: routing.suggestion.as_ref().is_some_and(|s| s.by_ai) && sorted,
                    feeds,
                    suggested,
                    table,
                    sorted,
                    needs_sorting: count > 0 && routing.needs_sorting(count),
                    added: routing.added,
                    removed: routing.removed,
                })
            })
            .collect()
    }

    /// Einat picked the sections a material feeds. Her choice always wins over the table and
    /// the sorting.
    pub fn set_input_sections(
        &mut self,
        case_id: &str,
        input_id: &str,
        sections: &[String],
    ) -> Result<(), CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let known: Vec<&str> = sortable(&structure)
            .iter()
            .map(|s| s.key.as_str())
            .collect();
        if let Some(bad) = sections.iter().find(|s| !known.contains(&s.as_str())) {
            return Err(CoreError::NotFound(bad.clone()));
        }
        let v = self.vault_mut()?;
        let input = v
            .inputs(case_id)?
            .into_iter()
            .find(|i| i.id == input_id)
            .ok_or_else(|| CoreError::NotFound(input_id.to_owned()))?;
        let mut routing = routing_of(v, case_id, input_id)?;
        let table = table_for(&structure, input.kind);
        routing.choose(sections, &table, passage_ranges(&input.content).len());
        let json =
            serde_json::to_string(&routing).map_err(|e| CoreError::Internal(e.to_string()))?;
        v.set_input_routing(case_id, input_id, &json)?;
        Ok(())
    }

    /// The text of a material changed: its passages are new, so the old sorting no longer fits.
    pub(crate) fn forget_sorting(
        &mut self,
        case_id: &str,
        input_id: &str,
    ) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        let mut routing = routing_of(v, case_id, input_id)?;
        if routing.suggestion.is_some() || routing.unplaced.is_some() {
            routing.text_changed();
            let json =
                serde_json::to_string(&routing).map_err(|e| CoreError::Internal(e.to_string()))?;
            v.set_input_routing(case_id, input_id, &json)?;
        }
        Ok(())
    }

    /// Build the sorting request for every material not sorted yet, with its review screen.
    pub fn prepare_sort(&mut self, case_id: &str) -> Result<Prepared, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let sections: Vec<SortSection> = sortable(&structure)
            .into_iter()
            .map(|s| SortSection {
                key: s.key.clone(),
                about: s.about.clone(),
            })
            .collect();
        let model = self.model_for(crate::Task::Sort)?;
        let data = self.privacy_data(case_id)?;
        let demo_mode = self.demo_mode()?;

        let (input, review, rows) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
            let is_name = |t: &str| data.is_name.contains(&v.token_hmac(t));
            let ctx = PrivacyContext {
                case_id: &data.case_id,
                identities: &data.identities,
                practitioner: &data.practitioner,
                allowlisted: &allow,
                confirmed_names: &is_name,
                today: today(),
            };
            let internal = |e: dv_privacy::PrivacyError| CoreError::Internal(e.to_string());
            let mut review = Review::default();
            let mut materials = Vec::new();
            let mut rows = Vec::new();
            // Materials that cannot be cut into passages right now (see below), by title.
            let mut uncut: Vec<String> = Vec::new();
            for inp in v.inputs(case_id)? {
                let ranges = passage_ranges(&inp.content);
                if ranges.is_empty()
                    || !routing_of(v, case_id, &inp.id)?.needs_sorting(ranges.len())
                {
                    continue;
                }
                let (whole, parts) = filter_split(&inp.content, &ctx, &ranges).map_err(internal)?;
                // A name across a passage edge: this material stays on the table for now.
                let Some(parts) = parts else {
                    uncut.push(inp.title.clone());
                    continue;
                };
                let title = filter(&inp.title, &ctx).map_err(internal)?;
                let sid = format!("S{}", materials.len() + 1);
                if title.original_segments.iter().any(|s| s.mark.is_some()) {
                    review.add(format!("{sid} · {} · כותרת", inp.kind.label_he()), &title);
                    review.add(format!("{sid} · {}", inp.kind.label_he()), &whole);
                } else {
                    review.add(
                        format!("{sid} · {} · {}", inp.kind.label_he(), title.tagged),
                        &whole,
                    );
                }
                materials.push(SortMaterial {
                    input_id: inp.id.clone(),
                    kind_label: inp.kind.label_he().to_owned(),
                    title_tagged: title.tagged,
                    passages_tagged: parts.into_iter().map(|p| p.tagged).collect(),
                });
                rows.push((inp.id.clone(), ranges.len(), fingerprint(&inp.content)));
            }
            if materials.is_empty() {
                // Shown only here, on this computer (titles may hold names).
                return Err(CoreError::Refused(if uncut.is_empty() {
                    "כל החומרים כבר ממוינים לסעיפים.".to_owned()
                } else {
                    format!(
                        "אי אפשר למיין אוטומטית את: {}. שם חשוד נמשך שם על פני שתי שורות, ולכן \
                         החומר לא נחתך לקטעים. אפשר לבחור לו סעיפים בעצמך (\"שינוי הסעיפים\").",
                        uncut.join(", ")
                    )
                }));
            }
            (
                SortInput {
                    sections,
                    materials,
                },
                review,
                rows,
            )
        };

        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let body = dv_ai::build_sort_request(&model, &input, &nonce);
        let mut prepared = review.into_prepared(demo_mode);
        let kind = PendingKind::Sort {
            case_id: case_id.to_owned(),
            materials: rows,
            sections: input.sections.iter().map(|s| s.key.clone()).collect(),
        };
        self.gate(&data, &body, kind, &mut prepared)?;
        Ok(prepared)
    }

    /// Send exactly what was approved and store the sorting.
    pub fn send_sort(&mut self, approval_id: &str) -> Result<SortResult, CoreError> {
        let out = self.begin_send(approval_id)?;
        let response = out.transmit();
        self.finish_sort(out, response)
    }

    /// Check Claude's sorting against what was sent and store it, material by material.
    pub fn finish_sort(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<SortResult, CoreError> {
        if !matches!(out.pending.kind, PendingKind::Sort { .. }) {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        }
        let (response, demo) = match response {
            Ok(r) => r,
            Err(e) => {
                self.restore_pending(out);
                return Err(e);
            }
        };
        self.note_usage(&out.pending.payload, &response, demo);
        let Pending { payload, kind } = out.pending;
        let PendingKind::Sort {
            case_id,
            materials,
            sections,
        } = kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        // The request went out: record it before anything about the reply can fail.
        let model = self.model_config()?.model;
        let payload_text = String::from_utf8_lossy(payload.body()).into_owned();
        let v = self.vault_mut()?;
        v.add_transmission(
            &case_id,
            "sorting",
            payload.sha256(),
            if demo { "demo" } else { &model },
            &payload_text,
        )?;
        v.record(
            AuditEvent::Send,
            Some(&case_id),
            &serde_json::json!({ "sorting": true, "demo": demo }),
        )?;
        let counts: Vec<usize> = materials.iter().map(|(_, n, _)| *n).collect();
        let reply = dv_ai::parse_sort(&response, &counts, &sections)?;
        let v = self.vault_mut()?;
        let current = v.inputs(&case_id)?;
        let mut result = SortResult {
            sorted: 0,
            unchanged: 0,
            links: 0,
            ignored: reply.ignored,
            demo,
        };
        for ((input_id, count, print), placed) in materials.iter().zip(reply.materials) {
            // Edited or deleted while Claude was reading: the answer is about another text.
            let still_same = current
                .iter()
                .any(|i| &i.id == input_id && fingerprint(&i.content) == *print);
            if !still_same {
                result.unchanged += 1;
                continue;
            }
            let mut routing = routing_of(v, &case_id, input_id)?;
            if placed.is_empty() {
                // Read and nothing found: keep the table, do not offer it again.
                routing.unplaced = Some(u32::try_from(*count).unwrap_or(u32::MAX));
                let json = serde_json::to_string(&routing)
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                match v.set_input_routing(&case_id, input_id, &json) {
                    Ok(()) | Err(VaultError::NotFound) => {}
                    Err(e) => return Err(e.into()),
                }
                result.unchanged += 1;
                continue;
            }
            result.links += u32::try_from(placed.len()).unwrap_or(u32::MAX);
            routing.suggestion = Some(Suggestion {
                by_ai: !demo,
                passage_count: u32::try_from(*count).unwrap_or(u32::MAX),
                sections: placed,
            });
            let json =
                serde_json::to_string(&routing).map_err(|e| CoreError::Internal(e.to_string()))?;
            match v.set_input_routing(&case_id, input_id, &json) {
                Ok(()) => result.sorted += 1,
                Err(VaultError::NotFound) => result.unchanged += 1,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(result)
    }
}
