//! No note gets lost (D-161, D-193): what an answer and the check against the sources had to
//! say, and the notes she left as they are on the finish page, kept with the case so the
//! report page shows them after any reload, a lock, a restart or a restore from a backup.
//!
//! Kept in the vault (`case_notes`, D-193): one row per case, sealed with the case key, so the
//! notes go with the case and with its backup. In memory, a case's notes are read the first
//! time the case needs them in a session, and the lock wipes that copy like every other state
//! of the session.
//! Per case and section: the questions the model asked, what it found missing or
//! contradicting, and the guide's lines no paragraph answers. Per draft paragraph: what was
//! found when its answer came (a name in the answer, a sentence of a past report, no source)
//! and the last verdict of the check against the sources. A paragraph's notes belong to its
//! wording: once the text she sees is another one (an edit, a new wording) or the paragraph
//! is gone (removed, replaced by a new draft), they are dropped.
//! What she left as it is (D-193) is kept by a keyed hash of what the finish page calls the
//! note, never by its words; one about a paragraph goes with the paragraph's wording too.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::{Zeroize, Zeroizing};

use crate::{style, Core, CoreError};

/// What a note under a paragraph is about. Fixed words over IPC; the page words each kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NoteKind {
    /// A number beside a measure that is not its score in the table.
    ScoreMismatch,
    /// A range word that is not the table's range for the score.
    BandWord,
    /// A percentile that is not the computed one.
    Percentile,
    /// A cut-off that is not the sheet's.
    Cutoff,
    /// An age that is not the child's age in the details.
    Age,
    /// Wording in the other grammatical gender.
    Gender,
    /// A class or kindergarten year unlike the rest of the report.
    Grade,
    /// A recommendation with nothing behind it in the body of the report.
    RecommendationBasis,
    /// A DSM level that is not the one she set.
    DsmSeverity,
    /// One thing called by two names.
    Term,
    /// An interpretation of hers written for another result.
    Interpretation,
    /// A number that nothing the paragraph rests on holds.
    NumberNotInSources,
    /// A name, or a tag of another case, in the answer.
    NameInAnswer,
    /// A run of words repeated from a past report.
    PastReportOverlap,
    /// The paragraph cites no source, or one that was not sent.
    NoSource,
    /// The check against the sources: the paragraph contradicts them.
    VerifyContradicted,
    /// The check against the sources: nothing in them supports the paragraph.
    VerifyUnsupported,
    /// The check against the sources: only part of the paragraph rests on them.
    VerifyPartial,
    /// Proofreading (D-167): advice for the finish, never a reason to hold a paragraph.
    Polish,
}

/// One thing there is to say about a paragraph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ParagraphNote {
    pub kind: NoteKind,
    pub text: String,
}

impl ParagraphNote {
    pub(crate) fn new(kind: NoteKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// What the last answer for a section said beside its paragraphs (names restored).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionNotes {
    /// What the model asked her.
    #[serde(default)]
    pub questions: Vec<String>,
    /// What it found missing in the materials.
    #[serde(default)]
    pub missing: Vec<String>,
    /// Where the materials contradict each other.
    #[serde(default)]
    pub contradictions: Vec<String>,
    /// The lines of the section's guide that no paragraph answers.
    #[serde(default)]
    pub unanswered: Vec<String>,
}

impl SectionNotes {
    fn is_empty(&self) -> bool {
        self.questions.is_empty()
            && self.missing.is_empty()
            && self.contradictions.is_empty()
            && self.unanswered.is_empty()
    }

    fn wipe(&mut self) {
        for list in [
            &mut self.questions,
            &mut self.missing,
            &mut self.contradictions,
            &mut self.unanswered,
        ] {
            for line in list.iter_mut() {
                line.zeroize();
            }
            list.clear();
        }
    }
}

/// What is kept for one paragraph, with the wording it was said about.
#[derive(Serialize, Deserialize)]
struct Kept {
    /// The paragraph's stored (tagged) wording then. Not the text with the names put back:
    /// a name she corrects, or her own name in the settings, does not change what was said
    /// about the wording, and must not drop a note that holds the paragraph back.
    wording: String,
    /// Found when the answer came, and not worked out again on each reading.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    answer: Vec<ParagraphNote>,
    /// The last verdict of the check against the sources that is worth a look.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    verdict: Vec<ParagraphNote>,
}

impl Kept {
    fn wipe(&mut self) {
        self.wording.zeroize();
        for n in self.answer.iter_mut().chain(self.verdict.iter_mut()) {
            n.text.zeroize();
        }
    }
}

/// A note she left as it is on the finish page (D-193). Only keyed hashes: of what the page
/// calls the note, and of the paragraph's stored wording when it is about one.
#[derive(Serialize, Deserialize)]
struct LeftMark {
    mac: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    draft_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wording: Option<String>,
}

/// The most notes of one case she can leave; the oldest goes first. Far more than a report
/// has, so only marks of wordings long gone are ever dropped.
const MAX_LEFT: usize = 2000;

/// The notes of one case, as kept in its sealed row (D-193).
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct CaseNotes {
    /// By section key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    sections: BTreeMap<String, SectionNotes>,
    /// By draft paragraph id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    paragraphs: BTreeMap<String, Kept>,
    /// What she left as it is, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    left: Vec<LeftMark>,
}

impl CaseNotes {
    fn is_empty(&self) -> bool {
        self.sections.is_empty() && self.paragraphs.is_empty() && self.left.is_empty()
    }

    /// The answer for a whole section: its notes take the place of the earlier ones.
    pub(crate) fn keep_section(&mut self, section_key: &str, notes: SectionNotes) {
        if let Some(mut old) = self.sections.remove(section_key) {
            old.wipe();
        }
        if !notes.is_empty() {
            self.sections.insert(section_key.to_owned(), notes);
        }
    }

    /// The answer that worded one paragraph again: what it asked or found missing is said
    /// now; the guide's lines are the whole draft's and stay as they were.
    pub(crate) fn keep_rewording(&mut self, section_key: &str, notes: SectionNotes) {
        if notes.questions.is_empty() && notes.missing.is_empty() && notes.contradictions.is_empty()
        {
            return;
        }
        // Added to what the draft's answer said, never instead of it: a question the first
        // answer asked is still open after one paragraph was worded again.
        let mut kept = self.section(section_key);
        let add = |into: &mut Vec<String>, new: Vec<String>| {
            for line in new {
                if !into.contains(&line) {
                    into.push(line);
                }
            }
        };
        add(&mut kept.questions, notes.questions);
        add(&mut kept.missing, notes.missing);
        add(&mut kept.contradictions, notes.contradictions);
        self.keep_section(section_key, kept);
    }

    pub(crate) fn section(&self, section_key: &str) -> SectionNotes {
        self.sections.get(section_key).cloned().unwrap_or_default()
    }

    /// What was found when the paragraph's answer came. A new answer starts the paragraph's
    /// notes over: an earlier verdict was about the earlier wording.
    pub(crate) fn keep_answer(
        &mut self,
        draft_id: &str,
        wording: String,
        notes: Vec<ParagraphNote>,
    ) {
        self.forget(draft_id);
        if notes.is_empty() {
            return;
        }
        self.paragraphs.insert(
            draft_id.to_owned(),
            Kept {
                wording,
                answer: notes,
                verdict: Vec::new(),
            },
        );
    }

    /// The verdict of the check against the sources for the paragraph as it reads now. A
    /// verdict with nothing to look at (supported, not checked) clears the one before it.
    pub(crate) fn keep_verdict(
        &mut self,
        draft_id: &str,
        wording: String,
        notes: Vec<ParagraphNote>,
    ) {
        match self.paragraphs.get_mut(draft_id) {
            Some(k) if k.wording == wording => {
                for n in &mut k.verdict {
                    n.text.zeroize();
                }
                k.verdict = notes;
                if k.answer.is_empty() && k.verdict.is_empty() {
                    self.forget(draft_id);
                }
            }
            _ => {
                self.forget(draft_id);
                if !notes.is_empty() {
                    self.paragraphs.insert(
                        draft_id.to_owned(),
                        Kept {
                            wording,
                            answer: Vec::new(),
                            verdict: notes,
                        },
                    );
                }
            }
        }
    }

    /// The notes kept for the paragraph, when its stored wording is still the one they were
    /// said about.
    pub(crate) fn of_paragraph(&self, draft_id: &str, wording: &str) -> Vec<ParagraphNote> {
        self.paragraphs
            .get(draft_id)
            .filter(|k| k.wording == wording)
            .map(|k| k.answer.iter().chain(&k.verdict).cloned().collect())
            .unwrap_or_default()
    }

    fn forget(&mut self, draft_id: &str) {
        if let Some(mut k) = self.paragraphs.remove(draft_id) {
            k.wipe();
        }
    }

    /// Drop what is kept about the case's paragraphs that are gone or read differently now:
    /// their notes, and the notes she left about them. `on_page`: every paragraph on the
    /// page, by id, with its stored wording; `mac`: the keyed hash of a wording. Returns
    /// whether anything was dropped.
    pub(crate) fn prune(
        &mut self,
        on_page: &HashMap<String, String>,
        mac: impl Fn(&str) -> String,
    ) -> bool {
        let gone: Vec<String> = self
            .paragraphs
            .iter()
            .filter(|(id, k)| on_page.get(*id) != Some(&k.wording))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &gone {
            self.forget(id);
        }
        let before = self.left.len();
        self.left.retain(|m| match (&m.draft_id, &m.wording) {
            (Some(id), Some(wording)) => on_page.get(id).is_some_and(|w| mac(w) == *wording),
            _ => true,
        });
        !gone.is_empty() || self.left.len() != before
    }

    /// She left the note the page calls by `mac` as it is (`about`: its paragraph and the
    /// keyed hash of the paragraph's wording), or brought it back. Returns whether anything
    /// changed.
    pub(crate) fn set_left(
        &mut self,
        mac: String,
        about: Option<(String, String)>,
        left: bool,
    ) -> bool {
        let before = self.left.len();
        if !left {
            self.left.retain(|m| m.mac != mac);
            return self.left.len() != before;
        }
        if self.left.iter().any(|m| m.mac == mac) {
            return false;
        }
        let (draft_id, wording) = about.unzip();
        self.left.push(LeftMark {
            mac,
            draft_id,
            wording,
        });
        if self.left.len() > MAX_LEFT {
            self.left.remove(0);
        }
        true
    }

    pub(crate) fn is_left(&self, mac: &str) -> bool {
        self.left.iter().any(|m| m.mac == mac)
    }

    fn wipe(&mut self) {
        for (_, mut s) in std::mem::take(&mut self.sections) {
            s.wipe();
        }
        for (_, mut k) in std::mem::take(&mut self.paragraphs) {
            k.wipe();
        }
        self.left.clear();
    }
}

/// The notes of the open session: each case's, read from the vault the first time it is
/// needed.
#[derive(Default)]
pub(crate) struct SessionNotes {
    cases: HashMap<String, CaseNotes>,
}

impl SessionNotes {
    pub(crate) fn get(&self, case_id: &str) -> Option<&CaseNotes> {
        self.cases.get(case_id)
    }

    /// A case erased from the vault: nothing of it stays in memory either.
    pub(crate) fn forget_case(&mut self, case_id: &str) {
        if let Some(mut n) = self.cases.remove(case_id) {
            n.wipe();
        }
    }

    /// On lock: overwritten, not only let go. The vault keeps its sealed copy.
    pub(crate) fn wipe(&mut self) {
        for (_, mut n) in self.cases.drain() {
            n.wipe();
        }
    }
}

/// A row that does not read as notes is reported without its words (they may be the case's).
fn unreadable() -> CoreError {
    CoreError::Vault(dv_vault::VaultError::Corrupt(
        "the notes of the case do not read".to_owned(),
    ))
}

/// The keyed hash of a paragraph's stored wording, for a note she left about it.
fn wording_mac(v: &dv_vault::Vault, case_id: &str, wording: &str) -> String {
    v.note_mac(case_id, &format!("wording|{wording}"))
}

/// The keyed hash of what the finish page calls a note.
fn left_mac(v: &dv_vault::Vault, case_id: &str, key: &str) -> String {
    v.note_mac(case_id, &format!("left|{key}"))
}

impl Core {
    /// The case's notes, read from the vault the first time this session needs them (D-193).
    pub(crate) fn case_notes(&mut self, case_id: &str) -> Result<&mut CaseNotes, CoreError> {
        if !self.notes.cases.contains_key(case_id) {
            let read = match self.vault_ref()?.case_notes(case_id)? {
                Some(json) => serde_json::from_str::<CaseNotes>(&json).map_err(|_| unreadable())?,
                None => CaseNotes::default(),
            };
            self.notes.cases.insert(case_id.to_owned(), read);
        }
        self.notes
            .cases
            .get_mut(case_id)
            .ok_or_else(|| CoreError::Internal("notes".to_owned()))
    }

    /// Keep the case's notes as they are now in its sealed row; with none, the row goes.
    pub(crate) fn save_notes(&mut self, case_id: &str) -> Result<(), CoreError> {
        let Some(notes) = self.notes.cases.get(case_id) else {
            return Ok(());
        };
        let json = if notes.is_empty() {
            None
        } else {
            Some(Zeroizing::new(serde_json::to_string(notes).map_err(
                |_| CoreError::Internal("the notes of the case were not written".to_owned()),
            )?))
        };
        self.vault_mut()?
            .set_case_notes(case_id, json.as_deref().map(String::as_str))?;
        Ok(())
    }

    /// Drop what is kept about the paragraphs that are gone or read differently now, and
    /// keep the rest (D-161). `on_page`: every paragraph of the case, by id, with its stored
    /// wording.
    pub(crate) fn prune_notes(
        &mut self,
        case_id: &str,
        on_page: &HashMap<String, String>,
    ) -> Result<(), CoreError> {
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let Some(notes) = self.notes.cases.get_mut(case_id) else {
            return Ok(());
        };
        if notes.prune(on_page, |w| wording_mac(v, case_id, w)) {
            self.save_notes(case_id)?;
        }
        Ok(())
    }

    /// "בסדר, להשאיר" on the finish page (D-193), or "להחזיר" with `left` false: the note
    /// the page calls `key` is left as it is. Kept with the case by a keyed hash of `key`. One
    /// about a paragraph (`draft_id`) goes when the paragraph's wording changes, as its notes
    /// do; a paragraph that is no longer there has nothing to leave.
    pub fn set_note_left(
        &mut self,
        case_id: &str,
        draft_id: Option<&str>,
        key: &str,
        left: bool,
    ) -> Result<(), CoreError> {
        let v = self.vault_ref()?;
        let mac = left_mac(v, case_id, key);
        let about = match draft_id {
            Some(id) if left => match style::find_draft(v, case_id, id)? {
                Some(d) => Some((d.id, wording_mac(v, case_id, &d.text_tagged))),
                None => return Ok(()),
            },
            _ => None,
        };
        if self.case_notes(case_id)?.set_left(mac, about, left) {
            self.save_notes(case_id)?;
        }
        Ok(())
    }

    /// Of `keys` (what the finish page calls the notes it shows), the ones she left as they
    /// are (D-193).
    pub fn left_notes(&mut self, case_id: &str, keys: &[String]) -> Result<Vec<String>, CoreError> {
        self.case_notes(case_id)?;
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let Some(notes) = self.notes.get(case_id) else {
            return Ok(Vec::new());
        };
        Ok(keys
            .iter()
            .filter(|k| notes.is_left(&left_mac(v, case_id, k)))
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(kind: NoteKind, text: &str) -> ParagraphNote {
        ParagraphNote::new(kind, text)
    }

    /// A stand-in for the vault's keyed hash.
    fn mac(w: &str) -> String {
        format!("#{w}")
    }

    #[test]
    fn a_paragraphs_notes_live_while_its_wording_stands() {
        let mut notes = CaseNotes::default();
        notes.keep_answer(
            "d1",
            "הטקסט".to_owned(),
            vec![note(NoteKind::NoSource, "הפסקה לא מציינת מקור")],
        );
        assert_eq!(notes.of_paragraph("d1", "הטקסט").len(), 1);
        // Another wording: nothing is said about it.
        assert!(notes.of_paragraph("d1", "טקסט אחר").is_empty());
        // The page as it is now: the paragraph was edited, so the note goes.
        let page = HashMap::from([("d1".to_owned(), "טקסט אחר".to_owned())]);
        assert!(notes.prune(&page, mac));
        assert!(notes.of_paragraph("d1", "הטקסט").is_empty());
        assert!(notes.is_empty());
        // Nothing more to drop: nothing changed.
        assert!(!notes.prune(&page, mac));
    }

    #[test]
    fn forgetting_one_case_keeps_the_notes_of_another() {
        let mut session = SessionNotes::default();
        for (case, draft) in [("c1", "d1"), ("c2", "d2")] {
            session
                .cases
                .entry(case.to_owned())
                .or_default()
                .keep_answer(draft, "א".to_owned(), vec![note(NoteKind::NoSource, "x")]);
        }
        session.forget_case("c1");
        assert!(session.get("c1").is_none());
        assert_eq!(session.get("c2").unwrap().of_paragraph("d2", "א").len(), 1);
    }

    #[test]
    fn a_verdict_joins_the_answers_notes_and_a_clean_one_clears_it() {
        let mut notes = CaseNotes::default();
        notes.keep_answer(
            "d",
            "ט".to_owned(),
            vec![note(NoteKind::NameInAnswer, "שם")],
        );
        notes.keep_verdict(
            "d",
            "ט".to_owned(),
            vec![note(NoteKind::VerifyUnsupported, "אין בסיס")],
        );
        let kinds: Vec<NoteKind> = notes
            .of_paragraph("d", "ט")
            .iter()
            .map(|n| n.kind)
            .collect();
        assert_eq!(kinds, [NoteKind::NameInAnswer, NoteKind::VerifyUnsupported]);
        // Checked again and supported: the verdict goes, the answer's note stays.
        notes.keep_verdict("d", "ט".to_owned(), Vec::new());
        let kinds: Vec<NoteKind> = notes
            .of_paragraph("d", "ט")
            .iter()
            .map(|n| n.kind)
            .collect();
        assert_eq!(kinds, [NoteKind::NameInAnswer]);
        // A verdict about a newer wording drops what was said about the older one.
        notes.keep_verdict(
            "d",
            "חדש".to_owned(),
            vec![note(NoteKind::VerifyPartial, "חלקי")],
        );
        assert!(notes.of_paragraph("d", "ט").is_empty());
        assert_eq!(notes.of_paragraph("d", "חדש").len(), 1);
    }

    #[test]
    fn a_new_answer_replaces_the_sections_notes_and_a_rewording_adds_to_them() {
        let mut notes = CaseNotes::default();
        notes.keep_section(
            "background",
            SectionNotes {
                questions: vec!["מתי התחיל?".to_owned()],
                unanswered: vec!["רקע משפחתי".to_owned()],
                ..SectionNotes::default()
            },
        );
        // A rewording that says nothing leaves the notes as they were.
        notes.keep_rewording("background", SectionNotes::default());
        assert_eq!(notes.section("background").questions.len(), 1);
        // One that asks something adds to what was asked and keeps the guide's lines.
        notes.keep_rewording(
            "background",
            SectionNotes {
                missing: vec!["גיל ההליכה".to_owned()],
                ..SectionNotes::default()
            },
        );
        let now = notes.section("background");
        assert_eq!(now.questions, ["מתי התחיל?"]);
        assert_eq!(now.missing, ["גיל ההליכה"]);
        assert_eq!(now.unanswered, ["רקע משפחתי"]);
        // The same line said again is kept once.
        notes.keep_rewording(
            "background",
            SectionNotes {
                missing: vec!["גיל ההליכה".to_owned()],
                ..SectionNotes::default()
            },
        );
        assert_eq!(notes.section("background").missing.len(), 1);
        // A whole new draft with nothing to say: nothing is kept.
        notes.keep_section("background", SectionNotes::default());
        assert_eq!(notes.section("background"), SectionNotes::default());
    }

    #[test]
    fn a_note_she_left_goes_with_its_paragraphs_wording_and_one_about_the_report_stays() {
        let mut notes = CaseNotes::default();
        assert!(notes.set_left("k1".into(), Some(("d".into(), mac("ט"))), true));
        assert!(notes.set_left("k2".into(), None, true));
        // Left twice is left once.
        assert!(!notes.set_left("k2".into(), None, true));
        assert!(notes.is_left("k1") && notes.is_left("k2"));
        // The paragraph reads as it did: kept.
        let page = HashMap::from([("d".to_owned(), "ט".to_owned())]);
        assert!(!notes.prune(&page, mac));
        assert!(notes.is_left("k1"));
        // Edited: the mark about it goes; the one about the whole report stays.
        let page = HashMap::from([("d".to_owned(), "אחר".to_owned())]);
        assert!(notes.prune(&page, mac));
        assert!(!notes.is_left("k1") && notes.is_left("k2"));
        // Brought back.
        assert!(notes.set_left("k2".into(), None, false));
        assert!(notes.is_empty());
    }

    #[test]
    fn kept_as_sealed_json_and_read_back_the_same() {
        let mut notes = CaseNotes::default();
        notes.keep_answer("d", "ט".to_owned(), vec![note(NoteKind::NoSource, "x")]);
        notes.keep_verdict(
            "d",
            "ט".to_owned(),
            vec![note(NoteKind::VerifyPartial, "y")],
        );
        notes.keep_section(
            "s",
            SectionNotes {
                questions: vec!["?".to_owned()],
                ..SectionNotes::default()
            },
        );
        notes.set_left("k".into(), Some(("d".into(), mac("ט"))), true);
        let json = serde_json::to_string(&notes).unwrap();
        let back: CaseNotes = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
        assert_eq!(back.of_paragraph("d", "ט"), notes.of_paragraph("d", "ט"));
        assert_eq!(back.section("s"), notes.section("s"));
        assert!(back.is_left("k"));
        // Nothing kept: the smallest row, and an empty one reads.
        assert_eq!(serde_json::to_string(&CaseNotes::default()).unwrap(), "{}");
        assert!(serde_json::from_str::<CaseNotes>("{}").unwrap().is_empty());
    }

    #[test]
    fn the_lock_wipes_the_sessions_copy() {
        let mut session = SessionNotes::default();
        let notes = session.cases.entry("c".to_owned()).or_default();
        notes.keep_answer("d", "ט".to_owned(), vec![note(NoteKind::NoSource, "x")]);
        notes.keep_section(
            "s",
            SectionNotes {
                questions: vec!["?".to_owned()],
                ..SectionNotes::default()
            },
        );
        session.wipe();
        assert!(session.get("c").is_none());
    }
}
