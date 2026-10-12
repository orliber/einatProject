//! Shared fixtures for the invariant tests. Every name is invented; identifiers that look
//! real (ID numbers, phones, e-mails) are built at run time so the repository scanner never
//! sees one in the source.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use dv_domain::{Identity, IdentitySource, Role};
use dv_privacy::patterns::is_valid_israeli_id;
use dv_privacy::text::normalize;
use dv_privacy::{clear, Blocked, ClearedPayload, FilterOutcome, GateRequest, PrivacyContext};

pub const TODAY: (i32, u32, u32) = (2026, 9, 28);
pub const CASE: &str = "case-a";
pub const OTHER: &str = "case-b";

pub fn id(case: &str, role: Role, tag: &str, value: &str, aliases: &[&str]) -> Identity {
    Identity {
        id: format!("{case}-{tag}"),
        case_id: case.to_owned(),
        role,
        tag: tag.to_owned(),
        value: value.to_owned(),
        aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
        source: IdentitySource::Manual,
        reason: String::new(),
    }
}

/// Two cases in one vault. The names are chosen to be no everyday Hebrew word, so a
/// sentence around them has one reading only.
pub fn identities() -> Vec<Identity> {
    vec![
        id(CASE, Role::Child, "[ילד]", "נועם", &["Noam"]),
        id(CASE, Role::Mother, "[אם]", "רותם", &[]),
        id(CASE, Role::Father, "[אב]", "עידו", &[]),
        id(CASE, Role::Teacher, "[גננת]", "אביטל", &[]),
        id(CASE, Role::Doctor, "[רופא_1]", "ד\"ר אבנר שטרן", &[]),
        id(OTHER, Role::Child, "[ילד]", "ינאי", &[]),
        id(OTHER, Role::Mother, "[אם]", "אסנת", &[]),
    ]
}

/// Single-word declared names of `CASE` that restore to themselves, with their tags.
pub const CASE_NAMES: &[(&str, &str)] = &[
    ("נועם", "[ילד]"),
    ("רותם", "[אם]"),
    ("עידו", "[אב]"),
    ("אביטל", "[גננת]"),
];

pub fn ctx<'a>(ids: &'a [Identity], case: &'a str) -> PrivacyContext<'a> {
    PrivacyContext {
        case_id: case,
        identities: ids,
        practitioner: &[],
        allowlisted: &|_: &str| false,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: TODAY,
    }
}

pub fn case_tags(ids: &[Identity], case: &str) -> HashSet<String> {
    ids.iter()
        .filter(|i| i.case_id == case)
        .map(|i| i.tag.clone())
        .collect()
}

/// The gate on one user message, with this case's tags and generous limits.
pub fn gate_text(text: &str, ids: &[Identity], case: &str) -> Result<ClearedPayload, Blocked> {
    let body = serde_json::json!({
        "model": "claude-test",
        "system": "עוזרת לכתיבת דוח. השתמשי בתגיות כפי שהן.",
        "messages": [{"role": "user", "content": text}],
    });
    gate_body(&body, ids, case)
}

pub fn gate_body(
    body: &serde_json::Value,
    ids: &[Identity],
    case: &str,
) -> Result<ClearedPayload, Blocked> {
    let c = ctx(ids, case);
    let tags = case_tags(ids, case);
    clear(&GateRequest {
        body,
        ctx: &c,
        case_tags: &tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 1_000_000,
    })
}

/// The tagged text and its segments describe the same outcome.
pub fn assert_segments_consistent(input: &str, out: &FilterOutcome) {
    let tagged: String = out
        .tagged_segments
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(
        tagged, out.tagged,
        "tagged segments vs tagged for «{input}»"
    );
    if !input.contains(['[', ']']) {
        let original: String = out
            .original_segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(original, input, "original segments must rebuild the input");
    }
}

/// A valid Israeli ID number with the given first eight digits (check digit computed).
pub fn valid_id(first8: &str) -> String {
    assert_eq!(first8.len(), 8);
    (0..10)
        .map(|d| format!("{first8}{d}"))
        .find(|s| is_valid_israeli_id(s))
        .expect("exactly one check digit fits")
}

/// A mobile number, assembled so no phone literal sits in the source.
pub fn mobile(prefix_digit: u8, rest: &str) -> String {
    assert_eq!(rest.len(), 7);
    ["05", &prefix_digit.to_string(), rest].concat()
}

/// An invented e-mail address, assembled at run time.
pub fn email(local: &str, domain: &str) -> String {
    [local, "@", domain].concat()
}

/// Replace ASCII digits with the digits of another script, starting at `zero`.
pub fn shift_digits(s: &str, zero: u32) -> String {
    s.chars()
        .map(|c| match c.to_digit(10) {
            Some(d) if c.is_ascii_digit() => char::from_u32(zero + d).unwrap(),
            _ => c,
        })
        .collect()
}

/// `needle` (normalized) is in `haystack` (normalized): the strictest leak check.
pub fn contains_norm(haystack: &str, needle: &str) -> bool {
    normalize(haystack).contains(&normalize(needle))
}

/// The ASCII digits of `s` in order.
pub fn digits(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}
