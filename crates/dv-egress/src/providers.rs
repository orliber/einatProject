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

use crate::{admit, build_client, check_fields, retry, EgressError, RateLimit, Transport};

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
pub const MISTRAL_MODELS: &[&str] = &["mistral-large", "mistral-medium"];
pub const LOCAL_MODELS: &[&str] = &["local-gemma", "local-qwen"];

/// The company's own name for a model on the lists above (a fixed table, never data).
fn api_model(model: &str) -> Option<&'static str> {
    Some(match model {
        "gpt-5-1" => "gpt-5.1",
        "gpt-5-mini" => "gpt-5-mini",
        "gemini-2-5-pro" => "gemini-2.5-pro",
        "gemini-2-5-flash" => "gemini-2.5-flash",
        // Dated versions, not `-latest` aliases: the model does not change under her.
        "mistral-large" => "mistral-large-2411",
        "mistral-medium" => "mistral-medium-2508",
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
        let assistant = match m.get("role").and_then(Value::as_str) {
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
        set(
            &mut body,
            "response_format",
            json!({
                "type": "json_schema",
                "json_schema": { "name": "answer", "strict": true, "schema": schema },
            }),
        );
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
        set(
            &mut body,
            "response_format",
            json!({
                "type": "json_schema",
                "json_schema": { "name": "answer", "strict": true, "schema": schema },
            }),
        );
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
        set(&mut body, "format", schema.clone());
    }
    body
}

/// The most output a Gemini model on the list writes in one answer (thinking included).
const GEMINI_MAX_OUTPUT: u64 = 65_536;

/// `generateContent` body. Thinking tokens count toward the output limit, so it is raised by
/// the thinking budget, up to the models' own maximum.
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
        // Never above what the Gemini models write at most: the request is refused past it.
        "maxOutputTokens": (c.max_tokens + budget).min(GEMINI_MAX_OUTPUT),
        "thinkingConfig": { "thinkingBudget": budget },
    });
    if let Some(schema) = &c.schema {
        set(&mut config, "responseMimeType", json!("application/json"));
        set(&mut config, "responseJsonSchema", schema.clone());
    }
    let mut body = json!({ "contents": contents, "generationConfig": config });
    if let Some(s) = &c.system {
        set(
            &mut body,
            "systemInstruction",
            json!({ "parts": [ { "text": s } ] }),
        );
    }
    body
}

/// Sets a field of a body this module built as a JSON object (a write through `value[key]`
/// would panic on anything else).
fn set(v: &mut Value, key: &str, value: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_owned(), value);
    }
}

/// Token counts in the Messages API `usage` shape (for the monthly cost ceiling).
fn usage(input: Option<&Value>, output: Option<&Value>) -> Value {
    let count = |v: Option<&Value>| v.and_then(Value::as_u64).unwrap_or(0);
    json!({
        "input_tokens": count(input),
        "output_tokens": count(output),
    })
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
        set(&mut v, "stop_details", json!({ "category": c }));
    }
    v
}

pub(crate) fn from_openai(model: &str, resp: &Value) -> Result<Value, EgressError> {
    let mut v = from_openai_inner(model, resp)?;
    let tokens = usage(
        resp.pointer("/usage/prompt_tokens"),
        resp.pointer("/usage/completion_tokens"),
    );
    set(&mut v, "usage", tokens);
    Ok(v)
}

fn from_openai_inner(model: &str, resp: &Value) -> Result<Value, EgressError> {
    let choice = resp
        .pointer("/choices/0")
        .filter(|c| !c.is_null())
        .ok_or_else(|| EgressError::Protocol("no choices in the answer".to_owned()))?;
    let message = choice.get("message").unwrap_or(&Value::Null);
    if message
        .get("refusal")
        .and_then(Value::as_str)
        .is_some_and(|r| !r.is_empty())
    {
        return Ok(answer(
            model,
            String::new(),
            "refusal",
            Some("refusal".to_owned()),
        ));
    }
    let text = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    Ok(match choice.get("finish_reason").and_then(Value::as_str) {
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
    let mut v = from_local_inner(model, resp)?;
    let tokens = usage(resp.get("prompt_eval_count"), resp.get("eval_count"));
    set(&mut v, "usage", tokens);
    Ok(v)
}

fn from_local_inner(model: &str, resp: &Value) -> Result<Value, EgressError> {
    let text = resp
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| EgressError::Protocol("no message in the answer".to_owned()))?
        .to_owned();
    Ok(match resp.get("done_reason").and_then(Value::as_str) {
        Some("length") => answer(model, text, "max_tokens", None),
        _ => answer(model, text, "end_turn", None),
    })
}

pub(crate) fn from_gemini(model: &str, resp: &Value) -> Result<Value, EgressError> {
    let mut v = from_gemini_inner(model, resp)?;
    let meta = resp.get("usageMetadata").unwrap_or(&Value::Null);
    let count = |k: &str| meta.get(k).and_then(Value::as_u64).unwrap_or(0);
    // Thinking tokens are billed as output. The counts are the server's: a sum that does not
    // fit is the most, never a crash.
    let output = count("candidatesTokenCount").saturating_add(count("thoughtsTokenCount"));
    let tokens = usage(meta.get("promptTokenCount"), Some(&json!(output)));
    set(&mut v, "usage", tokens);
    Ok(v)
}

fn from_gemini_inner(model: &str, resp: &Value) -> Result<Value, EgressError> {
    if let Some(reason) = resp
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
    {
        return Ok(answer(
            model,
            String::new(),
            "refusal",
            Some(reason.to_lowercase()),
        ));
    }
    let cand = resp
        .pointer("/candidates/0")
        .filter(|c| !c.is_null())
        .ok_or_else(|| EgressError::Protocol("no candidates in the answer".to_owned()))?;
    // Thought summaries (if any) are not part of the answer.
    let text: String = cand
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| p.get("thought") != Some(&Value::Bool(true)))
        .filter_map(|p| p.get("text").and_then(Value::as_str))
        .collect();
    Ok(match cand.get("finishReason").and_then(Value::as_str) {
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
    limit: &'static Mutex<RateLimit>,
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
            limit: &crate::MINUTE_LIMIT,
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
        let text = crate::read_answer(resp)?;
        if status != 200 {
            return Err(status_error(self.provider, status, &text));
        }
        let v = serde_json::from_str::<Value>(&text)
            .map_err(|e| EgressError::Protocol(e.to_string()))?;
        match self.provider {
            Provider::OpenAi | Provider::Mistral => from_openai(model, &v),
            Provider::Local => from_local(model, &v),
            _ => from_gemini(model, &v),
        }
    }
}

/// What an error answer of another provider means. Its own words go only into the variant,
/// for the details (D-121).
fn status_error(provider: Provider, status: u16, text: &str) -> EgressError {
    let parsed = serde_json::from_str::<Value>(text).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| v.get("error").and_then(Value::as_str))
                .or_else(|| v.get("message").and_then(Value::as_str))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let code = parsed
        .as_ref()
        .and_then(|v| v.pointer("/error/code"))
        .and_then(Value::as_str);
    match status {
        // Ollama answers a model that is not installed with 404.
        404 if provider == Provider::Local => EgressError::LocalUnavailable,
        401 | 403 => EgressError::Unauthorized,
        // Gemini answers a wrong key with 400.
        400 if message.contains("API key") => EgressError::Unauthorized,
        // OpenAI's empty account is a 429 with its own code, not a rate limit.
        402 => EgressError::Billing(message),
        429 if code == Some("insufficient_quota") => EgressError::Billing(message),
        404 => EgressError::ModelGone(message),
        429 => EgressError::RateLimited,
        500..=599 => EgressError::Unavailable(status),
        _ => EgressError::Rejected(status, message),
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
        admit(bytes.len(), self.provider != Provider::Local)?;
        crate::wait_for_place(self.limit, self.provider != Provider::Local)?;
        retry(|| self.post_once(c.model, &bytes))
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
        "mistral-large-2411",
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
        // D-143: a long answer's ceiling with the thinking budget stays within the models' own.
        let mut long = b.clone();
        long["max_tokens"] = json!(64_000);
        let out = to_gemini(&canonical(&long, Provider::Gemini).unwrap());
        assert_eq!(
            out["generationConfig"]["maxOutputTokens"],
            GEMINI_MAX_OUTPUT
        );
    }

    #[test]
    fn mistral_and_local_bodies_carry_only_the_cleared_text() {
        let b = section_body("mistral-large");
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
        let counted = from_openai(
            "gpt-5.1",
            &json!({"choices": [{"message": {"content": "x"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 12, "completion_tokens": 3}}),
        )
        .unwrap();
        assert_eq!(
            counted["usage"],
            json!({"input_tokens": 12, "output_tokens": 3})
        );
        let counted = from_gemini(
            "gemini-2.5-pro",
            &json!({"candidates": [{"content": {"parts": [{"text": "x"}]}, "finishReason": "STOP"}], "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 4, "thoughtsTokenCount": 6}}),
        )
        .unwrap();
        assert_eq!(
            counted["usage"],
            json!({"input_tokens": 10, "output_tokens": 10})
        );
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
    fn every_provider_address_is_https_to_its_own_host_or_plain_loopback() {
        for (address, host) in [
            (OPENAI_URL, OPENAI_HOST),
            (GEMINI_URL, GEMINI_HOST),
            (MISTRAL_URL, MISTRAL_HOST),
        ] {
            let url = reqwest::Url::parse(address).unwrap();
            assert_eq!(url.scheme(), "https", "{address}");
            assert_eq!(url.host_str(), Some(host), "{address}");
            assert_eq!(url.port(), None, "{address}");
            assert!(url.username().is_empty() && url.query().is_none());
        }
        let local = reqwest::Url::parse(LOCAL_URL).unwrap();
        assert_eq!(
            local.host_str(),
            Some("127.0.0.1"),
            "an IP, not a name DNS could move"
        );
    }

    #[test]
    fn only_listed_models_have_an_api_name_and_it_is_a_fixed_safe_path_segment() {
        let listed: Vec<&str> = OPENAI_MODELS
            .iter()
            .chain(GEMINI_MODELS)
            .chain(MISTRAL_MODELS)
            .chain(LOCAL_MODELS)
            .copied()
            .collect();
        for m in &listed {
            let api = api_model(m).unwrap();
            // Gemini puts the name in the URL path: nothing in it may change the address.
            assert!(
                api.chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c)),
                "{api}"
            );
        }
        for m in crate::ALLOWED_MODELS.iter().chain(&[
            "gpt-4o",
            "gemini-2.5-pro",
            "../x",
            "gpt-5-1 ",
            "GPT-5-1",
            "",
        ]) {
            assert_eq!(api_model(m), None, "{m}");
        }
    }

    #[test]
    fn max_tokens_out_of_range_or_missing_is_refused() {
        for v in [json!(0), json!(64_001), json!(-1), json!("100"), json!(1.5)] {
            let mut b = section_body("gpt-5-1");
            b["max_tokens"] = v.clone();
            assert!(canonical(&b, Provider::OpenAi).is_err(), "{v}");
        }
        let mut b = section_body("gpt-5-1");
        b.as_object_mut().unwrap().remove("max_tokens");
        assert!(canonical(&b, Provider::OpenAi).is_err());
        let mut b = section_body("gpt-5-1");
        b["thinking"] = json!({"type": "adaptive"});
        assert!(
            canonical(&b, Provider::OpenAi).is_err(),
            "thinking is not translated"
        );
        let mut b = section_body("gpt-5-1");
        b["system"] = json!([{"type": "document", "source": {"type": "file", "file_id": "f"}}]);
        assert!(canonical(&b, Provider::OpenAi).is_err(), "only text blocks");
    }

    /// D-121: other providers' error answers, by their real shapes.
    #[test]
    fn other_providers_error_answers_become_their_own_variants() {
        let quota = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#;
        let slow = r#"{"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}"#;
        let gone = r#"{"error":{"message":"The model `gpt-old` does not exist","code":"model_not_found"}}"#;
        let key = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        let cases = [
            (
                Provider::OpenAi,
                429,
                quota,
                EgressError::Billing("You exceeded your current quota".into()),
            ),
            (Provider::OpenAi, 429, slow, EgressError::RateLimited),
            (
                Provider::OpenAi,
                404,
                gone,
                EgressError::ModelGone("The model `gpt-old` does not exist".into()),
            ),
            (Provider::Gemini, 400, key, EgressError::Unauthorized),
            (
                Provider::Mistral,
                402,
                "",
                EgressError::Billing(String::new()),
            ),
            (Provider::Local, 404, "", EgressError::LocalUnavailable),
            (Provider::Gemini, 503, "", EgressError::Unavailable(503)),
            (
                Provider::OpenAi,
                400,
                r#"{"error":{"message":"bad"}}"#,
                EgressError::Rejected(400, "bad".into()),
            ),
        ];
        for (provider, status, text, want) in cases {
            assert_eq!(
                status_error(provider, status, text),
                want,
                "{provider:?} {status}"
            );
        }
    }

    #[test]
    fn transports_follow_the_model_and_need_a_key() {
        assert!(transport_for("gpt-5-1", "sk-test-not-real").is_ok());
        assert!(transport_for("gemini-2-5-flash", "test-not-real").is_ok());
        assert!(transport_for("claude-opus-5", "sk-ant-test-not-real").is_ok());
        assert!(transport_for("mistral-large", "test-not-real").is_ok());
        assert!(
            transport_for("local-gemma", "").is_ok(),
            "the local model takes no key"
        );
        assert!(matches!(
            transport_for("mistral-large", ""),
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

    /// Every value of a full answer, one at a time, replaced by a value of another shape: the
    /// answer is the server's JSON, so it may be refused, never crash the app.
    #[test]
    fn answers_with_the_wrong_shapes_are_read_or_refused_never_a_crash() {
        fn paths(v: &Value, at: String, out: &mut Vec<String>) {
            out.push(at.clone());
            match v {
                Value::Object(m) => {
                    for (k, c) in m {
                        paths(c, format!("{at}/{k}"), out);
                    }
                }
                Value::Array(a) => {
                    for (i, c) in a.iter().enumerate() {
                        paths(c, format!("{at}/{i}"), out);
                    }
                }
                _ => {}
            }
        }
        let odd = [
            Value::Null,
            json!(true),
            json!(u64::MAX),
            json!(-1),
            json!(1.5e300),
            json!("x"),
            json!([]),
            json!({}),
        ];
        let openai = json!({"choices": [{"message": {"content": "שלום", "refusal": null}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 3, "completion_tokens": 2}});
        let local = json!({"message": {"content": "שלום"}, "done_reason": "stop", "prompt_eval_count": 3, "eval_count": 2});
        let gemini = json!({"candidates": [{"content": {"parts": [{"text": "שלום"}, {"text": "x", "thought": true}]}, "finishReason": "STOP"}], "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": u64::MAX, "thoughtsTokenCount": u64::MAX}, "promptFeedback": {}});
        type Read = fn(&str, &Value) -> Result<Value, EgressError>;
        let cases: [(&Value, Read); 3] = [
            (&openai, from_openai),
            (&local, from_local),
            (&gemini, from_gemini),
        ];
        for (answer, read) in cases {
            assert!(read("m", answer).is_ok(), "{answer}");
            let mut all = Vec::new();
            paths(answer, String::new(), &mut all);
            for path in all {
                for o in &odd {
                    let mut changed = answer.clone();
                    match changed.pointer_mut(&path) {
                        Some(slot) => *slot = o.clone(),
                        None => continue,
                    }
                    let r = std::panic::catch_unwind(|| read("m", &changed));
                    assert!(r.is_ok(), "panic at {path:?} = {o}");
                }
            }
        }
    }
}
