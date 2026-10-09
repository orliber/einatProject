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
    clippy::expect_used
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use dv_domain::{assign_tag, Identity, Role};
use dv_privacy::text::normalize;
use dv_privacy::{clear, filter, AutoKind, FilterOutcome, GateRequest, Mark, PrivacyContext};

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
                source: dv_domain::IdentitySource::Manual,
                reason: String::new(),
            }
        })
        .collect()
}

#[derive(Debug, Default)]
struct Report {
    docs: usize,
    cases: usize,
    words: usize,
    /// group → (found, total)
    recall: BTreeMap<String, (u32, u32)>,
    keep_total: u32,
    keep_broken: u32,
    false_hides: u32,
    /// case → distinct tokens the summary card lists (hidden without asking)
    card: BTreeMap<String, BTreeSet<String>>,
    /// case → the ones hidden on a weaker sign
    uncertain: BTreeMap<String, BTreeSet<String>>,
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

    fn per_case(map: &BTreeMap<String, BTreeSet<String>>, cases: usize) -> f64 {
        let total: usize = map.values().map(BTreeSet::len).sum();
        total as f64 / cases.max(1) as f64
    }
}

/// The names dv-core would keep from `out`: new tags only, a recurring form as an alias.
fn keep_found(out: &FilterOutcome, case: &str, kept: &mut Vec<Identity>, known: &[Identity]) {
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
fn marked_ranges(doc: &Doc, out: &FilterOutcome) -> Vec<(usize, usize, String)> {
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

fn measure(docs: &[Doc]) -> Report {
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
            };
            (filter(&doc.text, &ctx).unwrap(), ids)
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

fn print(split: &str, r: &Report, details: bool) {
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
struct Floor {
    min_recall: &'static [(&'static str, f64)],
    max_false_per_1000: f64,
}

// Raised with stage 2 of the filter (D-042, 2026-10-03). Measured on v0.2.0, before it:
// dev name .727 place .604 inst .308, 11.1 false hides per 1,000 words, 20.7 questions per
// case; test name .702 place .644 inst .227, 11.3 per 1,000, 27.2 per case.
// Stage 3 (2026-10-04): no questions; names found are kept with the case. Adjacent hides
// count as one (stage 2 on that count: dev 2.6, test 4.9 per 1,000). Test gains 5 names
// for 4 more false hides (5.3), from names carried to the case's later documents.
const DEV_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 1.0),
        ("name", 0.85),
        ("surname", 1.0),
        ("place", 0.88),
        ("inst", 0.82),
        ("num", 1.0),
        ("date", 1.0),
    ],
    max_false_per_1000: 2.5,
};

const TEST_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 0.99),
        ("name", 0.86),
        ("surname", 0.96),
        ("place", 0.73),
        ("inst", 0.68),
        ("num", 1.0),
        ("date", 1.0),
    ],
    max_false_per_1000: 5.3,
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
    problems
}

// ---------------------------------------------------------------- the whole flow (D-049, A)

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
    details: Vec<String>,
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
                flow.details
                    .push(format!("{}: «{piece}» hidden (not gold)", doc.file));
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
    let _ = split;
}

/// Floors of the whole flow: they may only go down. Stage B of D-049 brings dead ends to 0.
struct FlowFloor {
    max_blocked: usize,
    max_dead_ends: usize,
    max_kept_wrong: usize,
    max_false_per_1000: f64,
}

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
    if false_rate > floor.max_false_per_1000 + 1e-9 {
        out.push(format!(
            "{split}: flow false hides {false_rate:.2} per 1,000 words, floor {}",
            floor.max_false_per_1000
        ));
    }
    out
}

// Baseline of the whole flow, 2026-10-09, before D-049 stage B. With every case's names in
// the context, as the app runs, a name typed in one case that is also a word ("גיל", "אלה",
// "שירה", "אור") is hidden in every other case and kept there as a name: about four times
// the false hides the per-case measure above shows.
const DEV_FLOW: FlowFloor = FlowFloor {
    max_blocked: 0,
    max_dead_ends: 0,
    max_kept_wrong: 62,
    max_false_per_1000: 9.33,
};
const TEST_FLOW: FlowFloor = FlowFloor {
    max_blocked: 0,
    max_dead_ends: 0,
    max_kept_wrong: 84,
    max_false_per_1000: 10.67,
};
const HARD_FLOW: FlowFloor = FlowFloor {
    max_blocked: 0,
    max_dead_ends: 0,
    max_kept_wrong: 1,
    max_false_per_1000: 34.1,
};

#[test]
fn corpus_measures_the_filter_on_whole_documents() {
    let dev = load("dev");
    let test = load("test");
    let show_test = std::env::var_os("CORPUS_SHOW_TEST").is_some();
    let rd = measure(&dev);
    let rt = measure(&test);
    let hard = load("hard");
    let rh = measure(&hard);
    let (fd, ft, fh) = (measure_flow(&dev), measure_flow(&test), measure_flow(&hard));
    print("dev", &rd, true);
    print_flow("dev", &fd, true);
    print("test", &rt, show_test);
    print_flow("test", &ft, show_test);
    // The hard cases (patterns seen in use, D-049) are measured apart: no recall floor, so
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
