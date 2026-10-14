//! The filter measured on whole documents (tests/corpus/README.md).
//!
//! Each corpus file is an invented document with every identifying span annotated, and a
//! few ordinary words that must pass marked `keep`. The test strips the annotations, runs
//! the filter as the case would, and prints per split (dev / test) and per group:
//! recall, ordinary words hidden by mistake (per 1,000 words) and the items the summary
//! card lists per case. Names the filter finds are kept with the case, as dv-core keeps
//! them, so each document is filtered twice and later documents of a case know them.
//! It fails when a number gets worse than the floor recorded below, so every change to the
//! filter shows up as a number. `test/` is sealed: its misses are listed only with
//! `CORPUS_SHOW_TEST=1`, for the final review, never for tuning.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing
)]

#[path = "support/corpus.rs"]
mod harness;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use dv_domain::{assign_tag, Identity};
use dv_privacy::text::normalize;
use dv_privacy::{clear, filter, GateRequest, PrivacyContext};
use harness::{
    check, identities, keep_found, load, marked_ranges, measure, print, Doc, Floor, PRACTITIONER,
};

// Raised with stage 2 of the filter (D-042, 2026-10-03). Measured on v0.2.0, before it:
// dev name .727 place .604 inst .308, 11.1 false hides per 1,000 words, 20.7 questions per
// case; test name .702 place .644 inst .227, 11.3 per 1,000, 27.2 per case.
// Stage 3 (2026-10-04): no questions; names found are kept with the case. Adjacent hides
// count as one (stage 2 on that count: dev 2.6, test 4.9 per 1,000). Test gains 5 names
// for 4 more false hides (5.3), from names carried to the case's later documents.
// Strong contexts for names that are also words (2026-10-11, pipeline/context.rs): dev
// name .852 → .931, place .887 → .925 at the same false hides; test (not looked at) name
// .860 → .891, place .733 → .800 for one more false hide and two more words kept as names.
// Leaks weigh more than false hides, so the trade is taken.
const DEV_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 1.0),
        ("name", 0.93),
        ("surname", 1.0),
        ("place", 0.92),
        ("inst", 0.82),
        ("num", 1.0),
        ("date", 1.0),
    ],
    max_false_per_1000: 2.5,
};

const TEST_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 0.99),
        ("name", 0.89),
        ("surname", 0.96),
        ("place", 0.80),
        ("inst", 0.68),
        ("num", 1.0),
        ("date", 1.0),
    ],
    max_false_per_1000: 5.42,
};

// ---------------------------------------------------------------- the whole flow (D-050, A)

/// What the app does with a case's documents, measured: every document filtered with every
/// case's names (as dv-core passes them), the names found kept with the case, and the final
/// gate run on what would go out. A block that the summary card cannot undo is a dead end:
/// she sees a red line and only "חזרה לעריכה".
#[derive(Debug, Default)]
struct Flow {
    docs: usize,
    /// documents whose request the gate refused
    blocked: usize,
    /// refused documents with at least one reason the card cannot undo
    dead_ends: usize,
    /// gate reason code → how many times
    reasons: BTreeMap<String, u32>,
    /// names kept with a case that are none of its gold spans (ordinary words kept as names)
    kept_wrong: BTreeSet<String>,
    words: usize,
    /// hides of what must pass ("keep") and hides of anything not gold, as the app does them
    keep_broken: u32,
    false_hides: u32,
    /// The rows of the summary card for each case, as the app groups them (U2): one row per
    /// person, one per generalized wording. Which of them ask for a look (a name inside a
    /// word, a close spelling, a weaker sign).
    card: BTreeMap<String, BTreeSet<String>>,
    card_look: BTreeMap<String, BTreeSet<String>>,
    details: Vec<String>,
}

impl Flow {
    /// The card rows a case shows, on average over the cases, and at most.
    fn card_per_case(&self) -> (f64, usize) {
        let n = self.card.len().max(1);
        let total: usize = self.card.values().map(BTreeSet::len).sum();
        let most = self.card.values().map(BTreeSet::len).max().unwrap_or(0);
        (total as f64 / n as f64, most)
    }

    fn look_per_case(&self) -> f64 {
        let total: usize = self.card_look.values().map(BTreeSet::len).sum();
        total as f64 / self.card.len().max(1) as f64
    }

    fn note_card(&mut self, case: &str, items: &[dv_privacy::AutoHidden]) {
        use dv_privacy::AutoKind;
        let rows = self.card.entry(case.to_owned()).or_default();
        let look = self.card_look.entry(case.to_owned()).or_default();
        for a in items {
            let key = if a.kind == AutoKind::Indirect {
                format!("{:?}|{}|{}", a.kind, normalize(&a.token), a.tag)
            } else {
                format!("{:?}|{}", a.kind, a.tag)
            };
            if a.uncertain || matches!(a.kind, AutoKind::DeclaredInWord | AutoKind::SimilarSpelling)
            {
                look.insert(key.clone());
            }
            rows.insert(key);
        }
    }
}

/// The words of every span that must be hidden in a case, normalized.
fn gold_words(docs: &[Doc], case: &str) -> BTreeSet<String> {
    docs.iter()
        .filter(|d| d.case == case)
        .flat_map(|d| d.spans.iter())
        .filter(|s| s.category != "keep")
        .flat_map(|s| {
            normalize(&s.surface)
                .split(' ')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn measure_flow(docs: &[Doc]) -> Flow {
    let mut flow = Flow::default();
    let practitioner = vec![PRACTITIONER.to_owned()];
    let allow = |_: &str| false;
    let confirmed = |_: &str| false;
    let mut declared: BTreeMap<String, Vec<Identity>> = BTreeMap::new();
    for doc in docs {
        declared
            .entry(doc.case.clone())
            .or_insert_with(|| identities(doc));
    }
    let mut kept: BTreeMap<String, Vec<Identity>> = BTreeMap::new();
    for doc in docs {
        // This case's names (declared, then kept, tags kept apart), then every other case's.
        let own = |kept: &BTreeMap<String, Vec<Identity>>| -> Vec<Identity> {
            let mut ids = declared[&doc.case].clone();
            for k in kept.get(&doc.case).into_iter().flatten() {
                let mut k = k.clone();
                if ids.iter().any(|i| i.tag == k.tag) {
                    let used: Vec<String> = ids.iter().map(|i| i.tag.clone()).collect();
                    k.tag = assign_tag(k.role, &used);
                }
                ids.push(k);
            }
            ids
        };
        let everyone = |own: &[Identity], kept: &BTreeMap<String, Vec<Identity>>| {
            let mut all = own.to_vec();
            for (case, ids) in &declared {
                if *case != doc.case {
                    all.extend(ids.iter().cloned());
                    all.extend(kept.get(case).into_iter().flatten().cloned());
                }
            }
            all
        };
        let mut out = None;
        // Like dv-core: filter, keep what was found, build again with it.
        for _ in 0..2 {
            let mine = own(&kept);
            let all = everyone(&mine, &kept);
            let ctx = PrivacyContext {
                case_id: &doc.case,
                identities: &all,
                practitioner: &practitioner,
                allowlisted: &allow,
                confirmed_names: &confirmed,
                past_names: &|_: &str| false,
                today: (2026, 10, 3),
            };
            let o = filter(&doc.text, &ctx).unwrap();
            flow.note_card(&doc.case, &o.auto_hidden);
            keep_found(
                &o,
                &doc.case,
                kept.entry(doc.case.clone()).or_default(),
                &mine,
            );
            out = Some(o);
        }
        let out = out.unwrap();
        flow.words += doc.text.split_whitespace().count();
        for (ms, me, piece) in marked_ranges(doc, &out) {
            let hit = |keep: bool| {
                doc.spans
                    .iter()
                    .any(|s| (s.category == "keep") == keep && s.start < me && ms < s.end)
            };
            if hit(true) {
                flow.keep_broken += 1;
            }
            if !hit(false) {
                flow.false_hides += 1;
                let around = |a: usize, b: usize| {
                    doc.text[a..b]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                let before = doc.text[..ms]
                    .char_indices()
                    .rev()
                    .nth(24)
                    .map_or(0, |(i, _)| i);
                let after = doc.text[me..]
                    .char_indices()
                    .nth(24)
                    .map_or(doc.text.len(), |(i, _)| me + i);
                flow.details.push(format!(
                    "{}: «{piece}» hidden (not gold) · {} ‹{piece}› {}",
                    doc.file,
                    around(before, ms),
                    around(me, after)
                ));
            }
        }
        let mine = own(&kept);
        let all = everyone(&mine, &kept);
        let ctx = PrivacyContext {
            case_id: &doc.case,
            identities: &all,
            practitioner: &practitioner,
            allowlisted: &allow,
            confirmed_names: &confirmed,
            past_names: &|_: &str| false,
            today: (2026, 10, 3),
        };
        let tags: BTreeSet<String> = mine.iter().map(|i| i.tag.clone()).collect();
        let tags: std::collections::HashSet<String> = tags.into_iter().collect();
        let body = serde_json::json!({"messages": [{"role": "user", "content": out.tagged}]});
        let verdict = clear(&GateRequest {
            body: &body,
            ctx: &ctx,
            case_tags: &tags,
            unresolved_suspects: 0,
            canaries: &[],
            max_bytes: 4_000_000,
        });
        flow.docs += 1;
        if let Err(blocked) = verdict {
            flow.blocked += 1;
            let mut dead = false;
            for r in &blocked.reasons {
                *flow.reasons.entry(r.code.clone()).or_default() += 1;
                let detail = r.detail.clone().unwrap_or_default();
                // What the card can undo (dv-core): a name kept automatically, in any case,
                // or one the filter hid in this request.
                let undoable = r.code == "identity"
                    && (all.iter().any(|i| {
                        i.source != dv_domain::IdentitySource::Manual
                            && std::iter::once(&i.value)
                                .chain(i.aliases.iter())
                                .any(|v| normalize(v) == normalize(&detail))
                    }) || out
                        .auto_hidden
                        .iter()
                        .any(|a| normalize(&a.token) == normalize(&detail)));
                if !undoable {
                    dead = true;
                }
                flow.details.push(format!(
                    "{}: refused {} «{detail}»{}",
                    doc.file,
                    r.code,
                    if undoable { "" } else { " (dead end)" }
                ));
            }
            if dead {
                flow.dead_ends += 1;
            }
        }
    }
    for (case, ids) in &kept {
        let gold = gold_words(docs, case);
        for i in ids {
            if !normalize(&i.value).split(' ').any(|w| gold.contains(w)) {
                flow.kept_wrong
                    .insert(format!("{case}: «{}» ({})", i.value, i.reason));
            }
        }
    }
    flow
}

fn print_flow(split: &str, f: &Flow, details: bool) {
    println!(
        "   flow: {} documents · false hides {} ({:.1} per 1,000 words, {} of them «keep») · refused {} · dead ends {} · ordinary words kept as names {} · reasons {:?}",
        f.docs,
        f.false_hides,
        f64::from(f.false_hides) * 1000.0 / f.words.max(1) as f64,
        f.keep_broken,
        f.blocked,
        f.dead_ends,
        f.kept_wrong.len(),
        f.reasons
    );
    if details {
        for d in &f.details {
            println!("   GATE  {d}");
        }
        for k in &f.kept_wrong {
            println!("   KEPT  {k}");
        }
    }
    let (card, most) = f.card_per_case();
    println!(
        "   card ({split}): {card:.1} rows per case (most {most}), {:.2} of them worth a look · target below 8",
        f.look_per_case()
    );
}

/// Floors of the whole flow: they may only go down. Stage B of D-050 brings dead ends to 0.
struct FlowFloor {
    max_blocked: usize,
    max_dead_ends: usize,
    max_kept_wrong: usize,
    max_false_per_1000: f64,
    /// Rows of the summary card that ask for a look, per case on average (U2, target below 8).
    max_card_look: f64,
}

/// What the card may ask her to look at in one case, whatever the floors say (U2).
const CARD_LOOK_TARGET: f64 = 8.0;

fn check_flow(split: &str, f: &Flow, floor: &FlowFloor) -> Vec<String> {
    let mut out = Vec::new();
    let false_rate = f64::from(f.false_hides) * 1000.0 / f.words.max(1) as f64;
    if f.blocked > floor.max_blocked {
        out.push(format!(
            "{split}: {} refused, floor {}",
            f.blocked, floor.max_blocked
        ));
    }
    if f.dead_ends > floor.max_dead_ends {
        out.push(format!(
            "{split}: {} dead ends, floor {}",
            f.dead_ends, floor.max_dead_ends
        ));
    }
    if f.kept_wrong.len() > floor.max_kept_wrong {
        out.push(format!(
            "{split}: {} ordinary words kept as names, floor {}",
            f.kept_wrong.len(),
            floor.max_kept_wrong
        ));
    }
    let look = f.look_per_case();
    if look > floor.max_card_look + 1e-9 || look >= CARD_LOOK_TARGET {
        out.push(format!(
            "{split}: {look:.2} card rows worth a look per case, floor {}, target below {CARD_LOOK_TARGET}",
            floor.max_card_look
        ));
    }
    if false_rate > floor.max_false_per_1000 + 1e-9 {
        out.push(format!(
            "{split}: flow false hides {false_rate:.2} per 1,000 words, floor {}",
            floor.max_false_per_1000
        ));
    }
    out
}

// The card (U2), 2026-10-12: rows grouped by person as the app shows them. Dev 25.0 rows per
// case and test 33.4, nearly all hidden with confidence and folded under one line; the rows
// that ask for a look are 1.33 and 3.0 per case, under the target of 8.
//
// The whole flow. Baseline 2026-10-09, before D-050 stage B: with every case's names in the
// context, as the app runs, a name typed in one case that is also a word ("גיל", "אלה",
// "שירה", "אור") was hidden in every other case and kept there as a name, about four times
// the false hides the per-case measure above shows (dev 9.33 and 62 kept, test 10.67 and
// 84). Stage B reads such a name as the word on positive evidence only ("בגיל", "משימות
// אלה"); what is left ("קשר עין", "שני הבנים", "גיל: 37") needs the morphology of stage C.
const DEV_FLOW: FlowFloor = FlowFloor {
    max_blocked: 0,
    max_dead_ends: 0,
    max_kept_wrong: 43,
    max_false_per_1000: 6.25,
    max_card_look: 1.34,
};
const TEST_FLOW: FlowFloor = FlowFloor {
    max_blocked: 0,
    max_dead_ends: 0,
    max_kept_wrong: 67,
    max_false_per_1000: 8.90,
    max_card_look: 3.0,
};
const HARD_FLOW: FlowFloor = FlowFloor {
    max_blocked: 0,
    max_dead_ends: 0,
    max_kept_wrong: 1,
    max_false_per_1000: 34.1,
    max_card_look: 0.67,
};

#[test]
fn corpus_measures_the_filter_on_whole_documents() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let dev = load(&corpus, "dev");
    let test = load(&corpus, "test");
    let show_test = std::env::var_os("CORPUS_SHOW_TEST").is_some();
    let rd = measure(&dev, &|_| Vec::new());
    let rt = measure(&test, &|_| Vec::new());
    let hard = load(&corpus, "hard");
    let rh = measure(&hard, &|_| Vec::new());
    let (fd, ft, fh) = (measure_flow(&dev), measure_flow(&test), measure_flow(&hard));
    print("dev", &rd, true);
    print_flow("dev", &fd, true);
    print("test", &rt, show_test);
    print_flow("test", &ft, show_test);
    // The hard cases (patterns seen in use, D-050) are measured apart: no recall floor, so
    // they never lower the dev and test numbers, only the flow floors below.
    print("hard", &rh, true);
    print_flow("hard", &fh, true);
    let mut problems = check("dev", &rd, &DEV_FLOOR);
    problems.extend(check("test", &rt, &TEST_FLOOR));
    problems.extend(check_flow("dev", &fd, &DEV_FLOW));
    problems.extend(check_flow("test", &ft, &TEST_FLOW));
    problems.extend(check_flow("hard", &fh, &HARD_FLOW));
    assert!(problems.is_empty(), "{problems:#?}");
}
