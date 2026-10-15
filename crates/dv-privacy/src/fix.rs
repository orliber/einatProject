//! One step of the automatic fix loop (round 25): what the gate refused in a request is hidden
//! in the outgoing copy, and the copy goes back to the gate, which is not changed and has no
//! way around it. Only spans are replaced, each with a tag that names a kind or this case's
//! book, so a fix can only take words out. What cannot be fixed this way (a canary, an
//! internal error, a request too large, a refused object key) stays refused.
//!
//! - A name (declared, from the lexicon, from her past reports) → `[שם_N]` of the case's book.
//! - A pattern (number, date, phone, address field…) → the numbered tag of its kind.
//! - A tag that is not the case's → its plain word (`[מוסד_1]` → "מוסד"), checked again.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::gate::{name_findings, tag_word, GateRequest};
use crate::masks::{is_generic_tag, MaskBook};
use crate::patterns;

/// The codes a fix can answer. Any other code in a refusal leaves it refused.
pub const FIXABLE_CODES: &[&str] = &[
    "identity",
    "unknown_name",
    "past_report_name",
    "pattern",
    "foreign_tag",
];

static TAG_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\[[^\[\]\n]{1,40}\]").ok());

/// One thing the fix hid: the words, what went out instead, and what kind they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedItem {
    pub original: String,
    pub tag: String,
    pub label: String,
}

/// The request with what the gate refused hidden, and what was hidden. `None`: nothing in its
/// string values could be fixed (the refusal stands).
#[must_use]
pub fn fix_refused(req: &GateRequest<'_>, book: &MaskBook) -> Option<(Value, Vec<FixedItem>)> {
    let mut items = Vec::new();
    let mut body = req.body.clone();
    fix_value(&mut body, req, book, &mut items);
    (!items.is_empty()).then_some((body, items))
}

fn fix_value(v: &mut Value, req: &GateRequest<'_>, book: &MaskBook, items: &mut Vec<FixedItem>) {
    match v {
        Value::String(s) => {
            if let Some(fixed) = fix_text(s, req, book, items) {
                *s = fixed;
            }
        }
        Value::Array(list) => list.iter_mut().for_each(|x| fix_value(x, req, book, items)),
        // Keys are the request's own fields and the schema's names: never rewritten. A key
        // the gate refuses keeps the request refused.
        Value::Object(map) => map
            .values_mut()
            .for_each(|x| fix_value(x, req, book, items)),
        _ => {}
    }
}

/// A span to replace: start, end, what goes out, and its kind for the list.
type Span = (usize, usize, String, String);

fn fix_text(
    text: &str,
    req: &GateRequest<'_>,
    book: &MaskBook,
    items: &mut Vec<FixedItem>,
) -> Option<String> {
    let mut spans: Vec<Span> = Vec::new();
    let words = |s: usize, e: usize| text.get(s..e).map(str::to_owned);
    for f in name_findings(text, req.ctx) {
        if let Some(w) = words(f.start, f.end) {
            spans.push((f.start, f.end, book.tag_for("שם", &w), "שם".to_owned()));
        }
    }
    // A text the scan cannot read is left as it is: the gate refuses it again ("internal").
    for h in patterns::find(text, req.ctx.today).unwrap_or_default() {
        if let Some(w) = words(h.start, h.end) {
            spans.push((
                h.start,
                h.end,
                book.tag_for(h.kind.mask_base(), &w),
                h.kind.label_he().to_owned(),
            ));
        }
    }
    for (start, end) in crate::pipeline::address_field_spans(text) {
        let tagged = text
            .get(start..end)
            .is_some_and(|part| TAG_RE.as_ref().is_some_and(|re| re.is_match(part)));
        if let (false, Some(w)) = (tagged, words(start, end)) {
            spans.push((
                start,
                end,
                book.tag_for("כתובת", &w),
                "כתובת מגורים".to_owned(),
            ));
        }
    }
    if let Some(re) = TAG_RE.as_ref() {
        for m in re.find_iter(text) {
            let tag = m.as_str();
            let allowed =
                req.case_tags.contains(tag) || is_generic_tag(tag) || tag.starts_with("[חסר");
            if !allowed {
                spans.push((m.start(), m.end(), tag_word(tag), "תגית".to_owned()));
            }
        }
    }
    if spans.is_empty() {
        return None;
    }
    // Widest first at each start; a span inside one already taken is dropped (it is hidden).
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut kept: Vec<Span> = Vec::new();
    for s in spans {
        match kept.last_mut() {
            Some(last) if s.0 < last.1 => {
                // Overlapping but reaching further: the taken span grows, as a number.
                if s.1 > last.1 {
                    let w = words(last.0, s.1)?;
                    *last = (last.0, s.1, book.tag_for("מספר", &w), last.3.clone());
                }
            }
            _ => kept.push(s),
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end, tag, label) in kept {
        out.push_str(text.get(at..start)?);
        items.push(FixedItem {
            original: words(start, end)?,
            tag: tag.clone(),
            label,
        });
        out.push_str(&tag);
        at = end;
    }
    out.push_str(text.get(at..)?);
    Some(out)
}
