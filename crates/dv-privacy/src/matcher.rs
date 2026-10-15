//! Multi-word phrase matching over tokens, with Hebrew prefixes on the first word.

use std::collections::HashMap;

use crate::text::{byte_len_of_letters, normalize, normalize_char, prefix_splits, Token};

/// A phrase occurrence. `start` is after any prefix letters, so "ל" in "לנועם" stays in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhraseMatch<T> {
    pub start: usize,
    pub end: usize,
    pub first_token: usize,
    pub last_token: usize,
    /// Prefix letters split off the first word (0 = the word itself matched).
    pub prefix: usize,
    pub payload: T,
}

#[derive(Debug, Clone)]
pub struct PhraseIndex<T> {
    by_first: HashMap<String, Vec<(Vec<String>, T)>>,
}

impl<T> Default for PhraseIndex<T> {
    fn default() -> Self {
        Self {
            by_first: HashMap::new(),
        }
    }
}

impl<T: Clone> PhraseIndex<T> {
    /// Add a phrase given as raw text; it is normalized and split on spaces.
    pub fn insert(&mut self, phrase: &str, payload: T) {
        let words: Vec<String> = normalize(phrase)
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect();
        self.insert_words(words, payload);
    }

    pub fn insert_words(&mut self, words: Vec<String>, payload: T) {
        let Some(first) = words.first().cloned() else {
            return;
        };
        let entry = self.by_first.entry(first).or_default();
        if !entry.iter().any(|(w, _)| w == &words) {
            entry.push((words, payload));
        }
    }

    /// Leftmost-longest, non-overlapping matches.
    #[must_use]
    pub fn find(&self, text: &str, tokens: &[Token]) -> Vec<PhraseMatch<T>> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(tok) = tokens.get(i) {
            // (words, prefix letters, bytes of a closing quote left out of the match, payload,
            // end of the last word)
            let mut best: Option<(usize, usize, usize, T, usize)> = None;
            for (prefix, head) in prefix_splits(&tok.norm) {
                let opened = quote_opened(text, tok, prefix);
                let (bare, quoted) = strip_quote_tail(&head, opened);
                let keys = std::iter::once((head.as_str(), false))
                    .chain((quoted > 0).then_some((bare, true)));
                for (key, stripped) in keys {
                    let Some(candidates) = self.by_first.get(key) else {
                        continue;
                    };
                    for (words, payload) in candidates {
                        let n = words.len();
                        // The tokens the phrase would cover, from this one on.
                        let Some(window) = tokens.get(i..i + n) else {
                            continue;
                        };
                        // A quote closes the word it follows: a stripped first word ends the phrase.
                        if stripped && n > 1 {
                            continue;
                        }
                        let Some(last) = window.last() else {
                            continue;
                        };
                        let middle = words
                            .iter()
                            .zip(window)
                            .take(n.saturating_sub(1))
                            .skip(1)
                            .all(|(w, t)| *w == t.norm);
                        let tail = match (n, stripped) {
                            (1, false) => Some(0),
                            (1, true) => Some(quote_bytes(text, tok, quoted)),
                            _ => words
                                .last()
                                .and_then(|w| last_word_tail(text, last, w, opened)),
                        };
                        let Some(tail) = tail.filter(|_| middle) else {
                            continue;
                        };
                        let better = match &best {
                            None => true,
                            Some((bn, bp, bt, _, _)) => {
                                n > *bn
                                    || (n == *bn && (prefix < *bp || (prefix == *bp && tail < *bt)))
                            }
                        };
                        if better {
                            best = Some((n, prefix, tail, payload.clone(), last.end));
                        }
                    }
                }
            }
            if let Some((n, prefix, tail, payload, last_end)) = best {
                // A token that cannot be read (never expected) is matched whole, prefix and all.
                let start = tok.start
                    + text
                        .get(tok.start..tok.end)
                        .map_or(0, |w| byte_len_of_letters(w, prefix));
                out.push(PhraseMatch {
                    start,
                    end: last_end - tail,
                    first_token: i,
                    last_token: i + n - 1,
                    prefix,
                    payload,
                });
                i += n;
            } else {
                i += 1;
            }
        }
        out
    }
}

/// A normalized word without a closing quote or an English possessive glued to it, and how
/// many characters that took off (0: none). The tokenizer keeps a final geresh in a word, as
/// names such as "ג'ורג'" need, and a closing curly quote too, so a name in quotes ("'נועם'",
/// "“נועם”", "ל'נועם'") or "Noam's" was not matched at all. A final geresh is a closing
/// quote only when a quote `opened` before the word: "טל'" (phone) and "גב'" stay as written.
pub(crate) fn strip_quote_tail(norm: &str, opened: bool) -> (&str, usize) {
    let long_enough = |b: &&str| b.chars().count() >= 2;
    if let Some(bare) = norm.strip_suffix("'s").filter(long_enough) {
        return (bare, 2);
    }
    if let Some(bare) = norm.strip_suffix('"').filter(long_enough) {
        return (bare, 1);
    }
    match norm.strip_suffix('\'').filter(long_enough) {
        Some(bare) if opened => (bare, 1),
        _ => (norm, 0),
    }
}

/// A quote opens right before the word: before the token, or between its prefix letters and
/// the rest ("ל'נועם'", split as ל + ' + נועם'). Text that cannot be read before the word
/// (never expected) counts as a quote: the word is then also tried without its last quote.
pub(crate) fn quote_opened(text: &str, tok: &Token, prefix: usize) -> bool {
    let is_quote = |c: char| matches!(normalize_char(c), Some('\'' | '"'));
    text.get(..tok.start)
        .is_none_or(|before| before.chars().next_back().is_some_and(is_quote))
        || prefix
            .checked_sub(1)
            .and_then(|k| tok.norm.chars().nth(k))
            .is_some_and(is_quote)
}

/// Bytes of the last `chars` characters of the token as written; 0 for a token that cannot
/// be read (never expected), so the match keeps the whole token.
pub(crate) fn quote_bytes(text: &str, tok: &Token, chars: usize) -> usize {
    text.get(tok.start..tok.end)
        .map_or(0, |w| w.chars().rev().take(chars).map(char::len_utf8).sum())
}

/// How the last word of a phrase matches its token: exactly (0), or with a closing quote
/// left out (its bytes).
fn last_word_tail(text: &str, tok: &Token, word: &str, opened: bool) -> Option<usize> {
    if tok.norm == word {
        return Some(0);
    }
    match strip_quote_tail(&tok.norm, opened) {
        (bare, n) if n > 0 && bare == word => Some(quote_bytes(text, tok, n)),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::string_slice)]
mod tests {
    use super::*;
    use crate::text::tokenize;

    #[test]
    fn matches_with_prefix_and_multiword_phrases() {
        let mut idx = PhraseIndex::default();
        idx.insert("נועם", "child");
        idx.insert("גן הדקל", "kindergarten");
        let text = "ולנועם יש חבר בגן הדקל.";
        let m = idx.find(text, &tokenize(text));
        assert_eq!(m.len(), 2);
        assert_eq!(&text[m[0].start..m[0].end], "נועם");
        assert_eq!(&text[m[1].start..m[1].end], "גן הדקל");
        assert_eq!(m[1].payload, "kindergarten");
    }

    #[test]
    fn longest_phrase_wins() {
        let mut idx = PhraseIndex::default();
        idx.insert("אבנר", 1);
        idx.insert("אבנר שטרן", 2);
        let text = "הגיע אבנר שטרן";
        let m = idx.find(text, &tokenize(text));
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].payload, 2);
    }

    #[test]
    fn no_match_inside_other_words() {
        let mut idx = PhraseIndex::default();
        idx.insert("טל", 1);
        let text = "מטלה וטלפון";
        assert!(idx.find(text, &tokenize(text)).is_empty());
    }
}
