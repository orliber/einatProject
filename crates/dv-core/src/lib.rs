//! Application services used by the desktop shell.
//!
//! The Tauri commands are thin wrappers around [`Core`], so all behavior is testable on any
//! machine without a WebView. `Core` owns the unlocked vault and walks every request through
//! the same path: filter → build → review → gate → (approval) → send → check → store.

mod activity;
mod backup;
mod cases;
mod consult;
mod consultations;
mod dates;
mod drafts;
mod export;
mod followup;
pub mod google;
mod intake;
mod library;
mod readiness;
mod retention;
mod seal;
mod security;
mod send;
mod session;
mod settings;
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

use dv_ai::{ModelConfig, Provider, SectionInput, TaggedInput, TaggedTurn, ALLOWED_MODELS};
use dv_domain::{
    passage_ranges, Author, CaseMeta, CaseSummary, ChatRole, DraftStatus, FoundName, IdentityInput,
    IdentitySource, InputKind, ReportStructure, Role,
};
use dv_egress::{EgressError, Transport};
use dv_ipc::{PingResponse, IPC_VERSION};
use dv_privacy::restore::{restore, scan_model_output};
use dv_privacy::text::normalize;
use dv_privacy::{
    clear, filter, ocr_misreads, AutoHidden, AutoKind, Checks, ClearedPayload, FilterOutcome,
    GateRequest, PrivacyContext,
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
pub use security::{SecurityItem, SecurityStatus};
pub use style::{
    StyleAnalysisResult, StyleImportPreview, StyleOverview, StylePartView, StyleProfileView,
    StyleSectionLabel, StyleSourceView, StyleSuggestion, StyleSuggestionKind, StyleVersionView,
};
pub use unsaved::UnsavedEdit;
pub use usage::UsageSummary;
pub use views::{
    ActivityEntry, ActivityPage, AppStatus, BackupCheckView, BackupDone, BackupStatus, CaseDetail,
    ChatView, ConsultResult, ConsultTurnView, ConsultationSummary, ConsultationView, CreatedVault,
    ExportCheck, ImportPreview, MaterialRouting, NameMatch, NameSuggestion, ParagraphVersionView,
    ParagraphView, Prepared, ReportSettings, RetentionItem, ReviewPart, SectionResult, SectionView,
    SortResult, SourceExcerpt, StagedBackup, SuspectDecision, TemplateView, UiError,
};

const API_KEY: &str = "anthropic_api_key";

/// Where each company's API key is kept: the vault's encrypted secrets, like Claude's (D-040).
/// The local model has none.
fn key_name(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::Anthropic => Some(API_KEY),
        Provider::OpenAi => Some("openai_api_key"),
        Provider::Gemini => Some("gemini_api_key"),
        Provider::Mistral => Some("mistral_api_key"),
        Provider::Local => None,
    }
}

/// The key to send with (`None`: no key saved, demo mode). The local model needs none.
pub(crate) fn key_of(
    v: &Vault,
    provider: Provider,
) -> Result<Option<zeroize::Zeroizing<String>>, VaultError> {
    match key_name(provider) {
        Some(name) => v.secret(name),
        None => Ok(Some(zeroize::Zeroizing::new(String::new()))),
    }
}

/// The company of a model; anything unknown reads as Claude (the default).
fn provider_of(model: &str) -> Provider {
    Provider::of_model(model).unwrap_or(Provider::Anthropic)
}
const MODEL_KEY: &str = "model";
/// Settings key prefix of her written zero-retention confirmation per company: `zdr/openai`.
const ZDR_PREFIX: &str = "zdr/";
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
/// Her Word template (EX-1), hex-encoded inside the encrypted vault.
const TEMPLATE_KEY: &str = "report_template";
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
    /// What the psychologist sees, naming Claude.
    #[must_use]
    pub fn to_ui(&self) -> UiError {
        self.to_ui_for(Provider::Anthropic.display_name())
    }

    /// What the psychologist sees, naming the AI she chose (`Core::ai_name`).
    #[must_use]
    pub fn to_ui_for(&self, ai: &str) -> UiError {
        let mut details = Vec::new();
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
            CoreError::Vault(e) => {
                details.push(e.to_string());
                ("vault", TECHNICAL_HE.to_owned())
            }
            CoreError::NotFound(what) => ("not_found", format!("לא נמצא: {what}")),
            CoreError::ConsentMissing => (
                "consent_missing",
                format!("לפני שליחה ל-{ai} צריך לרשום בתיק את הסכמת ההורים."),
            ),
            CoreError::Refused(why) => ("refused", why.clone()),
            CoreError::Backoff(s) => (
                "backoff",
                format!("יותר מדי ניסיונות. אפשר לנסות שוב בעוד {s} שניות."),
            ),
            CoreError::Egress(e) => ("egress", egress_he(e, ai)),
            CoreError::Ai(e) => ("ai", format!("התשובה של {ai} לא תקינה: {e}")),
            CoreError::Update(e) => ("update", update::update_he(e)),
            CoreError::Internal(e) => {
                details.push(e.clone());
                ("internal", TECHNICAL_HE.to_owned())
            }
        };
        UiError {
            code: code.to_owned(),
            message,
            details,
        }
    }
}

/// One calm sentence for failures only the developer can read; the raw text rides in
/// `UiError::details` behind a "copy details" button (F5).
const TECHNICAL_HE: &str =
    "משהו השתבש בתוכנה. שום דבר לא נשלח והחומרים שמורים. אפשר להעתיק את הפרטים ולשלוח לאור.";

fn egress_he(e: &EgressError, ai: &str) -> String {
    match e {
        EgressError::NoApiKey => {
            "לא הוגדר מפתח API. עד שיוגדר, התוכנה עובדת במצב הדגמה.".to_owned()
        }
        EgressError::Unauthorized => "מפתח ה-API נדחה. כדאי לבדוק אותו בהגדרות.".to_owned(),
        EgressError::RateLimited => "יותר מדי בקשות. אפשר לנסות שוב בעוד דקה.".to_owned(),
        EgressError::Offline => {
            format!("אין חיבור לאינטרנט. אפשר להמשיך לעבוד, ו-{ai} יחזור כשיהיה חיבור.")
        }
        EgressError::LocalUnavailable => "המודל המקומי לא עונה. צריך שהתוכנה Ollama תפעל במחשב, ושהמודל שנבחר יהיה מותקן בה (בהגדרות כתוב איך)."
            .to_owned(),
        EgressError::Tls => {
            "החיבור המאובטח נכשל. ייתכן שתוכנה במחשב (למשל אנטי-וירוס) מיירטת תעבורה מוצפנת."
                .to_owned()
        }
        EgressError::Stopped => "השליחה כבויה (עצירת חירום). אפשר להפעיל אותה מחדש במסך מצב האבטחה."
            .to_owned(),
        EgressError::TooLarge => "הבקשה גדולה מדי. כדאי לשלוח פחות חומרים בבת אחת.".to_owned(),
        EgressError::DailyLimit => "הגעת למכסת הבקשות היומית. אפשר להמשיך מחר.".to_owned(),
        other => format!("שגיאה בחיבור ל-{ai}: {other}"),
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
    /// The model named in the approved body: it decides the company and the key.
    model: String,
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
    /// Send the approved payload: the test transport, the chosen AI (its API key set), or
    /// local demo.
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
                dv_egress::transport_for(&self.model, key)?
                    .send_streaming(&self.pending.payload, progress)?,
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
    /// Windows Hello is set up on this computer; asked once (D-047).
    hello_available: Option<bool>,
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
    /// Where the rollback seal is kept (outside the vault folder); `None` turns it off.
    seal_dir: Option<PathBuf>,
    /// Set at unlock when the vault is older than this computer has seen (S6).
    rollback_warning: Option<String>,
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
    /// Keyed hashes of the names in her past reports (`style`).
    past: HashSet<String>,
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

/// The card's tag for a word kept as a name in another case: never a tag of this case, so
/// "להחזיר" on it can only mean that other case.
const OTHER_CASE_TAG: &str = "[תיק_אחר]";

/// Names the gate refused in the outgoing text that the program kept by itself (not typed by
/// her): they get "להחזיר" on the summary card like any other, in this case or in another
/// one ("אח" kept by mistake in another case blocked every case). Without this she saw only
/// a red line and "חזרה לעריכה". `code_of` names a case for the card.
fn refused_auto_names(
    data: &PrivacyData,
    reasons: &[dv_privacy::BlockReason],
    code_of: &dyn Fn(&str) -> String,
) -> Vec<AutoHidden> {
    let mut out: Vec<AutoHidden> = Vec::new();
    for r in reasons.iter().filter(|r| r.code == "identity") {
        let Some(found) = r.detail.as_deref().map(normalize) else {
            continue;
        };
        for i in data.identities.iter().filter(|i| {
            i.source != IdentitySource::Manual
                && std::iter::once(&i.value)
                    .chain(i.aliases.iter())
                    .any(|v| normalize(v) == found || normalize(v).split(' ').any(|w| w == found))
        }) {
            let here = i.case_id == data.case_id;
            let tag = if here {
                i.tag.clone()
            } else {
                OTHER_CASE_TAG.to_owned()
            };
            if out
                .iter()
                .any(|a| a.tag == tag && normalize(&a.token) == normalize(&i.value))
            {
                continue;
            }
            out.push(AutoHidden {
                token: i.value.clone(),
                tag,
                role: i.role,
                reason: if here {
                    "נשמר בתיק כשם, ועצר את השליחה".to_owned()
                } else {
                    format!(
                        "נשמר אוטומטית כשם בתיק «{}», ועצר את השליחה",
                        code_of(&i.case_id)
                    )
                },
                uncertain: true,
                kind: if here {
                    AutoKind::Name
                } else {
                    AutoKind::OtherCase
                },
            });
        }
    }
    out
}

/// A refused name she typed in another case is a real name there: it is not offered for
/// "להחזיר", but the red line says where it is, so she can look.
fn locate_refused_names(
    data: &PrivacyData,
    reasons: &mut [dv_privacy::BlockReason],
    code_of: &dyn Fn(&str) -> String,
) {
    for r in reasons.iter_mut().filter(|r| r.code == "identity") {
        let Some(found) = r.detail.as_deref().map(normalize) else {
            continue;
        };
        let cases: Vec<String> = data
            .identities
            .iter()
            .filter(|i| {
                i.case_id != data.case_id
                    && i.source == IdentitySource::Manual
                    && std::iter::once(&i.value).chain(i.aliases.iter()).any(|v| {
                        normalize(v) == found || normalize(v).split(' ').any(|w| w == found)
                    })
            })
            .map(|i| code_of(&i.case_id))
            .collect();
        if !cases.is_empty() {
            r.message = format!("{} (רשום כשם בתיק «{}»)", r.message, cases.join("», «"));
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

/// The report's font from the computer's own fonts (regular, and bold when there is one),
/// else Arial, else a common font with Hebrew letters. Read only, never copied anywhere but
/// into the PDF.
fn system_fonts(name: &str) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let windows = std::env::var_os("WINDIR")
        .map(|w| std::path::PathBuf::from(w).join("Fonts"))
        .unwrap_or_else(|| std::path::PathBuf::from("C:\\Windows\\Fonts"));
    let files: &[(&str, &str)] = match name {
        "David" => &[("david.ttf", "davidbd.ttf")],
        "Narkisim" => &[("nrkis.ttf", "")],
        "Frank Ruehl" => &[("frank.ttf", "")],
        "Times New Roman" => &[("times.ttf", "timesbd.ttf")],
        _ => &[],
    };
    let dirs = [
        windows,
        std::path::PathBuf::from("/System/Library/Fonts/Supplemental"),
        std::path::PathBuf::from("/Library/Fonts"),
        std::path::PathBuf::from("/usr/share/fonts/truetype/dejavu"),
    ];
    let fallbacks: &[(&str, &str)] = &[
        ("arial.ttf", "arialbd.ttf"),
        ("Arial.ttf", "Arial Bold.ttf"),
        ("DejaVuSans.ttf", "DejaVuSans-Bold.ttf"),
    ];
    files.iter().chain(fallbacks).find_map(|(regular, bold)| {
        dirs.iter().find_map(|d| {
            let r = std::fs::read(d.join(regular)).ok()?;
            let b = (!bold.is_empty())
                .then(|| std::fs::read(d.join(bold)).ok())
                .flatten();
            Some((r, b))
        })
    })
}

/// The two letters (EX-4). Their drafts are kept in the case under their own keys, so they
/// are deleted with the case, and never enter the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Letter {
    Parents,
    School,
}

impl Letter {
    fn of(section_key: &str) -> Option<Self> {
        match section_key {
            "letter_parents" => Some(Self::Parents),
            "letter_school" => Some(Self::School),
            _ => None,
        }
    }

    fn from_audience(audience: &str) -> Result<Self, CoreError> {
        match audience {
            "parents" => Ok(Self::Parents),
            "school" => Ok(Self::School),
            other => Err(CoreError::NotFound(other.to_owned())),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Parents => "letter_parents",
            Self::School => "letter_school",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Parents => dv_ai::prompts::LETTER_TITLE_PARENTS,
            Self::School => dv_ai::prompts::LETTER_TITLE_SCHOOL,
        }
    }

    fn instruction(self) -> &'static str {
        match self {
            Self::Parents => dv_ai::prompts::LETTER_PARENTS,
            Self::School => dv_ai::prompts::LETTER_SCHOOL,
        }
    }

    fn file_title(self) -> &'static str {
        match self {
            Self::Parents => "מכתב להורים",
            Self::School => "מכתב לצוות החינוכי",
        }
    }

    /// The approved sections a letter is written from (data minimization: the school gets
    /// only what it acts on).
    fn written_from(self) -> &'static [&'static str] {
        match self {
            Self::Parents => &["summary", "diagnoses", "recommendations"],
            Self::School => &["recommendations"],
        }
    }
}

fn template_view(bytes: &[u8]) -> Result<TemplateView, CoreError> {
    let s = dv_export::check_template(bytes).map_err(|e| match e {
        dv_export::ExportError::Template(m) => CoreError::Refused(m),
        other => CoreError::Internal(other.to_string()),
    })?;
    Ok(TemplateView {
        has_marker: s.has_marker,
        headers: s.headers,
        footers: s.footers,
        images: s.images,
        styles_matched: s.styles_matched,
        styles_total: s.styles_total,
        size_kb: u32::try_from(bytes.len().div_ceil(1024)).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests;
