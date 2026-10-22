//! Session: creating, unlocking and locking the vault, the password and the recovery kit.

use super::*;

/// A password or kit check (Argon2id, seconds at most) that has not come back after this
/// long is taken as lost, so the vault can never stay unopenable.
const SECRET_WORK_STALE: Duration = Duration::from_secs(120);

/// Wrong passwords or kits typed since the last opening, kept across a restart of the program
/// (a number only; the vault is locked, so it cannot hold it). Read and removed at the next
/// opening, which then says so (L4).
const FAILURES_FILE: &str = "unlock.failures";

fn failures_on_disk(dir: &Path) -> u32 {
    std::fs::read_to_string(dir.join(FAILURES_FILE))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Wrong tries this long before the opening that worked are taken as her own typos.
const OWN_TYPOS: Duration = Duration::from_secs(120);

/// What the opening says, only when there is something to say.
fn since_last_text(failed: u32) -> Option<String> {
    match failed {
        0 => None,
        1 => Some(
            "מאז הכניסה הקודמת היה ניסיון כניסה אחד שנכשל. אם זו לא את, כדאי להחליף סיסמה."
                .to_owned(),
        ),
        n => Some(format!(
            "מאז הכניסה הקודמת היו {n} ניסיונות כניסה שנכשלו. אם זו לא את, כדאי להחליף סיסמה."
        )),
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
            last_activity: Instant::now(),
            last_tick: None,
            failed_unlocks: 0,
            failure_times: Vec::new(),
            not_before: None,
            secret_work: None,
            secret_ticket: 0,
            cancel: jobs::Cancel::default(),
            disk_encryption: disk.to_owned(),
            transport: None,
            ingest_exe: None,
            ocr: None,
            staged_backup: None,
            style_staged: HashMap::new(),
            auto_backup_tried: None,
            unsaved: Vec::new(),
            removed_input: None,
            seal_dir: seal::default_dir(),
            rollback_warning: None,
            session: 0,
            models_dir: None,
            ner: None,
            ner_exe: None,
            hint_cache: HashMap::new(),
            settled: None,
            section_nonces: HashMap::new(),
            computer_seal: Arc::new(WindowsSeal),
            memory_lock: None,
            opened_cases: HashSet::new(),
            since_last: None,
            drive_generation: Arc::default(),
            drive_cleanup_at: None,
        }
    }

    /// What the shell managed at start (D-047, L7): `offered` where the computer can lock the
    /// program's memory in RAM, `locked` when the room for it was granted. Shown as it is on
    /// the security page.
    #[must_use]
    pub fn with_memory_lock(mut self, offered: bool, locked: bool) -> Self {
        self.memory_lock = offered.then_some(locked);
        self
    }

    /// Read documents in a separate worker process (the app passes its own binary).
    #[must_use]
    pub fn with_ingest_worker(mut self, exe: PathBuf) -> Self {
        self.ocr = dv_ingest::ocr::Engine::locate(&exe);
        self.ingest_exe = Some(exe);
        self
    }

    /// Tests: cheap KDF and a fake transport.
    #[must_use]
    pub fn for_tests(dir: &Path, transport: Option<Box<dyn Transport>>) -> Self {
        let mut core = Self::new(dir);
        core.argon = Some(Argon2Params::TEST);
        core.seal_dir = None;
        core.transport = transport.map(Arc::from);
        core
    }

    pub(crate) fn lock_minutes(&self) -> u32 {
        self.vault
            .as_ref()
            .and_then(|v| v.setting(LOCK_KEY).ok().flatten())
            .and_then(|s| s.parse().ok())
            .unwrap_or(10)
    }

    /// Auto-lock after inactivity, then hand out the vault.
    pub(crate) fn vault_mut(&mut self) -> Result<&mut Vault, CoreError> {
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
    pub fn idle_lock_in(&self) -> Option<u32> {
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

    pub(crate) fn vault_ref(&mut self) -> Result<&Vault, CoreError> {
        self.vault_mut().map(|v| &*v)
    }

    // ------------------------------------------------------------ lifecycle

    pub fn status(&mut self) -> AppStatus {
        let cloud = dv_vault::env::cloud_synced_component(&self.dir);
        let (practitioner, review_always) = match &self.vault {
            Some(v) => (
                v.practitioner().map(|p| p.names).unwrap_or_default(),
                v.setting(REVIEW_KEY).ok().flatten().as_deref() == Some("1"),
            ),
            None => (Vec::new(), true),
        };
        let speed = self
            .vault
            .as_ref()
            .and_then(|v| v.setting(SPEED_KEY).ok().flatten())
            .unwrap_or_else(|| DEFAULT_SPEED.to_owned());
        let model = self.chosen_model();
        let provider = provider_of(&model);
        let (demo, keys, integrity) = match &self.vault {
            Some(v) => (
                key_of(v, provider).ok().flatten().is_none(),
                Provider::ALL
                    .into_iter()
                    .filter(|p| key_name(*p).is_some_and(|k| v.secret(k).ok().flatten().is_some()))
                    .map(|p| p.id().to_owned())
                    .collect(),
                (!v.integrity().audit_ok || !v.integrity().header_ok)
                    .then(|| v.integrity().detail.clone().unwrap_or_default())
                    .or_else(|| self.rollback_warning.clone()),
            ),
            None => (true, Vec::new(), None),
        };
        AppStatus {
            vault_exists: Vault::exists(&self.dir),
            unlocked: self.vault.is_some(),
            disk_encryption: self.disk_encryption.clone(),
            cloud_synced_folder: cloud,
            fips_active: dv_vault::crypto::fips_active(),
            demo_mode: demo,
            model,
            provider: provider.id().to_owned(),
            ai_name: provider.display_name().to_owned(),
            keys,
            speed,
            integrity_warning: integrity,
            lock_minutes: self.lock_minutes(),
            idle_lock_in: self.idle_lock_in(),
            practitioner,
            review_always,
            screen_protection: self
                .vault
                .as_ref()
                .is_none_or(|v| v.setting(SCREEN_KEY).ok().flatten().as_deref() != Some("0")),
            since_last: self.since_last.clone(),
        }
    }

    /// Whether screenshots and screen sharing see the open vault (D-037). Off is recorded in
    /// the activity log; the locked screen stays protected either way.
    pub fn set_screen_protection(&mut self, on: bool) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        v.set_setting(SCREEN_KEY, if on { "1" } else { "0" })?;
        Ok(())
    }

    /// Show every request before it is sent (advanced mode), or send what the gate cleared.
    pub fn set_review_always(&mut self, on: bool) -> Result<(), CoreError> {
        self.vault_mut()?
            .set_setting(REVIEW_KEY, if on { "1" } else { "0" })?;
        Ok(())
    }

    pub(crate) fn refuse_cloud(&self) -> Result<(), CoreError> {
        match dv_vault::env::cloud_synced_component(&self.dir) {
            Some(folder) => Err(CoreError::Refused(format!(
                "תיקיית הכספת נמצאת בתוך תיקייה שמסונכרנת לענן ({folder}). יש לבחור תיקייה מקומית."
            ))),
            None => Ok(()),
        }
    }

    pub fn create_vault(&mut self, password: &str) -> Result<CreatedVault, CoreError> {
        self.refuse_cloud()?;
        // Not while a restore writes the same folder (or an unlock reads it).
        self.refuse_secret_work_running()?;
        let params = self.argon.unwrap_or_else(Argon2Params::calibrate);
        // Bound to this computer from the start where a key can be sealed here (D-076).
        let key = if Vault::exists(&self.dir) {
            None
        } else {
            std::fs::create_dir_all(&self.dir).map_err(VaultError::from)?;
            binding::key_here(self.computer_seal.as_ref(), &self.dir, false)
        };
        let created = Vault::create_here(&self.dir, password, params, key.as_ref())?;
        let recovery_key = created.recovery_key.to_string();
        let mut vault = created.vault;
        vault.set_state(FIRST_USE_KEY, &day_number().to_string())?;
        self.settled = None;
        self.vault = Some(vault);
        self.last_activity = Instant::now();
        Ok(CreatedVault { recovery_key })
    }

    pub(crate) fn check_backoff(&self) -> Result<(), CoreError> {
        match self.not_before {
            Some(t) if Instant::now() < t => Err(CoreError::Backoff(
                t.saturating_duration_since(Instant::now()).as_secs() + 1,
            )),
            _ => Ok(()),
        }
    }

    /// Take the one place for checking a password or kit off the lock, or refuse while
    /// another check runs. A check that never came back (a lost job) frees it after a while.
    /// Returns the ticket the job hands back to [`Self::release_secret_work`].
    pub(crate) fn claim_secret_work(&mut self) -> Result<u64, CoreError> {
        self.refuse_secret_work_running()?;
        self.secret_ticket = self.secret_ticket.wrapping_add(1);
        self.secret_work = Some((self.secret_ticket, Instant::now()));
        Ok(self.secret_ticket)
    }

    pub(crate) fn refuse_secret_work_running(&self) -> Result<(), CoreError> {
        match self.secret_work {
            Some((_, since)) if since.elapsed() < SECRET_WORK_STALE => Err(CoreError::Needs(
                "הסיסמה עדיין נבדקת. רגע אחד.".to_owned(),
                ErrorAction::Dismiss,
            )),
            _ => Ok(()),
        }
    }

    /// Frees the place, unless a newer check took it meanwhile (after a lock or the stale wait).
    pub(crate) fn release_secret_work(&mut self, ticket: u64) {
        if self.secret_work.is_some_and(|(t, _)| t == ticket) {
            self.secret_work = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn age_secret_work(&mut self) {
        if let Some((t, _)) = self.secret_work {
            let old = Instant::now().checked_sub(SECRET_WORK_STALE + Duration::from_secs(1));
            self.secret_work = old.map(|since| (t, since));
        }
    }

    /// `purge`: erase what waited in the recycle bin past its time. Not right after a restore,
    /// which is often done to get back something deleted by mistake.
    pub(crate) fn after_unlock(
        &mut self,
        result: Result<Vault, VaultError>,
        purge: bool,
    ) -> Result<AppStatus, CoreError> {
        match result {
            Ok(mut vault) => {
                // Also the tries before the program was last closed (every try is counted
                // on disk), but not her own typos just before this opening.
                let own = self
                    .failure_times
                    .iter()
                    .filter(|t| t.elapsed() < OWN_TYPOS)
                    .count();
                let failed = failures_on_disk(&self.dir)
                    .max(self.failed_unlocks)
                    .saturating_sub(u32::try_from(own).unwrap_or(u32::MAX));
                self.failure_times.clear();
                if failed > 0 {
                    let _ = vault.record(
                        AuditEvent::UnlockFailed,
                        None,
                        &serde_json::json!({ "count": failed }),
                    );
                }
                let _ = std::fs::remove_file(self.dir.join(FAILURES_FILE));
                self.since_last = since_last_text(failed);
                self.failed_unlocks = 0;
                self.not_before = None;
                self.rollback_warning = self
                    .seal_dir
                    .as_deref()
                    .and_then(|d| seal::check(d, &self.dir, vault.audit_seq()));
                // Windows Hello was taken out (D-054): a vault that had it turned on loses the
                // old slot here. Best effort; the password opens either way.
                if vault.has_hello_slot() {
                    let _ = vault.remove_hello_slot();
                }
                self.settled = None;
                self.vault = Some(vault);
                self.last_activity = Instant::now();
                // The gate and the sending rules try themselves once per opening; a failure
                // stops sending until the program is fixed (D-078, fail-closed).
                crate::layers::stop_if_broken(crate::layers::SelfChecks::run());
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
                // The name model's kept spans of materials that are gone go too (R4). Only
                // a saving: a failure here never keeps the vault from opening.
                if self.name_model_on() {
                    let _ = self.prune_name_hints();
                    self.preload_name_model();
                }
                Ok(self.status())
            }
            Err(e) => {
                if matches!(e, VaultError::WrongSecret | VaultError::RecoveryKeyInvalid) {
                    self.failed_unlocks += 1;
                    self.failure_times.push(Instant::now());
                    let wait = 2u64.saturating_pow(self.failed_unlocks.min(6)).min(60);
                    self.not_before = Some(Instant::now() + Duration::from_secs(wait));
                    let on_disk = failures_on_disk(&self.dir).saturating_add(1);
                    let _ = std::fs::write(self.dir.join(FAILURES_FILE), on_disk.to_string());
                }
                Err(e.into())
            }
        }
    }

    /// True when `typed` opens the vault: at creation, after a new kit, and when she checks
    /// the printed page in settings (D-076: on another computer the kit is the way in, so
    /// the security page asks for a check once a year). The day of the check is kept.
    pub fn confirm_recovery_key(&mut self, typed: &str) -> Result<bool, CoreError> {
        let ok = self.vault_ref()?.check_recovery_key(typed);
        if ok {
            self.vault_mut()?
                .set_setting(KIT_CHECKED_KEY, &crate::dates::unix_now().to_string())?;
        }
        Ok(ok)
    }

    /// When the printed kit was last typed and opened the vault.
    pub(crate) fn kit_checked_at(&self) -> Option<i64> {
        self.vault
            .as_ref()
            .and_then(|v| v.setting(KIT_CHECKED_KEY).ok().flatten())
            .and_then(|s| s.parse().ok())
    }

    /// Wipe keys, pending approvals and consultation history from memory.
    pub fn lock(&mut self) {
        self.lock_because(None);
    }

    pub(crate) fn lock_because(&mut self, reason: Option<&str>) {
        self.keep_unsaved();
        self.pending.clear();
        self.session = self.session.wrapping_add(1);
        self.cancel.set();
        self.cancel = jobs::Cancel::default();
        self.staged_backup = None;
        self.style_staged.clear();
        self.removed_input = None;
        // The cache is keyed by the case's texts themselves: overwritten, not only let go.
        for (mut text, _) in self.hint_cache.drain() {
            zeroize::Zeroize::zeroize(&mut text);
        }
        self.settled = None;
        self.section_nonces.clear();
        self.opened_cases.clear();
        self.since_last = None;
        // A cleanup owed at Google is tried again soon after the next unlock.
        self.drive_cleanup_at = None;
        // The model's process held texts of this session in its memory: it ends with the lock.
        self.end_name_model();
        // The filter's index of every case's names, kept between requests, goes too.
        dv_privacy::forget_identity_index();
        if let Some(v) = self.vault.take() {
            // A new password or kit begun while open lands as "locked meanwhile"; its check
            // must not keep her from opening the vault again.
            self.secret_work = None;
            let _ = v.lock_because(reason);
        }
    }

    // ------------------------------------------------------------ password and kit

    // ------------------------------------------------------------ settings
}
