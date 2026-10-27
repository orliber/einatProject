//! What reaches the wire, tested against a listener on this computer (never a real API).
//!
//! A request the policy refuses must fail before any connection is opened: the transport is
//! pointed at a local listener, and the listener must see nothing. The positive control shows
//! the same setup does see a connection when the request is allowed, so "nothing seen" means
//! something.

use std::collections::HashSet;
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::Duration;

use dv_privacy::{clear, Blocked, ClearedPayload, GateRequest, PrivacyContext};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::providers::{Provider, ProviderTransport};
use crate::{
    build_client, AnthropicTransport, EgressError, RateLimit, Transport, ALLOWED_MODELS, API_HOST,
    MESSAGES_URL,
};

/// A body through the real gate, with an empty case: the only way to get a [`ClearedPayload`].
/// (A `Result`, so CI's textual check for code that builds a payload itself stays exact.)
fn cleared(body: &Value) -> Result<ClearedPayload, Blocked> {
    let ctx = PrivacyContext {
        case_id: "case-test",
        identities: &[],
        practitioner: &[],
        allowlisted: &|_: &str| false,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: (2026, 10, 11),
        masks: None,
    };
    let tags = HashSet::new();
    clear(&GateRequest {
        body,
        ctx: &ctx,
        case_tags: &tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 8 * 1024 * 1024,
    })
}

fn ok_body() -> Value {
    json!({
        "model": "claude-opus-5",
        "max_tokens": 100,
        "inference_geo": "us",
        "stream": true,
        "messages": [{"role": "user", "content": "שאלה כללית"}]
    })
}

/// A listener on a free loopback port, and an Anthropic transport aimed at it.
fn aimed_at_listener(client: reqwest::blocking::Client) -> (TcpListener, AnthropicTransport) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let t = AnthropicTransport {
        client,
        api_key: Zeroizing::new("sk-ant-test-not-real".to_owned()),
        url: format!("https://127.0.0.1:{port}/v1/messages"),
        // Its own limit and breaker, so tests running side by side do not share them.
        limit: Box::leak(Box::new(Mutex::new(RateLimit::per_minute(1_000)))),
        breaker: Box::leak(Box::default()),
    };
    (listener, t)
}

/// A client that gives up quickly, so a regression shows as a failure and not a long hang.
fn short_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .use_preconfigured_tls(crate::tls_config())
        .timeout(Duration::from_secs(3))
        .no_proxy()
        .build()
        .unwrap()
}

fn nothing_connected(listener: &TcpListener) -> bool {
    matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
}

#[test]
fn a_refused_request_opens_no_connection() {
    let mut refused: Vec<(&str, Value)> = Vec::new();
    for model in [
        "claude-fable-five",
        "claude-mythos",
        "gpt-5-1",
        "gemini-2-5-pro",
        "mistral-large",
        "local-gemma",
        "Claude-Opus-5",
        "claude-opus-5 ",
        "",
    ] {
        let mut b = ok_body();
        b["model"] = json!(model);
        refused.push(("model", b));
    }
    let mut b = ok_body();
    b.as_object_mut().unwrap().remove("model");
    refused.push(("no model", b));
    for (field, value) in [
        ("metadata", json!({"user_id": "u"})),
        ("mcp_servers", json!([{"type": "url", "name": "server"}])),
        ("container", json!("container-one")),
        ("betas", json!(["files-api"])),
        ("service_tier", json!("auto")),
    ] {
        let mut b = ok_body();
        b[field] = value;
        refused.push((field, b));
    }
    let mut b = ok_body();
    b["inference_geo"] = json!("global");
    refused.push(("geo", b));
    let mut b = ok_body();
    b.as_object_mut().unwrap().remove("inference_geo");
    refused.push(("no geo", b));
    // Tool types without their dated suffix: the gate itself stops an eight-digit run as a
    // possible identifier, so a dated name never even reaches this check.
    for tool in [
        json!({"type": "code_execution", "name": "code_execution"}),
        json!({"type": "web_fetch", "name": "web_fetch"}),
        json!({"type": "bash", "name": "bash"}),
        json!({"type": "memory", "name": "memory"}),
        json!({"name": "lookup", "input_schema": {"type": "object"}}),
    ] {
        let mut b = ok_body();
        b["tools"] = json!([tool]);
        refused.push(("tool", b));
    }

    let (listener, t) = aimed_at_listener(short_client());
    for (why, body) in refused {
        let payload = cleared(&body).unwrap();
        let got = t.send(&payload);
        assert!(matches!(got, Err(EgressError::Policy(_))), "{why}: {got:?}");
        let got = t.send_streaming(&payload, &|_| {});
        assert!(matches!(got, Err(EgressError::Policy(_))), "{why}: {got:?}");
    }
    assert!(
        nothing_connected(&listener),
        "a refused request reached the wire"
    );
}

#[test]
fn an_oversized_request_opens_no_connection() {
    let mut b = ok_body();
    b["messages"][0]["content"] = json!("x".repeat(crate::MAX_BODY_BYTES));
    let payload = cleared(&b).unwrap();
    let (listener, t) = aimed_at_listener(short_client());
    assert_eq!(t.send(&payload), Err(EgressError::TooLarge));
    assert!(nothing_connected(&listener));
}

#[test]
fn an_open_breaker_opens_no_connection_and_says_how_long() {
    let (listener, mut t) = aimed_at_listener(short_client());
    let mut open = crate::Breaker::default();
    for _ in 0..crate::BREAKER_FAILURES {
        open.record(crate::Health::Busy, std::time::Instant::now());
    }
    t.breaker = Box::leak(Box::new(Mutex::new(open)));
    let payload = cleared(&ok_body()).unwrap();
    let got = t.send_streaming(&payload, &|_| {});
    assert!(
        matches!(got, Err(EgressError::CircuitOpen(s)) if (1..=60).contains(&s)),
        "{got:?}"
    );
    assert!(
        nothing_connected(&listener),
        "an open breaker let a request out"
    );
}

#[test]
fn an_allowed_request_does_reach_the_listener() {
    // The positive control for the tests above, with the real hardened client.
    let (listener, t) = aimed_at_listener(build_client().unwrap());
    listener.set_nonblocking(false).unwrap();
    let server = std::thread::spawn(move || listener.accept().is_ok());
    let got = t.post_once(cleared(&ok_body()).unwrap().body(), &|_| {});
    assert!(server.join().unwrap(), "no connection arrived");
    // The listener speaks no TLS: the hardened client must not treat that as an answer.
    assert!(
        matches!(got, Err(EgressError::Tls | EgressError::Offline)),
        "{got:?}"
    );
}

#[test]
fn other_providers_refuse_a_body_for_a_model_that_is_not_theirs() {
    let claude = cleared(&ok_body()).unwrap();
    for p in [
        Provider::OpenAi,
        Provider::Gemini,
        Provider::Mistral,
        Provider::Local,
    ] {
        let t = ProviderTransport::new(p, "test-key-not-real").unwrap();
        assert!(
            matches!(t.send(&claude), Err(EgressError::Policy(_))),
            "{p:?}"
        );
    }
    // And the Claude transport refuses every other company's model.
    let (listener, t) = aimed_at_listener(short_client());
    for model in crate::OPENAI_MODELS
        .iter()
        .chain(crate::GEMINI_MODELS)
        .chain(crate::MISTRAL_MODELS)
        .chain(crate::LOCAL_MODELS)
    {
        let mut b = ok_body();
        b["model"] = json!(model);
        assert!(matches!(
            t.send(&cleared(&b).unwrap()),
            Err(EgressError::Policy(_))
        ));
    }
    assert!(nothing_connected(&listener));
}

#[test]
fn the_messages_endpoint_is_the_only_anthropic_address() {
    let url = reqwest::Url::parse(MESSAGES_URL).unwrap();
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some(API_HOST));
    assert_eq!(url.port(), None);
    assert_eq!(url.path(), "/v1/messages");
    assert!(url.query().is_none() && url.username().is_empty());
    let t = AnthropicTransport::new("sk-ant-test-not-real").unwrap();
    assert_eq!(t.url, MESSAGES_URL);
    assert!(
        !ALLOWED_MODELS
            .iter()
            .any(|m| m.contains("fable") || m.contains("mythos")),
        "models that need 30-day retention stay off the list"
    );
}

#[test]
fn a_payload_in_a_log_line_shows_its_hash_not_its_text() {
    let mut b = ok_body();
    b["messages"][0]["content"] = json!("תוכן שאסור שיופיע ביומן");
    let p = cleared(&b).unwrap();
    let shown = format!("{p:?}");
    assert!(shown.contains(p.sha256()));
    assert!(!shown.contains("ביומן") && !shown.contains("claude-opus-5"));
}
