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
const FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
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

fn random_bytes<const N: usize>() -> Result<[u8; N], GoogleError> {
    let mut out = [0u8; N];
    aws_lc_rs::rand::fill(&mut out).map_err(|_| GoogleError::Protocol("random".to_owned()))?;
    Ok(out)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding (RFC 4648 §5), as PKCE and JWTs use it.
fn b64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..=chunk.len() {
            out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
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
    listener: TcpListener,
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
    let url = reqwest::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", cfg.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            // Always ask which account, so she sees which one she is about to use.
            ("prompt", "select_account"),
        ],
    )
    .map_err(|e| GoogleError::Protocol(e.to_string()))?
    .to_string();
    Ok(SignIn {
        listener,
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
    String::from_utf8(buf).ok().map(|s| s.trim_end().to_owned())
}

/// Wait for her browser to come back with the sign-in answer. `cancel` stops the wait.
pub fn wait(
    sign_in: &SignIn,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Zeroizing<String>, GoogleError> {
    sign_in
        .listener
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
        match sign_in.listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_nonblocking(false);
                let line = read_request_line(&stream).unwrap_or_default();
                match parse_answer(&line, &sign_in.state) {
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
    Delete,
}

pub struct Request {
    pub method: Method,
    pub url: String,
    pub bearer: Option<Zeroizing<String>>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
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
    fn send(&self, req: Request) -> Result<(u16, Vec<u8>), GoogleError>;
}

fn host_allowed(url: &reqwest::Url) -> bool {
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
    fn send(&self, req: Request) -> Result<(u16, Vec<u8>), GoogleError> {
        let url =
            reqwest::Url::parse(&req.url).map_err(|e| GoogleError::Protocol(e.to_string()))?;
        if !host_allowed(&url) {
            return Err(GoogleError::Protocol("host is not on the list".to_owned()));
        }
        let mut builder = match req.method {
            Method::Get => self.client.get(url),
            Method::Post => self.client.post(url),
            Method::Delete => self.client.delete(url),
        };
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
        // Small answers only: a key file, a token, a file list.
        let mut body = Vec::new();
        resp.take(256 * 1024)
            .read_to_end(&mut body)
            .map_err(|_| GoogleError::Offline)?;
        Ok((status, body))
    }
}

// ------------------------------------------------------------------ the account and the key

/// A signed-in account: the token (memory only) and the account's stable id.
pub struct Session {
    token: Zeroizing<String>,
    pub account_id: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

fn json(status: u16, body: &[u8]) -> Result<Value, GoogleError> {
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
    let (status, bytes) = http.send(Request {
        method: Method::Post,
        url: TOKEN_URL.to_owned(),
        bearer: None,
        content_type: Some("application/x-www-form-urlencoded".to_owned()),
        body: body.into_bytes(),
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
    Ok(Session {
        token: Zeroizing::new(token.to_owned()),
        account_id: account_from_id_token(id_token, &cfg.client_id)?,
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
    let (status, bytes) = http.send(Request {
        method: Method::Get,
        url: url.to_string(),
        bearer: Some(s.token.clone()),
        content_type: None,
        body: Vec::new(),
    })?;
    let v = json(status, &bytes)?;
    Ok(v.get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(|f| f.get("id").and_then(Value::as_str))
                .filter(|id| {
                    id.chars()
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
    let (status, bytes) = http.send(Request {
        method: Method::Get,
        url: format!("{FILES_URL}/{file_id}?alt=media"),
        bearer: Some(s.token.clone()),
        content_type: None,
        body: Vec::new(),
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
    let (status, bytes) = http.send(Request {
        method: Method::Post,
        url: format!("{UPLOAD_URL}&fields=id"),
        bearer: Some(s.token.clone()),
        content_type: Some(format!("multipart/related; boundary={boundary}")),
        body: body.as_bytes().to_vec(),
    })?;
    let v = json(status, &bytes)?;
    v.get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| GoogleError::Protocol("no file id".to_owned()))
}

/// Delete one key file (permanently, not to the Drive bin).
pub fn delete_key(http: &dyn Http, s: &Session, file_id: &str) -> Result<(), GoogleError> {
    let (status, _) = http.send(Request {
        method: Method::Delete,
        url: format!("{FILES_URL}/{file_id}"),
        bearer: Some(s.token.clone()),
        content_type: None,
        body: Vec::new(),
    })?;
    if (200..300).contains(&status) || status == 404 {
        Ok(())
    } else {
        Err(GoogleError::Protocol(format!("status {status}")))
    }
}

/// End the session at Google: the token stops working at once. Best effort.
pub fn revoke(http: &dyn Http, s: Session) {
    let body = form(&[("token", &s.token)]);
    let _ = http.send(Request {
        method: Method::Post,
        url: REVOKE_URL.to_owned(),
        bearer: None,
        content_type: Some("application/x-www-form-urlencoded".to_owned()),
        body: body.into_bytes(),
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
        fn send(&self, req: Request) -> Result<(u16, Vec<u8>), GoogleError> {
            let url = reqwest::Url::parse(&req.url).unwrap();
            assert!(host_allowed(&url), "{}", req.url);
            self.seen.lock().unwrap().push((
                req.method,
                req.url.clone(),
                req.bearer.as_ref().map(|t| t.to_string()),
                req.body.clone(),
            ));
            Ok(self.answers.lock().unwrap().pop().unwrap())
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
        let s = begin(&cfg()).unwrap();
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
        let code = wait(&s, &AtomicBool::new(false), Duration::from_secs(10)).unwrap();
        browser.join().unwrap();
        assert_eq!(code.as_str(), "the-code");
    }

    #[test]
    fn a_cancelled_wait_stops() {
        let s = begin(&cfg()).unwrap();
        assert_eq!(
            wait(&s, &AtomicBool::new(true), Duration::from_secs(10)).unwrap_err(),
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
}
