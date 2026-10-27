//! Local-only: put real names back for display and export, and check model output.

use std::borrow::Cow;

use dv_domain::{Identity, PRACTITIONER_TAG};

use crate::lexicon::LEXICON;
use crate::masks::{is_generic_tag, is_kept_words_tag, MaskEntry, KEPT_WORDS, TAG_RE};
use crate::patterns::{self, PatternKind};
use crate::pipeline::{Suspect, SuspectKind};
use crate::text::{normalize, prefix_splits, tokenize};

/// Replace the case's tags with the declared names, and each numbered tag of its mask book
/// with the words it hid. Prefixes stay attached ("ל[ילד]" → "לנועם").
///
/// A kept-words tag (D-126) stays as it is: it is put back only in her own paragraph
/// ([`restore_hers`]). Anywhere else (an answer, a note, a message) it came from no rule that
/// hid it, so the words it stands for, shown there and then edited, would go out with no rule
/// to hide them again.
#[must_use]
pub fn restore(
    tagged: &str,
    identities: &[Identity],
    practitioner: Option<&str>,
    masks: &[MaskEntry],
) -> String {
    put_back(tagged, identities, practitioner, masks, false)
}

/// [`restore`] for a paragraph she wrote or edited (`Author::User`): her kept words too, as
/// she wrote them.
#[must_use]
pub fn restore_hers(
    tagged: &str,
    identities: &[Identity],
    practitioner: Option<&str>,
    masks: &[MaskEntry],
) -> String {
    put_back(tagged, identities, practitioner, masks, true)
}

fn put_back(
    tagged: &str,
    identities: &[Identity],
    practitioner: Option<&str>,
    masks: &[MaskEntry],
    hers: bool,
) -> String {
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
        if let Some(i) = identities.iter().find(|i| i.tag == tag) {
            return i.value.clone();
        }
        masks
            .iter()
            .filter(|m| hers || m.kind != KEPT_WORDS)
            .find(|m| m.tag == tag)
            .map_or_else(|| tag.to_owned(), |m| m.original.clone())
    })
    .into_owned()
}

/// Every kept-words tag in text that came from outside (a model's answer) written as plain
/// words, "[מילים_1]" as "(מילים_1)", so it can never stand for her words anywhere: not when
/// it is shown, nor in a paragraph of hers once she edits it. Same length.
#[must_use]
pub fn plain_kept_words(text: &str) -> Cow<'_, str> {
    // Without the tag pattern (never expected) every opening of one is made plain.
    let Some(re) = TAG_RE.as_ref() else {
        return Cow::Owned(text.replace(crate::masks::KEPT_WORDS_OPEN, "(מילים_"));
    };
    re.replace_all(text, |caps: &regex::Captures<'_>| {
        let tag = &caps[0];
        match tag
            .strip_prefix('[')
            .and_then(|t| t.strip_suffix(']'))
            .filter(|_| is_kept_words_tag(tag))
        {
            Some(inner) => format!("({inner})"),
            None => tag.to_owned(),
        }
    })
}

/// Her own stored paragraph (D-126) back as she wrote it, to be filtered again before it is
/// sent: the filter then reads her words in their own context, as when she typed them, and
/// hides exactly what it hid then. Only for a paragraph she wrote or edited, whose kept-words
/// tags came from [`crate::filter_to_keep`]: in any other text (an answer, a material, a
/// message) such a tag is never put back, since out of its place a kept word loses the rule
/// that hid it. Text without a kept-words tag is returned as stored. `identities`,
/// `practitioner` and `masks` are the case's, as for [`restore`].
#[must_use]
pub fn her_text_to_send(
    stored: &str,
    identities: &[Identity],
    practitioner: Option<&str>,
    masks: &[MaskEntry],
) -> String {
    if stored.contains(crate::masks::KEPT_WORDS_OPEN) {
        restore_hers(stored, identities, practitioner, masks)
    } else {
        stored.to_owned()
    }
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
        // As written; in its normalized form if it cannot be read so (never expected), still
        // flagged.
        let token = text
            .get(t.start..t.end)
            .map_or_else(|| t.norm.clone(), str::to_owned);
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
                // Still flagged, by its kind, if it cannot be cut out (never expected).
                text.get(hit.start..hit.end)
                    .map_or_else(|| hit.kind.label_he().to_owned(), str::to_owned),
                SuspectKind::Indirect,
                "Claude כתב פרט מזהה",
            );
        }
    }
    for tag in remaining_tags(text) {
        let generic = tag.ends_with("_אחר]") || is_generic_tag(&tag);
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
            source: dv_domain::IdentitySource::Manual,
            reason: String::new(),
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

    #[test]
    fn her_kept_words_come_back_only_in_her_own_paragraph() {
        let masks = vec![MaskEntry {
            tag: "[מילים_1]".into(),
            kind: KEPT_WORDS.into(),
            original: "\"זרזירים\"".into(),
        }];
        let text = "שמח עם [מילים_1] בגן.";
        assert_eq!(restore(text, &[], None, &masks), text);
        assert_eq!(
            restore_hers(text, &[], None, &masks),
            "שמח עם \"זרזירים\" בגן."
        );
        assert_eq!(plain_kept_words(text), "שמח עם (מילים_1) בגן.");
        // Any other tag stays a tag.
        assert_eq!(plain_kept_words("[ילד] ו[מילים_0]"), "[ילד] ו[מילים_0]");
    }
}
