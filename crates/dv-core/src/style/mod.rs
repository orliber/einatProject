//! The writing-style profile (D-043): Einat's past reports, the profile learned from them, its
//! use in every drafting request, and what is learned from her edits.
//!
//! * A past report is read in the isolated worker, cut by its headings into template sections,
//!   and **neutralized**: the usual filter (with every case's names), then every suspect hidden
//!   without asking, every tag reduced to its role (`[אח_2]` → `[אח]`), every number `[מספר]`.
//!   The result is filtered again and must come out unchanged, or the upload is refused. Only
//!   the excerpts she keeps are stored; the file and the full text never are.
//! * Analysis goes out one report per request; the synthesis sends only the abstract analyses.
//!   Both pass the review screen and a gate of their own (role tags only).
//! * Every item that comes back is filtered and checked for overlap with the reports before it
//!   is kept: a run of [`PROFILE_NGRAM`] words shared with a report drops it.
//! * In a drafting request, each item is filtered with the case's names first: one that would
//!   stop the gate (a word holding the child's name) is left out of that request.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use dv_ai::{RawStyleItem, StyleExcerpt, StyleItem, StyleKind, StyleOrigin, StyleProfile};
use dv_domain::{Author, DraftParagraph, DraftStatus, Identity, ReportStructure, Role};
use dv_privacy::text::normalize;
use dv_privacy::{
    everyday_name, filter, AutoHidden, AutoKind, FilterOutcome, Mark, PrivacyContext,
};
use dv_vault::{AuditEvent, TokenHasher, Vault};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::dates::unix_now;
use crate::{today, Core, CoreError, Outgoing, Pending, PendingKind, Prepared, Review, Task};

mod headings;
mod interpret;
mod learning;
mod library;
mod requests;

pub(crate) use interpret::{interpretations_for_request, InterpretationCheck};
pub use interpret::{InterpretationResult, InterpretationView};
pub use library::{LibraryLine, SectionLibrary, SimilarReport};

use headings::*;
use learning::*;

/// The privacy context's case for past reports: every declared name is hidden by its role.
const STYLE_CASE: &str = "style";
/// Setting: the approved profile is not used (back to the default style).
const PROFILE_OFF_KEY: &str = "style_profile_off";
/// A run this long shared with a past report drops a profile item.
pub const PROFILE_NGRAM: usize = 5;
/// A run this long shared with a past report puts a warning on a drafted paragraph. Higher than
/// [`PROFILE_NGRAM`]: her own expressions of 5–7 words are exactly the style that is wanted.
pub const DRAFT_NGRAM: usize = 8;
const MAX_SOURCES: usize = 40;
/// Excerpts sent for one report (characters).
const MAX_EXCERPT_CHARS: usize = 60_000;
const TITLE_CHARS: usize = 80;

/// Sections included by default: where her style shows most and identifying detail least.
const DEFAULT_INCLUDED: &[&str] = &[
    "appearance",
    "cognitive",
    "adaptive",
    "communication",
    "dsm",
    "emotional",
    "summary",
    "recommendations",
];

// ------------------------------------------------------------ views

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StylePartView {
    pub index: u32,
    /// Template section key, or `None` when the heading was not recognized.
    pub section: Option<String>,
    /// The heading as found (neutralized), or the section's title.
    pub heading: String,
    /// Exactly the text that would be kept (neutralized).
    pub text: String,
    pub words: u32,
    /// Suggested: sections where style shows most and identifying detail least.
    pub included: bool,
}

/// What an upload would keep. Nothing is stored until she confirms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleImportPreview {
    pub token: String,
    pub title: String,
    pub format: String,
    pub parts: Vec<StylePartView>,
    /// Names, places, dates and other details hidden.
    pub hidden: u32,
    /// Numbers hidden.
    pub numbers: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleSourceView {
    pub id: String,
    pub title: String,
    pub format: String,
    #[ts(type = "number")]
    pub added_at: i64,
    pub words: u32,
    /// Headings of the parts kept.
    pub headings: Vec<String>,
    pub analyzed: bool,
    /// Analyzed in demo mode (no API key): worth analyzing again for real.
    pub analysis_demo: bool,
    pub analysis_items: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleProfileView {
    pub id: String,
    pub version: u32,
    /// `draft` | `active` | `retired`.
    pub status: String,
    #[ts(type = "number")]
    pub created_at: i64,
    pub profile: StyleProfile,
    /// Items that came back but were not kept (shape, privacy or overlap with a report).
    pub dropped: u32,
    pub demo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleVersionView {
    pub id: String,
    pub version: u32,
    pub status: String,
    #[ts(type = "number")]
    pub created_at: i64,
    pub items: u32,
    pub reports: u32,
}

/// What a suggestion is (D-052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StyleSuggestionKind {
    /// In her edits: "instead of «from» you write «to»".
    Replace,
    /// In her edits: she deletes «from».
    Avoid,
    /// In her past reports: «from» recurs across reports.
    Phrase,
    /// In her past reports: a section usually opens with «from».
    Opening,
    /// In her past reports: a section usually closes with «from».
    Closing,
    /// In her past reports: a sentence she writes the same way in several of them (K4).
    Formula,
}

/// Something learned on her computer, waiting for her yes. Nothing of it is sent until then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleSuggestion {
    pub id: String,
    pub kind: StyleSuggestionKind,
    pub from: String,
    /// Empty unless `kind` is `replace`.
    pub to: String,
    /// Times she made the change, or reports the phrase is in.
    pub count: u32,
    /// How sure: of the times the AI wrote it, she changed it at least this often (edits), or
    /// the share of her reports that hold it (phrases). 0 to 100.
    pub confidence: u32,
    /// A template section key, or `None` for the whole report.
    pub section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleSectionLabel {
    pub key: String,
    pub title: String,
}

/// What the program measured in her writing, numbers only (D-052): the screen "what the
/// program learned".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleLearned {
    /// Paragraphs learned from: her past reports, her edits (half weight each), and the
    /// reports she finished here.
    pub from_reports: u32,
    pub from_edits: u32,
    pub from_finals: u32,
    pub finished_reports: u32,
    /// Her usual figures; `None` with too little to go on.
    pub words_per_sentence: Option<f64>,
    pub sentences_per_paragraph: Option<f64>,
    pub commas_per_sentence: Option<f64>,
    /// Out of 100 sentences.
    pub hedges: Option<f64>,
    pub contrasts: Option<f64>,
    pub first_person: Option<f64>,
    /// How close the AI's paragraphs were to her writing as it wrote them, 0 to 100: the
    /// latest ones, and the ones before them (to see whether the profile helps).
    pub fit_recent: Option<u32>,
    pub fit_earlier: Option<u32>,
    pub fit_count: u32,
    /// The same figure by month, oldest first: is drafting getting closer to her (ST12)?
    pub fit_by_month: Vec<FitMonth>,
}

/// Beside a section she writes herself (round 30): from her past reports and her profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
pub struct OwnSectionHelp {
    /// The section's first paragraph in her reports, newest first.
    pub examples: Vec<SimilarParagraph>,
    /// How she opened and closed the section, one line per report, without repeats.
    pub openings: Vec<String>,
    pub closings: Vec<String>,
    /// Her approved phrases and templates for the section.
    pub phrases: Vec<String>,
}

const OWN_HELP_EXAMPLES: usize = 3;
const OWN_HELP_LINES: usize = 5;

/// A paragraph of her past reports, beside a draft paragraph (ST2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SimilarParagraph {
    /// As kept: neutralized, with roles in place of names.
    pub text: String,
    /// The report's title as she saved it.
    pub report: String,
    /// From the same section of that report.
    pub same_section: bool,
}

/// The average closeness of the AI's paragraphs in one month.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FitMonth {
    /// "2026-10".
    pub month: String,
    pub average: u32,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleOverview {
    pub sources: Vec<StyleSourceView>,
    pub active: Option<StyleProfileView>,
    pub draft: Option<StyleProfileView>,
    pub versions: Vec<StyleVersionView>,
    pub suggestions: Vec<StyleSuggestion>,
    /// The approved profile goes into drafting requests (a switch in the screen).
    pub enabled: bool,
    pub sections: Vec<StyleSectionLabel>,
    pub demo_mode: bool,
    pub learned: StyleLearned,
    /// How she interprets scores (round 33), approved or waiting for her.
    pub interpretations: Vec<InterpretationView>,
    /// How many subtests have a score in her reports: what a reading would cover.
    pub scored_measures: u32,
    /// The order she writes sections in, learned from her reports (K3, K13).
    pub skeletons: Vec<SkeletonView>,
    /// What she leaves out beside scores, so the scores material leaves it out too (K12).
    pub omissions: Vec<String>,
}

/// The learned order of one section, for the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkeletonView {
    pub section: String,
    pub title: String,
    /// The age band in words; `None`: from all her reports.
    pub band: Option<String>,
    pub reports: u32,
    /// The topics in their usual order.
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleAnalysisResult {
    pub source_id: String,
    pub kept: u32,
    pub dropped: u32,
    pub demo: bool,
}

// ------------------------------------------------------------ stored shapes

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPart {
    section: Option<String>,
    heading: String,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedItem {
    section: Option<String>,
    kind: StyleKind,
    text: String,
    support: u32,
}

impl From<&RawStyleItem> for SavedItem {
    fn from(i: &RawStyleItem) -> Self {
        Self {
            section: i.section.clone(),
            kind: i.kind,
            text: i.text.clone(),
            support: i.support,
        }
    }
}

impl From<&SavedItem> for RawStyleItem {
    fn from(i: &SavedItem) -> Self {
        Self {
            section: i.section.clone(),
            kind: i.kind,
            text: i.text.clone(),
            support: i.support,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAnalysis {
    items: Vec<SavedItem>,
    dropped: u32,
    demo: bool,
    at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSource {
    title: String,
    format: String,
    parts: Vec<StoredPart>,
    #[serde(default)]
    analysis: Option<StoredAnalysis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProfile {
    profile: StyleProfile,
    #[serde(default)]
    dropped: u32,
    #[serde(default)]
    demo: bool,
}

/// What was learned from her edits (the document keeps the earlier `pairs` shape).
type Learning = dv_style::EditModel;

/// The local style model and the phrases of her reports, kept in one sealed secret (D-052).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct LocalStyle {
    #[serde(default)]
    model: dv_style::StyleModel,
    #[serde(default)]
    phrases: Vec<dv_style::PhraseCandidate>,
    /// How close each of the AI's latest paragraphs was to her writing (0 to 100), oldest
    /// first, at most [`FIT_KEPT`]. Numbers only.
    #[serde(default)]
    draft_fit: Vec<u8>,
    /// The same figures by month ("2026-10" → (sum, count)), the latest [`MONTHS_KEPT`] (ST12).
    #[serde(default)]
    fit_months: BTreeMap<String, (u32, u32)>,
    /// Her usual name for terms that have several, from her past reports (ST10).
    #[serde(default)]
    terms: BTreeMap<String, String>,
    /// How she usually opens and closes each section, from her past reports (ST11).
    #[serde(default)]
    shapes: Vec<dv_style::Shape>,
    /// How she interprets the score of each subtest, from her past reports (round 33). Used
    /// only once she approved it.
    #[serde(default)]
    interpretations: Vec<dv_ai::Interpretation>,
    /// The order of the topics in each section, by age band (K3, K13). Numbers and labels.
    #[serde(default)]
    skeletons: Vec<dv_style::Skeleton>,
    /// Sentences she writes the same way in several reports (K4), offered to her.
    #[serde(default)]
    formulas: Vec<dv_style::Formula>,
    /// What she leaves out beside scores (K12).
    #[serde(default)]
    habits: dv_domain::SheetHabits,
}

/// How many months of closeness figures are kept for the trend.
const MONTHS_KEPT: usize = 12;

/// How many closeness figures are kept, and how many make "the latest".
const FIT_KEPT: usize = 120;
const FIT_WINDOW: usize = 30;

impl LocalStyle {
    /// Notes how close a paragraph the AI wrote is to her writing, before learning from it.
    fn note_fit(&mut self, section: &str, ai_text: &str) {
        let (y, m, _) = crate::dates::today();
        self.note_fit_in(section, ai_text, &format!("{y:04}-{m:02}"));
    }

    fn note_fit_in(&mut self, section: &str, ai_text: &str, month: &str) {
        if let Some(f) = self.model.fidelity(section, ai_text) {
            let e = self.fit_months.entry(month.to_owned()).or_insert((0, 0));
            e.0 = e.0.saturating_add(f.min(100));
            e.1 = e.1.saturating_add(1);
            while self.fit_months.len() > MONTHS_KEPT {
                self.fit_months.pop_first();
            }
            self.draft_fit.push(u8::try_from(f.min(100)).unwrap_or(100));
            if self.draft_fit.len() > FIT_KEPT {
                let extra = self.draft_fit.len() - FIT_KEPT;
                self.draft_fit.drain(..extra);
            }
        }
    }

    fn learned(&self) -> StyleLearned {
        let m = self.model.measures();
        let whole = |x: f64| u32::try_from(x.round() as i64).unwrap_or(0);
        let mean = |xs: &[u8]| {
            (!xs.is_empty()).then(|| {
                let sum: u32 = xs.iter().map(|x| u32::from(*x)).sum();
                (f64::from(sum) / xs.len() as f64).round() as u32
            })
        };
        let n = self.draft_fit.len();
        let split = n.saturating_sub(FIT_WINDOW);
        let earlier_from = split.saturating_sub(FIT_WINDOW);
        StyleLearned {
            from_reports: whole(m.from_reports),
            from_edits: whole(m.from_edits),
            from_finals: whole(m.from_finals),
            finished_reports: u32::try_from(m.finished_reports).unwrap_or(u32::MAX),
            words_per_sentence: m.words_per_sentence,
            sentences_per_paragraph: m.sentences_per_paragraph,
            commas_per_sentence: m.commas_per_sentence,
            hedges: m.hedges,
            contrasts: m.contrasts,
            first_person: m.first_person,
            fit_recent: mean(self.draft_fit.get(split..).unwrap_or_default()),
            fit_earlier: mean(self.draft_fit.get(earlier_from..split).unwrap_or_default()),
            fit_count: u32::try_from(n).unwrap_or(u32::MAX),
            fit_by_month: self
                .fit_months
                .iter()
                .filter(|(_, (_, count))| *count > 0)
                .map(|(month, (sum, count))| FitMonth {
                    month: month.clone(),
                    average: (f64::from(*sum) / f64::from(*count)).round() as u32,
                    count: *count,
                })
                .collect(),
        }
    }
}

/// An upload waiting for her confirmation (memory only, cleared at lock).
/// See [`Core::style_data`].
pub(crate) struct StyleData {
    identities: Vec<Identity>,
    practitioner: Vec<String>,
    allow: HashSet<String>,
    hasher: TokenHasher,
}

impl StyleData {
    pub(crate) fn with_ctx<R>(&self, f: impl FnOnce(&PrivacyContext<'_>, &TokenHasher) -> R) -> R {
        let allowed = |t: &str| self.allow.contains(&self.hasher.token_hmac(t));
        let none = |_: &str| false;
        let ctx = PrivacyContext {
            case_id: STYLE_CASE,
            identities: &self.identities,
            practitioner: &self.practitioner,
            allowlisted: &allowed,
            confirmed_names: &none,
            past_names: &none,
            today: today(),
            masks: None,
        };
        f(&ctx, &self.hasher)
    }

    /// Changes when a name or a decision about one changed: work done with an older copy
    /// is done again.
    pub(crate) fn stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for i in &self.identities {
            (&i.id, &i.value, &i.aliases, &i.tag).hash(&mut h);
        }
        self.practitioner.hash(&mut h);
        let mut allow: Vec<&String> = self.allow.iter().collect();
        allow.sort();
        allow.hash(&mut h);
        h.finish()
    }
}

/// A past report split into its parts and neutralized, before she chose what to keep.
pub(crate) struct StyleWork {
    stamp: u64,
    title: String,
    format: String,
    parts: Vec<StoredPart>,
    names: Vec<String>,
    /// A keyed hash of the report's main child: reports of the same child count once.
    child: Option<String>,
    hidden: u32,
    numbers: u32,
    warnings: Vec<String>,
}

/// Split a past report into its parts and neutralize each (the slow part of adding one,
/// run while the core is free).
pub(crate) fn style_work(
    data: &StyleData,
    file_name: &str,
    extracted: &dv_ingest::Extracted,
) -> Result<StyleWork, CoreError> {
    let structure =
        ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
    // Round 32: a score beside its subtest keeps its number, as a score tag with its range
    // and its place among the report's subtests; every other number is masked below.
    let (body, _) = tag_scores(&extracted.body);
    let raw_parts = split_parts(&body, &structure);
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("דוח")
        .to_owned();
    let (parts, title, hidden, numbers, names, child_hash) = data.with_ctx(|ctx, hasher| {
        let child = main_child(&extracted.body, ctx)?;
        let child = child.as_deref();
        let child_hash = child.map(|c| hasher.token_hmac(&normalize(c)));
        let run = |t: &str| filter(t, ctx).map_err(|e| CoreError::Internal(e.to_string()));
        let names = report_names(
            &[run(&extracted.body)?, run(&extracted.margins)?],
            child,
            hasher,
        );
        let (mut hidden, mut numbers) = (0, 0);
        let mut parts = Vec::new();
        for p in raw_parts {
            let (text, h, n) = neutralize(&p.text, ctx, child)?;
            let (heading, hh, _) = neutralize(&p.heading, ctx, child)?;
            hidden += h + hh;
            numbers += n;
            parts.push(StoredPart {
                section: p.section,
                heading,
                text,
            });
        }
        let (title, _, _) = neutralize(&stem, ctx, child)?;
        Ok::<_, CoreError>((parts, title, hidden, numbers, names, child_hash))
    })?;
    let mut warnings = extracted.warnings.clone();
    if parts.iter().all(|p| p.section.is_none()) {
        warnings.push(
            "לא זיהיתי כותרות של סעיפים בדוח, ולכן שום קטע לא סומן מראש. אפשר לסמן ידנית את הקטעים שמראים את הסגנון."
                .to_owned(),
        );
    }
    if !extracted.margins.trim().is_empty() {
        warnings.push("כותרות עליונות ותחתונות לא נלקחות בכלל.".to_owned());
    }
    Ok(StyleWork {
        stamp: data.stamp(),
        title: title.chars().take(TITLE_CHARS).collect(),
        format: extracted.format.name().to_owned(),
        parts,
        names,
        child: child_hash,
        hidden,
        numbers,
        warnings,
    })
}

pub(crate) struct StagedStyle {
    title: String,
    format: String,
    parts: Vec<StoredPart>,
    /// Keyed hashes of the names found in the report (its child, people, the margins).
    names: Vec<String>,
    child: Option<String>,
}

/// Keyed hashes of the names in her past reports, by report: hidden in every case and
/// refused by the gate ("שם מדוח ישן"). No name text is kept.
const PAST_NAMES_KEY: &str = "style_past_names";

type PastNames = BTreeMap<String, Vec<String>>;

/// The keyed hash of each past report's main child, by report (the review of stage 4): what
/// is "in 3 reports or more" counts children, so three reports of one child sharing a copied
/// history paragraph do not make it her fixed wording. A report without one counts alone.
const CHILDREN_KEY: &str = "style_children";

fn past_children(v: &Vault) -> Result<BTreeMap<String, String>, CoreError> {
    match v.secret(CHILDREN_KEY)? {
        Some(s) => parse(&s),
        None => Ok(BTreeMap::new()),
    }
}

/// For each report (by id, in order), the child it is about, as a small number: reports of
/// the same child get the same one.
pub(crate) fn child_groups(v: &Vault, ids: &[String]) -> Result<Vec<usize>, CoreError> {
    let children = past_children(v)?;
    let mut seen: HashMap<&str, usize> = HashMap::new();
    Ok(ids
        .iter()
        .enumerate()
        .map(|(i, id)| match children.get(id) {
            Some(h) => {
                let next = ids.len() + seen.len();
                *seen.entry(h.as_str()).or_insert(next)
            }
            None => i,
        })
        .collect())
}

fn past_names(v: &Vault) -> Result<PastNames, CoreError> {
    match v.secret(PAST_NAMES_KEY)? {
        Some(s) => parse(&s),
        None => Ok(PastNames::new()),
    }
}

/// Every past-report name hash, for a privacy context.
pub(crate) fn past_name_hmacs(v: &Vault) -> Result<HashSet<String>, CoreError> {
    Ok(past_names(v)?.into_values().flatten().collect())
}

/// The words of the names the filter found in a past report, as keyed hashes. Everyday words
/// ("גיל", "שמחה") are left out, as for names found in a case.
fn report_names(outs: &[FilterOutcome], child: Option<&str>, v: &TokenHasher) -> Vec<String> {
    let mut words: Vec<String> = outs
        .iter()
        .flat_map(|o| o.auto_hidden.iter())
        .filter(|a| matches!(a.kind, AutoKind::Name | AutoKind::OtherCase))
        .flat_map(|a| {
            a.token
                .split_whitespace()
                .map(normalize)
                .collect::<Vec<_>>()
        })
        .chain(child.map(normalize))
        .filter(|w| w.chars().count() >= 2 && !everyday_name(w))
        .collect();
    words.sort();
    words.dedup();
    let mut hashes: Vec<String> = words.iter().map(|w| v.token_hmac(w)).collect();
    hashes.sort();
    hashes
}

fn json<T: Serialize>(v: &T) -> Result<String, CoreError> {
    serde_json::to_string(v).map_err(|e| CoreError::Internal(e.to_string()))
}

fn parse<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T, CoreError> {
    serde_json::from_str(s).map_err(|e| CoreError::Internal(e.to_string()))
}

fn words(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

// ------------------------------------------------------------ neutralizing

/// A role's tag as a case would give its first person: `[ילד]` for a role that occurs once
/// per case, `[אח_1]` for the others. The numbered form matters: the filter reads the word in
/// `[אדם]` as a name in some contexts, never the one in `[אדם_1]`.
fn role_tag(role: Role) -> String {
    if role.is_singleton() {
        format!("[{}]", role.tag_base())
    } else {
        format!("[{}_1]", role.tag_base())
    }
}

/// `[אח_2]` → `[אח_1]`, `[ילד]` stays: every person by role only, numbering dropped. Generic
/// tags (`[טלפון]`) and generalizations ("המוסד") stay.
fn role_only(tag: &str) -> String {
    let Some(inner) = tag.strip_prefix('[').and_then(|t| t.strip_suffix(']')) else {
        return tag.to_owned();
    };
    let base = match inner.rsplit_once('_') {
        Some((base, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => inner,
    };
    Role::ALL
        .iter()
        .find(|r| r.tag_base() == base)
        .map_or_else(|| tag.to_owned(), |r| role_tag(*r))
}

/// Every tag a neutralized text may hold: one per role, plus the generic ones.
fn style_tags() -> HashSet<String> {
    Role::ALL.iter().map(|r| role_tag(*r)).collect()
}

/// The filter's output with every tag reduced to its role and every relative date `[תאריך]`.
/// A name the filter found that is `child` (normalized) is `[ילד]`; another child is `[אדם]`.
/// Returns the text and how many spans were hidden.
fn rebuild(out: &FilterOutcome, child: Option<&str>) -> (String, u32) {
    // A name the filter found on its own: which words it was, by its tag.
    let found = |tag: &str| -> Vec<&AutoHidden> {
        out.auto_hidden
            .iter()
            .filter(|a| a.tag == tag && matches!(a.kind, AutoKind::Name | AutoKind::OtherCase))
            .collect()
    };
    let mut text = String::with_capacity(out.tagged.len());
    let mut hidden = 0;
    for seg in &out.tagged_segments {
        match seg.mark {
            None | Some(Mark::Replaced) => text.push_str(&seg.text),
            Some(Mark::Tag) => {
                let names = found(&seg.text);
                let role = match names.first() {
                    None => None,
                    Some(_)
                        if names
                            .iter()
                            .any(|a| child.is_some_and(|c| normalize(&a.token) == c)) =>
                    {
                        Some(Role::Child)
                    }
                    // "Another child" reads wrong in a report about one child.
                    Some(first) => Some(match first.role {
                        Role::OtherChild | Role::Child => Role::Other,
                        r => r,
                    }),
                };
                match role {
                    Some(r) => text.push_str(&role_tag(r)),
                    None => text.push_str(&role_only(&seg.text)),
                }
                hidden += 1;
            }
            // A job, generalized ("עובדת בתחום החינוך"): it stays as it is.
            Some(Mark::Relative) if seg.label.as_deref() == Some("מקצוע") => {
                text.push_str(&seg.text);
                hidden += 1;
            }
            // A date, or a name left out (a kindergarten's: empty text).
            Some(Mark::Relative) => {
                if !seg.text.is_empty() {
                    text.push_str("[תאריך]");
                }
                hidden += 1;
            }
            Some(Mark::Suspect) => {
                text.push_str(&role_tag(Role::Other));
                hidden += 1;
            }
        }
    }
    (text, hidden)
}

/// Every number → `[מספר]`, except inside a tag and a test's edition ("WISC-5").
fn mask_numbers(text: &str) -> (String, u32) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut masked = 0;
    let mut depth = 0u32;
    let at = |k: usize| chars.get(k).copied();
    let mut i = 0;
    while let Some(c) = at(i) {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && c.is_ascii_digit() {
            let before = |back: usize| i.checked_sub(back).and_then(at);
            let latin_before = before(1).is_some_and(|p| {
                p.is_ascii_alphabetic()
                    || (p == '-' && before(2).is_some_and(|q| q.is_ascii_alphabetic()))
            });
            let mut j = i;
            while at(j).is_some_and(|d| {
                d.is_ascii_digit()
                    || (matches!(d, '.' | ':' | '/' | '-' | ',')
                        && at(j + 1).is_some_and(|e| e.is_ascii_digit()))
            }) {
                j += 1;
            }
            if latin_before {
                out.extend(chars.get(i..j).unwrap_or_default());
            } else {
                out.push_str("[מספר]");
                masked += 1;
            }
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    (out, masked)
}

/// Every score a past report gives beside its test or subtest, as a score tag
/// (`dv_domain::score_tag`). Returns the text and how many were tagged.
pub(crate) fn tag_scores(text: &str) -> (String, u32) {
    let found = dv_domain::find_score_mentions(text);
    let mut out = String::with_capacity(text.len() + found.len() * 24);
    let mut at = 0;
    let mut n = 0;
    for m in &found {
        let (Some(before), true) = (text.get(at..m.start), m.start >= at) else {
            continue;
        };
        out.push_str(before);
        out.push_str(&dv_domain::score_tag(m));
        at = m.end;
        n += 1;
    }
    out.push_str(text.get(at..).unwrap_or_default());
    (out, n)
}

/// The child a past report is about: the unknown name it mentions most (twice or more).
fn main_child(text: &str, ctx: &PrivacyContext<'_>) -> Result<Option<String>, CoreError> {
    let out = filter(text, ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
    let mut counts: Vec<(String, u32)> = Vec::new();
    for seg in out
        .tagged_segments
        .iter()
        .filter(|s| s.mark == Some(Mark::Tag))
    {
        let Some(name) = out
            .auto_hidden
            .iter()
            .find(|a| a.tag == seg.text && matches!(a.kind, AutoKind::Name | AutoKind::OtherCase))
        else {
            continue;
        };
        let n = normalize(&name.token);
        match counts.iter_mut().find(|(t, _)| *t == n) {
            Some(c) => c.1 += 1,
            None => counts.push((n, 1)),
        }
    }
    Ok(counts
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .max_by_key(|(_, c)| *c)
        .map(|(t, _)| t))
}

/// Neutralize a text for the style profile. Fails closed when it does not come out stable.
/// Returns `(text, hidden, numbers)`.
fn neutralize(
    text: &str,
    ctx: &PrivacyContext<'_>,
    child: Option<&str>,
) -> Result<(String, u32, u32), CoreError> {
    let run = |t: &str| filter(t, ctx).map_err(|e| CoreError::Internal(e.to_string()));
    let (rebuilt, mut hidden) = rebuild(&run(text)?, child);
    let (mut current, mut numbers) = mask_numbers(&rebuilt);
    for _ in 0..3 {
        let out = run(&current)?;
        if out.auto_hidden.is_empty() && out.tagged == current {
            return Ok((current, hidden, numbers));
        }
        let (again, h) = rebuild(&out, child);
        let (again, n) = mask_numbers(&again);
        hidden += h;
        numbers += n;
        current = again;
    }
    Err(CoreError::Refused(
        "לא הצלחתי לנטרל את הטקסט עד הסוף, ולכן הוא לא נשמר. אפשר לנסות קובץ אחר.".to_owned(),
    ))
}

/// A text that may go out as part of the profile: the filter leaves it exactly as it is.
fn clean(text: &str, ctx: &PrivacyContext<'_>) -> bool {
    filter(text, ctx).is_ok_and(|o| o.auto_hidden.is_empty() && o.tagged == text)
}

// ------------------------------------------------------------ overlap

/// Words for overlap checks: normalized, punctuation trimmed, tags by role.
fn overlap_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '[' || c == ']'));
            if w.starts_with('[') {
                role_only(w)
            } else {
                normalize(w)
            }
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn ngrams(text: &str, n: usize) -> HashSet<String> {
    let w = overlap_words(text);
    if w.len() < n {
        return HashSet::new();
    }
    w.windows(n).map(|g| g.join(" ")).collect()
}

/// The first run of `n` words `text` shares with the index, if any.
fn shared_run(text: &str, n: usize, index: &HashSet<String>) -> Option<String> {
    let w = overlap_words(text);
    if w.len() < n {
        return None;
    }
    w.windows(n)
        .map(|g| g.join(" "))
        .find(|g| index.contains(g))
}

/// The runs of `n` words of her past reports that are about a child: a run in
/// [`dv_style::formulas::MIN_REPORTS`] reports or more is her fixed wording (K4), which a
/// profile item or a draft may repeat.
/// `children` holds the child of each report ([`child_groups`]): a run counts once per child.
fn source_index(sources: &[StoredSource], children: &[usize], n: usize) -> HashSet<String> {
    let mut reports: HashMap<String, HashSet<usize>> = HashMap::new();
    for (i, s) in sources.iter().enumerate() {
        let child = children.get(i).copied().unwrap_or(i);
        let grams: HashSet<String> = s.parts.iter().flat_map(|p| ngrams(&p.text, n)).collect();
        for g in grams {
            reports.entry(g).or_default().insert(child);
        }
    }
    reports
        .into_iter()
        .filter(|(_, r)| r.len() < dv_style::formulas::MIN_REPORTS)
        .map(|(g, _)| g)
        .collect()
}

// ------------------------------------------------------------ use in drafting

/// The approved profile as it goes into a drafting request for `section_key`, with every item
/// that the case's filter would change left out. `None`: no profile, or it is switched off.
pub(crate) fn style_for_request(
    v: &Vault,
    ctx: &PrivacyContext<'_>,
    section_key: &str,
    query: &str,
) -> Result<Option<String>, CoreError> {
    if v.setting(PROFILE_OFF_KEY)?.as_deref() == Some("1") {
        return Ok(None);
    }
    let Some(active) = v
        .style_profiles()?
        .into_iter()
        .find(|p| p.status == "active")
    else {
        return Ok(None);
    };
    let stored: StoredProfile = parse(&active.json)?;
    let keep = |t: &str| clean(t, ctx);
    let focused = focus(&stored.profile, section_key, query);
    let Some(text) = dv_ai::render_for_section(&focused, section_key, &keep) else {
        return Ok(None);
    };
    // Learned only from her edits: her rules come on top of the default style.
    Ok(Some(if stored.profile.reports == 0 {
        format!("{}\n{text}", dv_ai::prompts::DEFAULT_STYLE)
    } else {
        text
    }))
}

/// How many items of a kind go into one request, for the section and for the whole report
/// each. Rules and things to avoid always go: they are short and they are her word.
const REQUEST_BUDGET: [(StyleKind, usize); 3] = [
    (StyleKind::Example, 2),
    (StyleKind::Template, 6),
    (StyleKind::Phrase, 12),
];

/// The profile with only the examples, templates and phrases that fit this request best
/// (BM25 against the case's filtered text for the section, on her computer; D-052).
fn focus(profile: &StyleProfile, section_key: &str, query: &str) -> StyleProfile {
    let mut drop: HashSet<usize> = HashSet::new();
    for scope in [Some(section_key), None] {
        for (kind, budget) in REQUEST_BUDGET {
            let group: Vec<usize> = profile
                .items
                .iter()
                .enumerate()
                .filter(|(_, i)| i.enabled && i.kind == kind && i.section.as_deref() == scope)
                .map(|(n, _)| n)
                .collect();
            let texts: Vec<&str> = group
                .iter()
                .filter_map(|n| profile.items.get(*n).map(|i| i.text.as_str()))
                .collect();
            let kept: HashSet<usize> = dv_style::retrieve::best(&texts, query, budget)
                .into_iter()
                .filter_map(|k| group.get(k).copied())
                .collect();
            drop.extend(group.into_iter().filter(|n| !kept.contains(n)));
        }
    }
    StyleProfile {
        items: profile
            .items
            .iter()
            .enumerate()
            .filter(|(n, _)| !drop.contains(n))
            .map(|(_, i)| i.clone())
            .collect(),
        reports: profile.reports,
    }
}

/// A warning for a drafted paragraph that repeats a past report word for word.
pub(crate) fn overlap_warning(v: &Vault, texts: &[&str]) -> Result<Vec<Option<String>>, CoreError> {
    let rows = v.style_sources()?;
    let sources: Vec<StoredSource> = rows
        .iter()
        .map(|s| parse(&s.json))
        .collect::<Result<_, _>>()?;
    if sources.is_empty() {
        return Ok(vec![None; texts.len()]);
    }
    let ids: Vec<String> = rows.into_iter().map(|r| r.id).collect();
    let index = source_index(&sources, &child_groups(v, &ids)?, DRAFT_NGRAM);
    Ok(texts
        .iter()
        .map(|t| {
            shared_run(t, DRAFT_NGRAM, &index)
                .map(|run| format!("חוזר מילה במילה על קטע מדוח ישן («{run}»). כדאי לנסח מחדש."))
        })
        .collect())
}

// ------------------------------------------------------------ local style model

const LOCAL_STYLE_KEY: &str = "style_local";
/// Where an earlier version kept sentence lengths only (cleared when it is re-measured).
const OLD_FINGERPRINT_KEY: &str = "style_fingerprint";

fn local_style(v: &Vault) -> Result<LocalStyle, CoreError> {
    match v.secret(LOCAL_STYLE_KEY)? {
        // An unreadable document only loses what was learned: start over rather than fail.
        Some(s) => Ok(parse(&s).unwrap_or_default()),
        None => Ok(LocalStyle::default()),
    }
}

/// The key a finished report is learned under: a keyed hash, so the style secret does not
/// name the case.
fn final_key(v: &Vault, case_id: &str) -> String {
    v.token_hmac(&format!("final|{case_id}"))
}

/// Her style model for the case screens, or `None` when the profile is switched off.
pub(crate) fn style_model(v: &Vault) -> Result<Option<dv_style::StyleModel>, CoreError> {
    if v.setting(PROFILE_OFF_KEY)?.as_deref() == Some("1") {
        return Ok(None);
    }
    Ok(Some(local_style(v)?.model))
}

/// Her usual name for terms that have several, learned from her past reports (ST10).
pub(crate) fn style_terms(v: &Vault) -> Result<BTreeMap<String, String>, CoreError> {
    Ok(local_style(v)?.terms)
}

/// A gentle note on a drafted paragraph in `section` that reads unlike her, or `None`.
pub(crate) fn style_note(
    model: &dv_style::StyleModel,
    section: &str,
    text: &str,
) -> Option<String> {
    model.note(section, text)
}

/// Re-learn from her past reports: the model's report half and the recurring phrases.
fn fit_reports(local: &mut LocalStyle, sources: &[StoredSource], children: &[usize]) {
    // The newest report counts in full; each earlier one a little less (ST5).
    let n = sources.len();
    local
        .model
        .fit_reports_weighted(sources.iter().enumerate().flat_map(|(i, s)| {
            let w = dv_style::report_weight(n - 1 - i);
            s.parts
                .iter()
                .map(move |p| (p.section.as_deref(), p.text.as_str(), w))
        }));
    let reports: Vec<dv_style::ReportParts<'_>> = sources
        .iter()
        .map(|s| {
            s.parts
                .iter()
                .map(|p| (p.section.as_deref(), p.text.as_str()))
                .collect()
        })
        .collect();
    local.phrases = dv_style::mine_phrases(&reports);
    local.shapes = dv_style::mine_shapes(&reports);
    let bands: Vec<Option<dv_style::AgeBand>> = sources.iter().map(report_band).collect();
    local.skeletons = dv_style::mine_skeletons(&reports, &bands);
    local.formulas = dv_style::mine_formulas(&reports, children, &formula_slot);
    local.habits = dv_domain::sheet_habits(
        sources
            .iter()
            .flat_map(|s| s.parts.iter())
            .map(|p| p.text.as_str()),
    );
    local.terms = dv_style::terms::usual(
        sources
            .iter()
            .flat_map(|s| s.parts.iter())
            .map(|p| p.text.as_str()),
        2,
    );
}

/// The age band of a past report, from the test it names most (K13): the Bayley is given
/// under three, the WPPSI from three to six, the WISC from six. `None`: none of them.
fn report_band(s: &StoredSource) -> Option<dv_style::AgeBand> {
    use dv_style::AgeBand;
    let text = s
        .parts
        .iter()
        .map(|p| p.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    [
        ("bayley", AgeBand::Toddler),
        ("wppsi", AgeBand::Preschool),
        ("wisc", AgeBand::School),
    ]
    .into_iter()
    .map(|(name, band)| (text.matches(name).count(), band))
    .filter(|(n, _)| *n > 0)
    .max_by_key(|(n, _)| *n)
    .map(|(_, band)| band)
}

/// The empty place a tag of a past report becomes in her fixed wording (K4): a score, or a
/// number. Any other tag is about a child, and the sentence is not learned.
fn formula_slot(tag: &str) -> Option<&'static str> {
    if dv_domain::is_score_tag(tag) {
        Some("{תוצאה}")
    } else if tag == "[מספר]" {
        Some("{מספר}")
    } else {
        None
    }
}

/// What she leaves out beside scores, in words (K12).
fn omissions(h: dv_domain::SheetHabits) -> Vec<String> {
    let mut out = Vec::new();
    if h.no_subtest_percentiles {
        out.push(
            "את לא כותבת אחוזונים ליד התוצאות של תתי-המבחנים, ולכן הם לא נכתבים בדף התוצאות שנשלח."
                .to_owned(),
        );
    }
    if h.no_index_percentiles {
        out.push(
            "את לא כותבת אחוזונים ליד המדדים, ולכן הם לא נכתבים בדף התוצאות שנשלח.".to_owned(),
        );
    }
    out
}

/// Her learned order of `section` for a case with `age`, unless the profile is switched off.
pub(crate) fn style_skeleton(
    v: &Vault,
    section: &str,
    age: Option<dv_domain::Age>,
) -> Result<Option<dv_style::Skeleton>, CoreError> {
    if v.setting(PROFILE_OFF_KEY)?.as_deref() == Some("1") {
        return Ok(None);
    }
    let band = age.map(|a| dv_style::AgeBand::of_years(a.years));
    Ok(dv_style::skeleton_for(&local_style(v)?.skeletons, section, band).cloned())
}

/// What she leaves out beside scores (K12), unless the profile is switched off.
pub(crate) fn sheet_habits(v: &Vault) -> Result<dv_domain::SheetHabits, CoreError> {
    if v.setting(PROFILE_OFF_KEY)?.as_deref() == Some("1") {
        return Ok(dv_domain::SheetHabits::default());
    }
    Ok(local_style(v)?.habits)
}

/// The draft paragraph `draft_id` of a case, from any section.
pub(crate) fn find_draft(
    v: &Vault,
    case_id: &str,
    draft_id: &str,
) -> Result<Option<DraftParagraph>, CoreError> {
    let structure =
        ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
    for s in structure.sections() {
        if let Some(d) = v
            .drafts(case_id, &s.key)?
            .into_iter()
            .find(|d| d.id == draft_id)
        {
            return Ok(Some(d));
        }
    }
    Ok(None)
}

// ------------------------------------------------------------ Core

impl Core {
    /// What filtering for her style needs, taken from the vault: every case's names (as the
    /// style case), the practitioner, the "not a name" decisions, and the word hasher. Owned,
    /// so the work can run while the core is free (F2).
    pub(crate) fn style_data(&mut self) -> Result<StyleData, CoreError> {
        let v = self.vault_ref()?;
        let mut identities: Vec<Identity> = v
            .all_identities()?
            .into_iter()
            .map(|mut i| {
                i.case_id = STYLE_CASE.to_owned();
                i
            })
            .collect();
        // One empty identity per role: it matches no text (nothing to index), and makes its
        // role tag a known tag, so `[ילד]` stays a tag instead of turning into plain words.
        identities.extend(Role::ALL.iter().map(|r| Identity {
            id: format!("style-{}", r.tag_base()),
            case_id: STYLE_CASE.to_owned(),
            role: *r,
            tag: role_tag(*r),
            value: String::new(),
            aliases: Vec::new(),
            source: dv_domain::IdentitySource::Manual,
            reason: String::new(),
        }));
        Ok(StyleData {
            practitioner: v.practitioner()?.names,
            allow: v.not_a_name_hmacs("")?.into_iter().collect(),
            hasher: v.token_hasher(),
            identities,
        })
    }

    fn with_style_ctx<R>(
        &mut self,
        f: impl FnOnce(&PrivacyContext<'_>, &TokenHasher) -> Result<R, CoreError>,
    ) -> Result<R, CoreError> {
        self.style_data()?.with_ctx(f)
    }

    fn stored_sources(&mut self) -> Result<Vec<(String, i64, StoredSource)>, CoreError> {
        self.vault_ref()?
            .style_sources()?
            .into_iter()
            .map(|s| Ok((s.id, s.created_at, parse(&s.json)?)))
            .collect()
    }

    fn profile_view(p: dv_vault::StoredStyleProfile) -> Result<StyleProfileView, CoreError> {
        let stored: StoredProfile = parse(&p.json)?;
        Ok(StyleProfileView {
            id: p.id,
            version: p.version,
            status: p.status,
            created_at: p.created_at,
            profile: stored.profile,
            dropped: stored.dropped,
            demo: stored.demo,
        })
    }

    /// The paragraph of her past reports closest to a draft paragraph (ST2), to read beside it
    /// while she edits. Her reports are kept neutralized, so it shows roles, not names. Shown
    /// on her computer only; it never goes into a request.
    pub fn similar_paragraph(
        &mut self,
        case_id: &str,
        draft_id: &str,
    ) -> Result<Option<SimilarParagraph>, CoreError> {
        let draft = find_draft(self.vault_ref()?, case_id, draft_id)?
            .ok_or_else(|| CoreError::NotFound("הפסקה".into()))?;
        let sources = self.stored_sources()?;
        let mut docs: Vec<(Option<&str>, &str)> = Vec::new();
        let mut from: Vec<(&str, Option<&str>)> = Vec::new();
        for (_, _, s) in &sources {
            for p in &s.parts {
                for para in dv_style::text::paragraphs(&p.text) {
                    docs.push((p.section.as_deref(), para));
                    from.push((s.title.as_str(), p.section.as_deref()));
                }
            }
        }
        Ok(
            dv_style::retrieve::closest(&docs, &draft.section_key, &draft.text_tagged)
                .and_then(|i| docs.get(i).zip(from.get(i)))
                .map(|((_, text), (report, section))| SimilarParagraph {
                    text: (*text).to_owned(),
                    report: (*report).to_owned(),
                    same_section: *section == Some(draft.section_key.as_str()),
                }),
        )
    }

    /// Help beside a section she writes herself (round 30), all from her past reports and her
    /// approved profile, on this computer: how she wrote this section before, how she usually
    /// opens and closes it, and her phrases for it. Nothing is sent.
    pub fn own_section_help(&mut self, section_key: &str) -> Result<OwnSectionHelp, CoreError> {
        let mut sources = self.stored_sources()?;
        // Newest report first.
        sources.sort_by_key(|(_, at, _)| std::cmp::Reverse(*at));
        let mut help = OwnSectionHelp::default();
        let push = |list: &mut Vec<String>, line: &str| {
            let line = format!("{}.", line.trim_end_matches(['.', ',', ':']));
            if list.len() < OWN_HELP_LINES && !list.contains(&line) {
                list.push(line);
            }
        };
        for (_, _, src) in &sources {
            let paras: Vec<&str> = src
                .parts
                .iter()
                .filter(|p| p.section.as_deref() == Some(section_key))
                .flat_map(|p| dv_style::text::paragraphs(&p.text))
                .collect();
            let (Some(first), Some(last)) = (paras.first(), paras.last()) else {
                continue;
            };
            if help.examples.len() < OWN_HELP_EXAMPLES {
                help.examples.push(SimilarParagraph {
                    text: (*first).to_owned(),
                    report: src.title.clone(),
                    same_section: true,
                });
            }
            if let Some(s) = dv_style::text::sentences(first).first() {
                push(&mut help.openings, s);
            }
            if let Some(s) = dv_style::text::sentences(last).last() {
                push(&mut help.closings, s);
            }
        }
        if let Some(p) = self.active_profile()? {
            help.phrases = p
                .profile
                .items
                .into_iter()
                .filter(|i| i.enabled && i.section.as_deref() == Some(section_key))
                .filter(|i| matches!(i.kind, StyleKind::Phrase | StyleKind::Template))
                .map(|i| i.text)
                .take(OWN_HELP_LINES)
                .collect();
        }
        Ok(help)
    }

    /// What was learned from her edits, for marking her usual changes in a draft.
    pub(crate) fn style_habits(&mut self) -> Result<Learning, CoreError> {
        self.learning()
    }

    fn learning(&mut self) -> Result<Learning, CoreError> {
        match self.vault_ref()?.style_learning()? {
            Some(j) => parse(&j),
            None => Ok(Learning::default()),
        }
    }

    // -------------------------------------------------------- past reports

    /// Room for one more past report.
    pub(crate) fn style_room(&mut self) -> Result<(), CoreError> {
        if self.vault_ref()?.style_sources()?.len() >= MAX_SOURCES {
            return Err(CoreError::Refused(format!(
                "אפשר לשמור עד {MAX_SOURCES} דוחות. כדאי למחוק דוח ישן לפני שמוסיפים חדש."
            )));
        }
        Ok(())
    }

    /// A past report already split and neutralized, waiting for her choice of parts
    /// ([`Core::save_style_source`]). Done again here when a name changed meanwhile.
    pub(crate) fn stage_style(
        &mut self,
        work: StyleWork,
        file_name: &str,
        extracted: &dv_ingest::Extracted,
    ) -> Result<StyleImportPreview, CoreError> {
        let data = self.style_data()?;
        let work = if work.stamp == data.stamp() {
            work
        } else {
            style_work(&data, file_name, extracted)?
        };
        let token = dv_vault::crypto::random_id()?;
        let views = work
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| StylePartView {
                index: count(i),
                section: p.section.clone(),
                heading: p.heading.clone(),
                text: p.text.clone(),
                words: words(&p.text),
                included: p
                    .section
                    .as_deref()
                    .is_some_and(|k| DEFAULT_INCLUDED.contains(&k)),
            })
            .collect();
        self.style_staged.clear();
        self.style_staged.insert(
            token.clone(),
            StagedStyle {
                title: work.title.clone(),
                format: work.format.clone(),
                parts: work.parts,
                names: work.names,
                child: work.child,
            },
        );
        Ok(StyleImportPreview {
            token,
            title: work.title,
            format: work.format,
            parts: views,
            hidden: work.hidden,
            numbers: work.numbers,
            warnings: work.warnings,
        })
    }

    /// Keep the parts she chose (by index), neutralized. `title`: her own name for it.
    pub fn save_style_source(
        &mut self,
        token: &str,
        included: &[u32],
        title: Option<&str>,
    ) -> Result<StyleSourceView, CoreError> {
        let staged = self.style_staged.remove(token).ok_or_else(|| {
            CoreError::NotFound("ההעלאה פגה. אפשר לבחור את הקובץ שוב.".to_owned())
        })?;
        let parts: Vec<StoredPart> = staged
            .parts
            .into_iter()
            .enumerate()
            .filter(|(i, p)| included.contains(&count(*i)) && !p.text.trim().is_empty())
            .map(|(_, p)| p)
            .collect();
        if parts.is_empty() {
            return Err(CoreError::Refused(
                "צריך לסמן לפחות קטע אחד לשמירה.".to_owned(),
            ));
        }
        let title = match title.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => self.with_style_ctx(|ctx, _| Ok(neutralize(t, ctx, None)?.0))?,
            None => staged.title,
        };
        let stored = StoredSource {
            title: title.chars().take(TITLE_CHARS).collect(),
            format: staged.format,
            parts,
            analysis: None,
        };
        let id = self.vault_mut()?.save_style_source(None, &json(&stored)?)?;
        let mut names = past_names(self.vault_ref()?)?;
        names.insert(id.clone(), staged.names);
        self.vault_mut()?
            .set_secret(PAST_NAMES_KEY, &json(&names)?)?;
        if let Some(child) = staged.child {
            let mut children = past_children(self.vault_ref()?)?;
            children.insert(id.clone(), child);
            self.vault_mut()?
                .set_secret(CHILDREN_KEY, &json(&children)?)?;
        }
        self.remeasure_style()?;
        Ok(Self::source_view(id, unix_now(), &stored))
    }

    /// Learn again from her past reports after they changed (D-052).
    fn remeasure_style(&mut self) -> Result<(), CoreError> {
        let rows = self.vault_ref()?.style_sources()?;
        let sources: Vec<StoredSource> = rows
            .iter()
            .map(|s| parse(&s.json))
            .collect::<Result<_, _>>()?;
        let ids: Vec<String> = rows.into_iter().map(|r| r.id).collect();
        let children = child_groups(self.vault_ref()?, &ids)?;
        let mut local = local_style(self.vault_ref()?)?;
        fit_reports(&mut local, &sources, &children);
        let v = self.vault_mut()?;
        v.set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        v.set_secret(OLD_FINGERPRINT_KEY, "null")?;
        Ok(())
    }

    pub fn discard_style_upload(&mut self) {
        self.style_staged.clear();
    }

    fn source_view(id: String, added_at: i64, s: &StoredSource) -> StyleSourceView {
        StyleSourceView {
            id,
            title: s.title.clone(),
            format: s.format.clone(),
            added_at,
            words: s.parts.iter().map(|p| words(&p.text)).sum(),
            headings: s.parts.iter().map(|p| p.heading.clone()).collect(),
            analyzed: s.analysis.is_some(),
            analysis_demo: s.analysis.as_ref().is_some_and(|a| a.demo),
            analysis_items: s.analysis.as_ref().map_or(0, |a| count(a.items.len())),
        }
    }

    /// Erase a past report. A profile already approved stays as it is until rebuilt.
    pub fn delete_style_source(&mut self, id: &str) -> Result<(), CoreError> {
        self.vault_mut()?.delete_style_source(id)?;
        let mut names = past_names(self.vault_ref()?)?;
        if names.remove(id).is_some() {
            self.vault_mut()?
                .set_secret(PAST_NAMES_KEY, &json(&names)?)?;
        }
        let mut children = past_children(self.vault_ref()?)?;
        if children.remove(id).is_some() {
            self.vault_mut()?
                .set_secret(CHILDREN_KEY, &json(&children)?)?;
        }
        self.remeasure_style()
    }

    // -------------------------------------------------------- the screen

    pub fn style_overview(&mut self) -> Result<StyleOverview, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let demo_mode = self.demo_mode()?;
        let stored = self.stored_sources()?;
        let scored_measures = interpret::scored_measures(
            &stored.iter().map(|(_, _, s)| s.clone()).collect::<Vec<_>>(),
        );
        let sources = stored
            .into_iter()
            .map(|(id, at, s)| Self::source_view(id, at, &s))
            .collect();
        let learning = self.learning()?;
        let v = self.vault_ref()?;
        let enabled = v.setting(PROFILE_OFF_KEY)?.as_deref() != Some("1");
        let profiles = v.style_profiles()?;
        let mut versions = Vec::new();
        let (mut active, mut draft) = (None, None);
        for p in profiles {
            let view = Self::profile_view(p)?;
            if view.status != "draft" {
                versions.push(StyleVersionView {
                    id: view.id.clone(),
                    version: view.version,
                    status: view.status.clone(),
                    created_at: view.created_at,
                    items: count(view.profile.items.len()),
                    reports: view.profile.reports,
                });
            }
            match view.status.as_str() {
                "active" if active.is_none() => active = Some(view),
                "draft" if draft.is_none() => draft = Some(view),
                _ => {}
            }
        }
        let local = local_style(v)?;
        let suggestions = suggestions(&learning, &local, active.as_ref());
        Ok(StyleOverview {
            sources,
            active,
            draft,
            versions,
            suggestions,
            enabled,
            sections: structure
                .sections()
                .filter(|s| s.key != "signature")
                .map(|s| StyleSectionLabel {
                    key: s.key.clone(),
                    title: s.title.clone(),
                })
                .collect(),
            demo_mode,
            learned: local.learned(),
            interpretations: Self::interpretation_views(v)?,
            scored_measures,
            skeletons: local
                .skeletons
                .iter()
                .filter_map(|k| {
                    let title = [None, Some(dv_domain::INSTITUTE_TEMPLATE)]
                        .into_iter()
                        .filter_map(|t| ReportStructure::for_template(t).ok())
                        .find_map(|st| st.section(&k.section).map(|x| x.title.clone()))?;
                    Some(SkeletonView {
                        section: k.section.clone(),
                        title,
                        band: k.band.map(|b| b.label().to_owned()),
                        reports: k.reports,
                        topics: k.topics.iter().map(|t| t.label.clone()).collect(),
                    })
                })
                .collect(),
            omissions: omissions(local.habits),
        })
    }

    /// Use the approved profile in drafting requests, or go back to the default style.
    pub fn set_style_enabled(&mut self, on: bool) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(PROFILE_OFF_KEY, if on { "0" } else { "1" })?;
        Ok(())
    }

    // -------------------------------------------------------- editing and approving

    fn active_profile(&mut self) -> Result<Option<StoredProfile>, CoreError> {
        self.vault_ref()?
            .style_profiles()?
            .into_iter()
            .find(|p| p.status == "active")
            .map(|p| parse(&p.json))
            .transpose()
    }

    fn store_draft(&mut self, stored: &StoredProfile) -> Result<StyleProfileView, CoreError> {
        let v = self.vault_mut()?;
        v.delete_style_drafts()?;
        v.add_style_profile("draft", &json(stored)?)?;
        let draft = v
            .style_profiles()?
            .into_iter()
            .find(|p| p.status == "draft")
            .ok_or_else(|| CoreError::Internal("draft".to_owned()))?;
        Self::profile_view(draft)
    }

    /// Check every item she wrote or changed: a name or other detail in it would go out with
    /// every request (and stop the gate for that case), so it is refused, with the item shown.
    fn check_items(&mut self, profile: &mut StyleProfile) -> Result<(), CoreError> {
        for item in &mut profile.items {
            item.text = item.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if item.id.is_empty() {
                item.id = dv_vault::crypto::random_id()?;
                item.origin = StyleOrigin::Manual;
            }
        }
        profile.items.retain(|i| !i.text.is_empty());
        let texts: Vec<String> = profile.items.iter().map(|i| i.text.clone()).collect();
        self.with_style_ctx(|ctx, _| {
            for t in &texts {
                if t.contains('[') || t.contains(']') || t.contains('<') || t.contains('>') {
                    return Err(CoreError::Refused(format!(
                        "בפריט «{t}» יש סוגריים מרובעים או משולשים. אפשר לכתוב \"הילד\" או את התפקיד במילים, ולמקום ריק בתבנית להשתמש בסוגריים מסולסלים."
                    )));
                }
                if !clean(t, ctx) {
                    return Err(CoreError::Refused(format!(
                        "בפריט «{t}» יש מילה שנראית כמו שם או פרט מזהה, והוא היה יוצא בכל בקשה. אפשר לנסח אותו מחדש."
                    )));
                }
            }
            Ok(())
        })
    }

    /// Save her edits of the profile (toggles, wording, deletions, new rules) as the draft.
    pub fn save_style_draft(
        &mut self,
        mut profile: StyleProfile,
    ) -> Result<StyleProfileView, CoreError> {
        self.check_items(&mut profile)?;
        let current = self
            .vault_ref()?
            .style_profiles()?
            .into_iter()
            .find(|p| p.status == "draft")
            .map(|p| parse::<StoredProfile>(&p.json))
            .transpose()?;
        let (dropped, demo) = current.map_or((0, false), |c| (c.dropped, c.demo));
        self.store_draft(&StoredProfile {
            profile,
            dropped,
            demo,
        })
    }

    /// The draft becomes the profile in use (a new version).
    pub fn approve_style_draft(&mut self) -> Result<StyleProfileView, CoreError> {
        let v = self.vault_mut()?;
        let draft = v
            .style_profiles()?
            .into_iter()
            .find(|p| p.status == "draft")
            .ok_or_else(|| CoreError::NotFound("אין טיוטה לאישור.".to_owned()))?;
        let mut stored: StoredProfile = parse(&draft.json)?;
        // Checked again: the draft was written by this program, but approval is the last door.
        self.check_items(&mut stored.profile)?;
        let v = self.vault_mut()?;
        v.delete_style_drafts()?;
        let (id, _) = v.add_style_profile("active", &json(&stored)?)?;
        let active = v
            .style_profiles()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| CoreError::Internal("profile".to_owned()))?;
        Self::profile_view(active)
    }

    pub fn discard_style_draft(&mut self) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.delete_style_drafts()?)
    }

    /// Bring back an earlier version: it becomes the profile in use, as a new version.
    pub fn restore_style_version(&mut self, id: &str) -> Result<StyleProfileView, CoreError> {
        let v = self.vault_mut()?;
        let old = v
            .style_profiles()?
            .into_iter()
            .find(|p| p.id == id && p.status != "draft")
            .ok_or_else(|| CoreError::NotFound("הגרסה".to_owned()))?;
        let (new_id, _) = v.add_style_profile("active", &old.json)?;
        let active = v
            .style_profiles()?
            .into_iter()
            .find(|p| p.id == new_id)
            .ok_or_else(|| CoreError::Internal("profile".to_owned()))?;
        Self::profile_view(active)
    }

    /// Erase every profile version and everything learned from her edits. Past reports stay.
    pub fn reset_style(&mut self) -> Result<(), CoreError> {
        self.vault_mut()?.reset_style()?;
        self.vault_mut()?
            .set_secret(PAST_NAMES_KEY, &json(&PastNames::new())?)?;
        self.vault_mut()?.set_secret(CHILDREN_KEY, "{}")?;
        self.vault_mut()?
            .set_secret(LOCAL_STYLE_KEY, &json(&LocalStyle::default())?)?;
        self.vault_mut()?.set_secret(OLD_FINGERPRINT_KEY, "null")?;
        Ok(())
    }
}

#[cfg(test)]
mod unit {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn tags_lose_their_numbers_only() {
        assert_eq!(role_only("[אח_2]"), "[אח_1]");
        assert_eq!(role_only("[ילד]"), "[ילד]");
        assert_eq!(role_only("[גננת]"), "[גננת]");
        assert_eq!(role_only("[ילד_גן_3]"), "[ילד_גן_1]");
        assert_eq!(role_only("[אדם_4]"), "[אדם_1]");
        assert_eq!(role_only("[טלפון]"), "[טלפון]");
        assert_eq!(role_only("המוסד"), "המוסד");
    }

    #[test]
    fn numbers_are_masked_but_not_test_editions_or_tags() {
        let (t, n) = mask_numbers("הציג תפקוד ממוצע (9), בגיל 5:4, ציון 112. WISC-5 ו-[אח_2]");
        assert_eq!(
            t,
            "הציג תפקוד ממוצע ([מספר]), בגיל [מספר], ציון [מספר]. WISC-5 ו-[אח_2]"
        );
        assert_eq!(n, 3);
    }

    #[test]
    fn headings_find_their_sections() {
        let s = ReportStructure::load_default().unwrap();
        let key = |l: &str| heading_key(l, &s);
        assert_eq!(key("סיכום והמלצות"), Some(Some("summary".into())));
        assert_eq!(key("פרופיל קוגניטיבי:"), Some(Some("cognitive".into())));
        assert_eq!(key("אבחנה על-פי DSM-5"), Some(Some("dsm".into())));
        assert_eq!(key("המלצות"), Some(Some("recommendations".into())));
        assert_eq!(key("משחק ומישור רגשי"), Some(Some("emotional".into())));
        assert_eq!(key("נושאים נוספים:"), Some(None));
        assert_eq!(
            key("הילד הגיע למפגש בשמחה ושיתף פעולה לאורך כל הזמן."),
            None
        );
        assert_eq!(
            key("ארגון"),
            None,
            "גן inside another word is not a heading"
        );
    }

    #[test]
    fn a_report_is_cut_by_its_headings() {
        let s = ReportStructure::load_default().unwrap();
        let parts = split_parts(
            "סיכום אבחון פסיכולוגי\nתאריך\n\nהופעה והתרשמות\nהגיע בשמחה.\nפרופיל קוגניטיבי:\nציון ממוצע.\nמבחנים מילוליים:\nהבנה טובה.\nסיכום\nלסיכום, ילד סקרן.",
            &s,
        );
        let got: Vec<(Option<&str>, &str)> = parts
            .iter()
            .map(|p| (p.section.as_deref(), p.text.as_str()))
            .collect();
        assert_eq!(got[0].0, None, "the header before any heading");
        assert_eq!(got[1], (Some("appearance"), "הגיע בשמחה."));
        assert_eq!(got[2].0, Some("cognitive"));
        assert!(got.iter().any(|(k, _)| *k == Some("summary")));
    }

    #[test]
    fn a_run_in_three_reports_is_her_wording_and_one_in_a_single_report_is_content() {
        let source = |text: &str| StoredSource {
            title: String::new(),
            format: "txt".into(),
            parts: vec![StoredPart {
                section: None,
                heading: String::new(),
                text: text.to_owned(),
            }],
            analysis: None,
        };
        let wording = "המבחן הוא מבחן אינטליגנציה לילדים בגילאי הגן";
        let sources: Vec<StoredSource> = (0..3)
            .map(|i| {
                if i == 0 {
                    source(&format!("{wording}. [ילד] בנה מגדל גבוה מקוביות צבעוניות"))
                } else {
                    source(wording)
                }
            })
            .collect();
        let index = source_index(&sources, &[0, 1, 2], 5);
        assert!(shared_run(wording, 5, &index).is_none(), "her wording");
        assert!(shared_run("גם [ילד] בנה מגדל גבוה מקוביות", 5, &index).is_some());
        assert!(
            shared_run(wording, 5, &source_index(&sources[..2], &[0, 1], 5)).is_some(),
            "in two reports it may still be about a child"
        );
        // The review of stage 4: three reports of one child are one child, so what they share
        // may be about that child, and a draft repeating it is still warned.
        assert!(
            shared_run(wording, 5, &source_index(&sources, &[7, 7, 7], 5)).is_some(),
            "three reports of the same child"
        );
    }

    #[test]
    fn overlap_finds_shared_runs_with_tags_by_role() {
        let index = ngrams("[ילד] מגיב היטב לתיווך של המבוגר בחדר", 5);
        assert!(shared_run("גם [ילד] מגיב היטב לתיווך של המבוגר", 5, &index).is_some());
        assert!(shared_run("מגיב היטב לתיווך בכל מצב", 5, &index).is_none());
    }

    #[test]
    fn a_request_gets_the_items_closest_to_its_material() {
        let item = |n: usize, kind: StyleKind, text: String| StyleItem {
            id: format!("i{n}"),
            section: Some("observation".into()),
            kind,
            text,
            enabled: true,
            origin: StyleOrigin::Reports,
            support: 1,
        };
        let topics = [
            "ציור",
            "שפה",
            "ריצה",
            "קשב",
            "משחק",
            "שינה",
            "אכילה",
            "חברים",
        ];
        let mut items: Vec<StyleItem> = topics
            .iter()
            .enumerate()
            .map(|(n, t)| item(n, StyleKind::Template, format!("בתחום ה{t} ניכר {{רמה}}.")))
            .collect();
        items.push(item(99, StyleKind::Rule, "משפטים קצרים.".into()));
        let profile = StyleProfile { items, reports: 3 };
        let focused = focus(&profile, "observation", "הגננת מספרת על קושי בשינה ובאכילה");
        let texts: Vec<&str> = focused.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(
            focused.items.len(),
            7,
            "six templates and the rule: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("שינה")) && texts.iter().any(|t| t.contains("אכילה"))
        );
        assert!(texts.contains(&"משפטים קצרים."));
    }

    #[test]
    fn edits_yield_short_replacements_only() {
        let r = |a: &str, b: &str| dv_style::edits::changes(a, b).0;
        assert_eq!(
            r(
                "[ילד] מראה קושי בוויסות, ונראה כי הוא זקוק לתיווך רב.",
                "[ילד] מפגין קושי בוויסות, וניכר כי הוא זקוק לתיווך רב.",
            ),
            vec![
                ("מראה".to_owned(), "מפגין".to_owned()),
                ("ונראה".to_owned(), "וניכר".to_owned())
            ]
        );
        assert!(r("ציון 9", "ציון 11").is_empty(), "numbers are not style");
        assert!(r("[ילד] הגיע", "[ילד_2] הגיע").is_empty());
        assert!(
            r("א ב ג ד ה ו", "ז ח ט י כ ל").is_empty(),
            "a rewritten sentence is not a replacement"
        );
    }
}

#[cfg(test)]
mod flows;
