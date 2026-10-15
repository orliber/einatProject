//! A request that is too large for one send is shortened, not refused (round 25): the longest
//! data block is cut at a line, then the next longest, until the body fits. Only text inside a
//! block goes; the instructions, the block marks and her request stay whole. What was cut stays
//! on the computer, and the cut ends with a plain note saying so, so the model knows the
//! material goes on. The shortened body goes through the gate again like any other.

use serde_json::Value;

/// What is left of a block is never cut further than this (bytes).
const FLOOR: usize = 400;
/// Room left under the limit, so the note and JSON escaping never tip it over.
const MARGIN: usize = 2048;
/// Plain words, no brackets: the gate reads it like any other text.
const NOTE: &str = "\n(המשך החומר הזה לא נשלח, כי הבקשה הייתה ארוכה מדי. הוא נשאר במחשב.)";

/// One shortened block: what it is, and how much of it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    /// The block's own words: its source id and kind, or the approved section's title.
    pub label: String,
    /// Bytes of the block that were sent, and its length before.
    pub kept: usize,
    pub total: usize,
}

/// `body` shortened to at most `max_bytes` serialized, with the cuts. `None`: it cannot be
/// shortened enough by cutting blocks (the refusal stands).
#[must_use]
pub fn shrink_to_fit(body: &Value, max_bytes: usize) -> Option<(Value, Vec<Cut>)> {
    let mut body = body.clone();
    let mut cuts: Vec<Cut> = Vec::new();
    for _ in 0..64 {
        let size = serde_json::to_vec(&body).ok()?.len();
        if size <= max_bytes {
            return (!cuts.is_empty()).then_some((body, cuts));
        }
        let excess = size - max_bytes + MARGIN;
        let mut texts = Vec::new();
        if let Some(messages) = body.get_mut("messages") {
            strings_mut(messages, &mut texts);
        }
        // The longest block anywhere in the conversation.
        let (s, block) = texts
            .into_iter()
            .filter_map(|t| {
                let longest = blocks(t).into_iter().max_by_key(Block::len)?;
                Some((t, longest))
            })
            .max_by_key(|(_, b)| b.len())?;
        let (start, end) = block.inner;
        let len = block.len();
        if len <= FLOOR {
            return None;
        }
        let keep = len.saturating_sub(excess).max(FLOOR);
        let inner = s.get(start..end)?;
        let at = cut_point(inner, keep);
        let kept_text = inner.get(..at)?.to_owned();
        s.replace_range(start..end, &format!("{kept_text}{NOTE}"));
        match cuts.iter_mut().find(|c| c.label == block.label) {
            Some(c) => c.kept = at,
            None => cuts.push(Cut {
                label: block.label,
                kept: at,
                total: len,
            }),
        }
    }
    None
}

/// Where to cut `text` to keep at most `keep` bytes: the last line break before it, else the
/// last space, else a character boundary. Never inside a tag (`[ילד]`, `[מספר_12]`): a cut
/// there moves to before its bracket, so no half tag goes out or comes back wrong.
fn cut_point(text: &str, keep: usize) -> usize {
    let mut at = keep.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let head = text.get(..at).unwrap_or_default();
    let at = head
        .rfind('\n')
        .filter(|&i| i >= at / 2)
        .or_else(|| head.rfind(' ').filter(|&i| i >= at / 2))
        .unwrap_or(at);
    let head = text.get(..at).unwrap_or_default();
    match (head.rfind('['), head.rfind(']')) {
        (Some(open), close) if close.is_none_or(|c| c < open) => open,
        _ => at,
    }
}

struct Block {
    /// The byte range of the block's text, between its marks.
    inner: (usize, usize),
    label: String,
}

impl Block {
    fn len(&self) -> usize {
        self.inner.1 - self.inner.0
    }
}

/// The data blocks of one text (`<data nonce=… attrs>\n…\n</data nonce=…>`).
fn blocks(text: &str) -> Vec<Block> {
    const OPEN: &str = "<data nonce=";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text.get(from..).and_then(|t| t.find(OPEN)) {
        let at = from + i + OPEN.len();
        let Some(head_end) = text.get(at..).and_then(|t| t.find(">\n")) else {
            break;
        };
        let Some(head) = text.get(at..at + head_end) else {
            break;
        };
        let (nonce, attrs) = head.split_once(' ').unwrap_or((head, ""));
        let inner_start = at + head_end + 2;
        let close = format!("\n</data nonce={nonce}>");
        let Some(j) = text.get(inner_start..).and_then(|t| t.find(&close)) else {
            break;
        };
        let inner_end = inner_start + j;
        out.push(Block {
            inner: (inner_start, inner_end),
            label: label_of(attrs),
        });
        from = inner_end + close.len();
    }
    out
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let i = attrs.find(&key)? + key.len();
    let rest = attrs.get(i..)?;
    rest.find('"').and_then(|j| rest.get(..j))
}

fn label_of(attrs: &str) -> String {
    if let Some(title) = attr(attrs, "approved_section") {
        return format!("סעיף מאושר: {title}");
    }
    if attr(attrs, "current_draft").is_some() {
        return "הטיוטה הנוכחית".to_owned();
    }
    match (attr(attrs, "id"), attr(attrs, "kind")) {
        (Some(id), Some(kind)) => format!("{id} · {kind}"),
        (Some(id), None) => id.to_owned(),
        _ => "חומר".to_owned(),
    }
}

fn strings_mut<'a>(v: &'a mut Value, out: &mut Vec<&'a mut String>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(list) => list.iter_mut().for_each(|x| strings_mut(x, out)),
        Value::Object(map) => map.values_mut().for_each(|x| strings_mut(x, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_section_request, content_text, ModelConfig, SectionInput, TaggedInput};

    fn input(sizes: &[usize]) -> SectionInput {
        SectionInput {
            section_key: "kindergarten".into(),
            section_title: "מסגרת חינוכית".into(),
            age: None,
            gender: None,
            sources: sizes
                .iter()
                .enumerate()
                .map(|(i, n)| TaggedInput {
                    input_id: format!("in{i}"),
                    kind_label: "מסגרת חינוכית".into(),
                    title_tagged: format!("שיחה {i}"),
                    content_tagged: "[ילד] משתף פעולה במשחק.\n".repeat(*n),
                })
                .collect(),
            approved_context: vec![("רקע".into(), "[ילד] נולד במועד.".into())],
            current_draft: vec![],
            history: vec![],
            instruction_tagged: "נסח/י פסקה".into(),
            style_profile: None,
        }
    }

    fn size(v: &Value) -> usize {
        serde_json::to_vec(v).map_or(usize::MAX, |b| b.len())
    }

    fn all_text(v: &Value) -> String {
        v["messages"]
            .as_array()
            .map(|m| m.iter().map(|x| content_text(&x["content"])).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_body_that_fits_is_left_alone() {
        let (body, _) = build_section_request(&ModelConfig::default(), &input(&[10]), "nn");
        assert_eq!(shrink_to_fit(&body, 1_000_000), None);
    }

    #[test]
    fn the_longest_block_is_cut_at_a_line_and_the_rest_stays_whole() {
        let (body, _) = build_section_request(&ModelConfig::default(), &input(&[40, 4000]), "nn");
        let max = size(&body) / 2;
        let (short, cuts) = shrink_to_fit(&body, max).expect("shortened");
        assert!(size(&short) <= max);
        assert_eq!(cuts.len(), 1, "{cuts:?}");
        assert_eq!(cuts[0].label, "S2 · מסגרת חינוכית");
        assert!(cuts[0].kept < cuts[0].total);
        let text = all_text(&short);
        // Every block is still closed, her request and the other blocks are whole.
        assert_eq!(text.matches("<data nonce=nn").count(), 3);
        assert_eq!(text.matches("</data nonce=nn>").count(), 3);
        assert!(text.contains("הפסיכולוגית מבקשת: נסח/י פסקה"));
        assert!(text.contains(&"[ילד] משתף פעולה במשחק.\n".repeat(40)));
        assert!(text.contains("[ילד] נולד במועד."));
        assert!(text.contains("הוא נשאר במחשב."));
        // Cut at a line: no half sentence before the note.
        assert!(text.contains("במשחק.\n(המשך החומר"));
        // The instructions and the schema did not move.
        assert_eq!(short["system"], body["system"]);
        assert_eq!(short["output_config"], body["output_config"]);
    }

    #[test]
    fn two_long_blocks_are_both_cut_when_one_is_not_enough() {
        let (body, _) = build_section_request(&ModelConfig::default(), &input(&[3000, 3000]), "nn");
        let max = size(&body) / 4;
        let (short, cuts) = shrink_to_fit(&body, max).expect("shortened");
        assert!(size(&short) <= max);
        assert_eq!(cuts.len(), 2, "{cuts:?}");
    }

    /// The review of stage 2: a cut with no line and no space near it lands inside a tag or a
    /// word. It moves to before the tag's bracket, so every tag that goes out is whole.
    #[test]
    fn a_cut_with_no_space_never_splits_a_tag() {
        let dense = "[ילד][מספר_12][תאריך_3]".repeat(20);
        for keep in 1..dense.len() {
            let at = cut_point(&dense, keep);
            let head = dense.get(..at).expect("a character boundary");
            assert!(
                head.matches('[').count() == head.matches(']').count(),
                "{keep}: {head}"
            );
            assert!(at <= keep);
        }
        let mut inp = input(&[1]);
        if let Some(s) = inp.sources.first_mut() {
            s.content_tagged = "[ילד]גדל[מספר_12]שנים".repeat(400);
        }
        let (body, _) = build_section_request(&ModelConfig::default(), &inp, "nn");
        let max = size(&body) / 2;
        let (short, _) = shrink_to_fit(&body, max).expect("shortened");
        let text = all_text(&short);
        let cut = text.find("\n(המשך החומר").expect("the note");
        let before = text.get(..cut).unwrap_or_default();
        let tail = before
            .get(before.rfind("<data nonce=nn").unwrap_or(0)..)
            .unwrap_or_default();
        assert_eq!(
            tail.matches('[').count(),
            tail.matches(']').count(),
            "{tail}"
        );
    }

    #[test]
    fn what_cannot_fit_stays_refused() {
        let (body, _) = build_section_request(&ModelConfig::default(), &input(&[200]), "nn");
        assert_eq!(shrink_to_fit(&body, 1_000), None);
    }
}
