//! The de-identification pipeline (docs/PRIVACY_PIPELINE.md, layers 2–6).
//!
//! Input: text written by the psychologist (real names). Output: the same text with tags,
//! plus everything the review screen needs: what was hidden, and what is suspect.

use std::fmt;

use dv_domain::{Identity, Role, PRACTITIONER_TAG};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::lexicon::{is_name_context, PlaceKind, GENERIC_PLACE_WORDS, LEXICON, TITLES};
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

fn find_suspects(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &[Replacement],
) -> Vec<SuspectSpan> {
    let lex = &*LEXICON;
    let declared = declared_words(ctx);
    let mut out: Vec<SuspectSpan> = Vec::new();
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
        if PROFESSION_CUES.iter().any(|c| normalize(c) == tok.norm) {
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
    out
}

/// Run layers 2–6 on one piece of text.
pub fn filter(text: &str, ctx: &PrivacyContext<'_>) -> Result<FilterOutcome, PrivacyError> {
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
    Ok(assemble(text, &reps, &suspects))
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
