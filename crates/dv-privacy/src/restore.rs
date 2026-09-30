//! Local-only: put real names back for display and export, and check model output.

use std::sync::LazyLock;

use dv_domain::{Identity, PRACTITIONER_TAG};
use regex::Regex;

use crate::gate::GENERIC_TAGS;
use crate::lexicon::LEXICON;
use crate::patterns::{self, PatternKind};
use crate::pipeline::{Suspect, SuspectKind};
use crate::text::{normalize, prefix_splits, tokenize};

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

/// Check what the model wrote: names it should not know, identifiers, and tags that are not
/// this case's. `identities` are the declared people of every case: the model only ever saw
/// tags, so a real declared name in its answer is a sign that one slipped through.
#[must_use]
pub fn scan_model_output(
    text: &str,
    case_tags: &[String],
    identities: &[Identity],
) -> Vec<Suspect> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |out: &mut Vec<Suspect>, token: String, kind: SuspectKind, message: &str| {
        if seen.insert(token.clone()) {
            out.push(Suspect {
                token,
                kind,
                message: message.to_owned(),
                suggested_role: dv_domain::Role::Other,
            });
        }
    };
    let declared: Vec<String> = identities
        .iter()
        .flat_map(|i| std::iter::once(&i.value).chain(i.aliases.iter()))
        .flat_map(|v| {
            normalize(v)
                .split(' ')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|w| w.chars().count() >= 2 && !LEXICON.common_words.contains(w))
        .collect();
    for t in tokenize(text) {
        let splits = prefix_splits(&t.norm);
        let token = text[t.start..t.end].to_owned();
        if splits.iter().any(|(_, h)| declared.iter().any(|d| d == h)) {
            add(
                &mut out,
                token,
                SuspectKind::OtherCaseIdentity,
                "שם אמיתי מהכספת מופיע בתשובה של Claude",
            );
        } else if splits.iter().any(|(_, h)| LEXICON.first_names.contains(h)) {
            add(
                &mut out,
                token,
                SuspectKind::UnknownName,
                "Claude כתב שם שלא הופיע בבקשה",
            );
        }
    }
    for hit in patterns::find(text, (2000, 1, 1)).unwrap_or_default() {
        if matches!(
            hit.kind,
            PatternKind::IdNumber | PatternKind::Phone | PatternKind::Email | PatternKind::Address
        ) {
            add(
                &mut out,
                text[hit.start..hit.end].to_owned(),
                SuspectKind::Indirect,
                "Claude כתב פרט מזהה",
            );
        }
    }
    for tag in remaining_tags(text) {
        let generic = tag.ends_with("_אחר]") || GENERIC_TAGS.contains(&tag.as_str());
        if !case_tags.contains(&tag) && !generic {
            add(
                &mut out,
                tag,
                SuspectKind::OtherCaseIdentity,
                "תגית שלא קיימת בתיק",
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dv_domain::Role;

    #[test]
    fn a_declared_name_an_identifier_or_a_foreign_tag_in_the_answer_is_pointed_out() {
        let ids = vec![Identity {
            id: "1".into(),
            case_id: "c".into(),
            role: Role::Child,
            tag: "[ילד]".into(),
            value: "ליאור".into(),
            aliases: vec![],
        }];
        let text = concat!(
            "[ילד] שיחק עם לליאור ועם אביגיל. טלפון 052-",
            "1234567. [גננת_7] ציינה [מספר]."
        );
        let found = scan_model_output(text, &["[ילד]".to_owned()], &ids);
        let tokens: Vec<&str> = found.iter().map(|s| s.token.as_str()).collect();
        assert!(tokens.contains(&"לליאור"), "{tokens:?}");
        assert!(tokens.contains(&"אביגיל"), "{tokens:?}");
        assert!(tokens.iter().any(|t| t.contains("1234567")), "{tokens:?}");
        assert!(tokens.contains(&"[גננת_7]"), "{tokens:?}");
        assert!(!tokens.contains(&"[מספר]") && !tokens.contains(&"[ילד]"));
    }
}
