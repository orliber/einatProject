//! What her past reports offer beside a section (round 36), all on her computer and never
//! sent: the diagnoses and recommendations she gives again and again (K9, K10), each with how
//! often she gave it in reports like this case, and the reports most like this case, with
//! what they wrote in the section (K14).
//!
//! A report is described by a few features: the age band, the tests it names, the range of
//! each index score, and the kind of diagnosis. A case is described the same way, from its
//! age, its score tables and its diagnoses section. Two are alike by the share of features
//! they share (Jaccard).

use std::collections::BTreeSet;

use dv_domain::{CaseMeta, GrammaticalGender};

use super::*;

/// Lines of a list section offered back when they appear in at least this many reports.
const LIST_REPORTS: usize = 2;
/// A report is like the case when this share of their features is shared...
const ALIKE: f64 = 0.3;
/// ...and at most this many reports are shown.
const SIMILAR_SHOWN: usize = 3;
/// The longest excerpt shown from a similar report (characters).
const EXCERPT_CHARS: usize = 1_200;

/// The sections whose lines are offered as a list to choose from.
const LIST_SECTIONS: [&str; 2] = ["diagnoses", "recommendations"];

/// The kinds of diagnosis a report or a case is described by: (feature, its words).
const DIAGNOSES: &[(&str, &str)] = &[
    ("asd", "אוטיזם אוטיסטי הספקטרום asd"),
    ("adhd", "adhd קשב וריכוז היפראקטיביות"),
    ("language", "שפה שפתית דיבור"),
    ("motor", "מוטורי מוטורית קואורדינציה dcd"),
    ("anxiety", "חרדה חרדתית"),
    ("global", "גלובלי גלובלית שכלית"),
    ("emotional", "רגשית רגשי ויסות"),
];

/// A line of her diagnoses or recommendations, for her to choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryLine {
    pub text: String,
    /// In how many of her reports.
    pub reports: u32,
    /// In how many of the reports like this case...
    pub in_similar: u32,
    /// ...out of this many with the section.
    pub similar: u32,
}

/// One of her reports most like this case, with its text for the section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SimilarReport {
    /// The report's title as she saved it.
    pub report: String,
    /// What it shares with this case, in words.
    pub reasons: Vec<String>,
    /// The section as she wrote it there (neutralized, roles in place of names).
    pub text: String,
}

/// What her past reports offer beside one section of a case.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionLibrary {
    pub lines: Vec<LibraryLine>,
    pub similar: Vec<SimilarReport>,
}

/// The stem a test is named by in a report ("WPPSI-IV" → "wppsi").
fn test_stem(name: &str) -> String {
    name.to_lowercase()
        .split(['-', ' '])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The kinds of diagnosis named in `text`.
fn diagnosis_features(text: &str) -> BTreeSet<String> {
    let words: HashSet<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| normalize(w).to_lowercase())
        .collect();
    let has = |w: &str| {
        let w = normalize(w).to_lowercase();
        words.contains(&w)
            || words.iter().any(|x| {
                x.strip_prefix(['ו', 'ב', 'ל', 'ה', 'מ', 'ש'])
                    .is_some_and(|rest| rest == w)
            })
    };
    DIAGNOSES
        .iter()
        .filter(|(_, list)| list.split_whitespace().any(has))
        .map(|(k, _)| format!("dx:{k}"))
        .collect()
}

/// A past report's features.
fn report_features(s: &StoredSource) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(b) = report_band(s) {
        out.insert(format!("band:{b:?}"));
    }
    let all = dv_domain::instruments();
    let text: String = s
        .parts
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let lower = text.to_lowercase();
    // By the full name ("WPPSI-IV") first; by the stem ("WPPSI") only when no edition of it
    // is named in full.
    let full: Vec<&dv_domain::Instrument> = all
        .iter()
        .filter(|i| lower.contains(&i.name.to_lowercase()))
        .collect();
    for i in &all {
        let stem = test_stem(&i.name);
        let named = full.iter().any(|f| f.key == i.key)
            || (stem.chars().count() >= 3
                && lower.contains(&stem)
                && !full.iter().any(|f| test_stem(&f.name) == stem));
        if named {
            out.insert(format!("test:{}", i.key));
        }
    }
    for t in dv_domain::find_tagged_scores(&text) {
        let standard = all
            .iter()
            .find(|i| i.key == t.instrument)
            .and_then(|i| i.measures.iter().find(|m| m.key == t.measure))
            .is_some_and(|m| m.scale == dv_domain::scores::ScoreScale::Standard);
        if let (true, Some(band)) = (standard, t.band) {
            out.insert(format!("index:{}:{}:{band}", t.instrument, t.measure));
        }
    }
    let diagnoses: String = s
        .parts
        .iter()
        .filter(|p| p.section.as_deref() == Some("diagnoses"))
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    out.extend(diagnosis_features(&diagnoses));
    out
}

/// The case's features: its age, its score tables and what its diagnoses section says.
fn case_features(
    meta: &CaseMeta,
    sheets: &[dv_domain::ScoreSheet],
    diagnoses: &str,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(a) = meta.age {
        out.insert(format!("band:{:?}", dv_style::AgeBand::of_years(a.years)));
    }
    let all = dv_domain::instruments();
    for sheet in sheets {
        let Some(inst) = all.iter().find(|i| i.key == sheet.instrument) else {
            continue;
        };
        out.insert(format!("test:{}", inst.key));
        for e in &sheet.entries {
            let Some(m) = inst.measures.iter().find(|m| m.key == e.measure) else {
                continue;
            };
            if m.scale != dv_domain::scores::ScoreScale::Standard {
                continue;
            }
            if let Some(b) = dv_domain::scores::classify(m, e.value) {
                out.insert(format!("index:{}:{}:{}", inst.key, m.key, b.label));
            }
        }
    }
    out.extend(diagnosis_features(diagnoses));
    out
}

#[allow(clippy::cast_precision_loss)]
fn alike(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// What a report shares with the case, in words.
fn reasons(shared: &BTreeSet<&String>) -> Vec<String> {
    let all = dv_domain::instruments();
    let mut out = Vec::new();
    for f in shared {
        if let Some(b) = f.strip_prefix("band:") {
            let band = match b {
                "Toddler" => dv_style::AgeBand::Toddler,
                "Preschool" => dv_style::AgeBand::Preschool,
                _ => dv_style::AgeBand::School,
            };
            out.push(band.label().to_owned());
        } else if let Some(k) = f.strip_prefix("test:") {
            if let Some(i) = all.iter().find(|i| i.key == k) {
                out.push(i.name.clone());
            }
        } else if let Some(rest) = f.strip_prefix("index:") {
            let mut it = rest.splitn(3, ':');
            let (Some(inst), Some(measure), Some(band)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            if let Some(m) = all
                .iter()
                .find(|i| i.key == inst)
                .and_then(|i| i.measures.iter().find(|m| m.key == measure))
            {
                out.push(format!("{} {band}", m.name_he));
            }
        } else if f.starts_with("dx:") && !out.iter().any(|r| r == "אבחנה דומה") {
            out.push("אבחנה דומה".to_owned());
        }
    }
    out
}

/// "[ילד]" in a line she gave: "הילד" or "הילדה", as the case's child.
fn child_word(meta: &CaseMeta) -> &'static str {
    match meta.child_gender {
        Some(GrammaticalGender::Female) => "הילדה",
        Some(GrammaticalGender::Male) => "הילד",
        _ => "הילד/ה",
    }
}

impl Core {
    /// What her past reports offer beside `section_key` of a case (round 36).
    pub fn section_library(
        &mut self,
        case_id: &str,
        section_key: &str,
    ) -> Result<SectionLibrary, CoreError> {
        let sources = self.stored_sources()?;
        if sources.is_empty() {
            return Ok(SectionLibrary::default());
        }
        let v = self.vault_ref()?;
        if crate::structure_for(v, case_id)?
            .section(section_key)
            .is_none()
        {
            return Err(CoreError::NotFound("הסעיף".to_owned()));
        }
        let meta = v.case_meta(case_id)?;
        let inputs = v.inputs(case_id)?;
        let sheets = crate::score_sheets(v, case_id, &inputs)?;
        let diagnoses: String = v
            .drafts(case_id, "diagnoses")?
            .iter()
            .map(|d| d.text_tagged.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let case = case_features(&meta, &sheets, &diagnoses);
        // Each report's text for the section, and how like the case it is.
        let rows: Vec<(&str, String, f64, BTreeSet<String>)> = sources
            .iter()
            .filter_map(|(_, _, s)| {
                let text: String = s
                    .parts
                    .iter()
                    .filter(|p| p.section.as_deref() == Some(section_key))
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    return None;
                }
                let f = report_features(s);
                Some((s.title.as_str(), text, alike(&case, &f), f))
            })
            .collect();
        let is_similar = |score: f64| score >= ALIKE;
        let similar_count = rows.iter().filter(|r| is_similar(r.2)).count();
        let mut lines = Vec::new();
        if LIST_SECTIONS.contains(&section_key) {
            let child = child_word(&meta);
            let texts: Vec<String> = rows.iter().map(|r| r.1.replace("[ילד]", child)).collect();
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            for item in dv_style::mine_list(&refs, LIST_REPORTS) {
                let in_similar = item
                    .reports
                    .iter()
                    .filter(|d| rows.get(**d).is_some_and(|r| is_similar(r.2)))
                    .count();
                lines.push(LibraryLine {
                    text: item.text,
                    reports: count(item.reports.len()),
                    in_similar: count(in_similar),
                    similar: count(similar_count),
                });
            }
            lines.sort_by(|a, b| {
                b.in_similar
                    .cmp(&a.in_similar)
                    .then(b.reports.cmp(&a.reports))
            });
        }
        let mut ranked: Vec<&(&str, String, f64, BTreeSet<String>)> =
            rows.iter().filter(|r| r.2 > 0.0).collect();
        ranked.sort_by(|a, b| b.2.total_cmp(&a.2));
        let similar = ranked
            .into_iter()
            .take(SIMILAR_SHOWN)
            .map(|(title, text, _, f)| SimilarReport {
                report: (*title).to_owned(),
                reasons: reasons(&case.intersection(f).collect()),
                text: text.chars().take(EXCERPT_CHARS).collect(),
            })
            .collect();
        Ok(SectionLibrary { lines, similar })
    }
}
