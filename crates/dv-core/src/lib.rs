//! Application services used by the desktop shell.
//!
//! The Tauri commands are thin wrappers around [`Core`], so all behavior is testable on any
//! machine without a WebView. `Core` owns the unlocked vault and walks every request through
//! the same path: filter → build → review → gate → (approval) → send → check → store.

mod views;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dv_ai::{ModelConfig, SectionInput, TaggedInput, TaggedTurn, ALLOWED_MODELS};
use dv_domain::{
    Author, CaseMeta, CaseSummary, ChatRole, DraftStatus, IdentityInput, InputKind, ReportStructure,
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

pub use views::{
    AppStatus, CaseDetail, ChatView, ConsultResult, CreatedVault, ParagraphView, Prepared,
    ReviewPart, SectionResult, SectionView, SuspectDecision, UiError,
};

const API_KEY: &str = "anthropic_api_key";
const MODEL_KEY: &str = "model";
const LOCK_KEY: &str = "lock_minutes";
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
fn today() -> (i32, u32, u32) {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400);
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        i32::try_from(y).unwrap_or(2026),
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

enum PendingKind {
    Section {
        case_id: String,
        section_key: String,
        instruction_tagged: String,
        hidden: Vec<String>,
        sources: Vec<(String, String, String)>,
    },
    Consult {
        case_id: Option<String>,
        message_tagged: String,
    },
}

struct Pending {
    payload: ClearedPayload,
    kind: PendingKind,
}

/// Session state. One per running app; the shell keeps it behind a mutex.
pub struct Core {
    dir: PathBuf,
    argon: Option<Argon2Params>,
    vault: Option<Vault>,
    pending: HashMap<String, Pending>,
    consult_history: HashMap<String, Vec<TaggedTurn>>,
    last_activity: Instant,
    failed_unlocks: u32,
    not_before: Option<Instant>,
    disk_encryption: String,
    transport: Option<Box<dyn Transport>>,
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
            consult_history: HashMap::new(),
            last_activity: Instant::now(),
            failed_unlocks: 0,
            not_before: None,
            disk_encryption: disk.to_owned(),
            transport: None,
        }
    }

    /// Tests: cheap KDF and a fake transport.
    #[must_use]
    pub fn for_tests(dir: &Path, transport: Option<Box<dyn Transport>>) -> Self {
        let mut core = Self::new(dir);
        core.argon = Some(Argon2Params::TEST);
        core.transport = transport;
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

    fn vault_ref(&mut self) -> Result<&Vault, CoreError> {
        self.vault_mut().map(|v| &*v)
    }

    // ------------------------------------------------------------ lifecycle

    pub fn status(&mut self) -> AppStatus {
        let cloud = dv_vault::env::cloud_synced_component(&self.dir);
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
            integrity_warning: integrity,
            lock_minutes: self.lock_minutes(),
        }
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
        self.vault = Some(created.vault);
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

    fn after_unlock(&mut self, result: Result<Vault, VaultError>) -> Result<AppStatus, CoreError> {
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
        self.after_unlock(result)
    }

    pub fn unlock_with_recovery(&mut self, recovery_key: &str) -> Result<AppStatus, CoreError> {
        self.refuse_cloud()?;
        self.check_backoff()?;
        let result = Vault::unlock_with_recovery(&self.dir, recovery_key);
        self.after_unlock(result)
    }

    pub fn confirm_recovery_key(&mut self, typed: &str) -> Result<bool, CoreError> {
        Ok(self.vault_ref()?.check_recovery_key(typed))
    }

    /// Wipe keys, pending approvals and consultation history from memory.
    pub fn lock(&mut self) {
        self.pending.clear();
        self.consult_history.clear();
        if let Some(v) = self.vault.take() {
            let _ = v.lock();
        }
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
        let v = self.vault_mut()?;
        let id = v.create_case(&meta)?;
        v.set_identities(&id, &identities)?;
        Ok(id)
    }

    pub fn update_case(&mut self, case_id: &str, meta: CaseMeta) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.update_case_meta(case_id, &meta)?)
    }

    pub fn delete_case(&mut self, case_id: &str) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.delete_case(case_id)?)
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
        Ok(self
            .vault_mut()?
            .update_input(case_id, input_id, title, content)?)
    }

    pub fn delete_input(&mut self, case_id: &str, input_id: &str) -> Result<(), CoreError> {
        Ok(self.vault_mut()?.delete_input(case_id, input_id)?)
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

    fn model_config(&mut self) -> Result<ModelConfig, CoreError> {
        let model = self
            .vault_ref()?
            .setting(MODEL_KEY)?
            .unwrap_or_else(|| dv_ai::DEFAULT_MODEL.to_owned());
        Ok(ModelConfig {
            model,
            ..ModelConfig::default()
        })
    }

    pub fn case_detail(&mut self, case_id: &str) -> Result<CaseDetail, CoreError> {
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let v = self.vault_ref()?;
        let meta = v.case_meta(case_id)?;
        let identities = v.identities(case_id)?;
        let inputs = v.inputs(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
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
                    })
                    .collect();
                let source_count =
                    u32::try_from(inputs.iter().filter(|i| s.inputs.contains(&i.kind)).count())
                        .unwrap_or(0);
                let approved = paragraphs.iter().any(|p| p.status == DraftStatus::Approved);
                sections.push(SectionView {
                    key: s.key.clone(),
                    title: s.title.clone(),
                    part: part.title.clone(),
                    source_count,
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
            sections,
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
        let structure =
            ReportStructure::load_default().map_err(|e| CoreError::Internal(e.to_string()))?;
        let section = structure
            .section(section_key)
            .ok_or_else(|| CoreError::NotFound(section_key.to_owned()))?
            .clone();
        let model = self.model_config()?;
        let data = self.privacy_data(case_id)?;
        let demo_mode = self.vault_ref()?.secret(API_KEY)?.is_none() && self.transport.is_none();

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

            let mut review = Review::default();
            let mut tagged_sources = Vec::new();
            let mut source_rows = Vec::new();
            let derived = DERIVED_SECTIONS.contains(&section_key);
            if !derived {
                for (n, inp) in v
                    .inputs(case_id)?
                    .into_iter()
                    .filter(|i| section.inputs.contains(&i.kind))
                    .enumerate()
                {
                    let sid = format!("S{}", n + 1);
                    let title = run(&inp.title)?;
                    let content = run(&inp.content)?;
                    review.add(format!("{sid} · {} · כותרת", inp.kind.label_he()), &title);
                    review.add(format!("{sid} · {}", inp.kind.label_he()), &content);
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
                        approved_context.push((s.title.clone(), out.tagged));
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
                section_title: section.title.clone(),
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
        };
        self.gate(&data, &body, kind, &mut prepared)?;
        Ok(prepared)
    }

    fn transport_send(&mut self, payload: &ClearedPayload) -> Result<(Value, bool), CoreError> {
        if let Some(t) = &self.transport {
            return Ok((t.send(payload)?, false));
        }
        match self.vault_ref()?.secret(API_KEY)? {
            Some(key) => Ok((AnthropicTransport::new(&key)?.send(payload)?, false)),
            None => {
                let body: Value = serde_json::from_slice(payload.body())
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
                Ok((dv_ai::demo::respond(&body), true))
            }
        }
    }

    /// Send exactly what was approved, check the answer, and store it (tagged).
    pub fn send_section(&mut self, approval_id: &str) -> Result<SectionResult, CoreError> {
        let pending = self
            .pending
            .remove(approval_id)
            .ok_or_else(|| CoreError::NotFound("האישור פג. יש להכין את הבקשה מחדש.".to_owned()))?;
        let PendingKind::Section {
            case_id,
            section_key,
            instruction_tagged,
            hidden,
            sources,
        } = pending.kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let (response, demo) = self.transport_send(&pending.payload)?;
        let refs: Vec<(String, String)> = sources
            .iter()
            .map(|(sid, _, text)| (sid.clone(), text.clone()))
            .collect();
        let mut reply = dv_ai::parse_section(&response, &refs)?;

        let model = self.model_config()?.model;
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
        for p in &reply.paragraphs {
            let input_ids: Vec<String> = p
                .source_refs
                .iter()
                .filter_map(|r| {
                    sources
                        .iter()
                        .find(|(sid, _, _)| sid == r)
                        .map(|(_, id, _)| id.clone())
                })
                .collect();
            v.add_draft(&case_id, &section_key, &p.text, Author::Ai, &input_ids)?;
        }
        let payload_text = String::from_utf8_lossy(pending.payload.body()).into_owned();
        v.add_transmission(
            &case_id,
            &section_key,
            pending.payload.sha256(),
            if demo { "demo" } else { &model },
            &payload_text,
        )?;
        v.record(
            AuditEvent::Send,
            Some(&case_id),
            &serde_json::json!({ "section": section_key, "demo": demo }),
        )?;

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
        let inputs = self.vault_ref()?.inputs(case_id)?;
        let mut out = Vec::new();
        for s in structure
            .sections()
            .filter(|s| !DERIVED_SECTIONS.contains(&s.key.as_str()))
        {
            if inputs.iter().any(|i| s.inputs.contains(&i.kind)) {
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
    pub fn prepare_consult(
        &mut self,
        case_id: Option<&str>,
        message: &str,
    ) -> Result<Prepared, CoreError> {
        let model = self.model_config()?;
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
                        lines.push(format!("{}: {}", s.title, d.text_tagged));
                    }
                }
                if !lines.is_empty() {
                    let out = filter(&lines.join("\n"), &ctx)
                        .map_err(|e| CoreError::Internal(e.to_string()))?;
                    review.add("סיכום התיק (סעיפים מאושרים)".to_owned(), &out);
                    case_context = Some(out.tagged);
                }
            }
            let history = self.consult_history.get(&key).cloned().unwrap_or_default();
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
            message_tagged: input.message_tagged,
        };
        self.gate(&data, &body, kind, &mut prepared)?;
        Ok(prepared)
    }

    pub fn send_consult(&mut self, approval_id: &str) -> Result<ConsultResult, CoreError> {
        let pending = self
            .pending
            .remove(approval_id)
            .ok_or_else(|| CoreError::NotFound("האישור פג. יש לשלוח שוב.".to_owned()))?;
        let PendingKind::Consult {
            case_id,
            message_tagged,
        } = pending.kind
        else {
            return Err(CoreError::NotFound("בקשה מסוג אחר".to_owned()));
        };
        let (response, demo) = self.transport_send(&pending.payload)?;
        let answer = dv_ai::parse_consult(&response)?;
        let key = case_id.clone().unwrap_or_default();
        let turns = self.consult_history.entry(key).or_default();
        turns.push(TaggedTurn {
            role: "user".to_owned(),
            text_tagged: message_tagged,
        });
        turns.push(TaggedTurn {
            role: "assistant".to_owned(),
            text_tagged: answer.clone(),
        });
        let v = self.vault_mut()?;
        v.record(
            AuditEvent::Send,
            case_id.as_deref(),
            &serde_json::json!({ "consult": true, "demo": demo }),
        )?;
        let shown = match &case_id {
            Some(c) => {
                let identities = v.identities(c)?;
                restore(&answer, &identities, None)
            }
            None => answer,
        };
        Ok(ConsultResult {
            answer: shown,
            demo,
        })
    }
}

#[cfg(test)]
mod tests;
