//! A one-way copy of the clinic's calendar to her Google Calendar (D-202). Off by default.
//!
//! * Turning it on: she signs in with Google in her browser and allows the app to manage the
//!   calendars it makes itself (`calendar.app.created`, nothing of her other calendars or her
//!   Drive). The lasting permission is a vault secret (sealed inside the encrypted vault), so
//!   it is usable only while the vault is open. It does not need sign-in with Google for a
//!   forgotten password (D-041).
//! * What goes: the app makes one calendar, "פגישות", and keeps one event for each visit that
//!   is planned or was held, from two weeks back to half a year ahead: its time and one fixed
//!   word (`dv_domain::clinic::wanted_events`). The event's id is a keyed hash of the visit's
//!   id, with a key kept sealed in the vault, so nothing of the vault's own ids shows there.
//! * When: a little after a visit is saved, moved, cancelled or deleted, after an unlock,
//!   every half hour while the vault is open, and when she asks. Never while locked, never
//!   while the emergency stop is on. Each round runs with the core free (F2): the plan is made
//!   under the lock, the calls go outside it, and the outcome is written under it again. A
//!   failed round is a quiet line in the settings; the next one tries again.
//! * Turning it off forgets the permission at once and keeps it, sealed, only as a cleanup
//!   that is owed: delete the app's calendar (with every event in it), then withdraw the
//!   permission at Google, unless the encrypted Drive copy (D-099) rests on the same grant.
//!   Until Google confirms, the cleanup is tried again while the vault is open.
//! * A restored backup never turns it on: the permission it carries becomes a cleanup.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dv_domain::clinic::{self as rules, CalendarDetail};
use dv_egress::calendar::{self as cal, Event};
use dv_egress::google::{self, ClientConfig, GoogleError, Http};
use dv_vault::crypto::{hex, hmac_sha256, Key32};
use dv_vault::{AuditEvent, Vault, VaultError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::dates::{iso, today, unix_now};
use crate::jobs::Cancel;
use crate::{Core, CoreError};

/// The lasting permission, a vault secret.
const TOKEN_SECRET: &str = "calendar_token";
/// The key the event ids are made with (hex), a vault secret.
const KEY_SECRET: &str = "calendar_event_key";
/// A permission she turned off whose cleanup at Google is not confirmed yet.
const CLEANUP_SECRET: &str = "calendar_cleanup";
/// The calendars that cleanup deletes (ids, space-separated).
const CLEANUP_CALENDARS_KEY: &str = "calendar_cleanup_calendars";
/// How the last cleanup went: a retry that ends the same way logs nothing more.
const CLEANUP_RESULT_KEY: &str = "calendar_cleanup_result";
/// The id of the app's calendar in her account.
const CALENDAR_KEY: &str = "calendar_id";
/// When the last round finished well (unix seconds).
const LAST_KEY: &str = "calendar_last_at";
/// Why the last round did not, or empty.
const PROBLEM_KEY: &str = "calendar_problem";

/// A change waits this long, so a few saves in a row go in one round.
const AFTER_CHANGE: Duration = Duration::from_secs(20);
/// A change after a failed round waits at least this long (no hammering a line that is down).
const AFTER_FAILURE: Duration = Duration::from_secs(2 * 60);
/// With no change, a round every half hour (she may have deleted an event there by hand).
const EVERY: Duration = Duration::from_secs(30 * 60);
/// How long a cleanup that did not finish waits before it is tried again.
const CLEANUP_RETRY: Duration = Duration::from_secs(15 * 60);

/// One round or cleanup at a time.
static CALENDAR: Mutex<()> = Mutex::new(());

/// Why the last round did not go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum CalendarProblem {
    /// No internet. Tried again later.
    Offline,
    /// The permission at Google was withdrawn: connect again.
    Reconnect,
    /// Google answered something else, or the secure connection failed.
    Failed,
    /// The emergency stop is on: nothing leaves until it is off.
    Paused,
}

impl CalendarProblem {
    fn as_str(self) -> &'static str {
        match self {
            CalendarProblem::Offline => "offline",
            CalendarProblem::Reconnect => "reconnect",
            CalendarProblem::Failed => "failed",
            CalendarProblem::Paused => "paused",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [
            CalendarProblem::Offline,
            CalendarProblem::Reconnect,
            CalendarProblem::Failed,
            CalendarProblem::Paused,
        ]
        .into_iter()
        .find(|p| p.as_str() == s)
    }

    fn of(e: &GoogleError) -> Self {
        match e {
            GoogleError::Revoked => CalendarProblem::Reconnect,
            GoogleError::Offline => CalendarProblem::Offline,
            _ => CalendarProblem::Failed,
        }
    }
}

/// How turning it off went at Google.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum CalendarOff {
    /// It was off, and nothing was owed.
    Off,
    /// The app's calendar was deleted and the permission withdrawn (or kept for the Drive
    /// copy, which rests on the same grant).
    Cleaned,
    /// Not finished (no internet, Google failed, the emergency stop): tried again while the
    /// vault is open.
    Pending,
    /// The permission was already withdrawn at Google, so the calendar can no longer be
    /// deleted from here. She can delete it in Google Calendar.
    Withdrawn,
}

impl CalendarOff {
    fn as_str(self) -> &'static str {
        match self {
            CalendarOff::Off => "off",
            CalendarOff::Cleaned => "cleaned",
            CalendarOff::Pending => "pending",
            CalendarOff::Withdrawn => "withdrawn",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [
            CalendarOff::Off,
            CalendarOff::Cleaned,
            CalendarOff::Pending,
            CalendarOff::Withdrawn,
        ]
        .into_iter()
        .find(|r| r.as_str() == s)
    }
}

/// What the clinic's settings show about the copy in Google Calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CalendarStatus {
    /// This version can talk to Google (it was built with a Google registration).
    pub available: bool,
    pub on: bool,
    /// When the last round went through.
    #[ts(type = "number | null")]
    pub last_at: Option<i64>,
    pub problem: Option<CalendarProblem>,
    /// Turned off, but deleting the calendar at Google is not confirmed yet.
    pub cleanup_pending: bool,
    /// How the last turning off ended, when it could not delete the calendar.
    pub last_off: Option<CalendarOff>,
    pub detail: CalendarDetail,
}

/// The copy's memory-only state in the core. Kept across locks except the timings.
#[derive(Debug, Default)]
pub(crate) struct CalendarState {
    /// Moves when the copy turns on or off: a round or cleanup begun before stops.
    generation: Arc<AtomicU64>,
    /// A visit changed since the last round was handed out.
    changed: bool,
    /// When the last round was handed out.
    tried: Option<Instant>,
    /// When the cleanup owed was last handed out.
    cleanup_at: Option<Instant>,
    /// Rounds and cleanups on their way.
    running: Arc<AtomicUsize>,
    /// A calendar a round made, with the generation the round began at, until the round's
    /// outcome is written (a cleanup that comes first deletes it too).
    fresh: Arc<Mutex<Option<(u64, String)>>>,
}

impl CalendarState {
    /// After a lock: the next unlock syncs soon, and an owed cleanup is tried soon.
    pub(crate) fn on_lock(&mut self) {
        self.changed = true;
        self.tried = None;
        self.cleanup_at = None;
    }
}

/// Counts a round or cleanup on its way, until dropped.
struct Running(Arc<AtomicUsize>);

impl Running {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Running(Arc::clone(count))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn refused(text: &str) -> CoreError {
    CoreError::Refused(text.to_owned())
}

/// The event id of a visit: the prefix and 32 hex digits of a keyed hash of its id.
fn event_id(key: &Key32, visit_id: &str) -> String {
    let tag = hex(&hmac_sha256(key, format!("calendar|{visit_id}").as_bytes()));
    format!("{}{}", cal::EVENT_PREFIX, tag.get(..32).unwrap_or(&tag))
}

fn calendars_of(list: &str) -> Vec<String> {
    list.split(' ')
        .filter(|id| cal::calendar_id_ok(id))
        .map(str::to_owned)
        .collect()
}

// ------------------------------------------------------------------ a round

/// One round of the copy, to run with the core free.
pub struct CalendarJob {
    session: u64,
    cancel: Cancel,
    generation: Arc<AtomicU64>,
    at: u64,
    token: Zeroizing<String>,
    calendar: Option<String>,
    from: String,
    to: String,
    wanted: Vec<Event>,
    fresh: Arc<Mutex<Option<(u64, String)>>>,
    _running: Running,
}

impl std::fmt::Debug for CalendarJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarJob")
            .field("events", &self.wanted.len())
            .finish_non_exhaustive()
    }
}

enum Round {
    Done,
    Failed(CalendarProblem),
    /// Locked or turned off meanwhile: nothing to record.
    Stopped,
}

/// How a round went, waiting for [`Core::finish_calendar_sync`].
pub struct CalendarSynced {
    session: u64,
    at: u64,
    calendar: Option<String>,
    outcome: Round,
}

impl std::fmt::Debug for CalendarSynced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarSynced").finish_non_exhaustive()
    }
}

impl CalendarJob {
    /// Make the app's calendar (when it has none, or she deleted it), then make its events
    /// equal to the visits. Never touches the core.
    #[must_use]
    pub fn run(self, http: &dyn Http, cfg: &ClientConfig) -> CalendarSynced {
        let _one = CALENDAR.lock().unwrap_or_else(PoisonError::into_inner);
        let still = || {
            !self.cancel.is_set()
                && self.generation.load(Ordering::Acquire) == self.at
                && !dv_egress::is_stopped()
        };
        let mut calendar = self.calendar.clone();
        let outcome = if dv_egress::is_stopped() {
            Round::Failed(CalendarProblem::Paused)
        } else if !still() {
            Round::Stopped
        } else {
            let round = google::session_from_refresh(http, cfg, &self.token).and_then(|s| {
                let id = match calendar.as_deref() {
                    Some(id) if cal::calendar_exists(http, &s, id)? => id.to_owned(),
                    _ => {
                        if !still() {
                            return Err(GoogleError::Cancelled);
                        }
                        let id = cal::create_calendar(http, &s)?;
                        *self.fresh.lock().unwrap_or_else(PoisonError::into_inner) =
                            Some((self.at, id.clone()));
                        calendar = Some(id.clone());
                        id
                    }
                };
                let remote = cal::list_events(http, &s, &id, &self.from, &self.to)?;
                cal::apply(http, &s, &id, &remote, &self.wanted, &still)
            });
            match round {
                Ok(_) => Round::Done,
                Err(GoogleError::Cancelled) if dv_egress::is_stopped() => {
                    Round::Failed(CalendarProblem::Paused)
                }
                Err(GoogleError::Cancelled) => Round::Stopped,
                Err(e) => Round::Failed(CalendarProblem::of(&e)),
            }
        };
        CalendarSynced {
            session: self.session,
            at: self.at,
            calendar,
            outcome,
        }
    }

    /// The connection could not be set up on this computer: noted as a failure.
    #[must_use]
    pub fn not_started(self) -> CalendarSynced {
        CalendarSynced {
            session: self.session,
            at: self.at,
            calendar: self.calendar.clone(),
            outcome: Round::Failed(CalendarProblem::Failed),
        }
    }
}

// ------------------------------------------------------------------ the cleanup

/// After turning off (or a restore), with the core free: delete the app's calendars, then
/// withdraw the permission at Google unless something else rests on the grant.
pub struct CalendarCleanup {
    session: u64,
    token: Zeroizing<String>,
    calendars: Vec<String>,
    generation: Arc<AtomicU64>,
    at: u64,
    revoke: bool,
    fresh: Arc<Mutex<Option<(u64, String)>>>,
    _running: Running,
}

impl std::fmt::Debug for CalendarCleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarCleanup").finish_non_exhaustive()
    }
}

/// How a cleanup went, waiting for [`Core::finish_calendar_cleanup`].
pub struct CalendarCleaned {
    session: u64,
    token: Zeroizing<String>,
    outcome: CalendarOff,
}

impl std::fmt::Debug for CalendarCleaned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarCleaned").finish_non_exhaustive()
    }
}

impl CalendarCleanup {
    /// Done only when Google confirms; anything short of that is tried again later.
    #[must_use]
    pub fn run(self, http: &dyn Http, cfg: &ClientConfig) -> CalendarCleaned {
        let _one = CALENDAR.lock().unwrap_or_else(PoisonError::into_inner);
        let mut calendars = self.calendars.clone();
        // A calendar a round made before it was turned off, whose outcome was not written.
        if let Some((at, id)) = self
            .fresh
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            if at < self.at && !calendars.contains(&id) {
                calendars.push(id);
            }
        }
        let outcome = if dv_egress::is_stopped() {
            CalendarOff::Pending
        } else {
            match google::session_from_refresh(http, cfg, &self.token) {
                Err(GoogleError::Revoked) => CalendarOff::Withdrawn,
                Err(_) => CalendarOff::Pending,
                Ok(s) => {
                    let deleted = calendars
                        .iter()
                        .all(|id| cal::delete_calendar(http, &s, id).is_ok());
                    // Turned on again meanwhile: the new permission is the same grant.
                    let moved = self.generation.load(Ordering::Acquire) != self.at;
                    if !deleted {
                        CalendarOff::Pending
                    } else if !self.revoke || moved {
                        CalendarOff::Cleaned
                    } else {
                        match google::revoke_token(http, &self.token) {
                            Ok(()) => CalendarOff::Cleaned,
                            Err(_) => CalendarOff::Pending,
                        }
                    }
                }
            }
        };
        if outcome == CalendarOff::Cleaned {
            let mut fresh = self.fresh.lock().unwrap_or_else(PoisonError::into_inner);
            if fresh.as_ref().is_some_and(|(at, _)| *at < self.at) {
                *fresh = None;
            }
        }
        CalendarCleaned {
            session: self.session,
            token: self.token,
            outcome,
        }
    }
}

/// Move the permission (if any) into the cleanup owed, with the app's calendar. `true` when
/// there was one.
fn to_cleanup(v: &mut Vault) -> Result<bool, VaultError> {
    let read = v.secret(TOKEN_SECRET);
    // Also when it could not be read: the permission must not stay.
    let had = !matches!(read, Ok(None));
    let token = read.ok().flatten().filter(|t| !t.is_empty());
    if had {
        v.delete_secret(TOKEN_SECRET)?;
    }
    let mut calendars = calendars_of(&v.setting(CLEANUP_CALENDARS_KEY)?.unwrap_or_default());
    if let Some(id) = v
        .setting(CALENDAR_KEY)?
        .filter(|id| cal::calendar_id_ok(id))
    {
        if !calendars.contains(&id) {
            calendars.push(id);
        }
    }
    if let Some(token) = &token {
        v.set_secret(CLEANUP_SECRET, token)?;
        v.set_state(CLEANUP_CALENDARS_KEY, &calendars.join(" "))?;
        v.set_state(CLEANUP_RESULT_KEY, "")?;
    }
    v.set_state(CALENDAR_KEY, "")?;
    v.set_state(LAST_KEY, "")?;
    v.set_state(PROBLEM_KEY, "")?;
    if v.secret(KEY_SECRET).ok().flatten().is_some() {
        v.delete_secret(KEY_SECRET)?;
    }
    Ok(token.is_some())
}

/// Inside a restore (a failure here undoes it): a backup taken while the copy was on carries
/// its permission, and a restore must never turn it on. The permission becomes a cleanup.
pub(crate) fn forget_on_restore(v: &mut Vault) -> Result<(), VaultError> {
    if to_cleanup(v)? {
        v.record(
            AuditEvent::CalendarSyncOff,
            None,
            &serde_json::json!({ "restore": true }),
        )?;
    }
    Ok(())
}

// ------------------------------------------------------------------ the core

impl Core {
    fn calendar_secret(&self, name: &str) -> Option<Zeroizing<String>> {
        self.vault
            .as_ref()?
            .secret(name)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty())
    }

    /// The copy is on (the vault holds the permission).
    #[must_use]
    pub fn calendar_on(&self) -> bool {
        self.calendar_secret(TOKEN_SECRET).is_some()
    }

    /// The copy rests on the app's grant at Google: on, or a cleanup owed. A sign-in's end,
    /// or the Drive copy's cleanup, leaves the grant alone meanwhile (D-099's rule).
    #[must_use]
    pub fn calendar_holds_grant(&self) -> bool {
        self.calendar_on() || self.calendar_secret(CLEANUP_SECRET).is_some()
    }

    fn calendar_moved(&self) -> u64 {
        self.calendar.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn calendar_problem(&self) -> Option<CalendarProblem> {
        self.vault
            .as_ref()?
            .setting(PROBLEM_KEY)
            .ok()
            .flatten()
            .as_deref()
            .and_then(CalendarProblem::parse)
    }

    /// A visit was saved, moved, cancelled or deleted: the next round goes soon.
    pub(crate) fn clinic_changed(&mut self) {
        self.calendar.changed = true;
    }

    pub fn calendar_status(&mut self) -> Result<CalendarStatus, CoreError> {
        let on = self.calendar_on();
        let cleanup_pending = self.calendar_secret(CLEANUP_SECRET).is_some();
        let problem = if on { self.calendar_problem() } else { None };
        let v = self.vault_ref()?;
        let last_at = if on {
            v.setting(LAST_KEY)?.and_then(|t| t.parse().ok())
        } else {
            None
        };
        let last_off = v
            .setting(CLEANUP_RESULT_KEY)?
            .as_deref()
            .and_then(CalendarOff::parse)
            .filter(|r| *r == CalendarOff::Withdrawn);
        Ok(CalendarStatus {
            available: google::configured().is_some(),
            on,
            last_at,
            problem,
            cleanup_pending,
            last_off,
            detail: CalendarDetail::TimesOnly,
        })
    }

    /// Turning it on, step 1 (under the lock). Returns `keep_grant` for the sign-in: whether
    /// its end must leave the app's grant at Google alone.
    pub fn calendar_turn_on_check(&mut self) -> Result<bool, CoreError> {
        self.vault_ref()?;
        if self.calendar_on() && self.calendar_problem() != Some(CalendarProblem::Reconnect) {
            return Err(refused("הסנכרון עם יומן גוגל כבר דלוק."));
        }
        Ok(self.drive_keeps_grant())
    }

    /// Turning it on, step 3 (under the lock), after her browser sign-in gave a lasting
    /// permission: keep it sealed in the vault. The first round goes at once.
    pub fn calendar_turn_on_finish(&mut self, token: &str) -> Result<(), CoreError> {
        if token.is_empty() {
            return Err(CoreError::Internal("an empty permission".to_owned()));
        }
        self.vault_ref()?;
        let reconnect = self.calendar_problem() == Some(CalendarProblem::Reconnect);
        if self.calendar_on() && !reconnect {
            // Two turn-ons at once: the one kept stays (the same grant at Google).
            return Ok(());
        }
        let key = if self.calendar_secret(KEY_SECRET).is_some() {
            None
        } else {
            Some(Key32::random()?)
        };
        self.calendar_moved();
        let v = self.vault_mut()?;
        v.set_secret(TOKEN_SECRET, token)?;
        if let Some(key) = key {
            v.set_secret(KEY_SECRET, &hex(key.as_bytes()))?;
        }
        v.set_state(PROBLEM_KEY, "")?;
        v.set_state(CLEANUP_RESULT_KEY, "")?;
        if !reconnect {
            v.record(AuditEvent::CalendarSyncOn, None, &serde_json::json!({}))?;
        }
        self.calendar.changed = true;
        self.calendar.tried = None;
        Ok(())
    }

    /// Turn it off (under the lock): the permission is forgotten at once, and no round goes
    /// any more (one on its way stops before its next change). The permission stays sealed
    /// only as the cleanup owed, returned to run with the core free.
    pub fn calendar_turn_off(&mut self) -> Result<Option<CalendarCleanup>, CoreError> {
        let was_on = self.calendar_on();
        let v = self.vault_mut()?;
        if to_cleanup(v)? {
            v.record(AuditEvent::CalendarSyncOff, None, &serde_json::json!({}))?;
        }
        if was_on {
            self.calendar_moved();
        }
        self.calendar.cleanup_at = Some(Instant::now());
        Ok(self.calendar_cleanup())
    }

    fn calendar_cleanup(&self) -> Option<CalendarCleanup> {
        let token = self.calendar_secret(CLEANUP_SECRET)?;
        let v = self.vault.as_ref()?;
        let calendars = calendars_of(&v.setting(CLEANUP_CALENDARS_KEY).ok().flatten()?);
        Some(CalendarCleanup {
            session: self.session,
            token,
            calendars,
            generation: Arc::clone(&self.calendar.generation),
            at: self.calendar.generation.load(Ordering::Acquire),
            // The grant is one per account: withdrawn only when nothing else rests on it.
            revoke: !self.calendar_on() && !self.drive_holds_grant(),
            fresh: Arc::clone(&self.calendar.fresh),
            _running: Running::new(&self.calendar.running),
        })
    }

    /// A cleanup still owed, to try again with the core free: after an unlock, then at most
    /// every 15 minutes, never two at once, never while a Google sign-in waits for her browser.
    pub fn take_calendar_cleanup(&mut self) -> Option<CalendarCleanup> {
        if self
            .calendar
            .cleanup_at
            .is_some_and(|t| t.elapsed() < CLEANUP_RETRY)
            || self.calendar.running.load(Ordering::Acquire) > 0
            || crate::google::signing_in()
        {
            return None;
        }
        self.calendar_secret(CLEANUP_SECRET)?;
        self.calendar.cleanup_at = Some(Instant::now());
        self.calendar_cleanup()
    }

    /// Record how a cleanup went (under the lock).
    pub fn finish_calendar_cleanup(
        &mut self,
        done: CalendarCleaned,
    ) -> Result<CalendarOff, CoreError> {
        self.same_session(done.session)?;
        let ours = self
            .calendar_secret(CLEANUP_SECRET)
            .is_some_and(|t| t.as_str() == done.token.as_str());
        if !ours {
            // Finished by another run meanwhile.
            return Ok(if self.calendar_secret(CLEANUP_SECRET).is_some() {
                CalendarOff::Pending
            } else {
                CalendarOff::Cleaned
            });
        }
        let result = done.outcome;
        let v = self.vault_mut()?;
        if result != CalendarOff::Pending {
            v.delete_secret(CLEANUP_SECRET)?;
            v.set_state(CLEANUP_CALENDARS_KEY, "")?;
        }
        if v.setting(CLEANUP_RESULT_KEY)?.as_deref() != Some(result.as_str()) {
            v.set_state(CLEANUP_RESULT_KEY, result.as_str())?;
            v.record(
                AuditEvent::CalendarSyncCleanup,
                None,
                &serde_json::json!({ "result": result.as_str() }),
            )?;
        }
        Ok(result)
    }

    /// The next round, when one is due: on, open, none on its way, and a visit changed a
    /// little while ago (longer after a failure), or half an hour passed. Not after Google
    /// withdrew the permission (only connecting again, or her own "sync now", tries).
    pub fn take_calendar_sync(&mut self) -> Option<CalendarJob> {
        if !self.calendar_on() || self.calendar.running.load(Ordering::Acquire) > 0 {
            return None;
        }
        let problem = self.calendar_problem();
        if problem == Some(CalendarProblem::Reconnect) {
            return None;
        }
        let since = self.calendar.tried.map(|t| t.elapsed());
        let due = match since {
            None => true,
            Some(s) if s >= EVERY => true,
            Some(s) if self.calendar.changed => {
                s >= if problem.is_some() {
                    AFTER_FAILURE
                } else {
                    AFTER_CHANGE
                }
            }
            Some(_) => false,
        };
        if !due {
            return None;
        }
        self.calendar_job().ok()
    }

    /// A round now, because she asked.
    pub fn calendar_sync_now(&mut self) -> Result<CalendarJob, CoreError> {
        if !self.calendar_on() {
            return Err(refused("הסנכרון עם יומן גוגל כבוי."));
        }
        if self.calendar.running.load(Ordering::Acquire) > 0 {
            return Err(refused("הסנכרון כבר רץ. עוד רגע הוא יסתיים."));
        }
        self.calendar_job()
    }

    fn calendar_job(&mut self) -> Result<CalendarJob, CoreError> {
        let token = self
            .calendar_secret(TOKEN_SECRET)
            .ok_or_else(|| refused("הסנכרון עם יומן גוגל כבוי."))?;
        let key_hex = self
            .calendar_secret(KEY_SECRET)
            .ok_or_else(|| CoreError::Internal("no event key".to_owned()))?;
        let key = dv_vault::crypto::unhex(&key_hex)
            .ok()
            .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
            .map(Key32::from_bytes)
            .ok_or_else(|| CoreError::Internal("event key".to_owned()))?;
        let now = iso(today());
        let (from, to) = rules::sync_window(&now)
            .ok_or_else(|| CoreError::Internal("sync window".to_owned()))?;
        let v = self.vault_ref()?;
        let calendar = v
            .setting(CALENDAR_KEY)?
            .filter(|id| cal::calendar_id_ok(id));
        let visits = crate::clinic::visits_between(v, &from, &to)?;
        let wanted = rules::wanted_events(&visits, &from, &to, CalendarDetail::TimesOnly)
            .into_iter()
            .map(|e| Event {
                id: event_id(&key, &e.visit_id),
                start: e.start,
                end: e.end,
                title: e.title.to_owned(),
            })
            .collect();
        self.calendar.changed = false;
        self.calendar.tried = Some(Instant::now());
        Ok(CalendarJob {
            session: self.session,
            cancel: self.cancel.clone(),
            generation: Arc::clone(&self.calendar.generation),
            at: self.calendar.generation.load(Ordering::Acquire),
            token,
            calendar,
            from,
            to,
            wanted,
            fresh: Arc::clone(&self.calendar.fresh),
            _running: Running::new(&self.calendar.running),
        })
    }

    /// Record how a round went (under the lock). A failure is a quiet line in the settings,
    /// logged once until it changes.
    pub fn finish_calendar_sync(&mut self, done: CalendarSynced) -> Result<(), CoreError> {
        self.same_session(done.session)?;
        if self.calendar.generation.load(Ordering::Acquire) != done.at || !self.calendar_on() {
            // Turned off (or on again) meanwhile: a cleanup deletes what it made.
            return Ok(());
        }
        {
            let mut fresh = self
                .calendar
                .fresh
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if fresh.as_ref().is_some_and(|(at, _)| *at == done.at) {
                *fresh = None;
            }
        }
        let before = self.calendar_problem();
        let v = self.vault_mut()?;
        if let Some(id) = &done.calendar {
            if v.setting(CALENDAR_KEY)?.as_deref() != Some(id.as_str()) {
                v.set_state(CALENDAR_KEY, id)?;
            }
        }
        match done.outcome {
            Round::Stopped => {}
            Round::Done => {
                v.set_state(LAST_KEY, &unix_now().to_string())?;
                v.set_state(PROBLEM_KEY, "")?;
            }
            Round::Failed(p) => {
                v.set_state(PROBLEM_KEY, p.as_str())?;
                if before != Some(p) {
                    v.record(
                        AuditEvent::CalendarSyncFailed,
                        None,
                        &serde_json::json!({ "problem": p.as_str() }),
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// Sign in for the copy (outside the core's lock) and come back with the lasting permission.
/// A permission that cannot be used is withdrawn at once, unless the grant is held
/// (`keep_grant`, see [`Core::calendar_turn_on_check`]).
pub fn lasting_permission(
    open: &dyn Fn(&str) -> Result<(), CoreError>,
    keep_grant: bool,
) -> Result<(google::GoogleHttp, Zeroizing<String>), CoreError> {
    let (http, mut session) = crate::google::sign_in_calendar(open, keep_grant)?;
    match session.take_refresh_token() {
        Some(token) => Ok((http, token)),
        None => {
            if !keep_grant {
                google::revoke(&http, session);
            }
            Err(refused(
                "גוגל לא נתנה לתוכנה הרשאה קבועה. אפשר לנסות שוב ולאשר בחלון של גוגל.",
            ))
        }
    }
}

/// A permission that was given but is not kept (the vault locked during the sign-in):
/// withdrawn at Google, unless the grant is held.
pub fn withdraw_unkept(http: &dyn Http, token: &str, keep_grant: bool) {
    if !keep_grant {
        let _ = google::revoke_token(http, token);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
