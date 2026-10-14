//! The stage 5 report: the gold corpus (dv-privacy `tests/corpus`) filtered without and with
//! the local name model, plus its speed, memory and size. It only runs when `DV_NER_MODEL`
//! names a folder with the model files; nothing here reaches the network.
//!
//! ```text
//! DV_NER_MODEL=/path/to/dictabert-ner cargo test --release -p dv-ner --test corpus_with_model -- --nocapture
//! ```
//!
//! `DV_NER_MIN_SCORE` (0–1) drops hints below that confidence, to see the trade-off. `test/`
//! stays sealed: its misses are listed only with `CORPUS_SHOW_TEST=1`.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing
)]

#[path = "../../dv-privacy/tests/support/corpus.rs"]
mod harness;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dv_ner::NerModel;
use dv_privacy::NameHint;
use harness::{load, measure, print};

/// Peak resident memory of this process, from /proc (Linux only).
fn peak_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

#[test]
fn corpus_without_and_with_the_model() {
    let Some(dir) = std::env::var_os("DV_NER_MODEL").map(PathBuf::from) else {
        println!("DV_NER_MODEL is not set: the model report is skipped");
        return;
    };
    let min_score: f32 = std::env::var("DV_NER_MIN_SCORE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let show_test = std::env::var_os("CORPUS_SHOW_TEST").is_some();
    let loaded = Instant::now();
    let model = NerModel::load(&dir).unwrap();
    println!(
        "model: {:.0} MB on disk, loaded in {:.1} s",
        std::fs::metadata(dir.join("model.safetensors")).map_or(0.0, |m| m.len() as f64 / 1e6),
        loaded.elapsed().as_secs_f64()
    );

    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dv-privacy/tests/corpus");
    for split in ["dev", "test"] {
        let docs = load(&corpus, split);
        let before = measure(&docs, &|_| Vec::new());
        let words = Cell::new(0_usize);
        let secs = Cell::new(0.0_f64);
        let hints = |text: &str| {
            let t = Instant::now();
            let people = model.people(text).unwrap();
            secs.set(secs.get() + t.elapsed().as_secs_f64());
            words.set(words.get() + text.split_whitespace().count());
            people
                .into_iter()
                .filter(|p| p.score >= min_score)
                .map(|p| NameHint {
                    start: p.start,
                    end: p.end,
                    score: p.score,
                })
                .collect()
        };
        let after = measure(&docs, &hints);
        println!("\n######## {split}: rules only");
        print(split, &before, false);
        println!("\n######## {split}: rules + model (hints from {min_score})");
        let details = split == "dev" || show_test;
        print(split, &after, false);
        if details {
            for m in after.misses.iter().filter(|m| before.misses.contains(m)) {
                println!("   STILL MISSED  {m}");
            }
            for m in before.misses.iter().filter(|m| !after.misses.contains(m)) {
                println!("   NOW FOUND     {m}");
            }
            for w in after.wrong.iter().filter(|w| !before.wrong.contains(w)) {
                println!("   NEW WRONG     {w}");
            }
        }
        println!(
            "   model time: {:.0} ms per 1,000 words (a page is about 300)",
            1000.0 * 1000.0 * secs.get() / words.get().max(1) as f64
        );
    }
    if let Some(mb) = peak_mb() {
        println!("\npeak memory of the whole run: {mb:.0} MB");
    }
}
