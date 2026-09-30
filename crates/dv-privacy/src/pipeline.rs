//! The de-identification pipeline (docs/PRIVACY_PIPELINE.md, layers 2–6).
//!
//! Input: text written by the psychologist (real names). Output: the same text with tags,
//! plus everything the review screen needs: what was hidden, and what is suspect.

use std::fmt;
use std::ops::Range;

use dv_domain::{Identity, Role, PRACTITIONER_TAG};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::lexicon::{
    is_name_context, PlaceKind, GENERIC_PLACE_WORDS, LEXICON, NAME_LABELS, NAME_STOP, PERSON_WORDS,
    ROLE_WORDS, SURNAME_ENDINGS, TITLES,
};
use crate::matcher::PhraseIndex;
use crate::patterns::{self, Ymd};
use crate::text::{normalize, prefix_splits, spelling_variants, tokenize, weak_near, Token};
use crate::PrivacyError;

/// Everything the pipeline and the gate need to know about the case being worked on.
pub struct PrivacyContext<'a> {
    pub case_id: &'a str,
    /// Identities of every case: another case's child must never slip out either.
    pub identities: &'a [Identity],
    /// The psychologist's and clinic's own names ([מאבחנת] everywhere).
    pub practitioner: &'a [String],
    /// Normalized tokens the psychologist marked "not a name" / "keep as is".
    pub allowlisted: &'a dyn Fn(&str) -> bool,
    /// Ordinary words confirmed, for this case, to be a prefix plus a declared name
    /// ("שאלון" = ש + אלון); the name inside them is hidden.
    pub confirmed_names: &'a dyn Fn(&str) -> bool,
    pub today: Ymd,
}

impl fmt::Debug for PrivacyContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivacyContext")
            .field("case_id", &self.case_id)
            .field("identities", &self.identities.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Identity {
        case_id: String,
        tag: String,
        role: Role,
    },
    Practitioner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SuspectKind {
    /// A first name that is not declared in the case.
    UnknownName,
    /// A spelling close to a declared name ("נעם" vs "נועם").
    SimilarToDeclared,
    /// A name that belongs to a different case.
    OtherCaseIdentity,
    /// A capitalized English word that may be a name.
    LatinName,
    /// Could identify indirectly (a parent's workplace, a unique detail).
    Indirect,
    /// A common word that contains a declared name after a prefix ("שאלון" = ש + אלון).
    /// Not replaced silently; the psychologist confirms once.
    AmbiguousWord,
}

/// A span that blocks sending until the psychologist decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Suspect {
    pub token: String,
    pub kind: SuspectKind,
    pub message: String,
    pub suggested_role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Mark {
    /// Original text that was replaced (left column).
    Replaced,
    /// A tag in the outgoing text (right column).
    Tag,
    /// A date turned into relative time.
    Relative,
    Suspect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Segment {
    pub text: String,
    pub mark: Option<Mark>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
pub struct Checks {
    pub declared_names: u32,
    pub patterns: u32,
    pub name_suspects: u32,
    pub indirect_suspects: u32,
}

/// Result of filtering one piece of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FilterOutcome {
    pub tagged: String,
    pub original_segments: Vec<Segment>,
    pub tagged_segments: Vec<Segment>,
    pub suspects: Vec<Suspect>,
    /// What was hidden, by kind ("שם הילד/ה", "תאריך"), for the line under each message.
    pub hidden: Vec<String>,
    pub checks: Checks,
}

#[derive(Debug, Clone)]
struct Replacement {
    start: usize,
    end: usize,
    out: String,
    label: String,
    relative: bool,
    declared: bool,
}

#[derive(Debug, Clone)]
struct SuspectSpan {
    start: usize,
    end: usize,
    suspect: Suspect,
}

fn is_generic(word: &str) -> bool {
    GENERIC_PLACE_WORDS.iter().any(|g| normalize(g) == word)
}

fn is_title(word: &str) -> bool {
    TITLES.iter().any(|t| normalize(t) == word)
}

fn is_role_word(word: &str) -> bool {
    prefix_splits(word)
        .iter()
        .any(|(_, h)| ROLE_WORDS.iter().any(|r| normalize(r) == *h))
}

/// A word for a person ("הגננת", "האח", "סבתא"), with or without "ה" and a prefix.
fn is_person_word(word: &str) -> bool {
    prefix_splits(word).iter().any(|(_, h)| {
        let bare = h.strip_prefix('ה').unwrap_or(h);
        PERSON_WORDS
            .iter()
            .any(|p| normalize(p) == *h || normalize(p) == bare)
    })
}

/// Phrases that identify one declared name: the full name, and each significant word with
/// its spelling variants ("ד\"ר אבנר שטרן" → "אבנר שטרן", "אבנר", "שטרן").
pub(crate) fn name_phrases(name: &str) -> Vec<Vec<String>> {
    let words: Vec<String> = normalize(name)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    let mut out = vec![words.clone()];
    let significant: Vec<&String> = words
        .iter()
        .filter(|w| !is_title(w) && !is_generic(w) && w.chars().count() >= 2)
        .collect();
    if significant.len() > 1 || significant.len() != words.len() {
        let joined: Vec<String> = significant.iter().map(|w| (*w).clone()).collect();
        if joined.len() > 1 {
            out.push(joined);
        }
    }
    for w in significant {
        // A defective spelling that is an ordinary word ("מכל" from "מיכל") is not a variant.
        for v in spelling_variants(w)
            .into_iter()
            .filter(|v| v == w || !LEXICON.common_words.contains(v))
        {
            out.push(vec![v]);
        }
    }
    out
}

/// Index of every declared name. Current-case identities are inserted first, so a name that
/// exists in two cases maps to this case's tag.
pub(crate) fn identity_index(ctx: &PrivacyContext<'_>) -> PhraseIndex<Target> {
    let mut idx = PhraseIndex::default();
    let mut ordered: Vec<&Identity> = ctx
        .identities
        .iter()
        .filter(|i| i.case_id == ctx.case_id)
        .collect();
    ordered.extend(ctx.identities.iter().filter(|i| i.case_id != ctx.case_id));
    let insert_identity = |idx: &mut PhraseIndex<Target>, identity: &Identity| {
        let target = Target::Identity {
            case_id: identity.case_id.clone(),
            tag: identity.tag.clone(),
            role: identity.role,
        };
        for name in std::iter::once(&identity.value).chain(identity.aliases.iter()) {
            for phrase in name_phrases(name) {
                idx.insert_words(phrase, target.clone());
            }
        }
    };
    for identity in ordered.iter().filter(|i| i.case_id == ctx.case_id) {
        insert_identity(&mut idx, identity);
    }
    for name in ctx.practitioner {
        for phrase in name_phrases(name) {
            idx.insert_words(phrase, Target::Practitioner);
        }
    }
    for identity in ordered.iter().filter(|i| i.case_id != ctx.case_id) {
        insert_identity(&mut idx, identity);
    }
    idx
}

fn overlaps(start: usize, end: usize, reps: &[Replacement], sus: &[SuspectSpan]) -> bool {
    reps.iter().any(|r| start < r.end && r.start < end)
        || sus.iter().any(|s| start < s.end && s.start < end)
}

fn role_label(role: Role) -> String {
    format!("שם {}", role.label_he())
}

/// Declared single words of the current case, for near-miss spelling detection.
fn declared_words(ctx: &PrivacyContext<'_>) -> Vec<(String, String)> {
    ctx.identities
        .iter()
        .filter(|i| i.case_id == ctx.case_id)
        .flat_map(|i| {
            std::iter::once(&i.value)
                .chain(i.aliases.iter())
                .flat_map(move |name| {
                    normalize(name)
                        .split(' ')
                        .filter(|w| w.chars().count() >= 3 && !is_title(w) && !is_generic(w))
                        .map(|w| (w.to_owned(), i.tag.clone()))
                        .collect::<Vec<_>>()
                })
        })
        .collect()
}

const PROFESSION_CUES: &[&str] = &[
    "עובד",
    "עובדת",
    "עובדים",
    "מועסק",
    "מועסקת",
    "במקצועו",
    "במקצועה",
    "מנהל",
    "מנהלת",
];

/// "עובדת" is a job only when a job or a workplace follows: "עובדת כמורה", "עובד בבנק",
/// "עובדת בתור…", "עובד אצל…". "עובדת איתו על המעברים", "מנהלת הגן" are ordinary words
/// in a report and are not asked about.
fn is_profession_context(cue: &str, next: Option<&str>) -> bool {
    let work = ["עובד", "עובדת", "עובדים", "מועסק", "מועסקת"]
        .iter()
        .any(|w| normalize(w) == cue);
    let manager = ["מנהל", "מנהלת"].iter().any(|w| normalize(w) == cue);
    let Some(next) = next else {
        return !work && !manager;
    };
    if work {
        return next.starts_with('כ')
            || (next.starts_with('ב')
                && !["בגן", "בקבוצה", "בבית", "בכיתה", "בשיתוף", "בטיפול"]
                    .iter()
                    .any(|w| normalize(w) == next))
            || ["בתור", "אצל"].iter().any(|w| normalize(w) == next);
    }
    if manager {
        return ![
            "הגן",
            "בית",
            "המסגרת",
            "הצהרון",
            "המעון",
            "הכיתה",
            "את",
            "שיחה",
        ]
        .iter()
        .any(|w| normalize(w) == next);
    }
    true
}

/// Names no lexicon can know: whatever follows a name label ("שם הילד: …"), the family name
/// after "משפחת", and a surname right after a first name or a declared name ("נועם ברקוביץ",
/// "דנה כהן-לוי"). Declared names are already replaced; what is left becomes a suspect.
fn name_run_suspects(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &[Replacement],
    out: &mut Vec<SuspectSpan>,
) {
    let lex = &*LEXICON;
    let hebrew = |t: &Token| {
        t.norm
            .chars()
            .any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c))
    };
    let digits = |t: &Token| t.norm.chars().any(|c| c.is_ascii_digit());
    let gap = |a: usize, b: usize| &text[tokens[a].end..tokens[b].start];
    let breaks = |g: &str| {
        g.chars().any(|c| {
            matches!(
                c,
                ',' | '.' | ';' | '(' | ')' | '\n' | '|' | '\t' | '·' | '–' | '—' | ':'
            )
        })
    };
    let joins =
        |g: &str| !g.is_empty() && g.chars().all(|c| c == ' ' || c == '-' || c == '\u{05BE}');
    let stop = |n: &str| NAME_STOP.iter().any(|s| normalize(s) == n);
    let push = |start: usize, end: usize, message: &str, out: &mut Vec<SuspectSpan>| {
        let token = &text[start..end];
        if !overlaps(start, end, reps, out) && !(ctx.allowlisted)(&normalize(token)) {
            out.push(SuspectSpan {
                start,
                end,
                suspect: Suspect {
                    token: token.to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: message.to_owned(),
                    suggested_role: Role::Other,
                },
            });
        }
    };

    // 1. After a label: up to three words, until punctuation or a word like "גיל".
    let labels: Vec<(Vec<String>, &str)> = NAME_LABELS
        .iter()
        .map(|l| {
            (
                normalize(l)
                    .split(' ')
                    .filter(|w| !w.is_empty())
                    .map(str::to_owned)
                    .collect(),
                *l,
            )
        })
        .collect();
    for i in 0..tokens.len() {
        for (words, label) in &labels {
            let n = words.len();
            if n == 0
                || i + n >= tokens.len()
                || !tokens[i..i + n]
                    .iter()
                    .zip(words)
                    .all(|(t, w)| &t.norm == w)
            {
                continue;
            }
            let last = i + n - 1;
            let g = gap(last, last + 1);
            let direct = matches!(*label, "לכבוד" | "בברכה" | "חתימה");
            let label_ends = if direct {
                true
            } else {
                g.contains(':') && !g.contains('\n')
            };
            if !label_ends {
                continue;
            }
            let message = format!("שם אחרי \"{label}\"");
            let mut j = last + 1;
            let mut taken = 0;
            while j < tokens.len() && taken < 3 {
                let t = &tokens[j];
                if stop(&t.norm) || !hebrew(t) || digits(t) {
                    break;
                }
                let role = prefix_splits(&t.norm)
                    .iter()
                    .any(|(_, h)| ROLE_WORDS.iter().any(|r| normalize(r) == *h));
                if !is_title(&t.norm) && !role && t.norm != normalize("משפחת") {
                    push(t.start, t.end, &message, out);
                    taken += 1;
                }
                if j + 1 >= tokens.len() || breaks(gap(j, j + 1)) {
                    break;
                }
                j += 1;
            }
        }
    }

    // 2. "משפחת כהן", "למשפחת כהן-לוי".
    let family = normalize("משפחת");
    for i in 0..tokens.len().saturating_sub(1) {
        if !prefix_splits(&tokens[i].norm)
            .iter()
            .any(|(_, h)| *h == family)
        {
            continue;
        }
        let mut j = i + 1;
        while j < tokens.len() && hebrew(&tokens[j]) && !digits(&tokens[j]) {
            push(tokens[j].start, tokens[j].end, "שם משפחה", out);
            if j + 1 >= tokens.len() || !gap(j, j + 1).contains(['-', '\u{05BE}']) {
                break;
            }
            j += 1;
        }
    }

    // 3. A name after a title: "ד\"ר קורנבליט", "הרב אלמוזנינו", "גב' ורשבסקי". Up to two words,
    // until punctuation; a declared name there is already replaced.
    for i in 0..tokens.len().saturating_sub(1) {
        if !is_title(&tokens[i].norm) || breaks(gap(i, i + 1)) {
            continue;
        }
        let mut j = i + 1;
        while j < tokens.len() && j <= i + 2 {
            let t = &tokens[j];
            if stop(&t.norm)
                || !hebrew(t)
                || digits(t)
                || is_role_word(&t.norm)
                || lex.common_words.contains(&t.norm)
            {
                break;
            }
            push(t.start, t.end, "שם אחרי תואר", out);
            // A second word only after a first name ("ד\"ר אבנר שטרן", not "ד\"ר כהן המליץ").
            if j + 1 >= tokens.len() || !joins(gap(j, j + 1)) || !lex.first_names.contains(&t.norm)
            {
                break;
            }
            j += 1;
        }
    }

    // 4. A name set off after a person word: "הגננת הקודמת, ברכה, תיארה…",
    // "האח הגדול – יהונתן – מתגייס", "הקלינאית (אינה) המליצה". Between the marks: one or
    // two words, which in a report can only be a name.
    let opens = |g: &str| g.contains([',', '–', '—', '(']) && !g.contains(['.', '\n', ':']);
    let closes = |g: &str| g.contains([',', '–', '—', ')', '.']);
    for i in 0..tokens.len() {
        if !is_person_word(&tokens[i].norm) {
            continue;
        }
        // One adjective may follow the person word ("הקודמת", "הגדול").
        let mut k = i;
        if k + 1 < tokens.len() && tokens[k + 1].norm.starts_with('ה') && joins(gap(k, k + 1)) {
            k += 1;
        }
        if k + 1 >= tokens.len() || !opens(gap(k, k + 1)) {
            continue;
        }
        let first = k + 1;
        let mut last = first;
        if last + 1 < tokens.len() && joins(gap(last, last + 1)) {
            last += 1;
        }
        if last + 1 >= tokens.len() || !closes(gap(last, last + 1)) {
            if first + 1 < tokens.len() && closes(gap(first, first + 1)) {
                last = first;
            } else {
                continue;
            }
        }
        let words = &tokens[first..=last];
        let known = |t: &Token| {
            prefix_splits(&t.norm).iter().any(|(p, h)| {
                *p == 0
                    && (lex.first_names.contains(h)
                        || lex.word_names.contains(h)
                        || lex.surnames.contains(h))
            })
        };
        // An unknown word counts only alone, and only if it does not read as an ordinary word
        // with a prefix ("בשיחה", "כמו", "לדבריה") or an adverb.
        let plausible = |t: &Token| {
            known(t)
                || (words.len() == 1
                    && !t.norm.starts_with(['ב', 'כ', 'ל', 'מ', 'ו', 'ש', 'ה'])
                    && !APPOSITIVE_STOP.iter().any(|w| normalize(w) == t.norm))
        };
        if words
            .iter()
            .all(|t| hebrew(t) && !digits(t) && !stop(&t.norm) && !is_person_word(&t.norm))
            && !words.iter().all(|t| lex.common_words.contains(&t.norm))
            && words
                .iter()
                .all(|t| plausible(t) || (words.len() == 2 && words.iter().any(&known)))
        {
            for t in words {
                push(t.start, t.end, "שם שמופיע ליד תפקיד", out);
            }
        }
    }

    // 5. A surname right after a first name or a declared name.
    let is_name = |i: usize| {
        let t = &tokens[i];
        reps.iter()
            .any(|r| r.declared && r.start <= t.start && t.end <= r.end)
            || prefix_splits(&t.norm)
                .iter()
                .any(|(_, h)| lex.first_names.contains(h))
    };
    let mut surname_at: Option<usize> = None;
    for (i, t) in tokens.iter().enumerate().skip(1) {
        let g = gap(i - 1, i);
        if !joins(g) || !hebrew(t) || digits(t) {
            continue;
        }
        let hyphen_after_surname = surname_at == Some(i - 1) && g.contains(['-', '\u{05BE}']);
        let raw = &text[t.start..t.end];
        let looks_like_surname = lex.surnames.contains(&t.norm)
            || (t.norm.chars().count() >= 4
                && SURNAME_ENDINGS
                    .iter()
                    .any(|e| t.norm.ends_with(&normalize(e))))
            // "אברג'יל", "חאג'": a geresh inside a Hebrew word is a borrowed sound, as in names.
            || (raw.chars().count() >= 4 && raw.contains(['\'', '׳']) && !raw.ends_with(['\'', '׳']));
        if hyphen_after_surname
            || (is_name(i - 1) && looks_like_surname && !lex.common_words.contains(&t.norm))
        {
            push(t.start, t.end, "נראה כמו שם משפחה", out);
            surname_at = Some(i);
        }
    }
}

fn find_suspects(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &[Replacement],
) -> Vec<SuspectSpan> {
    let lex = &*LEXICON;
    let declared = declared_words(ctx);
    let mut out: Vec<SuspectSpan> = Vec::new();
    name_run_suspects(text, tokens, ctx, reps, &mut out);
    for (i, tok) in tokens.iter().enumerate() {
        if overlaps(tok.start, tok.end, reps, &out)
            || (ctx.allowlisted)(&tok.norm)
            || lex.common_words.contains(&tok.norm)
        {
            continue;
        }
        let raw = &text[tok.start..tok.end];
        let splits = prefix_splits(&tok.norm);
        let prev = i
            .checked_sub(1)
            .and_then(|j| tokens.get(j))
            .map(|t| t.norm.as_str());
        let next = tokens.get(i + 1).map(|t| t.norm.as_str());
        let span_from = |prefix: usize| tok.start + crate::text::byte_len_of_letters(raw, prefix);

        let plain = splits
            .iter()
            .find(|(_, h)| lex.first_names.contains(h) && !(ctx.allowlisted)(h));
        let word = splits
            .iter()
            .find(|(_, h)| lex.word_names.contains(h) && !(ctx.allowlisted)(h));
        if let Some((p, _)) = plain.or_else(|| word.filter(|_| is_name_context(prev, next))) {
            let start = span_from(*p);
            out.push(SuspectSpan {
                start,
                end: tok.end,
                suspect: Suspect {
                    token: text[start..tok.end].to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: "שם שלא מופיע ברשימת התיק".to_owned(),
                    suggested_role: Role::OtherChild,
                },
            });
            continue;
        }
        let is_capitalized_latin = raw.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && raw.chars().skip(1).all(|c| c.is_ascii_lowercase())
            && raw.len() > 1;
        if is_capitalized_latin && !lex.latin_allow.contains(&tok.norm) {
            out.push(SuspectSpan {
                start: tok.start,
                end: tok.end,
                suspect: Suspect {
                    token: raw.to_owned(),
                    kind: SuspectKind::LatinName,
                    message: "מילה באנגלית שעשויה להיות שם".to_owned(),
                    suggested_role: Role::Other,
                },
            });
            continue;
        }
        let near = splits.iter().find_map(|(p, h)| {
            if h.chars().count() < 4 {
                return None;
            }
            declared
                .iter()
                .find(|(w, _)| weak_near(h, w))
                .map(|(_, tag)| (*p, tag.clone()))
        });
        if let Some((p, tag)) = near {
            let start = span_from(p);
            out.push(SuspectSpan {
                start,
                end: tok.end,
                suspect: Suspect {
                    token: text[start..tok.end].to_owned(),
                    kind: SuspectKind::SimilarToDeclared,
                    message: format!("כתיב קרוב לשם שבתיק ({tag})"),
                    suggested_role: Role::Other,
                },
            });
            continue;
        }
        if PROFESSION_CUES.iter().any(|c| normalize(c) == tok.norm)
            && is_profession_context(&tok.norm, next)
        {
            // Flag the cue and the next two words ("עובדת כמנהלת חשבונות").
            let end = tokens
                .get(i + 2)
                .or_else(|| tokens.get(i + 1))
                .map_or(tok.end, |t| t.end);
            if !overlaps(tok.start, end, reps, &out) {
                out.push(SuspectSpan {
                    start: tok.start,
                    end,
                    suspect: Suspect {
                        token: text[tok.start..end].to_owned(),
                        kind: SuspectKind::Indirect,
                        message: "מקצוע או מקום עבודה – כדאי להכליל (\"עובדת בתחום החינוך\")"
                            .to_owned(),
                        suggested_role: Role::Other,
                    },
                });
            }
        }
    }
    // An answer "not a name / keep as is" holds for every kind of question, whatever part
    // of the word was flagged: a question that comes back after it was answered cannot be
    // answered at all.
    out.retain(|s| {
        let whole = tokens
            .iter()
            .filter(|t| t.start < s.end && s.start < t.end)
            .map(|t| t.norm.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        !(ctx.allowlisted)(&normalize(&s.suspect.token)) && !(ctx.allowlisted)(&whole)
    });
    out
}

/// Run layers 2–6 on one piece of text.
pub fn filter(text: &str, ctx: &PrivacyContext<'_>) -> Result<FilterOutcome, PrivacyError> {
    let text = &plain_brackets(text, ctx);
    let (reps, suspects) = analyze(text, ctx)?;
    Ok(assemble(text, &reps, &suspects))
}

/// Square brackets around words that are not a tag ("[מחנכת]" written by the model, "[...]"
/// in a document) become round ones, so the words inside are filtered like any other text and
/// never pass for a tag. A real tag stays: this case's (already filtered), a generic one, or
/// another case's, which the gate then refuses. Same length, so every offset still holds.
fn plain_brackets(text: &str, ctx: &PrivacyContext<'_>) -> String {
    static BRACKETED: std::sync::LazyLock<Option<regex::Regex>> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\[[^\[\]\n]{1,40}\]").ok());
    let Some(re) = BRACKETED.as_ref() else {
        return text.to_owned();
    };
    let known = |t: &str| {
        crate::gate::GENERIC_TAGS.contains(&t)
            || t.starts_with("[חסר")
            || ctx.identities.iter().any(|i| i.tag == t)
    };
    let mut out = text.to_owned();
    for m in re.find_iter(text) {
        if !known(m.as_str()) {
            out.replace_range(m.start()..=m.start(), "(");
            out.replace_range(m.end() - 1..m.end(), ")");
        }
    }
    out
}

/// Filter a whole text once, so every rule sees the full context, and also return the outcome
/// of each range on its own (passages of a material, D-022).
///
/// Each part is exactly the whole outcome cut at the range: the same replacements, the same
/// suspects. When a replacement or a suspect crosses the edge of a range, the parts are `None`
/// and the caller must send the whole text.
pub fn filter_split(
    text: &str,
    ctx: &PrivacyContext<'_>,
    ranges: &[Range<usize>],
) -> Result<(FilterOutcome, Option<Vec<FilterOutcome>>), PrivacyError> {
    let text = &plain_brackets(text, ctx);
    let (reps, suspects) = analyze(text, ctx)?;
    let whole = assemble(text, &reps, &suspects);
    let mut parts = Vec::with_capacity(ranges.len());
    for r in ranges {
        let Some(piece) = text.get(r.clone()) else {
            return Ok((whole, None));
        };
        let crosses = |start: usize, end: usize| {
            start < r.end && end > r.start && (start < r.start || end > r.end)
        };
        if reps.iter().any(|x| crosses(x.start, x.end))
            || suspects.iter().any(|x| crosses(x.start, x.end))
        {
            return Ok((whole, None));
        }
        let inside = |start: usize, end: usize| start >= r.start && end <= r.end;
        let part_reps: Vec<Replacement> = reps
            .iter()
            .filter(|x| inside(x.start, x.end))
            .map(|x| Replacement {
                start: x.start - r.start,
                end: x.end - r.start,
                ..x.clone()
            })
            .collect();
        let part_suspects: Vec<SuspectSpan> = suspects
            .iter()
            .filter(|x| inside(x.start, x.end))
            .map(|x| SuspectSpan {
                start: x.start - r.start,
                end: x.end - r.start,
                suspect: x.suspect.clone(),
            })
            .collect();
        parts.push(assemble(piece, &part_reps, &part_suspects));
    }
    Ok((whole, Some(parts)))
}

impl FilterOutcome {
    /// Several outcomes shown and sent as one text, `separator` between each two (plain text,
    /// the same in both columns).
    #[must_use]
    pub fn join(parts: &[&FilterOutcome], separators: &[&str]) -> FilterOutcome {
        let mut out = FilterOutcome {
            tagged: String::new(),
            original_segments: Vec::new(),
            tagged_segments: Vec::new(),
            suspects: Vec::new(),
            hidden: Vec::new(),
            checks: Checks::default(),
        };
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                let sep = separators.get(i - 1).copied().unwrap_or("\n\n");
                let seg = Segment {
                    text: sep.to_owned(),
                    mark: None,
                    label: None,
                };
                out.original_segments.push(seg.clone());
                out.tagged_segments.push(seg);
                out.tagged.push_str(sep);
            }
            out.tagged.push_str(&p.tagged);
            out.original_segments
                .extend(p.original_segments.iter().cloned());
            out.tagged_segments
                .extend(p.tagged_segments.iter().cloned());
            for s in &p.suspects {
                if !out.suspects.contains(s) {
                    out.suspects.push(s.clone());
                }
            }
            for h in &p.hidden {
                if !out.hidden.contains(h) {
                    out.hidden.push(h.clone());
                }
            }
            out.checks.declared_names += p.checks.declared_names;
            out.checks.patterns += p.checks.patterns;
            out.checks.name_suspects += p.checks.name_suspects;
            out.checks.indirect_suspects += p.checks.indirect_suspects;
        }
        out
    }
}

/// Adverbs and connectives that are set off by commas like a name ("האם, אגב, …").
const APPOSITIVE_STOP: &[&str] = &[
    "אגב",
    "אולי",
    "עדיין",
    "גם",
    "אמנם",
    "שוב",
    "היום",
    "אז",
    "אך",
    "אבל",
    "או",
    "עכשיו",
    "תמיד",
    "אף",
    "רק",
    "יחד",
    "ככה",
    "זאת",
    "אחר",
    "אחרת",
    "אחרים",
    "ואז",
    "כנראה",
    "אחת",
    "אחד",
    "שניהם",
    "שתיהן",
    "כולם",
    "עצמה",
    "עצמו",
    "ייתכן",
    "אפילו",
    "אלא",
];

/// Words after "גן" / "בית הספר" that describe it rather than name it.
const INSTITUTION_STOP: &[&str] = &[
    "הזה",
    "הזו",
    "הזאת",
    "הקודם",
    "הקודמת",
    "החדש",
    "החדשה",
    "הישן",
    "הישנה",
    "הרגיל",
    "הרגילה",
    "היסודי",
    "העירוני",
    "העירונית",
    "הממלכתי",
    "הממלכתית",
    "הדתי",
    "הדתית",
    "הפרטי",
    "הפרטית",
    "המיוחד",
    "המיוחדת",
    "הילדים",
    "השכונתי",
    "השכונתית",
    "הקרוב",
    "הקרובה",
    "הוא",
    "היא",
    "הם",
    "הן",
    "היה",
    "הייתה",
    "היו",
    "הכללי",
    "הגדול",
    "הקטן",
    "התורני",
    "המשותף",
    "השני",
    "הראשון",
    "השנה",
    "הבוקר",
    "הצהריים",
    "הספר",
    "הזמן",
    "הערב",
    "הבא",
    "הבאה",
    "הנוכחי",
    "הנוכחית",
    "המקורי",
    "האחרון",
    "האחרונה",
    "שלו",
    "שלה",
    "שלהם",
    "הטיפולי",
    "הטיפולית",
    "השפתי",
    "התקשורתי",
    "הממ\"ד",
    "הקהילתי",
    "החיות",
    "השעשועים",
    "המשחקים",
    "הציבורי",
    "הלאומי",
    "הבוטני",
    "המדע",
    "העיר",
    "השכונה",
    "הקיץ",
    "החורף",
    "הירוק",
    "הגדולה",
    "הקטנה",
    "המקומי",
    "המקומית",
    "האזורי",
    "האזורית",
];

/// "גן השקד", "בגן \"הרימונים\"", "בית ספר \"אופקים חדשים\"", "בית הספר ע\"ש רבין": the name
/// after a word for a kindergarten or a school, as a replacement that leaves it out.
fn institution_names(text: &str, tokens: &[Token]) -> Vec<Replacement> {
    let lex = &*LEXICON;
    let hebrew = |t: &Token| {
        t.norm
            .chars()
            .any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c))
    };
    let is = |t: &Token, words: &[&str]| {
        prefix_splits(&t.norm)
            .iter()
            .any(|(_, h)| words.iter().any(|w| normalize(w) == *h))
    };
    let gap = |a: usize, b: usize| &text[tokens[a].end..tokens[b].start];
    let quote = |g: &str| g.contains(['"', '״', '“', '”', '„']);
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // The institution word(s): "גן", "מעון", "בית ספר", "בית הספר", "בי\"ס", "תלמוד תורה".
        let end_word = if is(
            &tokens[i],
            &[
                "גן",
                "גנון",
                "מעון",
                "צהרון",
                "משפחתון",
                "בי\"ס",
                "ביה\"ס",
                "ת\"ת",
            ],
        ) {
            Some(i)
        } else if is(&tokens[i], &["בית", "תלמוד"])
            && tokens.get(i + 1).is_some_and(|t| {
                ["ספר", "הספר", "תורה"]
                    .iter()
                    .any(|w| normalize(w) == t.norm)
            })
        {
            Some(i + 1)
        } else {
            None
        };
        let Some(k) = end_word else {
            i += 1;
            continue;
        };
        let mut first = k + 1;
        if first >= tokens.len() || gap(k, first).contains(['.', ',', '\n', ':', ';', '(']) {
            i = k + 1;
            continue;
        }
        // "ע\"ש רבין": whatever follows "named after" is a name.
        let named_after =
            tokens[first].norm == normalize("ע\"ש") || tokens[first].norm == normalize("עש");
        if named_after {
            first += 1;
        }
        let opened = first < tokens.len() && quote(gap(first - 1, first));
        let mut last = None;
        if opened {
            // Up to the closing quote, at most four words.
            for j in first..tokens.len().min(first + 4) {
                if !hebrew(&tokens[j]) {
                    break;
                }
                let after = if j + 1 < tokens.len() {
                    gap(j, j + 1)
                } else {
                    &text[tokens[j].end..]
                };
                if quote(after) {
                    last = Some(j);
                    break;
                }
                if after.contains(['.', ',', '\n']) {
                    break;
                }
            }
        } else if first < tokens.len() {
            let t = &tokens[first];
            let stop = INSTITUTION_STOP.iter().any(|w| normalize(w) == t.norm);
            let definite_name = t.norm.starts_with('ה') && t.norm.chars().count() >= 4;
            let known = lex.first_names.contains(&t.norm)
                || lex.surnames.contains(&t.norm)
                || !lex.places.find(&t.norm, &tokenize(&t.norm)).is_empty();
            if hebrew(t)
                && !stop
                && !lex.common_words.contains(&t.norm)
                && (named_after || definite_name || known)
            {
                last = Some(first);
                // "גן הדקל הירוק"? One more word only after "ע\"ש" ("ע\"ש יצחק רבין").
                if named_after
                    && first + 1 < tokens.len()
                    && gap(first, first + 1) == " "
                    && hebrew(&tokens[first + 1])
                    && lex.surnames.contains(&tokens[first + 1].norm)
                {
                    last = Some(first + 1);
                }
            }
        }
        if let Some(last) = last {
            let mut end = tokens[last].end;
            if opened {
                // Take the closing quote too.
                if let Some(c) = text[end..]
                    .chars()
                    .next()
                    .filter(|c| ['"', '״', '“', '”'].contains(c))
                {
                    end += c.len_utf8();
                }
            }
            out.push(Replacement {
                start: tokens[k].end,
                end,
                out: String::new(),
                label: "שם גן / בית ספר".to_owned(),
                relative: true,
                declared: false,
            });
            i = last + 1;
        } else {
            i = k + 1;
        }
    }
    out
}

/// Layers 2–6: what to replace and what to ask about, as spans of `text`, sorted.
fn analyze(
    text: &str,
    ctx: &PrivacyContext<'_>,
) -> Result<(Vec<Replacement>, Vec<SuspectSpan>), PrivacyError> {
    let tokens = tokenize(text);
    let mut reps: Vec<Replacement> = Vec::new();
    let mut suspects: Vec<SuspectSpan> = Vec::new();

    // Layer 2: declared identities (every case; another case's name is a suspect, not a tag).
    for m in identity_index(ctx).find(text, &tokens) {
        let whole = &tokens[m.first_token];
        if m.prefix > 0
            && LEXICON.common_words.contains(&whole.norm)
            && !(ctx.confirmed_names)(&whole.norm)
        {
            // "שאלון" = ש + אלון: never rewrite an ordinary word silently.
            if !(ctx.allowlisted)(&whole.norm) {
                suspects.push(SuspectSpan {
                    start: whole.start,
                    end: whole.end,
                    suspect: Suspect {
                        token: text[whole.start..whole.end].to_owned(),
                        kind: SuspectKind::AmbiguousWord,
                        message: "מילה רגילה שמכילה שם מהתיק – להסתיר או להשאיר?".to_owned(),
                        suggested_role: Role::Other,
                    },
                });
            }
            continue;
        }
        match m.payload {
            Target::Identity { case_id, tag, role } if case_id == ctx.case_id => {
                reps.push(Replacement {
                    start: m.start,
                    end: m.end,
                    out: tag,
                    label: role_label(role),
                    relative: false,
                    declared: true,
                })
            }
            Target::Identity { .. } => suspects.push(SuspectSpan {
                start: m.start,
                end: m.end,
                suspect: Suspect {
                    token: text[m.start..m.end].to_owned(),
                    kind: SuspectKind::OtherCaseIdentity,
                    message: "שם שמופיע בתיק אחר".to_owned(),
                    suggested_role: Role::OtherChild,
                },
            }),
            Target::Practitioner => reps.push(Replacement {
                start: m.start,
                end: m.end,
                out: PRACTITIONER_TAG.to_owned(),
                label: "שם המאבחנת".to_owned(),
                relative: false,
                declared: true,
            }),
        }
    }
    // Layer 4: patterns.
    for hit in patterns::find(text, ctx.today).map_err(|e| PrivacyError::Internal(e.to_string()))? {
        if !overlaps(hit.start, hit.end, &reps, &suspects) {
            reps.push(Replacement {
                start: hit.start,
                end: hit.end,
                relative: !hit.replacement.starts_with('['),
                out: hit.replacement,
                label: hit.kind.label_he().to_owned(),
                declared: false,
            });
        }
    }
    // Names of kindergartens and schools: only the name is left out ("בגן השקד שבשכונה" →
    // "בגן שבשכונה"), so the sentence still reads and nothing needs to come back.
    for r in institution_names(text, &tokens) {
        if !overlaps(r.start, r.end, &reps, &suspects) {
            reps.push(r);
        }
    }
    // Places from the lexicon (localities, hospitals) are hidden automatically.
    for m in LEXICON.places.find(text, &tokens) {
        if !overlaps(m.start, m.end, &reps, &suspects) {
            let (out, label) = match m.payload {
                PlaceKind::Locality => ("[יישוב_אחר]", "יישוב"),
                PlaceKind::Hospital => ("[בית_חולים]", "בית חולים"),
            };
            reps.push(Replacement {
                start: m.start,
                end: m.end,
                out: out.to_owned(),
                label: label.to_owned(),
                relative: false,
                declared: false,
            });
        }
    }
    // Layers 3, 5, 6: suspects on what is left.
    suspects.extend(find_suspects(text, &tokens, ctx, &reps));

    reps.sort_by_key(|r| r.start);
    suspects.sort_by_key(|s| s.start);
    Ok((reps, suspects))
}

fn assemble(text: &str, reps: &[Replacement], suspects: &[SuspectSpan]) -> FilterOutcome {
    enum Ev<'a> {
        Rep(&'a Replacement),
        Sus(&'a SuspectSpan),
    }
    let mut events: Vec<(usize, usize, Ev<'_>)> =
        reps.iter().map(|r| (r.start, r.end, Ev::Rep(r))).collect();
    events.extend(suspects.iter().map(|s| (s.start, s.end, Ev::Sus(s))));
    events.sort_by_key(|e| e.0);

    let seg = |t: &str, mark: Option<Mark>, label: Option<String>| Segment {
        text: t.to_owned(),
        mark,
        label,
    };
    let (mut original, mut tagged_segments, mut tagged) =
        (Vec::new(), Vec::new(), String::with_capacity(text.len()));
    let mut pos = 0;
    for (start, end, ev) in &events {
        if *start < pos {
            continue;
        }
        let plain = &text[pos..*start];
        if !plain.is_empty() {
            original.push(seg(plain, None, None));
            tagged_segments.push(seg(plain, None, None));
            tagged.push_str(plain);
        }
        let piece = &text[*start..*end];
        match ev {
            Ev::Rep(r) => {
                original.push(seg(piece, Some(Mark::Replaced), Some(r.label.clone())));
                let mark = if r.relative {
                    Mark::Relative
                } else {
                    Mark::Tag
                };
                tagged_segments.push(seg(&r.out, Some(mark), Some(r.label.clone())));
                tagged.push_str(&r.out);
            }
            Ev::Sus(s) => {
                original.push(seg(
                    piece,
                    Some(Mark::Suspect),
                    Some(s.suspect.message.clone()),
                ));
                tagged_segments.push(seg(
                    piece,
                    Some(Mark::Suspect),
                    Some(s.suspect.message.clone()),
                ));
                tagged.push_str(piece);
            }
        }
        pos = *end;
    }
    if pos < text.len() {
        original.push(seg(&text[pos..], None, None));
        tagged_segments.push(seg(&text[pos..], None, None));
        tagged.push_str(&text[pos..]);
    }

    let mut hidden: Vec<String> = Vec::new();
    for r in reps {
        if !hidden.contains(&r.label) {
            hidden.push(r.label.clone());
        }
    }
    let count = |f: &dyn Fn(&Replacement) -> bool| {
        u32::try_from(reps.iter().filter(|r| f(r)).count()).unwrap_or(u32::MAX)
    };
    let checks = Checks {
        declared_names: count(&|r| r.declared),
        patterns: count(&|r| !r.declared),
        name_suspects: u32::try_from(
            suspects
                .iter()
                .filter(|s| s.suspect.kind != SuspectKind::Indirect)
                .count(),
        )
        .unwrap_or(u32::MAX),
        indirect_suspects: u32::try_from(
            suspects
                .iter()
                .filter(|s| s.suspect.kind == SuspectKind::Indirect)
                .count(),
        )
        .unwrap_or(u32::MAX),
    };
    FilterOutcome {
        tagged,
        original_segments: original,
        tagged_segments,
        suspects: suspects.iter().map(|s| s.suspect.clone()).collect(),
        hidden,
        checks,
    }
}
