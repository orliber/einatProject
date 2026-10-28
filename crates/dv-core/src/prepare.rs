//! Preparing a request away from the core's lock (D-109): a section's, the check against the
//! sources, a consultation, the sorting of the materials and the check of the whole report.
//!
//! The filter reads every text that goes out with a section (its materials, the drafts, the
//! conversation, her style and guide), about 120 ms a section on her computer, and the core
//! used to hold its one lock all along: typing stalled while a request was prepared, and
//! "write the empty sections" held it for every section in a row. It is now cut like the
//! other long work (F2, `jobs`):
//!
//! 1. `begin_*` reads the texts and the case's privacy data under the lock, in a moment.
//! 2. [`RequestJob::run`] filters the texts, builds the request and asks the gate, with no
//!    access to the core. The sections of "write the empty sections" run side by side
//!    ([`run_requests`]), sharing one mask book, so they never give one number to two dates.
//! 3. [`Core::finish_request`] lands it under the lock. Only there does a cleared payload
//!    become an approval that can be sent.
//!
//! The gate's contract is unchanged: nothing is sent without a `ClearedPayload` from
//! `dv_privacy::gate`, and every doubt ends with no approval. The vault locked in between, the
//! approvals dropped, an error, a cancel or a panic in the worker: the result is dropped and
//! nothing lands. A change in what the filter decides by (a name of any case, hers, "not a
//! name", the past reports' names) or a numbered tag another request gave first: the request
//! is prepared again from the start, as if it had begun after the change. So is one whose
//! material was changed or deleted, whose case went to the recycle bin, or whose section she took
//! to write herself; each drops the approvals waiting for the case too.

use std::sync::Mutex;

use super::*;
use crate::drafts::CLOSING_SECTIONS;
use crate::send::{clear_request, filter_sources, read_sources, Cleared, SourceRead};
use dv_domain::GrammaticalGender;
use dv_vault::TokenHasher;
use zeroize::Zeroize;

/// Tries away from the lock for a request whose ground changed while it was filtered. The
/// next one is filtered under the lock, where nothing can change, so every request ends.
pub(crate) const MAX_TRIES: u32 = 3;

/// The request she asked for, kept to prepare it again.
#[derive(Clone)]
struct SectionAsk {
    case_id: String,
    section_key: String,
    instruction: String,
    replaces: Option<String>,
    /// Worded from her points only (round 30).
    points: bool,
    /// The check of the section's proposed paragraphs against its sources, not a draft.
    check: bool,
}

impl Drop for SectionAsk {
    fn drop(&mut self) {
        self.instruction.zeroize();
    }
}

/// A free question to the AI, optionally about a case, continuing a saved conversation or not.
#[derive(Clone)]
struct ConsultAsk {
    case_id: Option<String>,
    conversation_id: Option<String>,
    message: String,
}

impl Drop for ConsultAsk {
    fn drop(&mut self) {
        self.message.zeroize();
    }
}

/// What a request was asked for, kept to prepare it again from the start.
#[derive(Clone)]
enum Ask {
    Section(SectionAsk),
    Consult(ConsultAsk),
    /// Sorting the case's materials into sections (D-022): one material, or (`None`) every
    /// material not sorted yet.
    Sort(String, Option<String>),
    /// The whole approved report, read as one (D-097).
    ReportCheck(String),
}

impl Ask {
    /// The case the request is about; empty for a consultation without one.
    fn case_id(&self) -> &str {
        match self {
            Ask::Section(a) => &a.case_id,
            Ask::Consult(a) => a.case_id.as_deref().unwrap_or(""),
            Ask::Sort(c, _) | Ask::ReportCheck(c) => c,
        }
    }

    /// What it is, for logs that must hold nothing of the case.
    fn what(&self) -> &str {
        match self {
            Ask::Section(a) if a.check => "verify",
            Ask::Section(a) => &a.section_key,
            Ask::Consult(_) => "consult",
            Ask::Sort(..) => "sort",
            Ask::ReportCheck(_) => "report_check",
        }
    }
}

/// The texts of a section's request as the vault holds them, read under the lock and filtered
/// away from it. Wiped when dropped.
struct SectionRead {
    section: dv_domain::ReportSection,
    age: Option<String>,
    gender: Option<GrammaticalGender>,
    /// The title the materials are cut by (G4): the section's in the template.
    trim_title: String,
    sources: Vec<SourceRead>,
    /// The approved sections it is written from, each with its paragraphs joined.
    approved: Vec<(dv_domain::ReportSection, String)>,
    /// The current draft: it may hold her manual edits.
    current: Vec<String>,
    history: Vec<(ChatRole, String)>,
    style: Option<style::StyleSource>,
    interpretations: Option<String>,
    patterns: Vec<dv_domain::ScorePattern>,
    guide: String,
    severity: Option<String>,
}

impl Drop for SectionRead {
    fn drop(&mut self) {
        for (_, t) in &mut self.approved {
            t.zeroize();
        }
        for t in &mut self.current {
            t.zeroize();
        }
        for (_, t) in &mut self.history {
            t.zeroize();
        }
        self.guide.zeroize();
        self.interpretations.zeroize();
        self.severity.zeroize();
        self.age.zeroize();
        for p in &mut self.patterns {
            p.key.zeroize();
            p.label.zeroize();
        }
    }
}

/// The check of a section against its sources, as the vault holds it. Wiped when dropped.
struct CheckRead {
    section: dv_domain::ReportSection,
    trim_title: String,
    sources: Vec<SourceRead>,
    /// The proposed paragraphs, by id.
    paragraphs: Vec<(String, String)>,
}

impl Drop for CheckRead {
    fn drop(&mut self) {
        for (_, t) in &mut self.paragraphs {
            t.zeroize();
        }
    }
}

/// A consultation's texts as the vault holds them. Wiped when dropped.
struct ConsultRead {
    /// The saved conversation's turns: role, her words as she typed them, and as they went out.
    earlier: Vec<(String, String, String)>,
    /// The case's approved paragraphs, with the section each is in.
    approved: Vec<(dv_domain::ReportSection, String)>,
}

impl Drop for ConsultRead {
    fn drop(&mut self) {
        for (_, shown, tagged) in &mut self.earlier {
            shown.zeroize();
            tagged.zeroize();
        }
        for (_, t) in &mut self.approved {
            t.zeroize();
        }
    }
}

/// A material not sorted yet, as the vault holds it.
struct SortItem {
    id: String,
    kind: InputKind,
    title: String,
    content: String,
    /// Where its passages are, as the sorting cuts it.
    ranges: Vec<std::ops::Range<usize>>,
}

impl Drop for SortItem {
    fn drop(&mut self) {
        self.title.zeroize();
        self.content.zeroize();
    }
}

struct SortRead {
    sections: Vec<dv_ai::SortSection>,
    items: Vec<SortItem>,
}

/// The approved report, section by section: each with its approved paragraphs.
struct ReportCheckRead {
    sections: Vec<(dv_domain::ReportSection, Vec<String>)>,
}

impl Drop for ReportCheckRead {
    fn drop(&mut self) {
        for (_, texts) in &mut self.sections {
            for t in texts {
                t.zeroize();
            }
        }
    }
}

enum Texts {
    Section(Box<SectionRead>),
    Check(CheckRead),
    Consult(ConsultRead),
    Sort(SortRead),
    ReportCheck(ReportCheckRead),
}

/// A section's request read under the lock, waiting to be filtered away from it.
pub struct RequestJob {
    session: u64,
    epoch: u64,
    cancel: jobs::Cancel,
    /// What the filter decided by when it was read ([`PrivacyData::fingerprint`]).
    fingerprint: String,
    /// The names this request found were kept, and it was built again: nothing new is kept.
    learned: bool,
    tries: u32,
    ask: Ask,
    data: PrivacyData,
    hasher: TokenHasher,
    hints: NameHints,
    model: ModelConfig,
    demo_mode: bool,
    nonce: String,
    read: Texts,
    /// Tests: the worker panics in the middle of the filter.
    #[cfg(test)]
    pub(crate) panics: bool,
}

impl std::fmt::Debug for RequestJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestJob")
            .field("request", &self.ask.what())
            .field("tries", &self.tries)
            .finish_non_exhaustive()
    }
}

/// What the filter and the gate made of a [`RequestJob`], waiting for
/// [`Core::finish_request`]. Nothing in it can be sent until it lands there.
pub struct RequestRun {
    session: u64,
    epoch: u64,
    fingerprint: String,
    learned: bool,
    tries: u32,
    ask: Ask,
    data: PrivacyData,
    pub(crate) built: Result<Built, CoreError>,
}

impl std::fmt::Debug for RequestRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestRun")
            .field("request", &self.ask.what())
            .field("ok", &self.built.is_ok())
            .finish_non_exhaustive()
    }
}

pub(crate) struct Built {
    prepared: Prepared,
    kind: PendingKind,
    cleared: Cleared,
}

/// A request landed: ready for the review screen (with its approval when the gate cleared
/// it), or begun again because its ground changed while it was filtered.
#[derive(Debug)]
pub enum Landed {
    Ready(Prepared),
    Again(Box<RequestJob>),
}

/// On a lock while a job ran: the filter's index of every case's names, which the job may
/// have built again after the lock dropped it, goes too. Also when the job panics.
struct ForgetOnLock<'a>(&'a jobs::Cancel);

impl Drop for ForgetOnLock<'_> {
    fn drop(&mut self) {
        if self.0.is_set() {
            dv_privacy::forget_identity_index();
        }
    }
}

impl RequestJob {
    /// Filter, build and ask the gate. Reads nothing of the core.
    #[must_use]
    pub fn run(self) -> RequestRun {
        let built = {
            let _forget = ForgetOnLock(&self.cancel);
            self.build()
        };
        let RequestJob {
            session,
            epoch,
            fingerprint,
            learned,
            tries,
            ask,
            data,
            ..
        } = self;
        RequestRun {
            session,
            epoch,
            fingerprint,
            learned,
            tries,
            ask,
            data,
            built,
        }
    }

    fn build(&self) -> Result<Built, CoreError> {
        let stop = || {
            if self.cancel.is_set() {
                Err(CoreError::Locked)
            } else {
                Ok(())
            }
        };
        stop()?;
        let lookups = self.data.lookups(&self.hasher);
        let ctx = lookups.masked_context();
        let (body, review, mut kind) = match (&self.ask, &self.read) {
            (Ask::Section(ask), Texts::Section(read)) => {
                build_section(self, ask, read, &ctx, &stop)?
            }
            (Ask::Section(ask), Texts::Check(read)) => build_check(self, ask, read, &ctx, &stop)?,
            (Ask::Consult(ask), Texts::Consult(read)) => {
                build_consult(self, ask, read, &ctx, &stop)?
            }
            (Ask::Sort(case_id, _), Texts::Sort(read)) => {
                build_sort(self, case_id, read, &ctx, &stop)?
            }
            (Ask::ReportCheck(case_id), Texts::ReportCheck(read)) => {
                build_report_check(self, case_id, read, &ctx, &stop)?
            }
            _ => {
                return Err(CoreError::Internal(
                    "הבקשה והטקסטים שלה לא תואמים.".to_owned(),
                ))
            }
        };
        #[cfg(test)]
        #[allow(clippy::panic)]
        if self.panics {
            panic!("the filter's worker stopped in the middle");
        }
        stop()?;
        let prepared = review.into_case_prepared(self.demo_mode, &self.data);
        // A consultation keeps what was hidden in it, and its own book when it has no case.
        if let PendingKind::Consult { hidden, masks, .. } = &mut kind {
            *hidden = prepared.hidden.clone();
            *masks = self.data.masks.entries();
        }
        let cleared = clear_request(&self.data, &lookups, &body, &prepared, !self.learned);
        stop()?;
        Ok(Built {
            prepared,
            kind,
            cleared,
        })
    }
}

/// Run each job on a worker of its own, as many at a time as the computer has cores, in the
/// order given back. A worker that panicked fails them all: nothing of the batch lands.
pub fn run_requests(
    jobs: Vec<(String, RequestJob)>,
) -> Result<Vec<(String, RequestRun)>, CoreError> {
    let total = jobs.len();
    let workers = std::thread::available_parallelism()
        .map_or(2, std::num::NonZeroUsize::get)
        .min(total)
        .max(1);
    let queue = Mutex::new(jobs.into_iter().enumerate().rev().collect::<Vec<_>>());
    let out = Mutex::new(Vec::with_capacity(total));
    let whole = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| loop {
                    let next = match queue.lock() {
                        Ok(mut q) => q.pop(),
                        Err(_) => None,
                    };
                    let Some((n, (key, job))) = next else {
                        break;
                    };
                    let ran = job.run();
                    if let Ok(mut o) = out.lock() {
                        o.push((n, key, ran));
                    }
                })
            })
            .collect();
        // Every worker is joined, one that panicked included: a scope left with a panicked
        // worker not joined panics itself.
        let joined: Vec<bool> = handles.into_iter().map(|h| h.join().is_ok()).collect();
        joined.iter().all(|ok| *ok)
    });
    let mut out = out.into_inner().unwrap_or_default();
    if !whole || out.len() != total {
        return Err(CoreError::Internal(
            "הכנת הבקשות נעצרה באמצע, ושום דבר לא נשלח.".to_owned(),
        ));
    }
    out.sort_by_key(|(n, ..)| *n);
    Ok(out.into_iter().map(|(_, key, ran)| (key, ran)).collect())
}

/// What the filter makes of the section's texts: the request's body, the review screen and
/// what the answer is stored with. Called away from the lock; `stop` ends it on a lock.
fn build_section(
    job: &RequestJob,
    ask: &SectionAsk,
    read: &SectionRead,
    ctx: &PrivacyContext<'_>,
    stop: &dyn Fn() -> Result<(), CoreError>,
) -> Result<(Value, Review, PendingKind), CoreError> {
    let hints = &job.hints;
    let points = ask.points;
    let run = |t: &str| {
        stop()?;
        hints
            .filter(t, ctx)
            .map_err(|e| CoreError::Internal(e.to_string()))
    };
    let fixed_title = |sec: &dv_domain::ReportSection| model_title(sec, ctx);
    let mut review = Review::default();
    let (tagged_sources, source_rows) = filter_sources(
        &read.sources,
        &ask.section_key,
        &read.trim_title,
        ctx,
        hints,
        &mut review,
        stop,
    )?;
    let mut approved_context = Vec::new();
    for (s, text) in &read.approved {
        let out = run(text)?;
        review.add(format!("סעיף מאושר · {}", s.title), &out);
        approved_context.push((fixed_title(s)?, out.tagged));
    }
    let mut current = Vec::new();
    for t in &read.current {
        let out = run(t)?;
        review.add("הטיוטה הנוכחית".to_owned(), &out);
        current.push(out.tagged);
    }
    // The conversation so far is not shown again, but what the filter hides in it is kept
    // like everything else, so its tags belong to the case.
    let mut history: Vec<TaggedTurn> = Vec::new();
    for (role, text) in &read.history {
        let o = run(text)?;
        review.note_hidden(&o);
        history.push(TaggedTurn {
            role: if *role == ChatRole::User {
                "user".to_owned()
            } else {
                "assistant".to_owned()
            },
            text_tagged: o.tagged,
        });
    }
    let instr = run(&ask.instruction)?;
    review.add(
        if points {
            "הנקודות שלך"
        } else {
            "הבקשה שלך"
        }
        .to_owned(),
        &instr,
    );
    // Her approved style profile (D-043), as it goes out with this case's names hidden.
    // The items that fit this section's material best go out (D-052).
    let query: String = tagged_sources
        .iter()
        .map(|s| s.content_tagged.as_str())
        .chain([instr.tagged.as_str()])
        .collect::<Vec<_>>()
        .join("\n");
    stop()?;
    let mut style_profile = read
        .style
        .as_ref()
        .and_then(|s| s.render(ctx, &ask.section_key, &query, &read.patterns));
    // How she interprets the scores of this case (round 37), in a section written from the
    // scores: the approved interpretations that fit each score.
    if let Some(text) = &read.interpretations {
        style_profile = Some(match style_profile {
            Some(p) => format!("{p}\n{text}"),
            None => text.clone(),
        });
    }
    if let Some(text) = &style_profile {
        review.add_context("פרופיל הסגנון שלך".to_owned(), &run(text)?);
    }
    // How she wants the section written (round 29), filtered like her request: the case's own
    // lines may name someone.
    let guide_tagged = if read.guide.is_empty() {
        String::new()
    } else {
        let out = run(&read.guide)?;
        review.add("איך לכתוב את הסעיף".to_owned(), &out);
        out.tagged
    };
    // A first draft of the whole section is checked against the guide's lines, when it has
    // two or more; a rewrite of one paragraph or a turn on a draft is not.
    let lines = dv_ai::guide_lines(&guide_tagged);
    let guide_lines: Vec<String> =
        if current.is_empty() && ask.replaces.is_none() && lines.len() >= 2 {
            lines.into_iter().map(str::to_owned).collect()
        } else {
            Vec::new()
        };
    // The DSM-5 severity she set (round 43) goes with the DSM section as fixed words, after
    // the guide, so the model writes her levels and no other; while she has set none, that it
    // writes none (review of stage 6, R3).
    let guide_tagged = match &read.severity {
        Some(text) => {
            let out = run(text)?;
            review.add("רמת החומרה שקבעת".to_owned(), &out);
            if guide_tagged.is_empty() {
                out.tagged
            } else {
                format!("{guide_tagged}\n{}", out.tagged)
            }
        }
        None => guide_tagged,
    };
    let input = SectionInput {
        section_key: read.section.key.clone(),
        section_title: fixed_title(&read.section)?,
        age: read.age.clone(),
        gender: read.gender,
        sources: tagged_sources,
        approved_context,
        current_draft: current,
        history,
        instruction_tagged: instr.tagged.clone(),
        style_profile,
        guide_tagged,
        points_only: points,
        sources_in_order: ask.section_key == sorting::PRIOR_SECTION && !points,
    };
    // From her points, the numbers in the answer are checked against the points.
    let approved = if points {
        vec![instr.tagged.clone()]
    } else {
        input
            .approved_context
            .iter()
            .map(|(_, t)| t.clone())
            .collect::<Vec<_>>()
    };
    stop()?;
    let (body, _refs) = dv_ai::build_section_request(&job.model, &input, &job.nonce);
    let kind = PendingKind::Section {
        case_id: ask.case_id.clone(),
        section_key: read.section.key.clone(),
        instruction_tagged: instr.tagged,
        hidden: instr.hidden,
        sources: source_rows,
        approved,
        replaces: ask.replaces.clone(),
        guide: guide_lines,
        from_points: points,
    };
    Ok((body, review, kind))
}

/// The check against the sources (the verify stage): the section's sources and its proposed
/// paragraphs, filtered. Called away from the lock; `stop` ends it on a lock.
fn build_check(
    job: &RequestJob,
    ask: &SectionAsk,
    read: &CheckRead,
    ctx: &PrivacyContext<'_>,
    stop: &dyn Fn() -> Result<(), CoreError>,
) -> Result<(Value, Review, PendingKind), CoreError> {
    let mut review = Review::default();
    let (sources, _rows) = filter_sources(
        &read.sources,
        &ask.section_key,
        &read.trim_title,
        ctx,
        &job.hints,
        &mut review,
        stop,
    )?;
    let mut paragraphs = Vec::new();
    let mut draft_ids = Vec::new();
    for (id, text) in &read.paragraphs {
        stop()?;
        let out = filter(text, ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
        review.add("פסקה לבדיקה".to_owned(), &out);
        paragraphs.push(out.tagged);
        draft_ids.push(id.clone());
    }
    let input = dv_ai::VerifyInput {
        section_title: model_title(&read.section, ctx)?,
        sources,
        paragraphs,
    };
    stop()?;
    let (body, _paragraph_ids, source_ids) =
        dv_ai::build_verify_request(&job.model, &input, &job.nonce);
    let kind = PendingKind::Verify {
        case_id: ask.case_id.clone(),
        section_key: ask.section_key.clone(),
        draft_ids,
        source_ids,
    };
    Ok((body, review, kind))
}

/// A consultation: her question, the conversation so far and the case's approved sections,
/// filtered. Called away from the lock; `stop` ends it on a lock.
fn build_consult(
    job: &RequestJob,
    ask: &ConsultAsk,
    read: &ConsultRead,
    ctx: &PrivacyContext<'_>,
    stop: &dyn Fn() -> Result<(), CoreError>,
) -> Result<(Value, Review, PendingKind), CoreError> {
    let internal = |e: dv_privacy::PrivacyError| CoreError::Internal(e.to_string());
    stop()?;
    let msg = job.hints.filter(&ask.message, ctx).map_err(internal)?;
    let mut review = Review::default();
    let mut history = Vec::new();
    for (n, (role, shown, tagged)) in read.earlier.iter().enumerate() {
        stop()?;
        let label = |who: &str| format!("{who} ({})", n / 2 + 1);
        if role == "user" {
            // Her own words: filtered again, with today's names and decisions.
            let out = filter(shown, ctx).map_err(internal)?;
            review.add(label("שאלה קודמת"), &out);
            history.push(TaggedTurn {
                role: role.clone(),
                text_tagged: out.tagged,
            });
        } else {
            // Claude's own answer goes back as it came (it only ever saw tags); shown on the
            // review screen, without name questions about Claude's own words.
            let out = filter(tagged, ctx).map_err(internal)?;
            review.add_context(label("תשובה קודמת"), &out);
            history.push(TaggedTurn {
                role: role.clone(),
                text_tagged: tagged.clone(),
            });
        }
    }
    review.add("השאלה שלך".to_owned(), &msg);
    let mut case_context = None;
    if !read.approved.is_empty() {
        let mut lines = Vec::new();
        for (s, text) in &read.approved {
            lines.push(format!("{}: {}", model_title(s, ctx)?, text));
        }
        stop()?;
        let mut joined = lines.join("\n");
        for l in &mut lines {
            l.zeroize();
        }
        let out = filter(&joined, ctx).map_err(internal);
        joined.zeroize();
        let out = out?;
        review.add("סיכום התיק (סעיפים מאושרים)".to_owned(), &out);
        case_context = Some(out.tagged);
    }
    let input = dv_ai::ConsultInput {
        history,
        message_tagged: msg.tagged,
        case_context_tagged: case_context,
    };
    stop()?;
    let body = dv_ai::build_consult_request(&job.model, &input, &job.nonce);
    let kind = PendingKind::Consult {
        case_id: ask.case_id.clone(),
        conversation_id: ask.conversation_id.clone(),
        message_tagged: input.message_tagged,
        message_shown: ask.message.clone(),
        // Filled in once the review screen is made, with the request's own book.
        hidden: Vec::new(),
        masks: Vec::new(),
    };
    Ok((body, review, kind))
}

/// Sorting: each material not sorted yet, cut into passages and filtered. Called away from the
/// lock; `stop` ends it on a lock.
fn build_sort(
    job: &RequestJob,
    case_id: &str,
    read: &SortRead,
    ctx: &PrivacyContext<'_>,
    stop: &dyn Fn() -> Result<(), CoreError>,
) -> Result<(Value, Review, PendingKind), CoreError> {
    let internal = |e: dv_privacy::PrivacyError| CoreError::Internal(e.to_string());
    let hints = &job.hints;
    let mut review = Review::default();
    let mut materials = Vec::new();
    let mut rows = Vec::new();
    // Materials that cannot be cut into passages right now (see below), by title.
    let mut uncut: Vec<String> = Vec::new();
    for inp in &read.items {
        stop()?;
        let (whole, parts) = hints
            .filter_split(&inp.content, ctx, &inp.ranges)
            .map_err(internal)?;
        // A name across a passage edge: this material stays on the table for now.
        let Some(parts) = parts else {
            uncut.push(inp.title.clone());
            continue;
        };
        let title = hints.filter(&inp.title, ctx).map_err(internal)?;
        let sid = format!("S{}", materials.len() + 1);
        if title.original_segments.iter().any(|s| s.mark.is_some()) {
            review.add(format!("{sid} · {} · כותרת", inp.kind.label_he()), &title);
            review.add(format!("{sid} · {}", inp.kind.label_he()), &whole);
        } else {
            review.add(
                format!("{sid} · {} · {}", inp.kind.label_he(), title.tagged),
                &whole,
            );
        }
        let passages_tagged: Vec<String> = parts.into_iter().map(|p| p.tagged).collect();
        rows.push((
            inp.id.clone(),
            inp.ranges.len(),
            sorting::fingerprint(&inp.content),
            passages_tagged.join("\n\n"),
        ));
        materials.push(dv_ai::SortMaterial {
            input_id: inp.id.clone(),
            kind_label: inp.kind.label_he().to_owned(),
            title_tagged: title.tagged,
            passages_tagged,
        });
    }
    if materials.is_empty() {
        // Shown only here, on this computer (titles may hold names).
        return Err(CoreError::Refused(if uncut.is_empty() {
            "כל החומרים כבר ממוינים לסעיפים.".to_owned()
        } else {
            format!(
                "אי אפשר למיין אוטומטית את: {}. שם חשוד נמשך שם על פני שתי שורות, ולכן \
                 החומר לא נחתך לקטעים. אפשר לבחור לו סעיפים בעצמך (\"שינוי הסעיפים\").",
                uncut.join(", ")
            )
        }));
    }
    let input = dv_ai::SortInput {
        sections: read.sections.clone(),
        materials,
    };
    stop()?;
    let body = dv_ai::build_sort_request(&job.model, &input, &job.nonce);
    let kind = PendingKind::Sort {
        case_id: case_id.to_owned(),
        materials: rows,
        sections: input.sections.iter().map(|s| s.key.clone()).collect(),
    };
    Ok((body, review, kind))
}

/// The check of the whole report: each approved paragraph filtered by itself, section by
/// section. Called away from the lock; `stop` ends it on a lock.
fn build_report_check(
    job: &RequestJob,
    case_id: &str,
    read: &ReportCheckRead,
    ctx: &PrivacyContext<'_>,
    stop: &dyn Fn() -> Result<(), CoreError>,
) -> Result<(Value, Review, PendingKind), CoreError> {
    let mut review = Review::default();
    let mut sections = Vec::new();
    let mut keys = Vec::new();
    for (s, texts) in &read.sections {
        let mut text = Vec::new();
        for t in texts {
            stop()?;
            let out = job
                .hints
                .filter(t, ctx)
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            review.add(format!("סעיף מאושר · {}", s.title), &out);
            text.push(out.tagged);
        }
        sections.push(dv_ai::ReportCheckSection {
            title: model_title(s, ctx)?,
            text_tagged: text.join("\n"),
            closing: CLOSING_SECTIONS.contains(&s.key.as_str()),
        });
        keys.push(s.key.clone());
    }
    stop()?;
    let input = dv_ai::ReportCheckInput { sections };
    let (body, ids) = dv_ai::build_report_check_request(&job.model, &input, &job.nonce);
    let kind = PendingKind::ReportCheck {
        case_id: case_id.to_owned(),
        section_keys: keys,
        ids,
    };
    Ok((body, review, kind))
}

/// A numbered tag this request gave that the case now keeps for other words, or words the
/// case now keeps under another tag: another request was cleared first (D-109).
fn masks_taken(kept: &[dv_domain::MaskEntry], added: &[dv_domain::MaskEntry]) -> bool {
    added.iter().any(|a| {
        kept.iter().any(|k| {
            let same_words = k.kind == a.kind && normalize(&k.original) == normalize(&a.original);
            (k.tag == a.tag) != same_words
        })
    })
}

impl Core {
    /// Drop every approval waiting (a lock, an answer that changes what a request holds): a
    /// request still being filtered then never lands either.
    pub(crate) fn drop_pending(&mut self) {
        self.pending.clear();
        self.withdrawn.clear();
        self.pending_epoch = self.pending_epoch.wrapping_add(1);
    }

    /// A request still being filtered lands as begun again; the approvals waiting stay.
    pub(crate) fn restart_prepares(&mut self) {
        self.pending_epoch = self.pending_epoch.wrapping_add(1);
    }

    /// A material of the case changed or was deleted: a request prepared with it does not go
    /// (she may have taken a sentence out), waiting or still being filtered. Sending one that
    /// was waiting says why, in one line, and she prepares it again from the new text.
    pub(crate) fn withdraw_for_material(&mut self, case_id: &str) {
        let gone: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, p)| p.kind.case() == Some(case_id) || p.payload.case_id() == case_id)
            .map(|(id, _)| id.clone())
            .collect();
        self.forget_pending(case_id);
        for id in gone {
            self.withdrawn.insert(id, MATERIAL_CHANGED);
        }
    }

    /// Build the request for one section and everything the review screen needs.
    pub fn prepare_section(
        &mut self,
        case_id: &str,
        section_key: &str,
        instruction: &str,
    ) -> Result<Prepared, CoreError> {
        self.prepare_section_replacing(case_id, section_key, instruction, None)
    }

    /// A section request whose answer rewrites one proposed paragraph (`replaces`), or, with
    /// `None`, becomes the section's draft in place of the paragraphs not approved yet: one
    /// draft at a time, never a pile of versions.
    pub fn prepare_section_replacing(
        &mut self,
        case_id: &str,
        section_key: &str,
        instruction: &str,
        replaces: Option<&str>,
    ) -> Result<Prepared, CoreError> {
        let job = self.begin_prepare_section(case_id, section_key, instruction, replaces)?;
        self.prepare_here(job)
    }

    /// "לנסח מהנקודות שלי" (round 30): a section she writes herself, worded from the points
    /// she typed, with the section's guide and her style. No material, score or other section
    /// goes with it.
    pub fn prepare_from_points(
        &mut self,
        case_id: &str,
        section_key: &str,
        points: &str,
    ) -> Result<Prepared, CoreError> {
        let job = self.begin_prepare_from_points(case_id, section_key, points)?;
        self.prepare_here(job)
    }

    /// Prepare every section that has material and nothing written yet (the "write the empty
    /// sections" button), side by side. A section with a draft or approved paragraphs is left
    /// as it is: writing it again would put a second draft beside what she already approved.
    pub fn prepare_full_draft(
        &mut self,
        case_id: &str,
    ) -> Result<Vec<(String, Prepared)>, CoreError> {
        let mut jobs = self.begin_full_draft(case_id)?;
        let order: Vec<String> = jobs.iter().map(|(k, _)| k.clone()).collect();
        let mut out = Vec::new();
        while !jobs.is_empty() {
            let (ready, again) = self.finish_requests(run_requests(jobs)?)?;
            out.extend(ready);
            jobs = again;
        }
        out.sort_by_key(|(k, _)| order.iter().position(|o| o == k));
        Ok(out)
    }

    /// The three steps in a row, holding the core: for tests and callers that hold no lock.
    fn prepare_here(&mut self, mut job: RequestJob) -> Result<Prepared, CoreError> {
        loop {
            match self.finish_request(job.run())? {
                Landed::Ready(p) => return Ok(p),
                Landed::Again(next) => job = *next,
            }
        }
    }

    /// The section's request, read under the lock, for [`RequestJob::run`].
    pub fn begin_prepare_section(
        &mut self,
        case_id: &str,
        section_key: &str,
        instruction: &str,
        replaces: Option<&str>,
    ) -> Result<RequestJob, CoreError> {
        self.begin_section(
            SectionAsk {
                case_id: case_id.to_owned(),
                section_key: section_key.to_owned(),
                instruction: instruction.to_owned(),
                replaces: replaces.map(str::to_owned),
                points: false,
                check: false,
            },
            false,
            0,
            None,
        )
    }

    /// "לנסח מהנקודות שלי", read under the lock, for [`RequestJob::run`].
    pub fn begin_prepare_from_points(
        &mut self,
        case_id: &str,
        section_key: &str,
        points: &str,
    ) -> Result<RequestJob, CoreError> {
        if points.trim().is_empty() {
            return Err(CoreError::Refused(
                "כותבים קודם את הנקודות, ואז מנסחים מהן.".to_owned(),
            ));
        }
        self.begin_section(
            SectionAsk {
                case_id: case_id.to_owned(),
                section_key: section_key.to_owned(),
                instruction: points.to_owned(),
                replaces: None,
                points: true,
                check: false,
            },
            false,
            0,
            None,
        )
    }

    /// Every section "write the empty sections" writes, each read under the lock, in the
    /// report's order. Their mask books are siblings: run side by side, they give one tag to
    /// the same words and never one tag to two.
    pub fn begin_full_draft(
        &mut self,
        case_id: &str,
    ) -> Result<Vec<(String, RequestJob)>, CoreError> {
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let fed: HashSet<String> = {
            let v = self.vault_ref()?;
            let inputs = v.inputs(case_id)?;
            Core::material_routing(v, &structure, case_id, &inputs)?
                .into_iter()
                .flat_map(|r| r.feeds)
                .collect()
        };
        let mut out: Vec<(String, RequestJob)> = Vec::new();
        for s in structure
            .sections()
            .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()))
            .filter(|s| s.mode == dv_domain::SectionMode::Ai)
        {
            let written = self
                .vault_ref()?
                .drafts(case_id, &s.key)?
                .iter()
                .any(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved));
            if fed.contains(&s.key) && !written {
                let book = out.first().map(|(_, j)| j.data.masks.sibling());
                let job = self.begin_section(
                    SectionAsk {
                        case_id: case_id.to_owned(),
                        section_key: s.key.clone(),
                        instruction: FULL_DRAFT_INSTRUCTION.to_owned(),
                        replaces: None,
                        points: false,
                        check: false,
                    },
                    false,
                    0,
                    book,
                )?;
                out.push((s.key.clone(), job));
            }
        }
        Ok(out)
    }

    /// Land each run of a batch, in order: the requests ready, and those begun again.
    #[allow(clippy::type_complexity)]
    pub fn finish_requests(
        &mut self,
        runs: Vec<(String, RequestRun)>,
    ) -> Result<(Vec<(String, Prepared)>, Vec<(String, RequestJob)>), CoreError> {
        let mut ready = Vec::new();
        let mut again = Vec::new();
        for (key, ran) in runs {
            match self.finish_request(ran)? {
                Landed::Ready(p) => ready.push((key, p)),
                Landed::Again(job) => again.push((key, *job)),
            }
        }
        // Begun again one at a time, each with a book of its own: run side by side again,
        // the requests of one case share one book, read now that every run of the batch landed,
        // so they do not give one tag to two words and come back again.
        let mut books: HashMap<String, dv_privacy::MaskBook> = HashMap::new();
        for (_, job) in &mut again {
            let case_id = job.data.case_id.clone();
            if case_id.is_empty() {
                continue;
            }
            job.data.masks = match books.get(&case_id) {
                Some(book) => book.sibling(),
                None => {
                    let book = dv_privacy::MaskBook::new(self.vault_ref()?.masks(&case_id)?);
                    let mine = book.sibling();
                    books.insert(case_id, book);
                    mine
                }
            };
        }
        Ok((ready, again))
    }

    /// Land a request filtered away from the lock. Its approval is kept only here, and only
    /// when the vault was not locked meanwhile and what the filter decided by is what the vault
    /// holds now; else it is dropped, or begun again.
    pub fn finish_request(&mut self, ran: RequestRun) -> Result<Landed, CoreError> {
        let RequestRun {
            session,
            epoch,
            fingerprint,
            learned,
            tries,
            ask,
            data,
            built,
        } = ran;
        self.same_session(session)?;
        let built = built?;
        let case_id = ask.case_id().to_owned();
        // Consent removed meanwhile, or the case deleted or in the recycle bin: nothing lands.
        if !case_id.is_empty() {
            let v = self.vault_ref()?;
            if v.case_in_trash(&case_id)? {
                return Err(CoreError::NotFound("התיק".to_owned()));
            }
            if v.case_meta(&case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
        }
        let now = PrivacyData::read(self.vault_ref()?, &case_id)?;
        let stale = epoch != self.pending_epoch
            || now.fingerprint()? != fingerprint
            || masks_taken(&now.masks.entries(), &data.masks.added());
        if stale {
            return self.again(ask, learned, tries);
        }
        let Built {
            mut prepared,
            kind,
            cleared,
        } = built;
        match cleared {
            Cleared::Learn(found) => {
                self.vault_mut()?.add_found_names(&case_id, &found)?;
                self.again(ask, true, tries)
            }
            Cleared::Answered(answered) => {
                self.land(&data, answered, kind, &mut prepared)?;
                Ok(Landed::Ready(prepared))
            }
        }
    }

    /// The request begun again after a change. After [`MAX_TRIES`] it is filtered here,
    /// holding the core, where nothing can change in between.
    fn again(&mut self, ask: Ask, learned: bool, tries: u32) -> Result<Landed, CoreError> {
        let tries = tries.saturating_add(1);
        let job = match ask {
            Ask::Section(a) => self.begin_section(a, learned, tries, None)?,
            Ask::Consult(a) => self.begin_consult(a, learned, tries)?,
            Ask::Sort(c, only) => self.begin_sort(c, only, learned, tries, None)?,
            Ask::ReportCheck(c) => self.begin_report_check(c, learned, tries)?,
        };
        if tries >= MAX_TRIES {
            return self.finish_request(job.run());
        }
        Ok(Landed::Again(Box::new(job)))
    }

    /// Read everything a section's request is built from. `book`: the mask book of a request
    /// of the same case run side by side with this one.
    fn begin_section(
        &mut self,
        ask: SectionAsk,
        learned: bool,
        tries: u32,
        book: Option<dv_privacy::MaskBook>,
    ) -> Result<RequestJob, CoreError> {
        if ask.check {
            return self.begin_check(ask, learned, tries);
        }
        let (case_id, section_key, points) =
            (ask.case_id.as_str(), ask.section_key.as_str(), ask.points);
        // Only a paragraph of this section in this case can be rewritten (SEC9).
        if let Some(id) = &ask.replaces {
            let ours = self
                .vault_ref()?
                .drafts(case_id, section_key)?
                .iter()
                .any(|d| &d.id == id);
            if !ours {
                return Err(CoreError::NotFound("הפסקה".to_owned()));
            }
        }
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let letter = Letter::of(section_key);
        let section = match letter {
            Some(l) => dv_domain::ReportSection {
                key: section_key.to_owned(),
                title: l.title().to_owned(),
                inputs: Vec::new(),
                about: String::new(),
                mode: dv_domain::SectionMode::Ai,
                guide: String::new(),
            },
            None => structure
                .section(section_key)
                .ok_or_else(|| CoreError::NotFound(section_key.to_owned()))?
                .clone(),
        };
        // Her own sections (round 30): nothing is sent for them but her points, on request.
        let own = section.mode == dv_domain::SectionMode::Own;
        if own && !points {
            return Err(CoreError::Refused(
                "את כותבת את הסעיף הזה בעצמך, ולכן שום חומר לא נשלח בשבילו. אפשר לכתוב פסקה, או לנסח מהנקודות שלך."
                    .to_owned(),
            ));
        }
        if points && !own {
            return Err(CoreError::Refused(
                "ניסוח מהנקודות שלך הוא לסעיפים שאת כותבת בעצמך.".to_owned(),
            ));
        }
        // A paragraph worded again is a small job; a whole section is not (D-139).
        let model = if ask.replaces.is_some() {
            self.model_for(Task::Light)?
        } else {
            self.model_config()?
        };
        let mut data = self.privacy_data(case_id)?;
        if let Some(book) = book {
            data.masks = book;
        }
        let fingerprint = data.fingerprint()?;
        let hints = self.case_hints(case_id, &[])?;
        let demo_mode = self.demo_mode()?;

        let read = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            let meta = v.case_meta(case_id)?;
            if meta.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let derived = DERIVED_SECTIONS.contains(&section_key) || letter.is_some();
            let sources = if derived || points {
                Vec::new()
            } else {
                read_sources(v, &structure, case_id, section_key)?
            };
            let trim_title = structure
                .sections()
                .find(|s| s.key == section_key)
                .map_or_else(String::new, |s| s.title.clone());
            let mut approved = Vec::new();
            if derived && !points {
                // A letter gets only the sections it is written from (EX-4): the school letter
                // only the recommendations, never the background or the diagnoses.
                let wanted = |key: &str| match letter {
                    Some(l) => l.written_from().contains(&key),
                    None => key != section_key && !DERIVED_SECTIONS.contains(&key),
                };
                // The recommendations she left out of the letters (round 44); the rest go in.
                let marked = |key: &str, id: &str| match (&letter, &meta.letter_left_out) {
                    (Some(_), Some(ids)) if key == "recommendations" => {
                        !ids.iter().any(|i| i == id)
                    }
                    _ => true,
                };
                for s in structure.sections().filter(|s| wanted(&s.key)) {
                    let text: Vec<String> = v
                        .drafts(case_id, &s.key)?
                        .into_iter()
                        .filter(|d| d.status == DraftStatus::Approved && marked(&s.key, &d.id))
                        .map(|d| crate::drafts::paragraph_to_send(v, case_id, &d))
                        .collect::<Result<_, _>>()?;
                    if !text.is_empty() {
                        approved.push((s.clone(), text.join("\n")));
                    }
                }
            }
            // Nothing approved yet: there is nothing to write it from, so nothing is sent.
            if letter.is_some() && approved.is_empty() {
                return Err(CoreError::Refused(
                    "המכתב נכתב מתוך ההמלצות שאישרת בדוח, ועוד אין המלצות מאושרות. מאשרים קודם את סעיף ההמלצות.".to_owned(),
                ));
            }
            if derived && !points && approved.is_empty() {
                return Err(CoreError::Refused(
                    "הסעיף הזה נכתב מתוך הסעיפים שכבר אישרת, ועוד לא אישרת אף סעיף. מאשרים קודם את הטיוטות בסעיפים האחרים, ואז חוזרים לכאן."
                        .to_owned(),
                ));
            }
            // The current draft may contain manual edits, so it goes through review too.
            let current: Vec<String> = if points {
                Vec::new()
            } else {
                v.drafts(case_id, section_key)?
                    .into_iter()
                    .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
                    .map(|d| crate::drafts::paragraph_to_send(v, case_id, &d))
                    .collect::<Result<_, _>>()?
            };
            let history: Vec<(ChatRole, String)> = if points {
                Vec::new()
            } else {
                v.messages(case_id, section_key)?
                    .into_iter()
                    .map(|m| (m.role, m.text_tagged))
                    .collect()
            };
            // In a section written from the scores: the patterns of this case's scores
            // (D-100), worked out here, for her inferences about them.
            let from_scores = section.inputs.contains(&InputKind::TestScores) && !points;
            let sheets = if from_scores {
                score_sheets(v, case_id, &v.inputs(case_id)?)?
            } else {
                Vec::new()
            };
            let patterns = dv_domain::sheet_patterns(&sheets);
            let interpretations = if from_scores {
                style::interpretations_for_request(v, &sheets)?
            } else {
                None
            };
            let guide = if letter.is_some() {
                String::new()
            } else {
                guides::guide_for(v, &meta, &section)?.text()
            };
            let severity = (section.key == "dsm" && !points && letter.is_none())
                .then(|| dv_domain::severity_request(meta.dsm_severity.as_ref()));
            Texts::Section(Box::new(SectionRead {
                age: meta.age.map(dv_domain::Age::display),
                gender: meta.child_gender,
                trim_title,
                sources,
                approved,
                current,
                history,
                style: style::style_source(v)?,
                interpretations,
                patterns,
                guide,
                severity,
                section,
            }))
        };

        let nonce = match self
            .section_nonces
            .get(&(case_id.to_owned(), section_key.to_owned()))
        {
            Some(n) => n.clone(),
            None => {
                let n = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
                self.section_nonces
                    .insert((case_id.to_owned(), section_key.to_owned()), n.clone());
                n
            }
        };
        let hasher = self.vault_ref()?.token_hasher();
        Ok(RequestJob {
            session: self.session,
            epoch: self.pending_epoch,
            cancel: self.cancel.clone(),
            fingerprint,
            learned,
            tries,
            ask: Ask::Section(ask),
            data,
            hasher,
            hints,
            model,
            demo_mode,
            nonce,
            read,
            #[cfg(test)]
            panics: false,
        })
    }
}

impl Core {
    /// Build the request that checks the section's proposed paragraphs against its sources.
    pub fn prepare_verify(
        &mut self,
        case_id: &str,
        section_key: &str,
    ) -> Result<Prepared, CoreError> {
        let job = self.begin_prepare_verify(case_id, section_key)?;
        self.prepare_here(job)
    }

    /// The check against the sources, read under the lock, for [`RequestJob::run`].
    pub fn begin_prepare_verify(
        &mut self,
        case_id: &str,
        section_key: &str,
    ) -> Result<RequestJob, CoreError> {
        self.begin_check(
            SectionAsk {
                case_id: case_id.to_owned(),
                section_key: section_key.to_owned(),
                instruction: String::new(),
                replaces: None,
                points: false,
                check: true,
            },
            false,
            0,
        )
    }

    fn begin_check(
        &mut self,
        ask: SectionAsk,
        learned: bool,
        tries: u32,
    ) -> Result<RequestJob, CoreError> {
        let (case_id, section_key) = (ask.case_id.as_str(), ask.section_key.as_str());
        if !self.verify_stage_on()? {
            return Err(CoreError::Needs(
                "הבדיקה מול המקורות כבויה. אפשר להפעיל אותה בהגדרות.".to_owned(),
                ErrorAction::settings("ai"),
            ));
        }
        if DERIVED_SECTIONS.contains(&section_key) || Letter::of(section_key).is_some() {
            return Err(CoreError::Refused(
                "הבדיקה מול המקורות מיועדת לסעיפים שנכתבים מחומרי התיק.".to_owned(),
            ));
        }
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let section = structure
            .section(section_key)
            .ok_or_else(|| CoreError::NotFound(section_key.to_owned()))?
            .clone();
        // Her own sections (round 30, D-068): nothing of the case is sent for them, the check
        // against the sources included (the review of stage 3).
        if section.mode == dv_domain::SectionMode::Own {
            return Err(CoreError::Refused(
                "את כותבת את הסעיף הזה בעצמך, ולכן הוא לא נבדק מול החומרים ושום חומר לא נשלח בשבילו."
                    .to_owned(),
            ));
        }
        let model = self.model_for(Task::Light)?;
        let data = self.privacy_data(case_id)?;
        let fingerprint = data.fingerprint()?;
        let hints = self.case_hints(case_id, &[])?;
        let demo_mode = self.demo_mode()?;
        let read = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let sources = read_sources(v, &structure, case_id, section_key)?;
            if sources.is_empty() {
                return Err(CoreError::Refused(
                    "אין לסעיף הזה חומרים לבדוק מולם.".to_owned(),
                ));
            }
            let paragraphs: Vec<(String, String)> = v
                .drafts(case_id, section_key)?
                .into_iter()
                .filter(|d| d.status == DraftStatus::Proposed)
                .map(|d| (d.id, d.text_tagged))
                .collect();
            if paragraphs.is_empty() {
                return Err(CoreError::Refused(
                    "אין טיוטות מוצעות לבדיקה בסעיף הזה.".to_owned(),
                ));
            }
            Texts::Check(CheckRead {
                trim_title: section.title.clone(),
                section,
                sources,
                paragraphs,
            })
        };
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let hasher = self.vault_ref()?.token_hasher();
        Ok(RequestJob {
            session: self.session,
            epoch: self.pending_epoch,
            cancel: self.cancel.clone(),
            fingerprint,
            learned,
            tries,
            ask: Ask::Section(ask),
            data,
            hasher,
            hints,
            model,
            demo_mode,
            nonce,
            read,
            #[cfg(test)]
            panics: false,
        })
    }
}

impl Core {
    /// Free consultation, optionally about a case (then approved sections go along, filtered).
    /// `conversation_id`: a saved conversation to continue (its earlier turns go along, as
    /// they were sent: filtered), or `None` for a new one.
    pub fn prepare_consult(
        &mut self,
        case_id: Option<&str>,
        conversation_id: Option<&str>,
        message: &str,
    ) -> Result<Prepared, CoreError> {
        let job = self.begin_prepare_consult(case_id, conversation_id, message)?;
        self.prepare_here(job)
    }

    /// A consultation, read under the lock, for [`RequestJob::run`].
    pub fn begin_prepare_consult(
        &mut self,
        case_id: Option<&str>,
        conversation_id: Option<&str>,
        message: &str,
    ) -> Result<RequestJob, CoreError> {
        self.begin_consult(
            ConsultAsk {
                case_id: case_id.map(str::to_owned),
                conversation_id: conversation_id.map(str::to_owned),
                message: message.to_owned(),
            },
            false,
            0,
        )
    }

    fn begin_consult(
        &mut self,
        ask: ConsultAsk,
        learned: bool,
        tries: u32,
    ) -> Result<RequestJob, CoreError> {
        // Earlier turns, as she saw them: filtered again with today's names and decisions, and
        // shown on the review screen like everything else that leaves the computer.
        let earlier: Vec<(String, String, String)> = match &ask.conversation_id {
            Some(id) => {
                let (case_of, turns) = self.stored_consultation(id)?;
                if case_of != ask.case_id {
                    return Err(CoreError::Refused(
                        "השיחה הזו שייכת לתיק אחר. אפשר לפתוח שיחה חדשה.".to_owned(),
                    ));
                }
                turns
                    .into_iter()
                    .map(|mut t| {
                        (
                            std::mem::take(&mut t.role),
                            std::mem::take(&mut t.shown),
                            std::mem::take(&mut t.tagged),
                        )
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        let model = self.model_for(Task::Consult)?;
        let key = ask.case_id.clone().unwrap_or_default();
        let data = self.privacy_data(&key)?;
        let fingerprint = data.fingerprint()?;
        let hints = self.case_hints(&key, &[ask.message.as_str()])?;
        let demo_mode = self.demo_mode()?;
        let read = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            let mut approved = Vec::new();
            if let Some(c) = &ask.case_id {
                if v.case_meta(c)?.consent.is_none() {
                    return Err(CoreError::ConsentMissing);
                }
                let structure = structure_for(v, c)?;
                for s in structure.sections() {
                    for d in v
                        .drafts(c, &s.key)?
                        .into_iter()
                        .filter(|d| d.status == DraftStatus::Approved)
                    {
                        approved.push((s.clone(), crate::drafts::paragraph_to_send(v, c, &d)?));
                    }
                }
            }
            Texts::Consult(ConsultRead { earlier, approved })
        };
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let hasher = self.vault_ref()?.token_hasher();
        Ok(RequestJob {
            session: self.session,
            epoch: self.pending_epoch,
            cancel: self.cancel.clone(),
            fingerprint,
            learned,
            tries,
            ask: Ask::Consult(ask),
            data,
            hasher,
            hints,
            model,
            demo_mode,
            nonce,
            read,
            #[cfg(test)]
            panics: false,
        })
    }

    /// Sorting the case's materials into sections (D-022): everything not sorted yet, one
    /// review screen.
    pub fn prepare_sort(&mut self, case_id: &str) -> Result<Prepared, CoreError> {
        let job = self.begin_prepare_sort(case_id)?;
        self.prepare_here(job)
    }

    /// Sorting, read under the lock, for [`RequestJob::run`].
    pub fn begin_prepare_sort(&mut self, case_id: &str) -> Result<RequestJob, CoreError> {
        self.begin_sort(case_id.to_owned(), None, false, 0, None)
    }

    /// Sorting one material alone (a part of [`Core::begin_sort_parts`] prepared again).
    pub fn begin_prepare_sort_one(
        &mut self,
        case_id: &str,
        input_id: &str,
    ) -> Result<RequestJob, CoreError> {
        self.begin_sort(
            case_id.to_owned(),
            Some(input_id.to_owned()),
            false,
            0,
            None,
        )
    }

    /// Sorting one request per material, keyed by the material (D-134). One request for all
    /// of them answered slowly and, past about nine materials, ran past the answer's length
    /// and was lost whole; one each is short, and they go side by side. Their mask books are
    /// siblings, like the whole report's sections, so one date gets one tag in all of them.
    pub fn begin_sort_parts(
        &mut self,
        case_id: &str,
    ) -> Result<Vec<(String, RequestJob)>, CoreError> {
        let ids: Vec<String> = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let mut ids = Vec::new();
            for inp in crate::instruments::request_inputs(v, case_id)? {
                let count = dv_domain::passage_ranges(&inp.content).len();
                if count > 0 && sorting::routing_of(v, case_id, &inp.id)?.needs_sorting(count) {
                    ids.push(inp.id);
                }
            }
            ids
        };
        if ids.is_empty() {
            return Err(CoreError::Refused(
                "כל החומרים כבר ממוינים לסעיפים.".to_owned(),
            ));
        }
        let mut out: Vec<(String, RequestJob)> = Vec::new();
        for id in ids {
            let book = out.first().map(|(_, j)| j.data.masks.sibling());
            let job = self.begin_sort(case_id.to_owned(), Some(id.clone()), false, 0, book)?;
            out.push((id, job));
        }
        Ok(out)
    }

    /// [`Core::begin_sort_parts`], each part through the gate, holding the core (tests).
    pub fn prepare_sort_parts(
        &mut self,
        case_id: &str,
    ) -> Result<Vec<(String, Prepared)>, CoreError> {
        let mut jobs = self.begin_sort_parts(case_id)?;
        let order: Vec<String> = jobs.iter().map(|(k, _)| k.clone()).collect();
        let mut out = Vec::new();
        while !jobs.is_empty() {
            let (ready, again) = self.finish_requests(run_requests(jobs)?)?;
            out.extend(ready);
            jobs = again;
        }
        out.sort_by_key(|(k, _)| order.iter().position(|o| o == k));
        Ok(out)
    }

    fn begin_sort(
        &mut self,
        case_id: String,
        only: Option<String>,
        learned: bool,
        tries: u32,
        book: Option<dv_privacy::MaskBook>,
    ) -> Result<RequestJob, CoreError> {
        let structure = structure_for(self.vault_ref()?, &case_id)?;
        let sections: Vec<dv_ai::SortSection> = sorting::sortable(&structure)
            .into_iter()
            .map(|s| dv_ai::SortSection {
                key: s.key.clone(),
                about: s.about.clone(),
            })
            .collect();
        let model = self.model_for(Task::Sort)?;
        let mut data = self.privacy_data(&case_id)?;
        if let Some(book) = book {
            data.masks = book;
        }
        let fingerprint = data.fingerprint()?;
        let hints = self.case_hints(&case_id, &[])?;
        let demo_mode = self.demo_mode()?;
        let read = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(&case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let mut items = Vec::new();
            // A score table of an instrument she does not give is not sorted (D-098).
            for inp in crate::instruments::request_inputs(v, &case_id)? {
                if only.as_ref().is_some_and(|id| *id != inp.id) {
                    continue;
                }
                let ranges = dv_domain::passage_ranges(&inp.content);
                if ranges.is_empty()
                    || !sorting::routing_of(v, &case_id, &inp.id)?.needs_sorting(ranges.len())
                {
                    continue;
                }
                items.push(SortItem {
                    id: inp.id,
                    kind: inp.kind,
                    title: inp.title,
                    content: inp.content,
                    ranges,
                });
            }
            if items.is_empty() {
                return Err(CoreError::Refused(
                    "כל החומרים כבר ממוינים לסעיפים.".to_owned(),
                ));
            }
            Texts::Sort(SortRead { sections, items })
        };
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let hasher = self.vault_ref()?.token_hasher();
        Ok(RequestJob {
            session: self.session,
            epoch: self.pending_epoch,
            cancel: self.cancel.clone(),
            fingerprint,
            learned,
            tries,
            ask: Ask::Sort(case_id, only),
            data,
            hasher,
            hints,
            model,
            demo_mode,
            nonce,
            read,
            #[cfg(test)]
            panics: false,
        })
    }

    /// The one request that reads the approved report as a whole (D-097).
    pub fn prepare_report_check(&mut self, case_id: &str) -> Result<Prepared, CoreError> {
        let job = self.begin_prepare_report_check(case_id)?;
        self.prepare_here(job)
    }

    /// The check of the whole report, read under the lock, for [`RequestJob::run`].
    pub fn begin_prepare_report_check(&mut self, case_id: &str) -> Result<RequestJob, CoreError> {
        self.begin_report_check(case_id.to_owned(), false, 0)
    }

    fn begin_report_check(
        &mut self,
        case_id: String,
        learned: bool,
        tries: u32,
    ) -> Result<RequestJob, CoreError> {
        if !self.report_check_on()? {
            return Err(CoreError::Needs(
                "בדיקת הדוח כולו כבויה. אפשר להפעיל אותה בהגדרות.".to_owned(),
                ErrorAction::settings("ai"),
            ));
        }
        let structure = structure_for(self.vault_ref()?, &case_id)?;
        let model = self.model_for(Task::Draft)?;
        let data = self.privacy_data(&case_id)?;
        let fingerprint = data.fingerprint()?;
        // The approved drafts are not materials: the name model reads them as well.
        let mut approved = self.report_check_texts(&case_id)?;
        let extra: Vec<&str> = approved.iter().map(String::as_str).collect();
        let hints = self.case_hints(&case_id, &extra);
        for t in &mut approved {
            t.zeroize();
        }
        let hints = hints?;
        let demo_mode = self.demo_mode()?;
        let read = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if v.case_meta(&case_id)?.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let mut sections = Vec::new();
            let mut waiting = Vec::new();
            // The report's sections only: never a letter, never the signature, never a
            // material, a conversation or a draft not approved.
            for s in structure.sections().filter(|s| s.key != "signature") {
                let drafts = v.drafts(&case_id, &s.key)?;
                // A new draft waiting for her: the report is not approved yet. A proposed
                // rewrite of an approved paragraph leaves the approved one standing.
                if drafts
                    .iter()
                    .any(|d| d.status == DraftStatus::Proposed && d.replaces.is_none())
                {
                    waiting.push(s.title.clone());
                }
                let text: Vec<String> = drafts
                    .into_iter()
                    .filter(|d| d.status == DraftStatus::Approved)
                    .map(|d| crate::drafts::paragraph_to_send(v, &case_id, &d))
                    .collect::<Result<_, _>>()?;
                if !text.is_empty() {
                    sections.push((s.clone(), text));
                }
            }
            let read = ReportCheckRead { sections };
            if !waiting.is_empty() {
                return Err(CoreError::Refused(format!(
                    "הדוח נבדק כולו אחרי שכל הטיוטות אושרו. עוד מחכות לאישור: {}.",
                    waiting.join(", ")
                )));
            }
            if read.sections.len() < 2 {
                return Err(CoreError::Refused(
                    "בדיקת הדוח כולו משווה בין סעיפים, וצריך לפחות שני סעיפים מאושרים.".to_owned(),
                ));
            }
            Texts::ReportCheck(read)
        };
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let hasher = self.vault_ref()?.token_hasher();
        Ok(RequestJob {
            session: self.session,
            epoch: self.pending_epoch,
            cancel: self.cancel.clone(),
            fingerprint,
            learned,
            tries,
            ask: Ask::ReportCheck(case_id),
            data,
            hasher,
            hints,
            model,
            demo_mode,
            nonce,
            read,
            #[cfg(test)]
            panics: false,
        })
    }
}

/// Said when she sends a request whose material changed after it was prepared.
pub(crate) const MATERIAL_CHANGED: &str =
    "החומר השתנה אחרי שהבקשה הוכנה, ולכן היא לא נשלחה. צריך להכין אותה מחדש.";

/// The request each section of "write the empty sections" is sent with.
const FULL_DRAFT_INSTRUCTION: &str = "כתוב/י טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.";
