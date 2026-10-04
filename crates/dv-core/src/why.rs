//! "Why did you write this?" (AI-7): for one paragraph, the passages it leans on, shown on this
//! computer only. Nothing here is sent anywhere.
//!
//! A paragraph written from materials carries the materials it cited; of each one's passages
//! that went to the section, those sharing the most words with the paragraph come first. A
//! summary-like section has no materials: its sources are the approved paragraphs of the other
//! sections, chosen the same way.

use std::collections::HashSet;

use dv_domain::{passages, DraftStatus, Feed, ReportStructure};
use dv_privacy::restore::restore;

use crate::views::SourceExcerpt;
use crate::{sorting, Core, CoreError, DERIVED_SECTIONS};

/// Passages shown per material, and in all.
const PER_SOURCE: usize = 3;
const AT_MOST: usize = 6;

/// The words that say what a text is about: three letters and more, without one leading
/// Hebrew prefix letter ("ובגן" and "הגן" both count as "גן" + its forms).
fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .flat_map(|w| {
            let stripped = w
                .strip_prefix(['ו', 'ב', 'ה', 'ל', 'מ', 'ש', 'כ'])
                .filter(|r| r.chars().count() >= 3)
                .map(str::to_owned);
            std::iter::once(w.to_owned()).chain(stripped)
        })
        .collect()
}

/// Candidates ordered by how many of the paragraph's words they share; ties keep text order.
fn ranked(
    paragraph: &HashSet<String>,
    candidates: Vec<(String, String)>,
) -> Vec<(usize, String, String)> {
    let mut scored: Vec<(usize, usize, String, String)> = candidates
        .into_iter()
        .enumerate()
        .map(|(i, (label, text))| (words(&text).intersection(paragraph).count(), i, label, text))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(s, _, l, t)| (s, l, t)).collect()
}

impl Core {
    /// The passages one paragraph was written from, best match first, with real names (they
    /// never leave the computer). Empty for a paragraph she wrote herself.
    pub fn paragraph_sources(
        &mut self,
        case_id: &str,
        draft_id: &str,
    ) -> Result<Vec<SourceExcerpt>, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let v = self.vault_ref()?;
        let identities = v.identities(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let show = |t: &str| restore(t, &identities, practitioner.as_deref());
        let mut found = None;
        for s in structure.sections() {
            if let Some(d) = v
                .drafts(case_id, &s.key)?
                .into_iter()
                .find(|d| d.id == draft_id)
            {
                found = Some((s.key.clone(), d));
                break;
            }
        }
        let (section_key, draft) = found.ok_or_else(|| CoreError::NotFound("הפסקה".to_owned()))?;
        let paragraph = words(&show(&draft.text_tagged));

        let mut out = Vec::new();
        if DERIVED_SECTIONS.contains(&section_key.as_str()) {
            let mut candidates = Vec::new();
            for s in structure
                .sections()
                .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()))
            {
                for d in v
                    .drafts(case_id, &s.key)?
                    .into_iter()
                    .filter(|d| d.status == DraftStatus::Approved)
                {
                    candidates.push((format!("סעיף מאושר · {}", s.title), show(&d.text_tagged)));
                }
            }
            for (score, label, text) in ranked(&paragraph, candidates).into_iter().take(AT_MOST) {
                if score > 0 {
                    out.push(SourceExcerpt { label, text });
                }
            }
            return Ok(out);
        }

        let inputs = v.inputs(case_id)?;
        for input_id in &draft.source_refs {
            let Some(input) = inputs.iter().find(|i| &i.id == input_id) else {
                continue;
            };
            let all = passages(&input.content);
            let table = sorting::table_for(&structure, input.kind);
            let feed = sorting::routing_of(v, case_id, &input.id)?
                .feed(&section_key, &table, all.len())
                .unwrap_or(Feed::Whole);
            let label = if input.title.trim().is_empty() {
                input.kind.label_he().to_owned()
            } else {
                format!("{} · {}", input.kind.label_he(), input.title)
            };
            let candidates: Vec<(String, String)> = all
                .iter()
                .enumerate()
                .filter(|(i, _)| match &feed {
                    Feed::Whole => true,
                    Feed::Passages(chosen) => {
                        u32::try_from(i + 1).is_ok_and(|n| chosen.contains(&n))
                    }
                })
                .map(|(_, p)| (label.clone(), p.clone()))
                .collect();
            let best = ranked(&paragraph, candidates);
            let matching: Vec<_> = best
                .iter()
                .filter(|(s, _, _)| *s > 0)
                .take(PER_SOURCE)
                .collect();
            // Nothing in common (a paragraph that sums up): the first passages, as they were sent.
            let chosen: Vec<_> = if matching.is_empty() {
                best.iter().take(1).collect()
            } else {
                matching
            };
            for (_, label, text) in chosen {
                out.push(SourceExcerpt {
                    label: label.clone(),
                    text: text.clone(),
                });
            }
        }
        out.truncate(AT_MOST);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_passage_sharing_the_most_words_comes_first() {
        let p = words("[ילד] מתקשה במעברים בין פעילויות בגן.");
        let r = ranked(
            &p,
            vec![
                ("א".into(), "ההורים מספרים על שינה טובה.".into()),
                ("ב".into(), "הגננת מתארת קושי במעברים בין פעילויות.".into()),
            ],
        );
        assert_eq!(r[0].1, "ב");
        assert!(r[0].0 >= 2 && r[1].0 == 0, "{r:?}");
    }
}
