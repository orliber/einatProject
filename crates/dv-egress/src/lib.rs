//! The only code in the product that opens a network connection (CI enforces this).
//!
//! It accepts nothing but a [`ClearedPayload`] from the privacy gate, re-checks the request
//! against the Zero-Data-Retention rules, and sends the exact bytes the psychologist
//! approved to one host over TLS 1.3 with Mozilla's root store (not the OS store, so a
//! locally installed intercepting root cannot read the traffic).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dv_privacy::ClearedPayload;
use serde_json::Value;
use zeroize::Zeroizing;

pub const API_HOST: &str = "api.anthropic.com";
const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Must match `dv_ai::ALLOWED_MODELS` (checked by a test in dv-core).
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
];
/// The only tool allowed, and only with an allow-list of domains (D-014).
const ALLOWED_TOOL: &str = "web_search_20250305";

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
    #[error("the request was refused by the service ({0}): {1}")]
    Rejected(u16, String),
    #[error("no connection to the AI service (the rest of the program keeps working)")]
    Offline,
    #[error("the secure connection failed; something on this computer may be intercepting encrypted traffic")]
    Tls,
    #[error("unexpected answer from the service: {0}")]
    Protocol(String),
}

/// Check the outgoing body against the ZDR rules (STANDARDS.md §4, D-009).
pub fn check_policy(body: &Value) -> Result<(), EgressError> {
    let obj = body
        .as_object()
        .ok_or_else(|| EgressError::Policy("body is not an object".to_owned()))?;
    for key in obj.keys() {
        if !ALLOWED_FIELDS.contains(&key.as_str()) {
            return Err(EgressError::Policy(format!("field `{key}` is not allowed")));
        }
    }
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
        for tool in tools.as_array().into_iter().flatten() {
            let ok = tool["type"] == ALLOWED_TOOL
                && tool["allowed_domains"]
                    .as_array()
                    .is_some_and(|d| !d.is_empty())
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
}

/// Simple sliding-window limit: a runaway loop cannot send hundreds of requests.
#[derive(Debug)]
struct RateLimit {
    window: Duration,
    max: usize,
    sent: Vec<Instant>,
}

impl RateLimit {
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        self.sent.retain(|t| now.duration_since(*t) < self.window);
        if self.sent.len() >= self.max {
            return false;
        }
        self.sent.push(now);
        true
    }
}

fn tls_config() -> rustls::ClientConfig {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider);
    match builder.with_protocol_versions(&[&rustls::version::TLS13]) {
        Ok(b) => b.with_root_certificates(roots).with_no_client_auth(),
        // Unreachable with the aws-lc provider; fall back to the provider's safe defaults.
        Err(_) => rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map(|b| {
            b.with_root_certificates(rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            })
            .with_no_client_auth()
        })
        .unwrap_or_else(|_| {
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth()
        }),
    }
}

/// The real transport to the Anthropic Messages API.
pub struct AnthropicTransport {
    client: reqwest::blocking::Client,
    api_key: Zeroizing<String>,
    url: String,
    limit: Mutex<RateLimit>,
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
        let client = reqwest::blocking::Client::builder()
            .use_preconfigured_tls(tls_config())
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(20))
            .user_agent("diagnostic-vault")
            .build()
            .map_err(|e| EgressError::Protocol(e.to_string()))?;
        Ok(Self {
            client,
            api_key: Zeroizing::new(api_key.trim().to_owned()),
            url: MESSAGES_URL.to_owned(),
            limit: Mutex::new(RateLimit {
                window: Duration::from_secs(60),
                max: 20,
                sent: Vec::new(),
            }),
        })
    }

    fn post_once(&self, body: &[u8]) -> Result<Value, EgressError> {
        let resp = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("x-api-key", self.api_key.as_str())
            .header("anthropic-version", API_VERSION)
            .body(body.to_vec())
            .send()
            .map_err(|e| {
                let text = format!("{e:?}").to_lowercase();
                if text.contains("certificate")
                    || text.contains("tls")
                    || text.contains("handshake")
                {
                    EgressError::Tls
                } else {
                    EgressError::Offline
                }
            })?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .map_err(|e| EgressError::Protocol(e.to_string()))?;
        match status {
            200 => serde_json::from_str(&text).map_err(|e| EgressError::Protocol(e.to_string())),
            401 | 403 => Err(EgressError::Unauthorized),
            429 => Err(EgressError::RateLimited),
            500..=599 => Err(EgressError::Unavailable(status)),
            _ => {
                let message = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                Err(EgressError::Rejected(status, message))
            }
        }
    }
}

impl Transport for AnthropicTransport {
    fn send(&self, payload: &ClearedPayload) -> Result<Value, EgressError> {
        let body: Value = serde_json::from_slice(payload.body())
            .map_err(|e| EgressError::Policy(e.to_string()))?;
        check_policy(&body)?;
        if !self.limit.lock().is_ok_and(|mut l| l.allow()) {
            return Err(EgressError::RateLimited);
        }
        let mut delay = Duration::from_secs(2);
        let mut attempt = 0;
        loop {
            match self.post_once(payload.body()) {
                Err(EgressError::Unavailable(_) | EgressError::RateLimited) if attempt < 2 => {
                    std::thread::sleep(delay);
                    delay *= 2;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok_body() -> Value {
        json!({"model": "claude-opus-5", "max_tokens": 100, "inference_geo": "us", "messages": []})
    }

    #[test]
    fn allowed_request_passes_the_policy() {
        assert!(check_policy(&ok_body()).is_ok());
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
    fn empty_api_key_is_refused_and_client_builds() {
        assert!(matches!(
            AnthropicTransport::new("  "),
            Err(EgressError::NoApiKey)
        ));
        assert!(AnthropicTransport::new("sk-ant-test-not-real").is_ok());
    }

    #[test]
    fn rate_limit_stops_runaway_loops() {
        let mut l = RateLimit {
            window: Duration::from_secs(60),
            max: 3,
            sent: Vec::new(),
        };
        assert!(l.allow() && l.allow() && l.allow());
        assert!(!l.allow());
    }
}
