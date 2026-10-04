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
mod readiness;
mod retention;
mod sorting;
mod style;
mod unsaved;
pub mod update;
mod usage;
mod views;
mod why;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dv_ai::{ModelConfig, SectionInput, TaggedInput, TaggedTurn, ALLOWED_MODELS};
use dv_domain::{
    passage_ranges, Author, CaseMeta, CaseSummary, ChatRole, DraftStatus, FoundName, IdentityInput,
    IdentitySource, InputKind, ReportStructure, Role,
};
use dv_egress::{AnthropicTransport, EgressError, Transport};
use dv_ipc::{PingResponse, IPC_VERSION};
use dv_privacy::restore::{restore, scan_model_output};
use dv_privacy::text::normalize;
use dv_privacy::{
    clear, filter, AutoHidden, AutoKind, Checks, ClearedPayload, FilterOutcome, GateRequest,
    PrivacyContext,
};
use dv_vault::{Argon2Params, AuditEvent, Vault, VaultError};
use serde_json::Value;

pub use activity::ACTIVITY_PAGE;
pub use backup::{
    auto_backup_file_name, backup_file_name, AUTO_KEEP, BACKUP_DAYS, MAX_BACKUP_BYTES,
};
pub(crate) use dates::today;
pub use dv_ai::{StyleItem, StyleKind, StyleOrigin, StyleProfile};
pub use dv_vault::BACKUP_EXTENSION;
pub use followup::FollowUpView;
pub use library::TRASH_DAYS;
pub use readiness::{Readiness, ReadinessItem};
pub use retention::{KEEP_UNTIL_AGE, KEEP_YEARS_AFTER_LAST_CHANGE};
pub use style::{
    StyleAnalysisResult, StyleImportPreview, StyleOverview, StylePartView, StyleProfileView,
    StyleSectionLabel, StyleSourceView, StyleSuggestion, StyleVersionView,
};
pub use unsaved::UnsavedEdit;
pub use usage::UsageSummary;
pub use views::{
    ActivityEntry, ActivityPage, AppStatus, BackupCheckView, BackupDone, BackupStatus, CaseDetail,
    ChatView, ConsultResult, ConsultTurnView, ConsultationSummary, ConsultationView, CreatedVault,
    ExportCheck, ImportPreview, MaterialRouting, NameMatch, NameSuggestion, ParagraphView,
    Prepared, ReportSettings, RetentionItem, ReviewPart, SectionResult, SectionView, SortResult,
    SourceExcerpt, StagedBackup, SuspectDecision, UiError,
};

const API_KEY: &str = "anthropic_api_key";
const MODEL_KEY: &str = "model";
const LOCK_KEY: &str = "lock_minutes";
const FIRST_USE_KEY: &str = "first_use_day";
const REVIEW_KEY: &str = "review_only_suspect";
const SCREEN_KEY: &str = "screen_protection";
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

/// The cloud-synced folder (OneDrive, Dropbox…) that holds `path`, if any. A report with real
/// names is not saved where it would be uploaded on its own.
#[must_use]
pub fn cloud_synced_folder(path: &Path) -> Option<String> {
    dv_vault::env::cloud_synced_component(path)
}

/// The Word file's name. It shows in Downloads, in "recent files" and as an e-mail
/// attachment, so it never carries a name: a case code that holds a name declared in the
/// case gives way to the date.
fn report_file_name(
    code: &str,
    identities: &[dv_domain::Identity],
    (y, m, d): (i32, u32, u32),
) -> String {
    let lower = code.to_lowercase();
    let holds_name = identities
        .iter()
        .flat_map(|i| std::iter::once(&i.value).chain(i.aliases.iter()))
        .flat_map(|n| n.split_whitespace())
        .map(str::to_lowercase)
        .any(|w| w.chars().count() >= 2 && lower.contains(&w));
    if code.is_empty() || holds_name {
        format!("דוח אבחון {y:04}-{m:02}-{d:02}.docx")
    } else {
        format!("דוח אבחון {code}.docx")
    }
}

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

/// The score sheets entered in the case's score table (the scores the text is checked against).
fn score_sheets(
    v: &Vault,
    case_id: &str,
    inputs: &[dv_domain::CaseInput],
) -> Result<Vec<dv_domain::ScoreSheet>, CoreError> {
    let mut out = Vec::new();
    for i in inputs.iter().filter(|i| i.kind == InputKind::TestScores) {
        if let Some(data) = v.input_data(case_id, &i.id)? {
            if let Ok(sheet) = serde_json::from_str(&data) {
                out.push(sheet);
            }
        }
    }
    Ok(out)
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
        /// For a section written from the approved ones (summary…): their tagged text, which
        /// the numbers in the answer are checked against.
        approved: Vec<String>,
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
    /// Step 1 of the writing-style profile: one past report (D-043).
    StyleAnalysis { source_id: String },
    /// Step 2: the analyses of `reports` past reports.
    StyleSynthesis { reports: u32 },
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
        self.transmit_with(&|_| {})
    }

    /// The same, with the number of words Claude has written so far, as they arrive.
    pub fn transmit_with(&self, progress: &dyn Fn(u32)) -> Result<(Value, bool), CoreError> {
        if let Some(t) = &self.transport {
            return Ok((t.send_streaming(&self.pending.payload, progress)?, false));
        }
        match &self.api_key {
            Some(key) => Ok((
                AnthropicTransport::new(key)?.send_streaming(&self.pending.payload, progress)?,
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
    /// The local OCR engine shipped next to the app, for scans and photos (D-048).
    ocr: Option<dv_ingest::ocr::Engine>,
    /// A backup file chosen for the drill or a restore (encrypted bytes).
    staged_backup: Option<Vec<u8>>,
    /// A past report read for the style profile, waiting for her confirmation (D-043).
    style_staged: HashMap<String, style::StagedStyle>,
    /// The last automatic backup attempt this session (a failed one waits before the next).
    auto_backup_tried: Option<Instant>,
    /// The paragraph being edited right now (memory only; kept in the vault on lock).
    unsaved: Option<UnsavedEdit>,
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
    auto_hidden: Vec<AutoHidden>,
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
        self.note_hidden(out);
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

    /// What the filter hid on its own in a text that goes out, shown on the card or not.
    fn note_hidden(&mut self, out: &FilterOutcome) {
        for a in &out.auto_hidden {
            if !self
                .auto_hidden
                .iter()
                .any(|x| x.token == a.token && x.tag == a.tag)
            {
                self.auto_hidden.push(a.clone());
            }
        }
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

    /// The review screen of a case request: the card also lists the names the filter kept
    /// with the case earlier (from a saved material, a document, the first build of this
    /// request) wherever their tags go out now, so each one can still be restored.
    fn into_case_prepared(mut self, demo_mode: bool, data: &PrivacyData) -> Prepared {
        let out: String = self
            .parts
            .iter()
            .flat_map(|p| p.outgoing.iter().map(|s| s.text.as_str()))
            .collect();
        for i in data.identities.iter().filter(|i| {
            i.case_id == data.case_id && i.source != IdentitySource::Manual && out.contains(&i.tag)
        }) {
            if !self.auto_hidden.iter().any(|a| a.tag == i.tag) {
                self.auto_hidden.push(AutoHidden {
                    token: i.value.clone(),
                    tag: i.tag.clone(),
                    role: i.role,
                    reason: i.reason.clone(),
                    uncertain: false,
                    kind: AutoKind::Name,
                });
            }
        }
        self.into_prepared(demo_mode)
    }

    fn into_prepared(self, demo_mode: bool) -> Prepared {
        Prepared {
            approval_id: None,
            parts: self.parts,
            suspects: Vec::new(),
            auto_hidden: self.auto_hidden,
            hidden: self.hidden,
            checks: self.checks,
            blocked: Vec::new(),
            demo_mode,
        }
    }
}

/// A value from a file's properties that may be a person or an organization, not a program
/// or a placeholder ("Microsoft Office User", "admin", a number).
fn person_like(value: &str) -> bool {
    const GENERIC: &[&str] = &[
        "user",
        "admin",
        "administrator",
        "owner",
        "office",
        "microsoft",
        "windows",
        "word",
        "author",
        "unknown",
        "guest",
        "pc",
        "משתמש",
        "מנהל",
        "אורח",
    ];
    let words: Vec<String> = normalize(value)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    (1..=4).contains(&words.len())
        && words.iter().all(|w| {
            w.chars().count() >= 2
                && w.chars()
                    .all(|c| c.is_alphabetic() || matches!(c, '\'' | '"' | '-'))
                && !GENERIC.contains(&w.as_str())
        })
}

/// The new names among what the filter hid (`autos`): kept as identities of the case from
/// then on, so every text of it hides them under the same tag and the gate refuses them.
fn found_names(data: &PrivacyData, autos: &[AutoHidden]) -> Vec<FoundName> {
    let mut out: Vec<FoundName> = Vec::new();
    for a in autos {
        if !matches!(a.kind, AutoKind::Name | AutoKind::OtherCase)
            || data
                .identities
                .iter()
                .any(|i| i.case_id == data.case_id && i.tag == a.tag)
            || out
                .iter()
                .any(|f| normalize(&f.value) == normalize(&a.token))
        {
            continue;
        }
        out.push(FoundName {
            value: a.token.trim().to_owned(),
            role: a.role,
            source: IdentitySource::Auto,
            reason: a.reason.clone(),
        });
    }
    out
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
        Ok(o.auto_hidden.is_empty() && o.tagged == t)
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
            ocr: None,
            staged_backup: None,
            style_staged: HashMap::new(),
            auto_backup_tried: None,
            unsaved: None,
        }
    }

    /// Read documents in a separate worker process (the app passes its own binary).
    #[must_use]
    pub fn with_ingest_worker(mut self, exe: PathBuf) -> Self {
        self.ocr = dv_ingest::ocr::Engine::locate(&exe);
        self.ingest_exe = Some(exe);
        self
    }

    /// Read a document: in the isolated worker when the app set one, then OCR for a scan.
    pub(crate) fn read_document(
        &self,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<dv_ingest::Extracted, CoreError> {
        dv_ingest::import(
            self.ingest_exe.as_deref(),
            self.ocr.as_ref(),
            file_name,
            bytes,
        )
        .map_err(|e| CoreError::Refused(e.message_he()))
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

    /// The psychologist is at work in the window (typing a paragraph, scrolling) without
    /// calling the core: that counts as activity too, so a long paragraph is not lost to the
    /// idle lock. Never reopens or extends a vault that has already passed its idle time.
    pub fn touch(&mut self) {
        let limit = Duration::from_secs(u64::from(self.lock_minutes()) * 60);
        if self.vault.is_some() && self.last_activity.elapsed() <= limit {
            self.last_activity = Instant::now();
        }
    }

    /// The computer itself was locked (Win+L, the lock screen): lock the vault with it.
    /// Returns true when it locked.
    pub fn lock_with_computer(&mut self) -> bool {
        if self.vault.is_none() {
            return false;
        }
        self.lock_because(Some("computer_locked"));
        true
    }

    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.vault.is_some()
    }

    /// Seconds left before the idle lock, while the vault is open (shown as a warning in the
    /// last minute).
    fn idle_lock_in(&self) -> Option<u32> {
        self.vault.as_ref()?;
        let limit = Duration::from_secs(u64::from(self.lock_minutes()) * 60);
        let left = limit.saturating_sub(self.last_activity.elapsed()).as_secs();
        Some(u32::try_from(left).unwrap_or(u32::MAX))
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
            idle_lock_in: self.idle_lock_in(),
            practitioner,
            review_only_suspect,
            review_choice_available,
            screen_protection: self
                .vault
                .as_ref()
                .is_none_or(|v| v.setting(SCREEN_KEY).ok().flatten().as_deref() != Some("0")),
        }
    }

    /// Whether screenshots and screen sharing see the open vault (D-037). Off is recorded in
    /// the activity log; the locked screen stays protected either way.
    pub fn set_screen_protection(&mut self, on: bool) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        v.set_setting(SCREEN_KEY, if on { "1" } else { "0" })?;
        Ok(())
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
        self.keep_unsaved();
        self.pending.clear();
        self.staged_backup = None;
        self.style_staged.clear();
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
        let input = self.vault_mut()?.add_input(case_id, kind, title, content)?;
        self.learn_input(case_id, title, content)?;
        Ok(input)
    }

    /// A material's new names are kept with the case when it is saved, so every later text
    /// (a request, another material) hides them under the same tag without asking.
    fn learn_input(&mut self, case_id: &str, title: &str, content: &str) -> Result<(), CoreError> {
        self.learn(case_id, title)?;
        self.learn(case_id, content)?;
        Ok(())
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
        self.learn_input(case_id, title, content)
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
        self.learn_input(case_id, &input.title, &input.content)?;
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
        let extracted = self.read_document(file_name, bytes)?;

        // Names in the margins and the file's properties are not imported, but they are the
        // names most likely to appear in the body too: kept with the case first, so the body
        // hides them under their tags (fail-closed: if the import is cancelled, they stay
        // hidden, which costs nothing).
        let mut found: Vec<FoundName> = Vec::new();
        let keep = |value: &str, role: Role, reason: String, found: &mut Vec<FoundName>| {
            let value = value.trim();
            if person_like(value)
                && !found
                    .iter()
                    .any(|f| normalize(&f.value) == normalize(value))
            {
                found.push(FoundName {
                    value: value.to_owned(),
                    role,
                    source: IdentitySource::Metadata,
                    reason,
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
            keep(value, role, format!("מאפייני הקובץ · {label}"), &mut found);
        }
        let mut texts = vec![(extracted.margins.clone(), "כותרת עליונה/תחתונה".to_owned())];
        for (field, value) in &extracted.metadata {
            if matches!(
                field.as_str(),
                "title" | "subject" | "keywords" | "description"
            ) {
                texts.push((value.clone(), "מאפייני הקובץ · כותרת".to_owned()));
            }
        }
        for (text, reason) in &texts {
            for a in self.preview_filter(case_id, text)?.auto_hidden {
                if matches!(a.kind, AutoKind::Name | AutoKind::OtherCase) {
                    keep(&a.token, a.role, reason.clone(), &mut found);
                }
            }
        }
        let practitioner: Vec<String> = self
            .vault_ref()?
            .practitioner()?
            .names
            .iter()
            .map(|n| normalize(n))
            .collect();
        found.retain(|f| !practitioner.contains(&normalize(&f.value)));
        let mut auto_hidden: Vec<AutoHidden> = self
            .vault_mut()?
            .add_found_names(case_id, &found)?
            .into_iter()
            .map(|i| AutoHidden {
                token: i.value,
                tag: i.tag,
                role: i.role,
                reason: i.reason,
                uncertain: false,
                kind: AutoKind::Name,
            })
            .collect();
        let body = self.learn(case_id, &extracted.body)?;
        for a in &body.auto_hidden {
            if !auto_hidden.iter().any(|x| x.tag == a.tag) {
                auto_hidden.push(a.clone());
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
            format: extracted.format.name().to_owned(),
            pages: extracted.pages,
            title: stem,
            suggested_kind: InputKind::guess(&extracted.body, file_name),
            preview: body.original_segments,
            suspects: Vec::new(),
            auto_hidden,
            hidden: body.hidden,
            body: extracted.body,
            left_out: extracted
                .margins
                .lines()
                .map(str::to_owned)
                .filter(|l| !l.trim().is_empty())
                .collect(),
            name_suggestions: Vec::new(),
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

    /// Filter `text` for the case and keep the new names it found, then filter it again so
    /// they carry their kept tags (the outcome lists them, for the card).
    fn learn(&mut self, case_id: &str, text: &str) -> Result<FilterOutcome, CoreError> {
        let first = self.preview_filter(case_id, text)?;
        let data = self.privacy_data(case_id)?;
        let found = found_names(&data, &first.auto_hidden);
        if found.is_empty() {
            return Ok(first);
        }
        self.vault_mut()?.add_found_names(case_id, &found)?;
        self.preview_filter(case_id, text)
    }

    /// "להחזיר" on the summary card: from now on this case keeps `token` as it is written.
    /// A name the filter kept with the case (under `tag`) is dropped from its names.
    pub fn restore_auto_hidden(
        &mut self,
        case_id: &str,
        token: &str,
        tag: &str,
    ) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        let ids = v.identities(case_id)?;
        let value = ids
            .iter()
            .find(|i| i.tag == tag && i.source != IdentitySource::Manual)
            .map(|i| normalize(&i.value));
        // Every row the filter kept for that name (a role change keeps the earlier row, so
        // drafts with its tag can still be restored).
        for i in ids.iter().filter(|i| {
            i.source != IdentitySource::Manual
                && (i.tag == tag || Some(normalize(&i.value)) == value)
        }) {
            v.remove_identity(case_id, &i.id)?;
        }
        if let Some(value) = &value {
            v.mark_not_a_name(Some(case_id), value)?;
        }
        v.mark_not_a_name(Some(case_id), &normalize(token))?;
        Ok(())
    }

    /// The card's role menu: who a name the filter kept is ("סבתא", "המורה"). The name gets a
    /// tag for that role; "להחזיר" still works on it.
    pub fn change_role(
        &mut self,
        case_id: &str,
        tag: &str,
        role: Role,
    ) -> Result<Vec<dv_domain::Identity>, CoreError> {
        let v = self.vault_mut()?;
        let ids: Vec<IdentityInput> = v
            .identities(case_id)?
            .into_iter()
            .map(|i| IdentityInput {
                role: if i.tag == tag { role } else { i.role },
                id: Some(i.id),
                value: i.value,
                aliases: i.aliases,
            })
            .collect();
        Ok(v.set_identities(case_id, &ids)?)
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
        let sheets = score_sheets(v, case_id, &inputs)?;
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
                        // Every score in the text against the score table, each time (AI-6).
                        warnings: dv_domain::check_scores(&d.text_tagged, &sheets),
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
        // All the text deleted: the paragraph goes away (D-036).
        if text.trim().is_empty() {
            return self.reject_paragraph(case_id, draft_id);
        }
        let tagged = self.learn(case_id, text)?.tagged;
        let before = style::find_draft(self.vault_ref()?, case_id, draft_id)?;
        self.vault_mut()?.edit_draft(case_id, draft_id, &tagged)?;
        self.learn_from_draft_edit(before, &tagged);
        Ok(())
    }

    pub fn add_own_paragraph(
        &mut self,
        case_id: &str,
        section_key: &str,
        text: &str,
    ) -> Result<(), CoreError> {
        self.add_own_paragraph_at(case_id, section_key, text, None)
    }

    /// Her own paragraph, stored tagged and approved. `at`: `None` = at the end of the section,
    /// `Some(None)` = first, `Some(Some(id))` = right after that paragraph (D-036).
    pub fn add_own_paragraph_at(
        &mut self,
        case_id: &str,
        section_key: &str,
        text: &str,
        at: Option<Option<&str>>,
    ) -> Result<(), CoreError> {
        let tagged = self.learn(case_id, text)?.tagged;
        let v = self.vault_mut()?;
        let d = v.add_draft(case_id, section_key, &tagged, Author::User, &[])?;
        v.set_draft_status(case_id, &d.id, DraftStatus::Approved)?;
        if let Some(after) = at {
            v.place_draft(case_id, &d.id, after)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ AI: prepare → approve → send

    /// Clear the request, or keep the names the filter found first. `Ok(true)`: new names
    /// were kept with the case and the caller builds the request again, so every text of it
    /// carries their tags (a name found in two texts gets one tag, and Claude's answer can
    /// be restored). `may_learn` is false on that second build: anything new then blocks.
    fn gate(
        &mut self,
        data: &PrivacyData,
        body: &Value,
        kind: PendingKind,
        prepared: &mut Prepared,
        may_learn: bool,
    ) -> Result<bool, CoreError> {
        let found = found_names(data, &prepared.auto_hidden);
        if may_learn && !data.case_id.is_empty() && !found.is_empty() {
            self.vault_mut()?.add_found_names(&data.case_id, &found)?;
            return Ok(true);
        }
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
        let mut case_tags: HashSet<String> = data
            .identities
            .iter()
            .filter(|i| i.case_id == data.case_id)
            .map(|i| i.tag.clone())
            .collect();
        // A consultation without a case has nowhere to keep names: the tags the filter gave
        // them are this request's own.
        if data.case_id.is_empty() {
            case_tags.extend(
                prepared
                    .auto_hidden
                    .iter()
                    .filter(|a| matches!(a.kind, AutoKind::Name | AutoKind::OtherCase))
                    .map(|a| a.tag.clone()),
            );
        }
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
        Ok(false)
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
        self.prepare_section_once(case_id, section_key, instruction, replaces, false)
    }

    fn prepare_section_once(
        &mut self,
        case_id: &str,
        section_key: &str,
        instruction: &str,
        replaces: Option<&str>,
        learned: bool,
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
            // Nothing approved yet: there is nothing to write it from, so nothing is sent.
            if derived && approved_context.is_empty() {
                return Err(CoreError::Refused(
                    "הסעיף הזה נכתב מתוך הסעיפים שכבר אישרת, ועוד לא אישרת אף סעיף. מאשרים קודם את הטיוטות בסעיפים האחרים, ואז חוזרים לכאן."
                        .to_owned(),
                ));
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
            // The conversation so far is not shown again, but what the filter hides in it is
            // kept like everything else, so its tags belong to the case.
            let mut history: Vec<TaggedTurn> = Vec::new();
            for m in v.messages(case_id, section_key)? {
                let o = run(&m.text_tagged)?;
                review.note_hidden(&o);
                history.push(TaggedTurn {
                    role: if m.role == ChatRole::User {
                        "user".to_owned()
                    } else {
                        "assistant".to_owned()
                    },
                    text_tagged: o.tagged,
                });
            }
            let instr = run(instruction)?;
            review.add("הבקשה שלך".to_owned(), &instr);
            // Her approved style profile (D-043), as it goes out with this case's names hidden.
            let style_profile = style::style_for_request(v, &ctx, section_key)?;
            if let Some(text) = &style_profile {
                review.add_context("פרופיל הסגנון שלך".to_owned(), &run(text)?);
            }
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
                style_profile,
            };
            let approved = input
                .approved_context
                .iter()
                .map(|(_, t)| t.clone())
                .collect::<Vec<_>>();
            let sources = (
                section.key.clone(),
                instr.tagged,
                instr.hidden.clone(),
                source_rows,
                approved,
            );
            (input, review, sources)
        };

        let nonce = dv_ai::nonce_from(&dv_vault::crypto::random_array::<16>()?);
        let (body, _refs) = dv_ai::build_section_request(&model, &input, &nonce);
        let mut prepared = review.into_case_prepared(demo_mode, &data);
        let (key, instruction_tagged, instr_hidden, rows, approved) = sources;
        let kind = PendingKind::Section {
            case_id: case_id.to_owned(),
            section_key: key,
            instruction_tagged,
            hidden: instr_hidden,
            sources: rows,
            approved,
            replaces: replaces.map(str::to_owned),
        };
        if self.gate(&data, &body, kind, &mut prepared, !learned)? {
            return self.prepare_section_once(case_id, section_key, instruction, replaces, true);
        }
        Ok(prepared)
    }

    /// Take an approved request out of the core so it can be sent without holding the
    /// session (the app stays responsive while Claude answers).
    pub fn begin_send(&mut self, approval_id: &str) -> Result<Outgoing, CoreError> {
        let api_key = self.vault_ref()?.secret(API_KEY)?;
        // Demo mode costs nothing; anything else stops at the monthly ceiling she set.
        if api_key.is_some() || self.transport.is_some() {
            self.refuse_over_cap()?;
        }
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

    fn refuse_over_cap(&mut self) -> Result<(), CoreError> {
        let v = self.vault_ref()?;
        let Some(cap) = v
            .setting(usage::CAP_KEY)?
            .and_then(|s| s.parse::<u32>().ok())
        else {
            return Ok(());
        };
        let month = usage::Month::parse(v.setting(&usage::this_month_key())?.as_deref());
        if month.reached(cap) {
            return Err(CoreError::Refused(format!(
                "הגעת לתקרת ההוצאה החודשית שקבעת (${cap}). אפשר להעלות אותה בהגדרות, תחת \"שימוש ועלות\"."
            )));
        }
        Ok(())
    }

    /// Count the tokens of an answer toward this month (numbers only). Never fails a send:
    /// the answer has arrived and is worth more than the count.
    fn note_usage(&mut self, payload: &ClearedPayload, answer: &Value, demo: bool) {
        if demo {
            return;
        }
        let model = serde_json::from_slice::<Value>(payload.body())
            .ok()
            .and_then(|b| b["model"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let key = usage::this_month_key();
        let Ok(v) = self.vault_mut() else { return };
        let mut month = usage::Month::parse(v.setting(&key).ok().flatten().as_deref());
        month.add(&model, usage::Tokens::from_answer(answer));
        if let Ok(json) = serde_json::to_string(&month) {
            let _ = v.set_setting(&key, &json);
        }
    }

    /// This month's use of the AI and the ceiling, for settings.
    pub fn usage_summary(&mut self) -> Result<UsageSummary, CoreError> {
        let v = self.vault_mut()?;
        let key = usage::this_month_key();
        let cap = v
            .setting(usage::CAP_KEY)?
            .and_then(|s| s.parse::<u32>().ok());
        Ok(usage::Month::parse(v.setting(&key)?.as_deref()).view(&key, cap))
    }

    /// The monthly ceiling in dollars; `None` removes it.
    pub fn set_monthly_cap(&mut self, cap_usd: Option<u32>) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        match cap_usd {
            Some(0) => {
                return Err(CoreError::Refused(
                    "תקרה של 0 תעצור כל שליחה. כדי לבטל את התקרה, משאירים את השדה ריק.".to_owned(),
                ))
            }
            Some(cap) => v.set_setting(usage::CAP_KEY, &cap.to_string())?,
            // Empty = no ceiling (it does not parse as a number).
            None => v.set_setting(usage::CAP_KEY, "")?,
        }
        Ok(())
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
        self.note_usage(&out.pending.payload, &response, demo);
        let Pending { payload, kind } = out.pending;
        let PendingKind::Section {
            case_id,
            section_key,
            instruction_tagged,
            hidden,
            sources,
            approved,
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
        let mut reply = if DERIVED_SECTIONS.contains(&section_key.as_str()) {
            dv_ai::parse_derived_section(&response, &approved)?
        } else {
            dv_ai::parse_section(&response, &refs)?
        };
        let v = self.vault_mut()?;
        let identities = v.identities(&case_id)?;
        let case_tags: Vec<String> = identities.iter().map(|i| i.tag.clone()).collect();
        let everyone = v.all_identities()?;
        let inputs = v.inputs(&case_id)?;
        let sheets = score_sheets(v, &case_id, &inputs)?;
        for p in &mut reply.paragraphs {
            p.warnings.extend(dv_domain::check_scores(&p.text, &sheets));
            for s in scan_model_output(&p.text, &case_tags, &everyone) {
                p.warnings.push(format!("{}: {}", s.message, s.token));
            }
        }
        // D-043: a paragraph that repeats a past report word for word.
        let texts: Vec<&str> = reply.paragraphs.iter().map(|p| p.text.as_str()).collect();
        let overlaps = style::overlap_warning(v, &texts)?;
        for (p, w) in reply.paragraphs.iter_mut().zip(overlaps) {
            p.warnings.extend(w);
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

    /// Prepare every section that has material and nothing written yet (the "write the empty
    /// sections" button). A section with a draft or approved paragraphs is left as it is:
    /// writing it again would put a second draft beside what she already approved.
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
            let written = self
                .vault_ref()?
                .drafts(case_id, &s.key)?
                .iter()
                .any(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved));
            if fed.contains(&s.key) && !written {
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
        self.prepare_consult_once(case_id, conversation_id, message, false)
    }

    fn prepare_consult_once(
        &mut self,
        case_id: Option<&str>,
        conversation_id: Option<&str>,
        message: &str,
        learned: bool,
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
        let mut prepared = review.into_case_prepared(demo_mode, &data);
        let kind = PendingKind::Consult {
            case_id: case_id.map(str::to_owned),
            conversation_id: conversation_id.map(str::to_owned),
            message_tagged: input.message_tagged,
            message_shown: message.to_owned(),
            hidden: prepared.hidden.clone(),
        };
        if self.gate(&data, &body, kind, &mut prepared, !learned)? {
            return self.prepare_consult_once(case_id, conversation_id, message, true);
        }
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
        self.note_usage(&out.pending.payload, &response, demo);
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
                charts: Vec::new(),
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
        let file_name = report_file_name(&code, &identities, dates::today());
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
