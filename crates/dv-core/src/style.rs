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
/// An edit seen this many times becomes a suggestion.
pub const SUGGEST_AT: u32 = 3;
const MAX_SOURCES: usize = 40;
/// Excerpts sent for one report (characters).
const MAX_EXCERPT_CHARS: usize = 60_000;
const MAX_PAIRS: usize = 500;
const MAX_DIFF_WORDS: usize = 400;
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

/// "Instead of «from» you write «to»" (seen `count` times in her edits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StyleSuggestion {
    pub id: String,
    pub from: String,
    pub to: String,
    pub count: u32,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Pair {
    from: String,
    to: String,
    count: u32,
    #[serde(default)]
    dismissed: bool,
    #[serde(default)]
    accepted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Learning {
    pairs: Vec<Pair>,
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

// ------------------------------------------------------------ learning from edits

/// Short replacements between Claude's paragraph and her edit: `(from, to)`, up to three words
/// on each side, no tags or numbers.
fn replacements(before: &str, after: &str) -> Vec<(String, String)> {
    let clean_word = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_owned();
    let a: Vec<String> = before
        .split_whitespace()
        .map(clean_word)
        .filter(|w| !w.is_empty())
        .collect();
    let b: Vec<String> = after
        .split_whitespace()
        .map(clean_word)
        .filter(|w| !w.is_empty())
        .collect();
    if a.len() > MAX_DIFF_WORDS || b.len() > MAX_DIFF_WORDS {
        return Vec::new();
    }
    // Longest common subsequence, then walk it to find the changed runs.
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if normalize(&a[i]) == normalize(&b[j]) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut del, mut ins): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    let mut flush = |del: &mut Vec<&str>, ins: &mut Vec<&str>| {
        let ok = |run: &[&str]| {
            (1..=3).contains(&run.len())
                && run
                    .iter()
                    .all(|w| !w.contains('[') && !w.chars().any(|c| c.is_ascii_digit()))
        };
        if ok(del) && ok(ins) && normalize(&del.join(" ")) != normalize(&ins.join(" ")) {
            out.push((del.join(" "), ins.join(" ")));
        }
        del.clear();
        ins.clear();
    };
    while i < n || j < m {
        if i < n && j < m && normalize(&a[i]) == normalize(&b[j]) {
            flush(&mut del, &mut ins);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            ins.push(&b[j]);
            j += 1;
        } else {
            del.push(&a[i]);
            i += 1;
        }
    }
    flush(&mut del, &mut ins);
    out
}

fn suggestion_id(p: &Pair) -> String {
    format!("{}→{}", normalize(&p.from), normalize(&p.to))
}

// ------------------------------------------------------------ use in drafting

/// The approved profile as it goes into a drafting request for `section_key`, with every item
/// that the case's filter would change left out. `None`: no profile, or it is switched off.
pub(crate) fn style_for_request(
    v: &Vault,
    ctx: &PrivacyContext<'_>,
    section_key: &str,
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
    let Some(text) = dv_ai::render_for_section(&stored.profile, section_key, &keep) else {
        return Ok(None);
    };
    // Learned only from her edits: her rules come on top of the default style.
    Ok(Some(if stored.profile.reports == 0 {
        format!("{}\n{text}", dv_ai::prompts::DEFAULT_STYLE)
    } else {
        text
    }))
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
        let extracted = match &self.ingest_exe {
            Some(exe) => {
                dv_ingest::worker::run(exe, file_name, bytes, dv_ingest::worker::DEFAULT_TIMEOUT)
            }
            None => dv_ingest::extract(bytes, file_name),
        }
        .map_err(|e| CoreError::Refused(e.message_he()))?;
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
        let format = match extracted.format {
            dv_ingest::Format::Docx => "docx",
            dv_ingest::Format::Odt => "odt",
            dv_ingest::Format::Pdf => "pdf",
            dv_ingest::Format::Text => "text",
        }
        .to_owned();
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
        Ok(Self::source_view(id, unix_now(), &stored))
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
        Ok(())
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
        let suggestions = learning
            .pairs
            .iter()
            .filter(|p| p.count >= SUGGEST_AT && !p.dismissed && !p.accepted)
            .map(|p| StyleSuggestion {
                id: suggestion_id(p),
                from: p.from.clone(),
                to: p.to.clone(),
                count: p.count,
            })
            .collect();
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
        let out = self.begin_send(approval_id)?;
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
        if !matches!(out.pending.kind, PendingKind::StyleAnalysis { .. }) {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        }
        let (response, demo) = match response {
            Ok(r) => r,
            Err(e) => {
                self.restore_pending(out);
                return Err(e);
            }
        };
        let PendingKind::StyleAnalysis { source_id } = out.pending.kind else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        self.vault_mut()?.record(
            AuditEvent::Send,
            None,
            &serde_json::json!({ "style": "analysis", "demo": demo }),
        )?;
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
        let out = self.begin_send(approval_id)?;
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
        if !matches!(out.pending.kind, PendingKind::StyleSynthesis { .. }) {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        }
        let (response, demo) = match response {
            Ok(r) => r,
            Err(e) => {
                self.restore_pending(out);
                return Err(e);
            }
        };
        let PendingKind::StyleSynthesis { reports } = out.pending.kind else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        self.vault_mut()?.record(
            AuditEvent::Send,
            None,
            &serde_json::json!({ "style": "profile", "demo": demo }),
        )?;
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
        Ok(())
    }

    // -------------------------------------------------------- learning from edits

    /// Remember short replacements between Claude's paragraph and her edit. Local only.
    pub(crate) fn learn_from_edit(&mut self, before: &str, after: &str) -> Result<(), CoreError> {
        let found = replacements(before, after);
        if found.is_empty() {
            return Ok(());
        }
        let mut learning = self.learning()?;
        for (from, to) in found {
            let key = format!("{}→{}", normalize(&from), normalize(&to));
            match learning.pairs.iter_mut().find(|p| suggestion_id(p) == key) {
                Some(p) => p.count = p.count.saturating_add(1),
                None => learning.pairs.push(Pair {
                    from,
                    to,
                    count: 1,
                    ..Pair::default()
                }),
            }
        }
        if learning.pairs.len() > MAX_PAIRS {
            learning.pairs.sort_by(|a, b| {
                (b.accepted || b.dismissed, b.count).cmp(&(a.accepted || a.dismissed, a.count))
            });
            learning.pairs.truncate(MAX_PAIRS);
        }
        self.vault_mut()?.save_style_learning(&json(&learning)?)?;
        Ok(())
    }

    /// Her manual edit of a paragraph Claude wrote: learn from it (never blocks the edit).
    pub(crate) fn learn_from_draft_edit(&mut self, old: Option<DraftParagraph>, new_tagged: &str) {
        if let Some(old) = old.filter(|d| d.author == Author::Ai) {
            // Learning is a convenience: an error here must not lose her edit.
            let _ = self.learn_from_edit(&old.text_tagged, new_tagged);
        }
    }

    /// Add a suggestion to the profile in use, as a new version (her click is the approval).
    pub fn accept_style_suggestion(&mut self, id: &str) -> Result<StyleProfileView, CoreError> {
        let mut learning = self.learning()?;
        let pair = learning
            .pairs
            .iter_mut()
            .find(|p| suggestion_id(p) == id)
            .ok_or_else(|| CoreError::NotFound("ההצעה".to_owned()))?;
        let text = format!("לכתוב «{}» ולא «{}»", pair.to, pair.from);
        pair.accepted = true;
        let mut stored = self.active_profile()?.unwrap_or(StoredProfile {
            profile: StyleProfile::default(),
            dropped: 0,
            demo: false,
        });
        stored.profile.items.push(StyleItem {
            id: dv_vault::crypto::random_id()?,
            section: None,
            kind: StyleKind::Rule,
            text,
            enabled: true,
            origin: StyleOrigin::Edits,
            support: 0,
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
        let pair = learning
            .pairs
            .iter_mut()
            .find(|p| suggestion_id(p) == id)
            .ok_or_else(|| CoreError::NotFound("ההצעה".to_owned()))?;
        pair.dismissed = true;
        self.vault_mut()?.save_style_learning(&json(&learning)?)?;
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
    fn overlap_finds_shared_runs_with_tags_by_role() {
        let index = ngrams("[ילד] מגיב היטב לתיווך של המבוגר בחדר", 5);
        assert!(shared_run("גם [ילד] מגיב היטב לתיווך של המבוגר", 5, &index).is_some());
        assert!(shared_run("מגיב היטב לתיווך בכל מצב", 5, &index).is_none());
    }

    #[test]
    fn edits_yield_short_replacements_only() {
        let r = replacements(
            "[ילד] מראה קושי בוויסות, ונראה כי הוא זקוק לתיווך רב.",
            "[ילד] מפגין קושי בוויסות, וניכר כי הוא זקוק לתיווך רב.",
        );
        assert_eq!(
            r,
            vec![
                ("מראה".to_owned(), "מפגין".to_owned()),
                ("ונראה".to_owned(), "וניכר".to_owned())
            ]
        );
        assert!(
            replacements("ציון 9", "ציון 11").is_empty(),
            "numbers are not style"
        );
        assert!(replacements("[ילד] הגיע", "[ילד_2] הגיע").is_empty());
        assert!(
            replacements("א ב ג ד ה ו", "ז ח ט י כ ל").is_empty(),
            "a rewritten sentence is not a replacement"
        );
    }
}

#[cfg(test)]
#[path = "style_flows.rs"]
mod flows;
