//! The local name model (D-053): off by default. When she turns it on in Settings, the pinned
//! DictaBERT-NER files are downloaded once to this computer (`dv_egress::name_model`, nothing
//! from the vault goes out) and every document she adds is also read by the model, on this
//! computer. Its person spans are one more voter: they can add a hide, never remove one.
//! A name it finds is kept with the case like any name found automatically, so the later
//! filters and the gate know it without running the model again.
//!
//! The model never blocks work: when it cannot be read, documents are filtered on the rules
//! alone, as before it was turned on, and Settings says what went wrong.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use dv_egress::name_model::{self, HuggingFaceFetch, ModelError, ModelFetch};
use dv_ner::worker::NerWorker;
use dv_ner::NerModel;
use dv_privacy::{FilterOutcome, NameHint, PrivacyContext, PrivacyError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Core, CoreError, ErrorAction};

/// Vault setting: "on" when she turned the model on.
pub(crate) const NAME_MODEL_KEY: &str = "name_model";

/// What Settings shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NameModelView {
    /// She turned it on (and the files are here).
    pub enabled: bool,
    /// The pinned files are on this computer.
    pub downloaded: bool,
    /// The download, in MB.
    pub size_mb: u32,
    /// Why it is not in use right now, in Hebrew.
    pub problem: Option<String>,
}

/// The model, in its isolated process (the app) or in this one (tests, development).
#[derive(Debug)]
pub(crate) enum NameFinder {
    InProcess(Arc<NerModel>),
    Isolated(NerWorker),
    /// Tests: finds these exact words, and counts the texts it read.
    #[cfg(test)]
    Fixed(Vec<String>, Arc<std::sync::atomic::AtomicUsize>),
}

impl NameFinder {
    fn people(&mut self, text: &str) -> Result<Vec<NameHint>, ()> {
        match self {
            Self::InProcess(m) => m
                .people(text)
                .map(|p| {
                    p.into_iter()
                        .map(|p| NameHint {
                            start: p.start,
                            end: p.end,
                            score: p.score,
                        })
                        .collect()
                })
                .map_err(|_| ()),
            Self::Isolated(w) => w
                .people(text)
                .map(|p| {
                    p.into_iter()
                        .map(|p| NameHint {
                            start: p.start,
                            end: p.end,
                            score: p.score,
                        })
                        .collect()
                })
                .map_err(|_| ()),
            #[cfg(test)]
            Self::Fixed(words, read) => {
                read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(words
                    .iter()
                    .flat_map(|w| {
                        text.match_indices(w.as_str()).map(|(start, m)| NameHint {
                            start,
                            end: start + m.len(),
                            score: 0.99,
                        })
                    })
                    .collect())
            }
        }
    }
}

/// The model as the core holds it (F2): shared with a job that reads texts while the core is
/// free, and loaded in the background after unlock. Whoever needs it waits for the loading
/// to end. Lock order: the core, then this; a job takes only this.
pub(crate) struct NerCell {
    state: Mutex<NerState>,
    loaded: Condvar,
}

enum NerState {
    Loading,
    Ready(NameFinder),
    /// Why it is not in use (shown in Settings).
    Failed(String),
}

impl std::fmt::Debug for NerCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NerCell")
    }
}

impl NerCell {
    fn with_state(state: NerState) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
            loaded: Condvar::new(),
        })
    }

    /// Check the files and start the model on a thread of its own. When nobody holds the
    /// cell any more by the time it is up (the vault locked), it is ended at once.
    fn load_in_background(dir: PathBuf, exe: Option<PathBuf>) -> Arc<Self> {
        let cell = Self::with_state(NerState::Loading);
        let mine = Arc::clone(&cell);
        let started = std::thread::Builder::new()
            .name("name-model-load".to_owned())
            .spawn(move || {
                let loaded = load(&dir, exe.as_deref());
                if Arc::strong_count(&mine) > 1 {
                    mine.set(loaded.map_or_else(NerState::Failed, NerState::Ready));
                }
            });
        if started.is_err() {
            cell.set(NerState::Failed("לא הצלחתי להפעיל את המודל.".to_owned()));
        }
        cell
    }

    fn set(&self, state: NerState) {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        *s = state;
        self.loaded.notify_all();
    }

    /// Run `f` on the model, after waiting for it to load. `Err` says why it is not in use.
    fn with<T>(&self, f: impl FnOnce(&mut NameFinder) -> T) -> Result<T, String> {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        while matches!(*s, NerState::Loading) {
            s = self.loaded.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
        match &mut *s {
            NerState::Ready(m) => Ok(f(m)),
            NerState::Failed(why) => Err(why.clone()),
            NerState::Loading => Err(String::new()),
        }
    }

    /// The model read a text and failed: it is not used again until the next unlock, and its
    /// process ends now.
    fn fail(&self) {
        self.set(NerState::Failed("המודל לא הצליח לקרוא מסמך.".to_owned()));
    }

    /// End the model's process now (the vault locked, or it was turned off), even while a
    /// job still holds the cell. Waits for a loading in progress only when `wait`.
    fn end(&self, wait: bool) {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        while wait && matches!(*s, NerState::Loading) {
            s = self.loaded.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
        if !matches!(*s, NerState::Loading) {
            *s = NerState::Failed(String::new());
        }
    }

    /// Why it is not in use; `None` while it loads or works (never waits).
    fn problem(&self) -> Option<String> {
        match self.state.try_lock().as_deref() {
            Ok(NerState::Failed(why)) if !why.is_empty() => Some(why.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn ready(finder: NameFinder) -> Arc<Self> {
        Self::with_state(NerState::Ready(finder))
    }

    #[cfg(test)]
    fn is_isolated(&self) -> bool {
        matches!(
            *self.state.lock().unwrap_or_else(PoisonError::into_inner),
            NerState::Ready(NameFinder::Isolated(_))
        )
    }
}

/// Texts of a case the model has not read yet, read by a job while the core is free (F2).
pub struct HintJob {
    session: u64,
    cancel: crate::jobs::Cancel,
    case_id: String,
    cell: Arc<NerCell>,
    texts: Vec<String>,
}

/// What the model found in a [`HintJob`]'s texts, waiting for [`Core::finish_name_hints`].
pub struct HintsRead {
    session: u64,
    case_id: String,
    found: Vec<(String, Vec<NameHint>)>,
}

impl std::fmt::Debug for HintJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HintJob")
    }
}

impl std::fmt::Debug for HintsRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HintsRead")
    }
}

impl HintJob {
    /// Read each text with the model. Stops at the first failure: what is not found here is
    /// read (or filtered on the rules alone) by the request itself, as before.
    #[must_use]
    pub fn run(self) -> HintsRead {
        let mut found: Vec<(String, Vec<NameHint>)> = Vec::new();
        let mut texts = self.texts.into_iter();
        for mut text in texts.by_ref() {
            // Locked meanwhile: the model ended with the lock, and the texts go now.
            if self.cancel.is_set() {
                zeroize::Zeroize::zeroize(&mut text);
                for (t, _) in &mut found {
                    zeroize::Zeroize::zeroize(t);
                }
                found.clear();
                break;
            }
            match self.cell.with(|m| m.people(&text)) {
                Ok(Ok(hints)) => found.push((text, hints)),
                Ok(Err(())) => {
                    self.cell.fail();
                    zeroize::Zeroize::zeroize(&mut text);
                    break;
                }
                Err(_) => {
                    zeroize::Zeroize::zeroize(&mut text);
                    break;
                }
            }
        }
        // Texts not read (the model failed or the vault locked) are wiped too.
        for mut text in texts {
            zeroize::Zeroize::zeroize(&mut text);
        }
        HintsRead {
            session: self.session,
            case_id: self.case_id,
            found,
        }
    }
}

/// A span as kept in the vault.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct KeptHint {
    s: usize,
    e: usize,
    p: f32,
}

impl From<&NameHint> for KeptHint {
    fn from(h: &NameHint) -> Self {
        Self {
            s: h.start,
            e: h.end,
            p: h.score,
        }
    }
}

impl From<KeptHint> for NameHint {
    fn from(h: KeptHint) -> Self {
        Self {
            start: h.s,
            end: h.e,
            score: h.p,
        }
    }
}

/// Kept spans that fit the text (anything else is read again by the model).
fn valid(kept: &[KeptHint], text: &str) -> bool {
    kept.iter().all(|h| {
        h.s < h.e
            && h.e <= text.len()
            && text.is_char_boundary(h.s)
            && text.is_char_boundary(h.e)
            && h.p.is_finite()
    })
}

/// The model's hints for the texts of one request, by exact text. Empty when the model is off,
/// and then every call is the plain filter.
#[derive(Debug, Default, Clone)]
pub(crate) struct NameHints(HashMap<String, Vec<NameHint>>);

impl NameHints {
    fn of(&self, text: &str) -> &[NameHint] {
        self.0.get(text).map_or(&[], Vec::as_slice)
    }

    /// `dv_privacy::filter`, with this text's hints.
    pub(crate) fn filter(
        &self,
        text: &str,
        ctx: &PrivacyContext<'_>,
    ) -> Result<FilterOutcome, PrivacyError> {
        dv_privacy::filter_with_hints(text, ctx, self.of(text))
    }

    /// `dv_privacy::filter_split`, with this text's hints.
    pub(crate) fn filter_split(
        &self,
        text: &str,
        ctx: &PrivacyContext<'_>,
        ranges: &[std::ops::Range<usize>],
    ) -> Result<(FilterOutcome, Option<Vec<FilterOutcome>>), PrivacyError> {
        dv_privacy::filter_split_with_hints(text, ctx, ranges, self.of(text))
    }
}

/// The folder of the pinned revision under the models folder.
#[must_use]
pub fn folder(models_dir: &Path) -> PathBuf {
    models_dir.join(name_model::MODEL_FOLDER)
}

/// Download (or finish downloading) the pinned files. Runs without the core's lock; `progress`
/// gets bytes done and the total.
pub fn download(models_dir: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), CoreError> {
    let fetch = HuggingFaceFetch::new().map_err(model_error)?;
    download_with(&fetch, models_dir, progress)
}

pub fn download_with(
    fetch: &dyn ModelFetch,
    models_dir: &Path,
    progress: &dyn Fn(u64, u64),
) -> Result<(), CoreError> {
    let total = name_model::total_size();
    name_model::download(fetch, &folder(models_dir), &|n| progress(n, total)).map_err(model_error)
}

fn model_error(e: ModelError) -> CoreError {
    let he = match &e {
        ModelError::Offline => "אין חיבור לאתר של המודל. אפשר לנסות שוב מאוחר יותר.".to_owned(),
        ModelError::Server(code) => format!("האתר של המודל לא זמין כרגע ({code})."),
        ModelError::Tampered(_) => {
            "הקובץ שהגיע לא זהה לקובץ המאושר, ולכן נמחק. אפשר לנסות שוב.".to_owned()
        }
        ModelError::HostRefused(_) => "ההורדה הופנתה לכתובת לא מאושרת, ולכן נעצרה.".to_owned(),
        ModelError::Disk(_) => "אין מקום או גישה לשמירת המודל במחשב.".to_owned(),
    };
    CoreError::Refused(he)
}

/// Check the pinned files, then start the model: in its isolated worker when `exe` is given
/// (the app, SEC2), in this process otherwise (tests, development).
fn load(dir: &Path, exe: Option<&Path>) -> Result<NameFinder, String> {
    name_model::verify(dir)
        .map_err(|_| "קובצי המודל במחשב השתנו. כדאי לכבות ולהדליק אותו שוב.".to_owned())?;
    match exe {
        Some(exe) => NerWorker::start(exe, dir)
            .map(NameFinder::Isolated)
            .map_err(|_| "לא הצלחתי להפעיל את המודל בתהליך מבודד.".to_owned()),
        None => NerModel::load(dir)
            .map(|m| NameFinder::InProcess(Arc::new(m)))
            .map_err(|_| "לא הצלחתי לקרוא את המודל.".to_owned()),
    }
}

impl Core {
    /// Where downloaded models live (the app passes a folder next to the vault, not inside it,
    /// so a backup never carries them). Without one the model stays off.
    #[must_use]
    pub fn with_models_dir(mut self, dir: PathBuf) -> Self {
        self.models_dir = Some(dir);
        self
    }

    /// Run the model in its own isolated process: `exe` is the app's binary, which turns into
    /// the worker when started with [`dv_ner::worker::WORKER_ARG`] (SEC2).
    #[must_use]
    pub fn with_name_model_worker(mut self, exe: PathBuf) -> Self {
        self.ner_exe = Some(exe);
        self
    }

    pub(crate) fn name_model_on(&mut self) -> bool {
        self.vault_ref()
            .ok()
            .and_then(|v| v.setting(NAME_MODEL_KEY).ok().flatten())
            .is_some_and(|s| s == "on")
    }

    pub fn name_model_status(&mut self) -> Result<NameModelView, CoreError> {
        self.vault_ref()?;
        let downloaded = self
            .models_dir
            .as_deref()
            .is_some_and(|d| name_model::present(&folder(d)));
        let size_mb =
            u32::try_from(name_model::total_size().div_ceil(1024 * 1024)).unwrap_or(u32::MAX);
        Ok(NameModelView {
            enabled: self.name_model_on() && downloaded,
            downloaded,
            size_mb,
            problem: self.ner.as_ref().and_then(|c| c.problem()),
        })
    }

    /// Turn the model on (its files must be downloaded) or off (its files are deleted).
    pub fn set_name_model(&mut self, on: bool) -> Result<NameModelView, CoreError> {
        let dir = self.models_dir.clone();
        if on {
            let ready = dir
                .as_deref()
                .is_some_and(|d| name_model::present(&folder(d)));
            if !ready {
                return Err(CoreError::Needs(
                    "קודם צריך להוריד את המודל.".to_owned(),
                    ErrorAction::settings("privacy"),
                ));
            }
            self.vault_mut()?.set_setting(NAME_MODEL_KEY, "on")?;
            // A model that failed before gets another try.
            if self.ner.as_ref().is_some_and(|c| c.problem().is_some()) {
                self.ner = None;
            }
            self.preload_name_model();
            return self.name_model_status();
        }
        self.vault_mut()?.set_setting(NAME_MODEL_KEY, "")?;
        self.vault_mut()?.clear_name_hints()?;
        self.hint_cache.clear();
        // A worker still loading holds the files open: let it finish, then end it.
        if let Some(cell) = self.ner.take() {
            cell.end(true);
        }
        if let Some(d) = dir.as_deref() {
            name_model::remove(&folder(d)).map_err(model_error)?;
        }
        self.name_model_status()
    }

    /// The model, started once per unlock after its files pass their pinned hashes: in its
    /// own isolated process when the app gave one ([`Core::with_name_model_worker`], SEC2),
    /// in this process otherwise (tests, development). Started here and waited for when no
    /// one started it yet. `None` when it is off or there is no models folder.
    fn name_model(&mut self) -> Option<Arc<NerCell>> {
        if !self.name_model_on() {
            return None;
        }
        if self.ner.is_none() {
            let dir = folder(self.models_dir.as_deref()?);
            let exe = self.ner_exe.clone();
            self.ner = Some(match exe {
                Some(exe) => NerCell::load_in_background(dir, Some(exe)),
                None => NerCell::with_state(
                    load(&dir, None).map_or_else(NerState::Failed, NerState::Ready),
                ),
            });
        }
        self.ner.clone()
    }

    /// Right after unlock, check the files and start the isolated worker in the background,
    /// so the first request finds the model loaded instead of waiting for it. Only for the
    /// app's worker process; a lock ends it.
    pub(crate) fn preload_name_model(&mut self) {
        if self.ner.is_some() {
            return;
        }
        let (Some(exe), Some(dir)) = (self.ner_exe.clone(), self.models_dir.as_deref()) else {
            return;
        };
        self.ner = Some(NerCell::load_in_background(folder(dir), Some(exe)));
    }

    /// End the model's process with the vault (on lock).
    pub(crate) fn end_name_model(&mut self) {
        if let Some(cell) = self.ner.take() {
            cell.end(false);
        }
    }

    /// The model's person spans in `text`, as hints for the filter. Empty when it is off.
    /// Kept in memory until the vault locks, and in the vault for a case's texts (R4), so a
    /// material is read by the model once, not once per unlock.
    pub(crate) fn name_hints(&mut self, case_id: &str, text: &str) -> Vec<NameHint> {
        if text.trim().is_empty() || !self.name_model_on() {
            return Vec::new();
        }
        if let Some(h) = self.kept_hints(text) {
            return h;
        }
        let Some(cell) = self.name_model() else {
            return Vec::new();
        };
        match cell.with(|m| m.people(text)) {
            Ok(Ok(hints)) => {
                self.keep_hints(case_id, text, &hints);
                hints
            }
            Ok(Err(())) => {
                // A worker that failed is not used again until the next unlock.
                cell.fail();
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Kept spans for `text`, from memory or the vault, when they fit it.
    fn kept_hints(&mut self, text: &str) -> Option<Vec<NameHint>> {
        if let Some(h) = self.hint_cache.get(text) {
            return Some(h.clone());
        }
        let v = self.vault_ref().ok()?;
        let key = v.name_hints_key(name_model::MODEL_REVISION, text);
        let kept = v
            .name_hints(&key)
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str::<Vec<KeptHint>>(&j).ok())
            .filter(|k| valid(k, text))?;
        let hints: Vec<NameHint> = kept.into_iter().map(NameHint::from).collect();
        self.hint_cache.insert(text.to_owned(), hints.clone());
        Some(hints)
    }

    /// Keep what the model found in `text`: in memory, and in the vault for a case's text.
    fn keep_hints(&mut self, case_id: &str, text: &str, hints: &[NameHint]) {
        self.hint_cache.insert(text.to_owned(), hints.to_vec());
        if case_id.is_empty() {
            return;
        }
        let kept: Vec<KeptHint> = hints.iter().map(KeptHint::from).collect();
        if let (Ok(json), Ok(v)) = (serde_json::to_string(&kept), self.vault_mut()) {
            let key = v.name_hints_key(name_model::MODEL_REVISION, text);
            // Only a saving: when it fails, the model reads the text again later.
            let _ = v.set_name_hints(&key, case_id, &json);
        }
    }

    /// The texts of the case (its materials' titles and texts) and `extra` that the model has
    /// not read yet, as a job to run while the core is free; `None` when there is nothing to
    /// read or the model is off. Only a head start: a request reads anything still missing
    /// itself, so filtering never depends on the job.
    pub fn begin_name_hints(
        &mut self,
        case_id: &str,
        extra: &[String],
    ) -> Result<Option<HintJob>, CoreError> {
        if !self.name_model_on() {
            return Ok(None);
        }
        let mut texts: Vec<String> = extra.to_vec();
        if !case_id.is_empty() {
            for inp in self.vault_ref()?.inputs(case_id)? {
                texts.push(inp.title);
                texts.push(inp.content);
            }
        }
        let mut missing: Vec<String> = Vec::new();
        for t in texts {
            if !t.trim().is_empty() && !missing.contains(&t) && self.kept_hints(&t).is_none() {
                missing.push(t);
            }
        }
        if missing.is_empty() {
            return Ok(None);
        }
        // Started in the background when it is not yet: the job waits for it, not the core.
        if self.ner.is_none() {
            let Some(dir) = self.models_dir.as_deref().map(folder) else {
                return Ok(None);
            };
            self.ner = Some(NerCell::load_in_background(dir, self.ner_exe.clone()));
        }
        let Some(cell) = self.ner.clone() else {
            return Ok(None);
        };
        Ok(Some(HintJob {
            session: self.session,
            cancel: self.cancel.clone(),
            case_id: case_id.to_owned(),
            cell,
            texts: missing,
        }))
    }

    /// Keep what the job found. Dropped when the vault locked meanwhile.
    pub fn finish_name_hints(&mut self, read: HintsRead) {
        let HintsRead {
            session,
            case_id,
            found,
        } = read;
        let current = self.session == session && self.vault.is_some();
        for (mut text, hints) in found {
            if current {
                self.keep_hints(&case_id, &text, &hints);
            }
            // Landed after a lock, or kept: the copy carried back from the job is wiped.
            zeroize::Zeroize::zeroize(&mut text);
        }
    }

    /// At unlock: forget the spans of texts that are no longer materials of any case.
    pub(crate) fn prune_name_hints(&mut self) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        let mut keep = HashSet::new();
        for case in v.list_cases()? {
            for inp in v.inputs(&case.id)? {
                for t in [&inp.title, &inp.content] {
                    keep.insert(v.name_hints_key(name_model::MODEL_REVISION, t));
                }
            }
        }
        v.keep_name_hints(&keep)?;
        Ok(())
    }

    /// Hints for every material of the case (title and text) and for `extra` texts: what a
    /// request about the case filters. Empty when the model is off.
    pub(crate) fn case_hints(
        &mut self,
        case_id: &str,
        extra: &[&str],
    ) -> Result<NameHints, CoreError> {
        if !self.name_model_on() {
            return Ok(NameHints::default());
        }
        let mut texts: Vec<String> = extra.iter().map(|t| (*t).to_owned()).collect();
        if !case_id.is_empty() {
            for inp in self.vault_ref()?.inputs(case_id)? {
                texts.push(inp.title);
                texts.push(inp.content);
            }
        }
        let mut out = HashMap::new();
        for t in texts {
            let h = self.name_hints(case_id, &t);
            if !h.is_empty() {
                out.insert(t, h);
            }
        }
        Ok(NameHints(out))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Offline;
    impl ModelFetch for Offline {
        fn get_to(
            &self,
            _: &str,
            _: &mut dyn Write,
            _: u64,
            _: &dyn Fn(u64),
        ) -> Result<u64, ModelError> {
            Err(ModelError::Offline)
        }
    }

    fn core() -> (tempfile::TempDir, Core) {
        let dir = tempfile::tempdir().unwrap();
        let mut core = Core::for_tests(&dir.path().join("vault"), None)
            .with_models_dir(dir.path().join("models"));
        core.create_vault("correct horse battery staple 1").unwrap();
        (dir, core)
    }

    #[test]
    fn off_by_default_and_cannot_be_turned_on_before_the_download() {
        let (_d, mut c) = core();
        let s = c.name_model_status().unwrap();
        assert!(!s.enabled && !s.downloaded);
        assert_eq!(s.size_mb, 705);
        assert!(matches!(
            c.set_name_model(true),
            Err(CoreError::Needs(_, ErrorAction::Settings { ref pane })) if pane == "privacy"
        ));
        assert!(c.name_hints("", "רוני הגיע עם אמא").is_empty());
    }

    #[test]
    fn a_failed_download_leaves_it_off() {
        let (d, mut c) = core();
        let r = download_with(&Offline, &d.path().join("models"), &|_, _| {});
        assert!(matches!(r, Err(CoreError::Refused(_))));
        assert!(!c.name_model_status().unwrap().downloaded);
    }

    #[test]
    fn files_that_changed_on_disk_are_never_used() {
        let (d, mut c) = core();
        let f = folder(&d.path().join("models"));
        std::fs::create_dir_all(&f).unwrap();
        // The right sizes, the wrong bytes.
        for file in name_model::MODEL_FILES {
            std::fs::File::create(f.join(file.name))
                .unwrap()
                .set_len(file.size)
                .unwrap();
        }
        assert!(c.name_model_status().unwrap().downloaded);
        assert!(c.set_name_model(true).unwrap().enabled);
        assert!(c.name_hints("", "רוני הגיע עם אמא").is_empty());
        assert!(c.name_model_status().unwrap().problem.is_some());
    }

    #[test]
    fn turning_it_off_deletes_the_files() {
        let (d, mut c) = core();
        let f = folder(&d.path().join("models"));
        std::fs::create_dir_all(&f).unwrap();
        std::fs::write(f.join("config.json"), b"{}").unwrap();
        c.set_name_model(false).unwrap();
        assert!(!f.exists());
    }

    /// With the real files (`DV_NER_MODEL=<dir>`): a name no rule knows is hidden and kept
    /// with the case, so later texts hide it without the model.
    #[cfg(unix)]
    #[test]
    fn with_the_real_model_a_name_no_rule_knows_is_kept() {
        let Some(model) = std::env::var_os("DV_NER_MODEL") else {
            return;
        };
        let (d, mut c) = core();
        let f = folder(&d.path().join("models"));
        std::fs::create_dir_all(&f).unwrap();
        for file in name_model::MODEL_FILES {
            std::os::unix::fs::symlink(Path::new(&model).join(file.name), f.join(file.name))
                .unwrap();
        }
        let meta = dv_domain::CaseMeta {
            code: "TEST-0003".into(),
            consent: Some(dv_domain::Consent {
                given_on: "2026-09-01".into(),
                form_version: "v1".into(),
                given_by: "שני ההורים".into(),
            }),
            ..dv_domain::CaseMeta::default()
        };
        let child = dv_domain::IdentityInput {
            id: None,
            role: dv_domain::Role::Child,
            value: "אלון".into(),
            aliases: vec![],
        };
        let case = c.create_case(meta, vec![child]).unwrap();
        let text = "נוגה הזמינה את אלון הביתה אחרי הגן. הוא מספר הרבה על הודיה ועל הכלב שלה.";
        let rules = c.preview_filter(&case, text).unwrap().tagged;
        assert!(rules.contains("נוגה") && rules.contains("הודיה"), "{rules}");
        c.set_name_model(true).unwrap();
        c.add_input(
            &case,
            dv_domain::InputKind::Kindergarten,
            "שיחה עם הגננת",
            text,
        )
        .unwrap();
        // What would go out for the section, after a lock: the spans kept in the vault (R4)
        // stand in for the model, which is not even started.
        c.lock();
        c.unlock("correct horse battery staple 1").unwrap();
        let worker = std::env::var_os("DV_NER_WORKER").map(PathBuf::from);
        if let Some(exe) = &worker {
            c.ner_exe = Some(exe.clone());
        }
        let prepared = c
            .prepare_section(&case, "kindergarten", "תנסח פסקה על החברים בגן")
            .unwrap();
        assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
        let out: String = prepared
            .parts
            .iter()
            .flat_map(|p| p.outgoing.iter().map(|s| s.text.clone()))
            .collect();
        assert!(out.contains("הכלב שלה"), "{out}");
        for name in ["נוגה", "הודיה"] {
            assert!(!out.contains(name), "{out}");
        }
        assert!(c.name_model_status().unwrap().problem.is_none());
        assert!(c.ner.is_none(), "kept spans, no model run after the unlock");
        // A new text in the same case runs the model, in the worker when one is given.
        let more = "גם נוגה הגיעה למסיבה.";
        assert!(!c.name_hints(&case, more).is_empty());
        if worker.is_some() {
            assert!(c.ner.as_ref().is_some_and(|n| n.is_isolated()));
        }
    }

    /// R4: spans are kept per text and case; a deleted case takes them along, and turning the
    /// model off forgets them all.
    #[test]
    fn kept_spans_follow_the_case_and_go_when_the_model_is_off() {
        let (d, mut c) = core();
        let f = folder(&d.path().join("models"));
        std::fs::create_dir_all(&f).unwrap();
        for file in name_model::MODEL_FILES {
            std::fs::write(f.join(file.name), b"").unwrap();
        }
        c.vault_mut()
            .unwrap()
            .set_setting(NAME_MODEL_KEY, "on")
            .unwrap();
        let text = "נוגה הגיעה";
        let key = c
            .vault_ref()
            .unwrap()
            .name_hints_key(name_model::MODEL_REVISION, text);
        c.vault_mut()
            .unwrap()
            .set_name_hints(&key, "case-1", r#"[{"s":0,"e":8,"p":0.95}]"#)
            .unwrap();
        // Kept spans are used without the model (its files here are empty, it cannot load).
        let h = c.name_hints("case-1", text);
        assert_eq!((h.len(), h[0].start, h[0].end), (1, 0, 8));
        assert!(c.ner.is_none(), "the model was never started");
        // Spans that do not fit the text are never used.
        let other = "אבג";
        let k2 = c
            .vault_ref()
            .unwrap()
            .name_hints_key(name_model::MODEL_REVISION, other);
        c.vault_mut()
            .unwrap()
            .set_name_hints(&k2, "case-1", r#"[{"s":1,"e":2,"p":0.9}]"#)
            .unwrap();
        c.hint_cache.clear();
        assert!(
            c.name_hints("case-1", other).is_empty(),
            "not on a character boundary"
        );
        c.set_name_model(false).unwrap();
        assert_eq!(c.vault_ref().unwrap().name_hints(&key).unwrap(), None);
    }

    /// F2: the model reads a case's texts in a job while the core is free; the request then
    /// finds them read and does not run the model under the core's lock.
    #[test]
    fn a_job_reads_the_case_with_the_model_while_the_core_is_free() {
        let (d, mut c) = core();
        let f = folder(&d.path().join("models"));
        std::fs::create_dir_all(&f).unwrap();
        c.vault_mut()
            .unwrap()
            .set_setting(NAME_MODEL_KEY, "on")
            .unwrap();
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        c.ner = Some(NerCell::ready(NameFinder::Fixed(
            vec!["נוגה".into()],
            Arc::clone(&reads),
        )));
        let meta = dv_domain::CaseMeta {
            code: "TEST-0004".into(),
            ..dv_domain::CaseMeta::default()
        };
        let case = c.create_case(meta, vec![]).unwrap();
        let text = "נוגה הזמינה את הילד הביתה אחרי הגן.";
        c.vault_mut()
            .unwrap()
            .add_input(&case, dv_domain::InputKind::Kindergarten, "שיחה", text)
            .unwrap();
        let read_before = reads.load(std::sync::atomic::Ordering::SeqCst);

        let core = Arc::new(Mutex::new(c));
        let job = core
            .lock()
            .unwrap()
            .begin_name_hints(&case, &[])
            .unwrap()
            .expect("a text to read");
        // The core is free while the job runs.
        let worker = std::thread::spawn(move || job.run());
        assert!(core.lock().unwrap().case_detail(&case).is_ok());
        let read = worker.join().unwrap();
        let mut c = core.lock().unwrap();
        c.finish_name_hints(read);
        let after_job = reads.load(std::sync::atomic::Ordering::SeqCst);
        assert!(after_job > read_before, "the job ran the model");

        // Now the request finds the texts read: the model does not run under the lock.
        let hints = c.case_hints(&case, &[]).unwrap();
        assert!(
            !hints.of(text).is_empty(),
            "the name the model found is used"
        );
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), after_job);
        assert!(
            c.begin_name_hints(&case, &[]).unwrap().is_none(),
            "nothing left"
        );

        // A job begun before a lock lands nowhere after it.
        let job = c
            .begin_name_hints(&case, &["עוד טקסט על נוגה".to_owned()])
            .unwrap()
            .unwrap();
        c.lock();
        let read = job.run();
        c.unlock("correct horse battery staple 1").unwrap();
        c.finish_name_hints(read);
        assert!(c.hint_cache.is_empty());
    }
}
