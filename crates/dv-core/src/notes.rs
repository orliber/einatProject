//! No note gets lost (D-161): what an answer and the check against the sources had to say,
//! kept while the vault is open so the report page can show it after any reload.
//!
//! Memory only, never the vault: the lock wipes it like every other state of the session.
//! Per case and section: the questions the model asked, what it found missing or
//! contradicting, and the guide's lines no paragraph answers. Per draft paragraph: what was
//! found when its answer came (a name in the answer, a sentence of a past report, no source)
//! and the last verdict of the check against the sources. A paragraph's notes belong to its
//! wording: once the text she sees is another one (an edit, a new wording) or the paragraph
//! is gone (removed, replaced by a new draft), they are dropped.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroize;

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
    pub questions: Vec<String>,
    /// What it found missing in the materials.
    pub missing: Vec<String>,
    /// Where the materials contradict each other.
    pub contradictions: Vec<String>,
    /// The lines of the section's guide that no paragraph answers.
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
struct Kept {
    case_id: String,
    /// The paragraph's stored (tagged) wording then. Not the text with the names put back:
    /// a name she corrects, or her own name in the settings, does not change what was said
    /// about the wording, and must not drop a note that holds the paragraph back.
    wording: String,
    /// Found when the answer came, and not worked out again on each reading.
    answer: Vec<ParagraphNote>,
    /// The last verdict of the check against the sources that is worth a look.
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

/// The notes of the open session.
#[derive(Default)]
pub(crate) struct SessionNotes {
    sections: HashMap<(String, String), SectionNotes>,
    paragraphs: HashMap<String, Kept>,
}

impl SessionNotes {
    /// The answer for a whole section: its notes take the place of the earlier ones.
    pub(crate) fn keep_section(&mut self, case_id: &str, section_key: &str, notes: SectionNotes) {
        let key = (case_id.to_owned(), section_key.to_owned());
        if let Some(mut old) = self.sections.remove(&key) {
            old.wipe();
        }
        if !notes.is_empty() {
            self.sections.insert(key, notes);
        }
    }

    /// The answer that worded one paragraph again: what it asked or found missing is said
    /// now; the guide's lines are the whole draft's and stay as they were.
    pub(crate) fn keep_rewording(&mut self, case_id: &str, section_key: &str, notes: SectionNotes) {
        if notes.questions.is_empty() && notes.missing.is_empty() && notes.contradictions.is_empty()
        {
            return;
        }
        // Added to what the draft's answer said, never instead of it: a question the first
        // answer asked is still open after one paragraph was worded again.
        let mut kept = self.section(case_id, section_key);
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
        self.keep_section(case_id, section_key, kept);
    }

    pub(crate) fn section(&self, case_id: &str, section_key: &str) -> SectionNotes {
        self.sections
            .get(&(case_id.to_owned(), section_key.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    /// What was found when the paragraph's answer came. A new answer starts the paragraph's
    /// notes over: an earlier verdict was about the earlier wording.
    pub(crate) fn keep_answer(
        &mut self,
        case_id: &str,
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
                case_id: case_id.to_owned(),
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
        case_id: &str,
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
                            case_id: case_id.to_owned(),
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

    /// Drop the notes of the case's paragraphs that are gone or read differently now.
    /// `on_page`: every paragraph on the page, by id, with its stored wording.
    pub(crate) fn prune(&mut self, case_id: &str, on_page: &HashMap<String, String>) {
        let gone: Vec<String> = self
            .paragraphs
            .iter()
            .filter(|(id, k)| k.case_id == case_id && on_page.get(*id) != Some(&k.wording))
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            self.forget(&id);
        }
    }

    /// On lock: overwritten, not only let go.
    pub(crate) fn wipe(&mut self) {
        for (_, mut s) in self.sections.drain() {
            s.wipe();
        }
        for (_, mut k) in self.paragraphs.drain() {
            k.wipe();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(kind: NoteKind, text: &str) -> ParagraphNote {
        ParagraphNote::new(kind, text)
    }

    #[test]
    fn a_paragraphs_notes_live_while_its_wording_stands() {
        let mut notes = SessionNotes::default();
        notes.keep_answer(
            "c1",
            "d1",
            "הטקסט".to_owned(),
            vec![note(NoteKind::NoSource, "הפסקה לא מציינת מקור")],
        );
        assert_eq!(notes.of_paragraph("d1", "הטקסט").len(), 1);
        // Another wording: nothing is said about it.
        assert!(notes.of_paragraph("d1", "טקסט אחר").is_empty());
        // The page as it is now: the paragraph was edited, so the note goes.
        let page = HashMap::from([("d1".to_owned(), "טקסט אחר".to_owned())]);
        notes.prune("c1", &page);
        assert!(notes.of_paragraph("d1", "הטקסט").is_empty());
    }

    #[test]
    fn pruning_one_case_keeps_the_notes_of_another() {
        let mut notes = SessionNotes::default();
        notes.keep_answer(
            "c1",
            "d1",
            "א".to_owned(),
            vec![note(NoteKind::NoSource, "x")],
        );
        notes.keep_answer(
            "c2",
            "d2",
            "ב".to_owned(),
            vec![note(NoteKind::NoSource, "y")],
        );
        notes.prune("c1", &HashMap::new());
        assert!(notes.of_paragraph("d1", "א").is_empty());
        assert_eq!(notes.of_paragraph("d2", "ב").len(), 1);
    }

    #[test]
    fn a_verdict_joins_the_answers_notes_and_a_clean_one_clears_it() {
        let mut notes = SessionNotes::default();
        notes.keep_answer(
            "c",
            "d",
            "ט".to_owned(),
            vec![note(NoteKind::NameInAnswer, "שם")],
        );
        notes.keep_verdict(
            "c",
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
        notes.keep_verdict("c", "d", "ט".to_owned(), Vec::new());
        let kinds: Vec<NoteKind> = notes
            .of_paragraph("d", "ט")
            .iter()
            .map(|n| n.kind)
            .collect();
        assert_eq!(kinds, [NoteKind::NameInAnswer]);
        // A verdict about a newer wording drops what was said about the older one.
        notes.keep_verdict(
            "c",
            "d",
            "חדש".to_owned(),
            vec![note(NoteKind::VerifyPartial, "חלקי")],
        );
        assert!(notes.of_paragraph("d", "ט").is_empty());
        assert_eq!(notes.of_paragraph("d", "חדש").len(), 1);
    }

    #[test]
    fn a_new_answer_replaces_the_sections_notes_and_a_rewording_adds_to_them() {
        let mut notes = SessionNotes::default();
        notes.keep_section(
            "c",
            "background",
            SectionNotes {
                questions: vec!["מתי התחיל?".to_owned()],
                unanswered: vec!["רקע משפחתי".to_owned()],
                ..SectionNotes::default()
            },
        );
        // A rewording that says nothing leaves the notes as they were.
        notes.keep_rewording("c", "background", SectionNotes::default());
        assert_eq!(notes.section("c", "background").questions.len(), 1);
        // One that asks something adds to what was asked and keeps the guide's lines.
        notes.keep_rewording(
            "c",
            "background",
            SectionNotes {
                missing: vec!["גיל ההליכה".to_owned()],
                ..SectionNotes::default()
            },
        );
        let now = notes.section("c", "background");
        assert_eq!(now.questions, ["מתי התחיל?"]);
        assert_eq!(now.missing, ["גיל ההליכה"]);
        assert_eq!(now.unanswered, ["רקע משפחתי"]);
        // The same line said again is kept once.
        notes.keep_rewording(
            "c",
            "background",
            SectionNotes {
                missing: vec!["גיל ההליכה".to_owned()],
                ..SectionNotes::default()
            },
        );
        assert_eq!(notes.section("c", "background").missing.len(), 1);
        // A whole new draft with nothing to say: nothing is kept.
        notes.keep_section("c", "background", SectionNotes::default());
        assert_eq!(notes.section("c", "background"), SectionNotes::default());
    }

    #[test]
    fn the_lock_wipes_everything() {
        let mut notes = SessionNotes::default();
        notes.keep_answer(
            "c",
            "d",
            "ט".to_owned(),
            vec![note(NoteKind::NoSource, "x")],
        );
        notes.keep_section(
            "c",
            "s",
            SectionNotes {
                questions: vec!["?".to_owned()],
                ..SectionNotes::default()
            },
        );
        notes.wipe();
        assert!(notes.of_paragraph("d", "ט").is_empty());
        assert_eq!(notes.section("c", "s"), SectionNotes::default());
    }
}
