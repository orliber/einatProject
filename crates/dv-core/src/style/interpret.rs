//! The interpretation of scores (round 33, K5): what Einat's past reports say about the
//! score of each test or subtest, learned from all of them in one request and kept only once
//! she approves it.
//!
//! The sentences go out neutralized, as in the style analysis (D-042), grouped in code by the
//! subtest named beside each score tag. What comes back passes the filter unchanged, shares
//! no run of [`PROFILE_NGRAM`] words with any report and holds no digit, or it is dropped.

use dv_ai::{Interpretation, InterpretationGroup, RawInterpretation};

use super::*;

/// A sentence sent for a subtest, at most (characters).
const MAX_SENTENCE_CHARS: usize = 400;
/// Sentences sent for one subtest, at most.
const MAX_SENTENCES: usize = 8;
/// The whole request's sentences, at most (characters).
const MAX_INTERPRETATION_CHARS_SENT: usize = 40_000;
/// What she may write herself for one interpretation (characters).
const MAX_OWN_CHARS: usize = 600;

/// An interpretation on her style screen, with the names of its test and subtest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InterpretationView {
    pub item: Interpretation,
    pub instrument_name: String,
    pub measure_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InterpretationResult {
    pub kept: u32,
    pub dropped: u32,
    pub demo: bool,
}

/// A run of this many words of an interpretation in a paragraph means it was used there.
const USED_NGRAM: usize = 4;

/// Her approved interpretations that fit this case's scores, as they go into the drafting
/// request of a section written from the scores (round 37): for each score, the measure in
/// general, its range, and a relative strength or weakness. `None`: none fits, or the profile
/// is switched off.
pub(crate) fn interpretations_for_request(
    v: &Vault,
    sheets: &[dv_domain::ScoreSheet],
) -> Result<Option<String>, CoreError> {
    if v.setting(PROFILE_OFF_KEY)?.as_deref() == Some("1") {
        return Ok(None);
    }
    let scores: Vec<dv_domain::ScoreCases> =
        sheets.iter().flat_map(dv_domain::sheet_cases).collect();
    let fits = |i: &Interpretation| {
        scores.iter().any(|c| {
            c.instrument == i.instrument && c.measure == i.measure && c.cases.contains(&i.case)
        })
    };
    let items: Vec<Interpretation> = local_style(v)?
        .interpretations
        .into_iter()
        .filter(|i| i.approved && i.enabled && fits(i))
        .collect();
    Ok(dv_ai::render_interpretations(&items, &|inst, measure| {
        scores
            .iter()
            .find(|c| c.instrument == inst && c.measure == measure)
            .map(|c| c.label.clone())
    }))
}

/// Her approved interpretations written for a case this child's score is not, to note in a
/// paragraph that uses one (round 37).
pub(crate) struct InterpretationCheck {
    /// (the interpretation's runs of words, its case, the score's label, the score's range).
    misfits: Vec<(HashSet<String>, String, String, Option<String>)>,
}

impl InterpretationCheck {
    pub(crate) fn new(v: &Vault, sheets: &[dv_domain::ScoreSheet]) -> Result<Self, CoreError> {
        let scores: Vec<dv_domain::ScoreCases> =
            sheets.iter().flat_map(dv_domain::sheet_cases).collect();
        let mut misfits = Vec::new();
        if !scores.is_empty() {
            for i in local_style(v)?.interpretations {
                if !i.approved || !i.enabled || i.case == dv_domain::GENERAL_CASE {
                    continue;
                }
                let Some(score) = scores
                    .iter()
                    .find(|c| c.instrument == i.instrument && c.measure == i.measure)
                else {
                    continue;
                };
                if !score.cases.contains(&i.case) {
                    misfits.push((
                        ngrams(&i.text, USED_NGRAM),
                        i.case,
                        score.label.clone(),
                        score.band.clone(),
                    ));
                }
            }
        }
        Ok(Self { misfits })
    }

    /// A note for each interpretation in `text` that was written for another case.
    pub(crate) fn notes(&self, text: &str) -> Vec<String> {
        self.misfits
            .iter()
            .filter(|(grams, ..)| shared_run(text, USED_NGRAM, grams).is_some())
            .map(|(_, case, label, band)| match band {
                Some(b) => format!(
                    "בפסקה יש פרשנות שכתבת ל«{case}» ב{label}, אבל כאן התוצאה בטווח «{b}». כדאי לבדוק שהיא מתאימה."
                ),
                None => format!(
                    "בפסקה יש פרשנות שכתבת ל«{case}» ב{label}, והיא לא מתאימה לתוצאה כאן. כדאי לבדוק."
                ),
            })
            .collect()
    }
}

/// "WPPSI-IV", "קוביות" for the keys of the fixed tables.
pub(crate) fn measure_names(instrument: &str, measure: &str) -> Option<(String, String)> {
    dv_domain::instruments()
        .into_iter()
        .find(|i| i.key == instrument)
        .and_then(|i| {
            i.measures
                .iter()
                .find(|m| m.key == measure)
                .map(|m| (i.name.clone(), m.name_he.clone()))
        })
}

/// The sentence of `text` around the byte range `start..end`.
fn sentence_around(text: &str, start: usize, end: usize) -> &str {
    let before = text.get(..start).unwrap_or_default();
    let from = before
        .char_indices()
        .rev()
        .find(|(i, c)| {
            *c == '\n'
                || (matches!(c, '.' | '!' | '?' | ';')
                    && before.get(i + 1..).is_some_and(|r| r.starts_with(' ')))
        })
        .map_or(0, |(i, c)| i + c.len_utf8());
    let after = text.get(end..).unwrap_or_default();
    let to = after
        .char_indices()
        .find(|(i, c)| {
            *c == '\n'
                || (matches!(c, '.' | '!' | '?' | ';')
                    && after
                        .get(i + 1..)
                        .is_none_or(|r| r.is_empty() || r.starts_with([' ', '\n'])))
        })
        .map_or(text.len(), |(i, c)| end + i + c.len_utf8());
    text.get(from..to).unwrap_or_default().trim()
}

/// A subtest (instrument and measure keys) and its sentences: `(report number, text)`.
type Group = (String, String, Vec<(u32, String)>);

/// Each subtest with a score tag in the reports, and its sentences. Reports are numbered from
/// 1 in the order given; the groups follow the fixed tables.
pub(crate) fn group_sentences(reports: &[String]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (n, text) in reports.iter().enumerate() {
        let report = u32::try_from(n + 1).unwrap_or(u32::MAX);
        for t in dv_domain::find_tagged_scores(text) {
            let sentence: String = sentence_around(text, t.start, t.end)
                .chars()
                .take(MAX_SENTENCE_CHARS)
                .collect();
            let at = match groups
                .iter()
                .position(|(i, m, _)| *i == t.instrument && *m == t.measure)
            {
                Some(at) => at,
                None => {
                    groups.push((t.instrument.clone(), t.measure.clone(), Vec::new()));
                    groups.len() - 1
                }
            };
            if let Some((_, _, list)) = groups.get_mut(at) {
                if list.len() < MAX_SENTENCES && !list.iter().any(|(_, s)| *s == sentence) {
                    list.push((report, sentence));
                }
            }
        }
    }
    let order: Vec<(String, String)> = dv_domain::instruments()
        .into_iter()
        .flat_map(|i| {
            let key = i.key.clone();
            i.measures.into_iter().map(move |m| (key.clone(), m.key))
        })
        .collect();
    groups.sort_by_key(|(i, m, _)| {
        order
            .iter()
            .position(|(a, b)| a == i && b == m)
            .unwrap_or(usize::MAX)
    });
    groups
}

fn source_text(s: &StoredSource) -> String {
    s.parts
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// How many subtests have a score in her reports (shown before she asks for the reading).
pub(super) fn scored_measures(sources: &[StoredSource]) -> u32 {
    let texts: Vec<String> = sources.iter().map(source_text).collect();
    count(group_sentences(&texts).len())
}

impl Core {
    /// The interpretations on her style screen.
    pub(super) fn interpretation_views(v: &Vault) -> Result<Vec<InterpretationView>, CoreError> {
        Ok(local_style(v)?
            .interpretations
            .into_iter()
            .filter_map(|item| {
                measure_names(&item.instrument, &item.measure).map(|(i, m)| InterpretationView {
                    item,
                    instrument_name: i,
                    measure_name: m,
                })
            })
            .collect())
    }

    /// Every subtest with a score in her reports, its sentences neutralized again now (a name
    /// declared since the upload is hidden too), on the review screen, through the gate.
    pub fn prepare_interpretations(&mut self) -> Result<Prepared, CoreError> {
        let model = self.model_for(Task::Draft)?;
        let demo_mode = self.demo_mode()?;
        let sources: Vec<StoredSource> = self.stored_sources()?.into_iter().map(|s| s.2).collect();
        let (groups, review) = self.with_style_ctx(|ctx, _| {
            let mut texts = Vec::new();
            for s in &sources {
                let mut parts = Vec::new();
                for p in &s.parts {
                    parts.push(neutralize(&p.text, ctx, None)?.0);
                }
                texts.push(parts.join("\n"));
            }
            let mut review = Review::default();
            let mut groups = Vec::new();
            let mut budget = MAX_INTERPRETATION_CHARS_SENT;
            for (instrument, measure, sentences) in group_sentences(&texts) {
                let Some((iname, mname)) = measure_names(&instrument, &measure) else {
                    continue;
                };
                let joined: Vec<&str> = sentences.iter().map(|(_, s)| s.as_str()).collect();
                let joined = joined.join("\n");
                let size = joined.chars().count();
                if size > budget {
                    break;
                }
                budget -= size;
                let label = format!("{iname} · {mname}");
                let out = filter(&joined, ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
                review.add(format!("פרשנות התוצאות · {label}"), &out);
                let lines: Vec<&str> = out.tagged.split('\n').collect();
                if lines.len() != sentences.len() {
                    return Err(CoreError::Internal("sentence count changed".to_owned()));
                }
                groups.push(InterpretationGroup {
                    cases: dv_domain::interpretation_cases(&instrument, &measure),
                    instrument,
                    measure,
                    label,
                    sentences: sentences
                        .iter()
                        .zip(lines)
                        .map(|((r, _), l)| (*r, l.to_owned()))
                        .collect(),
                });
            }
            Ok((groups, review))
        })?;
        if groups.is_empty() {
            return Err(CoreError::Refused(
                "בדוחות הישנים אין עדיין תוצאה ליד שם של מבחן או תת-מבחן.".to_owned(),
            ));
        }
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let body = dv_ai::build_interpretation_request(&model, &groups, &nonce);
        let mut prepared = review.into_prepared(demo_mode);
        self.gate_style(&body, PendingKind::Interpretation { groups }, &mut prepared)?;
        Ok(prepared)
    }

    pub fn send_interpretations(
        &mut self,
        approval_id: &str,
    ) -> Result<InterpretationResult, CoreError> {
        let out = self.begin_send(approval_id, crate::SendKind::Interpretation)?;
        let response = out.transmit();
        self.finish_interpretations(out, response)
    }

    /// The new readings wait for her approval. What she approved or wrote stays; the earlier
    /// readings she did not approve are replaced.
    pub fn finish_interpretations(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<InterpretationResult, CoreError> {
        let crate::stage::Received {
            kind,
            response,
            demo,
            ..
        } = self.receive(out, response, crate::stage::Stage::Interpretation)?;
        let PendingKind::Interpretation { groups } = kind else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let (items, shape_dropped) = dv_ai::parse_interpretations(&response, &groups)?;
        let (ids, sources): (Vec<String>, Vec<StoredSource>) = self
            .stored_sources()?
            .into_iter()
            .map(|s| (s.0, s.2))
            .unzip();
        let children = super::child_groups(self.vault_ref()?, &ids)?;
        let index = source_index(&sources, &children, PROFILE_NGRAM);
        let parsed = items.len();
        let kept: Vec<RawInterpretation> = self.with_style_ctx(|ctx, _| {
            Ok(items
                .into_iter()
                .filter(|i| {
                    clean(&i.text, ctx) && shared_run(&i.text, PROFILE_NGRAM, &index).is_none()
                })
                .collect())
        })?;
        let dropped = count(shape_dropped) + count(parsed - kept.len());
        let v = self.vault_mut()?;
        let mut local = local_style(v)?;
        local.interpretations.retain(|i| i.approved || i.edited);
        let mut added = 0;
        for i in kept {
            let known = local.interpretations.iter().any(|o| {
                o.instrument == i.instrument && o.measure == i.measure && o.text == i.text
            });
            if known {
                continue;
            }
            local.interpretations.push(Interpretation {
                id: dv_vault::crypto::random_id()?,
                instrument: i.instrument,
                measure: i.measure,
                case: i.case,
                text: i.text,
                support: i.support,
                approved: false,
                enabled: true,
                edited: false,
            });
            added += 1;
        }
        v.set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        Ok(InterpretationResult {
            kept: added,
            dropped,
            demo,
        })
    }

    /// Her decision on one interpretation: approve it, turn it off, or write it her way. Her
    /// own words are checked like what comes back: nothing identifying, no tag.
    pub fn update_interpretation(
        &mut self,
        id: &str,
        text: Option<String>,
        approved: Option<bool>,
        enabled: Option<bool>,
    ) -> Result<InterpretationView, CoreError> {
        let text = match text {
            Some(t) => {
                let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                if t.is_empty() || t.chars().count() > MAX_OWN_CHARS {
                    return Err(CoreError::Refused(format!(
                        "הפרשנות צריכה להיות באורך של עד {MAX_OWN_CHARS} תווים."
                    )));
                }
                let ok =
                    !t.contains(['[', ']']) && self.with_style_ctx(|ctx, _| Ok(clean(&t, ctx)))?;
                if !ok {
                    return Err(CoreError::Refused(
                        "בטקסט יש משהו שנראה כמו שם או פרט מזהה. הפרשנות נשמרת במילים כלליות בלבד."
                            .to_owned(),
                    ));
                }
                Some(t)
            }
            None => None,
        };
        let v = self.vault_mut()?;
        let mut local = local_style(v)?;
        let item = local
            .interpretations
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| CoreError::NotFound("הפרשנות".to_owned()))?;
        if let Some(t) = text {
            if t != item.text {
                item.text = t;
                item.edited = true;
            }
        }
        if let Some(a) = approved {
            item.approved = a;
        }
        if let Some(e) = enabled {
            item.enabled = e;
        }
        let item = item.clone();
        v.set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        let (instrument_name, measure_name) = measure_names(&item.instrument, &item.measure)
            .ok_or_else(|| CoreError::NotFound("תת-המבחן".to_owned()))?;
        Ok(InterpretationView {
            item,
            instrument_name,
            measure_name,
        })
    }

    pub fn delete_interpretation(&mut self, id: &str) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        let mut local = local_style(v)?;
        let before = local.interpretations.len();
        local.interpretations.retain(|i| i.id != id);
        if local.interpretations.len() == before {
            return Err(CoreError::NotFound("הפרשנות".to_owned()));
        }
        v.set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        Ok(())
    }
}
