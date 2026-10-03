//! OpenAI (ChatGPT), Google (Gemini), Mistral and a local model (Ollama on this computer) as
//! alternatives to Claude (D-040).
//!
//! The rest of the program builds one request shape (the Messages API shape) and the privacy
//! gate clears exactly that body. Here, after the gate, the cleared body is re-shaped for the
//! chosen company: the same strings move into that company's fields, and nothing is added but
//! fixed field names and settings. The answer is re-shaped back, so parsing and the local
//! checks of the answer are the same whichever company wrote it.
//!
//! Fail-closed: anything in the cleared body that has no exact counterpart (a tool, an image,
//! thinking settings, an unknown field) is refused, not dropped.

use std::sync::Mutex;
use std::time::Duration;

use dv_privacy::ClearedPayload;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::{build_client, check_fields, EgressError, RateLimit, Transport};

pub const OPENAI_HOST: &str = "api.openai.com";
pub const GEMINI_HOST: &str = "generativelanguage.googleapis.com";
const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
const GEMINI_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
pub const MISTRAL_HOST: &str = "api.mistral.ai";
const MISTRAL_URL: &str = "https://api.mistral.ai/v1/chat/completions";
/// Ollama on this computer only: the loopback address, never a name that DNS could redirect.
const LOCAL_URL: &str = "http://127.0.0.1:11434/api/chat";

/// The program's names for the models. Must match `dv_ai::OPENAI_MODELS` /
/// `dv_ai::GEMINI_MODELS` (checked by a test in dv-core). Written with dashes, like
/// `claude-opus-4-8`: the gate reads "2.5" in an outgoing body as a possible date and stops it.
pub const OPENAI_MODELS: &[&str] = &["gpt-5-1", "gpt-5-mini"];
pub const GEMINI_MODELS: &[&str] = &["gemini-2-5-pro", "gemini-2-5-flash"];
pub const MISTRAL_MODELS: &[&str] = &["mistral-large-latest", "mistral-medium-latest"];
pub const LOCAL_MODELS: &[&str] = &["local-gemma", "local-qwen"];

/// The company's own name for a model on the lists above (a fixed table, never data).
fn api_model(model: &str) -> Option<&'static str> {
    Some(match model {
        "gpt-5-1" => "gpt-5.1",
        "gpt-5-mini" => "gpt-5-mini",
        "gemini-2-5-pro" => "gemini-2.5-pro",
        "gemini-2-5-flash" => "gemini-2.5-flash",
        "mistral-large-latest" => "mistral-large-latest",
        "mistral-medium-latest" => "mistral-medium-latest",
        "local-gemma" => "gemma3:12b",
        "local-qwen" => "qwen3:14b",
        _ => return None,
    })
}

/// The company a request goes to. Decided by the model in the cleared body, never by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Gemini,
    Mistral,
    /// Ollama on this computer: nothing leaves it.
    Local,
}

impl Provider {
    #[must_use]
    pub fn of_model(model: &str) -> Option<Self> {
        if crate::ALLOWED_MODELS.contains(&model) {
            Some(Self::Anthropic)
        } else if OPENAI_MODELS.contains(&model) {
            Some(Self::OpenAi)
        } else if GEMINI_MODELS.contains(&model) {
            Some(Self::Gemini)
        } else if MISTRAL_MODELS.contains(&model) {
            Some(Self::Mistral)
        } else if LOCAL_MODELS.contains(&model) {
            Some(Self::Local)
        } else {
            None
        }
    }
}

/// The transport for the model named in the cleared body, with that company's key (the local
/// model takes none).
pub fn transport_for(model: &str, api_key: &str) -> Result<Box<dyn Transport>, EgressError> {
    match Provider::of_model(model) {
        Some(Provider::Anthropic) => Ok(Box::new(crate::AnthropicTransport::new(api_key)?)),
        Some(p) => Ok(Box::new(ProviderTransport::new(p, api_key)?)),
        None => Err(EgressError::Policy(format!(
            "model `{model}` is not on the allow-list"
        ))),
    }
}

fn policy(why: &str) -> EgressError {
    EgressError::Policy(why.to_owned())
}

/// The text of a `system` value or a message `content`: a string, or text blocks only.
fn text_of(v: &Value) -> Result<String, EgressError> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Array(blocks) => {
            let mut out = Vec::new();
            for b in blocks {
                if b["type"] != "text" {
                    return Err(policy("only text can be sent to this provider"));
                }
                let t = b["text"]
                    .as_str()
                    .ok_or_else(|| policy("a text block without text"))?;
                out.push(t.to_owned());
            }
            Ok(out.join("\n\n"))
        }
        _ => Err(policy("unexpected content")),
    }
}

/// The parts of a cleared body that every provider understands.
#[derive(Debug)]
pub(crate) struct Canonical {
    /// The company's name for the model (see [`api_model`]).
    model: &'static str,
    max_tokens: u64,
    system: Option<String>,
    /// `(is_assistant, text)`.
    turns: Vec<(bool, String)>,
    effort: String,
    schema: Option<Value>,
}

/// Re-check the cleared body for a non-Anthropic provider (the same field allow-list, plus:
/// no tools, no thinking settings, text only).
pub(crate) fn canonical(body: &Value, provider: Provider) -> Result<Canonical, EgressError> {
    let obj = check_fields(body)?;
    let model = obj.get("model").and_then(Value::as_str).unwrap_or("");
    if Provider::of_model(model) != Some(provider) || provider == Provider::Anthropic {
        return Err(EgressError::Policy(format!(
            "model `{model}` is not on this provider's allow-list"
        )));
    }
    if obj.contains_key("tools") {
        return Err(policy(
            "web search is available only with Claude (allowed domains, D-014)",
        ));
    }
    if obj.contains_key("thinking") {
        return Err(policy("thinking settings are not translated"));
    }
    let api = api_model(model).ok_or_else(|| policy("model without an API name"))?;
    let max_tokens = obj
        .get("max_tokens")
        .and_then(Value::as_u64)
        .filter(|n| (1..=64_000).contains(n))
        .ok_or_else(|| policy("max_tokens is missing or out of range"))?;
    let system = obj.get("system").map(text_of).transpose()?;
    let mut turns = Vec::new();
    for m in obj
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| policy("no messages"))?
    {
        let assistant = match m["role"].as_str() {
            Some("user") => false,
            Some("assistant") => true,
            _ => return Err(policy("unknown role")),
        };
        turns.push((assistant, text_of(&m["content"])?));
    }
    let mut effort = "medium".to_owned();
    let mut schema = None;
    if let Some(cfg) = obj.get("output_config") {
        let cfg = cfg
            .as_object()
            .ok_or_else(|| policy("output_config is not an object"))?;
        for (k, v) in cfg {
            match k.as_str() {
                "effort" => {
                    effort = v
                        .as_str()
                        .ok_or_else(|| policy("effort is not text"))?
                        .to_owned();
                }
                "format" if v["type"] == "json_schema" && v["schema"].is_object() => {
                    schema = Some(v["schema"].clone());
                }
                _ => return Err(EgressError::Policy(format!("output_config `{k}`"))),
            }
        }
    }
    Ok(Canonical {
        model: api,
        max_tokens,
        system,
        turns,
        effort,
        schema,
    })
}

fn openai_effort(effort: &str) -> &'static str {
    match effort {
        "low" => "low",
        "medium" => "medium",
        _ => "high",
    }
}

fn gemini_thinking_budget(effort: &str) -> u64 {
    match effort {
        "low" => 1_024,
        "medium" => 8_192,
        _ => 16_384,
    }
}

/// Chat Completions body. `store: false`: OpenAI keeps no stored completion to show later.
pub(crate) fn to_openai(c: &Canonical) -> Value {
    let mut messages = Vec::new();
    if let Some(s) = &c.system {
        messages.push(json!({ "role": "developer", "content": s }));
    }
    for (assistant, text) in &c.turns {
        messages.push(json!({
            "role": if *assistant { "assistant" } else { "user" },
            "content": text,
        }));
    }
    let mut body = json!({
        "model": c.model,
        "messages": messages,
        "max_completion_tokens": c.max_tokens,
        "reasoning_effort": openai_effort(&c.effort),
        "store": false,
    });
    if let Some(schema) = &c.schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": "answer", "strict": true, "schema": schema },
        });
    }
    body
}

/// Mistral's Chat Completions: the OpenAI shape without OpenAI-only settings.
pub(crate) fn to_mistral(c: &Canonical) -> Value {
    let mut messages = Vec::new();
    if let Some(s) = &c.system {
        messages.push(json!({ "role": "system", "content": s }));
    }
    for (assistant, text) in &c.turns {
        messages.push(json!({
            "role": if *assistant { "assistant" } else { "user" },
            "content": text,
        }));
    }
    let mut body = json!({
        "model": c.model,
        "messages": messages,
        "max_tokens": c.max_tokens,
    });
    if let Some(schema) = &c.schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": "answer", "strict": true, "schema": schema },
        });
    }
    body
}

/// Ollama's own chat API: the answer whole, held to the schema with `format`.
pub(crate) fn to_local(c: &Canonical) -> Value {
    let mut messages = Vec::new();
    if let Some(s) = &c.system {
        messages.push(json!({ "role": "system", "content": s }));
    }
    for (assistant, text) in &c.turns {
        messages.push(json!({
            "role": if *assistant { "assistant" } else { "user" },
            "content": text,
        }));
    }
    let mut body = json!({
        "model": c.model,
        "messages": messages,
        "stream": false,
        "options": { "num_predict": c.max_tokens },
    });
    if let Some(schema) = &c.schema {
        body["format"] = schema.clone();
    }
    body
}

/// `generateContent` body. Thinking tokens count toward the output limit, so it is raised by
/// the thinking budget.
pub(crate) fn to_gemini(c: &Canonical) -> Value {
    let contents: Vec<Value> = c
        .turns
        .iter()
        .map(|(assistant, text)| {
            json!({
                "role": if *assistant { "model" } else { "user" },
                "parts": [ { "text": text } ],
            })
        })
        .collect();
    let budget = gemini_thinking_budget(&c.effort);
    let mut config = json!({
        "maxOutputTokens": c.max_tokens + budget,
        "thinkingConfig": { "thinkingBudget": budget },
    });
    if let Some(schema) = &c.schema {
        config["responseMimeType"] = json!("application/json");
        config["responseJsonSchema"] = schema.clone();
    }
    let mut body = json!({ "contents": contents, "generationConfig": config });
    if let Some(s) = &c.system {
        body["systemInstruction"] = json!({ "parts": [ { "text": s } ] });
    }
    body
}

/// The answer in the Messages API shape the rest of the program reads.
fn answer(model: &str, text: String, stop: &str, category: Option<String>) -> Value {
    let mut v = json!({
        "type": "message",
        "model": model,
        "stop_reason": stop,
        "content": [ { "type": "text", "text": text } ],
    });
    if let Some(c) = category {
        v["stop_details"] = json!({ "category": c });
    }
    v
}

pub(crate) fn from_openai(model: &str, resp: &Value) -> Result<Value, EgressError> {
    let choice = &resp["choices"][0];
    if choice.is_null() {
        return Err(EgressError::Protocol("no choices in the answer".to_owned()));
    }
    let message = &choice["message"];
    if message["refusal"].as_str().is_some_and(|r| !r.is_empty()) {
        return Ok(answer(
            model,
            String::new(),
            "refusal",
            Some("refusal".to_owned()),
        ));
    }
    let text = message["content"].as_str().unwrap_or("").to_owned();
    Ok(match choice["finish_reason"].as_str() {
        Some("length" | "model_length") => answer(model, text, "max_tokens", None),
        Some("content_filter") => answer(
            model,
            String::new(),
            "refusal",
            Some("content_filter".to_owned()),
        ),
        _ => answer(model, text, "end_turn", None),
    })
}

pub(crate) fn from_local(model: &str, resp: &Value) -> Result<Value, EgressError> {
    let text = resp["message"]["content"]
        .as_str()
        .ok_or_else(|| EgressError::Protocol("no message in the answer".to_owned()))?
        .to_owned();
    Ok(match resp["done_reason"].as_str() {
        Some("length") => answer(model, text, "max_tokens", None),
        _ => answer(model, text, "end_turn", None),
    })
}

pub(crate) fn from_gemini(model: &str, resp: &Value) -> Result<Value, EgressError> {
    if let Some(reason) = resp["promptFeedback"]["blockReason"].as_str() {
        return Ok(answer(
            model,
            String::new(),
            "refusal",
            Some(reason.to_lowercase()),
        ));
    }
    let cand = &resp["candidates"][0];
    if cand.is_null() {
        return Err(EgressError::Protocol(
            "no candidates in the answer".to_owned(),
        ));
    }
    // Thought summaries (if any) are not part of the answer.
    let text: String = cand["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["thought"] != true)
        .filter_map(|p| p["text"].as_str())
        .collect();
    Ok(match cand["finishReason"].as_str() {
        Some("STOP") | None => answer(model, text, "end_turn", None),
        Some("MAX_TOKENS") => answer(model, text, "max_tokens", None),
        Some(other) => answer(model, String::new(), "refusal", Some(other.to_lowercase())),
    })
}

/// ChatGPT, Gemini or Mistral over the same hardened client as Claude (TLS 1.3, Mozilla roots,
/// no proxy, no redirects); the local model over plain HTTP to the loopback address only. The
/// answer arrives whole (no word count while it is written).
pub struct ProviderTransport {
    provider: Provider,
    client: reqwest::blocking::Client,
    api_key: Zeroizing<String>,
    limit: Mutex<RateLimit>,
}

impl std::fmt::Debug for ProviderTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderTransport")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

impl ProviderTransport {
    pub fn new(provider: Provider, api_key: &str) -> Result<Self, EgressError> {
        if api_key.trim().is_empty() && provider != Provider::Local {
            return Err(EgressError::NoApiKey);
        }
        if provider == Provider::Anthropic {
            return Err(policy("Claude goes through AnthropicTransport"));
        }
        Ok(Self {
            provider,
            client: if provider == Provider::Local {
                local_client()?
            } else {
                build_client()?
            },
            api_key: Zeroizing::new(api_key.trim().to_owned()),
            limit: Mutex::new(RateLimit::per_minute(20)),
        })
    }

    fn post_once(&self, model: &str, body: &[u8]) -> Result<Value, EgressError> {
        let req = match self.provider {
            Provider::OpenAi => self
                .client
                .post(OPENAI_URL)
                .bearer_auth(self.api_key.as_str()),
            Provider::Mistral => self
                .client
                .post(MISTRAL_URL)
                .bearer_auth(self.api_key.as_str()),
            Provider::Local => self.client.post(LOCAL_URL),
            // A fixed name from `api_model`, so it is safe in the path.
            _ => self
                .client
                .post(format!("{GEMINI_URL}/{model}:generateContent"))
                .header("x-goog-api-key", self.api_key.as_str()),
        };
        let resp = req
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
            .map_err(|e| match self.provider {
                Provider::Local => EgressError::LocalUnavailable,
                _ => crate::send_error(&e),
            })?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .map_err(|e| EgressError::Protocol(e.to_string()))?;
        let parsed = serde_json::from_str::<Value>(&text);
        let message = parsed
            .as_ref()
            .ok()
            .and_then(|v| {
                v["error"]["message"]
                    .as_str()
                    .or_else(|| v["error"].as_str())
                    .or_else(|| v["message"].as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        match status {
            200 => {
                let v = parsed.map_err(|e| EgressError::Protocol(e.to_string()))?;
                match self.provider {
                    Provider::OpenAi | Provider::Mistral => from_openai(model, &v),
                    Provider::Local => from_local(model, &v),
                    _ => from_gemini(model, &v),
                }
            }
            // Ollama answers a model that is not installed with 404.
            404 if self.provider == Provider::Local => Err(EgressError::LocalUnavailable),
            401 | 403 => Err(EgressError::Unauthorized),
            // Gemini answers a wrong key with 400.
            400 if message.contains("API key") => Err(EgressError::Unauthorized),
            429 => Err(EgressError::RateLimited),
            500..=599 => Err(EgressError::Unavailable(status)),
            _ => Err(EgressError::Rejected(status, message)),
        }
    }
}

impl Transport for ProviderTransport {
    fn send(&self, payload: &ClearedPayload) -> Result<Value, EgressError> {
        let body: Value = serde_json::from_slice(payload.body())
            .map_err(|e| EgressError::Policy(e.to_string()))?;
        let c = canonical(&body, self.provider)?;
        let out = match self.provider {
            Provider::OpenAi => to_openai(&c),
            Provider::Mistral => to_mistral(&c),
            Provider::Local => to_local(&c),
            _ => to_gemini(&c),
        };
        let bytes = serde_json::to_vec(&out).map_err(|e| EgressError::Protocol(e.to_string()))?;
        if !self.limit.lock().is_ok_and(|mut l| l.allow()) {
            return Err(EgressError::RateLimited);
        }
        let mut delay = Duration::from_secs(2);
        let mut attempt = 0;
        loop {
            match self.post_once(c.model, &bytes) {
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

/// Plain HTTP to the loopback address only (the URL is a constant): no proxy, no redirects,
/// and a long timeout, since a model on an ordinary computer writes slowly.
fn local_client() -> Result<reqwest::blocking::Client, EgressError> {
    reqwest::blocking::Client::builder()
        // Never used for plain HTTP, but the client needs a TLS setup to be built.
        .use_preconfigured_tls(crate::tls_config())
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(1_800))
        .connect_timeout(Duration::from_secs(5))
        .user_agent("diagnostic-vault")
        .build()
        .map_err(|e| EgressError::Protocol(e.to_string()))
}

/// Every string in a JSON value (keys excluded).
#[cfg(test)]
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn section_body(model: &str) -> Value {
        json!({
            "model": model,
            "max_tokens": 16000,
            "inference_geo": "us",
            "output_config": {
                "effort": "medium",
                "format": {"type": "json_schema", "schema": {"type": "object", "additionalProperties": false}}
            },
            "system": [{"type": "text", "text": "כללי ניסוח", "cache_control": {"type": "ephemeral"}}],
            "stream": true,
            "messages": [
                {"role": "user", "content": "[ילד] מתקשה במעברים"},
                {"role": "assistant", "content": "טיוטה"},
                {"role": "user", "content": "לקצר"}
            ]
        })
    }

    /// Strings the translation itself may add: roles, settings and fixed names.
    const FIXED: &[&str] = &[
        "developer",
        "user",
        "assistant",
        "model",
        "low",
        "medium",
        "high",
        "json_schema",
        "answer",
        "application/json",
        "object",
        "gpt-5.1",
        "gemini-2.5-pro",
        "system",
        "mistral-large-latest",
        "gemma3:12b",
    ];

    fn adds_nothing(canonical_body: &Value, translated: &Value) {
        let mut allowed = Vec::new();
        strings(canonical_body, &mut allowed);
        let mut sent = Vec::new();
        strings(translated, &mut sent);
        for s in sent {
            assert!(
                allowed.contains(&s) || FIXED.contains(&s.as_str()),
                "the translation added text: {s}"
            );
        }
    }

    #[test]
    fn openai_body_carries_only_the_cleared_text() {
        let b = section_body("gpt-5-1");
        let out = to_openai(&canonical(&b, Provider::OpenAi).unwrap());
        adds_nothing(&b, &out);
        assert_eq!(out["store"], false);
        assert_eq!(out["messages"][0]["role"], "developer");
        assert_eq!(out["messages"][2]["role"], "assistant");
        assert_eq!(out["response_format"]["json_schema"]["strict"], true);
        assert_eq!(out["reasoning_effort"], "medium");
        assert_eq!(out["model"], "gpt-5.1");
        assert!(out.get("user").is_none() && out.get("metadata").is_none());
    }

    #[test]
    fn gemini_body_carries_only_the_cleared_text() {
        let b = section_body("gemini-2-5-pro");
        let out = to_gemini(&canonical(&b, Provider::Gemini).unwrap());
        adds_nothing(&b, &out);
        assert_eq!(out["contents"][1]["role"], "model");
        assert_eq!(out["systemInstruction"]["parts"][0]["text"], "כללי ניסוח");
        assert_eq!(
            out["generationConfig"]["responseMimeType"],
            "application/json"
        );
        assert_eq!(out["generationConfig"]["maxOutputTokens"], 16000 + 8192);
    }

    #[test]
    fn mistral_and_local_bodies_carry_only_the_cleared_text() {
        let b = section_body("mistral-large-latest");
        let out = to_mistral(&canonical(&b, Provider::Mistral).unwrap());
        adds_nothing(&b, &out);
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["response_format"]["type"], "json_schema");
        assert!(out.get("reasoning_effort").is_none() && out.get("store").is_none());

        let b = section_body("local-gemma");
        let out = to_local(&canonical(&b, Provider::Local).unwrap());
        adds_nothing(&b, &out);
        assert_eq!(out["model"], "gemma3:12b");
        assert_eq!(out["stream"], false);
        assert!(out["format"].is_object());
        let back = from_local(
            "gemma3:12b",
            &json!({"message": {"role": "assistant", "content": "{}"}, "done": true, "done_reason": "stop"}),
        )
        .unwrap();
        assert_eq!(back["stop_reason"], "end_turn");
        assert!(LOCAL_URL.starts_with("http://127.0.0.1:"));
    }

    /// Run by hand against Ollama (or a stand-in) on 127.0.0.1:11434:
    /// `cargo test -p dv-egress -- --ignored local_model_round_trip`.
    #[test]
    #[ignore = "needs a local model server"]
    fn local_model_round_trip() {
        let t = ProviderTransport::new(Provider::Local, "").unwrap();
        let body = serde_json::to_vec(&to_local(
            &canonical(&section_body("local-gemma"), Provider::Local).unwrap(),
        ))
        .unwrap();
        let answer = t.post_once("gemma3:12b", &body).unwrap();
        assert_eq!(answer["stop_reason"], "end_turn");
        assert!(answer["content"][0]["text"].is_string());
    }

    #[test]
    fn anything_without_an_exact_counterpart_is_refused() {
        // A model from another company, or off every list.
        assert!(canonical(&section_body("claude-opus-5"), Provider::OpenAi).is_err());
        assert!(canonical(&section_body("gpt-4o"), Provider::OpenAi).is_err());
        assert!(canonical(&section_body("gpt-5-1"), Provider::Gemini).is_err());
        // Web search (D-014) stays with Claude.
        let mut b = section_body("gpt-5-1");
        b["tools"] = json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 3, "allowed_domains": ["apa.org"]}]);
        assert!(canonical(&b, Provider::OpenAi).is_err());
        // Unknown fields, images, unknown roles.
        let mut b = section_body("gpt-5-1");
        b["metadata"] = json!({"user_id": "x"});
        assert!(canonical(&b, Provider::OpenAi).is_err());
        let mut b = section_body("gemini-2-5-flash");
        b["messages"][0]["content"] = json!([{"type": "image", "source": {}}]);
        assert!(canonical(&b, Provider::Gemini).is_err());
        let mut b = section_body("gemini-2-5-flash");
        b["messages"][0]["role"] = json!("system");
        assert!(canonical(&b, Provider::Gemini).is_err());
        let mut b = section_body("gemini-2-5-flash");
        b["output_config"]["task_budget"] = json!(1);
        assert!(canonical(&b, Provider::Gemini).is_err());
    }

    #[test]
    fn answers_come_back_in_one_shape() {
        let ok = from_openai(
            "gpt-5-1",
            &json!({"choices": [{"message": {"content": "{\"a\":1}"}, "finish_reason": "stop"}]}),
        )
        .unwrap();
        assert_eq!(ok["stop_reason"], "end_turn");
        assert_eq!(ok["content"][0]["text"], "{\"a\":1}");
        let cut = from_openai(
            "gpt-5-1",
            &json!({"choices": [{"message": {"content": "{"}, "finish_reason": "length"}]}),
        )
        .unwrap();
        assert_eq!(cut["stop_reason"], "max_tokens");
        let no = from_openai(
            "gpt-5-1",
            &json!({"choices": [{"message": {"content": null, "refusal": "no"}, "finish_reason": "stop"}]}),
        )
        .unwrap();
        assert_eq!(no["stop_reason"], "refusal");

        let ok = from_gemini(
            "gemini-2-5-pro",
            &json!({"candidates": [{"content": {"parts": [{"text": "חשבתי", "thought": true}, {"text": "תשובה"}]}, "finishReason": "STOP"}]}),
        )
        .unwrap();
        assert_eq!(ok["content"][0]["text"], "תשובה");
        let blocked = from_gemini(
            "gemini-2-5-pro",
            &json!({"promptFeedback": {"blockReason": "SAFETY"}}),
        )
        .unwrap();
        assert_eq!(blocked["stop_reason"], "refusal");
        assert_eq!(blocked["stop_details"]["category"], "safety");
        let cut = from_gemini(
            "gemini-2-5-pro",
            &json!({"candidates": [{"content": {"parts": [{"text": "{"}]}, "finishReason": "MAX_TOKENS"}]}),
        )
        .unwrap();
        assert_eq!(cut["stop_reason"], "max_tokens");
        assert!(from_gemini("gemini-2-5-pro", &json!({})).is_err());
    }

    #[test]
    fn transports_follow_the_model_and_need_a_key() {
        assert!(transport_for("gpt-5-1", "sk-test-not-real").is_ok());
        assert!(transport_for("gemini-2-5-flash", "test-not-real").is_ok());
        assert!(transport_for("claude-opus-5", "sk-ant-test-not-real").is_ok());
        assert!(transport_for("mistral-large-latest", "test-not-real").is_ok());
        assert!(
            transport_for("local-gemma", "").is_ok(),
            "the local model takes no key"
        );
        assert!(matches!(
            transport_for("mistral-large-latest", ""),
            Err(EgressError::NoApiKey)
        ));
        assert!(matches!(
            transport_for("gpt-5-1", " "),
            Err(EgressError::NoApiKey)
        ));
        assert!(matches!(
            transport_for("gpt-4o", "sk-test-not-real"),
            Err(EgressError::Policy(_))
        ));
    }
}
