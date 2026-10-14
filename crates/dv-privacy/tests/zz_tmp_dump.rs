//! TEMPORARY: dumps every output of the filter on the invented corpus, for a before/after diff.
#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing
)]

#[path = "support/corpus.rs"]
#[allow(dead_code)]
mod harness;

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dv_privacy::{
    clear, filter, filter_split, filter_with_hints, GateRequest, NameHint, PrivacyContext,
};

#[test]
fn zz_tmp_dump() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let practitioner = vec![harness::PRACTITIONER.to_owned()];
    let allow = |_: &str| false;
    let confirmed = |w: &str| w.len() % 7 == 0;
    let mut out = String::new();
    for split in ["dev", "hard", "test"] {
        for doc in harness::load(&corpus, split) {
            let ids = harness::identities(&doc);
            let ctx = PrivacyContext {
                case_id: &doc.case,
                identities: &ids,
                practitioner: &practitioner,
                allowlisted: &allow,
                confirmed_names: &confirmed,
                past_names: &|w: &str| w.len() % 11 == 0,
                today: (2026, 10, 3),
            };
            writeln!(out, "=== {} {}", split, doc.file).unwrap();
            let o = filter(&doc.text, &ctx);
            writeln!(out, "filter {o:?}").unwrap();
            let toks = dv_privacy::text::tokenize(&doc.text);
            let hints: Vec<NameHint> = toks
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 5 == 0)
                .map(|(i, t)| NameHint {
                    start: t.start,
                    end: t.end,
                    score: if i % 2 == 0 { 0.95 } else { 0.5 },
                })
                .collect();
            writeln!(
                out,
                "hints {:?}",
                filter_with_hints(&doc.text, &ctx, &hints)
            )
            .unwrap();
            let mut ranges = Vec::new();
            let mut s = 0;
            for (i, _) in doc.text.match_indices('\n') {
                ranges.push(s..i);
                s = i + 1;
            }
            ranges.push(s..doc.text.len());
            writeln!(out, "split {:?}", filter_split(&doc.text, &ctx, &ranges)).unwrap();
            writeln!(
                out,
                "misreads {:?}",
                dv_privacy::ocr_misreads(&doc.text, &ctx)
            )
            .unwrap();
            writeln!(
                out,
                "patterns {:?}",
                dv_privacy::patterns::find(&doc.text, (2026, 10, 3))
            )
            .unwrap();
            let tags: Vec<String> = ids.iter().map(|i| i.tag.clone()).collect();
            writeln!(
                out,
                "scan {:?}",
                dv_privacy::restore::scan_model_output(&doc.text, &tags, &ids)
            )
            .unwrap();
            writeln!(
                out,
                "remaining {:?}",
                dv_privacy::restore::remaining_tags(&doc.text)
            )
            .unwrap();
            let tagset: HashSet<String> = tags.iter().cloned().collect();
            writeln!(
                out,
                "foreign {:?}",
                dv_privacy::replace_foreign_tags(&doc.text, &tagset, |t| format!("<{t}>"))
            )
            .unwrap();
            for line in doc.text.lines() {
                writeln!(out, "line {:?}", filter(line, &ctx).map(|o| o.tagged)).unwrap();
            }
            if let Ok(o) = o {
                writeln!(
                    out,
                    "restore {:?}",
                    dv_privacy::restore::restore(&o.tagged, &ids, Some(harness::PRACTITIONER))
                )
                .unwrap();
                writeln!(
                    out,
                    "scan_out {:?}",
                    dv_privacy::restore::scan_model_output(&o.tagged, &tags, &ids)
                )
                .unwrap();
                let body = serde_json::json!({"model":"m","messages":[{"role":"user","content":o.tagged}]});
                let case_tags: HashSet<String> = tags.iter().cloned().collect();
                let req = GateRequest {
                    body: &body,
                    ctx: &ctx,
                    case_tags: &case_tags,
                    unresolved_suspects: 0,
                    canaries: &[],
                    max_bytes: 1 << 22,
                };
                writeln!(out, "gate {:?}", clear(&req).map(|p| p.body().len())).unwrap();
                let raw = serde_json::json!({"model":"m","messages":[{"role":"user","content":doc.text}]});
                let req = GateRequest { body: &raw, ..req };
                writeln!(out, "gate_raw {:?}", clear(&req).map(|p| p.body().len())).unwrap();
            }
        }
    }
    let path = std::env::var("ZZ_DUMP").unwrap();
    std::fs::write(path, out).unwrap();
}
