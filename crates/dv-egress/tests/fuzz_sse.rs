//! Mutation fuzzing of the stream reader (S3): the answer arrives as server-sent events from
//! the internet, so a damaged, cut or hostile stream is read here before anything else in
//! the app sees it. It may be refused, but must never panic and never run away.
//! Deterministic (the seed is printed); `DV_FUZZ_ROUNDS` raises the rounds.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// xorshift64*: small, fixed, no dependency (the same as `dv-ingest/tests/fuzz.rs`).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn rounds() -> u64 {
    std::env::var("DV_FUZZ_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
}

fn seed0() -> u64 {
    std::env::var("DV_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5eed_a15e)
}

/// Flipped bytes, cut short, repeated chunks, dropped chunks, and the stream's own
/// punctuation in the wrong places.
fn mutate(rng: &mut Rng, original: &[u8], punctuation: &[u8]) -> Vec<u8> {
    let mut b = original.to_vec();
    if b.is_empty() {
        return b;
    }
    for _ in 0..=rng.below(4) {
        match rng.below(6) {
            0 => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(b.len());
                b[i] = punctuation[rng.below(punctuation.len())];
            }
            2 => b.truncate(rng.below(b.len()) + 1),
            3 => {
                let start = rng.below(b.len());
                let end = (start + rng.below(64) + 1).min(b.len());
                let chunk = b[start..end].to_vec();
                for _ in 0..=rng.below(50) {
                    b.splice(start..start, chunk.clone());
                }
            }
            4 => {
                let i = rng.below(b.len());
                b.drain(i..(i + rng.below(32) + 1).min(b.len()));
                if b.is_empty() {
                    b.push(b'{');
                }
            }
            _ => {
                let i = rng.below(b.len());
                let wild: &[u8] = match rng.below(4) {
                    0 => b"99999999999999999999",
                    1 => b"-1",
                    2 => b"null",
                    _ => b"[[[[[[[[[[",
                };
                b.splice(i..i, wild.iter().copied());
            }
        }
    }
    b
}

fn stream(events: &[Value]) -> Vec<u8> {
    events
        .iter()
        .map(|e| {
            format!(
                "event: {}\ndata: {}\n\n",
                e["type"].as_str().unwrap_or(""),
                e
            )
        })
        .collect::<String>()
        .into_bytes()
}

fn seed_stream() -> Vec<u8> {
    stream(&[
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
    ])
}

#[test]
fn damaged_streams_never_panic_or_hang() {
    let original = seed_stream();
    assert!(
        dv_egress::sse::assemble(original.as_slice(), &|_| {}).is_ok(),
        "the undamaged stream must read"
    );
    let seed0 = seed0();
    for round in 0..rounds() {
        let seed = seed0 ^ round.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut rng = Rng(seed | 1);
        let bytes = mutate(&mut rng, &original, b"\n:{}[]\",0123456789 ");
        let started = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            dv_egress::sse::assemble(bytes.as_slice(), &|_| {})
        }));
        assert!(
            result.is_ok(),
            "panic reading a damaged stream: round {round}, seed {seed}, {} bytes",
            bytes.len()
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?} (round {round}, seed {seed})",
            started.elapsed()
        );
    }
}

#[test]
fn hostile_indexes_and_shapes_are_refused_without_a_panic() {
    for events in [
        vec![
            json!({"type": "content_block_delta", "index": 4_000_000_000_u64, "delta": {"type": "text_delta", "text": "x"}}),
        ],
        vec![
            json!({"type": "content_block_start", "index": -1, "content_block": {"type": "text"}}),
        ],
        vec![
            json!({"type": "content_block_start", "index": 1_000_000, "content_block": {"type": "text", "text": ""}}),
        ],
        vec![
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": 5}}),
        ],
        vec![json!({"type": "message_start", "message": "not an object"})],
        vec![json!({"type": "message_delta", "delta": null, "usage": []})],
        vec![json!({"type": "error", "error": {"type": "overloaded_error"}})],
        vec![json!([1, 2, 3])],
    ] {
        let bytes = stream(&events);
        let result = catch_unwind(AssertUnwindSafe(|| {
            dv_egress::sse::assemble(bytes.as_slice(), &|_| {})
        }));
        assert!(result.is_ok(), "panic on {events:?}");
    }
}

#[test]
fn an_answer_that_does_not_end_is_refused() {
    // One delta repeated past the limit, then a proper end: refused, not read.
    let start = stream(&[
        json!({"type": "message_start", "message": {"id": "m", "type": "message", "role": "assistant", "content": [], "stop_reason": null}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
    ]);
    let delta = stream(&[
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "מילה ".repeat(500)}}),
    ]);
    let end = stream(&[
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_stop"}),
    ]);
    let mut bytes = start;
    while bytes.len() <= dv_egress::MAX_ANSWER_BYTES {
        bytes.extend_from_slice(&delta);
    }
    bytes.extend_from_slice(&end);
    let started = Instant::now();
    let result = dv_egress::sse::assemble(bytes.as_slice(), &|_| {});
    assert!(
        matches!(result, Err(dv_egress::EgressError::Protocol(_))),
        "{:?}",
        result.map(|_| ())
    );
    assert!(started.elapsed() < Duration::from_secs(20));
}
