//! The only code in the product that opens a network connection (CI enforces this).
//!
//! It accepts nothing but a [`ClearedPayload`] from the privacy gate, re-checks the request
//! against the Zero-Data-Retention rules, and sends the exact bytes the psychologist
//! approved to one host over TLS 1.3 with Mozilla's root store (not the OS store, so a
//! locally installed intercepting root cannot read the traffic). When she chose ChatGPT or
//! Gemini instead of Claude (D-040), [`providers`] re-shapes the same cleared text for that
//! company, adding nothing.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dv_privacy::ClearedPayload;
use serde_json::Value;
use zeroize::Zeroizing;

pub mod drive_backup;
pub mod google;
pub mod name_model;
mod pinning;
pub mod providers;
pub mod sse;
pub mod update;

pub use providers::{
    transport_for, Provider, GEMINI_MODELS, LOCAL_MODELS, MISTRAL_MODELS, OPENAI_MODELS,
};

pub const API_HOST: &str = "api.anthropic.com";
const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Claude models. Must match `dv_ai::ANTHROPIC_MODELS` (checked by a test in dv-core).
pub const ALLOWED_MODELS: &[&str] = &["claude-opus-5", "claude-sonnet-5", "claude-opus-4-8"];
/// Top-level request fields that may be sent. Everything else is refused.
const ALLOWED_FIELDS: &[&str] = &[
    "model",
    "max_tokens",
    "system",
    "messages",
    "output_config",
    "inference_geo",
    "tools",
    "thinking",
    // The same answer, delivered as it is written (real progress; D-035).
    "stream",
];
/// The only tool allowed, and only with an allow-list of domains (D-014).
const ALLOWED_TOOL: &str = "web_search_20250305";
/// The fields that tool may carry. Anything else (`user_location`, `blocked_domains`…) is
/// refused rather than passed on unchecked.
const ALLOWED_TOOL_FIELDS: &[&str] = &["type", "name", "max_uses", "allowed_domains"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EgressError {
    #[error("the request breaks the data-retention rules: {0}")]
    Policy(String),
    #[error("no API key is configured")]
    NoApiKey,
    #[error("the API key was rejected")]
    Unauthorized,
    #[error("too many requests; try again in a minute")]
    RateLimited,
    #[error("the AI service is busy or unavailable ({0})")]
    Unavailable(u16),
    /// Anthropic's `overloaded_error` (529): the service is up but has no room right now.
    #[error("the AI service is overloaded")]
    Overloaded,
    /// No credit left on the account, or a billing problem (`billing_error`, 402, or a
    /// "credit balance is too low" refusal). The server's words are kept for the details only.
    #[error("the account has no credit left or a billing problem: {0}")]
    Billing(String),
    /// The model was not found (`not_found_error`, 404): retired, or never offered to this key.
    #[error("the model was not found by the service: {0}")]
    ModelGone(String),
    /// The key is valid but not allowed this request (`permission_error`, 403).
    #[error("the key is not allowed this request: {0}")]
    Forbidden(String),
    /// The service refused the request as built (`invalid_request_error`, 400).
    #[error("the service refused the request as invalid: {0}")]
    InvalidRequest(String),
    #[error("the request was refused by the service ({0}): {1}")]
    Rejected(u16, String),
    /// Several busy answers in a row: no request goes out until the cool-down passes (D-122).
    /// Nothing was sent; the number is the seconds left.
    #[error("the AI service failed several times in a row; paused for {0}s")]
    CircuitOpen(u64),
    #[error("no connection to the AI service (the rest of the program keeps working)")]
    Offline,
    /// The connection was made and then lost, or ran out of time, before the whole answer
    /// came back (a computer that slept, a network that changed, a server that hung up). Not
    /// the same as no internet; the reason (an error kind, never a path or any text of the
    /// request) goes to "copy details".
    #[error("the connection to the AI service was cut before the answer was complete: {0}")]
    Interrupted(String),
    #[error("the local model does not answer (is Ollama running, with the model installed?)")]
    LocalUnavailable,
    #[error("the secure connection failed; something on this computer may be intercepting encrypted traffic")]
    Tls,
    /// The service's certificate is valid but from an authority this version is not pinned
    /// to (D-077): a new version of the program is needed.
    #[error("the service's certificate is not from an authority this version trusts; an update is needed")]
    Pinned,
    #[error("unexpected answer from the service: {0}")]
    Protocol(String),
    #[error("sending is switched off (emergency stop)")]
    Stopped,
    #[error("the request is larger than any report should need")]
    TooLarge,
    #[error("the daily limit of requests was reached")]
    DailyLimit,
}

/// The body as an object with allowed top-level fields only.
pub(crate) fn check_fields(body: &Value) -> Result<&serde_json::Map<String, Value>, EgressError> {
    let obj = body
        .as_object()
        .ok_or_else(|| EgressError::Policy("body is not an object".to_owned()))?;
    for key in obj.keys() {
        if !ALLOWED_FIELDS.contains(&key.as_str()) {
            return Err(EgressError::Policy(format!("field `{key}` is not allowed")));
        }
    }
    Ok(obj)
}

/// The rules check themselves at run time, for the security page (D-078): a model off the
/// allow-list, a field that is not allowed and a request without the US region are each
/// refused, and an allowed request passes. Nothing is sent.
#[must_use]
pub fn policy_self_check() -> bool {
    let body = |model: &str, geo: &str, extra: bool| {
        let mut b = serde_json::json!({
            "model": model,
            "inference_geo": geo,
            "max_tokens": 16,
            "messages": [{"role": "user", "content": "בדיקה"}],
        });
        if extra {
            if let Some(o) = b.as_object_mut() {
                o.insert("metadata".to_owned(), serde_json::json!({ "user_id": "x" }));
            }
        }
        b
    };
    let model = ALLOWED_MODELS.first().copied().unwrap_or_default();
    check_policy(&body(model, "us", false)).is_ok()
        && check_policy(&body("some-other-model", "us", false)).is_err()
        && check_policy(&body(model, "us", true)).is_err()
        && check_policy(&body(model, "eu", false)).is_err()
}

/// The pins of the AI service are all in place (D-077).
#[must_use]
pub fn pinning_in_place() -> bool {
    pinning::pinned_roots(pinning::ANTHROPIC_ROOTS).roots.len() == pinning::ANTHROPIC_ROOTS.len()
}

/// How many roots the AI service may chain to (D-077).
#[must_use]
pub fn pinned_root_count() -> usize {
    pinning::ANTHROPIC_ROOTS.len()
}

/// Check the outgoing body against the ZDR rules (STANDARDS.md §4, D-009).
pub fn check_policy(body: &Value) -> Result<(), EgressError> {
    let obj = check_fields(body)?;
    let model = obj.get("model").and_then(Value::as_str).unwrap_or("");
    if !ALLOWED_MODELS.contains(&model) {
        return Err(EgressError::Policy(format!(
            "model `{model}` is not on the ZDR allow-list"
        )));
    }
    if obj.get("inference_geo").and_then(Value::as_str) != Some("us") {
        return Err(EgressError::Policy(
            "inference_geo must be pinned to `us`".to_owned(),
        ));
    }
    if let Some(tools) = obj.get("tools") {
        // A `tools` value that is not a list is refused, not skipped.
        let tools = tools
            .as_array()
            .ok_or_else(|| EgressError::Policy("tools is not a list".to_owned()))?;
        for tool in tools {
            let ok = tool
                .as_object()
                .is_some_and(|t| t.keys().all(|k| ALLOWED_TOOL_FIELDS.contains(&k.as_str())))
                && tool["type"] == ALLOWED_TOOL
                && tool["allowed_domains"]
                    .as_array()
                    .is_some_and(|d| !d.is_empty() && d.iter().all(Value::is_string))
                && tool["max_uses"]
                    .as_u64()
                    .is_some_and(|n| (1..=8).contains(&n));
            if !ok {
                return Err(EgressError::Policy(
                    "only web search on allowed domains is permitted".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

/// Anything that can answer a cleared request: the real API, or demo mode (`dv-ai::demo`).
pub trait Transport: Send + Sync {
    fn send(&self, payload: &ClearedPayload) -> Result<Value, EgressError>;

    /// The same, telling `progress` how many words were written so far (when the answer
    /// arrives as a stream). Transports without a stream just answer.
    fn send_streaming(
        &self,
        payload: &ClearedPayload,
        progress: &dyn Fn(u32),
    ) -> Result<Value, EgressError> {
        let _ = progress;
        self.send(payload)
    }
}

/// Largest request body that leaves the machine. A whole case with every document is far below
/// this; anything above means a runaway loop or a bug, and is refused (fail-closed, S7).
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Largest answer read back. A whole report with the model's thinking is far below this; an
/// answer that does not end means a broken or hostile server, and is refused.
pub const MAX_ANSWER_BYTES: usize = 16 * 1024 * 1024;

/// Reads an answer up to the limit (one byte past it is enough to know it is over).
pub(crate) fn capped(from: impl std::io::Read) -> std::io::Take<impl std::io::Read> {
    from.take(MAX_ANSWER_BYTES as u64 + 1)
}

/// The whole (non-streamed) answer as text, refused past the limit.
pub(crate) fn read_answer(from: impl std::io::Read) -> Result<String, EgressError> {
    let mut bytes = Vec::new();
    capped(from)
        .read_to_end(&mut bytes)
        .map_err(|e| EgressError::Protocol(e.to_string()))?;
    if bytes.len() > MAX_ANSWER_BYTES {
        return Err(EgressError::Protocol(format!(
            "answer over {MAX_ANSWER_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes).map_err(|e| EgressError::Protocol(e.to_string()))
}

/// Most requests to a remote service in 24 hours, for the whole process.
pub const DAILY_REQUESTS: usize = 400;

static STOPPED: AtomicBool = AtomicBool::new(false);
static DAILY: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

/// The emergency stop: while on, no request leaves the machine (local models still work).
/// Memory only, so a restart clears it; the shell flips it from one visible button.
pub fn set_stopped(on: bool) {
    STOPPED.store(on, Ordering::SeqCst);
}

pub fn is_stopped() -> bool {
    STOPPED.load(Ordering::SeqCst)
}

/// Last check before the wire: the emergency stop, the size limit and the daily limit.
pub(crate) fn admit(body_len: usize, remote: bool) -> Result<(), EgressError> {
    if body_len > MAX_BODY_BYTES {
        return Err(EgressError::TooLarge);
    }
    if !remote {
        return Ok(());
    }
    if is_stopped() {
        return Err(EgressError::Stopped);
    }
    let mut day = DAILY.lock().map_err(|_| EgressError::DailyLimit)?;
    admit_daily(&mut day, DAILY_REQUESTS, Instant::now())
}

fn admit_daily(sent: &mut Vec<Instant>, max: usize, now: Instant) -> Result<(), EgressError> {
    sent.retain(|t| now.duration_since(*t) < Duration::from_secs(24 * 3600));
    if sent.len() >= max {
        return Err(EgressError::DailyLimit);
    }
    sent.push(now);
    Ok(())
}

/// Simple sliding-window limit: a runaway loop cannot send hundreds of requests.
#[derive(Debug)]
pub(crate) struct RateLimit {
    window: Duration,
    max: usize,
    sent: Vec<Instant>,
}

impl RateLimit {
    pub(crate) const fn per_minute(max: usize) -> Self {
        Self {
            window: Duration::from_secs(60),
            max,
            sent: Vec::new(),
        }
    }

    #[cfg(test)]
    fn allow(&mut self) -> bool {
        self.take(Instant::now()).is_ok()
    }

    /// Takes a place in the window now, or says how long until the oldest place leaves it.
    fn take(&mut self, now: Instant) -> Result<(), Duration> {
        self.sent
            .retain(|t| now.saturating_duration_since(*t) < self.window);
        if self.sent.len() >= self.max {
            let oldest = self.sent.iter().min().copied().unwrap_or(now);
            let free_at = oldest + self.window;
            return Err(free_at
                .saturating_duration_since(now)
                .max(Duration::from_millis(1)));
        }
        self.sent.push(now);
        Ok(())
    }
}

/// The longest a request waits for a place in the minute window (D-141): a little more than
/// the window itself, so an ordinary burst always gets through and a runaway loop still stops.
pub(crate) const LIMIT_WAIT: Duration = Duration::from_secs(90);

/// A request past the per-minute limit waits until the oldest one leaves the window, then goes
/// (D-141), instead of failing at once. The lock is held only to look, never while waiting. The
/// emergency stop ends the wait of a request to a remote service; one that cannot get a place
/// within `LIMIT_WAIT` is refused, as before.
pub(crate) fn wait_for_place(limit: &Mutex<RateLimit>, remote: bool) -> Result<(), EgressError> {
    wait_for_place_with(
        limit,
        remote,
        &Instant::now,
        &std::thread::sleep,
        &is_stopped,
    )
}

fn wait_for_place_with(
    limit: &Mutex<RateLimit>,
    remote: bool,
    now: &dyn Fn() -> Instant,
    sleep: &dyn Fn(Duration),
    stopped: &dyn Fn() -> bool,
) -> Result<(), EgressError> {
    let start = now();
    loop {
        if remote && stopped() {
            return Err(EgressError::Stopped);
        }
        // A poisoned lock refuses, like everything here that cannot be checked.
        let left = match limit
            .lock()
            .map_err(|_| EgressError::RateLimited)?
            .take(now())
        {
            Ok(()) => return Ok(()),
            Err(left) => left,
        };
        if now().saturating_duration_since(start) + left > LIMIT_WAIT {
            return Err(EgressError::RateLimited);
        }
        // In short steps, so the emergency stop is heard while waiting.
        sleep(left.min(Duration::from_millis(500)));
    }
}

/// TLS 1.3 only, the Mozilla roots built into the program (never the computer's own store),
/// and for the AI service only the few roots it is pinned to (D-077). Anything that cannot be
/// set up trusts nothing at all, so no connection opens (fail closed).
pub(crate) fn tls_config() -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider));
    let Ok(builder) = builder.with_protocol_versions(&[&rustls::version::TLS13]) else {
        // Unreachable with the aws-lc provider.
        return rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
    };
    match pinning::PinnedVerifier::new(&provider) {
        // A verifier of the program's own: the Mozilla roots with the pins on top. Nothing is
        // loosened (the name `dangerous` is rustls's for any custom verifier).
        Some(v) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(v))
            .with_no_client_auth(),
        None => builder
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth(),
    }
}

/// The one HTTP client setup for every AI provider: TLS 1.3 only, Mozilla roots, HTTPS only,
/// no proxy, no redirects.
pub(crate) fn build_client() -> Result<reqwest::blocking::Client, EgressError> {
    reqwest::blocking::Client::builder()
        .use_preconfigured_tls(tls_config())
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(20))
        .user_agent("diagnostic-vault")
        .build()
        .map_err(|e| EgressError::Protocol(e.to_string()))
}

/// Send once, and again after 2 s and then 4 s while the server is busy (5xx, overloaded) or
/// asks to slow down (429): three tries at most. Any other answer, or error (an open breaker
/// among them), is returned as is.
pub(crate) fn retry(
    mut once: impl FnMut() -> Result<Value, EgressError>,
) -> Result<Value, EgressError> {
    let mut delay = Duration::from_secs(2);
    let mut attempt = 0;
    loop {
        match once() {
            Err(
                EgressError::Unavailable(_) | EgressError::Overloaded | EgressError::RateLimited,
            ) if attempt < 2 => {
                std::thread::sleep(delay);
                delay *= 2;
                attempt += 1;
            }
            other => return other,
        }
    }
}

pub(crate) fn send_error(e: &reqwest::Error) -> EgressError {
    let text = format!("{e:?}").to_lowercase();
    let tls = text.contains("certificate") || text.contains("tls") || text.contains("handshake");
    // The refusal is told apart by its mark, or by the flag the verifier set for it. The flag
    // is read (and cleared) every time, so an old refusal never names a later failure.
    let refused = pinning::take_pin_refused();
    if text.contains(pinning::PIN_MARK) || (tls && refused) {
        EgressError::Pinned
    } else if tls {
        EgressError::Tls
    } else if e.is_connect() {
        // No connection at all: no network, no name lookup, or nothing answering.
        EgressError::Offline
    } else {
        // Connected, and then lost or out of time: not "no internet" (D-134).
        EgressError::Interrupted(cut_reason(e))
    }
}

/// What cut a connection, in words fit for "copy details": the stage, and the system's error
/// kind when there is one. Never the URL, a path or anything of the request.
pub(crate) fn cut_reason(e: &reqwest::Error) -> String {
    let stage = if e.is_timeout() {
        "timed out"
    } else if e.is_body() || e.is_decode() {
        "while reading the answer"
    } else {
        "while sending"
    };
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        if let Some(io) = s.downcast_ref::<std::io::Error>() {
            return format!("{stage} ({:?})", io.kind());
        }
        source = s.source();
    }
    stage.to_owned()
}

/// What an error answer of the Anthropic API means: its `error.type` first, its status when
/// the type is missing or new. The server's own words go only into the variant, which the
/// program shows under "details" and never sends anywhere (D-121).
pub(crate) fn anthropic_error(status: u16, body: &str) -> EgressError {
    let v = serde_json::from_str::<Value>(body).ok();
    let field = |p: &str| {
        v.as_ref()
            .and_then(|v| v.pointer(p))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    anthropic_kind(&field("/error/type"), status, field("/error/message"))
}

/// The same for a type and message already read (a streamed `error` event has no status of
/// its own; it passes 400, the status of a refused request).
pub(crate) fn anthropic_kind(kind: &str, status: u16, message: String) -> EgressError {
    // No credit left comes back as an invalid request whose only mark is its wording.
    if kind == "billing_error"
        || status == 402
        || message.to_ascii_lowercase().contains("credit balance")
    {
        return EgressError::Billing(message);
    }
    match kind {
        "authentication_error" => EgressError::Unauthorized,
        "permission_error" => EgressError::Forbidden(message),
        "not_found_error" => EgressError::ModelGone(message),
        "rate_limit_error" => EgressError::RateLimited,
        "overloaded_error" => EgressError::Overloaded,
        "api_error" | "timeout_error" => EgressError::Unavailable(status.max(500)),
        "invalid_request_error" => EgressError::InvalidRequest(message),
        _ => match status {
            401 => EgressError::Unauthorized,
            403 => EgressError::Forbidden(message),
            404 => EgressError::ModelGone(message),
            429 => EgressError::RateLimited,
            529 => EgressError::Overloaded,
            500..=599 => EgressError::Unavailable(status),
            400 => EgressError::InvalidRequest(message),
            _ => EgressError::Rejected(status, message),
        },
    }
}

/// Consecutive busy answers (5xx, overloaded) that open the breaker.
const BREAKER_FAILURES: u32 = 5;
/// How long an open breaker refuses before one trial request may go.
const BREAKER_COOL_DOWN: Duration = Duration::from_secs(60);
/// What a request is told while the trial request is out: it may be back in seconds.
const BREAKER_TRIAL_WAIT: Duration = Duration::from_secs(5);
/// A trial request that never reported back (longer than the client's own timeout) is taken
/// as lost, so the breaker cannot stay shut for the rest of the run.
const BREAKER_TRIAL_LOST: Duration = Duration::from_secs(660);

/// A small circuit breaker for the AI service (THREAT_MODEL E-5, D-122): after
/// [`BREAKER_FAILURES`] busy answers in a row it refuses every request for
/// [`BREAKER_COOL_DOWN`] without opening a connection; then one trial request may go
/// (half-open). An answer from a working service closes it; another busy answer opens it again.
/// Memory only: a restart starts closed.
#[derive(Debug, Default)]
pub(crate) struct Breaker {
    failures: u32,
    /// Open until this moment.
    open_until: Option<Instant>,
    /// Half-open: the trial request out now, and since when.
    trial: Option<Instant>,
}

/// What one attempt says about the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    /// It answered (with an answer or a refusal of this request): it works.
    Up,
    /// A 5xx or overloaded answer.
    Busy,
    /// Nothing about the service: no connection, a refusal before sending, a rate limit.
    Unknown,
}

impl Health {
    pub(crate) fn of(r: &Result<Value, EgressError>) -> Self {
        match r {
            Ok(_)
            | Err(
                EgressError::Unauthorized
                | EgressError::Billing(_)
                | EgressError::ModelGone(_)
                | EgressError::Forbidden(_)
                | EgressError::InvalidRequest(_)
                | EgressError::Rejected(..),
            ) => Health::Up,
            Err(EgressError::Unavailable(_) | EgressError::Overloaded) => Health::Busy,
            Err(_) => Health::Unknown,
        }
    }
}

impl Breaker {
    /// May an attempt go now? While open, the seconds left; while half-open, only one trial.
    pub(crate) fn admit(&mut self, now: Instant) -> Result<(), EgressError> {
        if let Some(until) = self.open_until {
            if now < until {
                let left = until.saturating_duration_since(now);
                return Err(EgressError::CircuitOpen(left.as_secs() + 1));
            }
            // Half-open: one trial at a time.
            if self
                .trial
                .is_some_and(|since| now.saturating_duration_since(since) < BREAKER_TRIAL_LOST)
            {
                return Err(EgressError::CircuitOpen(BREAKER_TRIAL_WAIT.as_secs()));
            }
            self.trial = Some(now);
        }
        Ok(())
    }

    pub(crate) fn record(&mut self, health: Health, now: Instant) {
        match health {
            Health::Up => *self = Self::default(),
            Health::Busy => {
                self.failures = self.failures.saturating_add(1);
                self.trial = None;
                // A failed trial opens it again at once; otherwise the count decides.
                if self.open_until.is_some() || self.failures >= BREAKER_FAILURES {
                    self.open_until = Some(now + BREAKER_COOL_DOWN);
                }
            }
            // The trial told nothing: the next request may try.
            Health::Unknown => self.trial = None,
        }
    }
}

/// The breaker for the Anthropic service, and the per-minute limit shared by every request to
/// a remote service: each send builds a new transport, so both must outlive it.
static ANTHROPIC_BREAKER: Mutex<Breaker> = Mutex::new(Breaker {
    failures: 0,
    open_until: None,
    trial: None,
});
pub(crate) static MINUTE_LIMIT: Mutex<RateLimit> = Mutex::new(RateLimit::per_minute(20));

/// The real transport to the Anthropic Messages API.
pub struct AnthropicTransport {
    client: reqwest::blocking::Client,
    api_key: Zeroizing<String>,
    url: String,
    limit: &'static Mutex<RateLimit>,
    breaker: &'static Mutex<Breaker>,
}

impl std::fmt::Debug for AnthropicTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicTransport")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl AnthropicTransport {
    pub fn new(api_key: &str) -> Result<Self, EgressError> {
        if api_key.trim().is_empty() {
            return Err(EgressError::NoApiKey);
        }
        Ok(Self {
            client: build_client()?,
            api_key: Zeroizing::new(api_key.trim().to_owned()),
            url: MESSAGES_URL.to_owned(),
            limit: &MINUTE_LIMIT,
            breaker: &ANTHROPIC_BREAKER,
        })
    }

    fn post_once(&self, body: &[u8], progress: &dyn Fn(u32)) -> Result<Value, EgressError> {
        let resp = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("x-api-key", self.api_key.as_str())
            .header("anthropic-version", API_VERSION)
            .body(body.to_vec())
            .send()
            .map_err(|e| send_error(&e))?;
        let status = resp.status().as_u16();
        let streamed = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        if status == 200 && streamed {
            return sse::assemble(std::io::BufReader::new(capped(resp)), progress);
        }
        let text = read_answer(resp)?;
        match status {
            200 => serde_json::from_str(&text).map_err(|e| EgressError::Protocol(e.to_string())),
            _ => Err(anthropic_error(status, &text)),
        }
    }

    /// One attempt through the breaker: refused at once while it is open, and its outcome
    /// told to it.
    fn attempt(&self, body: &[u8], progress: &dyn Fn(u32)) -> Result<Value, EgressError> {
        // A poisoned lock refuses, like everything here that cannot be checked.
        self.breaker
            .lock()
            .map_err(|_| EgressError::CircuitOpen(BREAKER_COOL_DOWN.as_secs()))?
            .admit(Instant::now())?;
        let r = self.post_once(body, progress);
        if let Ok(mut b) = self.breaker.lock() {
            b.record(Health::of(&r), Instant::now());
        }
        r
    }
}

impl Transport for AnthropicTransport {
    fn send(&self, payload: &ClearedPayload) -> Result<Value, EgressError> {
        self.send_streaming(payload, &|_| {})
    }

    fn send_streaming(
        &self,
        payload: &ClearedPayload,
        progress: &dyn Fn(u32),
    ) -> Result<Value, EgressError> {
        let body: Value = serde_json::from_slice(payload.body())
            .map_err(|e| EgressError::Policy(e.to_string()))?;
        check_policy(&body)?;
        admit(payload.body().len(), true)?;
        wait_for_place(self.limit, true)?;
        retry(|| self.attempt(payload.body(), progress))
    }
}

#[cfg(test)]
mod wire_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn the_rules_and_the_pins_pass_their_own_check() {
        assert!(super::policy_self_check());
        assert!(super::pinning_in_place());
    }

    use super::*;
    use serde_json::json;

    fn ok_body() -> Value {
        json!({"model": "claude-opus-5", "max_tokens": 100, "inference_geo": "us", "messages": []})
    }

    #[test]
    fn allowed_request_passes_the_policy() {
        assert!(check_policy(&ok_body()).is_ok());
        let mut b = ok_body();
        b["stream"] = json!(true);
        assert!(check_policy(&b).is_ok());
    }

    #[test]
    fn non_zdr_models_extra_fields_and_other_tools_are_refused() {
        let mut b = ok_body();
        b["model"] = json!("claude-fable-5-1");
        assert!(check_policy(&b).is_err());

        let mut b = ok_body();
        b["metadata"] = json!({"user_id": "x"});
        assert!(check_policy(&b).is_err());

        let mut b = ok_body();
        b["inference_geo"] = json!("global");
        assert!(check_policy(&b).is_err());

        let mut b = ok_body();
        b["tools"] = json!([{"type": "code_execution_20260521", "name": "code_execution"}]);
        assert!(check_policy(&b).is_err());

        let mut b = ok_body();
        b["tools"] = json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 3}]);
        assert!(
            check_policy(&b).is_err(),
            "web search without a domain allow-list"
        );

        let mut b = ok_body();
        b["tools"] = json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 3, "allowed_domains": ["apa.org"]}]);
        assert!(check_policy(&b).is_ok());
    }

    #[test]
    fn a_tools_value_that_is_not_a_list_or_a_tool_with_extra_fields_is_refused() {
        // Both used to pass: a non-list `tools` was skipped, and fields beyond the checked
        // ones (a location, say) went out with the allowed search tool.
        let search = json!({"type": "web_search_20250305", "name": "web_search", "max_uses": 3, "allowed_domains": ["apa.org"]});
        for tools in [
            json!({"type": "code_execution_20250825", "name": "code_execution"}),
            json!("web_search_20250305"),
            json!(null),
            json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 3, "allowed_domains": ["apa.org"], "user_location": {"type": "approximate", "city": "x"}}]),
            json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 3, "allowed_domains": ["apa.org"], "blocked_domains": ["x.org"]}]),
            json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 3, "allowed_domains": [7]}]),
            json!(["web_search_20250305"]),
        ] {
            let mut b = ok_body();
            b["tools"] = tools.clone();
            assert!(check_policy(&b).is_err(), "{tools}");
        }
        let mut b = ok_body();
        b["tools"] = json!([search]);
        assert!(check_policy(&b).is_ok());
    }

    #[test]
    fn empty_api_key_is_refused_and_client_builds() {
        assert!(matches!(
            AnthropicTransport::new("  "),
            Err(EgressError::NoApiKey)
        ));
        assert!(AnthropicTransport::new("sk-ant-test-not-real").is_ok());
    }

    #[test]
    fn size_and_daily_limits_refuse_before_the_wire() {
        assert_eq!(admit(MAX_BODY_BYTES + 1, false), Err(EgressError::TooLarge));
        assert_eq!(admit(MAX_BODY_BYTES + 1, true), Err(EgressError::TooLarge));
        let mut day = Vec::new();
        let now = Instant::now();
        assert!(admit_daily(&mut day, 2, now).is_ok());
        assert!(admit_daily(&mut day, 2, now).is_ok());
        assert_eq!(admit_daily(&mut day, 2, now), Err(EgressError::DailyLimit));
    }

    #[test]
    fn emergency_stop_blocks_remote_but_not_local() {
        set_stopped(true);
        let remote = admit(10, true);
        let local = admit(10, false);
        set_stopped(false);
        assert_eq!(remote, Err(EgressError::Stopped));
        assert!(local.is_ok());
    }

    #[test]
    fn rate_limit_stops_runaway_loops() {
        let mut l = RateLimit::per_minute(3);
        assert!(l.allow() && l.allow() && l.allow());
        assert!(!l.allow());
    }

    /// D-141: a full window waits for the first place to free, never holding the lock meanwhile.
    #[test]
    fn a_full_minute_waits_for_a_place_instead_of_failing() {
        use std::cell::Cell;
        let t0 = Instant::now();
        let clock = Cell::new(t0);
        let limit = Mutex::new(RateLimit::per_minute(2));
        {
            let mut l = limit.lock().unwrap();
            assert!(l.take(t0).is_ok());
            assert!(l.take(t0 + Duration::from_secs(10)).is_ok());
        }
        clock.set(t0 + Duration::from_secs(30));
        let slept = Cell::new(Duration::ZERO);
        let r = wait_for_place_with(
            &limit,
            true,
            &|| clock.get(),
            &|d| {
                assert!(limit.try_lock().is_ok(), "the lock is free while waiting");
                slept.set(slept.get() + d);
                clock.set(clock.get() + d);
            },
            &|| false,
        );
        assert_eq!(r, Ok(()));
        // The first place left the window at 60 s: 30 s of waiting, no more.
        assert_eq!(slept.get(), Duration::from_secs(30));
        assert_eq!(limit.lock().unwrap().sent.len(), 2);
    }

    #[test]
    fn a_place_too_far_off_is_refused_at_once_and_the_stop_ends_the_wait() {
        use std::cell::Cell;
        let t0 = Instant::now();
        let far = Mutex::new(RateLimit {
            window: Duration::from_secs(300),
            max: 1,
            sent: vec![t0],
        });
        let never = |_: Duration| panic!("nothing to wait for");
        let r = wait_for_place_with(&far, true, &|| t0, &never, &|| false);
        assert_eq!(r, Err(EgressError::RateLimited));

        let clock = Cell::new(t0);
        let stop = Cell::new(false);
        let full = Mutex::new(RateLimit::per_minute(1));
        full.lock().unwrap().sent.push(t0);
        let r = wait_for_place_with(
            &full,
            true,
            &|| clock.get(),
            &|d| {
                clock.set(clock.get() + d);
                stop.set(true);
            },
            &|| stop.get(),
        );
        assert_eq!(r, Err(EgressError::Stopped));
        assert_eq!(full.lock().unwrap().sent.len(), 1, "nothing was sent");
        // A local model is not stopped by it: it waits for its place.
        stop.set(false);
        let r = wait_for_place_with(
            &full,
            false,
            &|| clock.get(),
            &|d| {
                clock.set(clock.get() + d);
                stop.set(true);
            },
            &|| stop.get(),
        );
        assert_eq!(r, Ok(()));
    }

    #[test]
    fn every_transport_shares_one_minute_limit_and_one_breaker() {
        // Each send builds a new transport: a limit or breaker of its own would start empty.
        let a = AnthropicTransport::new("sk-ant-test-not-real").unwrap();
        let b = AnthropicTransport::new("sk-ant-test-not-real").unwrap();
        assert!(std::ptr::eq(a.limit, b.limit) && std::ptr::eq(a.limit, &MINUTE_LIMIT));
        assert!(std::ptr::eq(a.breaker, b.breaker));
    }

    /// D-121: Anthropic's error answers, by type and by status, as real ones look.
    #[test]
    fn anthropic_error_answers_become_their_own_variants() {
        let body = |kind: &str, message: &str| {
            json!({"type": "error", "error": {"type": kind, "message": message}}).to_string()
        };
        let cases = [
            (529, body("overloaded_error", "Overloaded"), EgressError::Overloaded),
            (500, body("api_error", "Internal server error"), EgressError::Unavailable(500)),
            (504, body("timeout_error", "Request timeout"), EgressError::Unavailable(504)),
            (401, body("authentication_error", "invalid x-api-key"), EgressError::Unauthorized),
            (429, body("rate_limit_error", "Number of request tokens has exceeded"), EgressError::RateLimited),
            (
                400,
                body("invalid_request_error", "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits."),
                EgressError::Billing("Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.".into()),
            ),
            (402, body("billing_error", "payment required"), EgressError::Billing("payment required".into())),
            (404, body("not_found_error", "model: claude-old-1"), EgressError::ModelGone("model: claude-old-1".into())),
            (403, body("permission_error", "not allowed"), EgressError::Forbidden("not allowed".into())),
            (
                400,
                body("invalid_request_error", "messages: at least one message is required"),
                EgressError::InvalidRequest("messages: at least one message is required".into()),
            ),
            // No body the program can read (a proxy's page): the status decides.
            (529, "<html>busy</html>".to_owned(), EgressError::Overloaded),
            (503, String::new(), EgressError::Unavailable(503)),
            (404, String::new(), EgressError::ModelGone(String::new())),
            (403, String::new(), EgressError::Forbidden(String::new())),
            (402, String::new(), EgressError::Billing(String::new())),
            (418, body("teapot_error", "short and stout"), EgressError::Rejected(418, "short and stout".into())),
        ];
        for (status, text, want) in cases {
            assert_eq!(anthropic_error(status, &text), want, "{status} {text}");
        }
    }

    #[test]
    fn the_breaker_opens_after_busy_answers_in_a_row_and_lets_one_trial_through() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        for _ in 0..BREAKER_FAILURES - 1 {
            assert!(b.admit(t0).is_ok());
            b.record(Health::Busy, t0);
        }
        // An answer from a working service in between starts the count again.
        b.record(Health::Up, t0);
        for _ in 0..BREAKER_FAILURES - 1 {
            b.record(Health::Busy, t0);
        }
        assert!(b.admit(t0).is_ok(), "one short of the count");
        // Nothing about the service (no connection) neither counts nor clears.
        b.record(Health::Unknown, t0);
        b.record(Health::Busy, t0);
        assert_eq!(b.admit(t0), Err(EgressError::CircuitOpen(61)));
        let later = t0 + Duration::from_secs(30);
        assert_eq!(b.admit(later), Err(EgressError::CircuitOpen(31)));

        // Half-open: one trial, the rest wait for it.
        let after = t0 + BREAKER_COOL_DOWN;
        assert!(b.admit(after).is_ok());
        assert_eq!(b.admit(after), Err(EgressError::CircuitOpen(5)));
        // A busy trial opens it again for a whole cool-down.
        b.record(Health::Busy, after);
        assert!(matches!(
            b.admit(after + Duration::from_secs(59)),
            Err(EgressError::CircuitOpen(_))
        ));
        // A trial that told nothing lets the next one try; one that worked closes it.
        let again = after + BREAKER_COOL_DOWN;
        assert!(b.admit(again).is_ok());
        b.record(Health::Unknown, again);
        assert!(b.admit(again).is_ok());
        b.record(Health::Up, again);
        assert!(b.admit(again).is_ok() && b.admit(again).is_ok());
    }

    #[test]
    fn a_lost_trial_does_not_keep_the_breaker_shut() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        for _ in 0..BREAKER_FAILURES {
            b.record(Health::Busy, t0);
        }
        let after = t0 + BREAKER_COOL_DOWN;
        assert!(b.admit(after).is_ok());
        assert!(b.admit(after + BREAKER_TRIAL_LOST).is_ok());
    }

    #[test]
    fn what_each_outcome_says_about_the_service() {
        assert_eq!(Health::of(&Ok(json!({}))), Health::Up);
        assert_eq!(
            Health::of(&Err(EgressError::InvalidRequest("x".into()))),
            Health::Up
        );
        assert_eq!(
            Health::of(&Err(EgressError::Billing("x".into()))),
            Health::Up
        );
        assert_eq!(Health::of(&Err(EgressError::Overloaded)), Health::Busy);
        assert_eq!(
            Health::of(&Err(EgressError::Unavailable(502))),
            Health::Busy
        );
        assert_eq!(Health::of(&Err(EgressError::Offline)), Health::Unknown);
        assert_eq!(Health::of(&Err(EgressError::RateLimited)), Health::Unknown);
        assert_eq!(
            Health::of(&Err(EgressError::CircuitOpen(3))),
            Health::Unknown
        );
    }
}
