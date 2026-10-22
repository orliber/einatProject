//! Sign-in with Google as the forgotten-password path (D-041).
//!
//! When she turns it on, a random key is put in her own Google Drive, in the hidden folder
//! that only this app can see (`appDataFolder`, scope `drive.appdata`: the app sees none of
//! her other files). That key alone opens nothing: the vault's Google slot needs it together
//! with a second key sealed to her Windows account on this computer (see `dv_vault`). So a
//! broken-into Google account, even with a stolen backup, reads no case. When she forgets
//! the password she signs in with Google in her browser on her own computer, the key is read
//! back, and the vault opens so she can choose a new password.
//!
//! Rules, as for every other connection in this crate:
//! * Nothing from the vault ever leaves: only the key and a file name made of the vault's
//!   random id. No case, no name, no text.
//! * Only the hosts on [`GOOGLE_HOSTS`], HTTPS with TLS 1.3 and Mozilla's roots, no proxy, no
//!   redirects. The sign-in page itself opens in her browser, never inside the program.
//! * The access token lives in memory for one action and is revoked right after it. Nothing
//!   from Google is stored.
//! * OAuth for installed apps: a one-time listener on 127.0.0.1 for the answer, PKCE (S256)
//!   and a random `state`, so another program cannot slip its own answer in.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use zeroize::Zeroizing;

/// The only hosts the program itself talks to for this feature.
pub const GOOGLE_HOSTS: &[&str] = &["oauth2.googleapis.com", "www.googleapis.com"];
/// Opened in her browser (never fetched by the program).
pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
pub(crate) const FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";
/// Her account id (`openid`) and the app's own hidden Drive folder. Nothing else.
const SCOPES: &str = "openid https://www.googleapis.com/auth/drive.appdata";
const ISSUERS: &[&str] = &["https://accounts.google.com", "accounts.google.com"];
/// At most this many key files are tried (a few can exist after an interrupted switch).
const MAX_KEYS: usize = 5;
/// How long the program waits for her to finish signing in.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GoogleError {
    #[error("sign-in with Google is not set up in this version")]
    NotConfigured,
    #[error("the sign-in was cancelled")]
    Cancelled,
    #[error("the sign-in took too long")]
    TimedOut,
    #[error("no key for this vault in this Google account")]
    NoKey,
    #[error("no connection to Google")]
    Offline,
    #[error("the secure connection failed")]
    Tls,
    #[error("unexpected answer from Google: {0}")]
    Protocol(String),
    /// The lasting permission (a refresh token) was withdrawn or ran out: sign in again.
    #[error("the permission given to this app was withdrawn")]
    Revoked,
}

/// The app's registration with Google (a "Desktop app" OAuth client). Google treats the
/// secret of an installed app as public; it is set at build time, not kept in the repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub client_id: String,
    pub client_secret: String,
}

/// `None` when this build was made without a Google registration: the feature stays hidden.
#[must_use]
pub fn configured() -> Option<ClientConfig> {
    let id = option_env!("DV_GOOGLE_CLIENT_ID")?.trim();
    let secret = option_env!("DV_GOOGLE_CLIENT_SECRET")?.trim();
    (!id.is_empty() && !secret.is_empty()).then(|| ClientConfig {
        client_id: id.to_owned(),
        client_secret: secret.to_owned(),
    })
}

// ------------------------------------------------------------------ small encoders

pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], GoogleError> {
    let mut out = [0u8; N];
    aws_lc_rs::rand::fill(&mut out).map_err(|_| GoogleError::Protocol("random".to_owned()))?;
    Ok(out)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding (RFC 4648 §5), as PKCE and JWTs use it.
pub(crate) fn b64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    for chunk in bytes.chunks(3) {
        let at = |k: usize| u32::from(chunk.get(k).copied().unwrap_or(0));
        let n = (at(0) << 16) | (at(1) << 8) | at(2);
        for i in 0..=chunk.len() {
            // Six bits always name one of the 64 letters.
            let six = ((n >> (18 - 6 * i)) & 63) as usize;
            out.extend(B64.get(six).map(|&c| char::from(c)));
        }
    }
    out
}

fn b64url_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in text.trim_end_matches('=').bytes() {
        let v = B64.iter().position(|&a| a == c)?;
        acc = (acc << 6) | u32::try_from(v).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// `application/x-www-form-urlencoded`, built with the URL library reqwest already has.
fn form(pairs: &[(&str, &str)]) -> String {
    reqwest::Url::parse_with_params("http://form.invalid/", pairs)
        .ok()
        .and_then(|u| u.query().map(str::to_owned))
        .unwrap_or_default()
}

// ------------------------------------------------------------------ sign-in in the browser

/// A sign-in waiting for her browser: the address to open and the one-time listener.
pub struct SignIn {
    /// Closed on the first answer (or a refusal, a cancel, the timeout): one attempt only.
    listener: Option<TcpListener>,
    verifier: Zeroizing<String>,
    state: String,
    redirect: String,
    pub url: String,
}

impl std::fmt::Debug for SignIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignIn").finish_non_exhaustive()
    }
}

/// Start a sign-in: listen on a free port of 127.0.0.1 and build the Google address.
pub fn begin(cfg: &ClientConfig) -> Result<SignIn, GoogleError> {
    begin_with(cfg, false)
}

/// Start a sign-in that also asks for lasting permission (a refresh token), for the
/// encrypted backup copy that goes up after each backup without opening the browser (D-099).
/// The scope is the same: the app's hidden Drive folder and her account id, nothing else.
pub fn begin_lasting(cfg: &ClientConfig) -> Result<SignIn, GoogleError> {
    begin_with(cfg, true)
}

fn begin_with(cfg: &ClientConfig, lasting: bool) -> Result<SignIn, GoogleError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| GoogleError::Protocol(format!("listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| GoogleError::Protocol(format!("listener: {e}")))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = Zeroizing::new(b64url(&random_bytes::<32>()?));
    let challenge =
        b64url(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref());
    let state = b64url(&random_bytes::<16>()?);
    let mut params = vec![
        ("client_id", cfg.client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
        // Always ask which account, so she sees which one she is about to use.
        ("prompt", "select_account"),
    ];
    if lasting {
        params.push(("access_type", "offline"));
        // Google gives a refresh token again only when it asks for consent again.
        params.retain(|(k, _)| *k != "prompt");
        params.push(("prompt", "select_account consent"));
    }
    let url = reqwest::Url::parse_with_params(AUTH_URL, &params)
        .map_err(|e| GoogleError::Protocol(e.to_string()))?
        .to_string();
    Ok(SignIn {
        listener: Some(listener),
        verifier,
        state,
        redirect,
        url,
    })
}

/// What the browser brings back to the listener.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Code(String),
    Refused,
    /// Not the answer (a second tab, a favicon, someone else's request): keep waiting.
    Other,
}

/// Read the request line of one browser request and match it against `state`.
fn parse_answer(request_line: &str, state: &str) -> Answer {
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Answer::Other;
    }
    let Some(target) = parts.next() else {
        return Answer::Other;
    };
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Answer::Other;
    };
    let mut code = None;
    let mut got_state = None;
    let mut error = false;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => got_state = Some(v.into_owned()),
            "error" => error = true,
            _ => {}
        }
    }
    // Constant-time is not needed: `state` is single-use and only guards against mix-ups.
    if got_state.as_deref() != Some(state) {
        return Answer::Other;
    }
    match (code, error) {
        (Some(c), false) if !c.is_empty() => Answer::Code(c),
        _ => Answer::Refused,
    }
}

const PAGE_DONE: &str = "<!doctype html><html lang=\"he\" dir=\"rtl\"><meta charset=\"utf-8\"><title>כספת האבחון</title><body style=\"font-family:system-ui;text-align:center;padding:4em\"><h1>אפשר לחזור לכספת האבחון</h1><p>החלון הזה כבר לא נחוץ ואפשר לסגור אותו.</p></body></html>";

fn respond(mut stream: &TcpStream, body: &str) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn read_request_line(stream: &TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    let mut reader = stream;
    // Only the first line matters; it is short. Cap it.
    while buf.len() < 8 * 1024 {
        match reader.read(&mut byte) {
            Ok(1) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            _ => break,
        }
    }
    // The headers are read too, up to the empty line and within the same cap: a socket closed
    // with unread bytes is reset on macOS, and the browser then loses the page we send.
    let mut rest = 0usize;
    let mut line_len = 0usize;
    while rest < 8 * 1024 {
        match reader.read(&mut byte) {
            Ok(1) => {
                rest += 1;
                match byte[0] {
                    b'\n' if line_len == 0 => break,
                    b'\n' => line_len = 0,
                    b'\r' => {}
                    _ => line_len += 1,
                }
            }
            _ => break,
        }
    }
    String::from_utf8(buf).ok().map(|s| s.trim_end().to_owned())
}

/// Wait for her browser to come back with the sign-in answer. `cancel` stops the wait.
/// However it ends, the listener on 127.0.0.1 is closed before this returns, so nothing can
/// answer a second time.
pub fn wait(
    sign_in: &mut SignIn,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Zeroizing<String>, GoogleError> {
    let listener = sign_in
        .listener
        .take()
        .ok_or_else(|| GoogleError::Protocol("this sign-in was already answered".to_owned()))?;
    listen(&listener, &sign_in.state, cancel, timeout)
}

fn listen(
    listener: &TcpListener,
    state: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Zeroizing<String>, GoogleError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| GoogleError::Protocol(format!("listener: {e}")))?;
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(GoogleError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(GoogleError::TimedOut);
        }
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_nonblocking(false);
                let line = read_request_line(&stream).unwrap_or_default();
                match parse_answer(&line, state) {
                    Answer::Code(code) => {
                        respond(&stream, PAGE_DONE);
                        return Ok(Zeroizing::new(code));
                    }
                    Answer::Refused => {
                        respond(&stream, PAGE_DONE);
                        return Err(GoogleError::Cancelled);
                    }
                    Answer::Other => respond(&stream, ""),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(GoogleError::Protocol(format!("listener: {e}"))),
        }
    }
}

// ------------------------------------------------------------------ HTTP

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    /// Only the body of an encrypted backup copy, to the upload address Google gave (D-099).
    Put,
    Delete,
}

pub struct Request {
    pub method: Method,
    pub url: String,
    pub bearer: Option<Zeroizing<String>>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// Longer than the usual minute for an upload; `None` keeps the minute.
    pub timeout: Option<Duration>,
}

/// One answer: its status, its body (capped), and where it points (an upload's address).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub location: Option<String>,
}

impl Reply {
    #[must_use]
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            location: None,
        }
    }
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// One HTTP exchange. The real one is [`GoogleHttp`]; tests use a fake (no real API in tests).
pub trait Http: Send + Sync {
    fn send(&self, req: Request) -> Result<Reply, GoogleError>;
}

pub(crate) fn host_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(|h| GOOGLE_HOSTS.contains(&h))
}

/// The real connection: TLS 1.3, Mozilla's roots, HTTPS only, no proxy, no redirects.
pub struct GoogleHttp {
    client: reqwest::blocking::Client,
}

impl std::fmt::Debug for GoogleHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleHttp").finish_non_exhaustive()
    }
}

impl GoogleHttp {
    pub fn new() -> Result<Self, GoogleError> {
        let client = reqwest::blocking::Client::builder()
            .use_preconfigured_tls(crate::tls_config())
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(20))
            .user_agent("diagnostic-vault")
            .build()
            .map_err(|e| GoogleError::Protocol(e.to_string()))?;
        Ok(Self { client })
    }
}

impl Http for GoogleHttp {
    fn send(&self, req: Request) -> Result<Reply, GoogleError> {
        let url =
            reqwest::Url::parse(&req.url).map_err(|e| GoogleError::Protocol(e.to_string()))?;
        if !host_allowed(&url) {
            return Err(GoogleError::Protocol("host is not on the list".to_owned()));
        }
        let mut builder = match req.method {
            Method::Get => self.client.get(url),
            Method::Post => self.client.post(url),
            Method::Put => self.client.put(url),
            Method::Delete => self.client.delete(url),
        };
        if let Some(t) = req.timeout {
            builder = builder.timeout(t);
        }
        if let Some(token) = &req.bearer {
            builder = builder.header("authorization", format!("Bearer {}", token.as_str()));
        }
        if let Some(ct) = &req.content_type {
            builder = builder.header("content-type", ct.as_str());
        }
        let resp = builder.body(req.body).send().map_err(|e| {
            let text = format!("{e:?}").to_lowercase();
            if text.contains("certificate") || text.contains("tls") || text.contains("handshake") {
                GoogleError::Tls
            } else {
                GoogleError::Offline
            }
        })?;
        let status = resp.status().as_u16();
        // Only read here; the caller checks it (an upload address must stay on the list).
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        // Small answers only: a key file, a token, a file list.
        let mut body = Vec::new();
        resp.take(256 * 1024)
            .read_to_end(&mut body)
            .map_err(|_| GoogleError::Offline)?;
        Ok(Reply {
            status,
            body,
            location,
        })
    }
}

// ------------------------------------------------------------------ the account and the key

/// A signed-in account: the token (memory only) and the account's stable id.
pub struct Session {
    pub(crate) token: Zeroizing<String>,
    pub account_id: String,
    /// Only after [`begin_lasting`]: the lasting permission, for the caller to keep sealed.
    refresh: Option<Zeroizing<String>>,
}

impl Session {
    #[cfg(test)]
    pub(crate) fn for_tests(token: &str) -> Self {
        Self {
            token: Zeroizing::new(token.to_owned()),
            account_id: "1".to_owned(),
            refresh: None,
        }
    }

    /// The lasting permission Google gave, if it gave one. Taken once.
    pub fn take_refresh_token(&mut self) -> Option<Zeroizing<String>> {
        self.refresh.take()
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

pub(crate) fn json(status: u16, body: &[u8]) -> Result<Value, GoogleError> {
    if !(200..300).contains(&status) {
        return Err(GoogleError::Protocol(format!("status {status}")));
    }
    serde_json::from_slice(body).map_err(|e| GoogleError::Protocol(e.to_string()))
}

/// The account id from the ID token. It came straight from Google's token endpoint over TLS,
/// so its signature need not be checked (OpenID Connect Core §3.1.3.7); its issuer and
/// audience still are.
fn account_from_id_token(id_token: &str, client_id: &str) -> Result<String, GoogleError> {
    let payload = id_token
        .split('.')
        .nth(1)
        .and_then(b64url_decode)
        .ok_or_else(|| GoogleError::Protocol("id token".to_owned()))?;
    let claims: Value =
        serde_json::from_slice(&payload).map_err(|e| GoogleError::Protocol(e.to_string()))?;
    let iss = claims.get("iss").and_then(Value::as_str).unwrap_or("");
    let aud = claims.get("aud").and_then(Value::as_str).unwrap_or("");
    if !ISSUERS.contains(&iss) || aud != client_id {
        return Err(GoogleError::Protocol(
            "id token issuer or audience".to_owned(),
        ));
    }
    claims
        .get("sub")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| GoogleError::Protocol("id token has no account".to_owned()))
}

/// Trade the browser's one-time code for a token (PKCE).
pub fn finish(
    http: &dyn Http,
    cfg: &ClientConfig,
    sign_in: &SignIn,
    code: &str,
) -> Result<Session, GoogleError> {
    let body = form(&[
        ("code", code),
        ("client_id", &cfg.client_id),
        ("client_secret", &cfg.client_secret),
        ("redirect_uri", &sign_in.redirect),
        ("grant_type", "authorization_code"),
        ("code_verifier", &sign_in.verifier),
    ]);
    let Reply {
        status,
        body: bytes,
        ..
    } = http.send(Request {
        method: Method::Post,
        url: TOKEN_URL.to_owned(),
        bearer: None,
        content_type: Some("application/x-www-form-urlencoded".to_owned()),
        body: body.into_bytes(),
        timeout: None,
    })?;
    let v = json(status, &bytes)?;
    let token = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| GoogleError::Protocol("no access token".to_owned()))?;
    let id_token = v
        .get("id_token")
        .and_then(Value::as_str)
        .ok_or_else(|| GoogleError::Protocol("no id token".to_owned()))?;
    let refresh = v
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(|t| Zeroizing::new(t.to_owned()));
    Ok(Session {
        token: Zeroizing::new(token.to_owned()),
        account_id: account_from_id_token(id_token, &cfg.client_id)?,
        refresh,
    })
}

/// A short-lived token from the lasting permission, without the browser (D-099). The account
/// id is not asked for again (empty): the permission was checked against the vault's account
/// when it was given. A withdrawn permission is [`GoogleError::Revoked`].
pub fn session_from_refresh(
    http: &dyn Http,
    cfg: &ClientConfig,
    refresh_token: &str,
) -> Result<Session, GoogleError> {
    let body = Zeroizing::new(form(&[
        ("client_id", &cfg.client_id),
        ("client_secret", &cfg.client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ]));
    let Reply {
        status,
        body: bytes,
        ..
    } = http.send(Request {
        method: Method::Post,
        url: TOKEN_URL.to_owned(),
        bearer: None,
        content_type: Some("application/x-www-form-urlencoded".to_owned()),
        body: body.as_bytes().to_vec(),
        timeout: None,
    })?;
    // RFC 6749 §5.2: a withdrawn or expired grant answers 400 `invalid_grant`.
    if status == 400 || status == 401 {
        let withdrawn = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_owned))
            .is_some_and(|e| e == "invalid_grant" || e == "unauthorized_client");
        if withdrawn {
            return Err(GoogleError::Revoked);
        }
    }
    let v = json(status, &bytes)?;
    let token = v
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| GoogleError::Protocol("no access token".to_owned()))?;
    Ok(Session {
        token: Zeroizing::new(token.to_owned()),
        account_id: String::new(),
        refresh: None,
    })
}

/// The key file's name: only the vault's random id, nothing about her or a case.
#[must_use]
pub fn key_file_name(vault_id: &str) -> String {
    let safe: String = vault_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    format!("diagnostic-vault-{safe}.key")
}

/// The ids of this vault's key files in the app's hidden Drive folder, newest first.
pub fn find_keys(http: &dyn Http, s: &Session, vault_id: &str) -> Result<Vec<String>, GoogleError> {
    let q = format!("name = '{}' and trashed = false", key_file_name(vault_id));
    let url = reqwest::Url::parse_with_params(
        FILES_URL,
        &[
            ("spaces", "appDataFolder"),
            ("q", q.as_str()),
            ("fields", "files(id)"),
            ("orderBy", "createdTime desc"),
            ("pageSize", "10"),
        ],
    )
    .map_err(|e| GoogleError::Protocol(e.to_string()))?;
    let Reply {
        status,
        body: bytes,
        ..
    } = http.send(Request {
        method: Method::Get,
        url: url.to_string(),
        bearer: Some(s.token.clone()),
        content_type: None,
        body: Vec::new(),
        timeout: None,
    })?;
    let v = json(status, &bytes)?;
    Ok(v.get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(|f| f.get("id").and_then(Value::as_str))
                .filter(|id| {
                    !id.is_empty()
                        && id
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                })
                .take(MAX_KEYS)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default())
}

/// The content of one key file.
pub fn read_key(
    http: &dyn Http,
    s: &Session,
    file_id: &str,
) -> Result<Zeroizing<String>, GoogleError> {
    let Reply {
        status,
        body: bytes,
        ..
    } = http.send(Request {
        method: Method::Get,
        url: format!("{FILES_URL}/{file_id}?alt=media"),
        bearer: Some(s.token.clone()),
        content_type: None,
        body: Vec::new(),
        timeout: None,
    })?;
    if !(200..300).contains(&status) {
        return Err(GoogleError::Protocol(format!("status {status}")));
    }
    let bytes = Zeroizing::new(bytes);
    String::from_utf8(bytes.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| GoogleError::Protocol("key file is not text".to_owned()))
}

/// Write a new key file into the app's hidden Drive folder. Returns its id.
pub fn put_key(
    http: &dyn Http,
    s: &Session,
    vault_id: &str,
    key_text: &str,
) -> Result<String, GoogleError> {
    let boundary = format!("dv-{}", b64url(&random_bytes::<12>()?));
    let meta = serde_json::json!({
        "name": key_file_name(vault_id),
        "parents": ["appDataFolder"],
    });
    let body = Zeroizing::new(format!(
        "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: text/plain\r\n\r\n{key_text}\r\n--{boundary}--\r\n"
    ));
    let Reply {
        status,
        body: bytes,
        ..
    } = http.send(Request {
        method: Method::Post,
        url: format!("{UPLOAD_URL}&fields=id"),
        bearer: Some(s.token.clone()),
        content_type: Some(format!("multipart/related; boundary={boundary}")),
        body: body.as_bytes().to_vec(),
        timeout: None,
    })?;
    let v = json(status, &bytes)?;
    v.get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| GoogleError::Protocol("no file id".to_owned()))
}

/// Delete one key file (permanently, not to the Drive bin).
pub fn delete_key(http: &dyn Http, s: &Session, file_id: &str) -> Result<(), GoogleError> {
    delete_file(http, s, file_id)
}

/// Delete one file of the app's hidden folder (permanently, not to the Drive bin).
pub(crate) fn delete_file(http: &dyn Http, s: &Session, file_id: &str) -> Result<(), GoogleError> {
    let Reply { status, .. } = http.send(Request {
        method: Method::Delete,
        url: format!("{FILES_URL}/{file_id}"),
        bearer: Some(s.token.clone()),
        content_type: None,
        body: Vec::new(),
        timeout: None,
    })?;
    if (200..300).contains(&status) || status == 404 {
        Ok(())
    } else {
        Err(GoogleError::Protocol(format!("status {status}")))
    }
}

/// End the session at Google: the token stops working at once. Best effort.
pub fn revoke(http: &dyn Http, s: Session) {
    revoke_token(http, &s.token);
}

/// Withdraw a token at Google, a lasting permission included (D-099). Best effort.
pub fn revoke_token(http: &dyn Http, token: &str) {
    let body = form(&[("token", token)]);
    let _ = http.send(Request {
        method: Method::Post,
        url: REVOKE_URL.to_owned(),
        bearer: None,
        content_type: Some("application/x-www-form-urlencoded".to_owned()),
        body: body.into_bytes(),
        timeout: None,
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn cfg() -> ClientConfig {
        ClientConfig {
            client_id: "client-1.apps.googleusercontent.com".to_owned(),
            client_secret: "public-secret".to_owned(),
        }
    }

    fn id_token(iss: &str, aud: &str, sub: &str) -> String {
        let claims = serde_json::json!({"iss": iss, "aud": aud, "sub": sub});
        format!("e30.{}.sig", b64url(claims.to_string().as_bytes()))
    }

    /// Method, URL, bearer token and body of one request.
    type Seen = (Method, String, Option<String>, Vec<u8>);

    /// A fake Google: answers in order, and remembers every request.
    struct Fake {
        answers: Mutex<Vec<(u16, Vec<u8>)>>,
        seen: Mutex<Vec<Seen>>,
    }

    impl Fake {
        fn new(answers: Vec<(u16, Vec<u8>)>) -> Self {
            Self {
                answers: Mutex::new(answers.into_iter().rev().collect()),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl Http for Fake {
        fn send(&self, req: Request) -> Result<Reply, GoogleError> {
            let url = reqwest::Url::parse(&req.url).unwrap();
            assert!(host_allowed(&url), "{}", req.url);
            self.seen.lock().unwrap().push((
                req.method,
                req.url.clone(),
                req.bearer.as_ref().map(|t| t.to_string()),
                req.body.clone(),
            ));
            let (status, body) = self.answers.lock().unwrap().pop().unwrap();
            Ok(Reply::new(status, body))
        }
    }

    #[test]
    fn base64url_round_trips() {
        for n in 0..40u8 {
            let bytes: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(b64url_decode(&b64url(&bytes)).unwrap(), bytes);
        }
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = b64url(
            aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref(),
        );
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn the_sign_in_address_asks_for_the_hidden_folder_only_with_pkce() {
        let s = begin(&cfg()).unwrap();
        let url = reqwest::Url::parse(&s.url).unwrap();
        assert!(s.url.starts_with(AUTH_URL));
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], SCOPES);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], s.state);
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(!s.url.contains(s.verifier.as_str()));
    }

    #[test]
    fn only_the_answer_with_our_state_counts() {
        assert_eq!(
            parse_answer("GET /?state=abc&code=4%2F0X HTTP/1.1", "abc"),
            Answer::Code("4/0X".to_owned())
        );
        assert_eq!(
            parse_answer("GET /?state=zzz&code=1 HTTP/1.1", "abc"),
            Answer::Other
        );
        assert_eq!(
            parse_answer("GET /favicon.ico HTTP/1.1", "abc"),
            Answer::Other
        );
        assert_eq!(
            parse_answer("POST /?state=abc&code=1 HTTP/1.1", "abc"),
            Answer::Other
        );
        assert_eq!(
            parse_answer("GET /?state=abc&error=access_denied HTTP/1.1", "abc"),
            Answer::Refused
        );
    }

    #[test]
    fn the_browser_answer_reaches_the_listener() {
        let mut s = begin(&cfg()).unwrap();
        let redirect = s.redirect.clone();
        let state = s.state.clone();
        let browser = std::thread::spawn(move || {
            let addr = redirect.trim_start_matches("http://");
            // A stray request first, then the real answer.
            for path in [
                "/favicon.ico".to_owned(),
                format!("/?state={state}&code=the-code"),
            ] {
                let mut c = TcpStream::connect(addr).unwrap();
                c.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .unwrap();
                let mut out = String::new();
                let _ = c.read_to_string(&mut out);
            }
        });
        let code = wait(&mut s, &AtomicBool::new(false), Duration::from_secs(10)).unwrap();
        browser.join().unwrap();
        assert_eq!(code.as_str(), "the-code");
        // One attempt: the port is closed once the answer is in, and a second wait refuses.
        let addr = s.redirect.trim_start_matches("http://").to_owned();
        assert!(TcpStream::connect(&addr).is_err(), "the listener is closed");
        assert!(wait(&mut s, &AtomicBool::new(false), Duration::from_secs(1)).is_err());
    }

    #[test]
    fn a_cancelled_wait_stops() {
        let mut s = begin(&cfg()).unwrap();
        assert_eq!(
            wait(&mut s, &AtomicBool::new(true), Duration::from_secs(10)).unwrap_err(),
            GoogleError::Cancelled
        );
    }

    #[test]
    fn the_code_becomes_a_session_with_the_account_id() {
        let s = begin(&cfg()).unwrap();
        let answer = serde_json::json!({
            "access_token": "ya29.token",
            "id_token": id_token("https://accounts.google.com", &cfg().client_id, "1234567890"),
        });
        let fake = Fake::new(vec![(200, answer.to_string().into_bytes())]);
        let session = finish(&fake, &cfg(), &s, "the-code").unwrap();
        assert_eq!(session.account_id, "1234567890");
        let seen = fake.seen.lock().unwrap();
        let body = String::from_utf8(seen[0].3.clone()).unwrap();
        assert!(body.contains("code_verifier="));
        assert!(body.contains("grant_type=authorization_code"));
    }

    #[test]
    fn an_id_token_for_another_app_is_refused() {
        assert!(account_from_id_token(
            &id_token("https://accounts.google.com", "other-app", "1"),
            &cfg().client_id
        )
        .is_err());
        assert!(account_from_id_token(
            &id_token("https://evil.example", &cfg().client_id, "1"),
            &cfg().client_id
        )
        .is_err());
        assert!(account_from_id_token("garbage", &cfg().client_id).is_err());
    }

    #[test]
    fn the_key_goes_to_the_hidden_folder_and_comes_back() {
        let session = Session {
            token: Zeroizing::new("ya29.token".to_owned()),
            account_id: "1".to_owned(),
            refresh: None,
        };
        let fake = Fake::new(vec![
            (200, br#"{"id":"file-1"}"#.to_vec()),
            (
                200,
                br#"{"files":[{"id":"file-1"},{"id":"bad id'"}]}"#.to_vec(),
            ),
            (200, b"00ff".to_vec()),
            (204, Vec::new()),
            (200, Vec::new()),
        ]);
        assert_eq!(put_key(&fake, &session, "V1", "00ff").unwrap(), "file-1");
        assert_eq!(find_keys(&fake, &session, "V1").unwrap(), vec!["file-1"]);
        assert_eq!(
            read_key(&fake, &session, "file-1").unwrap().as_str(),
            "00ff"
        );
        delete_key(&fake, &session, "file-1").unwrap();
        revoke(&fake, session);

        let seen = fake.seen.lock().unwrap();
        let upload = String::from_utf8(seen[0].3.clone()).unwrap();
        assert!(upload.contains(r#""parents":["appDataFolder"]"#));
        assert!(upload.contains("diagnostic-vault-V1.key"));
        assert!(seen[1].1.contains("spaces=appDataFolder"));
        assert_eq!(seen[3].0, Method::Delete);
        assert_eq!(seen[2].2.as_deref(), Some("ya29.token"));
        // The revoke carries the token in its body, not as a bearer.
        assert!(seen[4].1.starts_with(REVOKE_URL));
        assert_eq!(seen[4].2, None);
    }

    #[test]
    fn an_empty_file_id_is_not_a_key() {
        // It used to pass the id check: "files/?alt=media" is the file list, not a key file.
        let session = Session {
            token: Zeroizing::new("ya29.token".to_owned()),
            account_id: "1".to_owned(),
            refresh: None,
        };
        let fake = Fake::new(vec![(
            200,
            br#"{"files":[{"id":""},{"id":"file-1"}]}"#.to_vec(),
        )]);
        assert_eq!(find_keys(&fake, &session, "V1").unwrap(), vec!["file-1"]);
    }

    #[test]
    fn only_google_hosts_over_https() {
        let ok = |u: &str| reqwest::Url::parse(u).is_ok_and(|u| host_allowed(&u));
        assert!(ok("https://www.googleapis.com/drive/v3/files"));
        assert!(ok("https://oauth2.googleapis.com/token"));
        assert!(!ok("http://www.googleapis.com/drive/v3/files"));
        assert!(!ok("https://accounts.google.com/o/oauth2/v2/auth"));
        assert!(!ok("https://api.anthropic.com/v1/messages"));
    }

    #[test]
    fn file_names_carry_only_the_vault_id() {
        assert_eq!(key_file_name("a1B2"), "diagnostic-vault-a1B2.key");
        assert_eq!(
            key_file_name("x' or name != 'y"),
            "diagnostic-vault-xornamey.key"
        );
    }

    // ---------------------------------------------------------- the sign-in address and answer

    #[test]
    fn the_sign_in_address_carries_exactly_the_expected_parameters_and_no_secret() {
        let s = begin(&cfg()).unwrap();
        let url = reqwest::Url::parse(&s.url).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        let mut names: Vec<&str> = q.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "client_id",
                "code_challenge",
                "code_challenge_method",
                "prompt",
                "redirect_uri",
                "response_type",
                "scope",
                "state"
            ]
        );
        assert!(
            !s.url.contains(&cfg().client_secret),
            "the secret stays out of the browser"
        );
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["prompt"], "select_account");
        // PKCE: the challenge is the hash of a verifier of the length RFC 7636 asks for.
        assert_eq!(s.verifier.len(), 43);
        assert!(s.verifier.bytes().all(|b| B64.contains(&b)));
        let challenge = b64url(
            aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, s.verifier.as_bytes()).as_ref(),
        );
        assert_eq!(q["code_challenge"], challenge);
        // The answer comes back to the listener itself, on the loopback address only.
        let addr = s.listener.as_ref().unwrap().local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        assert_eq!(
            q["redirect_uri"],
            format!("http://127.0.0.1:{}", addr.port())
        );
        assert_eq!(s.redirect, q["redirect_uri"]);
    }

    #[test]
    fn every_sign_in_gets_a_fresh_state_and_verifier() {
        let a = begin(&cfg()).unwrap();
        let b = begin(&cfg()).unwrap();
        assert_ne!(a.state, b.state);
        assert_ne!(a.verifier.as_str(), b.verifier.as_str());
        assert!(a.state.len() >= 22, "128 random bits");
    }

    #[test]
    fn answers_without_our_state_or_with_an_error_never_yield_a_code() {
        let cases = [
            ("GET /?code=1 HTTP/1.1", Answer::Other),
            ("GET /?state=&code=1 HTTP/1.1", Answer::Other),
            ("GET /?state=ab&code=1 HTTP/1.1", Answer::Other),
            ("GET /?state=abcd&code=1 HTTP/1.1", Answer::Other),
            ("GET /?state=ABC&code=1 HTTP/1.1", Answer::Other),
            ("GET /?state=abc HTTP/1.1", Answer::Refused),
            ("GET /?state=abc&code= HTTP/1.1", Answer::Refused),
            ("GET /?state=abc&code=1&error=x HTTP/1.1", Answer::Refused),
            ("HEAD /?state=abc&code=1 HTTP/1.1", Answer::Other),
            ("GET", Answer::Other),
            ("", Answer::Other),
            ("\u{0}\u{1}garbage", Answer::Other),
        ];
        for (line, want) in cases {
            assert_eq!(parse_answer(line, "abc"), want, "{line:?}");
        }
    }

    #[test]
    fn a_wait_without_an_answer_times_out() {
        let mut s = begin(&cfg()).unwrap();
        let started = Instant::now();
        assert_eq!(
            wait(&mut s, &AtomicBool::new(false), Duration::from_millis(250)).unwrap_err(),
            GoogleError::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_wrong_state_is_answered_empty_and_a_refusal_ends_the_wait() {
        let mut s = begin(&cfg()).unwrap();
        let addr = s.redirect.trim_start_matches("http://").to_owned();
        let state = s.state.clone();
        let browser = std::thread::spawn(move || {
            let mut pages = Vec::new();
            for path in [
                "/?state=someone-else&code=stolen".to_owned(),
                format!("/?state={state}&error=access_denied"),
            ] {
                let mut c = TcpStream::connect(&addr).unwrap();
                c.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .unwrap();
                let mut out = String::new();
                let _ = c.read_to_string(&mut out);
                pages.push(out);
            }
            pages
        });
        let got = wait(&mut s, &AtomicBool::new(false), Duration::from_secs(10));
        let pages = browser.join().unwrap();
        assert_eq!(got.unwrap_err(), GoogleError::Cancelled);
        assert!(
            pages[0].ends_with("\r\n\r\n"),
            "an empty page for a stranger"
        );
        assert!(pages[1].contains("no-store") && pages[1].contains("<h1>"));
    }

    // ---------------------------------------------------------- what the program sends

    #[test]
    fn a_host_off_the_list_is_refused_before_any_connection() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let http = GoogleHttp::new().unwrap();
        for url in [
            format!("https://127.0.0.1:{port}/token"),
            format!("http://127.0.0.1:{port}/token"),
            format!("https://localhost:{port}/token"),
            "http://oauth2.googleapis.com/token".to_owned(),
            AUTH_URL.to_owned(),
            "https://oauth2.googleapis.com.evil.example/token".to_owned(),
            "https://evil.example/?x=https://oauth2.googleapis.com/".to_owned(),
            "not a url".to_owned(),
        ] {
            let got = http.send(Request {
                method: Method::Post,
                url: url.clone(),
                bearer: Some(Zeroizing::new("ya29.token".to_owned())),
                content_type: None,
                body: b"secret".to_vec(),
                timeout: None,
            });
            assert!(
                matches!(got, Err(GoogleError::Protocol(_))),
                "{url}: {got:?}"
            );
        }
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "a refused request reached the wire"
        );
    }

    #[test]
    fn the_token_exchange_sends_this_sign_ins_verifier_and_redirect_without_a_bearer() {
        let s = begin(&cfg()).unwrap();
        let answer = serde_json::json!({
            "access_token": "ya29.token",
            "id_token": id_token("accounts.google.com", &cfg().client_id, "42"),
        });
        let fake = Fake::new(vec![(200, answer.to_string().into_bytes())]);
        finish(&fake, &cfg(), &s, "the-code").unwrap();
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].0, Method::Post);
        assert_eq!(seen[0].1, TOKEN_URL);
        assert_eq!(seen[0].2, None);
        let body = String::from_utf8(seen[0].3.clone()).unwrap();
        let pairs: std::collections::HashMap<String, String> =
            reqwest::Url::parse(&format!("http://x.invalid/?{body}"))
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
        assert_eq!(pairs["code_verifier"], s.verifier.as_str());
        assert_eq!(pairs["redirect_uri"], s.redirect);
        assert_eq!(pairs["code"], "the-code");
    }

    #[test]
    fn a_failed_or_incomplete_token_answer_is_no_session() {
        let s = begin(&cfg()).unwrap();
        let good_id = id_token("https://accounts.google.com", &cfg().client_id, "42");
        let answers = [
            (400, serde_json::json!({"error": "invalid_grant"})),
            (
                500,
                serde_json::json!({"access_token": "t", "id_token": good_id}),
            ),
            (200, serde_json::json!({"id_token": good_id})),
            (200, serde_json::json!({"access_token": "t"})),
            (
                200,
                serde_json::json!({"access_token": "t", "id_token": id_token("https://accounts.google.com", &cfg().client_id, "")}),
            ),
            (
                200,
                serde_json::json!({"access_token": "t", "id_token": id_token("https://accounts.google.com.evil.example", &cfg().client_id, "1")}),
            ),
        ];
        for (status, body) in answers {
            let fake = Fake::new(vec![(status, body.to_string().into_bytes())]);
            assert!(finish(&fake, &cfg(), &s, "c").is_err(), "{status} {body}");
        }
        let fake = Fake::new(vec![(200, b"<html>".to_vec())]);
        assert!(finish(&fake, &cfg(), &s, "c").is_err());
    }

    #[test]
    fn key_lists_are_capped_and_hold_only_safe_ids() {
        let session = Session {
            token: Zeroizing::new("ya29.token".to_owned()),
            account_id: "1".to_owned(),
            refresh: None,
        };
        let files: Vec<serde_json::Value> = [
            "a/../b", "ok-1", "x?y", "ok_2", "", "ok3", "ok4", "ok5", "ok6",
        ]
        .iter()
        .map(|id| serde_json::json!({"id": id}))
        .collect();
        let fake = Fake::new(vec![(
            200,
            serde_json::json!({ "files": files })
                .to_string()
                .into_bytes(),
        )]);
        let ids = find_keys(&fake, &session, "V1").unwrap();
        assert_eq!(ids, ["ok-1", "ok_2", "ok3", "ok4", "ok5"]);
        let fake = Fake::new(vec![(403, Vec::new())]);
        assert!(find_keys(&fake, &session, "V1").is_err());
        let fake = Fake::new(vec![(500, Vec::new())]);
        assert!(delete_key(&fake, &session, "ok-1").is_err());
        let fake = Fake::new(vec![(404, Vec::new())]);
        assert!(
            delete_key(&fake, &session, "ok-1").is_ok(),
            "already gone is fine"
        );
        let fake = Fake::new(vec![(200, vec![0xff, 0xfe])]);
        assert!(
            read_key(&fake, &session, "ok-1").is_err(),
            "a key file is text"
        );
    }

    #[test]
    fn the_upload_names_its_own_boundary_and_carries_only_the_name_and_the_key() {
        let session = Session {
            token: Zeroizing::new("ya29.token".to_owned()),
            account_id: "1".to_owned(),
            refresh: None,
        };
        let fake = Fake::new(vec![(200, br#"{"id":"f"}"#.to_vec())]);
        put_key(&fake, &session, "V1", "00ff").unwrap();
        let seen = fake.seen.lock().unwrap();
        let body = String::from_utf8(seen[0].3.clone()).unwrap();
        let boundary = body
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("--")
            .to_owned();
        assert!(boundary.starts_with("dv-"));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
        let meta_line = body.lines().find(|l| l.starts_with('{')).unwrap();
        let meta: serde_json::Value = serde_json::from_str(meta_line).unwrap();
        assert_eq!(
            meta,
            serde_json::json!({"name": "diagnostic-vault-V1.key", "parents": ["appDataFolder"]})
        );
    }

    // ---------------------------------------------------------- lasting permission (D-099)

    #[test]
    fn a_lasting_sign_in_asks_for_offline_access_and_the_same_scope() {
        let s = begin_lasting(&cfg()).unwrap();
        let url = reqwest::Url::parse(&s.url).unwrap();
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], SCOPES);
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "select_account consent");
        assert_eq!(url.query_pairs().filter(|(k, _)| k == "prompt").count(), 1);
        // The usual sign-in never asks for it.
        let plain = begin(&cfg()).unwrap();
        assert!(!plain.url.contains("access_type"));
    }

    #[test]
    fn the_refresh_token_is_taken_once_and_trades_for_a_short_token() {
        let s = begin_lasting(&cfg()).unwrap();
        let answer = serde_json::json!({
            "access_token": "ya29.token",
            "refresh_token": "1//lasting",
            "id_token": id_token("https://accounts.google.com", &cfg().client_id, "42"),
        });
        let fake = Fake::new(vec![(200, answer.to_string().into_bytes())]);
        let mut session = finish(&fake, &cfg(), &s, "code").unwrap();
        assert_eq!(session.take_refresh_token().unwrap().as_str(), "1//lasting");
        assert!(session.take_refresh_token().is_none());

        let fake = Fake::new(vec![(
            200,
            br#"{"access_token":"ya29.short","expires_in":3599}"#.to_vec(),
        )]);
        let short = session_from_refresh(&fake, &cfg(), "1//lasting").unwrap();
        assert_eq!(short.token.as_str(), "ya29.short");
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].1, TOKEN_URL);
        assert_eq!(seen[0].2, None, "the lasting token goes in the body only");
        let body = String::from_utf8(seen[0].3.clone()).unwrap();
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains("refresh_token=1%2F%2Flasting"));
    }

    #[test]
    fn a_withdrawn_permission_is_told_apart() {
        let fake = Fake::new(vec![(400, br#"{"error":"invalid_grant"}"#.to_vec())]);
        assert_eq!(
            session_from_refresh(&fake, &cfg(), "t").unwrap_err(),
            GoogleError::Revoked
        );
        let fake = Fake::new(vec![(500, Vec::new())]);
        assert!(matches!(
            session_from_refresh(&fake, &cfg(), "t"),
            Err(GoogleError::Protocol(_))
        ));
        let fake = Fake::new(vec![(200, br#"{"access_token":""}"#.to_vec())]);
        assert!(session_from_refresh(&fake, &cfg(), "t").is_err());
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(128))]

        /// Only an answer that carries exactly our state can ever give a code, and then the
        /// code comes back as it was sent.
        #[test]
        fn only_our_exact_state_yields_the_code(
            ours in "[A-Za-z0-9_-]{1,24}",
            theirs in "[A-Za-z0-9_-]{0,24}",
            code in "[A-Za-z0-9/._~-]{1,40}",
        ) {
            let enc = |s: &str| form(&[("v", s)]).strip_prefix("v=").unwrap_or_default().to_owned();
            let line = format!("GET /?state={}&code={} HTTP/1.1", enc(&theirs), enc(&code));
            let got = parse_answer(&line, &ours);
            if theirs == ours {
                proptest::prop_assert_eq!(got, Answer::Code(code));
            } else {
                proptest::prop_assert_eq!(got, Answer::Other);
            }
        }

        /// No file id, however strange, can move a key request to another host.
        #[test]
        fn a_file_id_never_changes_the_host(id in "\\PC{0,30}") {
            let session = Session {
                token: Zeroizing::new("ya29.token".to_owned()),
                account_id: "1".to_owned(),
            refresh: None,
            };
            let fake = Fake::new(vec![(200, b"k".to_vec()), (204, Vec::new())]);
            let _ = read_key(&fake, &session, &id);
            let _ = delete_key(&fake, &session, &id);
            for (_, url, _, _) in fake.seen.lock().unwrap().iter() {
                let parsed = reqwest::Url::parse(url).unwrap();
                proptest::prop_assert_eq!(parsed.host_str(), Some("www.googleapis.com"));
                proptest::prop_assert_eq!(parsed.scheme(), "https");
            }
        }

        /// The file name, and so the Drive query built from it, holds letters and digits of
        /// the vault id only.
        #[test]
        fn key_file_names_hold_only_letters_and_digits(id in "\\PC{0,40}") {
            let name = key_file_name(&id);
            let middle = name
                .strip_prefix("diagnostic-vault-")
                .and_then(|n| n.strip_suffix(".key"))
                .unwrap();
            proptest::prop_assert!(middle.chars().all(|c| c.is_ascii_alphanumeric()));
        }

        #[test]
        fn base64url_round_trips_and_decoding_garbage_never_panics(
            bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..100),
            garbage in "\\PC{0,60}",
        ) {
            let text = b64url(&bytes);
            proptest::prop_assert!(!text.contains('=') && !text.contains('+') && !text.contains('/'));
            proptest::prop_assert_eq!(b64url_decode(&text), Some(bytes));
            let _ = b64url_decode(&garbage);
        }
    }
}
