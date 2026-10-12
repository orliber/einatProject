//! The local name model (D-053): off by default. When she turns it on in Settings, the pinned
//! DictaBERT-NER files are downloaded once to this computer (`dv_egress::name_model`, nothing
//! from the vault goes out) and every document she adds is also read by the model, on this
//! computer. Its person spans are one more voter: they can add a hide, never remove one.
//! A name it finds is kept with the case like any name found automatically, so the later
//! filters and the gate know it without running the model again.
//!
//! The model never blocks work: when it cannot be read, documents are filtered on the rules
//! alone, as before it was turned on, and Settings says what went wrong.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dv_egress::name_model::{self, HuggingFaceFetch, ModelError, ModelFetch};
use dv_ner::NerModel;
use dv_privacy::{FilterOutcome, NameHint, PrivacyContext, PrivacyError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Core, CoreError};

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

impl Core {
    /// Where downloaded models live (the app passes a folder next to the vault, not inside it,
    /// so a backup never carries them). Without one the model stays off.
    #[must_use]
    pub fn with_models_dir(mut self, dir: PathBuf) -> Self {
        self.models_dir = Some(dir);
        self
    }

    fn name_model_on(&mut self) -> bool {
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
            problem: self.ner_problem.clone(),
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
                return Err(CoreError::Refused("קודם צריך להוריד את המודל.".to_owned()));
            }
            self.vault_mut()?.set_setting(NAME_MODEL_KEY, "on")?;
        } else {
            self.vault_mut()?.set_setting(NAME_MODEL_KEY, "")?;
            self.ner = None;
            if let Some(d) = dir.as_deref() {
                name_model::remove(&folder(d)).map_err(model_error)?;
            }
        }
        self.ner_problem = None;
        self.name_model_status()
    }

    /// The model, loaded once per run after its files pass their pinned hashes. `None` when it
    /// is off or cannot be read (then `ner_problem` says why).
    fn name_model(&mut self) -> Option<Arc<NerModel>> {
        if !self.name_model_on() {
            return None;
        }
        if let Some(m) = &self.ner {
            return Some(Arc::clone(m));
        }
        if self.ner_problem.is_some() {
            return None;
        }
        let dir = folder(self.models_dir.as_deref()?);
        let loaded = name_model::verify(&dir)
            .map_err(|_| "קובצי המודל במחשב השתנו. כדאי לכבות ולהדליק אותו שוב.".to_owned())
            .and_then(|()| {
                NerModel::load(&dir).map_err(|_| "לא הצלחתי לקרוא את המודל.".to_owned())
            });
        match loaded {
            Ok(m) => {
                let m = Arc::new(m);
                self.ner = Some(Arc::clone(&m));
                Some(m)
            }
            Err(why) => {
                self.ner_problem = Some(why);
                None
            }
        }
    }

    /// The model's person spans in `text`, as hints for the filter. Empty when it is off.
    /// Kept in memory until the vault locks, so a material is read by the model once.
    pub(crate) fn name_hints(&mut self, text: &str) -> Vec<NameHint> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        let Some(model) = self.name_model() else {
            return Vec::new();
        };
        if let Some(h) = self.hint_cache.get(text) {
            return h.clone();
        }
        match model.people(text) {
            Ok(people) => {
                let hints: Vec<NameHint> = people
                    .into_iter()
                    .map(|p| NameHint {
                        start: p.start,
                        end: p.end,
                        score: p.score,
                    })
                    .collect();
                self.hint_cache.insert(text.to_owned(), hints.clone());
                hints
            }
            Err(_) => {
                self.ner_problem = Some("המודל לא הצליח לקרוא מסמך.".to_owned());
                Vec::new()
            }
        }
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
            let h = self.name_hints(&t);
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
        assert!(matches!(c.set_name_model(true), Err(CoreError::Refused(_))));
        assert!(c.name_hints("רוני הגיע עם אמא").is_empty());
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
        assert!(c.name_hints("רוני הגיע עם אמא").is_empty());
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
        // What would go out for the section, after a lock: the model reads the material again.
        c.lock();
        c.unlock("correct horse battery staple 1").unwrap();
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
    }
}
