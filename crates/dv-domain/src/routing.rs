//! Which report sections a material feeds, and which of its passages go to each (D-022).
//!
//! Three layers, the most specific wins:
//! 1. the fixed table in the report structure (by kind of material) – always there;
//! 2. the sorting suggested per passage, by Claude or locally in demo mode;
//! 3. Einat's own choice (sections she added or removed), which always wins.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A passage longer than this many lines is closed even without a full stop, so a document
/// with no punctuation (a table pasted as text) is not one endless passage.
const MAX_LINES: usize = 12;

/// Split a text into passages: paragraphs, with wrapped lines joined and a heading kept with
/// what follows it. Returns byte ranges in `text`, trimmed of surrounding blank space.
///
/// The boundaries depend only on line breaks and on how a line ends, never on length, so they
/// are the same whatever the review decided about a name. Passage numbers are defined on the
/// original text of a material; what is sent is the filtered text of the same range.
#[must_use]
pub fn passage_ranges(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let (mut end, mut lines, mut offset) = (0, 0, 0);
    for raw in text.split_inclusive('\n') {
        let line_start = offset;
        offset += raw.len();
        let body = raw.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if trimmed.is_empty() {
            if let Some(s) = start.take() {
                out.push(s..end);
            }
            lines = 0;
            continue;
        }
        let first = line_start + (body.len() - body.trim_start().len());
        start.get_or_insert(first);
        end = first + trimmed.len();
        lines += 1;
        if ends_sentence(trimmed) || lines >= MAX_LINES {
            if let Some(s) = start.take() {
                out.push(s..end);
            }
            lines = 0;
        }
    }
    if let Some(s) = start {
        out.push(s..end);
    }
    out
}

/// The passages of a text, as strings (see [`passage_ranges`]).
#[must_use]
pub fn passages(text: &str) -> Vec<String> {
    passage_ranges(text)
        .into_iter()
        .filter_map(|r| text.get(r).map(str::to_owned))
        .collect()
}

/// A line that ends a sentence closes its passage. A colon does not: it introduces what follows.
fn ends_sentence(line: &str) -> bool {
    let end = line.trim_end_matches(['"', '\'', '״', '”', ')', ']']);
    end.ends_with(['.', '!', '?', ';', '׃']) && !end.ends_with("..")
}

/// The passages (1-based) of one material that were found relevant to one section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionPassages {
    pub section: String,
    pub passages: Vec<u32>,
}

/// The result of sorting one material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Suggestion {
    /// By Claude; false when sorted locally (demo mode, nothing sent).
    pub by_ai: bool,
    /// How many passages the material had when it was sorted. If the text changed since, the
    /// suggestion no longer fits and the table is used instead.
    pub passage_count: u32,
    pub sections: Vec<SectionPassages>,
}

/// What reaches one section from one material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feed {
    Whole,
    /// 1-based passage numbers, in order.
    Passages(Vec<u32>),
}

/// Everything stored about where a material goes. Sealed with the case key in the vault.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Routing {
    pub suggestion: Option<Suggestion>,
    /// Sections Einat added. The whole material goes there.
    pub added: Vec<String>,
    /// Sections Einat removed. Nothing from the material goes there.
    pub removed: Vec<String>,
    /// A sorting read the material (this many passages) and placed nothing: the table stays,
    /// and it is not offered for sorting again until the text changes.
    #[serde(default)]
    pub unplaced: Option<u32>,
}

impl Routing {
    /// The suggestion, if it still fits the material's current text.
    fn fresh(&self, passage_count: usize) -> Option<&Suggestion> {
        self.suggestion
            .as_ref()
            .filter(|s| usize::try_from(s.passage_count).is_ok_and(|n| n == passage_count))
    }

    /// Sections before Einat's changes: the suggestion when it fits, else the table.
    #[must_use]
    pub fn base(&self, table: &[String], passage_count: usize) -> Vec<String> {
        match self.fresh(passage_count) {
            Some(s) => s
                .sections
                .iter()
                .filter(|p| !p.passages.is_empty())
                .map(|p| p.section.clone())
                .collect(),
            None => table.to_vec(),
        }
    }

    /// True when the sections shown come from a sorting that still fits.
    #[must_use]
    pub fn is_sorted(&self, passage_count: usize) -> bool {
        self.fresh(passage_count).is_some()
    }

    /// Not read by a sorting since its text last changed.
    #[must_use]
    pub fn needs_sorting(&self, passage_count: usize) -> bool {
        !self.is_sorted(passage_count)
            && self
                .unplaced
                .is_none_or(|n| usize::try_from(n).is_ok_and(|n| n != passage_count))
    }

    /// What goes from this material to `section`, or `None`.
    #[must_use]
    pub fn feed(&self, section: &str, table: &[String], passage_count: usize) -> Option<Feed> {
        if self.removed.iter().any(|s| s == section) {
            return None;
        }
        if self.added.iter().any(|s| s == section) {
            return Some(Feed::Whole);
        }
        match self.fresh(passage_count) {
            Some(s) => {
                let max = u32::try_from(passage_count).unwrap_or(u32::MAX);
                let mut chosen: Vec<u32> = s
                    .sections
                    .iter()
                    .filter(|p| p.section == section)
                    .flat_map(|p| p.passages.iter().copied())
                    .filter(|n| (1..=max).contains(n))
                    .collect();
                chosen.sort_unstable();
                chosen.dedup();
                (!chosen.is_empty()).then_some(Feed::Passages(chosen))
            }
            None => table.iter().any(|s| s == section).then_some(Feed::Whole),
        }
    }

    /// Every section this material feeds, in the order of `sections` (the report order).
    #[must_use]
    pub fn feeds(
        &self,
        sections: &[String],
        table: &[String],
        passage_count: usize,
    ) -> Vec<String> {
        sections
            .iter()
            .filter(|s| self.feed(s, table, passage_count).is_some())
            .cloned()
            .collect()
    }

    /// Einat picked exactly `chosen`. Store it as changes against the base, so a later
    /// sorting still shows which sections were hers.
    pub fn choose(&mut self, chosen: &[String], table: &[String], passage_count: usize) {
        let base = self.base(table, passage_count);
        self.added = chosen
            .iter()
            .filter(|s| !base.contains(s))
            .cloned()
            .collect();
        self.removed = base.into_iter().filter(|s| !chosen.contains(s)).collect();
    }

    /// The text changed: the old passages no longer exist. Einat's choices stay.
    pub fn text_changed(&mut self) {
        self.suggestion = None;
        self.unplaced = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn paragraphs_wrapped_lines_and_headings() {
        let text = "רקע התפתחותי:\nהלך בגיל שנה.\n\nבגן הוא משחק לבד\nבעיקר בחצר.\nאוהב פאזלים.\n";
        assert_eq!(
            passages(text),
            s(&[
                "רקע התפתחותי:\nהלך בגיל שנה.",
                "בגן הוא משחק לבד\nבעיקר בחצר.",
                "אוהב פאזלים."
            ])
        );
    }

    #[test]
    fn a_list_stays_with_its_heading() {
        let text = "המלצות:\n- ריפוי בעיסוק\n- הדרכת הורים\n\nסוף.";
        assert_eq!(
            passages(text),
            s(&["המלצות:\n- ריפוי בעיסוק\n- הדרכת הורים", "סוף."])
        );
    }

    #[test]
    fn text_without_punctuation_is_cut_by_lines_not_length() {
        let text = (1..=30)
            .map(|i| format!("שורה {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let p = passages(&text);
        assert_eq!(p.len(), 3);
        assert!(p.iter().all(|x| x.lines().count() <= MAX_LINES));
    }

    #[test]
    fn replacing_a_name_by_a_tag_never_moves_a_boundary() {
        let original = "בגן משחק עם יובל\nבעיקר בחצר.\nסיפר על מיכל ואמר \"די\".\nבקבוצה שקט";
        let tagged = original
            .replace("יובל", "[ילד אחר 1]")
            .replace("מיכל", "[גננת]");
        let a = passages(original);
        let b = passages(&tagged);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.lines().count(), y.lines().count());
        }
    }

    #[test]
    fn an_ellipsis_does_not_end_a_sentence() {
        assert_eq!(passages("אמר שהוא...\nלא יודע.").len(), 1);
        assert_eq!(passages("סיים (בקושי).\nהמשיך.").len(), 2);
    }

    #[test]
    fn ranges_point_into_the_original_text() {
        let text = "  פתיחה:\n  הלך.\n\nבגן.";
        let r = passage_ranges(text);
        assert_eq!(r.len(), 2);
        assert_eq!(&text[r[0].clone()], "פתיחה:\n  הלך.");
        assert_eq!(&text[r[1].clone()], "בגן.");
    }

    #[test]
    fn crlf_and_blank_lines_are_handled() {
        assert_eq!(passages("א.\r\n\r\n\r\nב.\r\n"), s(&["א.", "ב."]));
        assert!(passages("\n \n").is_empty());
    }

    fn sorted(sections: &[(&str, &[u32])], count: u32) -> Routing {
        Routing {
            suggestion: Some(Suggestion {
                by_ai: true,
                passage_count: count,
                sections: sections
                    .iter()
                    .map(|(k, p)| SectionPassages {
                        section: (*k).to_owned(),
                        passages: p.to_vec(),
                    })
                    .collect(),
            }),
            ..Routing::default()
        }
    }

    #[test]
    fn without_sorting_the_table_decides_and_the_whole_material_goes() {
        let r = Routing::default();
        let table = s(&["referral", "background"]);
        assert_eq!(r.feed("background", &table, 5), Some(Feed::Whole));
        assert_eq!(r.feed("cognitive", &table, 5), None);
        assert!(!r.is_sorted(5));
    }

    #[test]
    fn a_fresh_sorting_sends_only_the_chosen_passages() {
        let r = sorted(&[("cognitive", &[3, 1, 3, 9]), ("kindergarten", &[])], 5);
        let table = s(&["background"]);
        // Out-of-range and duplicate numbers are dropped; order is restored.
        assert_eq!(
            r.feed("cognitive", &table, 5),
            Some(Feed::Passages(vec![1, 3]))
        );
        // The table no longer adds sections the sorting did not choose.
        assert_eq!(r.feed("background", &table, 5), None);
        // A section with no passages is not fed.
        assert_eq!(r.feed("kindergarten", &table, 5), None);
        assert!(r.is_sorted(5));
    }

    #[test]
    fn a_material_with_nothing_placed_keeps_the_table_and_is_not_offered_again() {
        let r = Routing {
            unplaced: Some(3),
            ..Routing::default()
        };
        let table = s(&["background"]);
        assert_eq!(r.feed("background", &table, 3), Some(Feed::Whole));
        assert!(!r.needs_sorting(3));
        assert!(r.needs_sorting(4), "a new text is offered again");
    }

    #[test]
    fn a_stale_sorting_falls_back_to_the_table() {
        let r = sorted(&[("cognitive", &[1])], 5);
        let table = s(&["background"]);
        assert_eq!(r.feed("cognitive", &table, 6), None);
        assert_eq!(r.feed("background", &table, 6), Some(Feed::Whole));
        assert!(!r.is_sorted(6));
    }

    #[test]
    fn einat_always_wins() {
        let mut r = sorted(&[("cognitive", &[2]), ("communication", &[1])], 3);
        let table = s(&["background"]);
        let order = s(&["background", "cognitive", "communication", "emotional"]);
        r.choose(&s(&["communication", "emotional"]), &table, 3);
        assert_eq!(r.added, s(&["emotional"]));
        assert_eq!(r.removed, s(&["cognitive"]));
        assert_eq!(r.feed("emotional", &table, 3), Some(Feed::Whole));
        assert_eq!(r.feed("cognitive", &table, 3), None);
        assert_eq!(
            r.feeds(&order, &table, 3),
            s(&["communication", "emotional"])
        );

        // After an edit the passages are gone, but her choices stay.
        r.text_changed();
        assert!(r.needs_sorting(4));
        assert_eq!(r.feeds(&order, &table, 4), s(&["background", "emotional"]));
    }
}
