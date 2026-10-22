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
    passage_ranges, CaseInput, DocDate, Feed, InputKind, MaterialSummary, ReportSection,
    ReportStructure, Routing, Setting, Suggestion,
};
use dv_privacy::{FilterOutcome, PrivacyContext};
use dv_vault::{Vault, VaultError};
use serde_json::Value;

use crate::name_model::NameHints;
use crate::views::{MaterialRouting, Prepared, PriorRecordView, SortResult};
use crate::{
    restore, scan_model_output, structure_for, Core, CoreError, Outgoing, PendingKind, Review,
    DERIVED_SECTIONS,
};

/// The section of previous assessments, written in an order fixed in code (round 31).
pub(crate) const PRIOR_SECTION: &str = "prior_assessments";

/// Materials whose reading includes a record of a previous assessment or treatment.
fn has_record(kind: InputKind) -> bool {
    matches!(kind, InputKind::PriorReport | InputKind::Professional)
}

/// Where a material stands in "אבחונים וטיפולים קודמים": the setting its record names, and
/// the date read from the document here.
pub(crate) fn prior_key(
    v: &Vault,
    case_id: &str,
    input: &CaseInput,
) -> Result<(Setting, Option<DocDate>), CoreError> {
    let count = passage_ranges(&input.content).len();
    let setting = routing_of(v, case_id, &input.id)?
        .summary_of(count)
        .and_then(|s| s.record.as_ref())
        .map_or(Setting::Unknown, |r| r.setting);
    Ok((setting, dv_domain::document_date(&input.content)))
}

/// Sections that are written from materials (not from other sections, not the signature).
pub(crate) fn sortable(structure: &ReportStructure) -> Vec<&ReportSection> {
    structure
        .sections()
        .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()) && !s.inputs.is_empty())
        // Her own sections take no material (round 30).
        .filter(|s| s.mode == dv_domain::SectionMode::Ai)
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

/// A material sent whole to a section is cut to the passages that bear on it above this many
/// words (G4), within [`TRIM_WORDS`]. Less of the case leaves, and the request is shorter.
const TRIM_ABOVE: usize = 700;
const TRIM_WORDS: usize = 500;

/// Words a passage about each section tends to use, besides its title (G4).
const SECTION_WORDS: &[(&str, &str)] = &[
    (
        "referral",
        "הפניה פנו פנייה סיבה קושי קשיים שאלה בקשה אבחון",
    ),
    (
        "background",
        "הריון לידה שבוע משקל התפתחות ישב הלך מילים ראשונות אכילה שינה בריאות רפואי",
    ),
    (
        "parents_view",
        "בבית הורים אמא אבא התנהגות משחק אחים שינה אכילה מעברים כעס",
    ),
    (
        "kindergarten",
        "גן גננת סייעת כיתה מורה חברים חצר מפגש פעילות קבוצה בית ספר",
    ),
    (
        "prior_assessments",
        "אבחון טיפול קלינאית ריפוי בעיסוק נוירולוג פסיכולוג הערכה דוח",
    ),
    (
        "appearance",
        "הגיע נכנס חדר מפגש קשר עין הופעה שיתף פעולה נפרד התרשמות",
    ),
    (
        "cognitive",
        "משימה מבחן ציון קשב ריכוז זיכרון חשיבה הבנה מילולי חזותי",
    ),
    (
        "adaptive",
        "שאלון הסתגלות עצמאות התלבשות אכילה מיומנויות יומיום",
    ),
    (
        "communication",
        "תקשורת שפה דיבור קשר עין הדדיות שיח משפטים אוצר מילים מחוות",
    ),
    (
        "behavior_attention",
        "קשב ריכוז שאלון התנהגות היפראקטיביות אימפולסיביות התפרצויות עכבה ארגון תכנון ויסות",
    ),
    (
        "emotional",
        "רגשי משחק סמלי ויסות תסכול חרדה פחד ביטחון עצמי רגשות",
    ),
];

/// The passages of a long material sent whole to `section_key` that bear on it, or `None`
/// when it is short, cut into few passages, or the section has no words to go by (G4).
pub(crate) fn trimmed(content: &str, section_key: &str, title: &str) -> Option<(Vec<u32>, usize)> {
    if content.split_whitespace().count() <= TRIM_ABOVE {
        return None;
    }
    let words = SECTION_WORDS.iter().find(|(k, _)| *k == section_key)?.1;
    let ranges = passage_ranges(content);
    if ranges.len() < 3 {
        return None;
    }
    let passages: Vec<&str> = ranges
        .iter()
        .map(|r| content.get(r.clone()).unwrap_or_default())
        .collect();
    let keep = dv_style::retrieve::within_words(&passages, &format!("{title} {words}"), TRIM_WORDS);
    if keep.len() == passages.len() {
        return None;
    }
    let chosen = keep
        .into_iter()
        .filter_map(|i| u32::try_from(i + 1).ok())
        .collect();
    Some((chosen, passages.len()))
}

/// What goes from one material to one section, filtered with the whole material as context.
/// `None` when nothing goes. Falls back to the whole material when the passages cannot be cut
/// safely (a name across an edge).
pub(crate) fn section_text(
    content: &str,
    feed: &Feed,
    ctx: &PrivacyContext<'_>,
    hints: &NameHints,
) -> Result<FilterOutcome, CoreError> {
    let internal = |e: dv_privacy::PrivacyError| CoreError::Internal(e.to_string());
    let Feed::Passages(chosen) = feed else {
        return hints.filter(content, ctx).map_err(internal);
    };
    let ranges = passage_ranges(content);
    let (whole, parts) = hints
        .filter_split(content, ctx, &ranges)
        .map_err(internal)?;
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
        let identities = v.identities(case_id)?;
        let masks = v.masks(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let show = |t: &str| restore(t, &identities, practitioner.as_deref(), &masks);
        let not_used = crate::instruments::not_used(v)?;
        inputs
            .iter()
            .map(|inp| {
                let routing = routing_of(v, case_id, &inp.id)?;
                let table = table_for(structure, inp.kind);
                let count = passage_ranges(&inp.content).len();
                // A score table of an instrument she does not give feeds nothing (D-098).
                let left_out = crate::instruments::left_out(v, case_id, inp, &not_used)?;
                let feeds = if left_out.is_some() {
                    Vec::new()
                } else {
                    routing.feeds(&order, &table, count)
                };
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
                let read = routing.summary_of(count);
                let record = read
                    .and_then(|s| s.record.as_ref())
                    .filter(|_| has_record(inp.kind))
                    .map(|r| PriorRecordView {
                        kind: show(&r.kind),
                        by: show(&r.by),
                        setting: r.setting,
                        findings: show(&r.findings),
                        recommendations: show(&r.recommendations),
                        date: dv_domain::document_date(&inp.content)
                            .map(dv_domain::date_text)
                            .unwrap_or_default(),
                    });
                let summary_notes = read.map(|s| s.notes.clone()).unwrap_or_default();
                Ok(MaterialRouting {
                    summary: read.map(|s| show(&s.text)).unwrap_or_default(),
                    record,
                    summary_notes,
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
                    needs_sorting: left_out.is_none() && count > 0 && routing.needs_sorting(count),
                    added: routing.added,
                    removed: routing.removed,
                    left_out,
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
        let structure = structure_for(self.vault_ref()?, case_id)?;
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
        self.prepare_sort_once(case_id, false)
    }

    fn prepare_sort_once(&mut self, case_id: &str, learned: bool) -> Result<Prepared, CoreError> {
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let sections: Vec<SortSection> = sortable(&structure)
            .into_iter()
            .map(|s| SortSection {
                key: s.key.clone(),
                about: s.about.clone(),
            })
            .collect();
        let model = self.model_for(crate::Task::Sort)?;
        let data = self.privacy_data(case_id)?;
        let hints = self.case_hints(case_id, &[])?;
        let demo_mode = self.demo_mode()?;

        let (input, review, rows) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let lookups = data.lookups(v);
            let ctx = lookups.masked_context();
            let internal = |e: dv_privacy::PrivacyError| CoreError::Internal(e.to_string());
            let mut review = Review::default();
            let mut materials = Vec::new();
            let mut rows = Vec::new();
            // Materials that cannot be cut into passages right now (see below), by title.
            let mut uncut: Vec<String> = Vec::new();
            // A score table of an instrument she does not give is not sorted (D-098).
            for inp in crate::instruments::request_inputs(v, case_id)? {
                let ranges = passage_ranges(&inp.content);
                if ranges.is_empty()
                    || !routing_of(v, case_id, &inp.id)?.needs_sorting(ranges.len())
                {
                    continue;
                }
                let (whole, parts) = hints
                    .filter_split(&inp.content, &ctx, &ranges)
                    .map_err(internal)?;
                // A name across a passage edge: this material stays on the table for now.
                let Some(parts) = parts else {
                    uncut.push(inp.title.clone());
                    continue;
                };
                let title = hints.filter(&inp.title, &ctx).map_err(internal)?;
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
                let passages_tagged: Vec<String> = parts.into_iter().map(|p| p.tagged).collect();
                rows.push((
                    inp.id.clone(),
                    ranges.len(),
                    fingerprint(&inp.content),
                    passages_tagged.join("\n\n"),
                ));
                materials.push(SortMaterial {
                    input_id: inp.id.clone(),
                    kind_label: inp.kind.label_he().to_owned(),
                    title_tagged: title.tagged,
                    passages_tagged,
                });
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
        let mut prepared = review.into_case_prepared(demo_mode, &data);
        let kind = PendingKind::Sort {
            case_id: case_id.to_owned(),
            materials: rows,
            sections: input.sections.iter().map(|s| s.key.clone()).collect(),
        };
        if self.gate(&data, &body, kind, &mut prepared, !learned)? {
            return self.prepare_sort_once(case_id, true);
        }
        Ok(prepared)
    }

    /// Send exactly what was approved and store the sorting.
    pub fn send_sort(&mut self, approval_id: &str) -> Result<SortResult, CoreError> {
        let out = self.begin_send(approval_id, crate::SendKind::Sort)?;
        let response = out.transmit();
        self.finish_sort(out, response)
    }

    /// Check Claude's sorting against what was sent and store it, material by material. A
    /// failure marks the case as stuck, like a section's.
    pub fn finish_sort(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<SortResult, CoreError> {
        let case_id = out.pending.kind.failing_case().map(str::to_owned);
        let result = self.store_sort(out, response);
        if let Some(case_id) = case_id {
            self.note_send_outcome(&case_id, result.is_ok());
        }
        result
    }

    fn store_sort(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<SortResult, CoreError> {
        let left_out = crate::send::had_left_out(&out);
        let crate::stage::Received {
            kind,
            response,
            demo,
            ..
        } = self.receive(out, response, crate::stage::Stage::Sort)?;
        let PendingKind::Sort {
            case_id,
            materials,
            sections,
        } = kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let counts: Vec<usize> = materials.iter().map(|(_, n, _, _)| *n).collect();
        let reply = dv_ai::parse_sort(&response, &counts, &sections)?;
        let v = self.vault_mut()?;
        let current = v.inputs(&case_id)?;
        let case_tags: Vec<String> = v.identities(&case_id)?.into_iter().map(|i| i.tag).collect();
        let everyone = v.all_identities()?;
        let mut result = SortResult {
            sorted: 0,
            unchanged: 0,
            links: 0,
            ignored: reply.ignored,
            demo,
            left_out,
        };
        for (((input_id, count, print, sent), placed), reading) in
            materials.iter().zip(reply.materials).zip(reply.readings)
        {
            // Edited or deleted while Claude was reading: the answer is about another text.
            let Some(input) = current
                .iter()
                .find(|i| &i.id == input_id && fingerprint(&i.content) == *print)
            else {
                result.unchanged += 1;
                continue;
            };
            let mut routing = routing_of(v, &case_id, input_id)?;
            let passage_count = u32::try_from(*count).unwrap_or(u32::MAX);
            // Round 31: what it says, kept with the text it was read from. Read and summed up
            // even when the answer said nothing about it, so it is not offered again.
            let reading = reading.unwrap_or_default();
            let record = reading.record.filter(|_| has_record(input.kind));
            let mut notes = Vec::new();
            let written = [
                Some(&reading.summary),
                record.as_ref().map(|r| &r.findings),
                record.as_ref().map(|r| &r.recommendations),
            ];
            let mut note = |n: String| {
                if !notes.contains(&n) {
                    notes.push(n);
                }
            };
            for text in written.into_iter().flatten() {
                for found in scan_model_output(text, &case_tags, &everyone) {
                    note(format!("{}: {}", found.message, found.token));
                }
                // A number that is neither in what was sent nor in the material itself.
                for n in dv_ai::numbers_not_in(text, &[sent, &input.content]) {
                    note(format!("המספר {n} לא מופיע בחומר – לבדוק"));
                }
            }
            routing.summary = Some(MaterialSummary {
                passage_count,
                text: reading.summary,
                record,
                notes,
            });
            let placed_any = !placed.is_empty();
            if placed_any {
                result.links += u32::try_from(placed.len()).unwrap_or(u32::MAX);
                routing.suggestion = Some(Suggestion {
                    by_ai: !demo,
                    passage_count,
                    sections: placed,
                });
            } else {
                // Read and nothing found: keep the table, do not offer it again.
                routing.unplaced = Some(passage_count);
            }
            let json =
                serde_json::to_string(&routing).map_err(|e| CoreError::Internal(e.to_string()))?;
            match v.set_input_routing(&case_id, input_id, &json) {
                Ok(()) if placed_any => result.sorted += 1,
                Ok(()) | Err(VaultError::NotFound) => result.unchanged += 1,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(result)
    }
}
