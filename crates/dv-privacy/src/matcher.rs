//! Multi-word phrase matching over tokens, with Hebrew prefixes on the first word.

use std::collections::HashMap;

use crate::text::{byte_len_of_letters, normalize, prefix_splits, Token};

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
        while i < tokens.len() {
            let mut best: Option<(usize, usize, T)> = None; // (words, prefix letters, payload)
            for (prefix, head) in prefix_splits(&tokens[i].norm) {
                let Some(candidates) = self.by_first.get(&head) else {
                    continue;
                };
                for (words, payload) in candidates {
                    let n = words.len();
                    if i + n > tokens.len() {
                        continue;
                    }
                    let rest_matches = words[1..]
                        .iter()
                        .zip(&tokens[i + 1..i + n])
                        .all(|(w, t)| *w == t.norm);
                    let better = match &best {
                        None => true,
                        Some((bn, bp, _)) => n > *bn || (n == *bn && prefix < *bp),
                    };
                    if rest_matches && better {
                        best = Some((n, prefix, payload.clone()));
                    }
                }
            }
            if let Some((n, prefix, payload)) = best {
                let tok = &tokens[i];
                let start = tok.start + byte_len_of_letters(&text[tok.start..tok.end], prefix);
                out.push(PhraseMatch {
                    start,
                    end: tokens[i + n - 1].end,
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

#[cfg(test)]
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
