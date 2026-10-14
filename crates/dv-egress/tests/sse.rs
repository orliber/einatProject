//! Properties of putting a streamed answer back together (`dv_egress::sse::assemble`).
//!
//! The network hands the stream over in pieces of any size, so the result must not depend on
//! where the pieces were cut. A stream that is cut short, broken or hostile must end in an
//! error, never in a panic or a half answer read as a whole one.

#![allow(clippy::string_slice)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::cell::RefCell;
use std::io::{BufReader, Read};

use dv_egress::sse::assemble;
use dv_egress::EgressError;
use proptest::prelude::*;
use serde_json::{json, Value};

/// A reader that hands out its bytes in the given piece sizes, as a socket might.
struct Pieces {
    data: Vec<u8>,
    pos: usize,
    sizes: Vec<usize>,
    turn: usize,
}

impl Read for Pieces {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.data.len() || buf.is_empty() {
            return Ok(0);
        }
        let want = self.sizes[self.turn % self.sizes.len()].max(1);
        self.turn += 1;
        let n = want.min(buf.len()).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// The result and every progress report, read through pieces of `sizes` and a buffer of `cap`.
fn run(bytes: &[u8], sizes: &[usize], cap: usize) -> (Result<Value, EgressError>, Vec<u32>) {
    let seen = RefCell::new(Vec::new());
    let reader = BufReader::with_capacity(
        cap.max(1),
        Pieces {
            data: bytes.to_vec(),
            pos: 0,
            sizes: if sizes.is_empty() {
                vec![bytes.len().max(1)]
            } else {
                sizes.to_vec()
            },
            turn: 0,
        },
    );
    let out = assemble(reader, &|w| seen.borrow_mut().push(w));
    (out, seen.into_inner())
}

fn whole(bytes: &[u8]) -> (Result<Value, EgressError>, Vec<u32>) {
    run(bytes, &[], 8 * 1024)
}

fn frame(e: &Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        e["type"].as_str().unwrap_or(""),
        e
    )
}

/// A complete stream: thinking, a text block written in `pieces`, and a tool block whose
/// input arrives as JSON cut at `cuts`.
fn events(pieces: &[String], json_cuts: &[usize]) -> Vec<Value> {
    let input = r#"{"query": "נורמות גיל", "n": [1, 2, 3], "nested": {"ok": true}}"#;
    let mut ev = vec![
        json!({"type": "message_start", "message": {"id": "m", "type": "message", "role": "assistant", "content": [], "stop_reason": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "מחשבה אחת ארוכה"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
    ];
    for p in pieces {
        ev.push(json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": p}}));
    }
    ev.push(json!({"type": "content_block_stop", "index": 1}));
    ev.push(json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "t", "name": "n", "input": {}}}));
    let mut cuts: Vec<usize> = json_cuts
        .iter()
        .map(|c| c % (input.len() + 1))
        .filter(|c| input.is_char_boundary(*c))
        .collect();
    cuts.push(0);
    cuts.push(input.len());
    cuts.sort_unstable();
    cuts.dedup();
    for w in cuts.windows(2) {
        ev.push(json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": &input[w[0]..w[1]]}}));
    }
    ev.push(json!({"type": "content_block_stop", "index": 2}));
    ev.push(json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 77}}));
    ev.push(json!({"type": "message_stop"}));
    ev
}

fn stream(ev: &[Value]) -> String {
    ev.iter().map(frame).collect()
}

fn text_piece() -> impl Strategy<Value = String> {
    prop_oneof![
        "[א-ת]{0,6}",
        "[a-z]{0,6}",
        Just(" ".to_owned()),
        Just("  ".to_owned()),
        Just("\n".to_owned()),
        Just("\t".to_owned()),
        Just("\"".to_owned()),
        Just("{\"reply\": ".to_owned()),
        Just("data: x\n\n".to_owned()),
    ]
}

#[test]
fn the_reference_stream_assembles_into_the_message() {
    let pieces = vec!["שלום ".to_owned(), "עו".to_owned(), "לם".to_owned()];
    let (out, seen) = whole(stream(&events(&pieces, &[5, 17])).as_bytes());
    let m = out.unwrap();
    assert_eq!(m["content"][0]["thinking"], "מחשבה אחת ארוכה");
    assert_eq!(m["content"][1]["text"], "שלום עולם");
    assert_eq!(m["content"][2]["input"]["n"], json!([1, 2, 3]));
    assert_eq!(m["content"][2]["input"]["query"], "נורמות גיל");
    assert_eq!(m["stop_reason"], "end_turn");
    assert_eq!(m["usage"]["output_tokens"], 77);
    assert_eq!(
        seen,
        vec![1, 2, 2],
        "a word cut between two pieces counts once"
    );
}

#[test]
fn crlf_line_ends_and_extra_fields_read_the_same() {
    let ev = events(&["אחת שתיים".to_owned()], &[]);
    let lf = stream(&ev);
    let crlf = lf.replace('\n', "\r\n");
    // Comments, ids, retry hints and a data line without a space are all valid SSE.
    let decorated: String = ev
        .iter()
        .map(|e| format!(": keep-alive\nid: 7\nretry: 1000\nevent: x\ndata:{e}\n\n"))
        .collect();
    let reference = whole(lf.as_bytes());
    assert!(reference.0.is_ok());
    assert_eq!(whole(crlf.as_bytes()), reference);
    assert_eq!(whole(decorated.as_bytes()), reference);
}

#[test]
fn one_event_written_over_several_data_lines_is_joined() {
    let mut ev = events(&["מילה".to_owned()], &[]);
    let pretty: String = ev
        .drain(..)
        .map(|e| {
            let lines: String = serde_json::to_string_pretty(&e)
                .unwrap()
                .lines()
                .map(|l| format!("data: {l}\n"))
                .collect();
            format!("{lines}\n")
        })
        .collect();
    let compact = stream(&events(&["מילה".to_owned()], &[]));
    assert_eq!(whole(pretty.as_bytes()), whole(compact.as_bytes()));
}

#[test]
fn a_stream_without_its_end_or_its_start_is_not_an_answer() {
    let mut ev = events(&["מילה".to_owned()], &[]);
    ev.pop();
    assert_eq!(whole(stream(&ev).as_bytes()).0, Err(EgressError::Offline));
    let mut ev = events(&["מילה".to_owned()], &[]);
    ev.remove(0);
    assert_eq!(whole(stream(&ev).as_bytes()).0, Err(EgressError::Offline));
    assert_eq!(whole(b"").0, Err(EgressError::Offline));
    assert_eq!(whole(b": only a comment\n\n").0, Err(EgressError::Offline));
}

#[test]
fn broken_events_are_protocol_errors() {
    let start = frame(&json!({"type": "message_start", "message": {"content": [], "usage": {}}}));
    let cases = [
        // A delta for a block that never started.
        json!({"type": "content_block_delta", "index": 3, "delta": {"type": "text_delta", "text": "x"}}),
        // Indexes out of range, negative, missing or not a number.
        json!({"type": "content_block_start", "index": 64, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_start", "index": u64::MAX, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_start", "index": -1, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_start", "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_start", "index": "0", "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_stop", "index": 1.5}),
    ];
    for bad in cases {
        let s = format!("{start}{}", frame(&bad));
        assert!(
            matches!(whole(s.as_bytes()).0, Err(EgressError::Protocol(_))),
            "{bad}"
        );
    }
    // Data that is not JSON.
    let s = format!("{start}data: {{not json\n\n");
    assert!(matches!(
        whole(s.as_bytes()).0,
        Err(EgressError::Protocol(_))
    ));
    // Tool input that does not add up to JSON.
    let s = format!(
        "{start}{}{}{}",
        frame(
            &json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "input": {}}})
        ),
        frame(
            &json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"a\": "}})
        ),
        frame(&json!({"type": "content_block_stop", "index": 0})),
    );
    assert!(matches!(
        whole(s.as_bytes()).0,
        Err(EgressError::Protocol(_))
    ));
}

#[test]
fn the_highest_allowed_index_still_works() {
    let s = stream(&[
        json!({"type": "message_start", "message": {"content": [], "usage": {}}}),
        json!({"type": "content_block_start", "index": 63, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 63, "delta": {"type": "text_delta", "text": "סוף"}}),
        json!({"type": "message_stop"}),
    ]);
    let m = whole(s.as_bytes()).0.unwrap();
    let content = m["content"].as_array().unwrap();
    assert_eq!(content.len(), 64);
    assert_eq!(content[63]["text"], "סוף");
}

#[test]
fn error_events_map_to_the_retry_rules() {
    let start = frame(&json!({"type": "message_start", "message": {"content": [], "usage": {}}}));
    let err = |kind: &str| {
        let s = format!(
            "{start}{}",
            frame(&json!({"type": "error", "error": {"type": kind, "message": "m"}}))
        );
        whole(s.as_bytes()).0
    };
    assert_eq!(err("overloaded_error"), Err(EgressError::Unavailable(529)));
    assert_eq!(err("api_error"), Err(EgressError::Unavailable(500)));
    assert_eq!(err("rate_limit_error"), Err(EgressError::RateLimited));
    assert_eq!(
        err("invalid_request_error"),
        Err(EgressError::Rejected(400, "m".to_owned()))
    );
    // An error after a complete answer still wins: the answer is not used.
    let mut ev = events(&["מילה".to_owned()], &[]);
    ev.insert(
        ev.len() - 1,
        json!({"type": "error", "error": {"type": "overloaded_error"}}),
    );
    assert_eq!(
        whole(stream(&ev).as_bytes()).0,
        Err(EgressError::Unavailable(529))
    );
}

#[test]
fn bytes_that_are_not_utf8_end_the_stream_with_an_error() {
    let mut bytes = stream(&events(&["מילה".to_owned()], &[])).into_bytes();
    let at = bytes.len() / 2;
    bytes.insert(at, 0xff);
    assert!(whole(&bytes).0.is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// However the bytes are cut into pieces, and whatever the buffer size, the message and
    /// the sequence of progress reports are the same.
    #[test]
    fn piece_boundaries_never_change_the_result(
        pieces in prop::collection::vec(text_piece(), 0..12),
        cuts in prop::collection::vec(0usize..80, 0..6),
        sizes in prop::collection::vec(1usize..40, 1..8),
        cap in 1usize..64,
    ) {
        let bytes = stream(&events(&pieces, &cuts)).into_bytes();
        let reference = whole(&bytes);
        prop_assert!(reference.0.is_ok());
        prop_assert_eq!(run(&bytes, &sizes, cap), reference);
    }

    /// The word count is the number of words in the text, however the words were cut across
    /// deltas; it never goes down; the thinking is not counted.
    #[test]
    fn words_are_counted_once_however_the_text_is_cut(
        pieces in prop::collection::vec(text_piece(), 1..12),
    ) {
        let (out, seen) = whole(stream(&events(&pieces, &[])).as_bytes());
        let text = pieces.concat();
        prop_assert_eq!(&out.unwrap()["content"][1]["text"], &json!(text));
        prop_assert_eq!(seen.len(), pieces.len(), "one report per text delta");
        prop_assert!(seen.windows(2).all(|w| w[0] <= w[1]));
        let words = u32::try_from(text.split_whitespace().count()).unwrap();
        prop_assert_eq!(seen.last().copied().unwrap_or(0), words);
    }

    /// Event and delta types this program does not know are skipped without changing the
    /// answer (the API adds new ones over time).
    #[test]
    fn unknown_event_and_delta_types_are_ignored(
        names in prop::collection::vec("[a-z_]{1,14}", 1..6),
        at in prop::collection::vec(0usize..12, 1..6),
    ) {
        const KNOWN: &[&str] = &[
            "message_start", "content_block_start", "content_block_delta",
            "content_block_stop", "message_delta", "message_stop", "error",
        ];
        let ev = events(&["אחת ".to_owned(), "שתיים".to_owned()], &[3]);
        let reference = whole(stream(&ev).as_bytes());
        let mut noisy = ev.clone();
        for (name, pos) in names.iter().zip(&at) {
            if KNOWN.contains(&name.as_str()) {
                continue;
            }
            // After the text block started (event 5) and before the last event.
            let pos = 6 + *pos % (noisy.len() - 6);
            noisy.insert(pos, json!({"type": name, "index": 1, "payload": {"text": "רעש"}}));
            // An unknown delta type on the text block adds nothing either.
            noisy.insert(pos + 1, json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": format!("{name}_delta"), "text": "רעש"}}));
        }
        prop_assume!(noisy.len() > ev.len());
        let (out, _) = whole(stream(&noisy).as_bytes());
        prop_assert_eq!(out, reference.0);
    }

    /// Any cut before the final event is complete is an error, never a shorter answer.
    #[test]
    fn a_stream_cut_anywhere_before_its_end_is_an_error(
        pieces in prop::collection::vec(text_piece(), 0..6),
        cut in 0.0f64..1.0,
    ) {
        let s = stream(&events(&pieces, &[11]));
        let end = s.rfind(r#"{"type":"message_stop"}"#).unwrap() + r#"{"type":"message_stop"}"#.len();
        let mut at = (cut * end as f64) as usize;
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        prop_assert!(at < end);
        prop_assert!(whole(&s.as_bytes()[..at]).0.is_err());
    }

    /// Arbitrary bytes, and valid streams with bytes changed, give an answer or an error,
    /// never a panic.
    #[test]
    fn arbitrary_or_mangled_bytes_never_panic(
        noise in prop::collection::vec(any::<u8>(), 0..400),
        flips in prop::collection::vec((any::<usize>(), any::<u8>()), 0..8),
    ) {
        let _ = whole(&noise);
        let mut bytes = stream(&events(&["א ב".to_owned()], &[4])).into_bytes();
        for (pos, b) in flips {
            let i = pos % bytes.len();
            bytes[i] = b;
        }
        let _ = whole(&bytes);
        let with_prefix = [b"data: ".as_slice(), &noise, b"\n\n"].concat();
        let _ = whole(&with_prefix);
    }
}
