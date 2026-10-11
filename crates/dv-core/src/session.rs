//! Session: creating, unlocking and locking the vault, the password and the recovery kit.

use super::*;

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
            removed_input: None,
            seal_dir: seal::default_dir(),
            rollback_warning: None,
            session: 0,
        }
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
    pub(crate) fn idle_lock_in(&self) -> Option<u32> {
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
        let params = self.argon.unwrap_or_else(Argon2Params::calibrate);
        let created = Vault::create(&self.dir, password, params)?;
        let recovery_key = created.recovery_key.to_string();
        let mut vault = created.vault;
        vault.set_setting(FIRST_USE_KEY, &day_number().to_string())?;
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

    /// `purge`: erase what waited in the recycle bin past its time. Not right after a restore,
    /// which is often done to get back something deleted by mistake.
    pub(crate) fn after_unlock(
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
                self.rollback_warning = self
                    .seal_dir
                    .as_deref()
                    .and_then(|d| seal::check(d, &self.dir, vault.audit_seq()));
                // Windows Hello was taken out (D-054): a vault that had it turned on loses the
                // old slot here. Best effort; the password opens either way.
                if vault.has_hello_slot() {
                    let _ = vault.remove_hello_slot();
                }
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

    pub(crate) fn lock_because(&mut self, reason: Option<&str>) {
        self.keep_unsaved();
        self.pending.clear();
        self.session = self.session.wrapping_add(1);
        self.staged_backup = None;
        self.style_staged.clear();
        self.removed_input = None;
        if let Some(v) = self.vault.take() {
            let _ = v.lock_because(reason);
        }
    }

    // ------------------------------------------------------------ password and kit

    /// `current` is the password, or the recovery kit when the password was forgotten.
    pub(crate) fn proof<'a>(current: &'a str, with_recovery: bool) -> dv_vault::Secret<'a> {
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
}
