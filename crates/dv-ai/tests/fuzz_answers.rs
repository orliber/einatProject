//! Mutation fuzzing of every reader of the AI's answers (S3). The answer's text is the
//! model's, so it is read as untrusted input: damaged JSON, wrong types, huge numbers, cut
//! answers. A reader may refuse, but must never panic and never run away. Deterministic (the
//! seed is printed); `DV_FUZZ_ROUNDS` raises the rounds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

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

fn api(text: &str) -> Value {
    json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": text}]})
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// One reader: its name, a valid answer, and how it is read.
type Reader = (&'static str, Value, Box<dyn Fn(&Value) -> bool>);

fn readers() -> Vec<Reader> {
    let sources = vec![("S1".to_owned(), "תצפית בדויה: [ילד] משחק לבד.".to_owned())];
    let sections = ids(&["background", "kindergarten", "cognitive", "summary"]);
    vec![
        (
            "section",
            json!({
                "reply": "ניסחתי",
                "paragraphs": [
                    {"text": "[ילד] הציג תפקוד ממוצע (9) במטלת אוצר מילים.", "source_refs": ["S1"]},
                    {"text": "משפט בלי מקור.", "source_refs": []}
                ],
                "questions": ["מה גילו?"], "missing": ["אינטייק"], "contradictions": []
            }),
            Box::new(move |r| dv_ai::parse_section(r, &sources).is_ok()),
        ),
        (
            "derived section",
            json!({
                "reply": "סיכמתי",
                "paragraphs": [{"text": "ציון כולל של 83.", "source_refs": []}],
                "questions": [], "missing": [], "contradictions": []
            }),
            Box::new(|r| dv_ai::parse_derived_section(r, &ids(&["אוצר מילים: 9."])).is_ok()),
        ),
        (
            "consult",
            json!({"reply": "שאלה טובה", "paragraphs": []}),
            Box::new(|r| dv_ai::parse_consult(r).is_ok()),
        ),
        (
            "sort",
            json!({"sections": [
                {"section": "background", "passages": ["S1P2", "S1P1"]},
                {"section": "kindergarten", "passages": ["S2P1"]}
            ]}),
            {
                let sections = sections.clone();
                Box::new(move |r| dv_ai::parse_sort(r, &[2, 1], &sections).is_ok())
            },
        ),
        (
            "style analysis",
            json!({"items": [
                {"section": "cognitive", "kind": "rule", "text": "תיאור מילולי ואחריו הציון בסוגריים"},
                {"section": "general", "kind": "phrase", "text": "עם זאת, ניכר"}
            ]}),
            {
                let sections = sections.clone();
                Box::new(move |r| dv_ai::parse_style_analysis(r, &sections).is_ok())
            },
        ),
        (
            "style profile",
            json!({"items": [
                {"section": "summary", "kind": "template", "text": "[ילד] מגיב לתיווך ____", "support": 3},
                {"section": "summary", "kind": "example", "text": "דוגמה בדויה", "support": 2}
            ]}),
            Box::new(move |r| dv_ai::parse_style_synthesis(r, &sections).is_ok()),
        ),
        (
            "verify",
            json!({"results": [
                {"paragraph": "P1", "verdict": "supported", "notes": []},
                {"paragraph": "P2", "verdict": "contradicted", "notes": [
                    {"note": "המקור אומר אחרת", "source_refs": ["S1"]}
                ]}
            ]}),
            Box::new(|r| dv_ai::parse_verify(r, &ids(&["P1", "P2"]), &ids(&["S1"])).is_ok()),
        ),
    ]
}

/// Damages the answer two ways: the model's text (inside the envelope), and the envelope
/// itself (when it still reads as JSON).
fn damaged(rng: &mut Rng, answer: &Value) -> Value {
    if rng.below(3) == 0 {
        let outer = api(&answer.to_string()).to_string();
        let bytes = mutate(rng, outer.as_bytes(), b"{}[]\":,0123456789-en ");
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            return v;
        }
    }
    let inner = answer.to_string();
    let bytes = mutate(rng, inner.as_bytes(), b"{}[]\":,0123456789-en ");
    api(&String::from_utf8_lossy(&bytes))
}

#[test]
fn damaged_answers_never_panic_or_hang() {
    let seed0 = seed0();
    for (name, answer, read) in readers() {
        // The undamaged answer reads, so the damage starts from something real.
        assert!(
            read(&api(&answer.to_string())),
            "the undamaged {name} answer must read"
        );
        for round in 0..rounds() {
            let seed = seed0 ^ round.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ name.len() as u64;
            let mut rng = Rng(seed | 1);
            let response = damaged(&mut rng, &answer);
            let started = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| read(&response)));
            assert!(
                result.is_ok(),
                "panic reading a damaged {name} answer: round {round}, seed {seed}"
            );
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{name} took {:?} (round {round}, seed {seed})",
                started.elapsed()
            );
        }
    }
}

#[test]
fn answers_of_the_wrong_shape_never_panic() {
    let shapes = [
        json!(null),
        json!([]),
        json!("text"),
        json!({"stop_reason": "end_turn"}),
        json!({"stop_reason": "end_turn", "content": "x"}),
        json!({"stop_reason": "end_turn", "content": [null, 5, {"type": "text"}]}),
        json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": 7}]}),
        json!({"stop_reason": 7, "content": []}),
        json!({"stop_reason": "max_tokens", "content": [{"type": "text", "text": "{"}]}),
        json!({"stop_reason": "refusal", "content": []}),
        api("null"),
        api("[]"),
        api("{}"),
        api(&"[".repeat(10_000)),
        api(&format!(
            "{{\"paragraphs\": [{}]}}",
            "{\"text\": 1e999},".repeat(50) + "{}"
        )),
        api(
            r#"{"sections": [{"section": "background", "passages": ["S0P0", "S99999999999999999999P1", "SP", "S1P-1"]}]}"#,
        ),
        api(r#"{"items": [{"section": null, "kind": "rule", "text": ""}, {"kind": 5}]}"#),
        api(
            r#"{"results": [{"paragraph": "P1", "verdict": "supported", "notes": [{"note": null, "source_refs": "S1"}]}]}"#,
        ),
    ];
    for shape in shapes {
        for (name, _, read) in readers() {
            let result = catch_unwind(AssertUnwindSafe(|| read(&shape)));
            assert!(result.is_ok(), "panic in {name} on {shape}");
        }
    }
}
