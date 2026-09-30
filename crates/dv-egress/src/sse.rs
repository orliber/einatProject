//! A streamed answer (server-sent events) put back together into the same message the
//! non-streamed API returns, while counting the words as they arrive. Streaming changes
//! nothing about what is sent; it only lets the program show real progress and keeps a long
//! answer from running into a timeout.

use std::io::BufRead;

use serde_json::{json, Map, Value};

use crate::EgressError;

/// Rebuild the final message from the event stream. `progress` gets the number of words
/// written so far (text only, not the model's thinking).
pub fn assemble(reader: impl BufRead, progress: &dyn Fn(u32)) -> Result<Value, EgressError> {
    let mut message: Option<Map<String, Value>> = None;
    let mut blocks: Vec<Value> = Vec::new();
    let mut partial_json: Vec<String> = Vec::new();
    let mut words: u32 = 0;
    let mut in_word = false;
    let mut data = String::new();
    let mut done = false;

    let mut handle = |data: &str,
                      message: &mut Option<Map<String, Value>>,
                      blocks: &mut Vec<Value>,
                      partial_json: &mut Vec<String>,
                      done: &mut bool|
     -> Result<(), EgressError> {
        let ev: Value =
            serde_json::from_str(data).map_err(|e| EgressError::Protocol(e.to_string()))?;
        match ev["type"].as_str().unwrap_or("") {
            "message_start" => {
                *message = ev["message"].as_object().cloned();
            }
            "content_block_start" => {
                let i = index(&ev)?;
                while blocks.len() <= i {
                    blocks.push(Value::Null);
                    partial_json.push(String::new());
                }
                blocks[i] = ev["content_block"].clone();
            }
            "content_block_delta" => {
                let i = index(&ev)?;
                let block = blocks
                    .get_mut(i)
                    .ok_or_else(|| EgressError::Protocol("delta before its block".to_owned()))?;
                let delta = &ev["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let piece = delta["text"].as_str().unwrap_or("");
                        for c in piece.chars() {
                            let space = c.is_whitespace();
                            if !space && !in_word {
                                words = words.saturating_add(1);
                            }
                            in_word = !space;
                        }
                        append(block, "text", piece);
                        progress(words);
                    }
                    "thinking_delta" => {
                        append(block, "thinking", delta["thinking"].as_str().unwrap_or(""))
                    }
                    "signature_delta" => append(
                        block,
                        "signature",
                        delta["signature"].as_str().unwrap_or(""),
                    ),
                    "input_json_delta" => {
                        if let Some(p) = partial_json.get_mut(i) {
                            p.push_str(delta["partial_json"].as_str().unwrap_or(""));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = index(&ev)?;
                if let (Some(block), Some(json)) = (blocks.get_mut(i), partial_json.get(i)) {
                    if !json.is_empty() {
                        block["input"] = serde_json::from_str(json)
                            .map_err(|e| EgressError::Protocol(e.to_string()))?;
                    }
                }
            }
            "message_delta" => {
                if let Some(m) = message.as_mut() {
                    for (k, v) in ev["delta"].as_object().into_iter().flatten() {
                        m.insert(k.clone(), v.clone());
                    }
                    if let Some(out) = ev["usage"]["output_tokens"].as_u64() {
                        if let Some(usage) = m.get_mut("usage").and_then(Value::as_object_mut) {
                            usage.insert("output_tokens".to_owned(), json!(out));
                        }
                    }
                }
            }
            "message_stop" => *done = true,
            "error" => {
                let kind = ev["error"]["type"].as_str().unwrap_or("");
                return Err(match kind {
                    "overloaded_error" => EgressError::Unavailable(529),
                    "rate_limit_error" => EgressError::RateLimited,
                    "api_error" => EgressError::Unavailable(500),
                    _ => EgressError::Rejected(
                        400,
                        ev["error"]["message"].as_str().unwrap_or("").to_owned(),
                    ),
                });
            }
            _ => {} // "ping" and anything new
        }
        Ok(())
    };

    for line in reader.lines() {
        let line = line.map_err(|_| EgressError::Offline)?;
        if line.is_empty() {
            if !data.is_empty() {
                handle(
                    &data,
                    &mut message,
                    &mut blocks,
                    &mut partial_json,
                    &mut done,
                )?;
                data.clear();
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        }
    }
    if !data.is_empty() {
        handle(
            &data,
            &mut message,
            &mut blocks,
            &mut partial_json,
            &mut done,
        )?;
    }
    let (Some(mut message), true) = (message, done) else {
        // A stream cut in the middle is not an answer.
        return Err(EgressError::Offline);
    };
    message.insert("content".to_owned(), Value::Array(blocks));
    Ok(Value::Object(message))
}

fn index(ev: &Value) -> Result<usize, EgressError> {
    ev["index"]
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .filter(|i| *i < 64)
        .ok_or_else(|| EgressError::Protocol("event without a valid index".to_owned()))
}

fn append(block: &mut Value, field: &str, piece: &str) {
    let now = block[field].as_str().unwrap_or("").to_owned();
    block[field] = Value::String(now + piece);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn stream(events: &[Value]) -> String {
        events
            .iter()
            .map(|e| {
                format!(
                    "event: {}\ndata: {}\n\n",
                    e["type"].as_str().unwrap_or(""),
                    e
                )
            })
            .collect()
    }

    #[test]
    fn a_stream_becomes_the_same_message_and_counts_words() {
        let s = stream(&[
            json!({"type": "message_start", "message": {"id": "m", "type": "message", "role": "assistant", "content": [], "stop_reason": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "חושב"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "abc"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "ping"}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "{\"reply\": \"שלום "}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "עולם\"}"}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 42}}),
            json!({"type": "message_stop"}),
        ]);
        let seen = Cell::new(0);
        let m = assemble(s.as_bytes(), &|w| seen.set(w)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m["content"][1]["text"], "{\"reply\": \"שלום עולם\"}");
        assert_eq!(m["content"][0]["thinking"], "חושב");
        assert_eq!(m["content"][0]["signature"], "abc");
        assert_eq!(m["stop_reason"], "end_turn");
        assert_eq!(m["usage"]["output_tokens"], 42);
        assert_eq!(seen.get(), 3, "words in the text, not in the thinking");
    }

    #[test]
    fn an_error_event_or_a_cut_stream_is_not_an_answer() {
        let s = stream(&[
            json!({"type": "message_start", "message": {"content": [], "usage": {}}}),
            json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}}),
        ]);
        assert_eq!(
            assemble(s.as_bytes(), &|_| {}),
            Err(EgressError::Unavailable(529))
        );
        let s = stream(&[
            json!({"type": "message_start", "message": {"content": [], "usage": {}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "חצי"}}),
        ]);
        assert_eq!(assemble(s.as_bytes(), &|_| {}), Err(EgressError::Offline));
    }
}
