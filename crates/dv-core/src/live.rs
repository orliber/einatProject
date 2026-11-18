//! What the AI is writing, shown while it writes (D-203): for a section and a consultation
//! only. The answer comes back with tags (`[ילד]`), never names; the words written so far are
//! put back here, on this computer, exactly as the finished answer is, and shown as text.
//!
//! Nothing here is kept: the text lives in memory while the request is on its way and is
//! dropped with it. The checks of the finished answer (a name it should not know, a number
//! not in the sources, …) run on the whole answer as before; what is shown meanwhile is a
//! preview that the finished draft replaces.

use dv_domain::{Identity, MaskEntry};
use dv_privacy::restore::restore;

/// How to show one request's answer while it is written: whose names to put back, and
/// whether the answer is the JSON of a section or plain text.
#[derive(Clone)]
pub struct LiveView {
    identities: Vec<Identity>,
    practitioner: Option<String>,
    masks: Vec<MaskEntry>,
    structured: bool,
    /// The session the request left in (`Core::session`): after a lock nothing is shown.
    session: u64,
}

/// The names and the words the masks hid are wiped when the last holder lets go, as the lock
/// wipes the core's own copies (`Core::lock`).
impl Drop for LiveView {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for i in &mut self.identities {
            i.value.zeroize();
            for a in &mut i.aliases {
                a.zeroize();
            }
        }
        if let Some(p) = &mut self.practitioner {
            p.zeroize();
        }
        for m in &mut self.masks {
            m.original.zeroize();
        }
    }
}

impl std::fmt::Debug for LiveView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveView")
            .field("structured", &self.structured)
            .finish_non_exhaustive()
    }
}

/// The fields of a section's answer that are words for her: the reply, then each paragraph.
const SHOWN_KEYS: &[&str] = &["reply", "text"];

impl LiveView {
    pub(crate) fn new(
        identities: Vec<Identity>,
        practitioner: Option<String>,
        masks: Vec<MaskEntry>,
        structured: bool,
        session: u64,
    ) -> Self {
        Self {
            identities,
            practitioner,
            masks,
            structured,
            session,
        }
    }

    /// The session it belongs to; [`crate::Core::is_session`] tells whether it is still open.
    #[must_use]
    pub fn session(&self) -> u64 {
        self.session
    }

    /// The words written so far (`written`, tagged, as it came), as she reads them: the
    /// section's reply and paragraphs out of the JSON, a tag cut in the middle held back,
    /// and the case's names put back.
    #[must_use]
    pub fn show(&self, written: &str) -> String {
        let words = if self.structured {
            let parts = json_strings(written, SHOWN_KEYS);
            parts
                .iter()
                .filter(|p| !p.trim().is_empty())
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            written.to_owned()
        };
        let whole = hold_back_open_tag(&words);
        restore(
            whole,
            &self.identities,
            self.practitioner.as_deref(),
            &self.masks,
        )
    }
}

impl crate::Core {
    /// How a request's answer is shown while it is written: put back as its finished answer
    /// will be (`finish_section`, `finish_consult`). `None`: only the number of words.
    pub(crate) fn live_view(
        &mut self,
        kind: &crate::PendingKind,
    ) -> Result<Option<LiveView>, crate::CoreError> {
        let session = self.session;
        let v = self.vault_ref()?;
        Ok(match kind {
            crate::PendingKind::Section { case_id, .. } => Some(LiveView::new(
                v.identities(case_id)?,
                v.practitioner()?.names.first().cloned(),
                v.masks(case_id)?,
                true,
                session,
            )),
            crate::PendingKind::Consult { case_id, masks, .. } => Some(LiveView::new(
                match case_id {
                    Some(c) => v.identities(c)?,
                    None => Vec::new(),
                },
                None,
                masks.clone(),
                false,
                session,
            )),
            _ => None,
        })
    }

    /// Whether `session` is the vault's current opening: after a lock, nothing written by a
    /// request that left before it is shown (D-203).
    #[must_use]
    pub fn is_session(&self, session: u64) -> bool {
        self.vault.is_some() && self.session == session
    }
}

/// Up to an opening `[` with no `]` after it in the last line, the one still being written:
/// half a tag ("[יל") is never shown as written. A tag never spans lines, so a stray `[` in a
/// finished line or paragraph holds nothing back.
fn hold_back_open_tag(text: &str) -> &str {
    let line = text.rfind('\n').map_or(0, |i| i + 1);
    let tail = text.get(line..).unwrap_or("");
    match (tail.rfind('['), tail.rfind(']')) {
        (Some(open), Some(close)) if close > open => text,
        (Some(open), _) => text.get(..line + open).unwrap_or(""),
        _ => text,
    }
}

#[derive(Clone, Copy)]
enum Container {
    /// An object, and whether the next string in it is a key.
    Object {
        key_next: bool,
    },
    Array,
}

/// The string values of `keys` in a JSON text that may stop anywhere, in the order written;
/// the last one as far as it goes. Keys of any object at any depth count ("text" of each
/// paragraph). An escape cut at the end is left out; nothing here fails.
fn json_strings(json: &str, keys: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<Container> = Vec::new();
    let mut key = String::new();
    let mut chars = json.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => stack.push(Container::Object { key_next: true }),
            '[' => stack.push(Container::Array),
            '}' | ']' => {
                stack.pop();
            }
            ':' => {
                if let Some(Container::Object { key_next }) = stack.last_mut() {
                    *key_next = false;
                }
            }
            ',' => {
                if let Some(Container::Object { key_next }) = stack.last_mut() {
                    *key_next = true;
                }
            }
            '"' => {
                let is_key = matches!(stack.last(), Some(Container::Object { key_next: true }));
                let wanted = !is_key
                    && matches!(stack.last(), Some(Container::Object { .. }))
                    && keys.contains(&key.as_str());
                let s = read_string(&mut chars);
                if is_key {
                    key = s;
                } else if wanted {
                    out.push(s);
                }
            }
            _ => {}
        }
    }
    out
}

/// A JSON string after its opening quote, up to its closing quote or the end of the text.
fn read_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => s.push('\n'),
                Some('t') => s.push('\t'),
                Some('r') => {}
                Some('b' | 'f') => {}
                Some('u') => match read_unicode(chars) {
                    Some(ch) => s.push(ch),
                    None => break,
                },
                Some(other) => s.push(other),
                None => break,
            },
            _ => s.push(c),
        }
    }
    s
}

/// `\uXXXX` (and a surrogate pair `😀`). `None` when cut at the end or not valid.
fn read_unicode(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    let hex = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            v = v * 16 + chars.next()?.to_digit(16)?;
        }
        Some(v)
    };
    let first = hex(chars)?;
    if (0xD800..0xDC00).contains(&first) {
        if chars.next()? != '\\' || chars.next()? != 'u' {
            return None;
        }
        let low = hex(chars)?;
        if !(0xDC00..0xE000).contains(&low) {
            return None;
        }
        return char::from_u32(0x10000 + ((first - 0xD800) << 10) + (low - 0xDC00));
    }
    char::from_u32(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dv_domain::Role;

    fn child() -> Identity {
        Identity {
            id: "i1".into(),
            case_id: "c1".into(),
            role: Role::Child,
            tag: "[ילד]".into(),
            value: "נועם".into(),
            aliases: Vec::new(),
            source: dv_domain::IdentitySource::default(),
            reason: String::new(),
        }
    }

    fn view(structured: bool) -> LiveView {
        LiveView::new(vec![child()], None, Vec::new(), structured, 1)
    }

    #[test]
    fn a_section_shows_its_reply_and_paragraphs_as_far_as_they_go() {
        let written = r#"{"reply": "כתבתי", "paragraphs": [{"text": "[ילד] הגיע בשמחה.", "source_refs": ["S1"], "guide": 1}, {"text": "הוא שיתף פעו"#;
        assert_eq!(
            view(true).show(written),
            "כתבתי\n\nנועם הגיע בשמחה.\n\nהוא שיתף פעו"
        );
    }

    #[test]
    fn lists_refs_and_numbers_are_not_shown() {
        let written = r#"{"reply": "", "paragraphs": [{"text": "א", "source_refs": ["S1", "S2"], "guide": 3}], "questions": ["מה גילו?"], "missing": ["text"]"#;
        assert_eq!(view(true).show(written), "א");
    }

    #[test]
    fn a_stray_bracket_in_a_finished_paragraph_holds_nothing_back() {
        let written = r#"{"reply": "", "paragraphs": [{"text": "ציון [ללא סוגר", "source_refs": [], "guide": 0}, {"text": "[ילד] שיחק"#;
        assert_eq!(view(true).show(written), "ציון [ללא סוגר\n\nנועם שיחק");
        assert_eq!(view(false).show("שורה [א\nשורה ב [יל"), "שורה [א\nשורה ב ");
    }

    #[test]
    fn half_a_tag_is_held_back_until_it_closes() {
        assert_eq!(view(false).show("שלום [יל"), "שלום ");
        assert_eq!(view(false).show("שלום ל[ילד]"), "שלום לנועם");
        assert_eq!(view(true).show(r#"{"reply": "ראיתי את [יל"#), "ראיתי את ");
    }

    #[test]
    fn escapes_are_read_and_a_cut_escape_is_left_out() {
        assert_eq!(
            json_strings(r#"{"text": "שורה\nשנייה \"ציטוט\" א"}"#, &["text"]),
            vec!["שורה\nשנייה \"ציטוט\" א".to_owned()]
        );
        assert_eq!(
            json_strings(r#"{"text": "עד כאן \u05"#, &["text"]),
            vec!["עד כאן ".to_owned()]
        );
        assert_eq!(
            json_strings(r#"{"text": "סמל 😀"#, &["text"]),
            vec!["סמל 😀".to_owned()]
        );
    }

    #[test]
    fn a_key_named_text_inside_a_value_is_not_a_key() {
        let written = r#"{"reply": "text", "paragraphs": [{"text": "\"text\": x"}]}"#;
        assert_eq!(
            json_strings(written, SHOWN_KEYS),
            vec!["text".to_owned(), "\"text\": x".to_owned()]
        );
    }

    #[test]
    fn nothing_written_shows_nothing_and_any_cut_never_panics() {
        assert_eq!(view(true).show(""), "");
        let full = r#"{"reply": "שלום [ילד]", "paragraphs": [{"text": "aא\"b", "source_refs": [], "guide": 0}]}"#;
        for (i, _) in full.char_indices() {
            let _ = view(true).show(full.get(..i).unwrap_or_default());
        }
        assert_eq!(view(true).show(full), "שלום נועם\n\naא\"b");
    }
}
