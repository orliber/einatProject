//! Application services used by the desktop shell.
//!
//! The Tauri commands are thin wrappers around [`Core`], so all behavior is testable on any
//! machine without a WebView. `Core` owns the unlocked vault and walks every request through
//! the same path: filter → build → review → gate → (approval) → send → check → store.

mod activity;
mod backup;
mod consultations;
mod dates;
mod followup;
mod library;
mod retention;
mod sorting;
pub mod update;
mod views;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dv_ai::{ModelConfig, SectionInput, TaggedInput, TaggedTurn, ALLOWED_MODELS};
use dv_domain::{
    passage_ranges, Author, CaseMeta, CaseSummary, ChatRole, DraftStatus, IdentityInput, InputKind,
    ReportStructure, Role,
};
use dv_egress::{AnthropicTransport, EgressError, Transport};
use dv_ipc::{PingResponse, IPC_VERSION};
use dv_privacy::restore::{restore, scan_model_output};
use dv_privacy::text::normalize;
use dv_privacy::{
    clear, filter, Checks, ClearedPayload, FilterOutcome, GateRequest, PrivacyContext,
};
use dv_vault::{Argon2Params, AuditEvent, Vault, VaultError};
use serde_json::Value;

pub use activity::ACTIVITY_PAGE;
pub use backup::{backup_file_name, BACKUP_DAYS, MAX_BACKUP_BYTES};
pub(crate) use dates::today;
pub use dv_vault::BACKUP_EXTENSION;
pub use followup::FollowUpView;
pub use library::TRASH_DAYS;
pub use retention::{KEEP_UNTIL_AGE, KEEP_YEARS_AFTER_LAST_CHANGE};
pub use views::{
    ActivityEntry, ActivityPage, AppStatus, BackupCheckView, BackupDone, BackupStatus, CaseDetail,
    ChatView, ConsultResult, ConsultTurnView, ConsultationSummary, ConsultationView, CreatedVault,
    ExportCheck, ImportPreview, MaterialRouting, NameMatch, NameSuggestion, ParagraphView,
    Prepared, ReportSettings, RetentionItem, ReviewPart, SectionResult, SectionView, SortResult,
    StagedBackup, SuspectDecision, UiError,
};

const API_KEY: &str = "anthropic_api_key";
const MODEL_KEY: &str = "model";
const LOCK_KEY: &str = "lock_minutes";
const FIRST_USE_KEY: &str = "first_use_day";
const REVIEW_KEY: &str = "review_only_suspect";
/// How long Claude may think (see `Core::model_for`).
const SPEED_KEY: &str = "answer_speed";
const DEFAULT_SPEED: &str = "balanced";

/// What a request to Claude is for (sets how long it may think).
#[derive(Clone, Copy)]
enum Task {
    Sort,
    Draft,
    Consult,
}
/// Days of use before "show the review only when suspicious" can be chosen (D-020).
const REVIEW_ALWAYS_DAYS: u64 = 14;

fn day_number() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}
const REPORT_KEY: &str = "report_settings";
/// A gap this long between two 15-second ticks means the computer slept.
const SLEEP_GAP: Duration = Duration::from_secs(60);
const MAX_REQUEST_BYTES: usize = 900_000;
/// Sections written from other, already approved sections.
const DERIVED_SECTIONS: &[&str] = &["dsm", "summary", "diagnoses", "recommendations"];

/// Answer the UI's liveness check.
#[must_use]
pub fn ping() -> PingResponse {
    PingResponse {
        ipc_version: IPC_VERSION,
        core_version: env!("CARGO_PKG_VERSION").to_owned(),
        build_commit: option_env!("DV_BUILD_COMMIT").unwrap_or("dev").to_owned(),
        fips_active: dv_vault::crypto::fips_active(),
        platform: std::env::consts::OS.to_owned(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("locked")]
    Locked,
    #[error("vault: {0}")]
    Vault(#[from] VaultError),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("consent missing")]
    ConsentMissing,
    #[error("refused: {0}")]
    Refused(String),
    #[error("wait {0}s")]
    Backoff(u64),
    #[error("egress: {0}")]
    Egress(#[from] EgressError),
    #[error("ai: {0}")]
    Ai(#[from] dv_ai::AiError),
    #[error("update: {0}")]
    Update(dv_egress::update::UpdateError),
    #[error("internal: {0}")]
    Internal(String),
}

impl CoreError {
    /// What the psychologist sees.
    #[must_use]
    pub fn to_ui(&self) -> UiError {
        let (code, message) = match self {
            CoreError::Locked => ("locked", "הכספת נעולה. יש לפתוח אותה מחדש.".to_owned()),
            CoreError::Vault(VaultError::WrongSecret) => (
                "wrong_secret",
                "הסיסמה או ערכת השחזור לא נכונות.".to_owned(),
            ),
            CoreError::Vault(VaultError::RecoveryKeyInvalid) => (
                "recovery_invalid",
                "ערכת השחזור לא תקינה. כדאי לבדוק שגיאות הקלדה.".to_owned(),
            ),
            CoreError::Vault(VaultError::Policy(_)) => (
                "weak_password",
                "הסיסמה קצרה או נפוצה מדי. מומלץ משפט של כמה מילים (12 תווים לפחות).".to_owned(),
            ),
            CoreError::Vault(VaultError::AlreadyExists) => {
                ("exists", "כבר קיימת כספת במחשב הזה.".to_owned())
            }
            CoreError::Vault(e) => ("vault", format!("שגיאה בכספת: {e}")),
            CoreError::NotFound(what) => ("not_found", format!("לא נמצא: {what}")),
            CoreError::ConsentMissing => (
                "consent_missing",
                "לפני שליחה ל-Claude צריך לרשום בתיק את הסכמת ההורים.".to_owned(),
            ),
            CoreError::Refused(why) => ("refused", why.clone()),
            CoreError::Backoff(s) => (
                "backoff",
                format!("יותר מדי ניסיונות. אפשר לנסות שוב בעוד {s} שניות."),
            ),
            CoreError::Egress(e) => ("egress", egress_he(e)),
            CoreError::Ai(e) => ("ai", format!("התשובה של Claude לא תקינה: {e}")),
            CoreError::Update(e) => ("update", update::update_he(e)),
            CoreError::Internal(e) => ("internal", format!("שגיאה פנימית: {e}")),
        };
        UiError {
            code: code.to_owned(),
            message,
            details: Vec::new(),
        }
    }
}

fn egress_he(e: &EgressError) -> String {
    match e {
        EgressError::NoApiKey => {
            "לא הוגדר מפתח API. עד שיוגדר, התוכנה עובדת במצב הדגמה.".to_owned()
        }
        EgressError::Unauthorized => "מפתח ה-API נדחה. כדאי לבדוק אותו בהגדרות.".to_owned(),
        EgressError::RateLimited => "יותר מדי בקשות. אפשר לנסות שוב בעוד דקה.".to_owned(),
        EgressError::Offline => {
            "אין חיבור לאינטרנט. אפשר להמשיך לעבוד, ו-Claude יחזור כשיהיה חיבור.".to_owned()
        }
        EgressError::Tls => {
            "החיבור המאובטח נכשל. ייתכן שתוכנה במחשב (למשל אנטי-וירוס) מיירטת תעבורה מוצפנת."
                .to_owned()
        }
        other => format!("שגיאה בחיבור ל-Claude: {other}"),
    }
}

/// Today's date `(y, m, d)` in UTC, for relative dates.
/// A consent is a record the psychologist may have to show: a real, past date and who signed.
fn check_meta(meta: &CaseMeta) -> Result<(), CoreError> {
    if meta
        .retention_until
        .as_deref()
        .is_some_and(|d| dates::parse_iso(d).is_none())
    {
        return Err(CoreError::Refused(
            "תאריך סוף תקופת השמירה לא תקין.".to_owned(),
        ));
    }
    let Some(consent) = &meta.consent else {
        return Ok(());
    };
    let refused = |why: &str| Err(CoreError::Refused(why.to_owned()));
    let Some((y, m, d)) = dates::parse_iso(&consent.given_on) else {
        return refused("תאריך ההסכמה לא תקין.");
    };
    // Local time may already be tomorrow in UTC terms (Israel is ahead of UTC).
    let (ty, tm, td) = today();
    if (y, m, d) > (ty, tm, td + 1) {
        return refused("תאריך ההסכמה עוד לא הגיע.");
    }
    let by = consent.given_by.trim();
    if by.is_empty() || by.chars().count() > 60 {
        return refused("צריך לרשום מי חתם על ההסכמה (תפקיד, עד 60 תווים).");
    }
    Ok(())
}

enum PendingKind {
    Section {
        case_id: String,
        section_key: String,
        instruction_tagged: String,
        hidden: Vec<String>,
        sources: Vec<(String, String, String)>,
        /// The one proposed paragraph the answer rewrites ("ניסוח מחדש"); `None`: the answer
        /// is the section's new draft and replaces the paragraphs not approved yet.
        replaces: Option<String>,
    },
    Consult {
        case_id: Option<String>,
        /// The saved conversation it continues (`None`: a new one).
        conversation_id: Option<String>,
        message_tagged: String,
        /// The question as typed (kept only in the vault, for display).
        message_shown: String,
        hidden: Vec<String>,
    },
    /// Sorting materials into sections (D-022): per material its id, how many passages were
    /// sent, and a fingerprint of its text then.
    Sort {
        case_id: String,
        materials: Vec<(String, usize, u64)>,
        sections: Vec<String>,
    },
}

struct Pending {
    payload: ClearedPayload,
    kind: PendingKind,
}

/// An approved request on its way out. Holds no vault; only the payload and how to send it.
pub struct Outgoing {
    pending: Pending,
    api_key: Option<zeroize::Zeroizing<String>>,
    transport: Option<Arc<dyn Transport>>,
}

impl std::fmt::Debug for Outgoing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outgoing")
            .field("sha256", &self.pending.payload.sha256())
            .finish_non_exhaustive()
    }
}

impl Outgoing {
    /// Send the approved payload: the test transport, Claude (API key set), or local demo.
    /// Returns the answer and whether it came from demo mode.
    pub fn transmit(&self) -> Result<(Value, bool), CoreError> {
        if let Some(t) = &self.transport {
            return Ok((t.send(&self.pending.payload)?, false));
        }
        match &self.api_key {
            Some(key) => Ok((
                AnthropicTransport::new(key)?.send(&self.pending.payload)?,
                false,
            )),
            None => {
                let body: Value = serde_json::from_slice(self.pending.payload.body())
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                Ok((dv_ai::demo::respond(&body), true))
            }
        }
    }
}

/// Session state. One per running app; the shell keeps it behind a mutex.
pub struct Core {
    dir: PathBuf,
    argon: Option<Argon2Params>,
    vault: Option<Vault>,
    pending: HashMap<String, Pending>,
    last_activity: Instant,
    /// Wall-clock time of the shell's last timer tick (sleep detection).
    last_tick: Option<SystemTime>,
    failed_unlocks: u32,
    not_before: Option<Instant>,
    disk_encryption: String,
    transport: Option<Arc<dyn Transport>>,
    /// The app's own binary, started as an isolated worker for each document.
    ingest_exe: Option<PathBuf>,
    /// A backup file chosen for the drill or a restore (encrypted bytes).
    staged_backup: Option<Vec<u8>>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("dir", &self.dir)
            .field("unlocked", &self.vault.is_some())
            .finish_non_exhaustive()
    }
}

/// Owned inputs for a privacy context (so the vault can be borrowed mutably afterwards).
struct PrivacyData {
    case_id: String,
    identities: Vec<dv_domain::Identity>,
    practitioner: Vec<String>,
    allow: HashSet<String>,
    is_name: HashSet<String>,
}

/// Everything the review screen shows, accumulated over the outgoing texts.
#[derive(Default)]
struct Review {
    parts: Vec<ReviewPart>,
    suspects: Vec<dv_privacy::Suspect>,
    hidden: Vec<String>,
    checks: Checks,
}

impl Review {
    fn add(&mut self, label: String, out: &FilterOutcome) {
        self.parts.push(ReviewPart {
            label,
            original: out.original_segments.clone(),
            outgoing: out.tagged_segments.clone(),
        });
        for s in &out.suspects {
            if !self.suspects.iter().any(|x| x.token == s.token) {
                self.suspects.push(s.clone());
            }
        }
        for h in &out.hidden {
            if !self.hidden.contains(h) {
                self.hidden.push(h.clone());
            }
        }
        self.checks.declared_names += out.checks.declared_names;
        self.checks.patterns += out.checks.patterns;
        self.checks.name_suspects += out.checks.name_suspects;
        self.checks.indirect_suspects += out.checks.indirect_suspects;
    }

    /// Show a text that goes out unchanged (Claude's earlier answer): on the review screen,
    /// but without raising name questions about it.
    fn add_context(&mut self, label: String, out: &FilterOutcome) {
        self.parts.push(ReviewPart {
            label,
            original: out.tagged_segments.clone(),
            outgoing: out.tagged_segments.clone(),
        });
    }

    fn into_prepared(self, demo_mode: bool) -> Prepared {
        Prepared {
            approval_id: None,
            parts: self.parts,
            suspects: self.suspects,
            hidden: self.hidden,
            checks: self.checks,
            blocked: Vec::new(),
            demo_mode,
        }
    }
}

/// A section's title as Claude gets it. The title is template text, not case text, yet
/// "שאלון הסתגלות" holds ש + "אלון" when that is the child's name: then the section's
/// description goes instead (or nothing), so there is nothing to ask, block or leak.
fn model_title(
    sec: &dv_domain::ReportSection,
    ctx: &PrivacyContext<'_>,
) -> Result<String, CoreError> {
    let clean = |t: &str| -> Result<bool, CoreError> {
        let o = filter(t, ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(o.suspects.is_empty() && o.tagged == t)
    };
    Ok(if clean(&sec.title)? {
        sec.title.clone()
    } else if clean(&sec.about)? {
        sec.about.clone()
    } else {
        String::new()
    })
}

impl Core {
    /// `dir` is the vault folder (the shell passes the OS local-app-data folder).
    #[must_use]
    pub fn new(dir: &Path) -> Self {
        let disk = match dv_vault::env::disk_encryption() {
            dv_vault::env::DiskEncryption::On => "on",
            dv_vault::env::DiskEncryption::Off => "off",
            dv_vault::env::DiskEncryption::Unknown => "unknown",
        };
        Self {
            dir: dir.to_path_buf(),
            argon: None,
            vault: None,
            pending: HashMap::new(),
            last_activity: Instant::now(),
            last_tick: None,
            failed_unlocks: 0,
            not_before: None,
            disk_encryption: disk.to_owned(),
            transport: None,
            ingest_exe: None,
            staged_backup: None,
        }
    }

    /// Read documents in a separate worker process (the app passes its own binary).
    #[must_use]
    pub fn with_ingest_worker(mut self, exe: PathBuf) -> Self {
        self.ingest_exe = Some(exe);
        self
    }

    /// Tests: cheap KDF and a fake transport.
    #[must_use]
    pub fn for_tests(dir: &Path, transport: Option<Box<dyn Transport>>) -> Self {
        let mut core = Self::new(dir);
        core.argon = Some(Argon2Params::TEST);
        core.transport = transport.map(Arc::from);
        core
    }

    fn lock_minutes(&self) -> u32 {
        self.vault
            .as_ref()
            .and_then(|v| v.setting(LOCK_KEY).ok().flatten())
            .and_then(|s| s.parse().ok())
            .unwrap_or(10)
    }

    /// Auto-lock after inactivity, then hand out the vault.
    fn vault_mut(&mut self) -> Result<&mut Vault, CoreError> {
        let limit = Duration::from_secs(u64::from(self.lock_minutes()) * 60);
        if self.vault.is_some() && self.last_activity.elapsed() > limit {
            self.lock();
            return Err(CoreError::Locked);
        }
        self.last_activity = Instant::now();
        self.vault.as_mut().ok_or(CoreError::Locked)
    }

    /// Called by the shell every 15 seconds. Locks after the idle time, and after the computer
    /// slept: while it sleeps the timer does not run, so the wall clock jumps between two
    /// ticks. (A monotonic clock does not always count sleep, so idle time alone misses it.)
    /// Returns true when it locked.
    pub fn tick(&mut self, now: SystemTime) -> bool {
        let slept = self
            .last_tick
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|gap| gap > SLEEP_GAP);
        self.last_tick = Some(now);
        if slept && self.vault.is_some() {
            self.lock_because(Some("sleep"));
            return true;
        }
        self.lock_if_idle()
    }

    /// Lock after the configured idle time even when nothing is clicked. Returns true when it
    /// locked.
    pub fn lock_if_idle(&mut self) -> bool {
        let limit = Duration::from_secs(u64::from(self.lock_minutes()) * 60);
        if self.vault.is_some() && self.last_activity.elapsed() > limit {
            self.lock();
            return true;
        }
        false
    }

    fn vault_ref(&mut self) -> Result<&Vault, CoreError> {
        self.vault_mut().map(|v| &*v)
    }

    // ------------------------------------------------------------ lifecycle

    pub fn status(&mut self) -> AppStatus {
        let cloud = dv_vault::env::cloud_synced_component(&self.dir);
        let (practitioner, review_only_suspect, review_choice_available) = match &self.vault {
            Some(v) => {
                let first = v
                    .setting(FIRST_USE_KEY)
                    .ok()
                    .flatten()
                    .and_then(|d| d.parse::<u64>().ok())
                    .unwrap_or_else(day_number);
                let available = day_number().saturating_sub(first) >= REVIEW_ALWAYS_DAYS;
                (
                    v.practitioner().map(|p| p.names).unwrap_or_default(),
                    available && v.setting(REVIEW_KEY).ok().flatten().as_deref() == Some("1"),
                    available,
                )
            }
            None => (Vec::new(), false, false),
        };
        let speed = self
            .vault
            .as_ref()
            .and_then(|v| v.setting(SPEED_KEY).ok().flatten())
            .unwrap_or_else(|| DEFAULT_SPEED.to_owned());
        let (demo, model, integrity) = match &self.vault {
            Some(v) => (
                v.secret(API_KEY).ok().flatten().is_none(),
                v.setting(MODEL_KEY)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| dv_ai::DEFAULT_MODEL.to_owned()),
                (!v.integrity().audit_ok || !v.integrity().header_ok)
                    .then(|| v.integrity().detail.clone().unwrap_or_default()),
            ),
            None => (true, dv_ai::DEFAULT_MODEL.to_owned(), None),
        };
        AppStatus {
            vault_exists: Vault::exists(&self.dir),
            unlocked: self.vault.is_some(),
            disk_encryption: self.disk_encryption.clone(),
            cloud_synced_folder: cloud,
            fips_active: dv_vault::crypto::fips_active(),
            demo_mode: demo,
            model,
            speed,
            integrity_warning: integrity,
            lock_minutes: self.lock_minutes(),
            practitioner,
            review_only_suspect,
            review_choice_available,
        }
    }

    /// After the first weeks, the review screen may be shown only when something is suspicious.
    pub fn set_review_only_suspect(&mut self, on: bool) -> Result<(), CoreError> {
        if on && !self.status().review_choice_available {
            return Err(CoreError::Refused(format!(
                "בשבועיים הראשונים ({REVIEW_ALWAYS_DAYS} ימים) מסך הבדיקה מוצג תמיד."
            )));
        }
        self.vault_mut()?
            .set_setting(REVIEW_KEY, if on { "1" } else { "0" })?;
        Ok(())
    }

    fn refuse_cloud(&self) -> Result<(), CoreError> {
        match dv_vault::env::cloud_synced_component(&self.dir) {
            Some(folder) => Err(CoreError::Refused(format!(
                "תיקיית הכספת נמצאת בתוך תיקייה שמסונכרנת לענן ({folder}). יש לבחור תיקייה מקומית."
            ))),
            None => Ok(()),
        }
    }

    pub fn create_vault(&mut self, password: &str) -> Result<CreatedVault, CoreError> {
        self.refuse_cloud()?;
        let params = self.argon.unwrap_or_else(Argon2Params::calibrate);
        let created = Vault::create(&self.dir, password, params)?;
        let recovery_key = created.recovery_key.to_string();
        let mut vault = created.vault;
        vault.set_setting(FIRST_USE_KEY, &day_number().to_string())?;
        self.vault = Some(vault);
        self.last_activity = Instant::now();
        Ok(CreatedVault { recovery_key })
    }

    fn check_backoff(&self) -> Result<(), CoreError> {
        match self.not_before {
            Some(t) if Instant::now() < t => Err(CoreError::Backoff(
                t.saturating_duration_since(Instant::now()).as_secs() + 1,
            )),
            _ => Ok(()),
        }
    }

    /// `purge`: erase what waited in the recycle bin past its time. Not right after a restore,
    /// which is often done to get back something deleted by mistake.
    fn after_unlock(
        &mut self,
        result: Result<Vault, VaultError>,
        purge: bool,
    ) -> Result<AppStatus, CoreError> {
        match result {
            Ok(mut vault) => {
                if self.failed_unlocks > 0 {
                    let _ = vault.record(
                        AuditEvent::UnlockFailed,
                        None,
                        &serde_json::json!({ "count": self.failed_unlocks }),
                    );
                }
                self.failed_unlocks = 0;
                self.not_before = None;
                self.vault = Some(vault);
                self.last_activity = Instant::now();
                if purge {
                    // Best effort: a case that fails to erase now is tried again next time, and
                    // never keeps the vault from opening.
                    if let Err(e) = self.purge_expired() {
                        if let Some(v) = self.vault.as_mut() {
                            let _ = v.record(
                                AuditEvent::IntegrityWarning,
                                None,
                                &serde_json::json!({ "reason": format!("recycle bin: {e}") }),
                            );
                        }
                    }
                }
                Ok(self.status())
            }
            Err(e) => {
                if matches!(e, VaultError::WrongSecret | VaultError::RecoveryKeyInvalid) {
                    self.failed_unlocks += 1;
                    let wait = 2u64.saturating_pow(self.failed_unlocks.min(6)).min(60);
                    self.not_before = Some(Instant::now() + Duration::from_secs(wait));
                }
                Err(e.into())
            }
        }
    }

    pub fn unlock(&mut self, password: &str) -> Result<AppStatus, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        let result = Vault::unlock_with_password(&self.dir, password);
        self.after_unlock(result, true)
    }

    pub fn unlock_with_recovery(&mut self, recovery_key: &str) -> Result<AppStatus, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        let result = Vault::unlock_with_recovery(&self.dir, recovery_key);
        self.after_unlock(result, true)
    }

    pub fn confirm_recovery_key(&mut self, typed: &str) -> Result<bool, CoreError> {
        Ok(self.vault_ref()?.check_recovery_key(typed))
    }

    /// Wipe keys, pending approvals and consultation history from memory.
    pub fn lock(&mut self) {
        self.lock_because(None);
    }

    fn lock_because(&mut self, reason: Option<&str>) {
        self.pending.clear();
        self.staged_backup = None;
        if let Some(v) = self.vault.take() {
            let _ = v.lock_because(reason);
        }
    }

    // ------------------------------------------------------------ password and kit

    /// `current` is the password, or the recovery kit when the password was forgotten.
    fn proof<'a>(current: &'a str, with_recovery: bool) -> dv_vault::Secret<'a> {
        if with_recovery {
            dv_vault::Secret::Recovery(current)
        } else {
            dv_vault::Secret::Password(current)
        }
    }

    /// A new password. Old backups keep opening with the old one, so a new backup is asked for.
    pub fn change_password(
        &mut self,
        current: &str,
        with_recovery: bool,
        new_password: &str,
    ) -> Result<(), CoreError> {
        self.check_backoff()?;
        let params = self.argon.unwrap_or_else(Argon2Params::calibrate);
        let result = self.vault_mut()?.rekey_password(
            Self::proof(current, with_recovery),
            new_password,
            params,
        );
        if let Err(e) = result {
            self.count_failure(&e);
            return Err(e.into());
        }
        self.failed_unlocks = 0;
        self.mark_secret_changed()
    }

    /// A new printed kit; the old one stops opening the vault (not old backups).
    pub fn new_recovery_kit(
        &mut self,
        current: &str,
        with_recovery: bool,
    ) -> Result<CreatedVault, CoreError> {
        self.check_backoff()?;
        let result = self
            .vault_mut()?
            .rotate_recovery_key(Self::proof(current, with_recovery));
        let key = match result {
            Ok(key) => key,
            Err(e) => {
                self.count_failure(&e);
                return Err(e.into());
            }
        };
        self.failed_unlocks = 0;
        self.mark_secret_changed()?;
        Ok(CreatedVault {
            recovery_key: key.to_string(),
        })
    }

    // ------------------------------------------------------------ settings

    pub fn set_api_key(&mut self, key: &str) -> Result<(), CoreError> {
        let key = key.trim();
        if key.is_empty() {
            self.vault_mut()?.delete_secret(API_KEY)?;
        } else {
            self.vault_mut()?.set_secret(API_KEY, key)?;
        }
        Ok(())
    }

    pub fn set_model(&mut self, model: &str) -> Result<(), CoreError> {
        if !ALLOWED_MODELS.contains(&model) {
            return Err(CoreError::Refused(
                "הדגם הזה לא זמין במסגרת ZDR.".to_owned(),
            ));
        }
        self.vault_mut()?.set_setting(MODEL_KEY, model)?;
        Ok(())
    }

    pub fn set_lock_minutes(&mut self, minutes: u32) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(LOCK_KEY, &minutes.clamp(1, 60).to_string())?;
        Ok(())
    }

    pub fn set_practitioner(&mut self, names: Vec<String>) -> Result<(), CoreError> {
        let names = names
            .into_iter()
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .collect();
        self.vault_mut()?
            .set_practitioner(&dv_vault::Practitioner { names })?;
        Ok(())
    }

    // ------------------------------------------------------------ cases

    pub fn list_cases(&mut self) -> Result<Vec<CaseSummary>, CoreError> {
        Ok(self.vault_ref()?.list_cases()?)
    }

    pub fn create_case(
        &mut self,
        meta: CaseMeta,
        identities: Vec<IdentityInput>,
    ) -> Result<String, CoreError> {
        check_meta(&meta)?;
        let v = self.vault_mut()?;
        let id = v.create_case(&meta)?;
        v.set_identities(&id, &identities)?;
        Ok(id)
    }

    pub fn update_case(&mut self, case_id: &str, mut meta: CaseMeta) -> Result<(), CoreError> {
        check_meta(&meta)?;
        // Which assessment a follow-up belongs to is set when it is opened, never edited.
        meta.follows = self.vault_ref()?.case_meta(case_id)?.follows;
        Ok(self.vault_mut()?.update_case_meta(case_id, &meta)?)
    }

    pub fn set_identities(
        &mut self,
        case_id: &str,
        identities: Vec<IdentityInput>,
    ) -> Result<Vec<dv_domain::Identity>, CoreError> {
        Ok(self.vault_mut()?.set_identities(case_id, &identities)?)
    }

    pub fn add_input(
        &mut self,
        case_id: &str,
        kind: InputKind,
        title: &str,
        content: &str,
    ) -> Result<dv_domain::CaseInput, CoreError> {
        Ok(self.vault_mut()?.add_input(case_id, kind, title, content)?)
    }

    pub fn update_input(
        &mut self,
        case_id: &str,
        input_id: &str,
        title: &str,
        content: &str,
    ) -> Result<(), CoreError> {
        let before = self
            .vault_ref()?
            .inputs(case_id)?
            .into_iter()
            .find(|i| i.id == input_id)
            .map(|i| i.content);
        self.vault_mut()?
            .update_input(case_id, input_id, title, content)?;
        if before.as_deref() != Some(content) {
            self.forget_sorting(case_id, input_id)?;
        }
        Ok(())
    }

    /// The instruments with their fixed tables (knowledge/instruments.md).
    #[must_use]
    pub fn score_instruments() -> Vec<dv_domain::Instrument> {
        dv_domain::instruments()
    }

    /// The exact text [`Core::save_scores`] would store, for the live preview while typing.
    pub fn preview_scores(sheet: &dv_domain::ScoreSheet) -> Result<String, CoreError> {
        dv_domain::format_sheet(sheet).map_err(CoreError::Refused)
    }

    /// Save entered scores as a "test scores" material: the text (with ranges computed from the
    /// fixed tables) is what feeds the sections; the sheet itself is kept sealed for editing.
    pub fn save_scores(
        &mut self,
        case_id: &str,
        input_id: Option<&str>,
        sheet: &dv_domain::ScoreSheet,
    ) -> Result<dv_domain::CaseInput, CoreError> {
        if sheet.entries.is_empty() {
            return Err(CoreError::Refused("לא הוזנו ציונים.".into()));
        }
        let text = dv_domain::format_sheet(sheet).map_err(CoreError::Refused)?;
        let title = dv_domain::instruments()
            .into_iter()
            .find(|i| i.key == sheet.instrument)
            .map(|i| format!("ציוני {}", i.name))
            .unwrap_or_default();
        let data = serde_json::to_string(sheet).map_err(|e| CoreError::Internal(e.to_string()))?;
        let v = self.vault_mut()?;
        let input = match input_id {
            Some(id) => {
                let existing = v
                    .inputs(case_id)?
                    .into_iter()
                    .find(|i| i.id == id && i.kind == InputKind::TestScores)
                    .ok_or_else(|| CoreError::NotFound("טבלת הציונים".into()))?;
                v.update_input(case_id, id, &title, &text)?;
                dv_domain::CaseInput {
                    title,
                    content: text,
                    ..existing
                }
            }
            None => v.add_input(case_id, InputKind::TestScores, &title, &text)?,
        };
        v.set_input_data(case_id, &input.id, &data)?;
        Ok(input)
    }

    /// The sheet behind a scores material, if it was entered in the score table.
    pub fn score_sheet(
        &mut self,
        case_id: &str,
        input_id: &str,
    ) -> Result<Option<dv_domain::ScoreSheet>, CoreError> {
        let Some(data) = self.vault_ref()?.input_data(case_id, input_id)? else {
            return Ok(None);
        };
        serde_json::from_str(&data)
            .map(Some)
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    pub fn delete_input(&mut self, case_id: &str, input_id: &str) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.delete_input(case_id, input_id)?)
    }

    /// Read a document and show what would be stored and hidden. Nothing is saved until the
    /// psychologist confirms (then the UI calls [`Core::add_input`] with the reviewed text).
    pub fn import_document(
        &mut self,
        case_id: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<ImportPreview, CoreError> {
        self.vault_ref()?.case_meta(case_id)?;
        let extracted = match &self.ingest_exe {
            Some(exe) => {
                dv_ingest::worker::run(exe, file_name, bytes, dv_ingest::worker::DEFAULT_TIMEOUT)
            }
            None => dv_ingest::extract(bytes, file_name),
        }
        .map_err(|e| CoreError::Refused(e.message_he()))?;

        let body = self.preview_filter(case_id, &extracted.body)?;
        let margins = self.preview_filter(case_id, &extracted.margins)?;
        let data = self.privacy_data(case_id)?;
        let known: HashSet<String> = data
            .identities
            .iter()
            .flat_map(|i| std::iter::once(i.value.clone()).chain(i.aliases.clone()))
            .chain(data.practitioner.iter().cloned())
            .map(|v| normalize(&v))
            .collect();

        let mut suggestions: Vec<NameSuggestion> = Vec::new();
        let mut suggest = |value: &str, source: String, role: Role| {
            let norm = normalize(value);
            if !norm.is_empty()
                && !known.contains(&norm)
                && !suggestions.iter().any(|s| normalize(&s.value) == norm)
            {
                suggestions.push(NameSuggestion {
                    value: value.trim().to_owned(),
                    source,
                    role,
                });
            }
        };
        for (field, value) in &extracted.metadata {
            let (label, role) = match field.as_str() {
                "creator" => ("יוצר המסמך", Role::Other),
                "lastModifiedBy" => ("שמר לאחרונה", Role::Other),
                "Manager" => ("מנהל", Role::Other),
                "Company" => ("ארגון", Role::Institution),
                _ => continue,
            };
            suggest(value, format!("מאפייני הקובץ · {label}"), role);
        }
        for s in &margins.suspects {
            suggest(&s.token, "כותרת עליונה/תחתונה".to_owned(), s.suggested_role);
        }
        // Names the filter already knows are hidden anyway; declared names in the margins
        // need nothing. Title / subject fields may hide names too.
        for (field, value) in &extracted.metadata {
            if matches!(
                field.as_str(),
                "title" | "subject" | "keywords" | "description"
            ) {
                for s in self.preview_filter(case_id, value)?.suspects {
                    suggest(
                        &s.token,
                        "מאפייני הקובץ · כותרת".to_owned(),
                        s.suggested_role,
                    );
                }
            }
        }

        // The document's own first line ("דוח קלינאית תקשורת – אבחון שפתי") names it better
        // than a file name ("slp2").
        let first_line = extracted
            .body
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .filter(|l| {
                (3..=80).contains(&l.chars().count())
                    && l.chars().filter(char::is_ascii_digit).count() < 6
                    && !l.trim_end_matches('.').contains(". ")
            })
            .map(str::to_owned);
        let stem = first_line.unwrap_or_else(|| {
            Path::new(file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(file_name)
                .trim()
                .to_owned()
        });
        Ok(ImportPreview {
            file_name: file_name.to_owned(),
            format: match extracted.format {
                dv_ingest::Format::Docx => "docx",
                dv_ingest::Format::Odt => "odt",
                dv_ingest::Format::Pdf => "pdf",
                dv_ingest::Format::Text => "text",
            }
            .to_owned(),
            pages: extracted.pages,
            title: stem,
            suggested_kind: InputKind::guess(&extracted.body, file_name),
            preview: body.original_segments,
            suspects: body.suspects,
            hidden: body.hidden,
            body: extracted.body,
            left_out: extracted
                .margins
                .lines()
                .map(str::to_owned)
                .filter(|l| !l.trim().is_empty())
                .collect(),
            name_suggestions: suggestions,
            warnings: extracted.warnings,
        })
    }

    fn privacy_data(&mut self, case_id: &str) -> Result<PrivacyData, CoreError> {
        let v = self.vault_ref()?;
        Ok(PrivacyData {
            case_id: case_id.to_owned(),
            identities: v.all_identities()?,
            practitioner: v.practitioner()?.names,
            allow: v.not_a_name_hmacs(case_id)?.into_iter().collect(),
            is_name: v.is_name_hmacs(case_id)?.into_iter().collect(),
        })
    }

    /// Filter one text for the case (used by the upload preview and the review screen).
    pub fn preview_filter(
        &mut self,
        case_id: &str,
        text: &str,
    ) -> Result<FilterOutcome, CoreError> {
        let data = self.privacy_data(case_id)?;
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
        let is_name = |t: &str| data.is_name.contains(&v.token_hmac(t));
        let ctx = PrivacyContext {
            case_id: &data.case_id,
            identities: &data.identities,
            practitioner: &data.practitioner,
            allowlisted: &allow,
            confirmed_names: &is_name,
            today: today(),
        };
        filter(text, &ctx).map_err(|e| CoreError::Internal(e.to_string()))
    }

    pub fn decide_suspect(
        &mut self,
        case_id: &str,
        token: &str,
        decision: SuspectDecision,
    ) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        match decision {
            SuspectDecision::Hide { role } => {
                let mut ids: Vec<IdentityInput> = v
                    .identities(case_id)?
                    .into_iter()
                    .map(|i| IdentityInput {
                        id: Some(i.id),
                        role: i.role,
                        value: i.value,
                        aliases: i.aliases,
                    })
                    .collect();
                ids.push(IdentityInput {
                    id: None,
                    role,
                    value: token.trim().to_owned(),
                    aliases: Vec::new(),
                });
                v.set_identities(case_id, &ids)?;
            }
            SuspectDecision::IsName => v.mark_is_name(case_id, &normalize(token))?,
            // Per case: a word that is ordinary here may be a name in another case.
            SuspectDecision::NotAName => v.mark_not_a_name(Some(case_id), &normalize(token))?,
        }
        Ok(())
    }

    /// No API key and no test transport: answers are built locally and nothing is sent.
    fn demo_mode(&mut self) -> Result<bool, CoreError> {
        Ok(self.vault_ref()?.secret(API_KEY)?.is_none() && self.transport.is_none())
    }

    fn model_config(&mut self) -> Result<ModelConfig, CoreError> {
        self.model_for(Task::Draft)
    }

    /// How long Claude may think, by task and by the speed chosen in settings. Sorting only
    /// classifies passages; drafting and consultation weigh more, and "thorough" asks for most.
    fn model_for(&mut self, task: Task) -> Result<ModelConfig, CoreError> {
        let v = self.vault_ref()?;
        let model = v
            .setting(MODEL_KEY)?
            .unwrap_or_else(|| dv_ai::DEFAULT_MODEL.to_owned());
        let speed = v
            .setting(SPEED_KEY)?
            .unwrap_or_else(|| DEFAULT_SPEED.to_owned());
        let effort = match (task, speed.as_str()) {
            (Task::Sort, _) | (_, "fast") => "low",
            (_, "thorough") => "high",
            _ => "medium",
        };
        Ok(ModelConfig {
            model,
            effort: effort.to_owned(),
        })
    }

    /// `fast` | `balanced` | `thorough`.
    pub fn set_speed(&mut self, speed: &str) -> Result<(), CoreError> {
        if !["fast", "balanced", "thorough"].contains(&speed) {
            return Err(CoreError::Refused("מהירות לא מוכרת.".to_owned()));
        }
        self.vault_mut()?.set_setting(SPEED_KEY, speed)?;
        Ok(())
    }

    pub fn case_detail(&mut self, case_id: &str) -> Result<CaseDetail, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let v = self.vault_ref()?;
        let meta = v.case_meta(case_id)?;
        let identities = v.identities(case_id)?;
        let inputs = v.inputs(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let routing = Core::material_routing(v, &structure, case_id, &inputs)?;
        let retention_default = v
            .list_cases()?
            .into_iter()
            .chain(v.list_trash()?)
            .find(|c| c.id == case_id)
            .map(|c| retention::default_until(c.created_at, c.updated_at, c.meta.age.as_ref()))
            .unwrap_or_default();
        let sortable: Vec<&str> = sorting::sortable(&structure)
            .iter()
            .map(|s| s.key.as_str())
            .collect();
        let mut sections = Vec::new();
        for part in &structure.parts {
            for s in &part.sections {
                let paragraphs: Vec<ParagraphView> = v
                    .drafts(case_id, &s.key)?
                    .into_iter()
                    .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
                    .map(|d| ParagraphView {
                        id: d.id,
                        text: restore(&d.text_tagged, &identities, practitioner.as_deref()),
                        status: d.status,
                        by_ai: d.author == Author::Ai,
                        sources: d
                            .source_refs
                            .iter()
                            .map(|r| {
                                inputs.iter().find(|i| &i.id == r).map_or_else(
                                    || r.clone(),
                                    |i| format!("{} · {}", i.kind.label_he(), i.title),
                                )
                            })
                            .collect(),
                        warnings: Vec::new(),
                        replaces: d.replaces,
                    })
                    .collect();
                let source_count =
                    u32::try_from(routing.iter().filter(|r| r.feeds.contains(&s.key)).count())
                        .unwrap_or(0);
                let approved = paragraphs.iter().any(|p| p.status == DraftStatus::Approved);
                sections.push(SectionView {
                    key: s.key.clone(),
                    title: s.title.clone(),
                    part: part.title.clone(),
                    source_count,
                    sortable: sortable.contains(&s.key.as_str()),
                    paragraphs,
                    approved,
                });
            }
        }
        Ok(CaseDetail {
            id: case_id.to_owned(),
            meta,
            identities,
            inputs,
            routing,
            sections,
            retention_default,
        })
    }

    pub fn chat(&mut self, case_id: &str, section_key: &str) -> Result<Vec<ChatView>, CoreError> {
        let v = self.vault_ref()?;
        let identities = v.identities(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        Ok(v.messages(case_id, section_key)?
            .into_iter()
            .map(|m| ChatView {
                role: if m.role == ChatRole::User {
                    "user".to_owned()
                } else {
                    "assistant".to_owned()
                },
                text: restore(&m.text_tagged, &identities, practitioner.as_deref()),
                hidden: m.hidden,
                demo: m.demo,
            })
            .collect())
    }

    pub fn approve_paragraph(&mut self, case_id: &str, draft_id: &str) -> Result<(), CoreError> {
        Ok(self
            .vault_mut()?
            .set_draft_status(case_id, draft_id, DraftStatus::Approved)?)
    }

    /// "אישור כל הטיוטה": every paragraph still waiting in the section, as it stands.
    pub fn approve_section(&mut self, case_id: &str, section_key: &str) -> Result<u32, CoreError> {
        let v = self.vault_mut()?;
        let waiting: Vec<String> = v
            .drafts(case_id, section_key)?
            .into_iter()
            .filter(|d| d.status == DraftStatus::Proposed)
            .map(|d| d.id)
            .collect();
        for id in &waiting {
            v.set_draft_status(case_id, id, DraftStatus::Approved)?;
        }
        Ok(u32::try_from(waiting.len()).unwrap_or(u32::MAX))
    }

    pub fn reject_paragraph(&mut self, case_id: &str, draft_id: &str) -> Result<(), CoreError> {
        Ok(self
            .vault_mut()?
            .set_draft_status(case_id, draft_id, DraftStatus::Rejected)?)
    }

    /// Manual edit: the text (with real names) is stored tagged.
    pub fn edit_paragraph(
        &mut self,
        case_id: &str,
        draft_id: &str,
        text: &str,
    ) -> Result<(), CoreError> {
        let tagged = self.preview_filter(case_id, text)?.tagged;
        Ok(self.vault_mut()?.edit_draft(case_id, draft_id, &tagged)?)
    }

    pub fn add_own_paragraph(
        &mut self,
        case_id: &str,
        section_key: &str,
        text: &str,
    ) -> Result<(), CoreError> {
        let tagged = self.preview_filter(case_id, text)?.tagged;
        let v = self.vault_mut()?;
        let d = v.add_draft(case_id, section_key, &tagged, Author::User, &[])?;
        v.set_draft_status(case_id, &d.id, DraftStatus::Approved)?;
        Ok(())
    }

    // ------------------------------------------------------------ AI: prepare → approve → send

    fn gate(
        &mut self,
        data: &PrivacyData,
        body: &Value,
        kind: PendingKind,
        prepared: &mut Prepared,
    ) -> Result<(), CoreError> {
        let suspects = prepared.suspects.len();
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
        let is_name = |t: &str| data.is_name.contains(&v.token_hmac(t));
        let ctx = PrivacyContext {
            case_id: &data.case_id,
            identities: &data.identities,
            practitioner: &data.practitioner,
            allowlisted: &allow,
            confirmed_names: &is_name,
            today: today(),
        };
        let case_tags: HashSet<String> = data
            .identities
            .iter()
            .filter(|i| i.case_id == data.case_id)
            .map(|i| i.tag.clone())
            .collect();
        let req = GateRequest {
            body,
            ctx: &ctx,
            case_tags: &case_tags,
            unresolved_suspects: suspects,
            canaries: &[],
            max_bytes: MAX_REQUEST_BYTES,
        };
        match clear(&req) {
            Ok(payload) => {
                let id = payload.sha256().to_owned();
                prepared.approval_id = Some(id.clone());
                self.pending.insert(id, Pending { payload, kind });
            }
            Err(blocked) => {
                let codes: Vec<String> = blocked.reasons.iter().map(|r| r.code.clone()).collect();
                let case_opt = (!data.case_id.is_empty()).then_some(data.case_id.as_str());
                self.vault_mut()?.record(
                    AuditEvent::Blocked,
                    case_opt,
                    &serde_json::json!({ "codes": codes }),
                )?;
                prepared.blocked = blocked.reasons;
            }
        }
        Ok(())
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
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let section = structure
            .section(section_key)
            .ok_or_else(|| CoreError::NotFound(section_key.to_owned()))?
            .clone();
        let model = self.model_config()?;
        let data = self.privacy_data(case_id)?;
        let demo_mode = self.demo_mode()?;

        let (input, review, sources) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            let meta = v.case_meta(case_id)?;
            if meta.consent.is_none() {
                return Err(CoreError::ConsentMissing);
            }
            let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
            let is_name = |t: &str| data.is_name.contains(&v.token_hmac(t));
            let ctx = PrivacyContext {
                case_id: &data.case_id,
                identities: &data.identities,
                practitioner: &data.practitioner,
                allowlisted: &allow,
                confirmed_names: &is_name,
                today: today(),
            };
            let run = |t: &str| filter(t, &ctx).map_err(|e| CoreError::Internal(e.to_string()));
            let fixed_title = |sec: &dv_domain::ReportSection| model_title(sec, &ctx);
            let mut review = Review::default();
            let mut tagged_sources = Vec::new();
            let mut source_rows = Vec::new();
            let derived = DERIVED_SECTIONS.contains(&section_key);
            if !derived {
                for inp in v.inputs(case_id)? {
                    // D-022: the table, the sorting, and Einat's choice decide what goes here.
                    let table = sorting::table_for(&structure, inp.kind);
                    let count = passage_ranges(&inp.content).len();
                    let Some(feed) =
                        sorting::routing_of(v, case_id, &inp.id)?.feed(section_key, &table, count)
                    else {
                        continue;
                    };
                    let sid = format!("S{}", tagged_sources.len() + 1);
                    let title = run(&inp.title)?;
                    let content = sorting::section_text(&inp.content, &feed, &ctx)?;
                    let kind = match feed {
                        dv_domain::Feed::Whole => inp.kind.label_he().to_owned(),
                        dv_domain::Feed::Passages(_) => {
                            format!("{} · קטעים שנבחרו לסעיף", inp.kind.label_he())
                        }
                    };
                    // A title with nothing to hide is shown as the source's label, not as a part.
                    if title.original_segments.iter().any(|s| s.mark.is_some()) {
                        review.add(format!("{sid} · {kind} · כותרת"), &title);
                        review.add(format!("{sid} · {kind}"), &content);
                    } else {
                        review.add(format!("{sid} · {kind} · {}", title.tagged), &content);
                    }
                    tagged_sources.push(TaggedInput {
                        input_id: inp.id.clone(),
                        kind_label: inp.kind.label_he().to_owned(),
                        title_tagged: title.tagged,
                        content_tagged: content.tagged.clone(),
                    });
                    source_rows.push((sid, inp.id, content.tagged));
                }
            }
            let mut approved_context = Vec::new();
            if derived {
                for s in structure
                    .sections()
                    .filter(|s| s.key != section_key && !DERIVED_SECTIONS.contains(&s.key.as_str()))
                {
                    let text: Vec<String> = v
                        .drafts(case_id, &s.key)?
                        .into_iter()
                        .filter(|d| d.status == DraftStatus::Approved)
                        .map(|d| d.text_tagged)
                        .collect();
                    if !text.is_empty() {
                        let out = run(&text.join("\n"))?;
                        review.add(format!("סעיף מאושר · {}", s.title), &out);
                        approved_context.push((fixed_title(s)?, out.tagged));
                    }
                }
            }
            // The current draft may contain manual edits, so it goes through review too.
            let mut current = Vec::new();
            for d in v
                .drafts(case_id, section_key)?
                .into_iter()
                .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
            {
                let out = run(&d.text_tagged)?;
                review.add("הטיוטה הנוכחית".to_owned(), &out);
                current.push(out.tagged);
            }
            let history: Vec<TaggedTurn> = v
                .messages(case_id, section_key)?
                .into_iter()
                .map(|m| {
                    run(&m.text_tagged).map(|o| TaggedTurn {
                        role: if m.role == ChatRole::User {
                            "user".to_owned()
                        } else {
                            "assistant".to_owned()
                        },
                        text_tagged: o.tagged,
                    })
                })
                .collect::<Result<_, _>>()?;
            let instr = run(instruction)?;
            review.add("הבקשה שלך".to_owned(), &instr);
            let input = SectionInput {
                section_key: section.key.clone(),
                section_title: fixed_title(&section)?,
                age: meta.age.map(dv_domain::Age::display),
                gender: meta.child_gender,
                sources: tagged_sources,
                approved_context,
                current_draft: current,
                history,
                instruction_tagged: instr.tagged.clone(),
                style_profile: None,
            };
            let sources = (
                section.key.clone(),
                instr.tagged,
                instr.hidden.clone(),
                source_rows,
            );
            (input, review, sources)
        };

        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let (body, _refs) = dv_ai::build_section_request(&model, &input, &nonce);
        let mut prepared = review.into_prepared(demo_mode);
        let (key, instruction_tagged, instr_hidden, rows) = sources;
        let kind = PendingKind::Section {
            case_id: case_id.to_owned(),
            section_key: key,
            instruction_tagged,
            hidden: instr_hidden,
            sources: rows,
            replaces: replaces.map(str::to_owned),
        };
        self.gate(&data, &body, kind, &mut prepared)?;
        Ok(prepared)
    }

    /// Take an approved request out of the core so it can be sent without holding the
    /// session (the app stays responsive while Claude answers).
    pub fn begin_send(&mut self, approval_id: &str) -> Result<Outgoing, CoreError> {
        let api_key = self.vault_ref()?.secret(API_KEY)?;
        let pending = self
            .pending
            .remove(approval_id)
            .ok_or_else(|| CoreError::NotFound("האישור פג. יש להכין את הבקשה מחדש.".to_owned()))?;
        Ok(Outgoing {
            pending,
            api_key,
            transport: self.transport.clone(),
        })
    }

    /// A failed send is not a send: the approval stays valid for a retry.
    fn restore_pending(&mut self, out: Outgoing) {
        self.pending
            .insert(out.pending.payload.sha256().to_owned(), out.pending);
    }

    /// Send exactly what was approved, check the answer, and store it (tagged).
    pub fn send_section(&mut self, approval_id: &str) -> Result<SectionResult, CoreError> {
        let out = self.begin_send(approval_id)?;
        let response = out.transmit();
        self.finish_section(out, response)
    }

    /// Check and store the answer to a section request.
    pub fn finish_section(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<SectionResult, CoreError> {
        if !matches!(out.pending.kind, PendingKind::Section { .. }) {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        }
        let (response, demo) = match response {
            Ok(r) => r,
            Err(e) => {
                self.restore_pending(out);
                return Err(e);
            }
        };
        let Pending { payload, kind } = out.pending;
        let PendingKind::Section {
            case_id,
            section_key,
            instruction_tagged,
            hidden,
            sources,
            replaces,
        } = kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        // The request went out: record it before anything about the reply can fail.
        let model = self.model_config()?.model;
        let payload_text = String::from_utf8_lossy(payload.body()).into_owned();
        let v = self.vault_mut()?;
        v.add_transmission(
            &case_id,
            &section_key,
            payload.sha256(),
            if demo { "demo" } else { &model },
            &payload_text,
        )?;
        v.record(
            AuditEvent::Send,
            Some(&case_id),
            &serde_json::json!({ "section": section_key, "demo": demo }),
        )?;
        let refs: Vec<(String, String)> = sources
            .iter()
            .map(|(sid, _, text)| (sid.clone(), text.clone()))
            .collect();
        let mut reply = dv_ai::parse_section(&response, &refs)?;
        let v = self.vault_mut()?;
        let identities = v.identities(&case_id)?;
        let case_tags: Vec<String> = identities.iter().map(|i| i.tag.clone()).collect();
        for p in &mut reply.paragraphs {
            for s in scan_model_output(&p.text, &case_tags) {
                p.warnings.push(format!("{}: {}", s.message, s.token));
            }
        }
        v.add_message(
            &case_id,
            &section_key,
            ChatRole::User,
            &instruction_tagged,
            &hidden,
            demo,
        )?;
        v.add_message(
            &case_id,
            &section_key,
            ChatRole::Assistant,
            &reply.reply,
            &[],
            demo,
        )?;
        let input_ids = |p: &dv_ai::ProposedParagraph| -> Vec<String> {
            p.source_refs
                .iter()
                .filter_map(|r| {
                    sources
                        .iter()
                        .find(|(sid, _, _)| sid == r)
                        .map(|(_, id, _)| id.clone())
                })
                .collect()
        };
        // A rewrite of one paragraph stays in its place; if it was approved or removed
        // meanwhile, the answer is added as a new proposal instead of touching it.
        let rewritten = match (&replaces, reply.paragraphs.is_empty()) {
            (Some(id), false) => {
                let text: Vec<&str> = reply.paragraphs.iter().map(|p| p.text.as_str()).collect();
                let mut refs: Vec<String> = reply.paragraphs.iter().flat_map(&input_ids).collect();
                refs.dedup();
                let text = text.join("\n\n");
                // A proposal is rewritten in place; an approved paragraph gets a new wording
                // beside it, which replaces it only when she approves (D-032).
                v.rewrite_proposal(&case_id, id, &text, &refs)?
                    || v.propose_rewording(&case_id, id, &text, &refs)?
            }
            _ => false,
        };
        if !rewritten {
            if replaces.is_none() && !reply.paragraphs.is_empty() {
                v.supersede_proposals(&case_id, &section_key)?;
            }
            for p in &reply.paragraphs {
                v.add_draft(&case_id, &section_key, &p.text, Author::Ai, &input_ids(p))?;
            }
        }

        let practitioner = v.practitioner()?.names.first().cloned();
        let show = |t: &str| restore(t, &identities, practitioner.as_deref());
        Ok(SectionResult {
            reply: show(&reply.reply),
            paragraphs: reply
                .paragraphs
                .into_iter()
                .map(|mut p| {
                    p.text = show(&p.text);
                    p
                })
                .collect(),
            questions: reply.questions.iter().map(|q| show(q)).collect(),
            missing: reply.missing.iter().map(|q| show(q)).collect(),
            contradictions: reply.contradictions.iter().map(|q| show(q)).collect(),
            demo,
        })
    }

    /// Prepare every section that has material (the "prepare report draft" button).
    pub fn prepare_full_draft(
        &mut self,
        case_id: &str,
    ) -> Result<Vec<(String, Prepared)>, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let fed: HashSet<String> = {
            let v = self.vault_ref()?;
            let inputs = v.inputs(case_id)?;
            Core::material_routing(v, &structure, case_id, &inputs)?
                .into_iter()
                .flat_map(|r| r.feeds)
                .collect()
        };
        let mut out = Vec::new();
        for s in structure
            .sections()
            .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()))
        {
            if fed.contains(&s.key) {
                let p = self.prepare_section(
                    case_id,
                    &s.key,
                    "כתוב/י טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.",
                )?;
                out.push((s.key.clone(), p));
            }
        }
        Ok(out)
    }

    /// Free consultation, optionally about a case (then approved sections go along, filtered).
    /// `conversation_id`: a saved conversation to continue (its earlier turns go along, as
    /// they were sent: filtered), or `None` for a new one.
    pub fn prepare_consult(
        &mut self,
        case_id: Option<&str>,
        conversation_id: Option<&str>,
        message: &str,
    ) -> Result<Prepared, CoreError> {
        // Earlier turns, as Einat saw them: filtered again now, with today's names and decisions,
        // and shown on the review screen like everything else that leaves the computer.
        let earlier = match conversation_id {
            Some(id) => {
                let (case_of, turns) = self.stored_consultation(id)?;
                if case_of.as_deref() != case_id {
                    return Err(CoreError::Refused(
                        "השיחה הזו שייכת לתיק אחר. אפשר לפתוח שיחה חדשה.".to_owned(),
                    ));
                }
                turns
            }
            None => Vec::new(),
        };
        let model = self.model_for(Task::Consult)?;
        let key = case_id.unwrap_or("").to_owned();
        let data = self.privacy_data(&key)?;
        let demo_mode = self.vault_ref()?.secret(API_KEY)?.is_none() && self.transport.is_none();
        let (input, review) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            if let Some(c) = case_id {
                if v.case_meta(c)?.consent.is_none() {
                    return Err(CoreError::ConsentMissing);
                }
            }
            let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
            let is_name = |t: &str| data.is_name.contains(&v.token_hmac(t));
            let ctx = PrivacyContext {
                case_id: &data.case_id,
                identities: &data.identities,
                practitioner: &data.practitioner,
                allowlisted: &allow,
                confirmed_names: &is_name,
                today: today(),
            };
            let msg = filter(message, &ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
            let mut review = Review::default();
            let mut history = Vec::new();
            for (n, t) in earlier.iter().enumerate() {
                let label = |who: &str| format!("{who} ({})", n / 2 + 1);
                if t.role == "user" {
                    // Einat's own words: filtered again, with today's names and decisions.
                    let out =
                        filter(&t.shown, &ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
                    review.add(label("שאלה קודמת"), &out);
                    history.push(TaggedTurn {
                        role: t.role.clone(),
                        text_tagged: out.tagged,
                    });
                } else {
                    // Claude's own answer goes back as it came (it only ever saw tags); shown
                    // on the review screen, without name questions about Claude's own words.
                    let out =
                        filter(&t.tagged, &ctx).map_err(|e| CoreError::Internal(e.to_string()))?;
                    review.add_context(label("תשובה קודמת"), &out);
                    history.push(TaggedTurn {
                        role: t.role.clone(),
                        text_tagged: t.tagged.clone(),
                    });
                }
            }
            review.add("השאלה שלך".to_owned(), &msg);
            let mut case_context = None;
            if let Some(c) = case_id {
                let structure = ReportStructure::load_default()
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                let mut lines = Vec::new();
                for s in structure.sections() {
                    for d in v
                        .drafts(c, &s.key)?
                        .into_iter()
                        .filter(|d| d.status == DraftStatus::Approved)
                    {
                        lines.push(format!("{}: {}", model_title(s, &ctx)?, d.text_tagged));
                    }
                }
                if !lines.is_empty() {
                    let out = filter(&lines.join("\n"), &ctx)
                        .map_err(|e| CoreError::Internal(e.to_string()))?;
                    review.add("סיכום התיק (סעיפים מאושרים)".to_owned(), &out);
                    case_context = Some(out.tagged);
                }
            }
            let input = dv_ai::ConsultInput {
                history,
                message_tagged: msg.tagged.clone(),
                case_context_tagged: case_context,
            };
            (input, review)
        };
        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let body = dv_ai::build_consult_request(&model, &input, &nonce);
        let mut prepared = review.into_prepared(demo_mode);
        let kind = PendingKind::Consult {
            case_id: case_id.map(str::to_owned),
            conversation_id: conversation_id.map(str::to_owned),
            message_tagged: input.message_tagged,
            message_shown: message.to_owned(),
            hidden: prepared.hidden.clone(),
        };
        self.gate(&data, &body, kind, &mut prepared)?;
        Ok(prepared)
    }

    pub fn send_consult(&mut self, approval_id: &str) -> Result<ConsultResult, CoreError> {
        let out = self.begin_send(approval_id)?;
        let response = out.transmit();
        self.finish_consult(out, response)
    }

    /// Check and keep the answer to a consultation (history in memory only).
    pub fn finish_consult(
        &mut self,
        out: Outgoing,
        response: Result<(Value, bool), CoreError>,
    ) -> Result<ConsultResult, CoreError> {
        if !matches!(out.pending.kind, PendingKind::Consult { .. }) {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        }
        let (response, demo) = match response {
            Ok(r) => r,
            Err(e) => {
                self.restore_pending(out);
                return Err(e);
            }
        };
        let PendingKind::Consult {
            case_id,
            conversation_id,
            message_tagged,
            message_shown,
            hidden,
        } = out.pending.kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        // The request went out: record it before anything about the reply can fail.
        self.vault_mut()?.record(
            AuditEvent::Send,
            case_id.as_deref(),
            &serde_json::json!({ "consult": true, "demo": demo }),
        )?;
        let answer = dv_ai::parse_consult(&response)?;
        let shown = match &case_id {
            Some(c) => {
                let identities = self.vault_ref()?.identities(c)?;
                restore(&answer, &identities, None)
            }
            None => answer.clone(),
        };
        let conversation_id = self.keep_consultation(
            conversation_id.as_deref(),
            case_id.as_deref(),
            consultations::Exchange {
                question_tagged: message_tagged,
                question_shown: message_shown,
                hidden,
                answer_tagged: answer,
                answer_shown: shown.clone(),
                demo,
            },
        )?;
        Ok(ConsultResult {
            answer: shown,
            demo,
            conversation_id,
        })
    }

    // ------------------------------------------------------------ Word report

    pub fn report_settings(&mut self) -> Result<ReportSettings, CoreError> {
        let v = self.vault_ref()?;
        if let Some(json) = v.setting(REPORT_KEY)? {
            if let Ok(s) = serde_json::from_str(&json) {
                return Ok(s);
            }
        }
        Ok(ReportSettings {
            title: "דוח אבחון פסיכולוגי".to_owned(),
            font: "David".to_owned(),
            confidentiality: "חסוי – מידע רפואי. מיועד להורים ולמי שההורים אישרו בלבד.".to_owned(),
            signature: v
                .practitioner()?
                .names
                .first()
                .cloned()
                .into_iter()
                .collect(),
        })
    }

    pub fn set_report_settings(&mut self, settings: &ReportSettings) -> Result<(), CoreError> {
        let json =
            serde_json::to_string(settings).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.vault_mut()?.set_setting(REPORT_KEY, &json)?;
        Ok(())
    }

    /// The report with real names, from approved paragraphs only, and what stops the export.
    fn build_report(
        &mut self,
        case_id: &str,
    ) -> Result<(dv_export::Report, ExportCheck), CoreError> {
        let settings = self.report_settings()?;
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let v = self.vault_ref()?;
        let meta = v.case_meta(case_id)?;
        let identities = v.identities(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let show = |t: &str| restore(t, &identities, practitioner.as_deref());

        let mut blocking = Vec::new();
        let mut to_complete = Vec::new();
        let known_tags: HashSet<String> = v
            .all_identities()?
            .into_iter()
            .map(|i| i.tag)
            .chain(dv_privacy::GENERIC_TAGS.iter().map(|t| (*t).to_owned()))
            .collect();
        let mut empty_sections = Vec::new();
        let mut included = 0u32;
        let mut parts = Vec::new();
        let mut signature = settings.signature.clone();
        for part in &structure.parts {
            let mut sections = Vec::new();
            for s in &part.sections {
                let mut paragraphs: Vec<String> = v
                    .drafts(case_id, &s.key)?
                    .into_iter()
                    .filter(|d| d.status == DraftStatus::Approved)
                    .map(|d| show(&d.text_tagged))
                    .collect();
                for p in &mut paragraphs {
                    for tag in dv_privacy::restore::remaining_tags(p) {
                        if known_tags.contains(&tag) {
                            // A real tag the names could not be put back into: never in a file.
                            blocking.push(format!("{}: נשארה תגית {tag}", s.title));
                        } else {
                            // Words the model put in square brackets ("[מחנכת]"): plain text.
                            let plain = format!("({})", &tag[1..tag.len() - 1]);
                            *p = p.replace(&tag, &plain);
                        }
                    }
                    for m in p.match_indices("[חסר") {
                        let rest = &p[m.0..];
                        let end = rest.find(']').map_or(rest.len(), |i| i + 1);
                        to_complete.push(format!("{}: {}", s.title, &rest[..end]));
                    }
                }
                if s.key == "signature" {
                    if !paragraphs.is_empty() {
                        signature = paragraphs
                            .iter()
                            .flat_map(|p| p.lines().map(str::to_owned))
                            .collect();
                    }
                    continue;
                }
                if paragraphs.is_empty() {
                    empty_sections.push(s.title.clone());
                } else {
                    included += 1;
                    sections.push(dv_export::ReportSection {
                        title: s.title.clone(),
                        paragraphs,
                    });
                }
            }
            if !sections.is_empty() {
                parts.push(dv_export::ReportPart {
                    title: part.title.clone(),
                    sections,
                });
            }
        }

        let child = identities
            .iter()
            .find(|i| i.role == Role::Child)
            .map(|i| i.value.clone());
        let child_label = match meta.child_gender {
            Some(dv_domain::GrammaticalGender::Female) => "שם הילדה",
            Some(dv_domain::GrammaticalGender::Male) => "שם הילד",
            None => "שם הילד/ה",
        };
        let (y, m, d) = today();
        let mut info = Vec::new();
        if let Some(name) = child {
            info.push(dv_export::InfoLine {
                label: child_label.to_owned(),
                value: name,
            });
        }
        if let Some(age) = meta.age {
            info.push(dv_export::InfoLine {
                label: "גיל בעת האבחון".to_owned(),
                value: age.display(),
            });
        }
        info.push(dv_export::InfoLine {
            label: "תאריך הדוח".to_owned(),
            value: format!("{d}.{m}.{y}"),
        });

        // Score tables entered in the table (not imported text), in the order they were added.
        let mut tables = Vec::new();
        for input in v
            .inputs(case_id)?
            .into_iter()
            .filter(|i| i.kind == InputKind::TestScores)
        {
            let Some(data) = v.input_data(case_id, &input.id)? else {
                continue;
            };
            let sheet: dv_domain::ScoreSheet =
                serde_json::from_str(&data).map_err(|e| CoreError::Internal(e.to_string()))?;
            let (title, rows, note) = dv_domain::sheet_table(&sheet).map_err(CoreError::Refused)?;
            tables.push(dv_export::ScoreTable {
                title,
                columns: ["מדד", "ציון", "אחוזון", "טווח"]
                    .map(str::to_owned)
                    .to_vec(),
                rows: rows
                    .into_iter()
                    .map(|r| vec![r.measure, r.score, r.percentile, r.range])
                    .collect(),
                note,
            });
        }
        let score_tables = u32::try_from(tables.len()).unwrap_or(u32::MAX);

        let report = dv_export::Report {
            title: settings.title,
            info,
            parts,
            tables,
            signature,
            confidentiality: settings.confidentiality,
            font: settings.font,
        };
        for leftover in dv_export::leftover_placeholders(&report) {
            if leftover.starts_with("[חסר") {
                continue; // Pointed out in `to_complete`; hers to fill in.
            }
            let msg = format!("נשאר בדוח סימון בסוגריים מרובעים: {leftover}");
            if !blocking.iter().any(|b| b.contains(&leftover)) {
                blocking.push(msg);
            }
        }
        if included == 0 {
            blocking.push(
                "אין עדיין פסקאות מאושרות. מאשרים פסקאות בטיוטה, והן נכנסות לדוח.".to_owned(),
            );
        }
        let code: String = meta
            .code
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        let file_name = format!(
            "דוח אבחון {}.docx",
            if code.is_empty() { "תיק" } else { &code }
        );
        Ok((
            report,
            ExportCheck {
                blocking,
                to_complete,
                empty_sections,
                included_sections: included,
                score_tables,
                file_name,
            },
        ))
    }

    pub fn check_export(&mut self, case_id: &str) -> Result<ExportCheck, CoreError> {
        Ok(self.build_report(case_id)?.1)
    }

    /// The Word file's bytes. Refused while anything blocks; with a password it is encrypted
    /// (ECMA-376 Agile, AES-256), the way Word protects files.
    pub fn export_report(
        &mut self,
        case_id: &str,
        password: Option<&str>,
    ) -> Result<Vec<u8>, CoreError> {
        let (report, check) = self.build_report(case_id)?;
        if !check.blocking.is_empty() {
            return Err(CoreError::Refused(check.blocking.join(" · ")));
        }
        let docx = dv_export::render(&report).map_err(|e| CoreError::Internal(e.to_string()))?;
        let out = match password {
            Some(pw) => dv_export::encrypt(&docx, pw).map_err(|e| match e {
                dv_export::ExportError::WeakPassword => CoreError::Refused(format!(
                    "סיסמה לקובץ צריכה להיות באורך {} תווים לפחות.",
                    dv_export::MIN_PASSWORD_CHARS
                )),
                other => CoreError::Internal(other.to_string()),
            })?,
            None => docx,
        };
        self.vault_mut()?.record(
            AuditEvent::Export,
            Some(case_id),
            &serde_json::json!({ "protected": password.is_some(), "sections": check.included_sections }),
        )?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
