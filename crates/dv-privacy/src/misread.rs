//! Scanned text (OCR) can misread a declared name by one letter ("אלוו" for "אלון"). The
//! near-spelling rule only forgives weak letters, so for OCR-sourced text alone any one-letter
//! change to one of this case's names counts as the name (D-048). Only for OCR text, to keep
//! false hides down in typed documents.

use crate::lexicon::{word_bucket, LEXICON};
use crate::pipeline::{is_generic, is_title, PrivacyContext};
use crate::text::{edit_distance, normalize, prefix_splits, tokenize};

/// A word in scanned text one letter away from a name in this case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misread {
    /// As it is written in the text, without a Hebrew prefix ("אלוו" in "ואלוו").
    pub written: String,
    /// The tag of the name it is close to.
    pub tag: String,
}

/// Everyday words ("אלוף" next to "אלון") are read as words: the misread then shows nothing
/// of the name. Same threshold as the near-spelling rule's "known word".
fn known_word(norm: &str) -> bool {
    !LEXICON.first_names.contains(norm)
        && (word_bucket(norm) >= 7 || LEXICON.common_words.contains(norm))
}

/// Every word of `text` (OCR output) one edit away from a name of this case: a letter
/// replaced, added or dropped, whichever letter it is.
#[must_use]
pub fn ocr_misreads(text: &str, ctx: &PrivacyContext<'_>) -> Vec<Misread> {
    let names: Vec<(String, &str)> = ctx
        .identities
        .iter()
        .filter(|i| i.case_id == ctx.case_id)
        .flat_map(|i| {
            std::iter::once(&i.value)
                .chain(i.aliases.iter())
                .flat_map(move |name| {
                    normalize(name)
                        .split(' ')
                        .filter(|w| w.chars().count() >= 3 && !is_title(w) && !is_generic(w))
                        .map(|w| (w.to_owned(), i.tag.as_str()))
                        .collect::<Vec<_>>()
                })
        })
        .collect();
    let mut out: Vec<Misread> = Vec::new();
    for tok in tokenize(text) {
        if (ctx.allowlisted)(&tok.norm) || known_word(&tok.norm) {
            continue;
        }
        let raw = &text[tok.start..tok.end];
        let hit = prefix_splits(&tok.norm).into_iter().find_map(|(p, h)| {
            if h.chars().count() < 3 || known_word(&h) || (ctx.allowlisted)(&h) {
                return None;
            }
            names
                .iter()
                .find(|(w, _)| edit_distance(&h, w, 1) == 1)
                .map(|(_, tag)| (p, (*tag).to_owned()))
        });
        let Some((p, tag)) = hit else { continue };
        let written = raw[crate::text::byte_len_of_letters(raw, p)..].to_owned();
        if !out.iter().any(|m| m.written == written && m.tag == tag) {
            out.push(Misread { written, tag });
        }
    }
    out
}
