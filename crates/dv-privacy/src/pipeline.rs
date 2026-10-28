//! The de-identification pipeline (docs/PRIVACY_PIPELINE.md, layers 2–6).
//!
//! Input: text written by the psychologist (real names). Output: the same text with tags,
//! plus everything the review screen needs: what was hidden, and what is suspect.

use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use dv_domain::{assign_tag, Identity, IdentitySource, Role, PRACTITIONER_TAG};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::lexicon::{
    is_name_context, word_bucket, PlaceKind, GENERIC_PLACE_WORDS, LEXICON, NAME_LABELS, NAME_STOP,
    PERSON_WORDS, ROLE_WORDS, SURNAME_ENDINGS, TITLES,
};
use crate::matcher::PhraseIndex;
use crate::patterns::{self, Ymd};
use crate::text::{
    norm_eq, norm_set, normalize, prefix_splits, spelling_variants, tokenize, weak_near, Token,
    PREFIX_LETTERS,
};
use crate::PrivacyError;

mod context;

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
    /// Names found in the psychologist's past reports ("הדוחות שלי"), known only by keyed
    /// hash of the normalized word: hidden wherever they appear, and refused by the gate.
    pub past_names: &'a dyn Fn(&str) -> bool,
    pub today: Ymd,
    /// The case's mask book: numbers, dates, addresses and places get numbered tags that come
    /// back in the answer. `None`: the fixed tag of each kind (`[תאריך]`), which stays.
    pub masks: Option<&'a crate::MaskBook>,
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

/// What the filter hid on its own, as the summary card lists it (D-042, stage 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AutoKind {
    /// A name the case did not list. It is kept as an identity of the case from then on.
    Name,
    /// A name of the case inside an ordinary word ("שאלון" when the child is "אלון").
    DeclaredInWord,
    /// A spelling close to a name of the case ("נואם" for "נועם"), hidden as that name.
    SimilarSpelling,
    /// A name that belongs to another case.
    OtherCase,
    /// A job or a workplace, generalized ("עובדת בתחום החינוך").
    Indirect,
}

/// One thing the filter hid without asking. "להחזיר" on the card keeps it as it was written
/// from then on, for this case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AutoHidden {
    /// The words as written (the whole word for a name inside a word).
    pub token: String,
    /// What goes out instead: a tag ("[גננת_2]") or a generalization.
    pub tag: String,
    pub role: Role,
    /// Why, in a few Hebrew words ("אחרי 'הגננת'").
    pub reason: String,
    /// Hidden on a weaker sign (a name that is also a word, a close spelling, English).
    pub uncertain: bool,
    pub kind: AutoKind,
}

/// A question about a word (the model's answer scan, and the earlier review screen).
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
    /// What the filter hid on its own; nothing is left to ask.
    pub auto_hidden: Vec<AutoHidden>,
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

/// A span the rules found doubtful. Every one is hidden (stage 3): as `known` when the
/// filter already knows what goes out, otherwise with a new tag by its role.
#[derive(Debug, Clone)]
struct SuspectSpan {
    start: usize,
    end: usize,
    suspect: Suspect,
    /// A declared tag (a name inside a word, a close spelling) or a generalized job.
    known: Option<String>,
}

/// An auto-hidden item and the span of the replacement it explains.
#[derive(Debug, Clone)]
struct AutoSpan {
    start: usize,
    end: usize,
    item: AutoHidden,
}

pub(crate) fn is_generic(word: &str) -> bool {
    norm_set(GENERIC_PLACE_WORDS).contains(word)
}

pub(crate) fn is_title(word: &str) -> bool {
    norm_set(TITLES).contains(word)
}

fn is_role_word(word: &str) -> bool {
    prefix_splits(word)
        .iter()
        .any(|(_, h)| norm_set(ROLE_WORDS).contains(h))
}

/// A word for a person ("הגננת", "האח", "סבתא"), with or without "ה" and a prefix.
fn is_person_word(word: &str) -> bool {
    prefix_splits(word).iter().any(|(_, h)| {
        let bare = h.strip_prefix('ה').unwrap_or(h);
        let set = norm_set(PERSON_WORDS);
        set.contains(h) || set.contains(bare)
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
        // A defective spelling that is an everyday word ("מכל" from "מיכל", "בלי" from
        // "בילי"), or makes one after a prefix ("כללי" = כ + "ללי" from "לילי"), is not a
        // variant: a guess at a misspelling never rewrites an ordinary word.
        for v in spelling_variants(w)
            .into_iter()
            .filter(|v| v == w || !variant_reads_as_word(v))
        {
            out.push(vec![v]);
        }
        // The same person by a nickname or the full form ("שמעון" ↔ "שימי", "ולדה" ↔
        // "ולדימיר"): hidden, and refused by the gate, like the declared spelling.
        for nick in LEXICON.nicknames.get(w).into_iter().flatten() {
            if !out
                .iter()
                .any(|p| matches!(p.as_slice(), [only] if only == nick))
            {
                out.push(vec![nick.clone()]);
            }
        }
    }
    out
}

fn variant_reads_as_word(v: &str) -> bool {
    LEXICON.common_words.contains(v)
        || word_bucket(v) >= EVERYDAY
        || PREFIX_LETTERS
            .iter()
            .any(|p| word_bucket(&format!("{p}{v}")) >= EVERYDAY)
}

/// A name the filter added on its own (not the psychologist) that is also an everyday word
/// ("גיל", "שמחה", "בן") is not matched as a declared name: it was found by its context, and
/// is found again by it (with the same tag), in the pipeline and in the gate alike. Matched
/// everywhere it would hide "בגיל 3" and "בשמחה" in every later document of the case.
fn found_word(identity: &Identity, phrase: &[String]) -> bool {
    identity.source != IdentitySource::Manual && matches!(phrase, [w] if everyday_name(w))
}

/// A name that is also an everyday word on its own ("אלה", "חן", "מימון", "אדם").
#[must_use]
pub fn everyday_name(w: &str) -> bool {
    LEXICON.word_names.contains(w) || LEXICON.common_words.contains(w) || word_bucket(w) >= EVERYDAY
}

/// The place of token `i` is where a name goes: after a word for a person or a naming word
/// ("הגננת", "עם", "בשם"), before a reporting verb ("סיפרה", "הגיע"), next to another name, or
/// "with X" ending its clause ("משחקת עם אלה."). Used for a name that is also a word: only
/// here is it read as the name.
pub(crate) fn name_context_at(text: &str, tokens: &[Token], i: usize) -> bool {
    let lex = &*LEXICON;
    // No token at `i`: a name's place, so the word stays hidden.
    let Some(here) = tokens.get(i) else {
        return true;
    };
    let prev = i
        .checked_sub(1)
        .and_then(|j| tokens.get(j))
        .map(|t| t.norm.as_str());
    let next = tokens.get(i + 1).map(|t| t.norm.as_str());
    let in_list = |w: Option<&str>, list: &'static [&'static str]| {
        w.is_some_and(|w| norm_set(list).contains(w))
    };
    let after_prev =
        in_list(prev, crate::lexicon::NAME_CONTEXT_PREV) || prev.is_some_and(is_person_word);
    let before_next = in_list(next, crate::lexicon::NAME_CONTEXT_NEXT);
    let next_is_name = tokens.get(i + 1).is_some_and(|t| {
        lex.first_names.contains(&t.norm)
            || lex.surnames.contains(&t.norm)
            || t.norm
                .strip_prefix('ו')
                .is_some_and(|h| lex.first_names.contains(h))
    });
    let definite_next = next.is_some_and(|n| n.starts_with('ה') && word_bucket(n) >= EVERYDAY);
    // A gap that cannot be read (never expected) reads as a name's context: hidden.
    let ends_clause = tokens.get(i + 1).is_none_or(|t| {
        text.get(here.end..t.start)
            .is_none_or(|g| g.contains([',', '.', ';', ')', '\n']))
    });
    let with = in_list(prev, &["עם", "ועם", "אצל", "ואצל"]);
    // "אלון, הילד מהתיק השני," / "נוגה, אחותו": a person word right after a comma.
    let apposition = tokens.get(i + 1).is_some_and(|t| {
        text.get(here.end..t.start).is_none_or(|g| g.trim() == ",")
            && is_person_word(t.norm.strip_prefix('ה').unwrap_or(&t.norm))
    });
    before_next
        || next_is_name
        || apposition
        || (after_prev && !definite_next)
        || (with && ends_clause)
}

/// Another case's name matched here is the everyday word it also is (D-050), on positive
/// evidence only, never in a name's place:
/// * an ordinary word that merely contains it ("שאלון" when another child is "אלון");
/// * a preposition or article glued on, where the joined form is itself everyday ("בגיל",
///   "לאור", "האור");
/// * "אלה"/"אלו" right after a plural noun ("משימות אלה");
/// * she pressed "להחזיר" on it in this case (it is "not a name" here): without this the next
///   filter hid it again and the button did nothing.
///
/// Otherwise it is hidden, fail-closed ("אלון, הילד מהתיק השני"). Such names used to be hidden
/// everywhere, and kept as names in every case. The pipeline and the gate both use this.
/// The case's own names are not affected: what she typed for this case stays hidden.
pub(crate) fn other_case_word(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    first: usize,
    last: usize,
    prefix: usize,
) -> bool {
    if first != last || name_context_at(text, tokens, first) {
        return false;
    }
    // No token at `first`: not shown to be a word, so it stays hidden.
    let Some(whole) = tokens.get(first) else {
        return false;
    };
    let head: String = whole.norm.chars().skip(prefix).collect();
    if (ctx.allowlisted)(&whole.norm) || (ctx.allowlisted)(&head) {
        return true;
    }
    if prefix > 0
        && (LEXICON.common_words.contains(&whole.norm) || reads_as_word(&whole.norm, &head))
    {
        return true;
    }
    let before = first.checked_sub(1).and_then(|j| tokens.get(j));
    let plural_before = before.is_some_and(|t| {
        let w = t.norm.strip_prefix('ה').unwrap_or(&t.norm);
        // A gap that cannot be read (never expected) is not shown to be one space.
        text.get(t.end..whole.start) == Some(" ")
            && (w.ends_with("ימ") || w.ends_with("ות"))
            && word_bucket(w) >= EVERYDAY
    });
    prefix == 0 && matches!(head.as_str(), "אלה" | "אלו") && plural_before
}

/// A word that was a name in one of her past reports. Everyday words never count: they are not
/// kept, and a lexicon change must not turn "בגיל" into a name.
pub(crate) fn past_name(ctx: &PrivacyContext<'_>, h: &str) -> bool {
    !everyday_name(h) && !(ctx.allowlisted)(h) && (ctx.past_names)(h)
}

/// The last index built, with exactly what it was built from. The filter and the gate ask
/// for it 30–60 times while one section is prepared, each time with the same names, and a
/// build walks every name of every case (the review of the night's rounds). A different case,
/// name, alias or practitioner builds a new one. Equality of the inputs, not a hash, decides,
/// so what comes back is always what a fresh build would give. Cleared when the vault locks
/// ([`forget_identity_index`]).
struct BuiltIndex {
    case_id: String,
    identities: Vec<Identity>,
    practitioner: Vec<String>,
    index: Arc<PhraseIndex<Target>>,
}

static LAST_INDEX: Mutex<Option<BuiltIndex>> = Mutex::new(None);

/// Index of every declared name (see [`build_identity_index`]), built once for the same names.
pub(crate) fn identity_index(ctx: &PrivacyContext<'_>) -> Arc<PhraseIndex<Target>> {
    // A poisoned lock is never trusted: the index is built afresh.
    if let Ok(last) = LAST_INDEX.lock() {
        if let Some(b) = last.as_ref().filter(|b| {
            b.case_id == ctx.case_id
                && b.identities.as_slice() == ctx.identities
                && b.practitioner.as_slice() == ctx.practitioner
        }) {
            return Arc::clone(&b.index);
        }
    }
    let index = Arc::new(build_identity_index(ctx));
    if let Ok(mut last) = LAST_INDEX.lock() {
        *last = Some(BuiltIndex {
            case_id: ctx.case_id.to_owned(),
            identities: ctx.identities.to_vec(),
            practitioner: ctx.practitioner.to_vec(),
            index: Arc::clone(&index),
        });
    }
    index
}

/// Drops the kept index and the names it was built from (the vault locked).
pub fn forget_identity_index() {
    if let Ok(mut last) = LAST_INDEX.lock() {
        *last = None;
    }
}

/// Index of every declared name. Current-case identities are inserted first, so a name that
/// exists in two cases maps to this case's tag.
fn build_identity_index(ctx: &PrivacyContext<'_>) -> PhraseIndex<Target> {
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
                if !found_word(identity, &phrase) {
                    idx.insert_words(phrase, target.clone());
                }
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

/// The consonants of a name, in one alphabet for both scripts, so a name declared in Hebrew
/// is found in Arabic ("ג'סר" = "جسر", "סמאח" = "سماح"). Vowel letters (א ו י / ا و ي) and
/// a final ה / ه are left out, and letters that Hebrew spells alike share a class
/// (س/ص = ס/צ, ت/ط = ת/ט, ك/ق = כ/ק). `None` when the word is not in that script.
fn consonants(word: &str, arabic: bool) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let geresh = chars.get(i + 1).is_some_and(|n| *n == '\'');
        let last = i + 1 == chars.len() || (geresh && i + 2 == chars.len());
        let class = if arabic {
            match c {
                'ب' => 'b',
                'ت' | 'ط' | 'ث' => 't',
                'ج' => 'j',
                'ح' => 'h',
                'خ' => 'x',
                'د' | 'ذ' | 'ض' => 'd',
                'ر' => 'r',
                'ز' | 'ظ' => 'z',
                'س' | 'ص' => 's',
                'ش' => 'c',
                'ع' => 'e',
                'غ' => 'g',
                'ف' => 'f',
                'ق' | 'ك' => 'k',
                'ل' => 'l',
                'م' => 'm',
                'ن' => 'n',
                'ه' if !last => 'w',
                'ا' | 'و' | 'ي' | 'ه' | 'ء' => ' ',
                _ => return None,
            }
        } else {
            let class = match (c, geresh) {
                ('ג', true) | ('ז', true) => 'j',
                ('ח', true) => 'x',
                ('ע', true) => 'g',
                ('צ', true) | ('ד', _) => 'd',
                ('ט', true) => 'z',
                ('ת', _) | ('ט', _) => 't',
                ('ב', _) => 'b',
                ('ג', _) => 'j',
                ('ז', _) => 'z',
                ('ח', _) => 'h',
                ('כ', _) | ('ק', _) => 'k',
                ('ל', _) => 'l',
                ('מ', _) => 'm',
                ('נ', _) => 'n',
                ('ס', _) | ('צ', _) => 's',
                ('ע', _) => 'e',
                ('פ', _) => 'f',
                ('ר', _) => 'r',
                ('ש', _) => 'c',
                ('ה', _) if !last => 'w',
                ('א', _) | ('ו', _) | ('י', _) | ('ה', _) => ' ',
                _ => return None,
            };
            if geresh {
                i += 1;
            }
            class
        };
        if class != ' ' {
            out.push(class);
        }
        i += 1;
    }
    Some(out)
}

/// Declared names written in Arabic script ("روضة جسر" when the kindergarten is "ג'סר"):
/// the pipeline replaces them and the gate refuses them, like the Hebrew spelling. Only
/// names of three consonants or more, so short words do not collide.
pub(crate) fn arabic_identity_matches(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
) -> Vec<(usize, usize, Target)> {
    let arabic = |t: &Token| {
        t.norm
            .chars()
            .any(|c| ('\u{0620}'..='\u{064A}').contains(&c))
    };
    if !tokens.iter().any(arabic) {
        return Vec::new();
    }
    let mut declared: Vec<(String, Target)> = Vec::new();
    // A place ("כפר כנא", "ג'סר") is matched from two consonants (D-132): its words are
    // short, and the Arabic token must then be three letters or more. People from three.
    let mut add = |name: &str, target: Target, floor: usize| {
        for w in normalize(name).split(' ') {
            if is_title(w) || is_generic(w) {
                continue;
            }
            if let Some(k) = consonants(w, false).filter(|k| k.chars().count() >= floor) {
                declared.push((k, target.clone()));
            }
        }
    };
    let mut ordered: Vec<&Identity> = ctx
        .identities
        .iter()
        .filter(|i| i.case_id == ctx.case_id)
        .collect();
    ordered.extend(ctx.identities.iter().filter(|i| i.case_id != ctx.case_id));
    for identity in ordered {
        let target = Target::Identity {
            case_id: identity.case_id.clone(),
            tag: identity.tag.clone(),
            role: identity.role,
        };
        let floor = match identity.role {
            Role::Kindergarten | Role::School | Role::Town | Role::Institution => 2,
            _ => 3,
        };
        for name in std::iter::once(&identity.value).chain(identity.aliases.iter()) {
            add(name, target.clone(), floor);
        }
    }
    for name in ctx.practitioner {
        add(name, Target::Practitioner, 3);
    }
    // The generic words of a place in Arabic ("روضة", "مدرسة", "كفر"): never a name on
    // their own, as `is_generic` reads their Hebrew counterparts.
    const ARABIC_GENERIC: &[&str] = &[
        "روضة",
        "روضه",
        "حضانة",
        "حضانه",
        "مدرسة",
        "مدرسه",
        "بستان",
        "قرية",
        "قريه",
        "مدينة",
        "مدينه",
        "بلدة",
        "بلده",
        "كفر",
        "بيت",
        "مستشفى",
        "عيادة",
        "عياده",
        "مركز",
        "شارع",
        "حي",
        "جامعة",
        "جامعه",
        "مؤسسة",
        "مؤسسه",
    ];
    let mut out = Vec::new();
    for t in tokens
        .iter()
        .filter(|t| arabic(t) && t.norm.chars().count() >= 3)
    {
        // Prefixes: "و", "ال", "ب", "ل", "ك", "ف" and their combinations.
        let chars: Vec<char> = t.norm.chars().collect();
        for skip in 0..chars.len().min(4) {
            let glued: String = chars.iter().take(skip).collect();
            if ![
                "", "و", "ال", "وال", "ب", "بال", "ل", "لل", "ول", "وب", "ك", "ف",
            ]
            .contains(&glued.as_str())
            {
                continue;
            }
            let rest: String = chars.iter().skip(skip).collect();
            if ARABIC_GENERIC.contains(&rest.as_str()) {
                break;
            }
            let Some(k) = consonants(&rest, true) else {
                continue;
            };
            if let Some((_, target)) = declared.iter().find(|(d, _)| *d == k) {
                // A word that cannot be read (never expected) is matched whole.
                let start = t.start
                    + text
                        .get(t.start..t.end)
                        .map_or(0, |raw| crate::text::byte_len_of_letters(raw, skip));
                out.push((start, t.end, target.clone()));
                break;
            }
        }
    }
    out
}

fn overlaps(start: usize, end: usize, reps: &[Replacement], sus: &[SuspectSpan]) -> bool {
    reps.iter().any(|r| start < r.end && r.start < end)
        || sus.iter().any(|s| start < s.end && s.start < end)
}

fn role_label(role: Role) -> String {
    format!("שם {}", role.label_he())
}

/// Declared single words of the current case, for near-miss spelling detection. Names the
/// filter found itself are left out: a close spelling of a guess is not evidence.
fn declared_words(ctx: &PrivacyContext<'_>) -> Vec<(String, String)> {
    ctx.identities
        .iter()
        .filter(|i| i.case_id == ctx.case_id && i.source != IdentitySource::Auto)
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
fn is_profession_context(cue: &str, next: Option<&str>, after: Option<&str>) -> bool {
    let work = ["עובד", "עובדת", "עובדים", "מועסק", "מועסקת"]
        .iter()
        .any(|w| norm_eq(w, &cue));
    let manager = ["מנהל", "מנהלת"].iter().any(|w| norm_eq(w, &cue));
    let Some(next) = next else {
        return !work && !manager;
    };
    if work
        && [
            "בלילות",
            "במשמרות",
            "במשמרת",
            "במשרה",
            "בחצי",
            "בבקרים",
            "בערבים",
            "בשעות",
            "בסופי",
            "בימי",
            "בשבתות",
            "בחופשה",
            "בעבודה",
            "מהבית",
            "בבוקר",
            "בערב",
            "בלילה",
            "קשה",
            "הרבה",
            "שעות",
            "עכשיו",
            "כרגיל",
            "כמו",
        ]
        .iter()
        .any(|w| norm_eq(w, &next))
    {
        // "עובד בלילות ופחות זמין" is a schedule; "עובד במשמרות בנמל" still names the place.
        return after.is_some_and(|a| a.starts_with(['ב', 'כ']) && a != normalize("בבית"));
    }
    if work {
        return next.starts_with('כ')
            || (next.starts_with('ב')
                && !["בגן", "בקבוצה", "בבית", "בכיתה", "בשיתוף", "בטיפול"]
                    .iter()
                    .any(|w| norm_eq(w, &next)))
            || ["בתור", "אצל"].iter().any(|w| norm_eq(w, &next));
    }
    if manager {
        return ![
            "הגן",
            "בית",
            "המסגרת",
            "הצהרון",
            "המעון",
            "הכיתה",
            "הצוות",
            "השפ\"ח",
            "החינוך",
            "את",
            "שיחה",
        ]
        .iter()
        .any(|w| norm_eq(w, &next));
    }
    true
}

/// A form seen often enough in ordinary text to be an everyday word (1 + log2 of its count in
/// about 200 million words; 9 ≈ 256 times).
const EVERYDAY: u8 = 9;

/// `whole` = a prefix + `name`, and the whole form is an everyday word that is not mostly
/// that name with a prefix: "מלאה" (מ + לאה), "בשמחה", but not "לדני".
pub(crate) fn reads_as_word(whole: &str, name: &str) -> bool {
    let w = word_bucket(whole);
    w >= EVERYDAY && w + 3 >= word_bucket(name)
}

/// A declared name that is also an everyday word, used where only the word fits the grammar:
/// "שני ההורים", "אלה הדברים", "בגיל 3", "לגיל הכרונולוגי", "מתן זמן", "בוקר אור". A name
/// takes no possessive and no counted plural noun, so these are certain; anything else,
/// the article included ("המיכל" may be a typo for the name), stays hidden. The pipeline and
/// the gate both use this, so what one leaves the other accepts.
pub(crate) fn declared_reads_as_word(
    text: &str,
    tokens: &[Token],
    first: usize,
    last: usize,
    prefix: usize,
) -> bool {
    if first != last {
        return false;
    }
    // No token at `first`: not shown to be a word, so the name stays hidden.
    let Some(whole) = tokens.get(first) else {
        return false;
    };
    let letters: Vec<char> = whole.norm.chars().collect();
    let head: String = letters.iter().skip(prefix).collect();
    if word_bucket(&head) < EVERYDAY {
        return false;
    }
    // The last prefix letter glued on, if any.
    let glued_last = prefix.checked_sub(1).and_then(|k| letters.get(k)).copied();
    // "בגיל", "לגיל", "מגיל", "כגיל": a preposition glued on.
    let preposition = matches!(glued_last, Some('ב' | 'ל' | 'מ' | 'כ'));
    // A gap that cannot be read (never expected) leaves no neighbour: no sign of the word.
    let plain_after = tokens
        .get(first + 1)
        .filter(|n| text.get(whole.end..n.start) == Some(" "))
        .map(|n| n.norm.as_str());
    let dash_after = tokens
        .get(first + 1)
        .filter(|n| {
            text.get(whole.end..n.start)
                .is_some_and(|g| g.trim().chars().all(|c| matches!(c, '-' | '–' | '־')))
        })
        .map(|n| n.norm.as_str());
    let before = first
        .checked_sub(1)
        .and_then(|j| tokens.get(j))
        .filter(|t| text.get(t.end..whole.start) == Some(" "))
        .map(|t| t.norm.as_str());
    let one_of =
        |w: Option<&str>, list: &[&str]| w.is_some_and(|w| list.iter().any(|l| norm_eq(l, &w)));
    const NUMBER_WORDS: &[&str] = &[
        "שלוש",
        "שלושה",
        "ארבע",
        "ארבעה",
        "חמש",
        "חמישה",
        "שש",
        "שישה",
        "שבע",
        "שבעה",
    ];
    // A plural noun: "ההורים", "פריטים", "מילים", "השפות" – not "לפעמים" or "בדרך".
    let plural = |w: Option<&str>| {
        w.is_some_and(|w| {
            let bare = w.strip_prefix('ה').unwrap_or(w);
            (bare.ends_with("ימ") || bare.ends_with("ות"))
                && !bare.starts_with(['ל', 'ב', 'כ', 'ו', 'ש'])
                && word_bucket(bare) >= EVERYDAY
        })
    };
    match head.as_str() {
        "שני" | "שתי" | "אלה" | "אלו" => {
            plural(plain_after)
                || one_of(dash_after, NUMBER_WORDS)
                || (head == "שני" && one_of(before, &["יומ", "ביומ", "וביומ", "ימי", "בימי"]))
        }
        "גיל" => {
            // A number after a bare "גיל" may be a sibling's age in a list ("גיל 5, שני 3").
            let counted = preposition || one_of(before, &["עד", "מעל", "מתחת", "בסביבות"]);
            (counted
                && (plain_after.is_some_and(|w| w.starts_with(|c: char| c.is_ascii_digit()))
                    || one_of(
                        plain_after,
                        &[
                            "אפס",
                            "שנה",
                            "שנתיימ",
                            "חודש",
                            "חודשימ",
                            "חצי",
                            "שלוש",
                            "ארבע",
                            "חמש",
                            "שש",
                            "שבע",
                        ],
                    )))
                || one_of(
                    plain_after,
                    &[
                        "הזה",
                        "הזאת",
                        "הגנ",
                        "הכרונולוגי",
                        "כרונולוגי",
                        "צעיר",
                        "צעירה",
                        "מוקדמ",
                        "מבוגר",
                        "הרכ",
                        "ההתבגרות",
                        "שלו",
                        "שלה",
                        "שלהמ",
                    ],
                )
        }
        "מתנ" => one_of(
            plain_after,
            &[
                "זמנ",
                "מענה",
                "הוראות",
                "טיפול",
                "משוב",
                "אפשרות",
                "הסבר",
                "הסברימ",
                "דוגמה",
                "חיזוק",
                "חיזוקימ",
                "תגמול",
                "תמיכה",
                "הפסקות",
                "התאמות",
                "הקלות",
                "ביטוי",
                "עדיפות",
                "תשובה",
                "ציונ",
                "כלימ",
                "מקומ",
            ],
        ),
        "אור" => {
            // "לאור זאת", "ולאור הממצאים": "in light of".
            let in_light_of = glued_last == Some('ל')
                && one_of(
                    plain_after,
                    &[
                        "זאת",
                        "זה",
                        "האמור",
                        "הממצאימ",
                        "הנתונימ",
                        "העובדה",
                        "התוצאות",
                        "הקשיימ",
                        "המצב",
                        "הדברימ",
                        "כל",
                        "מה",
                        "ההתרשמות",
                        "הממצא",
                    ],
                );
            in_light_of
                || one_of(before, &["בוקר"])
                || one_of(
                    plain_after,
                    &["דולק", "כבוי", "עמומ", "בהיר", "מהבהב", "חזק"],
                )
        }
        // "ציון 112", "ציון T", "ציון כולל", "ציון מסתגל כללי": a score. Not before an
        // adjective alone ("ציון גבוה מאחיו") or a verb ("ציון מסתגל לגן").
        "ציונ" => {
            plain_after.is_some_and(|w| w.starts_with(|c: char| c.is_ascii_digit()))
                || one_of(
                    plain_after,
                    &[
                        "t",
                        "z",
                        "iq",
                        "כולל",
                        "השוואה",
                        "תקנ",
                        "גולמי",
                        "משוקלל",
                        "חתכ",
                    ],
                )
                || (one_of(plain_after, &["מסתגל"])
                    && tokens.get(first + 2).is_some_and(|t| t.norm == "כללי"))
        }
        // "מוטוריקה עדינה": an adjective.
        "עדינה" => one_of(
            before,
            &["מוטוריקה", "ומוטוריקה", "במוטוריקה", "המוטוריקה", "בצורה"],
        ),
        // "חרדת פרידה", "פרידה מההורה": separating, not a person.
        "פרידה" => {
            one_of(
                before,
                &[
                    "חרדת",
                    "קשיי",
                    "קושי",
                    "תגובת",
                    "תהליכ",
                    "רגע",
                    "רגעי",
                    "מצבי",
                ],
            ) || one_of(
                plain_after,
                &[
                    "מההורה",
                    "מההורימ",
                    "מהאמ",
                    "מהאב",
                    "מאמא",
                    "מאבא",
                    "מאמ",
                    "מאב",
                ],
            )
        }
        _ => false,
    }
}

/// Inflected forms of `word` (plural, feminine) appear in the word statistics: an adjective,
/// a noun or a verb, not a name.
fn inflects(word: &str) -> bool {
    let stem = word
        .strip_suffix("ית")
        .or_else(|| word.strip_suffix('ה'))
        .or_else(|| word.strip_suffix('ת'))
        .unwrap_or(word);
    let forms = [
        format!("{word}ימ"),
        format!("{word}ות"),
        format!("{word}ית"),
        format!("{stem}ימ"),
        format!("{stem}ות"),
        format!("{stem}יות"),
        format!("{stem}יימ"),
    ];
    // A past-tense verb ("תיווכה" → "תיווכתי", "תיווכנו"): first-person forms are only verbs.
    let conjugated = word.strip_suffix('ה').is_some_and(|s| {
        word_bucket(&format!("{s}תי")) >= 4 || word_bucket(&format!("{s}נו")) >= 4
    });
    conjugated
        || forms
            .iter()
            .filter(|f| f.as_str() != word)
            .any(|f| word_bucket(f) >= 4)
}

/// `a` and `b` differ only in an ending (the last letter, or a letter added at the end) or in
/// the article ה at the start: inflections of a word, not a misspelling inside it.
fn differs_at_end(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (long, short) = if a.len() >= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    match long.len() - short.len() {
        // The same but for the last letter (and more than that letter).
        0 => matches!(
            (long.split_last(), short.split_last()),
            (Some((_, a)), Some((_, b))) if !a.is_empty() && a == b
        ),
        // A letter added at the end, or the article at the start.
        1 => {
            long.starts_with(short)
                || long
                    .split_first()
                    .is_some_and(|(&c, rest)| c == 'ה' && rest == short.as_slice())
        }
        _ => false,
    }
}

/// The text between two words, `a` before `b`.
fn between<'t>(text: &'t str, a: &Token, b: &Token) -> Result<&'t str, PrivacyError> {
    span_text(text, a.end, b.start)
}

/// `text[start..end]`. Every offset the filter cuts at comes from the tokenizer, a regex
/// match or a character count, so it is always a character boundary; were one not, the whole
/// filtering fails (fail-closed) rather than guess what is there, and nothing is sent.
fn span_text(text: &str, start: usize, end: usize) -> Result<&str, PrivacyError> {
    text.get(start..end).ok_or_else(off_boundary)
}

/// An offset of the filter's own off a character boundary. Never expected. No text in it.
fn off_boundary() -> PrivacyError {
    PrivacyError::Internal("offset off a character boundary".to_owned())
}

/// A word the filter located itself is missing. Never expected; if it happens the whole
/// filtering fails (fail-closed) rather than guess what to hide.
fn out_of_range() -> PrivacyError {
    PrivacyError::Internal("token index out of range".to_owned())
}

/// Only spaces, a bullet or a list number between the start of the line (or a table cell)
/// and `at`. Text that cannot be read (never expected) counts as the start of a line: a
/// field label there takes the words after it as a name.
fn starts_line(text: &str, at: usize) -> bool {
    let Some(before) = text.get(..at) else {
        return true;
    };
    // After the last line break, bar or tab (all of the text when there is none).
    let line = before.rsplit(['\n', '|', '\t']).next().unwrap_or(before);
    line.chars().all(|c| {
        c.is_whitespace()
            || c.is_ascii_digit()
            || matches!(
                c,
                '.' | ')' | '(' | '-' | '•' | '*' | '·' | '–' | '\u{200F}' | '\u{200E}'
            )
    })
}

/// The tags already in the text (a stored draft sent again): a person's tag or a generic
/// one, numbered or not, which hold no word of their own. Not a "[חסר: …]" note or a score:
/// they hold words, still read by every rule.
fn protected_tags(text: &str, ctx: &PrivacyContext<'_>) -> Vec<Range<usize>> {
    crate::masks::TAG_RE
        .as_ref()
        .map(|re| {
            re.find_iter(text)
                .filter(|m| protected_tag(m.as_str(), ctx))
                .map(|m| m.range())
                .collect()
        })
        .unwrap_or_default()
}

fn protected_tag(tag: &str, ctx: &PrivacyContext<'_>) -> bool {
    crate::masks::is_generic_tag(tag) || ctx.identities.iter().any(|i| i.tag == tag)
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
) -> Result<(), PrivacyError> {
    let lex = &*LEXICON;
    let hebrew = |t: &Token| {
        t.norm
            .chars()
            .any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c))
    };
    let digits = |t: &Token| t.norm.chars().any(|c| c.is_ascii_digit());
    // The text between tokens `a` and `b`; `None` when either is past the last word.
    let gap = |a: usize, b: usize| match (tokens.get(a), tokens.get(b)) {
        (Some(x), Some(y)) => between(text, x, y).map(Some),
        _ => Ok(None),
    };
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
    let stop = |n: &str| NAME_STOP.iter().any(|s| norm_eq(s, &n));
    // A word inside a tag already in the text is never a name. Without this "משפחת [ילד]"
    // became "משפחת [[משפחה_1]]".
    let tags = protected_tags(text, ctx);
    // Inside a tag, or a prefix glued to one ("משפחת ו[ילד]"); only prefix letters, so a
    // word glued to a tag ("כהן[ת.ז.]") is still read.
    let in_tag = |start: usize, end: usize| {
        tags.iter().any(|t| {
            (t.start < start && end < t.end)
                || (t.start == end
                    && text
                        .get(start..end)
                        .is_some_and(|p| !p.is_empty() && crate::text::is_valid_prefix(p)))
        })
    };
    let push = |start: usize,
                end: usize,
                message: &str,
                out: &mut Vec<SuspectSpan>|
     -> Result<(), PrivacyError> {
        let token = span_text(text, start, end)?;
        if !in_tag(start, end)
            && !overlaps(start, end, reps, out)
            && !(ctx.allowlisted)(&normalize(token))
        {
            out.push(SuspectSpan {
                start,
                end,
                suspect: Suspect {
                    token: token.to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: message.to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            });
        }
        Ok(())
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
    for (i, tok) in tokens.iter().enumerate() {
        for (words, label) in &labels {
            let n = words.len();
            // The label's words, and the word right after them.
            let (Some(run), Some(after)) = (tokens.get(i..i + n), tokens.get(i + n)) else {
                continue;
            };
            let Some(last) = run.last() else {
                continue; // an empty label
            };
            if !run.iter().zip(words).all(|(t, w)| &t.norm == w) {
                continue;
            }
            // A form field starts its line (or a table cell): "הדרכת הורים: מתן זמן" is
            // a sentence, not the field "הורים:".
            if !starts_line(text, tok.start) {
                continue;
            }
            let g = between(text, last, after)?;
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
            let mut j = i + n;
            let mut taken = 0;
            while taken < 3 {
                let Some(t) = tokens.get(j) else {
                    break;
                };
                if stop(&t.norm) || !hebrew(t) || digits(t) {
                    break;
                }
                let role = prefix_splits(&t.norm)
                    .iter()
                    .any(|(_, h)| norm_set(ROLE_WORDS).contains(h));
                if !is_title(&t.norm)
                    && !role
                    && !is_person_word(&t.norm)
                    && t.norm != normalize("משפחת")
                {
                    push(t.start, t.end, &message, out)?;
                    taken += 1;
                }
                match tokens.get(j + 1) {
                    Some(next) if !breaks(between(text, t, next)?) => j += 1,
                    _ => break,
                }
            }
        }
    }

    // 2. "משפחת כהן", "למשפחת כהן-לוי".
    let family = normalize("משפחת");
    for (i, tok) in tokens.iter().enumerate() {
        if !prefix_splits(&tok.norm).iter().any(|(_, h)| *h == family) {
            continue;
        }
        let mut j = i + 1;
        while let Some(t) = tokens.get(j).filter(|t| hebrew(t) && !digits(t)) {
            push(t.start, t.end, "שם משפחה", out)?;
            match tokens.get(j + 1) {
                Some(next) if between(text, t, next)?.contains(['-', '\u{05BE}']) => j += 1,
                _ => break,
            }
        }
    }

    // 3. A name after a title: "ד\"ר קורנבליט", "הרב אלמוזנינו", "גב' ורשבסקי". Up to two words,
    // until punctuation; a declared name there is already replaced.
    for (i, (tok, next)) in tokens.iter().zip(tokens.iter().skip(1)).enumerate() {
        if !is_title(&tok.norm) || breaks(between(text, tok, next)?) {
            continue;
        }
        let mut j = i + 1;
        while j <= i + 2 {
            let Some(t) = tokens.get(j) else {
                break;
            };
            if stop(&t.norm)
                || !hebrew(t)
                || digits(t)
                || is_role_word(&t.norm)
                || lex.common_words.contains(&t.norm)
            {
                break;
            }
            // The second word after a first name is a surname only if it is no everyday
            // word: "ד\"ר אבנר בדק" hid the verb as a second doctor (D-132).
            if j > i + 1 && (word_bucket(&t.norm) >= EVERYDAY || inflects(&t.norm)) {
                break;
            }
            push(t.start, t.end, "שם אחרי תואר", out)?;
            // A second word only after a first name ("ד\"ר אבנר שטרן", not "ד\"ר כהן המליץ").
            match tokens.get(j + 1) {
                Some(n) if joins(between(text, t, n)?) && lex.first_names.contains(&t.norm) => {
                    j += 1;
                }
                _ => break,
            }
        }
    }

    let possessive = ["שלו", "שלה", "שלי", "שלנו", "שלהם", "שלהן", "שלך"].map(normalize);

    // 4. A name set off after a person word: "הגננת הקודמת, ברכה, תיארה…",
    // "האח הגדול – יהונתן – מתגייס", "הקלינאית (אינה) המליצה". Between the marks: one or
    // two words, which in a report can only be a name.
    let opens = |g: &str| g.contains([',', '–', '—', '(']) && !g.contains(['.', '\n', ':']);
    let closes = |g: &str| g.contains([',', '–', '—', ')', '.']);
    for (i, tok) in tokens.iter().enumerate() {
        if !is_person_word(&tok.norm) {
            continue;
        }
        // One adjective, possessive or "from the …" may follow the person word ("הקודמת",
        // "שלהם", "חברתה מהגן").
        let mut k = i;
        if tokens.get(k + 1).is_some_and(|n| {
            n.norm.starts_with('ה') || possessive.contains(&n.norm) || n.norm.starts_with("מה")
        }) && gap(k, k + 1)?.is_some_and(joins)
        {
            k += 1;
        }
        if !gap(k, k + 1)?.is_some_and(opens) {
            continue;
        }
        let first = k + 1;
        let mut last = first;
        if gap(last, last + 1)?.is_some_and(joins) {
            last += 1;
        }
        if !gap(last, last + 1)?.is_some_and(closes) {
            if gap(first, first + 1)?.is_some_and(closes) {
                last = first;
            } else {
                continue;
            }
        }
        // Both ends were read just above.
        let words = tokens.get(first..=last).ok_or_else(out_of_range)?;
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
                    && word_bucket(&t.norm) < EVERYDAY
                    && !APPOSITIVE_STOP.iter().any(|w| norm_eq(w, &t.norm)))
        };
        if words
            .iter()
            .all(|t| hebrew(t) && !digits(t) && !stop(&t.norm) && !is_person_word(&t.norm))
            && !words.iter().all(|t| lex.common_words.contains(&t.norm))
            && words
                .iter()
                .all(|t| plausible(t) || (words.len() == 2 && words.iter().any(&known)))
        {
            for t in words.iter().filter(|t| !is_title(&t.norm)) {
                push(t.start, t.end, "שם שמופיע ליד תפקיד", out)?;
            }
        }
    }

    // 4b. A child introduced by age or school year at the start of a sentence: "שירה, בת
    // שנתיים", "איתי, תלמיד כיתה ב'". A name that is also an everyday word ("with me", "a
    // song") is read as a name here: nothing else opens a sentence this way (round 32).
    let introduces = [
        "בן",
        "בת",
        "תלמיד",
        "תלמידת",
        "ילד",
        "ילדה",
        "פעוט",
        "פעוטה",
    ];
    for (i, tok) in tokens.iter().enumerate() {
        let opens_sentence = match i.checked_sub(1) {
            None => true,
            Some(p) => gap(p, i)?.is_some_and(|g| g.contains(['\n', '.', ':'])),
        };
        let known = lex.first_names.contains(&tok.norm)
            || lex.word_names.contains(&tok.norm)
            || lex.surnames.contains(&tok.norm);
        // No stop words here: "גיל, תלמיד כיתה ג'" can only be a name.
        if !opens_sentence || !known || !hebrew(tok) || digits(tok) {
            continue;
        }
        let comma = gap(i, i + 1)?.is_some_and(|g| g.trim() == ",");
        let introduced = tokens
            .get(i + 1)
            .is_some_and(|n| introduces.iter().any(|w| norm_eq(w, &n.norm)));
        if comma && introduced && !is_person_word(&tok.norm) && !is_title(&tok.norm) {
            push(tok.start, tok.end, "שם בפתח משפט, לפני גיל או כיתה", out)?;
        }
    }

    // 5. A name right after a word for a person, with no punctuation: "הגננת אסתי",
    // "סבא מוטי", "דודה שלו ולדה", "הסייעת החדשה רבקה", "המורה לאנגלית שירלי". What follows
    // must look like a name: a known name, or a word that is not an everyday word.
    for (i, tok) in tokens.iter().enumerate() {
        if !is_person_word(&tok.norm) {
            continue;
        }
        let mut j = i + 1;
        // One modifier may come between: "שלו", "הקודמת", "לאנגלית".
        let modifier = |m: &Token| {
            possessive.contains(&m.norm)
                || ["רבתא", "רבא", "רבה"].iter().any(|w| norm_eq(w, &m.norm))
                || (m.norm.starts_with(['ה', 'ל'])
                    && word_bucket(&m.norm) >= EVERYDAY
                    && !is_person_word(&m.norm))
        };
        if j + 1 < tokens.len() && gap(i, j)? == Some(" ") && tokens.get(j).is_some_and(modifier) {
            j += 1;
        }
        // A nickname may be quoted: סבתא שלה "נונה".
        let plain_gap = |g: &str| {
            g.contains(' ')
                && g.chars()
                    .all(|c| c == ' ' || matches!(c, '"' | '״' | '“' | '”' | '\''))
        };
        let Some(t) = tokens.get(j) else {
            continue;
        };
        if !gap(j - 1, j)?.is_some_and(plain_gap) {
            continue;
        }
        let known = lex.first_names.contains(&t.norm)
            || lex.word_names.contains(&t.norm)
            || lex.surnames.contains(&t.norm);
        // A rare word is a name only if it does not behave like a word: no article ("ה…"),
        // no prefix in front of an everyday word ("ומתקנת"), no inflected forms ("חייכן" →
        // "חייכנים", "משלימה" → "משלימים"). Names do not inflect.
        let rare = word_bucket(&t.norm) < EVERYDAY
            && !lex.common_words.contains(&t.norm)
            && t.norm.chars().count() >= 2
            && !t.norm.starts_with('ה')
            && !prefix_splits(&t.norm)
                .iter()
                .any(|(p, h)| *p > 0 && word_bucket(h) >= EVERYDAY)
            && !inflects(&t.norm);
        // "סבתא שרה לו", "הגננת קראה לה": a word that is also a name, followed by "לו" / "לה",
        // is the verb.
        let dative = match tokens.get(j + 1) {
            Some(n) => {
                between(text, t, n)? == " "
                    && ["לו", "לה", "לי", "לנו", "להם", "להן", "לך"]
                        .iter()
                        .any(|w| norm_eq(w, &n.norm))
            }
            None => false,
        };
        let known = known && !(dative && word_bucket(&t.norm) >= EVERYDAY);
        if hebrew(t)
            && !digits(t)
            && !t.norm.contains('"')
            && !stop(&t.norm)
            && !is_title(&t.norm)
            && !is_person_word(&t.norm)
            && !is_role_word(&t.norm)
            && (known || rare)
        {
            push(t.start, t.end, "שם אחרי תפקיד או קרבה", out)?;
        }
    }

    // 6. Whatever follows "קוראים לה", "בשם", "המכונה" is a name, even an ordinary word
    // ("הילדים קוראים לה נונה", "כלב בשם שוקו"). Not "שמה" / "שמו", which are also verbs
    // ("שמה לב"), and not a function word ("בשם כל הצוות").
    let naming: Vec<Vec<String>> = [
        "קוראים לו",
        "קוראים לה",
        "קוראים לי",
        "קוראים להם",
        "קוראת לו",
        "קוראת לה",
        "קורא לו",
        "קורא לה",
        "מכנים אותו",
        "מכנים אותה",
        "שם החיבה שלו",
        "שם החיבה שלה",
        "הכינוי שלו",
        "הכינוי שלה",
        "בשם",
        "המכונה",
    ]
    .iter()
    .map(|p| normalize(p).split(' ').map(str::to_owned).collect())
    .collect();
    for i in 0..tokens.len() {
        for phrase in &naming {
            let n = phrase.len();
            // The phrase's words, and the word right after them.
            let (Some(run), Some(t)) = (tokens.get(i..i + n), tokens.get(i + n)) else {
                continue;
            };
            if !run
                .iter()
                .zip(phrase)
                .all(|(t, w)| t.norm == *w || (n == 1 && t.norm == format!("ו{w}")))
            {
                continue;
            }
            let plain = gap((i + n).saturating_sub(1), i + n)?.is_some_and(|g| {
                g.chars()
                    .all(|c| c == ' ' || matches!(c, '"' | '״' | '“' | '”' | '\'' | ':'))
            });
            if plain
                && hebrew(t)
                && !digits(t)
                && !t.norm.starts_with('ה')
                && word_bucket(&t.norm) < 14
                && !stop(&t.norm)
                && !lex.common_words.contains(&t.norm)
                && !is_person_word(&t.norm)
                && !is_role_word(&t.norm)
            {
                push(t.start, t.end, "שם אחרי \"קוראים לו\" או \"בשם\"", out)?;
            }
        }
    }

    // 7. Two parts of one name joined by a hyphen, one of them a known name and the other
    // not an everyday word: "אביבה-טגסט", "שרה-לי". Ordinary compounds ("דו-לשוני",
    // "אי-שקט") have everyday words on both sides.
    for (a, b) in tokens.iter().zip(tokens.iter().skip(1)) {
        let g = between(text, a, b)?;
        if g != "-" && g != "\u{05BE}" {
            continue;
        }
        if !hebrew(a) || !hebrew(b) || digits(a) || digits(b) {
            continue;
        }
        let known =
            |t: &Token| lex.first_names.contains(&t.norm) || lex.word_names.contains(&t.norm);
        let unusual = |t: &Token| {
            known(t)
                || (word_bucket(&t.norm) < EVERYDAY
                    && !lex.common_words.contains(&t.norm)
                    && t.norm.chars().count() >= 3)
        };
        if (known(a) && unusual(b)) || (known(b) && unusual(a)) {
            push(a.start, b.end, "שם עם מקף", out)?;
        }
    }

    // 8. A surname right after a first name, a declared name or a name found above.
    let is_name = |t: &Token, out: &[SuspectSpan]| {
        reps.iter()
            .any(|r| r.declared && r.start <= t.start && t.end <= r.end)
            || out.iter().any(|s| s.start <= t.start && t.end <= s.end)
            || prefix_splits(&t.norm)
                .iter()
                .any(|(_, h)| lex.first_names.contains(h))
    };
    let mut surname_at: Option<usize> = None;
    for (k, (prev, t)) in tokens.iter().zip(tokens.iter().skip(1)).enumerate() {
        let i = k + 1;
        let g = between(text, prev, t)?;
        if !joins(g) || !hebrew(t) || digits(t) {
            continue;
        }
        // After a declared name only a tight hyphen joins a second part ("כהן-טגסט"): a
        // spaced dash is a pause ("נועם אביטל - מתקשה" hid "מתקשה" as a surname).
        let declared_before = reps
            .iter()
            .any(|r| r.declared && r.start <= prev.start && prev.end <= r.end);
        let hyphen_after_surname = surname_at == Some(k)
            && g.contains(['-', '\u{05BE}'])
            && (!declared_before || !g.contains(' '));
        let raw = span_text(text, t.start, t.end)?;
        let looks_like_surname = lex.surnames.contains(&t.norm)
            || (t.norm.chars().count() >= 4
                && SURNAME_ENDINGS
                    .iter()
                    .any(|e| t.norm.ends_with(&normalize(e))))
            // "אברג'יל", "חאג'": a geresh inside a Hebrew word is a borrowed sound, as in names.
            || (raw.chars().count() >= 4 && raw.contains(['\'', '׳']) && !raw.ends_with(['\'', '׳']));
        if hyphen_after_surname
            || (is_name(prev, out) && looks_like_surname && !lex.common_words.contains(&t.norm))
        {
            push(t.start, t.end, "נראה כמו שם משפחה", out)?;
            surname_at = Some(i);
        }
    }
    Ok(())
}

/// A name that is also one of the commonest words ("איתי", "שרה", "מתן", "שמחה": the list
/// `common_words`, otherwise never read as names) where only a name can stand: before a
/// reporting verb or another name ("שרה סיפרה", "מתן ועידו"), "עם X" ending its clause
/// ("משחקת בעיקר עם שרה."), and "עם איתי" anywhere: "with with-me" is not Hebrew, so it is
/// the friend ("מתרועע עם איתי מהשכונה" let an unlisted child's name out). "שוחח איתי" and
/// "עם שמחה רבה" stay words.
fn common_name_in_place(text: &str, tokens: &[Token], i: usize) -> Result<bool, PrivacyError> {
    let lex = &*LEXICON;
    let Some(tok) = tokens.get(i) else {
        return Ok(false);
    };
    if !(lex.first_names.contains(&tok.norm) || lex.word_names.contains(&tok.norm)) {
        return Ok(false);
    }
    let prev = i
        .checked_sub(1)
        .and_then(|j| tokens.get(j))
        .map(|t| t.norm.as_str());
    let next = tokens.get(i + 1);
    let with = prev.is_some_and(|p| ["עם", "ועם", "אצל", "ואצל"].iter().any(|w| norm_eq(w, p)));
    let before_next = next.is_some_and(|n| {
        crate::lexicon::NAME_CONTEXT_NEXT
            .iter()
            .any(|l| norm_eq(l, &n.norm))
    });
    // "שלום יעל," greets; "שלום אוחיון" is a man's name before his surname.
    let greeting = norm_eq("שלום", &tok.norm);
    let next_is_name = next.is_some_and(|t| {
        let first = lex.first_names.contains(&t.norm)
            || t.norm
                .strip_prefix('ו')
                .is_some_and(|h| lex.first_names.contains(h));
        (first && !greeting) || (lex.surnames.contains(&t.norm) && !first)
    });
    let ends_clause = match next {
        Some(t) => between(text, tok, t)?.contains([',', '.', ';', ')', '\n']),
        None => true,
    };
    let with_me = norm_eq("איתי", &tok.norm);
    // "לדבר, שרה, מספרת": a verb in a list of verbs, not a name before its verb.
    Ok((!ends_clause && (before_next || next_is_name)) || (with && (ends_clause || with_me)))
}

/// The commonest words that are names, where [`common_name_in_place`] says only a name can
/// stand and no other rule decided; a surname right after one goes with it ("שמחה אוחיון").
fn common_names(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &[Replacement],
    suspects: &[SuspectSpan],
) -> Result<Vec<SuspectSpan>, PrivacyError> {
    let lex = &*LEXICON;
    let tags = protected_tags(text, ctx);
    let mut out: Vec<SuspectSpan> = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        let taken = |s: usize, e: usize, out: &[SuspectSpan]| {
            overlaps(s, e, reps, suspects)
                || overlaps(s, e, &[], out)
                || tags.iter().any(|t| t.start < e && s < t.end)
        };
        if !lex.common_words.contains(&tok.norm)
            || (ctx.allowlisted)(&tok.norm)
            || taken(tok.start, tok.end, &out)
            || !common_name_in_place(text, tokens, i)?
        {
            continue;
        }
        let mut end = tok.end;
        if let Some(n) = tokens.get(i + 1) {
            if between(text, tok, n)? == " "
                && lex.surnames.contains(&n.norm)
                && !lex.common_words.contains(&n.norm)
                && !taken(n.start, n.end, &out)
            {
                end = n.end;
            }
        }
        out.push(SuspectSpan {
            start: tok.start,
            end,
            suspect: Suspect {
                token: span_text(text, tok.start, end)?.to_owned(),
                kind: SuspectKind::UnknownName,
                message: "שם שהוא גם מילה, בהקשר של שם".to_owned(),
                suggested_role: Role::Other,
            },
            known: None,
        });
    }
    Ok(out)
}

fn find_suspects(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &[Replacement],
) -> Result<Vec<SuspectSpan>, PrivacyError> {
    let lex = &*LEXICON;
    let declared = declared_words(ctx);
    let mut out: Vec<SuspectSpan> = Vec::new();
    name_run_suspects(text, tokens, ctx, reps, &mut out)?;
    for (i, tok) in tokens.iter().enumerate() {
        if overlaps(tok.start, tok.end, reps, &out) || (ctx.allowlisted)(&tok.norm) {
            continue;
        }
        // The commonest words are names only where `common_names` finds them, last.
        if lex.common_words.contains(&tok.norm) {
            continue;
        }
        let raw = span_text(text, tok.start, tok.end)?;
        let splits = prefix_splits(&tok.norm);
        let prev = i
            .checked_sub(1)
            .and_then(|j| tokens.get(j))
            .map(|t| t.norm.as_str());
        let next = tokens.get(i + 1).map(|t| t.norm.as_str());
        let span_from = |prefix: usize| tok.start + crate::text::byte_len_of_letters(raw, prefix);

        // "מלאה" is מ + לאה only on paper: an everyday word is read as a word, unless the
        // prefixed form is mostly the name ("לדני" is far rarer than "דני").
        let as_word = |p: usize, h: &str| p > 0 && reads_as_word(&tok.norm, h);
        // A name from one of her past reports, in any context (D-042, stage 6).
        if let Some((p, _)) = splits
            .iter()
            .find(|(p, h)| past_name(ctx, h) && !as_word(*p, h))
        {
            let start = span_from(*p);
            out.push(SuspectSpan {
                start,
                end: tok.end,
                suspect: Suspect {
                    token: span_text(text, start, tok.end)?.to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: "שם מדוח ישן".to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            });
            continue;
        }
        let plain = splits
            .iter()
            .find(|(p, h)| lex.first_names.contains(h) && !(ctx.allowlisted)(h) && !as_word(*p, h));
        // A name that is also a word needs a name-like context; "עם שני ההורים" is not one
        // (a definite noun right after it makes it a number or an adjective).
        let definite_next = next.is_some_and(|n| n.starts_with('ה') && word_bucket(n) >= EVERYDAY);
        let word = splits
            .iter()
            .find(|(p, h)| lex.word_names.contains(h) && !(ctx.allowlisted)(h) && !as_word(*p, h));
        // A very common word ("שני", "אלה", "טובה", "אור") is a name only before a reporting
        // verb or next to another name; a preposition before it is not enough.
        let very_common = word.is_some_and(|(_, h)| word_bucket(h) >= 14);
        let next_is_name = tokens.get(i + 1).is_some_and(|t| {
            lex.first_names.contains(&t.norm)
                || lex.surnames.contains(&t.norm)
                || t.norm
                    .strip_prefix('ו')
                    .is_some_and(|h| lex.first_names.contains(h))
        });
        // "משחקת בעיקר עם אלה." / "עם אדם, הבן של השכנים": the word ends its clause.
        let ends_clause = match tokens.get(i + 1) {
            Some(t) => between(text, tok, t)?.contains([',', '.', ';', ')', '\n']),
            None => true,
        };
        let word_in_context = word.filter(|_| {
            let after_prev = prev.is_some_and(|p| {
                crate::lexicon::NAME_CONTEXT_PREV
                    .iter()
                    .any(|l| norm_eq(l, &p))
            });
            let before_next = next.is_some_and(|n| {
                crate::lexicon::NAME_CONTEXT_NEXT
                    .iter()
                    .any(|l| norm_eq(l, &n))
            });
            if very_common {
                // "את אלה." is "these"; only "with" or a person word says a person follows.
                let with = prev.is_some_and(|p| {
                    ["עם", "ועם", "אצל", "ואצל"].iter().any(|w| norm_eq(w, &p)) || is_person_word(p)
                });
                return before_next || next_is_name || (with && ends_clause);
            }
            before_next || (after_prev && !definite_next)
        });
        if let Some((p, _)) = plain.or(word_in_context) {
            let start = span_from(*p);
            out.push(SuspectSpan {
                start,
                end: tok.end,
                suspect: Suspect {
                    token: span_text(text, start, tok.end)?.to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: if plain.is_some() {
                        "שם פרטי".to_owned()
                    } else {
                        "שם שהוא גם מילה, בהקשר של שם".to_owned()
                    },
                    suggested_role: Role::Other,
                },
                known: None,
            });
            continue;
        }
        // "ג'וד", "צ'רלי": a geresh inside a word that is not an everyday loanword is a
        // borrowed sound, as in names.
        // Loanwords have it too ("צ'אט", "ג'ינס"), so only where a name is expected.
        let letters: Vec<char> = raw.chars().collect();
        let inner_geresh = letters.len() >= 3
            && letters
                .split_last()
                .is_some_and(|(_, head)| head.iter().any(|c| matches!(c, '\'' | '׳')))
            && letters
                .iter()
                .filter(|c| ('\u{05D0}'..='\u{05EA}').contains(*c))
                .count()
                >= 2
            && !letters.iter().any(char::is_ascii_digit);
        let geresh_context = is_name_context(prev, next)
            || prev.is_some_and(|p| {
                ["הוא", "היא", "שמו", "שמה", "בשם", "קוראים"]
                    .iter()
                    .any(|w| norm_eq(w, &p))
            });
        if inner_geresh
            && geresh_context
            && word_bucket(&tok.norm) < EVERYDAY
            && !lex.common_words.contains(&tok.norm)
        {
            out.push(SuspectSpan {
                start: tok.start,
                end: tok.end,
                suspect: Suspect {
                    token: raw.to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: "מילה עם גרש שנראית כמו שם".to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            });
            continue;
        }
        // Two letters ("Sc", "Dr") are abbreviations, not names.
        let is_capitalized_latin = raw.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && raw.chars().skip(1).all(|c| c.is_ascii_lowercase())
            && raw.len() > 2;
        if is_capitalized_latin && !lex.latin_allow.contains(&tok.norm) {
            out.push(SuspectSpan {
                start: tok.start,
                end: tok.end,
                suspect: Suspect {
                    token: raw.to_owned(),
                    kind: SuspectKind::LatinName,
                    message: "מילה באנגלית שנראית כמו שם".to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            });
            continue;
        }
        let near = splits.iter().find_map(|(p, h)| {
            if h.chars().count() < 4 {
                return None;
            }
            // "איתה" next to "איתי", "דמיון" next to "דמיוני": an everyday word that differs
            // from the name only in its ending is the word, unless the context says it is a
            // person. A change inside the word ("נואם" for "נועם") is still a misspelling.
            // Short names make many such neighbours ("שוני", "שאני" next to "שני"), so for a
            // known word only a same-length change inside it counts.
            let known_word = word_bucket(h) >= 7 && !is_name_context(prev, next);
            declared
                .iter()
                .find(|(w, _)| {
                    weak_near(h, w)
                        && !(known_word
                            && (h.chars().count() != w.chars().count() || differs_at_end(h, w)))
                })
                .map(|(_, tag)| (*p, tag.clone()))
        });
        if let Some((p, tag)) = near {
            let start = span_from(p);
            out.push(SuspectSpan {
                start,
                end: tok.end,
                suspect: Suspect {
                    token: span_text(text, start, tok.end)?.to_owned(),
                    kind: SuspectKind::SimilarToDeclared,
                    message: "כתיב קרוב לשם שבתיק".to_owned(),
                    suggested_role: Role::Other,
                },
                known: Some(tag),
            });
            continue;
        }
        if PROFESSION_CUES.iter().any(|c| norm_eq(c, &tok.norm))
            && is_profession_context(&tok.norm, next, tokens.get(i + 2).map(|t| t.norm.as_str()))
        {
            // The cue and the rest of the job, up to the end of the clause ("עובדת כמורה
            // בבית ספר יסודי"), so the generalization replaces all of it.
            let end = job_end(text, tokens, i)?;
            if !overlaps(tok.start, end, reps, &out) {
                out.push(SuspectSpan {
                    start: tok.start,
                    end,
                    suspect: Suspect {
                        token: span_text(text, tok.start, end)?.to_owned(),
                        kind: SuspectKind::Indirect,
                        message: "מקצוע או מקום עבודה".to_owned(),
                        suggested_role: Role::Other,
                    },
                    known: Some(generalize_job(text, tokens, tok, end)?),
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
    Ok(out)
}

/// Run layers 2–6 on one piece of text.
pub fn filter(text: &str, ctx: &PrivacyContext<'_>) -> Result<FilterOutcome, PrivacyError> {
    filter_with_hints(text, ctx, &[])
}

/// A span a local name model (`dv-ner`) reads as a person, as a byte range of the text given
/// to the filter. A hint can only add a hide: whatever a rule already hid or kept stays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NameHint {
    pub start: usize,
    pub end: usize,
    /// The model's confidence, 0–1. Below [`NAME_HINT_SURE`] the card marks it uncertain.
    pub score: f32,
}

/// From this confidence up a model hint is listed as certain.
pub const NAME_HINT_SURE: f32 = 0.9;
const HINT_SURE: &str = "זוהה כשם לפי ההקשר";
const HINT_UNSURE: &str = "ייתכן שם, לפי ההקשר";

/// [`filter`], with the person spans of a local name model as one more voter (stage 5).
pub fn filter_with_hints(
    text: &str,
    ctx: &PrivacyContext<'_>,
    hints: &[NameHint],
) -> Result<FilterOutcome, PrivacyError> {
    let text = &plain_brackets(text, ctx);
    let (mut reps, suspects) = analyze(text, ctx, hints)?;
    number_masks(text, ctx, &mut reps)?;
    assemble(text, &reps, &suspects)
}

/// Her own words as the case keeps them (a paragraph she edited or wrote, D-126): hidden
/// exactly as [`filter_with_hints`] hides them, but nothing is left out or generalized. What
/// the outgoing filter would leave out (a kindergarten's name) or turn into relative time (a
/// date) is kept under a numbered tag of the case's mask book (`[מילים_3]`), so restoring
/// gives back exactly what she wrote. The tag carries only a number. Before her paragraph is
/// sent, the caller restores it whole ([`crate::restore::her_text_to_send`]) and filters it as
/// any text, so the model sees exactly the text it would have seen had she typed it then.
///
/// Fails closed without the mask book: the words would have nowhere to be kept.
pub fn filter_to_keep(
    text: &str,
    ctx: &PrivacyContext<'_>,
    hints: &[NameHint],
) -> Result<FilterOutcome, PrivacyError> {
    let Some(book) = ctx.masks else {
        return Err(PrivacyError::Internal(
            "keeping her words needs the case's mask book".to_owned(),
        ));
    };
    let text = &plain_brackets(text, ctx);
    let (mut reps, suspects) = analyze(text, ctx, hints)?;
    number_masks(text, ctx, &mut reps)?;
    for r in &mut reps {
        // A tag that restores to exactly these words stays: a person of this case as
        // declared, the practitioner's first name (what restoring writes), or a numbered tag
        // of the book kept for these very words. Anything else (left out, relative time, a
        // generic tag, a name spelled otherwise, "Noam" or "נעם" for "נועם", a number the book
        // keeps as written elsewhere, with a point or a space more) keeps her words under a
        // number.
        let words = span_text(text, r.start, r.end)?;
        let restorable = (crate::masks::is_mask_tag(&r.out)
            && book.original(&r.out).is_some_and(|o| o == words))
            || (r.out == PRACTITIONER_TAG && ctx.practitioner.first().is_some_and(|p| p == words))
            || ctx
                .identities
                .iter()
                .find(|i| i.case_id == ctx.case_id && i.tag == r.out)
                .is_some_and(|i| i.value == words);
        if !restorable {
            r.out = book.kept_words_tag(words);
            r.relative = false;
        }
    }
    assemble(text, &reps, &suspects)
}

/// With the case's mask book, each fixed tag of a kind (`[תאריך]`) becomes the numbered tag of
/// those words (`[תאריך_2]`), so the answer can be given back as she wrote it. The same span is
/// hidden either way; only the tag's name changes.
fn number_masks(
    text: &str,
    ctx: &PrivacyContext<'_>,
    reps: &mut [Replacement],
) -> Result<(), PrivacyError> {
    let Some(book) = ctx.masks else {
        return Ok(());
    };
    for r in reps.iter_mut().filter(|r| !r.declared && !r.relative) {
        if let Some(base) = crate::masks::mask_base(&r.out) {
            r.out = book.tag_for(base, span_text(text, r.start, r.end)?);
        }
    }
    Ok(())
}

/// Square brackets around words that are not a tag ("[מחנכת]" written by the model, "[...]"
/// in a document) become round ones, so the words inside are filtered like any other text and
/// never pass for a tag. A real tag stays: this case's (already filtered), a generic one, a
/// score of a past report in its fixed form (round 32), or another case's, which the gate
/// then refuses. Same length, so every offset still holds.
fn plain_brackets(text: &str, ctx: &PrivacyContext<'_>) -> String {
    let Some(re) = crate::masks::TAG_RE.as_ref() else {
        return text.to_owned();
    };
    let known = |t: &str| {
        crate::masks::is_generic_tag(t)
            || crate::masks::is_missing_note(t)
            || dv_domain::is_score_tag(t)
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
    filter_split_with_hints(text, ctx, ranges, &[])
}

/// [`filter_split`], with a local name model's person spans as hints ([`filter_with_hints`]).
pub fn filter_split_with_hints(
    text: &str,
    ctx: &PrivacyContext<'_>,
    ranges: &[Range<usize>],
    hints: &[NameHint],
) -> Result<(FilterOutcome, Option<Vec<FilterOutcome>>), PrivacyError> {
    let text = &plain_brackets(text, ctx);
    let (mut reps, suspects) = analyze(text, ctx, hints)?;
    number_masks(text, ctx, &mut reps)?;
    let whole = assemble(text, &reps, &suspects)?;
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
        let part_autos: Vec<AutoSpan> = suspects
            .iter()
            .filter(|x| inside(x.start, x.end))
            .map(|x| AutoSpan {
                start: x.start - r.start,
                end: x.end - r.start,
                item: x.item.clone(),
            })
            .collect();
        parts.push(assemble(piece, &part_reps, &part_autos)?);
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
            auto_hidden: Vec::new(),
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
            for a in &p.auto_hidden {
                if !out.auto_hidden.contains(a) {
                    out.auto_hidden.push(a.clone());
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
    "הילד",
    "הילדה",
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

/// Where the values of address fields are, for the gate.
pub(crate) fn address_field_spans(text: &str) -> Vec<(usize, usize)> {
    address_fields(text)
        .into_iter()
        .map(|r| (r.start, r.end))
        .collect()
}

fn address_fields(text: &str) -> Vec<Replacement> {
    const LABELS: &[&str] = &["כתובת", "כתובת מגורים", "כתובת המשפחה", "כתובת הבית", "מען"];
    let mut out = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let rest = body.trim_start();
        let lead = body.len() - rest.len();
        for label in LABELS {
            let Some(after) = rest.strip_prefix(label) else {
                continue;
            };
            let Some(value) = after.trim_start().strip_prefix(':') else {
                continue;
            };
            // The value ends at the next field on the same line ("   טלפון: …", "| …").
            let value_start = line_start + lead + label.len() + (after.len() - value.len());
            let end_rel = value
                .find("  ")
                .into_iter()
                .chain(value.find(['|', '\t']))
                .min()
                .unwrap_or(value.len());
            let mail = ["מייל", "דוא\"ל", "אימייל", "דואר"]
                .iter()
                .any(|m| after.trim_start().starts_with(m));
            if mail {
                break;
            }
            // Each comma-separated part on its own, so a part already hidden (the declared
            // town) does not keep the rest from being hidden.
            let mut part_start = value_start;
            // `end_rel` is where a `find` stopped, or the end; were it not a boundary (never
            // expected), the whole value is taken.
            for part in value.get(..end_rel).unwrap_or(value).split(',') {
                let trimmed = part.trim();
                if !trimmed.is_empty() {
                    let start = part_start + (part.len() - part.trim_start().len());
                    out.push(Replacement {
                        start,
                        end: start + trimmed.len(),
                        out: "[כתובת]".to_owned(),
                        label: "כתובת".to_owned(),
                        relative: false,
                        declared: false,
                    });
                }
                part_start += part.len() + 1;
            }
            break;
        }
        line_start += line.len();
    }
    out
}

/// Is "מספרת" at `i` the verb ("tells")? A speaker right before it (a tag, a pronoun, a
/// person's role) or a clause right after it ("ש…" with a prefix, "כי", "על", "לי").
fn telling(text: &str, tokens: &[Token], i: usize) -> bool {
    const SPEAKERS: &[&str] = &[
        "היא",
        "אמה",
        "אמו",
        "האם",
        "האמא",
        "אמא",
        "הגננת",
        "גננת",
        "המורה",
        "הסייעת",
        "המטפלת",
        "הקלינאית",
        "המרפאה",
        "הסבתא",
        "האחות",
        "המחנכת",
        "ההורים",
        "היועצת",
    ];
    const AFTER: &[&str] = &[
        "כי",
        "על",
        "את",
        "לי",
        "לנו",
        "לה",
        "לו",
        "להם",
        "גם",
        "היום",
        "שהוא",
        "שהיא",
        "שלא",
        "שיש",
        "שאין",
        "שהם",
        "שלפעמים",
        "שכאשר",
        "שלאחרונה",
    ];
    // Text before that cannot be read (never expected) shows no speaker: "מספרת" is then
    // the place, and the word after it is hidden.
    let tag_before = tokens.get(i).is_some_and(|t| {
        text.get(..t.start)
            .is_some_and(|before| before.trim_end().ends_with(']'))
    });
    let before = tag_before
        || i.checked_sub(1)
            .and_then(|j| tokens.get(j))
            .is_some_and(|t| SPEAKERS.contains(&t.norm.as_str()));
    let after = tokens.get(i + 1).is_some_and(|t| {
        let n = t.norm.as_str();
        AFTER.contains(&n)
            || n.strip_prefix('ש')
                .and_then(|r| r.chars().next())
                .is_some_and(|c| matches!(c, 'ב' | 'ה'))
    });
    before || after
}

/// "גן השקד", "בגן \"הרימונים\"", "בית ספר \"אופקים חדשים\"", "בית הספר ע\"ש רבין": the name
/// after a word for a kindergarten or a school, as a replacement that leaves it out.
fn institution_names(text: &str, tokens: &[Token]) -> Result<Vec<Replacement>, PrivacyError> {
    let lex = &*LEXICON;
    let hebrew = |t: &Token| {
        t.norm
            .chars()
            .any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c))
    };
    let is = |t: &Token, words: &[&str]| {
        prefix_splits(&t.norm)
            .iter()
            .any(|(_, h)| words.iter().any(|w| norm_eq(w, h)))
    };
    // The text between tokens `a` and `b`; `None` when either is past the last word.
    let gap = |a: usize, b: usize| match (tokens.get(a), tokens.get(b)) {
        (Some(x), Some(y)) => between(text, x, y).map(Some),
        _ => Ok(None),
    };
    let quote = |g: &str| g.contains(['"', '״', '“', '”', '„']);
    let mut out = Vec::new();
    // Token ranges of the names found, and whether they may be hidden wherever they recur.
    let mut names: Vec<(usize, usize, bool)> = Vec::new();
    // Positions of the institution words, so a name found once is also taken after them.
    let mut keywords: Vec<usize> = Vec::new();
    let mut i = 0;
    while let Some(tok) = tokens.get(i) {
        // The institution word(s): "גן", "מעון", "בית ספר", "בית הספר", "בי\"ס", "תלמוד תורה".
        let next_is = |words: &[&str]| {
            tokens
                .get(i + 1)
                .is_some_and(|t| words.iter().any(|w| norm_eq(w, &t.norm)))
        };
        // `broad`: clinics, institutes and the like, whose descriptions are often ordinary
        // nouns ("מכון שמיעה"), so an unquoted name must not be an everyday word.
        let (end_word, broad) = if is(
            tok,
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
            (Some(i), false)
        } else if is(tok, &["בית", "תלמוד"]) && next_is(&["ספר", "הספר", "תורה"])
        {
            (Some(i + 1), false)
        } else if (is(tok, &["בית"]) && next_is(&["חולים", "החולים", "מרקחת", "המרקחת"]))
            || (is(tok, &["מרכז", "המרכז"]) && next_is(&["רפואי", "הרפואי"]))
            || (is(tok, &["מרפאת"]) && next_is(&["ילדים", "הילדים"]))
            || (is(tok, &["קופת"]) && next_is(&["חולים"]))
        {
            (Some(i + 1), true)
        } else if is(
            tok,
            &[
                "מכון",
                "מרכז",
                "מרפאת",
                "מרפאה",
                "קונסרבטוריון",
                "עמותת",
                "ישיבת",
                "אולפנת",
                "תיכון",
                "בניין",
                "ביה\"ח",
                "קופ\"ח",
                "משתלת",
                "חנות",
                "מסעדת",
                "מאפיית",
                "מספרת",
                "סטודיו",
                "חברת",
                "מפעל",
            ],
        ) {
            (Some(i), true)
        } else {
            (None, false)
        };
        // "מספרת" is a hair salon and also "tells": "הגננת מספרת שבמפגש…" dropped the word
        // after it. The verb has a speaker before it or a clause after it.
        let end_word = end_word.filter(|_| !(tok.norm == "מספרת" && telling(text, tokens, i)));
        let Some(k) = end_word else {
            i += 1;
            continue;
        };
        keywords.push(k);
        // A health fund is one of four names, also after a colon ("קופת חולים: כללית").
        let fund = is(tok, &["קופת", "קופ\"ח"]);
        // The institution word was read above (`i`, or the word after it).
        let keyword = tokens.get(k).ok_or_else(out_of_range)?;
        let mut first = k + 1;
        // "]": the institution word is inside a tag ("[ילד_גן] הופנה", "[גן] שלו"), so the next
        // word is not its name. "[": the next word opens a tag (`לגן "[ילד]"`), already hidden.
        // Without these, filtering a tagged draft again dropped that word or cut the tag.
        let Some(first_tok) = tokens.get(first) else {
            i = k + 1;
            continue;
        };
        let g = between(text, keyword, first_tok)?;
        if g.contains(['.', ',', '\n', ';', '(', ']', '[']) || (g.contains(':') && !fund) {
            i = k + 1;
            continue;
        }
        if fund && HEALTH_FUNDS.iter().any(|f| norm_eq(f, &first_tok.norm)) {
            // Left out, like a kindergarten's name: "קופ\"ח מכבי, מס' חבר" → "קופ\"ח, מס' חבר".
            out.push(Replacement {
                start: keyword.end,
                end: first_tok.end,
                out: String::new(),
                label: "קופת חולים".to_owned(),
                relative: true,
                declared: false,
            });
            names.push((first, first, true));
            i = first + 1;
            continue;
        }
        // A letterhead: the institution word starts the line and its name runs up to a
        // dash, a bar or the end of the line ("מכון צמיחה – היחידה להתפתחות הילד",
        // "מרפאת ילדים אופק | רחוב…"). There even an ordinary word is the name.
        if broad && starts_line(text, tok.start) {
            if let Some((last, last_end)) = letterhead_name(text, tokens, first)? {
                out.push(Replacement {
                    start: keyword.end,
                    end: last_end,
                    out: String::new(),
                    label: "שם מוסד".to_owned(),
                    relative: true,
                    declared: false,
                });
                names.push((first, last, true));
                i = last + 1;
                continue;
            }
        }
        // "ע\"ש רבין": whatever follows "named after" is a name.
        let named_after = first_tok.norm == normalize("ע\"ש") || first_tok.norm == normalize("עש");
        if named_after {
            first += 1;
        }
        let opened = gap(first - 1, first)?.is_some_and(quote);
        // The last word of the name, and where it ends.
        let mut last: Option<(usize, usize)> = None;
        if opened {
            // Up to the closing quote, at most four words.
            for (j, t) in tokens.iter().enumerate().skip(first).take(4) {
                if !hebrew(t) {
                    break;
                }
                let after = match tokens.get(j + 1) {
                    Some(n) => between(text, t, n)?,
                    None => span_text(text, t.end, text.len())?,
                };
                // "המוסד" is what a recurring name became (`לגן "המוסד"`): not a name.
                if j == first && norm_eq(INSTITUTION_WORD, &t.norm) {
                    break;
                }
                // A tag inside the quotes (a tagged draft, `לגן " [ילד] "`): not a name, as
                // on the first filtering, where the declared name took that span. A tag after
                // the closing quote (`לגן "ניצנים" [ילד]`) leaves the name as it is.
                let closing = after.find(['"', '״', '“', '”', '„']);
                if after
                    .find('[')
                    .is_some_and(|b| closing.is_none_or(|q| b < q))
                {
                    break;
                }
                if quote(after) {
                    last = Some((j, t.end));
                    break;
                }
                if after.contains(['.', ',', '\n']) {
                    break;
                }
            }
        } else if let Some(t) = tokens.get(first) {
            let stop = INSTITUTION_STOP.iter().any(|w| norm_eq(w, &t.norm));
            let everyday = |w: &str| crate::lexicon::word_bucket(w) >= EVERYDAY;
            // Without its first letter (all of it when it has one letter).
            let stem = t
                .norm
                .char_indices()
                .nth(1)
                .and_then(|(b, _)| t.norm.get(b..))
                .unwrap_or(&t.norm);
            // "גן הדקל": the word without the article is at least as common as with it; not
            // so for a verb ("בגן הוכנסו התאמות").
            let definite_name = t.norm.starts_with('ה')
                && t.norm.chars().count() >= 4
                && word_bucket(stem) >= word_bucket(&t.norm)
                && !(broad && everyday(stem));
            let known = lex.first_names.contains(&t.norm)
                || lex.surnames.contains(&t.norm)
                || !lex.places.find(&t.norm, &tokenize(&t.norm)).is_empty();
            let rare_name = broad
                && !everyday(&t.norm)
                && !inflects(&t.norm)
                && !t.norm.starts_with(['ל', 'ב', 'כ'])
                && !t.norm.contains('"');
            if hebrew(t)
                && !stop
                && !lex.common_words.contains(&t.norm)
                && (named_after || definite_name || known || rare_name)
            {
                last = Some((first, t.end));
                // "גן הדקל הירוק"? One more word only after "ע\"ש" ("ע\"ש יצחק רבין").
                if let Some(n) = tokens.get(first + 1) {
                    if named_after
                        && between(text, t, n)? == " "
                        && hebrew(n)
                        && lex.surnames.contains(&n.norm)
                    {
                        last = Some((first + 1, n.end));
                    }
                }
            }
        }
        if let Some((last, mut end)) = last {
            if opened {
                // Take the closing quote too, also after a space (`לגן " ניצנים "`): left
                // behind, it went out on its own (`לגן "`).
                let rest = span_text(text, end, text.len())?;
                let blank = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                if let Some(c) = rest
                    .get(blank..)
                    .and_then(|r| r.chars().next())
                    .filter(|c| ['"', '״', '“', '”'].contains(c))
                {
                    end += blank + c.len_utf8();
                }
            }
            out.push(Replacement {
                start: keyword.end,
                end,
                out: String::new(),
                label: "שם גן / בית ספר".to_owned(),
                relative: true,
                declared: false,
            });
            // A quoted name, a name of two words or an unusual word is the name wherever
            // it recurs; a single ordinary word ("גן פרפר") only after an institution word.
            // (`first` is a word of the name here: `last` is at or after it.)
            let strong = opened
                || last > first
                || tokens
                    .get(first)
                    .is_none_or(|t| crate::lexicon::word_bucket(&t.norm) < EVERYDAY);
            names.push((first, last, strong));
            i = last + 1;
        } else {
            i = k + 1;
        }
    }
    recurring_institutions(text, tokens, &names, &keywords, &mut out)?;
    Ok(out)
}

/// The four health funds; after "קופ\"ח" or "קופת חולים" one of them is the member's fund.
const HEALTH_FUNDS: &[&str] = &["כללית", "מכבי", "מאוחדת", "לאומית"];

/// A recurring institution name with no institution word before it becomes this description,
/// not a tag: nothing to put back, and nothing that blocks the export (as in D-034).
const INSTITUTION_WORD: &str = "המוסד";

/// What an institution is for, after "ל": "מרכז להתפתחות הילד", "מכון לטיפול בשפה".
const DESCRIPTION_HEADS: &[&str] = &[
    "טיפול",
    "התפתחות",
    "בריאות",
    "ילדים",
    "אבחון",
    "נוער",
    "שיקום",
    "חינוך",
    "הורים",
    "משפחה",
    "הפרעות",
    "לקויות",
    "ליקויי",
    "גיל",
    "שמיעה",
    "ראייה",
    "ויסות",
    "הערכה",
    "העשרה",
    "קשב",
    "למידה",
    "תקשורת",
    "שפה",
    "רפואת",
    "פסיכולוגיה",
    "גנטיקה",
    "נוירולוגיה",
];

/// The name in a letterhead line: one to three words after the institution word, ending at
/// a dash, a bar, an opening parenthesis or the end of the line. Not a description: the first
/// word has no article and is not "ל" + a purpose ("מרכז להתפתחות הילד"); no function words.
/// Later words may be in construct ("המרכז הרפואי גבעות החוף"). Returns the last word of
/// the name and where it ends.
fn letterhead_name(
    text: &str,
    tokens: &[Token],
    first: usize,
) -> Result<Option<(usize, usize)>, PrivacyError> {
    let lex = &*LEXICON;
    let mut last = None;
    for (j, t) in tokens.iter().enumerate().skip(first).take(3) {
        let hebrew = t
            .norm
            .chars()
            .all(|c| ('\u{05D0}'..='\u{05EA}').contains(&c));
        let described = prefix_splits(&t.norm).iter().any(|(p, h)| {
            *p == 1
                && t.norm.starts_with('ל')
                && (DESCRIPTION_HEADS.iter().any(|d| norm_eq(d, h))
                    || (h.starts_with('ה') && word_bucket(h) >= EVERYDAY))
        });
        if !hebrew
            || (j == first && (t.norm.starts_with('ה') || described))
            || word_bucket(&t.norm) >= 14
            || lex.common_words.contains(&t.norm)
            || INSTITUTION_STOP.iter().any(|w| norm_eq(w, &t.norm))
        {
            return Ok(None);
        }
        let after = match tokens.get(j + 1) {
            Some(n) => between(text, t, n)?,
            None => span_text(text, t.end, text.len())?,
        };
        if after.contains(['–', '—', '|', '(', '\n'])
            || after.contains(" - ")
            || j + 1 == tokens.len()
        {
            last = Some((j, t.end));
            break;
        }
        if after != " " {
            return Ok(None);
        }
    }
    Ok(last)
}

/// A name found once ("מכון \"צעדים קטנים\"") is hidden where it comes back
/// ("מ\"צעדים קטנים\"", "ממכון צמיחה").
fn recurring_institutions(
    text: &str,
    tokens: &[Token],
    names: &[(usize, usize, bool)],
    keywords: &[usize],
    out: &mut Vec<Replacement>,
) -> Result<(), PrivacyError> {
    let quotes = ['"', '״', '“', '”', '„'];
    for (first, last, strong) in names {
        // The name's words, as found in this text just before.
        let words: Vec<&str> = tokens
            .get(*first..=*last)
            .ok_or_else(out_of_range)?
            .iter()
            .map(|t| t.norm.as_str())
            .collect();
        let Some(&first_word) = words.first() else {
            return Err(out_of_range());
        };
        // Every run of as many words as the name.
        for (j, window) in tokens.windows(words.len()).enumerate() {
            let [t, rest @ ..] = window else {
                return Err(out_of_range());
            };
            // The first word may carry a prefix and an opening quote glued on (מ"צעדים).
            let lead = prefix_splits(&t.norm)
                .into_iter()
                .find(|(_, h)| h.trim_start_matches(quotes) == first_word);
            let Some((p, _)) = lead else { continue };
            let raw = span_text(text, t.start, t.end)?;
            let pre = crate::text::byte_len_of_letters(raw, p);
            let after_pre = span_text(raw, pre, raw.len())?;
            let lead = pre + (after_pre.len() - after_pre.trim_start_matches(quotes).len());
            // Each later word in turn, with the word before it.
            let mut rest_matches = true;
            for (w, pair) in words.iter().skip(1).zip(window.windows(2)) {
                let matched = match pair {
                    [prev, cur] => cur.norm == *w && between(text, prev, cur)? == " ",
                    _ => false,
                };
                if !matched {
                    rest_matches = false;
                    break;
                }
            }
            let start = t.start + lead;
            let end = rest.last().unwrap_or(t).end;
            // The institution word right before, when the name has no prefix of its own.
            let keyword_before = j
                .checked_sub(1)
                .filter(|k| lead == 0 && keywords.contains(k))
                .and_then(|k| tokens.get(k));
            let after_keyword = keyword_before.is_some();
            if rest_matches
                && (*strong || after_keyword)
                && !out.iter().any(|r| r.start < end && start < r.end)
            {
                // After "ממכון" the name is just left out; elsewhere it reads "המוסד".
                let (start, text_out) = match keyword_before {
                    Some(keyword) => (keyword.end, String::new()),
                    None => (start, INSTITUTION_WORD.to_owned()),
                };
                out.push(Replacement {
                    start,
                    end,
                    out: text_out,
                    label: "שם מוסד".to_owned(),
                    relative: true,
                    declared: false,
                });
            }
        }
    }
    Ok(())
}

/// A one-word place that is read as an ordinary word here. A place takes no article and
/// no "ש" / "כ" glued on ("שאלונים", "כשמכבים", "הגדרה"); after "ב/מ/ל" it is a word only
/// when that form is far more common than the place ("מדברת", "בחורה"). A place that is
/// itself an everyday word ("קדימה", "אזור", "מעלות") counts only after "ב/מ/ל" when the
/// prefixed form is rarer than the bare word ("במעלות", not "באזור"), or after a word for a
/// place ("מושב אלונים", "גרים"). Anything else stays hidden.
fn place_reads_as_word(text: &str, tokens: &[Token], i: usize, prefix: usize) -> bool {
    // No token at `i`: not shown to be a word, so the place stays hidden.
    let Some(whole) = tokens.get(i) else {
        return false;
    };
    let letters: Vec<char> = whole.norm.chars().collect();
    let head: String = letters.iter().skip(prefix).collect();
    let glued: String = letters.iter().take(prefix).collect();
    let place_word = i
        .checked_sub(1)
        .and_then(|j| tokens.get(j))
        // A gap that cannot be read (never expected) counts as none: the place stays hidden.
        .filter(|t| {
            text.get(t.end..whole.start)
                .is_none_or(|g| g.trim().is_empty())
        })
        .is_some_and(|t| {
            [
                "יישוב",
                "היישוב",
                "קיבוץ",
                "מושב",
                "עיר",
                "העיר",
                "בעיר",
                "כפר",
                "גרים",
                "גרה",
                "גר",
                "מתגוררים",
                "תושב",
                "תושבת",
                "ליד",
                "עברו",
            ]
            .iter()
            .any(|w| norm_eq(w, &t.norm))
        });
    if place_word {
        return false;
    }
    let hb = word_bucket(&head);
    if prefix > 0 {
        let preposition = glued
            .strip_prefix('ו')
            .unwrap_or(&glued)
            .trim_start_matches('ש')
            .chars()
            .collect::<Vec<_>>();
        if !matches!(preposition.as_slice(), ['ב'] | ['מ'] | ['ל']) {
            return true;
        }
        // "ומדברת": the form without the conjunction.
        let form = whole.norm.strip_prefix('ו').unwrap_or(&whole.norm);
        let w = word_bucket(form).max(word_bucket(&whole.norm));
        if w >= EVERYDAY && w >= hb + 3 {
            return true;
        }
    }
    if !PLACE_WORDS.iter().any(|p| norm_eq(p, &head)) {
        return false;
    }
    prefix == 0 || word_bucket(&whole.norm) >= hb
}

/// Localities whose name is first of all an everyday word in a report.
const PLACE_WORDS: &[&str] = &[
    "קדימה",
    "מעלות",
    "אזור",
    "נשר",
    "מודיעין",
    "טירה",
    "מבטחים",
    "אמונים",
    "שריד",
    "מכבים",
    "מיתר",
    "כליל",
    "ישע",
    "יבול",
    "אופקים",
    "קדומים",
    "אלונים",
    "אשלים",
];

/// Layers 2–6: what to replace and what to ask about, as spans of `text`, sorted.
fn analyze(
    text: &str,
    ctx: &PrivacyContext<'_>,
    hints: &[NameHint],
) -> Result<(Vec<Replacement>, Vec<AutoSpan>), PrivacyError> {
    let tokens = tokenize(text);
    let mut reps: Vec<Replacement> = Vec::new();
    let mut suspects: Vec<SuspectSpan> = Vec::new();

    // Layer 2: declared identities (every case; another case's name is a suspect, not a tag).
    declared_matches(text, &tokens, ctx, &mut reps, &mut suspects)?;
    // The same text with every bidirectional control as a word break (D-132): a name a
    // direction mark glued to the next word ("נועם\u{200F}הלך") is found this way.
    if crate::text::has_bidi_control(text) {
        let split = crate::text::tokenize_split_bidi(text);
        declared_matches(text, &split, ctx, &mut reps, &mut suspects)?;
    }
    // A declared name inside a longer word ("עידוש", "נועם1", "מיכלוש", D-132): the whole
    // word is hidden, as "שאלון" is, and listed so "להחזיר" can keep it.
    declared_inside_tokens(text, &tokens, ctx, &mut reps, &mut suspects)?;
    absorb_titles(text, &tokens, ctx, &mut reps);
    declared_arabic(text, &tokens, ctx, &mut reps, &mut suspects)?;
    layer_4_and_on(text, &tokens, ctx, hints, reps, suspects)
}

/// Every declared name matched on `tokens` (one view of the text), as a replacement of this
/// case, a suspect of another case, or the word it also is; a span already taken is skipped.
fn declared_matches(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &mut Vec<Replacement>,
    suspects: &mut Vec<SuspectSpan>,
) -> Result<(), PrivacyError> {
    for m in identity_index(ctx).find(text, tokens) {
        if overlaps(m.start, m.end, reps, suspects) {
            continue;
        }
        if declared_reads_as_word(text, tokens, m.first_token, m.last_token, m.prefix) {
            continue;
        }
        if matches!(&m.payload, Target::Identity { case_id, .. } if *case_id != ctx.case_id)
            && other_case_word(text, tokens, ctx, m.first_token, m.last_token, m.prefix)
        {
            continue;
        }
        // The match was found on these tokens.
        let whole = tokens.get(m.first_token).ok_or_else(out_of_range)?;
        if m.prefix > 0
            && LEXICON.common_words.contains(&whole.norm)
            && !(ctx.confirmed_names)(&whole.norm)
        {
            // "שאלון" = ש + אלון: hidden as the name (fail-closed) and listed first on the
            // card, where "להחזיר" keeps the word; only then does the gate let it out.
            if let (false, Target::Identity { case_id, tag, .. }) =
                ((ctx.allowlisted)(&whole.norm), &m.payload)
            {
                if *case_id == ctx.case_id {
                    suspects.push(SuspectSpan {
                        start: m.start,
                        end: m.end,
                        suspect: Suspect {
                            token: span_text(text, whole.start, whole.end)?.to_owned(),
                            kind: SuspectKind::AmbiguousWord,
                            message: INSIDE_THIS_CASE.to_owned(),
                            suggested_role: Role::Other,
                        },
                        known: Some(tag.clone()),
                    });
                    continue;
                }
            }
            if !(ctx.allowlisted)(&whole.norm) {
                suspects.push(SuspectSpan {
                    start: whole.start,
                    end: whole.end,
                    suspect: Suspect {
                        token: span_text(text, whole.start, whole.end)?.to_owned(),
                        kind: SuspectKind::OtherCaseIdentity,
                        message: INSIDE_OTHER_CASE.to_owned(),
                        suggested_role: Role::Other,
                    },
                    known: None,
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
                    token: span_text(text, m.start, m.end)?.to_owned(),
                    kind: SuspectKind::OtherCaseIdentity,
                    message: "שם שמופיע בתיק אחר".to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
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
    Ok(())
}

/// Shortest declared name the containment rule reads: shorter names ("דנה", "לוי") sit
/// inside too many ordinary words.
const INSIDE_MIN_LETTERS: usize = 4;
/// What the card says of a word that holds a name of this case, of another case, or of
/// the practitioner (D-132). A pattern that holds such a word is hidden whole instead.
const INSIDE_THIS_CASE: &str = "שם מהתיק בתוך מילה";
const INSIDE_OTHER_CASE: &str = "שם מתיק אחר בתוך מילה";
const INSIDE_PRACTITIONER: &str = "שם המאבחנת בתוך מילה";

fn inside_word_suspect(s: &SuspectSpan) -> bool {
    s.suspect.message == INSIDE_THIS_CASE || s.suspect.message == INSIDE_OTHER_CASE
}

/// The declared name a whole word holds inside it, when the word is no everyday word and
/// no name by itself: "עידוש", "נועם1", "מיכלוש", "שטרנים", "noam123" (D-132). Only words
/// of four letters or more that are not everyday words themselves ("הילה" is a word), of
/// this case first. The pipeline hides the whole word, and the gate refuses it, by this one
/// function, so what leaves the filter passes the gate.
pub(crate) fn declared_inside(ctx: &PrivacyContext<'_>, norm: &str) -> Option<Target> {
    // An everyday word, also with prefix letters before it ("ובשאלון" is ו + ב + "שאלון")
    // and with an ending after it ("שאלונים", "שאלוני"): the word, not the name.
    let everyday = |w: &str| LEXICON.common_words.contains(w) || word_bucket(w) >= EVERYDAY;
    let stems = |w: &str| {
        let mut out = vec![w.to_owned()];
        for ending in [
            "ימ", "ות", "יות", "י", "ה", "ת", "נו", "כמ", "יו", "יה", "ו", "ית",
        ] {
            if let Some(stem) = w.strip_suffix(ending) {
                if stem.chars().count() >= 3 {
                    out.push(stem.to_owned());
                }
            }
        }
        out
    };
    // Letters, not marks or punctuation: "ג'וד" is three letters, "ג'ודו" is judo.
    let letters = |w: &str| w.chars().filter(|c| c.is_alphabetic()).count();
    if letters(norm) < INSIDE_MIN_LETTERS
        || prefix_splits(norm)
            .iter()
            .any(|(_, h)| stems(h).iter().any(|s| everyday(s)))
        || (ctx.allowlisted)(norm)
    {
        return None;
    }
    // A name she typed is a name also where it is a word ("מיכל", "עידו"): "מיכלוש" holds
    // it. A name the filter found by its context is held only when it is no word, as
    // `found_word` reads it.
    // The name starts the word after its prefix letters ("עידוש", "ולנועמים", "נועם'ס",
    // "noam123") or ends it ("ננועם", "123noam"): a name in the middle of a rarer word
    // ("התיאורים" holds "אורי") is that word.
    let at_edge = |w: &str, typed: bool| {
        // Before a name that ends the word: digits, a letter or two of a typo (only for a
        // name she typed: "Score" ends in an English word the filter once took for a
        // name), or a generic place word ("בכפרורדים"), not the body of a longer word
        // ("מסולסלים" ends in "סלים").
        let short_before = |before: &str| {
            before.chars().count() <= 3 && (typed || before.chars().all(|c| !c.is_alphabetic()))
                || prefix_splits(before).iter().any(|(_, h)| is_generic(h))
        };
        // After a name that starts the word: an ending, a typo, "'ס", or glued digits and
        // letters of another script ("noam123", "נועם2019"), not the body of a longer word
        // ("קוגניטיבי" starts with "קוגן").
        let short_after = |after: &str| {
            after.chars().count() <= 3
                || after.chars().all(|c| {
                    !c.is_alphabetic()
                        || c.is_ascii_alphabetic() != w.chars().any(|n| n.is_ascii_alphabetic())
                })
        };
        norm.strip_suffix(w).is_some_and(short_before)
            || prefix_splits(norm)
                .iter()
                .any(|(_, h)| h.strip_prefix(w).is_some_and(short_after))
    };
    let holds = |name: &str, typed: bool| {
        normalize(name).split(' ').any(|w| {
            letters(w) >= INSIDE_MIN_LETTERS
                && !is_title(w)
                && !is_generic(w)
                && (typed || !everyday_name(w))
                && norm != w
                && at_edge(w, typed)
        })
    };
    let mut ordered: Vec<&Identity> = ctx
        .identities
        .iter()
        .filter(|i| i.case_id == ctx.case_id)
        .collect();
    ordered.extend(ctx.identities.iter().filter(|i| i.case_id != ctx.case_id));
    for identity in ordered {
        let typed = identity.source == IdentitySource::Manual;
        if std::iter::once(&identity.value)
            .chain(identity.aliases.iter())
            .any(|n| holds(n, typed))
        {
            return Some(Target::Identity {
                case_id: identity.case_id.clone(),
                tag: identity.tag.clone(),
                role: identity.role,
            });
        }
    }
    ctx.practitioner
        .iter()
        .any(|n| holds(n, true))
        .then_some(Target::Practitioner)
}

/// Every word that holds a declared name ([`declared_inside`]): hidden whole, under the
/// name's tag for this case (listed as a name inside a word, with "להחזיר"), as another
/// case's name otherwise.
fn declared_inside_tokens(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &mut Vec<Replacement>,
    suspects: &mut Vec<SuspectSpan>,
) -> Result<(), PrivacyError> {
    for tok in tokens {
        if overlaps(tok.start, tok.end, reps, suspects) {
            continue;
        }
        let Some(target) = declared_inside(ctx, &tok.norm) else {
            continue;
        };
        let token = span_text(text, tok.start, tok.end)?.to_owned();
        match target {
            Target::Identity { case_id, tag, .. } if case_id == ctx.case_id => {
                suspects.push(SuspectSpan {
                    start: tok.start,
                    end: tok.end,
                    suspect: Suspect {
                        token,
                        kind: SuspectKind::AmbiguousWord,
                        message: INSIDE_THIS_CASE.to_owned(),
                        suggested_role: Role::Other,
                    },
                    known: Some(tag),
                });
            }
            Target::Identity { .. } => suspects.push(SuspectSpan {
                start: tok.start,
                end: tok.end,
                suspect: Suspect {
                    token,
                    kind: SuspectKind::OtherCaseIdentity,
                    message: INSIDE_OTHER_CASE.to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            }),
            Target::Practitioner => reps.push(Replacement {
                start: tok.start,
                end: tok.end,
                out: PRACTITIONER_TAG.to_owned(),
                label: INSIDE_PRACTITIONER.to_owned(),
                relative: false,
                declared: true,
            }),
        }
    }
    Ok(())
}

/// A title right before part of a titled identity joins the replacement: "ד\"ר שטרן" for
/// "ד\"ר אבנר שטרן" is all "[רופא_1]", so the name comes back once, not "ד\"ר ד\"ר אבנר
/// שטרן" (D-132). Only when the identity's own name starts with a title, and the matched
/// words do not already include it.
fn absorb_titles(text: &str, tokens: &[Token], ctx: &PrivacyContext<'_>, reps: &mut [Replacement]) {
    for r in reps.iter_mut().filter(|r| r.declared) {
        let titled = ctx.identities.iter().any(|i| {
            i.case_id == ctx.case_id
                && i.tag == r.out
                && normalize(&i.value).split(' ').next().is_some_and(is_title)
        });
        if !titled
            || text
                .get(r.start..r.end)
                .is_some_and(|m| tokenize(m).first().is_some_and(|t| is_title(&t.norm)))
        {
            continue;
        }
        let before = tokens
            .iter()
            .rev()
            .find(|t| t.end <= r.start)
            .filter(|t| is_title(&t.norm))
            .filter(|t| {
                text.get(t.end..r.start)
                    .is_some_and(|g| g.chars().all(|c| c == ' ' || c == '\u{00A0}'))
            });
        if let Some(t) = before {
            r.start = t.start;
        }
    }
}

/// Layer 2, Arabic script: a declared name spelled in Arabic ("روضة جسر").
fn declared_arabic(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &mut Vec<Replacement>,
    suspects: &mut Vec<SuspectSpan>,
) -> Result<(), PrivacyError> {
    for (start, end, target) in arabic_identity_matches(text, tokens, ctx) {
        if overlaps(start, end, reps, suspects) {
            continue;
        }
        match target {
            Target::Identity { case_id, tag, role } if case_id == ctx.case_id => {
                reps.push(Replacement {
                    start,
                    end,
                    out: tag,
                    label: role_label(role),
                    relative: false,
                    declared: true,
                });
            }
            Target::Identity { .. } => suspects.push(SuspectSpan {
                start,
                end,
                suspect: Suspect {
                    token: span_text(text, start, end)?.to_owned(),
                    kind: SuspectKind::OtherCaseIdentity,
                    message: "שם שמופיע בתיק אחר".to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            }),
            Target::Practitioner => reps.push(Replacement {
                start,
                end,
                out: PRACTITIONER_TAG.to_owned(),
                label: "שם המאבחנת".to_owned(),
                relative: false,
                declared: true,
            }),
        }
    }
    Ok(())
}

/// Layers 4 and on (patterns, the lexicons, hints, auto-hiding), on what layer 2 left.
fn layer_4_and_on(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    hints: &[NameHint],
    mut reps: Vec<Replacement>,
    mut suspects: Vec<SuspectSpan>,
) -> Result<(Vec<Replacement>, Vec<AutoSpan>), PrivacyError> {
    // Layer 4: patterns, read with the tags already in the text blanked: "[מספר_זהות_1] מרץ"
    // was read as "1 במרץ" and cut the tag.
    // The words already replaced (a declared person, the practitioner) are blanked too: a
    // sister named "מאי" next to a date ("12.5.2020 (מאי)") is not its month, and a date
    // joined over her name was dropped whole and reached the gate (D-128, third review).
    let unblanked = patterns::blank_tags(text, |t| protected_tag(t, ctx));
    let mut blanked = unblanked.clone().into_owned();
    // The people whose name is also a month ("מאי", "יוני").
    let month_named: Vec<(usize, usize)> = reps
        .iter()
        .filter(|r| {
            span_text(text, r.start, r.end)
                .is_ok_and(|w| patterns::month_word(&normalize(w)).is_some())
        })
        .map(|r| (r.start, r.end))
        .collect();
    for r in &reps {
        if let Some(len) = blanked.get(r.start..r.end).map(str::len) {
            blanked.replace_range(r.start..r.end, &"x".repeat(len));
        }
    }
    for hit in
        patterns::find(&blanked, ctx.today).map_err(|e| PrivacyError::Internal(e.to_string()))?
    {
        // A month the name rules took for a name ("1 יוני 2015", "ביולי 2020") is the date's
        // (D-128): the whole date is hidden instead, so neither the day nor the year is left
        // beside it. Only when nothing else is in the way, so the name is never dropped
        // without the date that hides it.
        let month_suspect = |s: &SuspectSpan| {
            s.known.is_none()
                && hit.start <= s.start
                && s.end <= hit.end
                && span_text(text, s.start, s.end)
                    .is_ok_and(|w| patterns::month_word(&normalize(w)).is_some())
        };
        if hit.kind == patterns::PatternKind::Date
            && !overlaps(hit.start, hit.end, &reps, &[])
            && suspects
                .iter()
                .filter(|s| hit.start < s.end && s.start < hit.end)
                .all(month_suspect)
        {
            suspects.retain(|s| !month_suspect(s));
        }
        // A pattern that holds a word with a name inside it ("svetak19@example.com", with
        // "sveta" declared) is hidden whole: the word alone would leave the rest of the
        // address beside its tag, and the gate would refuse it (D-132).
        let within = |s: usize, e: usize| hit.start <= s && e <= hit.end;
        let touching = |s: usize, e: usize| hit.start < e && s < hit.end;
        if suspects
            .iter()
            .filter(|s| touching(s.start, s.end))
            .all(|s| inside_word_suspect(s) && within(s.start, s.end))
            && reps
                .iter()
                .filter(|r| touching(r.start, r.end))
                .all(|r| r.label == INSIDE_PRACTITIONER && within(r.start, r.end))
        {
            suspects.retain(|s| !touching(s.start, s.end));
            reps.retain(|r| !touching(r.start, r.end));
        }
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
    // Read again with those names in place: where one of them stands in a date as its month
    // ("20 מאי 15", a sister named מאי), the rest of that date is hidden too, so no day or
    // year of it is left beside her name. The strict reading: it may be her name, it may be
    // the month.
    if !month_named.is_empty() {
        let found = patterns::find(&unblanked, ctx.today)
            .map_err(|e| PrivacyError::Internal(e.to_string()))?;
        let mut more = Vec::new();
        for hit in found
            .iter()
            .filter(|h| h.kind == patterns::PatternKind::Date)
        {
            let within = |s: usize, e: usize| hit.start < e && s < hit.end;
            if !month_named.iter().any(|&(s, e)| within(s, e)) {
                continue;
            }
            // The date without what is already hidden or in question in it: the names, and
            // the year the first reading took ("מאי 3, 2020" left the "3" when the piece
            // " 3, 2020" was skipped whole for the year in it, fourth review of D-128).
            let mut taken: Vec<(usize, usize)> = month_named
                .iter()
                .copied()
                .chain(reps.iter().map(|r| (r.start, r.end)))
                .chain(suspects.iter().map(|s| (s.start, s.end)))
                .chain(more.iter().map(|r: &Replacement| (r.start, r.end)))
                .filter(|&(s, e)| within(s, e))
                .collect();
            taken.sort_unstable();
            let mut pieces = Vec::new();
            let mut pos = hit.start;
            for (s, e) in taken {
                if s > pos {
                    pieces.push((pos, s));
                }
                pos = pos.max(e);
            }
            if pos < hit.end {
                pieces.push((pos, hit.end));
            }
            for (s, e) in pieces {
                let Ok(piece) = span_text(text, s, e) else {
                    continue;
                };
                // Without the spaces and marks around it.
                let lead = piece.len()
                    - piece
                        .trim_start_matches(|c: char| !c.is_alphanumeric())
                        .len();
                let body = piece.trim_matches(|c: char| !c.is_alphanumeric());
                let (start, end) = (s + lead, s + lead + body.len());
                // Only a day or a year: "ממאי" (from a sister named מאי) leaves the letter
                // "מ", which is no part of a date, and hiding it read "[תאריך][אחות_2]".
                let dated = body.chars().any(|c| c.is_ascii_digit())
                    || piece.contains(['\'', '"', '\u{05F3}', '\u{05F4}']);
                if dated
                    && !overlaps(start, end, &reps, &suspects)
                    && !overlaps(start, end, &more, &[])
                {
                    more.push(Replacement {
                        start,
                        end,
                        relative: false,
                        out: patterns::PatternKind::Date.fixed_replacement().to_owned(),
                        label: patterns::PatternKind::Date.label_he().to_owned(),
                        declared: false,
                    });
                }
            }
        }
        reps.extend(more);
    }
    // An address field: whatever follows "כתובת:" on its line is the address, with or without
    // the word "רחוב" ("כתובת: מסדה 33, חיפה").
    for r in address_fields(text) {
        if !overlaps(r.start, r.end, &reps, &suspects) {
            reps.push(r);
        }
    }
    // Names of kindergartens and schools: only the name is left out ("בגן השקד שבשכונה" →
    // "בגן שבשכונה"), so the sentence still reads and nothing needs to come back.
    // A name never takes part of a tag already in the text: "גן ע\"ש [טלפון_1] מרץ" (a
    // stored paragraph sent again) lost 'ע"ש [טלפון' and went out as "גן_1] מרץ", read as
    // the first of March (found by the long property runs of D-130). The name ends before the tag.
    let tags = protected_tags(text, ctx);
    for mut r in institution_names(text, tokens)? {
        if let Some(cut) = tags
            .iter()
            .filter(|t| t.start < r.end && r.start < t.end)
            .map(|t| t.start)
            .min()
        {
            let kept = text
                .get(r.start..cut.max(r.start))
                .map_or(0, |n| n.trim_end().len());
            r.end = r.start + kept;
        }
        if r.start < r.end && !overlaps(r.start, r.end, &reps, &suspects) {
            reps.push(r);
        }
    }
    // Places from the lexicon (localities, hospitals) are hidden automatically.
    for m in LEXICON.places.find(text, tokens) {
        if m.first_token == m.last_token
            && place_reads_as_word(text, tokens, m.first_token, m.prefix)
        {
            continue;
        }
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
    // Where a child was born or came from ("נולד בחרקוב"), when it is not an everyday word.
    for r in context::origin_places(text, tokens, &reps, &suspects)? {
        reps.push(r);
    }
    // Layers 3, 5, 6: suspects on what is left.
    suspects.extend(find_suspects(text, tokens, ctx, &reps)?);
    // Names that are also words, in a context that leaves no other reading (D-049).
    let strong = context::strong_names(text, tokens, ctx.allowlisted, &reps, &suspects)?;
    suspects.extend(strong);
    // The commonest words ("איתי", "שרה") where only a name can stand, where no rule decided.
    let common = common_names(text, tokens, ctx, &reps, &suspects)?;
    suspects.extend(common);
    // Layer 5b: what a local name model reads as a person, where no rule decided (stage 5).
    let hinted = hint_suspects(text, tokens, ctx, hints, &reps, &suspects)?;
    suspects.extend(hinted);
    // A name found once is the same name elsewhere in the text ("קוראים לה נוגה" … "נוגה
    // צחקה").
    let again = recurring_names(text, tokens, ctx, &reps, &suspects)?;
    suspects.extend(again);

    // Nothing is asked any more: every suspect is hidden, and the card lists it.
    let mut autos = auto_hide(text, tokens, ctx, &suspects, &mut reps)?;
    autos.extend(declared_auto(ctx, &reps));

    // A fixed point with the gate (D-050): what would go out is checked by the gate's own name
    // test, and whatever it would refuse is hidden here, on the card, with "להחזיר". So the
    // gate never refuses a name this filter let out, and she is never left at a dead end.
    for _ in 0..3 {
        reps.sort_by_key(|r| r.start);
        autos.sort_by_key(|a| a.start);
        let out = assemble(text, &reps, &autos)?;
        let found = crate::gate::name_findings(&out.tagged, ctx);
        let extra: Vec<SuspectSpan> = found
            .iter()
            .filter_map(|f| to_original(&out, f.start, f.end))
            .filter(|(s, e)| !overlaps(*s, *e, &reps, &[]))
            .map(|(start, end)| {
                Ok(SuspectSpan {
                    start,
                    end,
                    suspect: Suspect {
                        token: span_text(text, start, end)?.to_owned(),
                        kind: SuspectKind::OtherCaseIdentity,
                        message: "שם שהבדיקה האחרונה זיהתה בטקסט היוצא".to_owned(),
                        suggested_role: Role::Other,
                    },
                    known: None,
                })
            })
            .collect::<Result<_, PrivacyError>>()?;
        if extra.is_empty() {
            break;
        }
        autos.extend(auto_hide(text, tokens, ctx, &extra, &mut reps)?);
    }

    reps.sort_by_key(|r| r.start);
    autos.sort_by_key(|a| a.start);
    Ok((reps, autos))
}

/// A model's person spans as suspects, on whole words: a hint that starts inside a word keeps
/// only Hebrew prefix letters out ("ו" + "אסתי"), anything else widens to the word, and so
/// does a hint that ends inside one, so no part of a name is left showing. Titles and person
/// words at the edges ("ד\"ר", "הגננת") stay, as the rules handle them. A hint on text a rule
/// already hid or held, or on a word the psychologist kept, adds nothing.
fn hint_suspects(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    hints: &[NameHint],
    reps: &[Replacement],
    suspects: &[SuspectSpan],
) -> Result<Vec<SuspectSpan>, PrivacyError> {
    let mut out: Vec<SuspectSpan> = Vec::new();
    for h in hints {
        let inside: Vec<&Token> = tokens
            .iter()
            .filter(|t| t.start < h.end && h.start < t.end)
            .filter(|t| !is_title(&t.norm) && !is_person_word(&t.norm) && !is_role_word(&t.norm))
            .collect();
        let (Some(first), Some(last)) = (inside.first(), inside.last()) else {
            continue;
        };
        let lead = text
            .get(first.start..h.start.max(first.start))
            .unwrap_or("");
        let start = if !lead.is_empty() && crate::text::is_valid_prefix(lead) && h.start < first.end
        {
            h.start
        } else {
            first.start
        };
        let end = last.end;
        if start >= end
            || overlaps(start, end, reps, suspects)
            || overlaps(start, end, &[], &out)
            || inside.iter().any(|t| (ctx.allowlisted)(&t.norm))
        {
            continue;
        }
        let raw = span_text(text, start, end)?;
        let words = normalize(raw);
        if (ctx.allowlisted)(&words) {
            continue;
        }
        out.push(SuspectSpan {
            start,
            end,
            suspect: Suspect {
                token: raw.to_owned(),
                kind: SuspectKind::UnknownName,
                message: if h.score >= NAME_HINT_SURE {
                    HINT_SURE
                } else {
                    HINT_UNSURE
                }
                .to_owned(),
                suggested_role: Role::Other,
            },
            known: None,
        });
    }
    Ok(out)
}

/// A span of the outgoing text back to the original: within one segment kept as written, by
/// offset; a span that touches a tag or crosses segments takes the whole original pieces it
/// touches (fail-closed: more is hidden, never less).
fn to_original(out: &FilterOutcome, start: usize, end: usize) -> Option<(usize, usize)> {
    let (mut o, mut t) = (0usize, 0usize);
    let mut range: Option<(usize, usize)> = None;
    for (orig, tagged) in out.original_segments.iter().zip(&out.tagged_segments) {
        let (ol, tl) = (orig.text.len(), tagged.text.len());
        if t < end && start < t + tl {
            let piece = if orig.mark.is_none() && tagged.text == orig.text {
                let s = o + start.saturating_sub(t);
                let e = o + (end - t).min(tl);
                (s, e)
            } else {
                (o, o + ol)
            };
            range = Some(match range {
                Some((s, e)) => (s.min(piece.0), e.max(piece.1)),
                None => piece,
            });
        }
        o += ol;
        t += tl;
    }
    range.filter(|(s, e)| s < e)
}

/// The role a person word just before a name gives it, and that word: "הגננת אסתי",
/// "סבתא רבתא שמחה", "חבר מהגן, רועי". Up to three words back, within the sentence.
fn role_from_context(
    text: &str,
    tokens: &[Token],
    start: usize,
) -> Result<Option<(Role, String)>, PrivacyError> {
    const ROLES: &[(Role, &[&str])] = &[
        (Role::Teacher, &["גננת", "מחנכת", "מחנך"]),
        (Role::Assistant, &["סייעת", "סייע", "מטפלת"]),
        (Role::SchoolTeacher, &["מורה", "המורה"]),
        (Role::Slp, &["קלינאית"]),
        (
            Role::Therapist,
            &["מרפאה", "מרפא", "פיזיותרפיסטית", "פיזיותרפיסט", "מטפל"],
        ),
        (
            Role::Doctor,
            &[
                "רופא",
                "רופאה",
                "רופאת",
                "ד\"ר",
                "דר",
                "דוקטור",
                "פרופ'",
                "פרופסור",
            ],
        ),
        (Role::Psychologist, &["פסיכולוגית", "פסיכולוג"]),
        (
            Role::Professional,
            &[
                "יועצת",
                "יועץ",
                "מנהלת",
                "מנהל",
                "עובדת",
                "עו\"ס",
                "מדריך",
                "מדריכה",
                "רכזת",
                "מאבחנת",
                "מאבחן",
            ],
        ),
        (Role::Brother, &["אח", "אחיו", "אחיה", "אחי"]),
        (Role::Sister, &["אחות", "אחותו", "אחותה", "אחותי"]),
        (Role::Mother, &["אמא", "אמו", "אמה"]),
        (Role::Father, &["אבא", "אביו", "אביה"]),
        (
            Role::Relative,
            &[
                "סבא",
                "סבתא",
                "סבו",
                "סבתו",
                "דוד",
                "דודה",
                "דודו",
                "דודתו",
                "נכד",
                "נכדה",
                "אחיין",
                "אחיינית",
                "בבושקה",
                "סבתוש",
                "דדושקה",
                "ג'דו",
                "ג'דה",
                "סיתו",
                "תיתה",
                "בעלה",
                "אשתו",
                "גיס",
                "גיסה",
            ],
        ),
        (
            Role::OtherChild,
            &[
                "חבר",
                "חברה",
                "חברו",
                "חברתו",
                "חברתה",
                "ילד",
                "ילדה",
                "תאום",
                "תאומה",
            ],
        ),
        (Role::Family, &["משפחת", "משפ'"]),
    ];
    let Some(i) = tokens.iter().position(|t| t.start >= start) else {
        return Ok(None);
    };
    // Up to three words back, each with the word after it.
    let Some(back) = tokens.get(i.saturating_sub(3)..=i) else {
        return Ok(None);
    };
    for pair in back.windows(2).rev() {
        let [t, next] = pair else {
            continue;
        };
        if between(text, t, next)?.contains(['.', '\n', ';', '!', '?']) {
            return Ok(None);
        }
        for (_, head) in prefix_splits(&t.norm) {
            let bare = head.strip_prefix('ה').unwrap_or(&head);
            for (role, words) in ROLES {
                if words.iter().any(|w| norm_eq(w, &head) || norm_eq(w, &bare)) {
                    return Ok(Some((*role, span_text(text, t.start, t.end)?.to_owned())));
                }
            }
        }
    }
    Ok(None)
}

/// Every suspect becomes a replacement: a declared tag or a generalization when the filter
/// knows it, otherwise a new tag by the role the context gives. One name keeps one tag in the
/// text; a name the case already has (an identity the filter added before) keeps its own.
fn auto_hide(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    suspects: &[SuspectSpan],
    reps: &mut Vec<Replacement>,
) -> Result<Vec<AutoSpan>, PrivacyError> {
    let case: Vec<&Identity> = ctx
        .identities
        .iter()
        .filter(|i| i.case_id == ctx.case_id)
        .collect();
    let mut used: Vec<String> = case.iter().map(|i| i.tag.clone()).collect();
    let mut named: Vec<(String, String, Role)> = Vec::new();
    let mut out = Vec::new();
    for s in suspects {
        if overlaps(s.start, s.end, reps, &[]) {
            continue;
        }
        let token = s.suspect.token.clone();
        let (kind, uncertain) = match s.suspect.kind {
            SuspectKind::AmbiguousWord => (AutoKind::DeclaredInWord, false),
            SuspectKind::SimilarToDeclared => (AutoKind::SimilarSpelling, true),
            SuspectKind::OtherCaseIdentity => (AutoKind::OtherCase, false),
            SuspectKind::Indirect => (AutoKind::Indirect, false),
            SuspectKind::LatinName => (AutoKind::Name, true),
            SuspectKind::UnknownName => (
                AutoKind::Name,
                s.suspect.message.starts_with("שם שהוא גם מילה")
                    || s.suspect.message == HINT_UNSURE,
            ),
        };
        let (tag, role, reason, relative) = match (&s.known, kind) {
            (Some(g), AutoKind::Indirect) => {
                (g.clone(), Role::Other, s.suspect.message.clone(), true)
            }
            (Some(tag), _) => {
                let role = case
                    .iter()
                    .find(|i| &i.tag == tag)
                    .map_or(Role::Other, |i| i.role);
                (tag.clone(), role, s.suspect.message.clone(), false)
            }
            (None, _) => {
                let key = normalize(span_text(text, s.start, s.end)?);
                let context = role_from_context(text, tokens, s.start)?;
                let role = match &context {
                    Some((role, _)) => *role,
                    None if s.suspect.message.contains("שם משפחה") => Role::Family,
                    None => Role::Other,
                };
                let reason = match &context {
                    Some((_, cue)) => format!("אחרי '{cue}'"),
                    None => s.suspect.message.clone(),
                };
                let tag = if let Some(i) = case.iter().find(|i| normalize(&i.value) == key) {
                    i.tag.clone()
                } else if let Some((_, t, _)) = named.iter().find(|(k, _, _)| *k == key) {
                    t.clone()
                } else {
                    let t = assign_tag(role, &used);
                    used.push(t.clone());
                    named.push((key, t.clone(), role));
                    t
                };
                let role = named
                    .iter()
                    .find(|(_, t, _)| *t == tag)
                    .map_or(role, |(_, _, r)| *r);
                (tag, role, reason, false)
            }
        };
        reps.push(Replacement {
            start: s.start,
            end: s.end,
            out: tag.clone(),
            label: if kind == AutoKind::Indirect {
                "מקצוע".to_owned()
            } else {
                role_label(role)
            },
            relative,
            declared: false,
        });
        out.push(AutoSpan {
            start: s.start,
            end: s.end,
            item: AutoHidden {
                token,
                tag,
                role,
                reason,
                uncertain,
                kind,
            },
        });
    }
    Ok(out)
}

/// Names the filter added to the case earlier are hidden as identities now; the card still
/// lists them, so "להחזיר" stays one click away.
fn declared_auto(ctx: &PrivacyContext<'_>, reps: &[Replacement]) -> Vec<AutoSpan> {
    reps.iter()
        .filter(|r| r.declared)
        .filter_map(|r| {
            let i = ctx.identities.iter().find(|i| {
                i.case_id == ctx.case_id && i.tag == r.out && i.source != IdentitySource::Manual
            })?;
            Some(AutoSpan {
                start: r.start,
                end: r.end,
                item: AutoHidden {
                    token: i.value.clone(),
                    tag: i.tag.clone(),
                    role: i.role,
                    reason: i.reason.clone(),
                    uncertain: false,
                    kind: AutoKind::Name,
                },
            })
        })
        .collect()
}

/// The words of every new name found in this text, hidden wherever they recur in it (with
/// or without a prefix, unless the prefixed form is an everyday word, "מלאה" ≠ מ + לאה).
/// A name that is also a word ("אלה", "חן") recurs only where its context shows a name:
/// "משימות אלה" and "בעלת חן" stay.
fn recurring_names(
    text: &str,
    tokens: &[Token],
    ctx: &PrivacyContext<'_>,
    reps: &[Replacement],
    suspects: &[SuspectSpan],
) -> Result<Vec<SuspectSpan>, PrivacyError> {
    let mut words: Vec<String> = Vec::new();
    for s in suspects {
        if !matches!(
            s.suspect.kind,
            SuspectKind::UnknownName | SuspectKind::LatinName
        ) {
            continue;
        }
        for w in normalize(span_text(text, s.start, s.end)?).split(' ') {
            if w.chars().count() >= 2
                && !is_title(w)
                && !is_person_word(w)
                && !everyday_name(w)
                && !words.iter().any(|x| x == w)
            {
                words.push(w.to_owned());
            }
        }
    }
    let mut out: Vec<SuspectSpan> = Vec::new();
    if words.is_empty() {
        return Ok(out);
    }
    for tok in tokens {
        if overlaps(tok.start, tok.end, reps, suspects)
            || overlaps(tok.start, tok.end, &[], &out)
            || (ctx.allowlisted)(&tok.norm)
        {
            continue;
        }
        let raw = span_text(text, tok.start, tok.end)?;
        let hit = prefix_splits(&tok.norm).into_iter().find(|(p, h)| {
            words.iter().any(|w| w == h) && (*p == 0 || !reads_as_word(&tok.norm, h))
        });
        if let Some((p, _)) = hit {
            let start = tok.start + crate::text::byte_len_of_letters(raw, p);
            out.push(SuspectSpan {
                start,
                end: tok.end,
                suspect: Suspect {
                    token: span_text(text, start, tok.end)?.to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: "אותו שם במקום אחר בטקסט".to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            });
        }
    }
    Ok(out)
}

/// A job or a workplace, generalized: the cue stays ("עובדת", "מנהל") and what follows it
/// becomes a field ("בתחום החינוך"), or nothing when the field is unknown.
/// Where a job phrase that starts at the cue `cue` ends (the end of its last token): up to
/// five words, ending at punctuation, at a word that opens a new clause ("ולכן", "כי"), or
/// at "ו" + a word that is not part of a job ("ומגיעה"; "ומורה" goes on).
fn job_end(text: &str, tokens: &[Token], cue: usize) -> Result<usize, PrivacyError> {
    const STOP: &[&str] = &[
        "אבל",
        "אך",
        "כי",
        "אולם",
        "ולכן",
        "לכן",
        "כאשר",
        "והוא",
        "והיא",
        "והם",
        "ואילו",
        "אשר",
        "מאז",
        "עד",
        "אחרי",
        "לפני",
        "בזמן",
        "ביום",
        "בבוקר",
        "בערב",
        "שעות",
    ];
    let job_word = |h: &str| {
        JOB_FIELDS
            .iter()
            .any(|(_, words)| words.iter().any(|w| norm_eq(w, &h)))
            || PROFESSION_CUES.iter().any(|c| norm_eq(c, &h))
    };
    // The last word taken after the cue.
    let mut last: Option<&Token> = None;
    let pairs = tokens.iter().skip(cue).zip(tokens.iter().skip(cue + 1));
    for (t, next_tok) in pairs.take(5) {
        let gap = between(text, t, next_tok)?;
        if !gap
            .chars()
            .all(|c| matches!(c, ' ' | '-' | '–' | '־' | '"' | '״' | '\''))
        {
            break;
        }
        let next = &next_tok.norm;
        if STOP.iter().any(|w| norm_eq(w, next)) || next.starts_with(|c: char| c.is_ascii_digit()) {
            break;
        }
        if next.starts_with('ו')
            && !prefix_splits(next)
                .iter()
                .any(|(p, h)| *p > 0 && job_word(h))
        {
            break;
        }
        last = Some(next_tok);
    }
    // At least the word after the cue ("עובדת כגננת"), or the cue when it ends the text.
    // With no word at all, everything to the end of the text is taken (fail-closed).
    Ok(last
        .or_else(|| tokens.get(cue + 1))
        .or_else(|| tokens.last())
        .map_or(text.len(), |t| t.end))
}

/// Words that give a job its field, for the generalization ("עובדת בתחום החינוך").
const JOB_FIELDS: &[(&str, &[&str])] = &[
    (
        "בתחום החינוך",
        &[
            "מורה",
            "גננת",
            "סייעת",
            "מחנכת",
            "מחנך",
            "חינוכ",
            "בית ספר",
            "ספר",
            "גנ",
            "תיכונ",
            "אוניברסיטה",
            "מכללה",
            "הוראה",
            "חינוך",
        ],
    ),
    (
        "בתחום הבריאות",
        &[
            "רופא",
            "רופאה",
            "אח",
            "אחות",
            "חולימ",
            "מרפאה",
            "קופת",
            "בית חולימ",
            "רוקח",
            "רוקחת",
            "פיזיותרפיסט",
            "פיזיותרפיסטית",
            "סיעוד",
            "רפואה",
            "רפואי",
        ],
    ),
    (
        "בתחום ההייטק",
        &[
            "הייטק",
            "מהנדס",
            "מהנדסת",
            "מתכנת",
            "מתכנתת",
            "תוכנה",
            "סטארטאפ",
            "סטרטאפ",
            "מחשבימ",
            "טכנולוגיה",
            "חברת",
        ],
    ),
    (
        "בתחום הכספים",
        &[
            "חשבונות",
            "חשבונ",
            "בנק",
            "רואה",
            "רואת",
            "כספימ",
            "ביטוח",
            "פיננסימ",
            "השקעות",
        ],
    ),
    (
        "בתחום המשפט",
        &["עורך", "עורכת", "עו\"ד", "משפט", "משפטימ", "שופט", "שופטת"],
    ),
    (
        "בתחום הביטחון",
        &[
            "צבא",
            "צה\"ל",
            "משטרה",
            "שוטר",
            "שוטרת",
            "קבע",
            "ביטחונ",
            "כבאי",
        ],
    ),
    (
        "בתחום המסחר",
        &[
            "חנות",
            "מכירות",
            "קופאי",
            "קופאית",
            "סופר",
            "סופרמרקט",
            "מוכר",
            "מוכרת",
            "שיווק",
            "קניונ",
        ],
    ),
    (
        "בתחום המזון",
        &["מסעדה", "מטבח", "טבח", "טבחית", "שף", "מאפייה", "קייטרינג"],
    ),
    (
        "בתחום הבנייה",
        &["בנייה", "בניינ", "קבלנ", "שיפוצימ", "אדריכל", "אדריכלית"],
    ),
    (
        "בתחום התחבורה",
        &["נהג", "נהגת", "הובלות", "משאית", "אוטובוס", "מונית"],
    ),
    ("בתחום הרווחה", &["סוציאלי", "סוציאלית", "רווחה", "עו\"ס"]),
];

/// What goes out instead of the job phrase: the cue as written, and its field.
fn generalize_job(
    text: &str,
    tokens: &[Token],
    cue: &Token,
    end: usize,
) -> Result<String, PrivacyError> {
    let after: Vec<String> = tokens
        .iter()
        .filter(|t| t.start > cue.start && t.end <= end)
        .flat_map(|t| prefix_splits(&t.norm))
        .map(|(_, h)| h)
        .collect();
    let field = JOB_FIELDS
        .iter()
        .find(|(_, words)| after.iter().any(|h| words.iter().any(|w| norm_eq(w, h))));
    let raw_cue = span_text(text, cue.start, cue.end)?;
    let cue_word = match cue.norm.as_str() {
        "במקצועו" => "עובד",
        "במקצועה" => "עובדת",
        _ => raw_cue,
    };
    Ok(match field {
        Some((f, _)) => format!("{cue_word} {f}"),
        None => cue_word.to_owned(),
    })
}

/// The outgoing text and both columns. A replacement that does not cut `text` on character
/// boundaries (never expected) fails it with an internal error, so no text is copied out
/// unfiltered and nothing is sent.
fn assemble(
    text: &str,
    reps: &[Replacement],
    autos: &[AutoSpan],
) -> Result<FilterOutcome, PrivacyError> {
    let mut events: Vec<&Replacement> = reps.iter().collect();
    events.sort_by_key(|r| r.start);

    let seg = |t: &str, mark: Option<Mark>, label: Option<String>| Segment {
        text: t.to_owned(),
        mark,
        label,
    };
    let (mut original, mut tagged_segments, mut tagged) =
        (Vec::new(), Vec::new(), String::with_capacity(text.len()));
    let mut pos = 0;
    for r in &events {
        if r.start < pos {
            continue;
        }
        let plain = span_text(text, pos, r.start)?;
        if !plain.is_empty() {
            original.push(seg(plain, None, None));
            tagged_segments.push(seg(plain, None, None));
            tagged.push_str(plain);
        }
        let piece = span_text(text, r.start, r.end)?;
        original.push(seg(piece, Some(Mark::Replaced), Some(r.label.clone())));
        let mark = if r.relative {
            Mark::Relative
        } else {
            Mark::Tag
        };
        tagged_segments.push(seg(&r.out, Some(mark), Some(r.label.clone())));
        tagged.push_str(&r.out);
        pos = r.end;
    }
    if pos < text.len() {
        let rest = span_text(text, pos, text.len())?;
        original.push(seg(rest, None, None));
        tagged_segments.push(seg(rest, None, None));
        tagged.push_str(rest);
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
    let mut auto_hidden: Vec<AutoHidden> = Vec::new();
    for a in autos {
        if !auto_hidden
            .iter()
            .any(|x| normalize(&x.token) == normalize(&a.item.token) && x.tag == a.item.tag)
        {
            auto_hidden.push(a.item.clone());
        }
    }
    let autos_of = |indirect: bool| {
        u32::try_from(
            autos
                .iter()
                .filter(|a| (a.item.kind == AutoKind::Indirect) == indirect)
                .count(),
        )
        .unwrap_or(u32::MAX)
    };
    let checks = Checks {
        declared_names: count(&|r| r.declared),
        patterns: count(&|r| !r.declared),
        name_suspects: autos_of(false),
        indirect_suspects: autos_of(true),
    };
    Ok(FilterOutcome {
        tagged,
        original_segments: original,
        tagged_segments,
        auto_hidden,
        hidden,
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kept index answers exactly as a fresh build, through a change of names, of aliases,
    /// of the practitioner and of the case, and after it is forgotten.
    #[test]
    fn the_kept_index_is_always_the_one_a_fresh_build_gives() {
        let ident = |id: &str, case: &str, value: &str, aliases: &[&str]| Identity {
            id: id.into(),
            case_id: case.into(),
            role: Role::Child,
            tag: "[ילד]".into(),
            value: value.into(),
            aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
            source: IdentitySource::Manual,
            reason: String::new(),
        };
        let text = "נועם ונועמי שיחקו עם עידו ועם ד\"ר כרמלה, ואחר כך ינאי הגיע";
        let tokens = tokenize(text);
        let no = |_: &str| false;
        let first = vec![ident("1", "a", "נועם", &[])];
        let alias = vec![ident("1", "a", "נועם", &["נועמי"])];
        let more = vec![
            ident("1", "a", "נועם", &["נועמי"]),
            ident("2", "b", "עידו", &[]),
        ];
        let fewer = vec![ident("2", "b", "עידו", &[])];
        let doctor = vec!["כרמלה".to_owned()];
        let steps: [(&str, &[Identity], &[String]); 6] = [
            ("a", &first, &[]),
            ("a", &alias, &[]),
            ("a", &more, &[]),
            ("b", &more, &[]),
            ("b", &more, &doctor),
            ("b", &fewer, &doctor),
        ];
        for (case_id, identities, practitioner) in steps {
            let ctx = PrivacyContext {
                case_id,
                identities,
                practitioner,
                allowlisted: &no,
                confirmed_names: &no,
                past_names: &no,
                today: (2026, 10, 15),
                masks: None,
            };
            let fresh = format!("{:?}", build_identity_index(&ctx).find(text, &tokens));
            for _ in 0..2 {
                let kept = format!("{:?}", identity_index(&ctx).find(text, &tokens));
                assert_eq!(kept, fresh, "{case_id} {identities:?} {practitioner:?}");
            }
        }
        // Other tests share the kept index, so only what forgetting leaves is checked here:
        // the next request builds afresh and answers the same.
        forget_identity_index();
        let ctx = PrivacyContext {
            case_id: "a",
            identities: &first,
            practitioner: &[],
            allowlisted: &no,
            confirmed_names: &no,
            past_names: &no,
            today: (2026, 10, 15),
            masks: None,
        };
        assert_eq!(
            format!("{:?}", identity_index(&ctx).find(text, &tokens)),
            format!("{:?}", build_identity_index(&ctx).find(text, &tokens))
        );
    }

    fn rep(start: usize, end: usize) -> Replacement {
        Replacement {
            start,
            end,
            out: "[ילד]".to_owned(),
            label: "שם".to_owned(),
            relative: false,
            declared: true,
        }
    }

    #[test]
    fn a_cut_inside_a_character_fails_closed() {
        let text = "נועם צייר";
        // Byte 1 and byte 3 are inside a letter: nothing is copied out, the filtering fails.
        assert!(matches!(
            assemble(text, &[rep(1, 4)], &[]),
            Err(PrivacyError::Internal(_))
        ));
        assert!(matches!(
            assemble(text, &[rep(0, 3)], &[]),
            Err(PrivacyError::Internal(_))
        ));
        assert!(span_text(text, 1, 2).is_err());
        let name = "נועם".len();
        assert_eq!(
            assemble(text, &[rep(0, name)], &[]).unwrap().tagged,
            "[ילד] צייר"
        );
        // Where the line starts cannot be read: a field label there still hides what follows.
        assert!(starts_line(text, 1));
        assert!(!starts_line(text, name + 1));
    }
}
