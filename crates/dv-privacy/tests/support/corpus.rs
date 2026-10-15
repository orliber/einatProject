//! The corpus harness (tests/corpus/README.md), shared by `tests/corpus.rs` and by
//! `dv-ner`'s measurement with the local name model (stage 5).
//!
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

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use dv_domain::{assign_tag, Identity, Role};
use dv_privacy::text::normalize;
use dv_privacy::{filter_with_hints, AutoKind, FilterOutcome, Mark, NameHint, PrivacyContext};

const JOIN_MARK: char = '¦';
pub const PRACTITIONER: &str = "ישראלה בדויה";

/// Groups whose spans must be hidden (or, for `indirect`, at least generalized or held).
const HIDE: &[&str] = &[
    "decl", "name", "surname", "place", "inst", "num", "date", "indirect",
];

#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub surface: String,
    pub category: String,
}

#[derive(Debug)]
pub struct Doc {
    pub file: String,
    pub case: String,
    pub declared: Vec<(Role, String)>,
    pub text: String,
    pub spans: Vec<Span>,
}

fn parse(path: &Path) -> Doc {
    let file = path.file_name().unwrap().to_string_lossy().into_owned();
    // A Windows checkout may still turn LF into CRLF (an old clone, autocrlf).
    let raw = fs::read_to_string(path).unwrap().replace("\r\n", "\n");
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

/// The documents of `split` ("dev" or "test") under `corpus` (dv-privacy's `tests/corpus`).
pub fn load(corpus: &Path, split: &str) -> Vec<Doc> {
    let dir = corpus.join(split);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "txt"));
    files.sort();
    files.iter().map(|p| parse(p)).collect()
}

pub fn identities(doc: &Doc) -> Vec<Identity> {
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
                source: dv_domain::IdentitySource::Manual,
                reason: String::new(),
            }
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct Report {
    pub docs: usize,
    pub cases: usize,
    pub words: usize,
    /// group → (found, total)
    pub recall: BTreeMap<String, (u32, u32)>,
    pub keep_total: u32,
    pub keep_broken: u32,
    pub false_hides: u32,
    /// case → distinct tokens the summary card lists (hidden without asking)
    pub card: BTreeMap<String, BTreeSet<String>>,
    /// case → the ones hidden on a weaker sign
    pub uncertain: BTreeMap<String, BTreeSet<String>>,
    pub misses: Vec<String>,
    pub wrong: Vec<String>,
}

impl Report {
    pub fn rate(&self, group: &str) -> f64 {
        let (found, total) = self.recall.get(group).copied().unwrap_or((0, 0));
        if total == 0 {
            1.0
        } else {
            f64::from(found) / f64::from(total)
        }
    }

    pub fn false_per_1000(&self) -> f64 {
        f64::from(self.false_hides) * 1000.0 / self.words.max(1) as f64
    }

    fn per_case(map: &BTreeMap<String, BTreeSet<String>>, cases: usize) -> f64 {
        let total: usize = map.values().map(BTreeSet::len).sum();
        total as f64 / cases.max(1) as f64
    }
}

/// The names dv-core would keep from `out`: new tags only, a recurring form as an alias.
pub fn keep_found(out: &FilterOutcome, case: &str, kept: &mut Vec<Identity>, known: &[Identity]) {
    for a in &out.auto_hidden {
        if !matches!(a.kind, AutoKind::Name | AutoKind::OtherCase) {
            continue;
        }
        if known.iter().any(|i| i.tag == a.tag) {
            continue;
        }
        if let Some(i) = kept.iter_mut().find(|i| i.tag == a.tag) {
            if normalize(&i.value) != normalize(&a.token) && !i.aliases.contains(&a.token) {
                i.aliases.push(a.token.clone());
            }
            continue;
        }
        kept.push(Identity {
            id: format!("{case}-auto-{}", kept.len()),
            case_id: case.to_owned(),
            role: a.role,
            tag: a.tag.clone(),
            value: a.token.clone(),
            aliases: Vec::new(),
            source: dv_domain::IdentitySource::Auto,
            reason: a.reason.clone(),
        });
    }
}

/// Byte ranges of the original text that were hidden; adjacent hides count as one ("רוני
/// דמיוני" → "[אדם_1] [משפחה_1]" is one hide, as it was one question before).
pub fn marked_ranges(doc: &Doc, out: &FilterOutcome) -> Vec<(usize, usize, String)> {
    let mut marked: Vec<(usize, usize, String)> = Vec::new();
    let mut pos = 0;
    for seg in &out.original_segments {
        let end = pos + seg.text.len();
        if matches!(seg.mark, Some(Mark::Replaced | Mark::Suspect)) {
            match marked.last_mut() {
                Some(last) if doc.text[last.1..pos].trim().is_empty() => {
                    last.1 = end;
                    last.2 = doc.text[last.0..end].to_owned();
                }
                _ => marked.push((pos, end, seg.text.clone())),
            }
        }
        pos = end;
    }
    assert_eq!(
        pos,
        doc.text.len(),
        "{}: segments do not cover the text",
        doc.file
    );
    marked
}

/// Filter every document as its case would; `hints` stands in for a local name model.
pub fn measure(docs: &[Doc], hints: &dyn Fn(&str) -> Vec<NameHint>) -> Report {
    let mut report = Report::default();
    let practitioner = vec![PRACTITIONER.to_owned()];
    let allow = |_: &str| false;
    let confirmed = |_: &str| false;
    // case → names found in its earlier documents
    let mut found: BTreeMap<String, Vec<Identity>> = BTreeMap::new();
    let mut cases: BTreeSet<String> = BTreeSet::new();
    for doc in docs {
        cases.insert(doc.case.clone());
        let declared = identities(doc);
        let kept = found.entry(doc.case.clone()).or_default();
        let pass = |kept: &[Identity]| {
            // A kept name whose tag a document's own declarations took gets the next free one.
            let mut ids = declared.clone();
            for k in kept {
                let mut k = k.clone();
                if ids.iter().any(|i| i.tag == k.tag) {
                    let used: Vec<String> = ids.iter().map(|i| i.tag.clone()).collect();
                    k.tag = assign_tag(k.role, &used);
                }
                ids.push(k);
            }
            let ctx = PrivacyContext {
                case_id: &doc.case,
                identities: &ids,
                practitioner: &practitioner,
                allowlisted: &allow,
                confirmed_names: &confirmed,
                past_names: &|_: &str| false,
                today: (2026, 10, 3),
                masks: None,
            };
            (
                filter_with_hints(&doc.text, &ctx, &hints(&doc.text)).unwrap(),
                ids,
            )
        };
        let (first, ids) = pass(kept);
        keep_found(&first, &doc.case, kept, &ids);
        let (out, ids) = pass(kept);
        keep_found(&out, &doc.case, kept, &ids);

        let marked = marked_ranges(doc, &out);

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
        for a in &out.auto_hidden {
            report
                .card
                .entry(doc.case.clone())
                .or_default()
                .insert(normalize(&a.token));
            if a.uncertain {
                report
                    .uncertain
                    .entry(doc.case.clone())
                    .or_default()
                    .insert(normalize(&a.token));
            }
        }
        report.docs += 1;
        report.words += doc.text.split_whitespace().count();
    }
    report.cases = cases.len();
    report
}

pub fn print(split: &str, r: &Report, details: bool) {
    println!(
        "\n== corpus/{split}: {} documents, {} words, {} cases",
        r.docs, r.words, r.cases
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
        "   false hides {} ({:.1} per 1,000 words) · questions per case 0 · card items per case {:.1} ({:.1} on a weaker sign)",
        r.false_hides,
        r.false_per_1000(),
        Report::per_case(&r.card, r.cases),
        Report::per_case(&r.uncertain, r.cases)
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
pub struct Floor {
    pub min_recall: &'static [(&'static str, f64)],
    pub max_false_per_1000: f64,
}

pub fn check(split: &str, r: &Report, floor: &Floor) -> Vec<String> {
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
    problems
}
