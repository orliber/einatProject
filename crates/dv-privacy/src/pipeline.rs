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
    is_name_context, word_bucket, PlaceKind, GENERIC_PLACE_WORDS, LEXICON, NAME_LABELS, NAME_STOP,
    PERSON_WORDS, ROLE_WORDS, SURNAME_ENDINGS, TITLES,
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
        // The same person by a nickname or the full form ("שמעון" ↔ "שימי", "ולדה" ↔
        // "ולדימיר"): hidden, and refused by the gate, like the declared spelling.
        for nick in LEXICON.nicknames.get(w).into_iter().flatten() {
            if !out.iter().any(|p| p.len() == 1 && &p[0] == nick) {
                out.push(vec![nick.clone()]);
            }
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

/// The consonants of a name, in one alphabet for both scripts, so a name declared in Hebrew
/// is found in Arabic ("ג'סר" = "جسر", "סמאח" = "سماح"). Vowel letters (א ו י / ا و ي) and
/// a final ה / ه are left out, and letters that Hebrew spells alike share a class
/// (س/ص = ס/צ, ت/ط = ת/ט, ك/ق = כ/ק). `None` when the word is not in that script.
fn consonants(word: &str, arabic: bool) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
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
    let mut add = |name: &str, target: Target| {
        for w in normalize(name).split(' ') {
            if is_title(w) || is_generic(w) {
                continue;
            }
            if let Some(k) = consonants(w, false).filter(|k| k.chars().count() >= 3) {
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
        for name in std::iter::once(&identity.value).chain(identity.aliases.iter()) {
            add(name, target.clone());
        }
    }
    for name in ctx.practitioner {
        add(name, Target::Practitioner);
    }
    let mut out = Vec::new();
    for t in tokens.iter().filter(|t| arabic(t)) {
        // Prefixes: "و", "ال", "ب", "ل", "ك", "ف" and their combinations.
        let chars: Vec<char> = t.norm.chars().collect();
        for skip in 0..chars.len().min(4) {
            let glued: String = chars[..skip].iter().collect();
            if ![
                "", "و", "ال", "وال", "ب", "بال", "ل", "لل", "ول", "وب", "ك", "ف",
            ]
            .contains(&glued.as_str())
            {
                continue;
            }
            let rest: String = chars[skip..].iter().collect();
            let Some(k) = consonants(&rest, true) else {
                continue;
            };
            if let Some((_, target)) = declared.iter().find(|(d, _)| *d == k) {
                let raw = &text[t.start..t.end];
                let start = t.start + crate::text::byte_len_of_letters(raw, skip);
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
fn is_profession_context(cue: &str, next: Option<&str>, after: Option<&str>) -> bool {
    let work = ["עובד", "עובדת", "עובדים", "מועסק", "מועסקת"]
        .iter()
        .any(|w| normalize(w) == cue);
    let manager = ["מנהל", "מנהלת"].iter().any(|w| normalize(w) == cue);
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
        .any(|w| normalize(w) == next)
    {
        // "עובד בלילות ופחות זמין" is a schedule; "עובד במשמרות בנמל" still names the place.
        return after.is_some_and(|a| a.starts_with(['ב', 'כ']) && a != normalize("בבית"));
    }
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
            "הצוות",
            "השפ\"ח",
            "החינוך",
            "את",
            "שיחה",
        ]
        .iter()
        .any(|w| normalize(w) == next);
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
    let whole = &tokens[first];
    let letters: Vec<char> = whole.norm.chars().collect();
    let head: String = letters[prefix.min(letters.len())..].iter().collect();
    if word_bucket(&head) < EVERYDAY {
        return false;
    }
    // "בגיל", "לגיל", "מגיל", "כגיל": a preposition glued on.
    let preposition = prefix > 0 && matches!(letters[prefix - 1], 'ב' | 'ל' | 'מ' | 'כ');
    let plain_after = tokens
        .get(first + 1)
        .filter(|n| &text[whole.end..n.start] == " ")
        .map(|n| n.norm.as_str());
    let dash_after = tokens
        .get(first + 1)
        .filter(|n| {
            text[whole.end..n.start]
                .trim()
                .chars()
                .all(|c| matches!(c, '-' | '–' | '־'))
        })
        .map(|n| n.norm.as_str());
    let before = first
        .checked_sub(1)
        .map(|j| tokens[j].norm.as_str())
        .filter(|_| first > 0 && &text[tokens[first - 1].end..whole.start] == " ");
    let one_of =
        |w: Option<&str>, list: &[&str]| w.is_some_and(|w| list.iter().any(|l| normalize(l) == w));
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
            let in_light_of = prefix > 0
                && letters[prefix - 1] == 'ל'
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
        0 => long.len() > 1 && long[..long.len() - 1] == short[..short.len() - 1],
        1 => long[..short.len()] == short[..] || (long[0] == 'ה' && long[1..] == short[..]),
        _ => false,
    }
}

/// Only spaces, a bullet or a list number between the start of the line (or a table cell)
/// and `at`.
fn starts_line(text: &str, at: usize) -> bool {
    let before = &text[..at];
    let line = before
        .rfind(['\n', '|', '\t'])
        .map_or(before, |i| &before[i + 1..]);
    line.chars().all(|c| {
        c.is_whitespace()
            || c.is_ascii_digit()
            || matches!(
                c,
                '.' | ')' | '(' | '-' | '•' | '*' | '·' | '–' | '\u{200F}' | '\u{200E}'
            )
    })
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
            // A form field starts its line (or a table cell): "הדרכת הורים: מתן זמן" is
            // a sentence, not the field "הורים:".
            if !starts_line(text, tokens[i].start) {
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
                if !is_title(&t.norm)
                    && !role
                    && !is_person_word(&t.norm)
                    && t.norm != normalize("משפחת")
                {
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

    let possessive = ["שלו", "שלה", "שלי", "שלנו", "שלהם", "שלהן", "שלך"].map(normalize);

    // 4. A name set off after a person word: "הגננת הקודמת, ברכה, תיארה…",
    // "האח הגדול – יהונתן – מתגייס", "הקלינאית (אינה) המליצה". Between the marks: one or
    // two words, which in a report can only be a name.
    let opens = |g: &str| g.contains([',', '–', '—', '(']) && !g.contains(['.', '\n', ':']);
    let closes = |g: &str| g.contains([',', '–', '—', ')', '.']);
    for i in 0..tokens.len() {
        if !is_person_word(&tokens[i].norm) {
            continue;
        }
        // One adjective, possessive or "from the …" may follow the person word ("הקודמת",
        // "שלהם", "חברתה מהגן").
        let mut k = i;
        if k + 1 < tokens.len()
            && (tokens[k + 1].norm.starts_with('ה')
                || possessive.contains(&tokens[k + 1].norm)
                || tokens[k + 1].norm.starts_with("מה"))
            && joins(gap(k, k + 1))
        {
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
                    && word_bucket(&t.norm) < EVERYDAY
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
            for t in words.iter().filter(|t| !is_title(&t.norm)) {
                push(t.start, t.end, "שם שמופיע ליד תפקיד", out);
            }
        }
    }

    // 5. A name right after a word for a person, with no punctuation: "הגננת אסתי",
    // "סבא מוטי", "דודה שלו ולדה", "הסייעת החדשה רבקה", "המורה לאנגלית שירלי". What follows
    // must look like a name: a known name, or a word that is not an everyday word.
    for i in 0..tokens.len() {
        if !is_person_word(&tokens[i].norm) {
            continue;
        }
        let mut j = i + 1;
        // One modifier may come between: "שלו", "הקודמת", "לאנגלית".
        if j + 1 < tokens.len()
            && gap(i, j) == " "
            && (possessive.contains(&tokens[j].norm)
                || ["רבתא", "רבא", "רבה"]
                    .iter()
                    .any(|w| normalize(w) == tokens[j].norm)
                || (tokens[j].norm.starts_with(['ה', 'ל'])
                    && word_bucket(&tokens[j].norm) >= EVERYDAY
                    && !is_person_word(&tokens[j].norm)))
        {
            j += 1;
        }
        // A nickname may be quoted: סבתא שלה "נונה".
        let plain_gap = |g: &str| {
            g.contains(' ')
                && g.chars()
                    .all(|c| c == ' ' || matches!(c, '"' | '״' | '“' | '”' | '\''))
        };
        if j >= tokens.len() || !plain_gap(gap(j - 1, j)) {
            continue;
        }
        let t = &tokens[j];
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
        let dative = tokens.get(j + 1).is_some_and(|n| {
            gap(j, j + 1) == " "
                && ["לו", "לה", "לי", "לנו", "להם", "להן", "לך"]
                    .iter()
                    .any(|w| normalize(w) == n.norm)
        });
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
            push(t.start, t.end, "שם אחרי תפקיד או קרבה", out);
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
            if i + n >= tokens.len()
                || !tokens[i..i + n]
                    .iter()
                    .zip(phrase)
                    .all(|(t, w)| t.norm == *w || (n == 1 && t.norm == format!("ו{w}")))
            {
                continue;
            }
            let j = i + n;
            let t = &tokens[j];
            let plain = gap(j - 1, j)
                .chars()
                .all(|c| c == ' ' || matches!(c, '"' | '״' | '“' | '”' | '\'' | ':'));
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
                push(t.start, t.end, "שם אחרי \"קוראים לו\" או \"בשם\"", out);
            }
        }
    }

    // 7. Two parts of one name joined by a hyphen, one of them a known name and the other
    // not an everyday word: "אביבה-טגסט", "שרה-לי". Ordinary compounds ("דו-לשוני",
    // "אי-שקט") have everyday words on both sides.
    for i in 0..tokens.len().saturating_sub(1) {
        let g = gap(i, i + 1);
        if g != "-" && g != "\u{05BE}" {
            continue;
        }
        let (a, b) = (&tokens[i], &tokens[i + 1]);
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
            push(a.start, b.end, "שם עם מקף", out);
        }
    }

    // 8. A surname right after a first name, a declared name or a name found above.
    let is_name = |i: usize, out: &[SuspectSpan]| {
        let t = &tokens[i];
        reps.iter()
            .any(|r| r.declared && r.start <= t.start && t.end <= r.end)
            || out.iter().any(|s| s.start <= t.start && t.end <= s.end)
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
            || (is_name(i - 1, out) && looks_like_surname && !lex.common_words.contains(&t.norm))
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

        // "מלאה" is מ + לאה only on paper: an everyday word is read as a word, unless the
        // prefixed form is mostly the name ("לדני" is far rarer than "דני").
        let as_word = |p: usize, h: &str| p > 0 && reads_as_word(&tok.norm, h);
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
        let ends_clause = tokens
            .get(i + 1)
            .is_none_or(|t| text[tok.end..t.start].contains([',', '.', ';', ')', '\n']));
        let word_in_context = word.filter(|_| {
            let after_prev = prev.is_some_and(|p| {
                crate::lexicon::NAME_CONTEXT_PREV
                    .iter()
                    .any(|l| normalize(l) == p)
            });
            let before_next = next.is_some_and(|n| {
                crate::lexicon::NAME_CONTEXT_NEXT
                    .iter()
                    .any(|l| normalize(l) == n)
            });
            if very_common {
                // "את אלה." is "these"; only "with" or a person word says a person follows.
                let with = prev.is_some_and(|p| {
                    ["עם", "ועם", "אצל", "ואצל"]
                        .iter()
                        .any(|w| normalize(w) == p)
                        || is_person_word(p)
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
                    token: text[start..tok.end].to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: "שם שלא מופיע ברשימת התיק".to_owned(),
                    suggested_role: Role::OtherChild,
                },
            });
            continue;
        }
        // "ג'וד", "צ'רלי": a geresh inside a word that is not an everyday loanword is a
        // borrowed sound, as in names.
        // Loanwords have it too ("צ'אט", "ג'ינס"), so only where a name is expected.
        let letters: Vec<char> = raw.chars().collect();
        let inner_geresh = letters.len() >= 3
            && letters[..letters.len() - 1]
                .iter()
                .any(|c| matches!(c, '\'' | '׳'))
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
                    .any(|w| normalize(w) == p)
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
                    token: text[start..tok.end].to_owned(),
                    kind: SuspectKind::SimilarToDeclared,
                    message: format!("כתיב קרוב לשם שבתיק ({tag})"),
                    suggested_role: Role::Other,
                },
            });
            continue;
        }
        if PROFESSION_CUES.iter().any(|c| normalize(c) == tok.norm)
            && is_profession_context(&tok.norm, next, tokens.get(i + 2).map(|t| t.norm.as_str()))
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

fn address_fields(text: &str) -> Vec<Replacement> {
    const LABELS: &[&str] = &["כתובת", "כתובת מגורים", "כתובת המשפחה", "כתובת הבית", "מען"];
    let mut out = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let lead = body.len() - body.trim_start().len();
        let rest = &body[lead..];
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
            for part in value[..end_rel].split(',') {
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
    // Token ranges of the names found, and whether they may be hidden wherever they recur.
    let mut names: Vec<(usize, usize, bool)> = Vec::new();
    // Positions of the institution words, so a name found once is also taken after them.
    let mut keywords: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // The institution word(s): "גן", "מעון", "בית ספר", "בית הספר", "בי\"ס", "תלמוד תורה".
        let next_is = |words: &[&str]| {
            tokens
                .get(i + 1)
                .is_some_and(|t| words.iter().any(|w| normalize(w) == t.norm))
        };
        // `broad`: clinics, institutes and the like, whose descriptions are often ordinary
        // nouns ("מכון שמיעה"), so an unquoted name must not be an everyday word.
        let (end_word, broad) = if is(
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
            (Some(i), false)
        } else if is(&tokens[i], &["בית", "תלמוד"]) && next_is(&["ספר", "הספר", "תורה"])
        {
            (Some(i + 1), false)
        } else if (is(&tokens[i], &["בית"]) && next_is(&["חולים", "החולים", "מרקחת", "המרקחת"]))
            || (is(&tokens[i], &["מרכז", "המרכז"]) && next_is(&["רפואי", "הרפואי"]))
            || (is(&tokens[i], &["מרפאת"]) && next_is(&["ילדים", "הילדים"]))
            || (is(&tokens[i], &["קופת"]) && next_is(&["חולים"]))
        {
            (Some(i + 1), true)
        } else if is(
            &tokens[i],
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
        let Some(k) = end_word else {
            i += 1;
            continue;
        };
        keywords.push(k);
        // A health fund is one of four names, also after a colon ("קופת חולים: כללית").
        let fund = is(&tokens[i], &["קופת", "קופ\"ח"]);
        let mut first = k + 1;
        if first >= tokens.len()
            || gap(k, first).contains(['.', ',', '\n', ';', '('])
            || (gap(k, first).contains(':') && !fund)
        {
            i = k + 1;
            continue;
        }
        if fund
            && HEALTH_FUNDS
                .iter()
                .any(|f| normalize(f) == tokens[first].norm)
        {
            // Left out, like a kindergarten's name: "קופ\"ח מכבי, מס' חבר" → "קופ\"ח, מס' חבר".
            out.push(Replacement {
                start: tokens[k].end,
                end: tokens[first].end,
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
        if broad && starts_line(text, tokens[i].start) {
            if let Some(last) = letterhead_name(text, tokens, first) {
                out.push(Replacement {
                    start: tokens[k].end,
                    end: tokens[last].end,
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
            let everyday = |w: &str| crate::lexicon::word_bucket(w) >= EVERYDAY;
            let stem = &t.norm[t.norm.char_indices().nth(1).map_or(0, |(b, _)| b)..];
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
            // A quoted name, a name of two words or an unusual word is the name wherever
            // it recurs; a single ordinary word ("גן פרפר") only after an institution word.
            let strong = opened
                || last > first
                || crate::lexicon::word_bucket(&tokens[first].norm) < EVERYDAY;
            names.push((first, last, strong));
            i = last + 1;
        } else {
            i = k + 1;
        }
    }
    recurring_institutions(text, tokens, &names, &keywords, &mut out);
    out
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
/// Later words may be in construct ("המרכז הרפואי גבעות החוף").
fn letterhead_name(text: &str, tokens: &[Token], first: usize) -> Option<usize> {
    let lex = &*LEXICON;
    let mut last = None;
    for j in first..tokens.len().min(first + 3) {
        let t = &tokens[j];
        let hebrew = t
            .norm
            .chars()
            .all(|c| ('\u{05D0}'..='\u{05EA}').contains(&c));
        let described = prefix_splits(&t.norm).iter().any(|(p, h)| {
            *p == 1
                && t.norm.starts_with('ל')
                && (DESCRIPTION_HEADS.iter().any(|d| normalize(d) == *h)
                    || (h.starts_with('ה') && word_bucket(h) >= EVERYDAY))
        });
        if !hebrew
            || (j == first && (t.norm.starts_with('ה') || described))
            || word_bucket(&t.norm) >= 14
            || lex.common_words.contains(&t.norm)
            || INSTITUTION_STOP.iter().any(|w| normalize(w) == t.norm)
        {
            return None;
        }
        let after = tokens
            .get(j + 1)
            .map_or(&text[t.end..], |n| &text[t.end..n.start]);
        if after.contains(['–', '—', '|', '(', '\n'])
            || after.contains(" - ")
            || j + 1 == tokens.len()
        {
            last = Some(j);
            break;
        }
        if after != " " {
            return None;
        }
    }
    last
}

/// A name found once ("מכון \"צעדים קטנים\"") is hidden where it comes back
/// ("מ\"צעדים קטנים\"", "ממכון צמיחה").
fn recurring_institutions(
    text: &str,
    tokens: &[Token],
    names: &[(usize, usize, bool)],
    keywords: &[usize],
    out: &mut Vec<Replacement>,
) {
    let quotes = ['"', '״', '“', '”', '„'];
    for (first, last, strong) in names {
        let words: Vec<&str> = tokens[*first..=*last]
            .iter()
            .map(|t| t.norm.as_str())
            .collect();
        for j in 0..tokens.len() {
            if j + words.len() > tokens.len() {
                break;
            }
            let t = &tokens[j];
            // The first word may carry a prefix and an opening quote glued on (מ"צעדים).
            let lead = prefix_splits(&t.norm).into_iter().find_map(|(p, h)| {
                let bare = h.trim_start_matches(quotes);
                (bare == words[0]).then(|| {
                    let raw = &text[t.start..t.end];
                    let pre = crate::text::byte_len_of_letters(raw, p);
                    pre + (raw[pre..].len() - raw[pre..].trim_start_matches(quotes).len())
                })
            });
            let Some(lead) = lead else { continue };
            let rest_matches = (1..words.len()).all(|n| {
                tokens[j + n].norm == words[n]
                    && &text[tokens[j + n - 1].end..tokens[j + n].start] == " "
            });
            let start = t.start + lead;
            let end = tokens[j + words.len() - 1].end;
            let after_keyword = j > 0 && keywords.contains(&(j - 1)) && lead == 0;
            if rest_matches
                && (*strong || after_keyword)
                && !out.iter().any(|r| r.start < end && start < r.end)
            {
                // After "ממכון" the name is just left out; elsewhere it reads "המוסד".
                let (start, text_out) = if after_keyword {
                    (tokens[j - 1].end, String::new())
                } else {
                    (start, INSTITUTION_WORD.to_owned())
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
}

/// A one-word place that is read as an ordinary word here. A place takes no article and
/// no "ש" / "כ" glued on ("שאלונים", "כשמכבים", "הגדרה"); after "ב/מ/ל" it is a word only
/// when that form is far more common than the place ("מדברת", "בחורה"). A place that is
/// itself an everyday word ("קדימה", "אזור", "מעלות") counts only after "ב/מ/ל" when the
/// prefixed form is rarer than the bare word ("במעלות", not "באזור"), or after a word for a
/// place ("מושב אלונים", "גרים"). Anything else stays hidden.
fn place_reads_as_word(text: &str, tokens: &[Token], i: usize, prefix: usize) -> bool {
    let whole = &tokens[i];
    let letters: Vec<char> = whole.norm.chars().collect();
    let head: String = letters[prefix.min(letters.len())..].iter().collect();
    let glued: String = letters[..prefix.min(letters.len())].iter().collect();
    let place_word = i
        .checked_sub(1)
        .and_then(|j| tokens.get(j))
        .filter(|t| text[t.end..whole.start].trim().is_empty())
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
            .any(|w| normalize(w) == t.norm)
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
    if !PLACE_WORDS.iter().any(|p| normalize(p) == head) {
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
) -> Result<(Vec<Replacement>, Vec<SuspectSpan>), PrivacyError> {
    let tokens = tokenize(text);
    let mut reps: Vec<Replacement> = Vec::new();
    let mut suspects: Vec<SuspectSpan> = Vec::new();

    // Layer 2: declared identities (every case; another case's name is a suspect, not a tag).
    for m in identity_index(ctx).find(text, &tokens) {
        if declared_reads_as_word(text, &tokens, m.first_token, m.last_token, m.prefix) {
            continue;
        }
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
    // Layer 2, Arabic script: a declared name spelled in Arabic ("روضة جسر").
    for (start, end, target) in arabic_identity_matches(text, &tokens, ctx) {
        if overlaps(start, end, &reps, &suspects) {
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
                    token: text[start..end].to_owned(),
                    kind: SuspectKind::OtherCaseIdentity,
                    message: "שם שמופיע בתיק אחר".to_owned(),
                    suggested_role: Role::OtherChild,
                },
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
    // An address field: whatever follows "כתובת:" on its line is the address, with or without
    // the word "רחוב" ("כתובת: מסדה 33, חיפה").
    for r in address_fields(text) {
        if !overlaps(r.start, r.end, &reps, &suspects) {
            reps.push(r);
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
        if !overlaps(m.start, m.end, &reps, &suspects)
            && !(m.first_token == m.last_token
                && place_reads_as_word(text, &tokens, m.first_token, m.prefix))
        {
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
