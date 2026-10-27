//! One step of the automatic fix loop (round 25): what the gate refused in a request is hidden
//! in the outgoing copy, and the copy goes back to the gate, which is not changed and has no
//! way around it. Only spans are replaced, each with a tag that names a kind or this case's
//! book, so a fix can only take words out. What cannot be fixed this way (a canary, an
//! internal error, a refused object key) stays refused. A request too large is not this
//! module's: the core shortens it (`dv_ai::shrink_to_fit`) and asks the gate again.
//!
//! - A name (declared, from the lexicon, from her past reports, of another case) → the whole
//!   line is left out, as [`LEFT_OUT`]. A gate refusal means the filter never saw this text,
//!   so the words next to the name (a surname, a second name) are not known to be safe:
//!   hiding only the word the gate matched could let them out. Nothing of the line is kept
//!   in the case's book, so another case's name can never be restored into this one.
//! - A tag that is not the case's → the whole line is left out too: what is in the brackets
//!   (up to 40 characters, "[דנה_שטרנברג]") was never filtered.
//! - A pattern (number, date, phone, address field…) → the numbered tag of its kind.
//!
//! When hiding was not enough, the last step (D-082) leaves out whole every line the gate
//! still refuses on its own for one of those reasons: the line goes out as [`LEFT_OUT`], and
//! the copy goes to the same gate once more. A line with a canary or an error is never left
//! out, so such a request stays refused. Only words go out less.

use serde_json::Value;

use crate::gate::{name_findings, GateRequest};
use crate::masks::{is_generic_tag, MaskBook, TAG_RE};
use crate::patterns;

/// The codes a fix can answer. Any other code in a refusal leaves it refused.
pub const FIXABLE_CODES: &[&str] = &[
    "identity",
    "unknown_name",
    "past_report_name",
    "pattern",
    "foreign_tag",
];

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

/// What a line left out goes out as: plain words, no tag, so the gate checks it like any
/// text and the model writes without it.
pub const LEFT_OUT: &str = "(קטע שהושמט בבדיקה)";

/// A span to replace: start, end, what goes out, and its kind for the list.
type Span = (usize, usize, String, String);

/// A tag the case does not know: neither its own, nor a kind, nor a marker to complete.
fn foreign(tag: &str, req: &GateRequest<'_>) -> bool {
    !(req.case_tags.contains(tag) || is_generic_tag(tag) || crate::masks::is_missing_note(tag))
}

/// Every line with a name or a foreign tag in it, left out whole; `None` when there is none.
fn drop_name_lines(
    text: &str,
    req: &GateRequest<'_>,
    items: &mut Vec<FixedItem>,
) -> Option<String> {
    let mut marks: Vec<usize> = name_findings(text, req.ctx)
        .iter()
        .map(|f| f.start)
        .collect();
    if let Some(re) = TAG_RE.as_ref() {
        marks.extend(
            re.find_iter(text)
                .filter(|m| foreign(m.as_str(), req))
                .map(|m| m.start()),
        );
    }
    if marks.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let end = at + line.len();
        let (body, newline) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
        // A line with a trap string is never left out: the request stays refused.
        let trapped = req.canaries.iter().any(|c| body.contains(c.as_str()));
        if !trapped && marks.iter().any(|m| (at..end).contains(m)) {
            items.push(FixedItem {
                original: body.trim().to_owned(),
                tag: LEFT_OUT.to_owned(),
                label: "קטע שלא נשלח".to_owned(),
            });
            out.push_str(LEFT_OUT);
        } else {
            out.push_str(body);
        }
        out.push_str(newline);
        at = end;
    }
    Some(out)
}

fn fix_text(
    text: &str,
    req: &GateRequest<'_>,
    book: &MaskBook,
    items: &mut Vec<FixedItem>,
) -> Option<String> {
    // Names and foreign tags first: their lines go whole. Patterns on what is left.
    let dropped = drop_name_lines(text, req, items);
    let changed = dropped.is_some();
    let text = dropped.as_deref().unwrap_or(text);
    let mut spans: Vec<Span> = Vec::new();
    let words = |s: usize, e: usize| text.get(s..e).map(str::to_owned);
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
    if spans.is_empty() {
        return changed.then(|| text.to_owned());
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

/// The last step of the fix loop (D-082): every line of the request's string values that
/// the gate refuses on its own, only for reasons a fix answers, is replaced by [`LEFT_OUT`].
/// Keys are never touched. `None`: no line could be left out (the refusal stands).
#[must_use]
pub fn drop_refused_lines(req: &GateRequest<'_>) -> Option<(Value, Vec<FixedItem>)> {
    let mut items = Vec::new();
    let mut body = req.body.clone();
    drop_value(&mut body, req, &mut items);
    (!items.is_empty()).then_some((body, items))
}

fn drop_value(v: &mut Value, req: &GateRequest<'_>, items: &mut Vec<FixedItem>) {
    match v {
        Value::String(s) => {
            if let Some(kept) = drop_lines(s, req, items) {
                *s = kept;
            }
        }
        Value::Array(list) => list.iter_mut().for_each(|x| drop_value(x, req, items)),
        Value::Object(map) => map.values_mut().for_each(|x| drop_value(x, req, items)),
        _ => {}
    }
}

fn drop_lines(text: &str, req: &GateRequest<'_>, items: &mut Vec<FixedItem>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut dropped = false;
    for line in text.split_inclusive('\n') {
        let (body, end) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
        if !body.trim().is_empty() && refused_alone(body, req) {
            items.push(FixedItem {
                original: body.trim().to_owned(),
                tag: LEFT_OUT.to_owned(),
                label: "קטע שלא נשלח".to_owned(),
            });
            out.push_str(LEFT_OUT);
            dropped = true;
        } else {
            out.push_str(body);
        }
        out.push_str(end);
    }
    dropped.then_some(out)
}

/// The line by itself before the same gate: refused, and only for reasons a fix answers.
fn refused_alone(line: &str, req: &GateRequest<'_>) -> bool {
    let value = Value::String(line.to_owned());
    let alone = GateRequest {
        body: &value,
        ctx: req.ctx,
        case_tags: req.case_tags,
        unresolved_suspects: 0,
        canaries: req.canaries,
        max_bytes: usize::MAX,
    };
    match crate::gate::clear(&alone) {
        Ok(_) => false,
        Err(blocked) => {
            !blocked.reasons.is_empty()
                && blocked
                    .reasons
                    .iter()
                    .all(|r| FIXABLE_CODES.contains(&r.code.as_str()))
        }
    }
}
