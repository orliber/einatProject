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

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use dv_ai::{RawStyleItem, StyleExcerpt, StyleItem, StyleKind, StyleOrigin, StyleProfile};
use dv_domain::{Author, DraftParagraph, Identity, ReportStructure, Role};
use dv_privacy::text::normalize;
use dv_privacy::{
    clear, everyday_name, filter, AutoHidden, AutoKind, FilterOutcome, GateRequest, Mark,
    PrivacyContext,
};
use dv_vault::{AuditEvent, Vault};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::dates::unix_now;
use crate::{
    today, Core, CoreError, Outgoing, Pending, PendingKind, Prepared, Review, Task,
    MAX_REQUEST_BYTES,
};

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

/// Words in a heading that point to a template section. Checked in this order ("סיכום
/// והמלצות" is the summary; "אבחנה על-פי DSM" is the DSM section, not the diagnoses).
const HEADING_WORDS: &[(&str, &[&str])] = &[
    ("dsm", &["dsm"]),
    ("summary", &["סיכום"]),
    ("recommendations", &["המלצות", "המלצה"]),
    ("referral", &["סיבת הפניה", "סיבת ההפניה", "הפניה"]),
    (
        "parents_view",
        &[
            "לפי ההורים",
            "דיווח ההורים",
            "תיאור ההורים",
            "תפקוד והתנהגות",
        ],
    ),
    (
        "background",
        &[
            "רקע",
            "הריון",
            "לידה",
            "אבני דרך",
            "היסטוריה התפתחותית",
            "התפתחות מוקדמת",
        ],
    ),
    (
        "prior_assessments",
        &["אבחונים", "טיפולים", "אבחון קודם", "בדיקות קודמות"],
    ),
    (
        "kindergarten",
        &["מסגרת", "גן", "בית ספר", "בית הספר", "דיווח הצוות", "גננת"],
    ),
    (
        "tools",
        &["כלי האבחון", "כלי אבחון", "כלים", "מבחנים שהועברו"],
    ),
    ("adaptive", &["הסתגלות", "abas", "תפקוד מסתגל"]),
    (
        "cognitive",
        &[
            "קוגניטיבי",
            "קוגניטיבית",
            "אינטליגנציה",
            "wppsi",
            "wisc",
            "וכסלר",
            "וקסלר",
            "bayley",
            "ביילי",
            "יכולות חשיבה",
        ],
    ),
    (
        "communication",
        &["תקשורת", "ados", "asrs", "cars", "שפה ודיבור"],
    ),
    ("emotional", &["רגשי", "רגשית", "משחק", "השלכתי", "htp"]),
    (
        "appearance",
        &["הופעה", "התרשמות", "תצפית", "התנהגות במהלך"],
    ),
    ("diagnoses", &["אבחנות", "אבחנה"]),
];

/// Words of a report's title line, which is not a section heading.
const REPORT_TITLES: &[&str] = &[
    "סיכום אבחון",
    "דוח אבחון",
    "דו\"ח אבחון",
    "חוות דעת",
    "אבחון פסיכולוגי",
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
}

/// An upload waiting for her confirmation (memory only, cleared at lock).
pub(crate) struct StagedStyle {
    title: String,
    format: String,
    parts: Vec<StoredPart>,
    /// Keyed hashes of the names found in the report (its child, people, the margins).
    names: Vec<String>,
}

/// Keyed hashes of the names in her past reports, by report: hidden in every case and
/// refused by the gate ("שם מדוח ישן"). No name text is kept.
const PAST_NAMES_KEY: &str = "style_past_names";

type PastNames = BTreeMap<String, Vec<String>>;

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
fn report_names(outs: &[FilterOutcome], child: Option<&str>, v: &Vault) -> Vec<String> {
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
                let role = if names.is_empty() {
                    None
                } else if names
                    .iter()
                    .any(|a| child.is_some_and(|c| normalize(&a.token) == c))
                {
                    Some(Role::Child)
                } else {
                    // "Another child" reads wrong in a report about one child.
                    Some(match names[0].role {
                        Role::OtherChild | Role::Child => Role::Other,
                        r => r,
                    })
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
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && c.is_ascii_digit() {
            let latin_before = i > 0
                && (chars[i - 1].is_ascii_alphabetic()
                    || (chars[i - 1] == '-' && i > 1 && chars[i - 2].is_ascii_alphabetic()));
            let mut j = i;
            while j < chars.len()
                && (chars[j].is_ascii_digit()
                    || (matches!(chars[j], '.' | ':' | '/' | '-' | ',')
                        && j + 1 < chars.len()
                        && chars[j + 1].is_ascii_digit()))
            {
                j += 1;
            }
            if latin_before {
                out.extend(&chars[i..j]);
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

// ------------------------------------------------------------ sections

/// A heading's words, normalized, with the common one-letter prefixes as separate readings.
fn heading_matches(norm: &str, keyword: &str) -> bool {
    let keyword = normalize(keyword).to_lowercase();
    let keyword = keyword.as_str();
    if keyword.contains(' ') {
        return norm.contains(keyword);
    }
    norm.split(|c: char| !c.is_alphanumeric()).any(|w| {
        let mut forms = vec![w];
        let mut rest = w;
        for _ in 0..2 {
            match rest.chars().next() {
                Some(p @ ('ו' | 'ה' | 'ב' | 'ל' | 'ש' | 'מ')) => {
                    rest = &rest[p.len_utf8()..];
                    forms.push(rest);
                }
                _ => break,
            }
        }
        forms
            .iter()
            .any(|f| *f == keyword || (keyword.chars().count() >= 4 && f.starts_with(keyword)))
    })
}

/// `Some(Some(key))`: a heading of that section. `Some(None)`: a heading of something else.
/// `None`: not a heading.
fn heading_key(line: &str, structure: &ReportStructure) -> Option<Option<String>> {
    let line = line.trim();
    let n = line.chars().count();
    let colon = line.ends_with(':');
    let word_count = line.split_whitespace().count();
    if !(2..=70).contains(&n) || line.ends_with('.') || word_count > if colon { 9 } else { 6 } {
        return None;
    }
    let norm = normalize(line.trim_end_matches(':')).to_lowercase();
    // The report's own title ("סיכום אבחון פסיכולוגי") is not its summary section.
    if REPORT_TITLES
        .iter()
        .any(|t| norm.contains(&normalize(t).to_lowercase()))
    {
        return None;
    }
    for s in structure.sections() {
        if normalize(&s.title).to_lowercase() == norm {
            return Some(Some(s.key.clone()));
        }
    }
    for (key, keywords) in HEADING_WORDS {
        if keywords.iter().any(|k| heading_matches(&norm, k)) {
            return Some(Some((*key).to_owned()));
        }
    }
    colon.then_some(None)
}

/// Cut a report into parts by its headings. Text before the first heading is its own part
/// (usually the header: names and dates), not included by default.
fn split_parts(body: &str, structure: &ReportStructure) -> Vec<StoredPart> {
    let mut parts: Vec<StoredPart> = Vec::new();
    let mut current = StoredPart {
        section: None,
        heading: "פתיחת הדוח".to_owned(),
        text: String::new(),
    };
    for line in body.lines() {
        if let Some(key) = heading_key(line, structure) {
            if !current.text.trim().is_empty() {
                parts.push(current);
            }
            current = StoredPart {
                section: key,
                heading: line.trim().trim_end_matches(':').trim().to_owned(),
                text: String::new(),
            };
        } else {
            current.text.push_str(line);
            current.text.push('\n');
        }
    }
    if !current.text.trim().is_empty() {
        parts.push(current);
    }
    // Two parts of the same section in a row (a sub-heading) are one excerpt.
    let mut merged: Vec<StoredPart> = Vec::new();
    for p in parts {
        match merged.last_mut() {
            Some(last) if p.section.is_some() && last.section == p.section => {
                last.text.push_str(&p.text);
            }
            _ => merged.push(p),
        }
    }
    for p in &mut merged {
        p.text = p.text.trim().to_owned();
    }
    merged
}

/// Every template key the profile can describe (the signature has no prose).
fn section_keys(structure: &ReportStructure) -> Vec<String> {
    structure
        .sections()
        .filter(|s| s.key != "signature")
        .map(|s| s.key.clone())
        .collect()
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

fn source_index(sources: &[StoredSource], n: usize) -> HashSet<String> {
    sources
        .iter()
        .flat_map(|s| s.parts.iter())
        .flat_map(|p| ngrams(&p.text, n))
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
                .map(|n| profile.items[*n].text.as_str())
                .collect();
            let kept: HashSet<usize> = dv_style::retrieve::best(&texts, query, budget)
                .into_iter()
                .map(|k| group[k])
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
    let sources: Vec<StoredSource> = v
        .style_sources()?
        .iter()
        .map(|s| parse(&s.json))
        .collect::<Result<_, _>>()?;
    if sources.is_empty() {
        return Ok(vec![None; texts.len()]);
    }
    let index = source_index(&sources, DRAFT_NGRAM);
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

/// Her style model for the case screens, or `None` when the profile is switched off.
pub(crate) fn style_model(v: &Vault) -> Result<Option<dv_style::StyleModel>, CoreError> {
    if v.setting(PROFILE_OFF_KEY)?.as_deref() == Some("1") {
        return Ok(None);
    }
    Ok(Some(local_style(v)?.model))
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
fn fit_reports(local: &mut LocalStyle, sources: &[StoredSource]) {
    local.model.fit_reports(
        sources
            .iter()
            .flat_map(|s| s.parts.iter())
            .map(|p| (p.section.as_deref(), p.text.as_str())),
    );
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
    /// The privacy context for past reports: every case's names, each hidden by its role; the
    /// practitioner; the words marked "not a name" for every case.
    fn with_style_ctx<R>(
        &mut self,
        f: impl FnOnce(&PrivacyContext<'_>, &Vault) -> Result<R, CoreError>,
    ) -> Result<R, CoreError> {
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
        let practitioner = v.practitioner()?.names;
        let allow: HashSet<String> = v.not_a_name_hmacs("")?.into_iter().collect();
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let allowed = |t: &str| allow.contains(&v.token_hmac(t));
        let none = |_: &str| false;
        let ctx = PrivacyContext {
            case_id: STYLE_CASE,
            identities: &identities,
            practitioner: &practitioner,
            allowlisted: &allowed,
            confirmed_names: &none,
            past_names: &none,
            today: today(),
        };
        f(&ctx, v)
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

    fn learning(&mut self) -> Result<Learning, CoreError> {
        match self.vault_ref()?.style_learning()? {
            Some(j) => parse(&j),
            None => Ok(Learning::default()),
        }
    }

    // -------------------------------------------------------- past reports

    /// Read a past report and show what would be kept. Nothing is stored until she confirms
    /// with [`Core::save_style_source`].
    pub fn import_style_source(
        &mut self,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<StyleImportPreview, CoreError> {
        if self.vault_ref()?.style_sources()?.len() >= MAX_SOURCES {
            return Err(CoreError::Refused(format!(
                "אפשר לשמור עד {MAX_SOURCES} דוחות. כדאי למחוק דוח ישן לפני שמוסיפים חדש."
            )));
        }
        let extracted = self.read_document(file_name, bytes)?;
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let raw_parts = split_parts(&extracted.body, &structure);
        let stem = Path::new(file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("דוח")
            .to_owned();
        let body = extracted.body.clone();
        let margins = extracted.margins.clone();
        let (parts, title, hidden, numbers, names) = self.with_style_ctx(|ctx, v| {
            let child = main_child(&body, ctx)?;
            let child = child.as_deref();
            let run = |t: &str| filter(t, ctx).map_err(|e| CoreError::Internal(e.to_string()));
            let names = report_names(&[run(&body)?, run(&margins)?], child, v);
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
            Ok((parts, title, hidden, numbers, names))
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
        let token = dv_vault::crypto::random_id()?;
        let views = parts
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
        let format = extracted.format.name().to_owned();
        let title: String = title.chars().take(TITLE_CHARS).collect();
        self.style_staged.clear();
        self.style_staged.insert(
            token.clone(),
            StagedStyle {
                title: title.clone(),
                format: format.clone(),
                parts,
                names,
            },
        );
        Ok(StyleImportPreview {
            token,
            title,
            format,
            parts: views,
            hidden,
            numbers,
            warnings,
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
        self.remeasure_style()?;
        Ok(Self::source_view(id, unix_now(), &stored))
    }

    /// Learn again from her past reports after they changed (D-052).
    fn remeasure_style(&mut self) -> Result<(), CoreError> {
        let sources: Vec<StoredSource> = self
            .vault_ref()?
            .style_sources()?
            .iter()
            .map(|s| parse(&s.json))
            .collect::<Result<_, _>>()?;
        let mut local = local_style(self.vault_ref()?)?;
        fit_reports(&mut local, &sources);
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
        self.remeasure_style()
    }

    // -------------------------------------------------------- the screen

    pub fn style_overview(&mut self) -> Result<StyleOverview, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let demo_mode = self.demo_mode()?;
        let sources = self
            .stored_sources()?
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
        let suggestions = suggestions(&learning, &local_style(v)?, active.as_ref());
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
        })
    }

    /// Use the approved profile in drafting requests, or go back to the default style.
    pub fn set_style_enabled(&mut self, on: bool) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(PROFILE_OFF_KEY, if on { "0" } else { "1" })?;
        Ok(())
    }

    // -------------------------------------------------------- sending

    /// A gate of its own: the past reports' context, and only role tags.
    fn gate_style(
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
    fn keep_items(
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
        self.vault_mut()?
            .set_secret(LOCAL_STYLE_KEY, &json(&LocalStyle::default())?)?;
        self.vault_mut()?.set_secret(OLD_FINGERPRINT_KEY, "null")?;
        Ok(())
    }

    // -------------------------------------------------------- learning from edits

    /// Learn from her edit of the AI's paragraph in `section`: the changes she made, and her
    /// final wording into the style model. Local only (D-052).
    pub(crate) fn learn_from_edit(
        &mut self,
        section: &str,
        before: &str,
        after: &str,
    ) -> Result<(), CoreError> {
        let mut learning = self.learning()?;
        learning.observe_edit(section, before, after);
        let mut local = local_style(self.vault_ref()?)?;
        local.model.observe_edit(section, after);
        let v = self.vault_mut()?;
        v.save_style_learning(&json(&learning)?)?;
        v.set_secret(LOCAL_STYLE_KEY, &json(&local)?)?;
        Ok(())
    }

    /// Her manual edit of a paragraph Claude wrote: learn from it (never blocks the edit).
    pub(crate) fn learn_from_draft_edit(&mut self, old: Option<DraftParagraph>, new_tagged: &str) {
        if let Some(old) = old.filter(|d| d.author == Author::Ai) {
            // Learning is a convenience: an error here must not lose her edit.
            let _ = self.learn_from_edit(&old.section_key, &old.text_tagged, new_tagged);
        }
    }

    /// She approved the AI's paragraphs as written: the phrases in them were accepted. Never
    /// blocks the approval.
    pub(crate) fn learn_from_approval(&mut self, approved: &[DraftParagraph]) {
        let texts: Vec<&str> = approved
            .iter()
            .filter(|d| d.author == Author::Ai)
            .map(|d| d.text_tagged.as_str())
            .collect();
        if texts.is_empty() {
            return;
        }
        let Ok(mut learning) = self.learning() else {
            return;
        };
        for t in texts {
            learning.observe_approval(t);
        }
        if let (Ok(doc), Ok(v)) = (json(&learning), self.vault_mut()) {
            let _ = v.save_style_learning(&doc);
        }
    }

    /// Add a suggestion to the profile in use, as a new version (her click is the approval).
    pub fn accept_style_suggestion(&mut self, id: &str) -> Result<StyleProfileView, CoreError> {
        let mut learning = self.learning()?;
        let local = local_style(self.vault_ref()?)?;
        let not_found = || CoreError::NotFound("ההצעה".to_owned());
        let item = if let Some(norm) = id.strip_prefix(PHRASE_ID) {
            let p = local
                .phrases
                .iter()
                .find(|p| p.norm == norm)
                .ok_or_else(not_found)?;
            learning.dismissed_phrases.push(p.norm.clone());
            (
                p.section.clone(),
                StyleKind::Phrase,
                p.text.clone(),
                StyleOrigin::Reports,
                p.reports,
            )
        } else {
            let section = learning
                .suggestions()
                .into_iter()
                .find(|s| s.id == id)
                .and_then(|s| s.section);
            let pair = learning.find_mut(id).ok_or_else(not_found)?;
            pair.accepted = true;
            let (kind, text) = if pair.is_deletion() {
                (StyleKind::Avoid, format!("לא לכתוב «{}»", pair.from))
            } else {
                (
                    StyleKind::Rule,
                    format!("לכתוב «{}» ולא «{}»", pair.to, pair.from),
                )
            };
            (section, kind, text, StyleOrigin::Edits, 0)
        };
        let (section, kind, text, origin, support) = item;
        let mut stored = self.active_profile()?.unwrap_or(StoredProfile {
            profile: StyleProfile::default(),
            dropped: 0,
            demo: false,
        });
        stored.profile.items.push(StyleItem {
            id: dv_vault::crypto::random_id()?,
            section,
            kind,
            text,
            enabled: true,
            origin,
            support,
        });
        self.check_items(&mut stored.profile)?;
        let v = self.vault_mut()?;
        let (new_id, _) = v.add_style_profile("active", &json(&stored)?)?;
        v.save_style_learning(&json(&learning)?)?;
        let active = v
            .style_profiles()?
            .into_iter()
            .find(|p| p.id == new_id)
            .ok_or_else(|| CoreError::Internal("profile".to_owned()))?;
        Self::profile_view(active)
    }

    pub fn dismiss_style_suggestion(&mut self, id: &str) -> Result<(), CoreError> {
        let mut learning = self.learning()?;
        if let Some(norm) = id.strip_prefix(PHRASE_ID) {
            learning.dismissed_phrases.push(norm.to_owned());
        } else {
            learning
                .find_mut(id)
                .ok_or_else(|| CoreError::NotFound("ההצעה".to_owned()))?
                .dismissed = true;
        }
        self.vault_mut()?.save_style_learning(&json(&learning)?)?;
        Ok(())
    }
}

/// Suggestion ids of phrases from her reports start with this.
const PHRASE_ID: &str = "phrase:";

/// What to offer her: her usual edits, then phrases of her reports not yet in the profile.
fn suggestions(
    learning: &Learning,
    local: &LocalStyle,
    active: Option<&StyleProfileView>,
) -> Vec<StyleSuggestion> {
    let mut out: Vec<StyleSuggestion> = learning
        .suggestions()
        .into_iter()
        .map(|s| StyleSuggestion {
            kind: if s.to.is_empty() {
                StyleSuggestionKind::Avoid
            } else {
                StyleSuggestionKind::Replace
            },
            id: s.id,
            from: s.from,
            to: s.to,
            count: s.count,
            confidence: s.confidence,
            section: s.section,
        })
        .collect();
    let in_profile: Vec<String> = active
        .map(|a| a.profile.items.iter().map(|i| normalize(&i.text)).collect())
        .unwrap_or_default();
    out.extend(
        local
            .phrases
            .iter()
            .filter(|p| !learning.dismissed_phrases.contains(&p.norm))
            .filter(|p| !in_profile.iter().any(|t| t.contains(&p.norm)))
            .map(|p| StyleSuggestion {
                id: format!("{PHRASE_ID}{}", p.norm),
                kind: StyleSuggestionKind::Phrase,
                from: p.text.clone(),
                to: String::new(),
                count: p.reports,
                confidence: p.share,
                section: p.section.clone(),
            }),
    );
    out
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
#[path = "style_flows.rs"]
mod flows;
