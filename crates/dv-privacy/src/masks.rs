//! The mask book of a case (the plan's "פנקס הסתרות"): what each numbered tag of a number, a
//! date, an address or a place stands for, so the answer comes back with the words she wrote
//! and never with square brackets in it.
//!
//! The same words get the same tag in every request of the case (`[תאריך_2]` is always that
//! date), and a tag carries nothing but its kind and a running number. The originals stay in
//! the vault, encrypted with the case; nothing of them goes out.

use std::cell::RefCell;
use std::sync::LazyLock;

pub use dv_domain::MaskEntry;
use regex::Regex;

use crate::text::normalize;

/// The kinds a numbered tag may name. None of them is the tag base of a role (`dv-domain`),
/// so a numbered tag is never read as a person's or a place's tag, and back.
pub const MASK_BASES: &[&str] = &[
    "מספר_זהות",
    "טלפון",
    "דואל",
    "קישור",
    "מספר",
    "תאריך",
    "שנה",
    "כתובת",
    "מקום",
    "בית_חולים",
    // A name the gate refused in text the filter never saw (round 25's fix loop).
    "שם",
    // Her own words the outgoing filter leaves out or generalizes, kept in a stored text of
    // hers (D-126). Put back before every request, so the model never sees one.
    KEPT_WORDS,
];

/// The base of a kept-words tag (D-126): words of hers stored under a number, never sent.
pub const KEPT_WORDS: &str = "מילים";

/// How every kept-words tag starts.
pub(crate) const KEPT_WORDS_OPEN: &str = "[מילים_";

/// The kind of a fixed generic tag the filter writes, or `None` for any other text.
#[must_use]
pub fn mask_base(fixed_tag: &str) -> Option<&'static str> {
    Some(match fixed_tag {
        "[ת.ז.]" => "מספר_זהות",
        "[טלפון]" => "טלפון",
        "[דוא\"ל]" => "דואל",
        "[קישור]" => "קישור",
        "[מספר]" => "מספר",
        "[תאריך]" => "תאריך",
        "[שנה]" => "שנה",
        "[כתובת]" => "כתובת",
        "[יישוב_אחר]" => "מקום",
        "[בית_חולים]" => "בית_חולים",
        _ => return None,
    })
}

/// `[base_N]` with a mask base and a number of 1 to 4 digits.
#[must_use]
pub fn is_mask_tag(tag: &str) -> bool {
    let Some(inner) = tag.strip_prefix('[').and_then(|t| t.strip_suffix(']')) else {
        return false;
    };
    let Some((base, n)) = inner.rsplit_once('_') else {
        return false;
    };
    MASK_BASES.contains(&base)
        && (1..=4).contains(&n.len())
        && n.bytes().all(|b| b.is_ascii_digit())
        && !n.starts_with('0')
}

/// Any square-bracketed run of up to 40 characters on one line: what may be a tag. The gate,
/// the fix loop, the restore step and the bracket check of the filter all read tags with it.
pub(crate) static TAG_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\[[^\[\]\n]{1,40}\]").ok());

/// A tag the filter writes by itself, numbered or not: it names a kind, never a person.
#[must_use]
pub fn is_generic_tag(tag: &str) -> bool {
    crate::gate::GENERIC_TAGS.contains(&tag) || is_mask_tag(tag)
}

/// The case's book while one request is built: what the vault keeps, and what this request
/// adds. Filtering several texts of one request with one book gives one tag to the same words
/// in all of them.
#[derive(Debug, Default)]
pub struct MaskBook {
    entries: RefCell<Vec<MaskEntry>>,
    kept: usize,
}

impl MaskBook {
    #[must_use]
    pub fn new(kept: Vec<MaskEntry>) -> Self {
        let n = kept.len();
        Self {
            entries: RefCell::new(kept),
            kept: n,
        }
    }

    /// The tag of `original` as a `base`: the one it already has, or the next number.
    #[must_use]
    pub fn tag_for(&self, base: &str, original: &str) -> String {
        let norm = normalize(original);
        let mut entries = self.entries.borrow_mut();
        if let Some(e) = entries
            .iter()
            .find(|e| e.kind == base && normalize(&e.original) == norm)
        {
            return e.tag.clone();
        }
        let prefix = format!("[{base}_");
        let next = entries
            .iter()
            .filter_map(|e| {
                e.tag
                    .strip_prefix(&prefix)?
                    .strip_suffix(']')?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let tag = format!("{prefix}{next}]");
        entries.push(MaskEntry {
            tag: tag.clone(),
            kind: base.to_owned(),
            original: original.to_owned(),
        });
        tag
    }

    /// The kept-words tag of exactly these words (D-126), as she wrote them: niqqud, spacing
    /// and quotes included, so restoring gives back the very same text.
    #[must_use]
    pub fn kept_words_tag(&self, original: &str) -> String {
        if let Some(e) = self
            .entries
            .borrow()
            .iter()
            .find(|e| e.kind == KEPT_WORDS && e.original == original)
        {
            return e.tag.clone();
        }
        let prefix = format!("[{KEPT_WORDS}_");
        let next = self
            .entries
            .borrow()
            .iter()
            .filter_map(|e| {
                e.tag
                    .strip_prefix(&prefix)?
                    .strip_suffix(']')?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let tag = format!("{prefix}{next}]");
        self.entries.borrow_mut().push(MaskEntry {
            tag: tag.clone(),
            kind: KEPT_WORDS.to_owned(),
            original: original.to_owned(),
        });
        tag
    }

    /// The words a kept-words tag stands for, if the book has it.
    #[must_use]
    pub fn kept_words_original(&self, tag: &str) -> Option<String> {
        self.entries
            .borrow()
            .iter()
            .find(|e| e.kind == KEPT_WORDS && e.tag == tag)
            .map(|e| e.original.clone())
    }

    /// What this request added, to keep with the case.
    #[must_use]
    pub fn added(&self) -> Vec<MaskEntry> {
        self.entries
            .borrow()
            .iter()
            .skip(self.kept)
            .cloned()
            .collect()
    }

    /// Every entry, kept and added.
    #[must_use]
    pub fn entries(&self) -> Vec<MaskEntry> {
        self.entries.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_words_keep_their_tag_and_new_ones_get_the_next_number() {
        let book = MaskBook::new(vec![MaskEntry {
            tag: "[תאריך_4]".into(),
            kind: "תאריך".into(),
            original: "12.3.2020".into(),
        }]);
        assert_eq!(book.tag_for("תאריך", "12.3.2020"), "[תאריך_4]");
        assert_eq!(book.tag_for("תאריך", "1.1.2021"), "[תאריך_5]");
        assert_eq!(book.tag_for("מספר", "1.1.2021"), "[מספר_1]");
        assert_eq!(book.tag_for("תאריך", "1.1.2021"), "[תאריך_5]");
        let added: Vec<String> = book.added().into_iter().map(|e| e.tag).collect();
        assert_eq!(added, ["[תאריך_5]", "[מספר_1]"]);
    }

    #[test]
    fn a_numbered_tag_is_known_by_its_form_only() {
        assert!(is_mask_tag("[מספר_3]") && is_mask_tag("[בית_חולים_12]"));
        assert!(!is_mask_tag("[מספר]") && !is_mask_tag("[מספר_03]") && !is_mask_tag("[ילד_2]"));
        assert!(!is_mask_tag("[מספר_12345]") && !is_mask_tag("[מספר_x]") && !is_mask_tag("מספר_3"));
        assert!(is_generic_tag("[תאריך]") && is_generic_tag("[תאריך_1]"));
        for role in dv_domain::Role::ALL {
            assert!(
                !MASK_BASES.contains(&role.tag_base()),
                "{}",
                role.tag_base()
            );
        }
        for base in MASK_BASES {
            assert!(is_mask_tag(&format!("[{base}_1]")), "{base}");
        }
    }
}
