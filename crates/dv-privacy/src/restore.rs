//! Local-only: put real names back for display and export, and check model output.

use std::sync::LazyLock;

use dv_domain::{Identity, PRACTITIONER_TAG};
use regex::Regex;

use crate::lexicon::LEXICON;
use crate::pipeline::{Suspect, SuspectKind};
use crate::text::{prefix_splits, tokenize};

static TAG_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\[[^\[\]\n]{1,40}\]").ok());

/// Replace the case's tags with the declared names. Prefixes stay attached ("ל[ילד]" → "לנועם").
#[must_use]
pub fn restore(tagged: &str, identities: &[Identity], practitioner: Option<&str>) -> String {
    let Some(re) = TAG_RE.as_ref() else {
        return tagged.to_owned();
    };
    re.replace_all(tagged, |caps: &regex::Captures<'_>| {
        let tag = &caps[0];
        if tag == PRACTITIONER_TAG {
            if let Some(p) = practitioner {
                return p.to_owned();
            }
        }
        identities
            .iter()
            .find(|i| i.tag == tag)
            .map_or_else(|| tag.to_owned(), |i| i.value.clone())
    })
    .into_owned()
}

/// Identity-like tags left in text (everything but `[חסר: …]` notes).
#[must_use]
pub fn remaining_tags(text: &str) -> Vec<String> {
    let Some(re) = TAG_RE.as_ref() else {
        return Vec::new();
    };
    re.find_iter(text)
        .map(|m| m.as_str().to_owned())
        .filter(|t| !t.starts_with("[חסר"))
        .collect()
}

/// Check what the model wrote: names it should not know, and tags that are not this case's.
#[must_use]
pub fn scan_model_output(text: &str, case_tags: &[String]) -> Vec<Suspect> {
    let mut out = Vec::new();
    for t in tokenize(text) {
        if prefix_splits(&t.norm)
            .iter()
            .any(|(_, h)| LEXICON.first_names.contains(h))
        {
            out.push(Suspect {
                token: text[t.start..t.end].to_owned(),
                kind: SuspectKind::UnknownName,
                message: "Claude כתב שם שלא הופיע בבקשה".to_owned(),
                suggested_role: dv_domain::Role::Other,
            });
        }
    }
    for tag in remaining_tags(text) {
        let generic = tag.ends_with("_אחר]")
            || [
                "[ת.ז.]",
                "[טלפון]",
                "[מספר]",
                "[תאריך]",
                "[שנה]",
                "[כתובת]",
                "[בית_חולים]",
                PRACTITIONER_TAG,
            ]
            .contains(&tag.as_str());
        if !case_tags.contains(&tag) && !generic {
            out.push(Suspect {
                token: tag,
                kind: SuspectKind::OtherCaseIdentity,
                message: "תגית שלא קיימת בתיק".to_owned(),
                suggested_role: dv_domain::Role::Other,
            });
        }
    }
    out
}
