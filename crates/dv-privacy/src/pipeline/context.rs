//! Strong contexts for names that are also words (D-049): "שירה אמרה לי", "עם ים ומתן",
//! "סייעת הודיה", "יום ההולדת של ים", "נולד בחרקוב". Alone, such a word is too common to
//! suspect; in these contexts it can only be a person or a place. Only words on the name
//! lists can become names here, so an ordinary word never turns into one.

use dv_domain::Role;

use super::{
    is_person_word, overlaps, span_text, Replacement, Suspect, SuspectKind, SuspectSpan, EVERYDAY,
};
use crate::lexicon::{word_bucket, LEXICON};
use crate::text::{byte_len_of_letters, norm_eq, normalize, prefix_splits, Token};
use crate::PrivacyError;

/// Verbs that make the word before them a person: "שירה אמרה", "הודיה מתקרבת ושואלת".
const ACTION_NEXT: &[&str] = &[
    "אמר",
    "אמרה",
    "אומר",
    "אומרת",
    "סיפר",
    "סיפרה",
    "מספר",
    "מספרת",
    "שאל",
    "שאלה",
    "שואל",
    "שואלת",
    "ביקש",
    "ביקשה",
    "מבקש",
    "מבקשת",
    "נתן",
    "נתנה",
    "נותן",
    "נותנת",
    "הצליח",
    "הצליחה",
    "לימד",
    "לימדה",
    "מלמד",
    "מלמדת",
    "דיווח",
    "דיווחה",
    "מדווח",
    "מדווחת",
    "המליץ",
    "המליצה",
    "ממליץ",
    "ממליצה",
    "הזכיר",
    "הזכירה",
    "ענה",
    "ענתה",
    "עונה",
    "עזר",
    "עזרה",
    "עוזר",
    "עוזרת",
    "כתב",
    "כתבה",
    "כותב",
    "כותבת",
    "התקשר",
    "התקשרה",
    "הביא",
    "הביאה",
    "מביא",
    "מביאה",
    "מתקרב",
    "מתקרבת",
    "מציין",
    "מציינת",
    "ציין",
    "ציינה",
    "מתאר",
    "מתארת",
    "תיאר",
    "תיארה",
    "הגיע",
    "הגיעה",
    "מגיע",
    "מגיעה",
    "שיחק",
    "שיחקה",
    "בכה",
    "בכתה",
    "צחק",
    "צחקה",
    "דחף",
    "דחפה",
    "הרביץ",
    "הרביצה",
    "לקח",
    "לקחה",
    "רצתה",
    "פנה",
    "פנתה",
    "טוען",
    "טוענת",
    "משתף",
    "משתפת",
    "שיתף",
    "שיתפה",
    "הסביר",
    "הסבירה",
    "מסביר",
    "מסבירה",
];

/// Words that may stand between a person word and the name: "הגננת הקודמת שלו שולי".
const BETWEEN: &[&str] = &[
    "הקודמת",
    "הקודם",
    "החדשה",
    "החדש",
    "הקבועה",
    "הקבוע",
    "שלו",
    "שלה",
    "שלנו",
    "שלי",
    "שלהם",
    "שלהן",
];

/// People words missing from the general list, for this rule only.
const MORE_PEOPLE: &[&str] = &["נהג", "הנהג", "בעיסוק", "מר", "גב'", "גברת"];

/// Forms of address: whatever follows is a name, however common the word.
const ADDRESS: &[&str] = &["מר", "גב'", "גברת"];

/// "יום ההולדת של ים", "האמא של יהל": a person follows "של".
const OF_PERSON: &[&str] = &[
    "ההולדת",
    "אמא",
    "האמא",
    "אבא",
    "האבא",
    "הבן",
    "הבת",
    "אחיו",
    "אחותו",
    "אחיה",
    "אחותה",
    "החבר",
    "החברה",
    "הבית",
];

/// Words that make what follows a description, not a name: "הסייעת טובה מאוד".
const DEGREE_NEXT: &[&str] = &["מאוד", "יותר", "פחות", "מדי", "ביותר", "כל", "גם"];

/// A clause boundary in the text between two words.
fn boundary(gap: &str) -> bool {
    gap.contains([
        '.', ',', ';', ':', '!', '?', '\n', '(', ')', '"', '״', '–', '-',
    ])
}

fn is_in(norm: &str, words: &[&str]) -> bool {
    words.iter().any(|w| norm_eq(w, &norm))
}

/// The name inside a token: the whole word, or after "ו" when `vav` allows it.
fn name_in(tok: &Token, vav: bool) -> Option<(usize, String)> {
    let lex = &*LEXICON;
    prefix_splits(&tok.norm).into_iter().find(|(p, h)| {
        let prefix_ok = *p == 0 || (vav && *p == 1 && tok.norm.starts_with('ו'));
        prefix_ok && (lex.first_names.contains(h) || lex.word_names.contains(h))
    })
}

struct Scan<'a> {
    text: &'a str,
    tokens: &'a [Token],
}

// Every index passed here is a token of `tokens`. Were one not, the reading that keeps
// looking for a name is taken: words count as joined and a clause as starting.
impl Scan<'_> {
    fn gap(&self, a: usize, b: usize) -> Option<&str> {
        let (a, b) = (self.tokens.get(a)?, self.tokens.get(b)?);
        self.text.get(a.end..b.start)
    }

    fn joined(&self, a: usize, b: usize) -> bool {
        self.gap(a, b).is_none_or(|g| g.trim().is_empty())
    }

    fn norm(&self, i: usize) -> &str {
        self.tokens.get(i).map_or("", |t| t.norm.as_str())
    }

    /// The first word after `i`, past a parenthesis: "מרינה (המרפאה בעיסוק שלו) נתנה".
    fn next_word(&self, i: usize) -> Option<usize> {
        let j = i + 1;
        let gap = self.gap(i, j)?;
        if !gap.contains('(') {
            // "לדבר, שרה, מספרת סיפורים": a verb after a comma is the next item in a list.
            return gap.trim().is_empty().then_some(j);
        }
        let from = self.tokens.get(i)?.end;
        let close = self.text.get(from..)?.find(')')? + from;
        let mut after = self.tokens.iter().enumerate().skip(j);
        after.find(|(_, t)| t.start > close).map(|(k, _)| k)
    }

    fn starts_clause(&self, i: usize) -> bool {
        i == 0 || self.gap(i - 1, i).is_none_or(boundary)
    }

    /// A person word before `i`, joined to it, with up to two words such as "הקודמת שלו"
    /// between them.
    fn person_before(&self, i: usize) -> bool {
        let mut k = i;
        for _ in 0..3 {
            let Some(prev) = k.checked_sub(1) else {
                return false;
            };
            if !self.joined(prev, k) {
                return false;
            }
            let w = self.norm(prev);
            if is_person_word(w) || is_in(w, MORE_PEOPLE) {
                return true;
            }
            if !is_in(w, BETWEEN) {
                return false;
            }
            k = prev;
        }
        false
    }
}

/// Names that are also words, read from a context that leaves no other reading. Runs after
/// the general rules, on what they left. A word that cannot be cut out of the text (never
/// expected) fails the whole filtering, so nothing is sent.
pub(super) fn strong_names(
    text: &str,
    tokens: &[Token],
    allowlisted: &dyn Fn(&str) -> bool,
    reps: &[Replacement],
    suspects: &[SuspectSpan],
) -> Result<Vec<SuspectSpan>, PrivacyError> {
    let lex = &*LEXICON;
    let s = Scan { text, tokens };
    let mut out: Vec<SuspectSpan> = Vec::new();
    let taken = |i: usize, out: &[SuspectSpan]| {
        tokens.get(i).is_some_and(|t| {
            overlaps(t.start, t.end, reps, suspects) || overlaps(t.start, t.end, &[], out)
        })
    };
    // A person already hidden at `i`: a name found by the rules or a declared person.
    let person_at = |i: usize, out: &[SuspectSpan]| {
        let Some(t) = tokens.get(i) else {
            return false;
        };
        let hit = |a: usize, b: usize| t.start < b && a < t.end;
        suspects
            .iter()
            .chain(out.iter())
            .any(|x| hit(x.start, x.end) && x.suspect.kind == SuspectKind::UnknownName)
            || reps.iter().any(|r| {
                hit(r.start, r.end)
                    && r.declared
                    && !["[גן", "[יישוב", "[מוסד", "[בית"]
                        .iter()
                        .any(|p| r.out.starts_with(p))
            })
    };
    // `any`: the word need not be on the name lists ("הנהג אבי פרץ": a family name follows).
    let push_word =
        |i: usize, why: &str, any: bool, out: &mut Vec<SuspectSpan>| -> Result<(), PrivacyError> {
            // No token at `i`: no word there to hide.
            let Some(tok) = tokens.get(i) else {
                return Ok(());
            };
            let found = name_in(tok, true);
            let (p, head) = match found {
                Some(f) => f,
                None if any => (0, tok.norm.clone()),
                None => return Ok(()),
            };
            if taken(i, out) || allowlisted(&head) || allowlisted(&tok.norm) {
                return Ok(());
            }
            let raw = span_text(text, tok.start, tok.end)?;
            let start = tok.start + byte_len_of_letters(raw, p);
            let mut end = tok.end;
            // A family name right after it: "מר שמחה אוחיון", "הנהג אבי פרץ".
            if let Some(n) = tokens.get(i + 1) {
                if s.joined(i, i + 1)
                    && lex.surnames.contains(&n.norm)
                    && !lex.common_words.contains(&n.norm)
                    && word_bucket(&n.norm) < EVERYDAY
                    && !overlaps(n.start, n.end, reps, suspects)
                {
                    end = n.end;
                }
            }
            out.push(SuspectSpan {
                start,
                end,
                suspect: Suspect {
                    token: span_text(text, start, end)?.to_owned(),
                    kind: SuspectKind::UnknownName,
                    message: why.to_owned(),
                    suggested_role: Role::Other,
                },
                known: None,
            });
            Ok(())
        };
    let push = |i: usize, why: &str, out: &mut Vec<SuspectSpan>| push_word(i, why, false, out);
    // A rare family name joined to the word after `i`.
    let surname_after = |i: usize| {
        tokens.get(i + 1).is_some_and(|n| {
            s.joined(i, i + 1)
                && lex.surnames.contains(&n.norm)
                && !lex.common_words.contains(&n.norm)
                && word_bucket(&n.norm) < EVERYDAY
        })
    };

    for (i, tok) in tokens.iter().enumerate() {
        if taken(i, &out) {
            continue;
        }
        // "הנהג אבי פרץ", "מר שמחה אוחיון": a person word or a form of address, a word and a
        // family name.
        let addressed = i > 0 && s.joined(i - 1, i) && is_in(s.norm(i - 1), ADDRESS);
        if (s.person_before(i) || addressed)
            && surname_after(i)
            && !lex.common_words.contains(&tok.norm)
        {
            push_word(i, "שם ושם משפחה", true, &mut out)?;
            continue;
        }
        let Some((p, head)) = name_in(tok, true) else {
            continue;
        };
        let bare = p == 0;
        let next = s.next_word(i);
        let next_is = |words: &[&str]| next.is_some_and(|j| is_in(s.norm(j), words));
        // The most common words ("אלה", "שמחה", "טובה") are names here only before a verb or
        // next to another name.
        let very_common = word_bucket(&head) >= 15 && !addressed;

        // "ליבי גורדון, B.O.T.": a name and a family name opening a line or a clause.
        if bare && s.starts_clause(i) && surname_after(i) {
            push(i, "שם ושם משפחה", &mut out)?;
            continue;
        }

        // "שירה אמרה לי", "סבא מוטי מביא אותו".
        if bare && next_is(ACTION_NEXT) && (s.starts_clause(i) || s.person_before(i)) {
            push(i, "שם לפני פועל", &mut out)?;
            continue;
        }

        // "הגננות: סמאח ונוגה", "עם ים ומתן", "עם מתן ועם ג'סיקה".
        let pair_with = |j: usize, out: &[SuspectSpan]| {
            tokens
                .get(j)
                .is_some_and(|t| person_at(j, out) || name_in(t, true).is_some())
        };
        if bare && i + 1 < tokens.len() && s.joined(i, i + 1) {
            let (partner, via) = if s.norm(i + 1) == normalize("ועם") {
                (i + 2, true)
            } else {
                (i + 1, false)
            };
            let partner_ok = tokens.get(partner).is_some_and(|t| {
                (via && name_in(t, false).is_some() || !via && t.norm.starts_with('ו'))
                    && pair_with(partner, &out)
            });
            let with = i
                .checked_sub(1)
                .is_some_and(|k| s.joined(k, i) && is_in(s.norm(k), &["עם", "ועם"]));
            let anchored = with
                || (i > 0 && s.gap(i - 1, i).is_none_or(|g| g.contains(':')))
                || person_at(partner.min(tokens.len() - 1), &out)
                || lex.first_names.contains(&head);
            if partner_ok && anchored && (with || !very_common) {
                push(i, "שם ליד שם אחר", &mut out)?;
                if !person_at(partner, &out) {
                    push(partner, "שם ליד שם אחר", &mut out)?;
                }
                continue;
            }
        }
        // The second of a pair whose first is already hidden: "[גננת] ונוגה".
        if p == 1 && i > 0 && s.joined(i - 1, i) && person_at(i - 1, &out) && !very_common {
            push(i, "שם ליד שם אחר", &mut out)?;
            continue;
        }

        // "יום ההולדת של ים", "האמא של יהל".
        if bare
            && i >= 2
            && s.joined(i - 1, i)
            && s.joined(i - 2, i - 1)
            && s.norm(i - 1) == normalize("של")
            && is_in(s.norm(i - 2), OF_PERSON)
        {
            push(i, "שם אחרי \"של\"", &mut out)?;
            continue;
        }

        // "סייעת הודיה", "הגננת הקודמת שלו שולי", "המרפאה בעיסוק מרינה".
        let idiom = head == normalize("אדם")
            && i > 0
            && is_in(s.norm(i - 1), &["בן", "הבן", "בני", "לבן", "כבן", "מבן"]);
        if bare && s.person_before(i) && !very_common && !idiom && !next_is(DEGREE_NEXT) {
            push(i, "שם אחרי מילה לאדם", &mut out)?;
        }
    }
    Ok(out)
}

/// "נולד בחרקוב", "עלתה מגונדר": the place of birth or of origin, when it is not an
/// everyday word ("נולד בשבוע 36", "נולדה ברכב"). A word that cannot be cut out of the text
/// (never expected) fails the whole filtering.
pub(super) fn origin_places(
    text: &str,
    tokens: &[Token],
    reps: &[Replacement],
    suspects: &[SuspectSpan],
) -> Result<Vec<Replacement>, PrivacyError> {
    let lex = &*LEXICON;
    let s = Scan { text, tokens };
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        let (letter, verbs): (char, &[&str]) = if is_in(s.norm(i), &["נולד", "נולדה", "נולדו"])
        {
            ('ב', &[])
        } else if is_in(s.norm(i), &["עלה", "עלתה", "עלו", "עלינו"]) {
            ('מ', &["לארץ", "ארצה"])
        } else {
            continue;
        };
        let mut j = i + 1;
        if tokens.get(j).is_some_and(|t| is_in(&t.norm, verbs)) && s.joined(i, j) {
            j += 1;
        }
        let Some(t) = tokens.get(j) else { continue };
        if !s.joined(j - 1, j) || !t.norm.starts_with(letter) {
            continue;
        }
        let head: String = t.norm.chars().skip(1).collect();
        let hebrew = head.chars().all(|c| ('\u{05D0}'..='\u{05EA}').contains(&c));
        if !hebrew
            || head.chars().count() < 3
            || head.starts_with('ה')
            || lex.common_words.contains(&head)
            || word_bucket(&head) >= EVERYDAY
            || overlaps(t.start, t.end, reps, suspects)
        {
            continue;
        }
        let raw = span_text(text, t.start, t.end)?;
        out.push(Replacement {
            start: t.start + byte_len_of_letters(raw, 1),
            end: t.end,
            out: "[יישוב_אחר]".to_owned(),
            label: "יישוב".to_owned(),
            relative: false,
            declared: false,
        });
    }
    Ok(out)
}
