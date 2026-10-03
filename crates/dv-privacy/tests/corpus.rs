//! The filter measured on whole documents (tests/corpus/README.md).
//!
//! Each corpus file is an invented document with every identifying span annotated, and a
//! few ordinary words that must pass marked `keep`. The test strips the annotations, runs
//! the filter as the case would, and prints per split (dev / test) and per group:
//! recall, ordinary words hidden by mistake (per 1,000 words) and questions per case.
//! It fails when a number gets worse than the floor recorded below, so every change to the
//! filter shows up as a number. `test/` is sealed: its misses are listed only with
//! `CORPUS_SHOW_TEST=1`, for the final review, never for tuning.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::panic,
    clippy::expect_used
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use dv_domain::{assign_tag, Identity, Role};
use dv_privacy::text::normalize;
use dv_privacy::{filter, Mark, PrivacyContext};

const JOIN_MARK: char = '¦';
const PRACTITIONER: &str = "ישראלה בדויה";

/// Groups whose spans must be hidden (or, for `indirect`, at least generalized or held).
const HIDE: &[&str] = &[
    "decl", "name", "surname", "place", "inst", "num", "date", "indirect",
];

#[derive(Debug, Clone)]
struct Span {
    start: usize,
    end: usize,
    surface: String,
    category: String,
}

#[derive(Debug)]
struct Doc {
    file: String,
    case: String,
    declared: Vec<(Role, String)>,
    text: String,
    spans: Vec<Span>,
}

fn parse(path: &Path) -> Doc {
    let file = path.file_name().unwrap().to_string_lossy().into_owned();
    let raw = fs::read_to_string(path).unwrap();
    let (head, body) = raw
        .split_once("\n---\n")
        .unwrap_or_else(|| panic!("{file}: missing --- separator"));
    let mut case = String::new();
    let mut declared = Vec::new();
    for line in head.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        match k.trim() {
            "case" => v.trim().clone_into(&mut case),
            "declared" => {
                for part in v.split(';').map(str::trim).filter(|p| !p.is_empty()) {
                    let (role, value) = part
                        .split_once('=')
                        .unwrap_or_else(|| panic!("{file}: bad declared entry {part}"));
                    let role: Role = serde_json::from_str(&format!("\"{}\"", role.trim()))
                        .unwrap_or_else(|_| panic!("{file}: unknown role {role}"));
                    declared.push((role, value.trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    assert!(!case.is_empty(), "{file}: missing case");

    // Strip `{{surface|category|role}}` and the join mark, keeping byte offsets of spans.
    let mut text = String::with_capacity(body.len());
    let mut spans = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        text.extend(rest[..open].chars().filter(|c| *c != JOIN_MARK));
        let after = &rest[open + 2..];
        let close = after
            .find("}}")
            .unwrap_or_else(|| panic!("{file}: unclosed annotation"));
        let inner = &after[..close];
        let mut parts = inner.split('|');
        let surface: String = parts
            .next()
            .unwrap_or_default()
            .chars()
            .filter(|c| *c != JOIN_MARK)
            .collect();
        let category = parts.next().unwrap_or_default().to_owned();
        assert!(
            HIDE.contains(&category.as_str()) || category == "keep",
            "{file}: unknown category in {{{{{inner}}}}}"
        );
        let start = text.len();
        text.push_str(&surface);
        spans.push(Span {
            start,
            end: text.len(),
            surface,
            category,
        });
        rest = &after[close + 2..];
    }
    text.extend(rest.chars().filter(|c| *c != JOIN_MARK));
    Doc {
        file,
        case,
        declared,
        text,
        spans,
    }
}

fn load(split: &str) -> Vec<Doc> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(split);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "txt"));
    files.sort();
    files.iter().map(|p| parse(p)).collect()
}

fn identities(doc: &Doc) -> Vec<Identity> {
    let mut used: Vec<String> = Vec::new();
    doc.declared
        .iter()
        .enumerate()
        .map(|(i, (role, value))| {
            let tag = assign_tag(*role, &used);
            used.push(tag.clone());
            Identity {
                id: format!("{}-{i}", doc.case),
                case_id: doc.case.clone(),
                role: *role,
                tag,
                value: value.clone(),
                aliases: Vec::new(),
            }
        })
        .collect()
}

#[derive(Debug, Default)]
struct Report {
    docs: usize,
    words: usize,
    /// group → (found, total)
    recall: BTreeMap<String, (u32, u32)>,
    keep_total: u32,
    keep_broken: u32,
    false_hides: u32,
    /// case → distinct suspect tokens (each one is a question today)
    questions: BTreeMap<String, BTreeSet<String>>,
    misses: Vec<String>,
    wrong: Vec<String>,
}

impl Report {
    fn rate(&self, group: &str) -> f64 {
        let (found, total) = self.recall.get(group).copied().unwrap_or((0, 0));
        if total == 0 {
            1.0
        } else {
            f64::from(found) / f64::from(total)
        }
    }

    fn false_per_1000(&self) -> f64 {
        f64::from(self.false_hides) * 1000.0 / self.words.max(1) as f64
    }

    fn questions_per_case(&self) -> f64 {
        let total: usize = self.questions.values().map(BTreeSet::len).sum();
        total as f64 / self.questions.len().max(1) as f64
    }
}

fn measure(docs: &[Doc]) -> Report {
    let mut report = Report::default();
    let practitioner = vec![PRACTITIONER.to_owned()];
    let allow = |_: &str| false;
    let confirmed = |_: &str| false;
    for doc in docs {
        let ids = identities(doc);
        let ctx = PrivacyContext {
            case_id: &doc.case,
            identities: &ids,
            practitioner: &practitioner,
            allowlisted: &allow,
            confirmed_names: &confirmed,
            today: (2026, 10, 3),
        };
        let out = filter(&doc.text, &ctx).unwrap();

        // Byte ranges of the original text that were replaced or held as a question.
        let mut marked: Vec<(usize, usize, String)> = Vec::new();
        let mut pos = 0;
        for seg in &out.original_segments {
            let end = pos + seg.text.len();
            if matches!(seg.mark, Some(Mark::Replaced | Mark::Suspect)) {
                marked.push((pos, end, seg.text.clone()));
            }
            pos = end;
        }
        assert_eq!(
            pos,
            doc.text.len(),
            "{}: segments do not cover the text",
            doc.file
        );

        let overlaps = |s: usize, e: usize| marked.iter().any(|(ms, me, _)| *ms < e && s < *me);
        for span in &doc.spans {
            if span.category == "keep" {
                report.keep_total += 1;
                if overlaps(span.start, span.end) {
                    report.keep_broken += 1;
                    report
                        .wrong
                        .push(format!("{}: keep «{}» was hidden", doc.file, span.surface));
                }
                continue;
            }
            let entry = report.recall.entry(span.category.clone()).or_default();
            entry.1 += 1;
            if overlaps(span.start, span.end) {
                entry.0 += 1;
            } else {
                report.misses.push(format!(
                    "{}: {} «{}» not hidden",
                    doc.file, span.category, span.surface
                ));
            }
        }
        for (ms, me, piece) in &marked {
            let gold = doc
                .spans
                .iter()
                .any(|s| s.category != "keep" && s.start < *me && *ms < s.end);
            if !gold {
                report.false_hides += 1;
                report
                    .wrong
                    .push(format!("{}: «{piece}» hidden or asked about", doc.file));
            }
        }
        let questions = report.questions.entry(doc.case.clone()).or_default();
        for s in &out.suspects {
            questions.insert(normalize(&s.token));
        }
        report.docs += 1;
        report.words += doc.text.split_whitespace().count();
    }
    report
}

fn print(split: &str, r: &Report, details: bool) {
    println!(
        "\n== corpus/{split}: {} documents, {} words, {} cases",
        r.docs,
        r.words,
        r.questions.len()
    );
    for group in HIDE {
        let (found, total) = r.recall.get(*group).copied().unwrap_or((0, 0));
        println!(
            "   {group:<9} {found:>4}/{total:<4} {:>6.1}%",
            100.0 * r.rate(group)
        );
    }
    println!(
        "   keep      {:>4}/{:<4} hidden by mistake",
        r.keep_broken, r.keep_total
    );
    println!(
        "   false hides {} ({:.1} per 1,000 words) · questions per case {:.1}",
        r.false_hides,
        r.false_per_1000(),
        r.questions_per_case()
    );
    if details {
        for m in &r.misses {
            println!("   MISS  {m}");
        }
        for w in &r.wrong {
            println!("   WRONG {w}");
        }
    }
}

/// Floors: the numbers may only get better. Raise them with every improvement.
struct Floor {
    min_recall: &'static [(&'static str, f64)],
    max_false_per_1000: f64,
    max_questions_per_case: f64,
}

// Measured on the filter as of v0.2.0, before the corpus existed (2026-10-03).
const DEV_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 0.94),
        ("name", 0.72),
        ("surname", 0.73),
        ("place", 0.61),
        ("inst", 0.30),
        ("num", 0.98),
        ("date", 0.96),
    ],
    max_false_per_1000: 11.2,
    max_questions_per_case: 20.7,
};

const TEST_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 0.98),
        ("name", 0.70),
        ("surname", 0.82),
        ("place", 0.64),
        ("inst", 0.22),
        ("num", 1.0),
        ("date", 0.98),
    ],
    max_false_per_1000: 11.4,
    max_questions_per_case: 27.3,
};

fn check(split: &str, r: &Report, floor: &Floor) -> Vec<String> {
    let mut problems = Vec::new();
    for (group, min) in floor.min_recall {
        if r.rate(group) + 1e-9 < *min {
            problems.push(format!(
                "{split}: recall for {group} is {:.4}, floor {min}",
                r.rate(group)
            ));
        }
    }
    if r.false_per_1000() > floor.max_false_per_1000 + 1e-9 {
        problems.push(format!(
            "{split}: {:.2} false hides per 1,000 words, ceiling {}",
            r.false_per_1000(),
            floor.max_false_per_1000
        ));
    }
    if r.questions_per_case() > floor.max_questions_per_case + 1e-9 {
        problems.push(format!(
            "{split}: {:.2} questions per case, ceiling {}",
            r.questions_per_case(),
            floor.max_questions_per_case
        ));
    }
    problems
}

#[test]
fn corpus_measures_the_filter_on_whole_documents() {
    let dev = load("dev");
    let test = load("test");
    let show_test = std::env::var_os("CORPUS_SHOW_TEST").is_some();
    let rd = measure(&dev);
    let rt = measure(&test);
    print("dev", &rd, true);
    print("test", &rt, show_test);
    let mut problems = check("dev", &rd, &DEV_FLOOR);
    problems.extend(check("test", &rt, &TEST_FLOOR));
    assert!(problems.is_empty(), "{problems:#?}");
}
