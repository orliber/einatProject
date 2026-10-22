//! Application services used by the desktop shell.
//!
//! The Tauri commands are thin wrappers around [`Core`], so all behavior is testable on any
//! machine without a WebView. `Core` owns the unlocked vault and walks every request through
//! the same path: filter → build → review → gate → (approval) → send → check → store.

mod activity;
mod backup;
mod binding;
mod cases;
mod consult;
mod consultations;
mod dates;
mod drafts;
mod export;
mod failures;
mod followup;
pub use binding::{ComputerSeal, WindowsSeal};
pub mod google;
mod guides;
mod history;
mod instruments;
mod intake;
mod library;
pub mod name_model;
mod own;
use name_model::NameHints;
mod jobs;
mod layers;
mod readiness;
mod report_check;
mod retention;
mod seal;
mod security;
mod send;
mod session;
mod settings;
mod sorting;
mod stage;
mod style;
mod unsaved;
pub mod update;
mod usage;
mod verify;
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
use dv_privacy::restore::{restore, scan_model_output};
use dv_privacy::text::normalize;
use dv_privacy::{
    clear, filter, filter_with_hints, ocr_misreads, AutoHidden, AutoKind, Checks, ClearedPayload,
    FilterOutcome, GateRequest, NameHint, PrivacyContext,
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
pub use history::{HiddenWord, SentItem, SentPart, SentText};
pub use instruments::{InstrumentApproval, InstrumentStatus};
pub use jobs::{
    BackupRun, BackupTask, DocumentRead, DrillJob, Drilled, ExportJob, Exported, ReadJob,
    RestoreJob, Restored, SlotJob, SlotMade, UnlockJob, Unlocked,
};
pub use layers::SecurityLayer;
pub use library::TRASH_DAYS;
pub use name_model::{HintJob, HintsRead};
pub use own::SectionModeView;
pub use readiness::{Readiness, ReadinessItem};
pub use report_check::{ReportCheckNote, ReportCheckResult, ReportCheckSetting};
pub use retention::{KEEP_UNTIL_AGE, KEEP_YEARS_AFTER_LAST_CHANGE};
pub use security::{SecurityItem, SecurityStatus};
pub use style::{
    FitMonth, InterpretationResult, InterpretationView, LibraryLine, OwnSectionHelp,
    SectionLibrary, SimilarParagraph, SimilarReport, SkeletonView, StyleAnalysisResult,
    StyleImportPreview, StyleOverview, StylePartView, StyleProfileView, StyleSectionLabel,
    StyleSourceView, StyleSuggestion, StyleSuggestionKind, StyleVersionView,
};
pub use unsaved::UnsavedEdit;
pub use usage::UsageSummary;
pub use verify::VerifyResult;
pub use views::{
    ActivityEntry, ActivityPage, AppStatus, BackupCheckView, BackupDone, BackupStatus, CaseDetail,
    ChatView, ConsultResult, ConsultTurnView, ConsultationSummary, ConsultationView, CreatedVault,
    ExportCheck, ImportPreview, MaterialRouting, NameMatch, NameSuggestion, ParagraphVersionView,
    ParagraphView, Prepared, ReportSettings, RetentionItem, ReviewPart, SectionResult, SectionView,
    SortResult, SourceExcerpt, StagedBackup, StyleHint, StyleMark, SuspectDecision, TemplateView,
    UiError,
};
pub use views::{ErrorAction, PingResponse, IPC_VERSION};

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

/// The report template of a case (D-066): the institute's for a new case, the one it was
/// opened with for an older one.
pub(crate) fn structure_for(v: &Vault, case_id: &str) -> Result<ReportStructure, CoreError> {
    let mut structure = ReportStructure::for_template(v.case_meta(case_id)?.template.as_deref())
        .map_err(|e| CoreError::Internal(e.to_string()))?;
    // Who writes each section, as she set it in settings (round 30).
    for part in &mut structure.parts {
        for s in &mut part.sections {
            if let Some(mode) = own::mode_setting(v, &s.key)? {
                s.mode = mode;
            }
        }
    }
    Ok(structure)
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
/// When the printed recovery kit was last checked (D-076).
const KIT_CHECKED_KEY: &str = "recovery_kit_checked_at";
/// "Show me every request before it is sent" (round 26). On until she turns it off: the
/// review screen stays the default until Einat confirms turning it off (D-062). The key of the
/// earlier "only when suspicious" choice is not read.
const REVIEW_KEY: &str = "review_always";
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

fn day_number() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| {
        d.as_secs() / dv_domain::dates::DAY_SECS.unsigned_abs()
    })
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

/// How many crash lines a bug report carries.
const CRASHES_SHOWN: usize = 5;

/// The last crashes from the desktop's crash log, for the bug report she copies for the
/// developer. Only lines in the hook's own shape are kept
/// (`<seconds> · v<version> · thread <name> · <file>:<line> · <fixed text>`); anything else in
/// the file is dropped, so nothing but what the hook wrote can reach the report.
#[must_use]
pub fn recent_crashes(log: &str) -> Vec<String> {
    fn well_formed(line: &str) -> bool {
        let parts: Vec<&str> = line.split(" · ").collect();
        let [at, version, thread, place, text] = parts.as_slice() else {
            return false;
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let place_ok = place == &"?"
            || place
                .rsplit_once(':')
                .is_some_and(|(file, n)| digits(n) && file.is_ascii() && !file.contains(' '));
        line.chars().count() <= 300
            && digits(at)
            && version.strip_prefix('v').is_some_and(|v| {
                v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            })
            && thread.strip_prefix("thread ").is_some_and(|t| t.is_ascii())
            && place_ok
            && !text.is_empty()
            && !line.chars().any(char::is_control)
    }
    let kept: Vec<String> = log
        .lines()
        .map(str::trim_end)
        .filter(|l| well_formed(l))
        .map(str::to_owned)
        .collect();
    let skip = kept.len().saturating_sub(CRASHES_SHOWN);
    kept.into_iter().skip(skip).collect()
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
    /// A refusal with its own next step (F6): the place that fixes it, or a retry.
    #[error("refused: {0}")]
    Needs(String, ErrorAction),
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
            CoreError::Vault(VaultError::OtherComputer) => (
                "other_computer",
                "הסיסמה פותחת את הכספת רק במחשב ובחשבון ה-Windows שבהם היא נקבעה. בעותק של תיקיית הכספת, וגם באותו מחשב אחרי התקנה מחדש של Windows או איפוס החשבון, נכנסים כאן עם ערכת השחזור, ואחר כך בוחרים סיסמה בהגדרות כדי שתעבוד גם כאן. אם המחשב הקודם אבד, אפשר גם לשחזר את הגיבוי עם הסיסמה.".to_owned(),
            ),
            CoreError::Vault(VaultError::Busy) => (
                "busy",
                "Windows לא ענה בזמן לבדיקה שהסיסמה שייכת למחשב הזה. זה קורה לפעמים מיד אחרי הדלקת המחשב. אפשר לנסות שוב בעוד רגע.".to_owned(),
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
            CoreError::Refused(why) | CoreError::Needs(why, _) => ("refused", why.clone()),
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
            action: self.action(),
        }
    }

    /// The one next step this error offers (F6).
    #[must_use]
    pub fn action(&self) -> ErrorAction {
        match self {
            CoreError::Locked => ErrorAction::Unlock,
            CoreError::Vault(
                VaultError::WrongSecret
                | VaultError::OtherComputer
                | VaultError::Busy
                | VaultError::RecoveryKeyInvalid
                | VaultError::Policy(_)
                | VaultError::AlreadyExists,
            )
            | CoreError::NotFound(_)
            | CoreError::Refused(_) => ErrorAction::Dismiss,
            CoreError::Vault(_) | CoreError::Internal(_) => ErrorAction::Report,
            CoreError::ConsentMissing => ErrorAction::Consent,
            CoreError::Needs(_, action) => action.clone(),
            CoreError::Backoff(s) => ErrorAction::Retry {
                after_seconds: u32::try_from(*s).unwrap_or(u32::MAX),
            },
            CoreError::Egress(e) => egress_action(e),
            // The answer came and was not usable; its approval is spent, so sending "again"
            // would do nothing. She asks again from the screen (a new request).
            CoreError::Ai(_) => ErrorAction::Dismiss,
            CoreError::Update(_) => ErrorAction::retry(),
        }
    }
}

/// Where a failed connection is fixed: her key and her choice of AI in Settings, the emergency
/// stop on the security page, a retry only for what passes by itself, and a report for Or for
/// what no retry can fix (a refused or broken request, an intercepted connection). Every
/// variant is named, so a new one needs its own answer.
fn egress_action(e: &EgressError) -> ErrorAction {
    match e {
        EgressError::NoApiKey | EgressError::Unauthorized | EgressError::LocalUnavailable => {
            ErrorAction::settings("ai")
        }
        EgressError::Stopped => ErrorAction::settings("security"),
        // The service's certificate is from an authority this version is not pinned to.
        EgressError::Pinned => ErrorAction::settings("about"),
        EgressError::RateLimited => ErrorAction::Retry { after_seconds: 60 },
        EgressError::Offline | EgressError::Unavailable(_) => ErrorAction::retry(),
        // A timeout or a conflict passes by itself; any other refusal comes back the same.
        EgressError::Rejected(408 | 409, _) => ErrorAction::retry(),
        EgressError::Rejected(..)
        | EgressError::Policy(_)
        | EgressError::Tls
        | EgressError::Protocol(_) => ErrorAction::Report,
        EgressError::TooLarge | EgressError::DailyLimit => ErrorAction::Dismiss,
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
        EgressError::Pinned => format!(
            "החיבור ל-{ai} נעצר ושום דבר לא נשלח: התעודה שלו היא מגורם שהגרסה הזו של התוכנה לא סומכת עליו. צריך לעדכן את התוכנה (בהגדרות, באודות)."
        ),
        EgressError::Stopped => "השליחה כבויה (עצירת חירום). אפשר להפעיל אותה מחדש במסך מצב האבטחה."
            .to_owned(),
        EgressError::TooLarge => "הבקשה גדולה מדי. כדאי לשלוח פחות חומרים בבת אחת.".to_owned(),
        EgressError::DailyLimit => "הגעת למכסת הבקשות היומית. אפשר להמשיך מחר.".to_owned(),
        other => format!("שגיאה בחיבור ל-{ai}: {other}"),
    }
}

/// The score sheets entered in the case's score table (the scores the text is checked against),
/// but for instruments she does not give.
fn score_sheets(
    v: &Vault,
    case_id: &str,
    inputs: &[dv_domain::CaseInput],
) -> Result<Vec<dv_domain::ScoreSheet>, CoreError> {
    // An instrument she does not give is in no request and no check (D-098).
    let skip = instruments::not_used(v)?;
    let mut out = Vec::new();
    for i in inputs.iter().filter(|i| i.kind == InputKind::TestScores) {
        if let Some(data) = v.input_data(case_id, &i.id)? {
            if let Ok(sheet) = serde_json::from_str::<dv_domain::ScoreSheet>(&data) {
                if !skip.contains(&sheet.instrument) {
                    out.push(sheet);
                }
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
        /// A first draft: the guide's lines as they went out, numbered from 1, to find the
        /// ones no paragraph answers (round 29). Empty: not checked.
        guide: Vec<String>,
        /// Worded from her points only (round 30): no `S#` sources; `approved` holds the points.
        from_points: bool,
    },
    Consult {
        case_id: Option<String>,
        /// The saved conversation it continues (`None`: a new one).
        conversation_id: Option<String>,
        message_tagged: String,
        /// The question as typed (kept only in the vault, for display).
        message_shown: String,
        hidden: Vec<String>,
        /// The request's mask book, to give the answer back as she wrote the question.
        masks: Vec<dv_domain::MaskEntry>,
    },
    /// Sorting materials into sections (D-022): per material its id, how many passages were
    /// sent, and a fingerprint of its text then.
    Sort {
        case_id: String,
        /// Per material: id, passage count, fingerprint of the text, and the text as sent
        /// (tagged), which the numbers of its summary are checked against (round 31).
        materials: Vec<(String, usize, u64, String)>,
        sections: Vec<String>,
    },
    /// Step 1 of the writing-style profile: one past report (D-043).
    StyleAnalysis { source_id: String },
    /// Step 2: the analyses of `reports` past reports.
    StyleSynthesis { reports: u32 },
    /// The interpretation of scores in her past reports (round 33): the groups that were sent.
    Interpretation {
        groups: Vec<dv_ai::InterpretationGroup>,
    },
    /// The check of a section's proposed paragraphs against its sources: the drafts in the
    /// order of `P1`…, and the `S#` ids that were sent.
    Verify {
        case_id: String,
        section_key: String,
        draft_ids: Vec<String>,
        source_ids: Vec<String>,
    },
    /// The check of the whole approved report (D-097): the keys of the sections sent, in the
    /// order of `R1`…
    ReportCheck {
        case_id: String,
        section_keys: Vec<String>,
        /// The request's ids ("R1"…), one per key: the notes are read by the ids sent.
        ids: Vec<String>,
    },
}

/// What an approval is expected to be for. `Core::begin_send` checks it before anything
/// leaves, so an approval for one kind of request is never sent by the command for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendKind {
    Section,
    Consult,
    Sort,
    StyleAnalysis,
    StyleSynthesis,
    Interpretation,
    Verify,
    ReportCheck,
}

impl PendingKind {
    fn send_kind(&self) -> SendKind {
        match self {
            PendingKind::Section { .. } => SendKind::Section,
            PendingKind::Consult { .. } => SendKind::Consult,
            PendingKind::Sort { .. } => SendKind::Sort,
            PendingKind::StyleAnalysis { .. } => SendKind::StyleAnalysis,
            PendingKind::StyleSynthesis { .. } => SendKind::StyleSynthesis,
            PendingKind::Interpretation { .. } => SendKind::Interpretation,
            PendingKind::Verify { .. } => SendKind::Verify,
            PendingKind::ReportCheck { .. } => SendKind::ReportCheck,
        }
    }
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
    /// The session it was approved in (`Core::session`): a lock in between ends it.
    session: u64,
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
    /// When this run's wrong passwords or kits were typed: the ones just before she got in
    /// are her own typos, and the opening does not report them (review of stage 5, U5).
    failure_times: Vec<Instant>,
    not_before: Option<Instant>,
    /// Since when a password or kit is being checked off the lock (an unlock, a new password
    /// or kit). One at a time: a second click never runs a second Argon2 in parallel, never
    /// slips past the wait after wrong tries, and never writes the header at the same time.
    secret_work: Option<(u64, Instant)>,
    /// Numbers the checks, so only the check that took the place frees it.
    secret_ticket: u64,
    /// Set by the next lock, for the jobs begun before it (F2).
    cancel: jobs::Cancel,
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
    /// The text being typed right now (memory only; kept in the vault on lock).
    unsaved: Vec<UnsavedEdit>,
    /// The material deleted last, still sealed, for "ביטול" (memory only; dropped on lock).
    removed_input: Option<dv_vault::RemovedInput>,
    /// Where the rollback seal is kept (outside the vault folder); `None` turns it off.
    seal_dir: Option<PathBuf>,
    /// Set at unlock when the vault is older than this computer has seen (S6).
    rollback_warning: Option<String>,
    /// Counts the locks. An approval taken out before a lock never comes back after it.
    session: u64,
    /// Where the optional name model is downloaded (D-053); `None` keeps it off.
    models_dir: Option<PathBuf>,
    /// The name model, started on first use or right after unlock while it is on, and
    /// shared with the job that reads a case's texts while the core is free (ended on lock).
    ner: Option<Arc<name_model::NerCell>>,
    /// The app's binary, started as the name model's isolated worker (SEC2).
    ner_exe: Option<PathBuf>,
    /// One data-block mark per section conversation (case, section), so its materials are the
    /// same bytes on every turn and the API's cache can serve them (dropped on lock).
    section_nonces: HashMap<(String, String), String>,
    /// The name model's hints by text, so a material is read once (dropped on lock).
    hint_cache: HashMap<String, Vec<dv_privacy::NameHint>>,
    /// The case whose stored texts were last settled against its tags, and the vault's write
    /// counts right after: while they have not moved, no text or name changed, and settling
    /// again would find nothing (dropped on lock).
    settled: Option<(String, (u64, u64))>,
    /// The half of the password slot kept on this computer (D-076).
    computer_seal: Arc<dyn ComputerSeal>,
    /// The program's memory is kept out of the page file: `None` where the computer offers
    /// no such thing, else whether the room for it was granted at start (D-047, L7).
    memory_lock: Option<bool>,
    /// The cases opened this session, so each opening is logged once (L3; dropped on lock).
    opened_cases: HashSet<String>,
    /// What this opening has to say about the time since the last one (L4; dropped on lock).
    since_last: Option<String>,
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
    /// The case's mask book while this request is built (round 25): what it keeps, and the
    /// numbered tags this request adds, kept with the case once the gate clears it.
    masks: dv_privacy::MaskBook,
}

/// A [`PrivacyData`]'s word lookups against the open vault's keyed hash, kept alive while a
/// [`PrivacyContext`] borrows them. See [`PrivacyData::lookups`].
struct CaseLookups<'a, A, N, P> {
    data: &'a PrivacyData,
    allow: A,
    is_name: N,
    past: P,
}

impl PrivacyData {
    /// The lookups every case context uses: "not a name", "a name in a word", and her past
    /// reports' names, each by keyed hash of the normalized token.
    fn lookups<'a>(
        &'a self,
        v: &'a Vault,
    ) -> CaseLookups<
        'a,
        impl Fn(&str) -> bool + 'a,
        impl Fn(&str) -> bool + 'a,
        impl Fn(&str) -> bool + 'a,
    > {
        CaseLookups {
            data: self,
            allow: |t: &str| self.allow.contains(&v.token_hmac(t)),
            is_name: |t: &str| self.is_name.contains(&v.token_hmac(t)),
            past: |t: &str| !self.past.is_empty() && self.past.contains(&v.token_hmac(t)),
        }
    }
}

impl<A, N, P> CaseLookups<'_, A, N, P>
where
    A: Fn(&str) -> bool,
    N: Fn(&str) -> bool,
    P: Fn(&str) -> bool,
{
    /// The case's context with the fixed tag of each kind (`[תאריך]`), no mask book.
    fn context(&self) -> PrivacyContext<'_> {
        self.with_masks(None)
    }

    /// The case's context with its mask book: numbered tags that come back in the answer.
    fn masked_context(&self) -> PrivacyContext<'_> {
        self.with_masks(Some(&self.data.masks))
    }

    fn with_masks<'s>(&'s self, masks: Option<&'s dv_privacy::MaskBook>) -> PrivacyContext<'s> {
        PrivacyContext {
            case_id: &self.data.case_id,
            identities: &self.data.identities,
            practitioner: &self.data.practitioner,
            allowlisted: &self.allow,
            confirmed_names: &self.is_name,
            past_names: &self.past,
            today: today(),
            masks,
        }
    }
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
pub(crate) fn system_fonts(name: &str) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
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
    // Only how it looks; a template the export accepted never fails here, and if it did the
    // page would simply keep its own look.
    let look = dv_export::template_look(bytes).unwrap_or_default();
    Ok(TemplateView {
        has_marker: s.has_marker,
        headers: s.headers,
        footers: s.footers,
        images: s.images,
        styles_matched: s.styles_matched,
        styles_total: s.styles_total,
        size_kb: u32::try_from(bytes.len().div_ceil(1024)).unwrap_or(u32::MAX),
        font: look.font,
        size_half_points: look.size_half_points,
        heading_font: look.heading_font,
        page_twips: look.page.map(|(w, h)| [w, h]),
        margins_twips: look.margins,
    })
}

#[cfg(test)]
mod tests;
