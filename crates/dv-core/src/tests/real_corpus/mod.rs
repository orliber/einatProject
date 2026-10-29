//! A realistic end-to-end exercise of the whole app on a fabricated corpus: three invented
//! cases, each with the documents a developmental psychologist really receives, in the formats
//! they really come in (Word 365, document2.xml, ODT, RTF, UTF-8 and Windows-1255 text, PDFs
//! with a Hebrew text layer, a ten-page scan and a phone photo). Every stage runs through the
//! core as the app runs it, and each is timed and checked:
//!
//! import (read + preview) → filter → scores → sorting → drafting (each section and the whole
//! report) → approvals → finishing checks → export (Word, PDF, parents) → backup and restore →
//! style import → opening a growing case → failures.
//!
//! Nothing leaves the machine: the transport is a fake that answers like the API from what it
//! received (`dv_ai::demo`), and records every request so the leak check reads them all.
//!
//! Slow (OCR of 14 pages, LibreOffice): ignored by default. Run it with
//!
//! ```text
//! cargo build --release -p dv-ingest --bins      # optional: read in the isolated worker
//! cargo test --release -p dv-core real_corpus -- --ignored --nocapture
//! ```
//!
//! PDFs, scans and photos need LibreOffice, poppler and ImageMagick; OCR needs Tesseract with
//! Hebrew. Without them those documents are skipped and the table says so.
//! `DV_BENCH_DEBUG=1` adds the paragraphs a PDF did not read back whole and every warning.
//! The table prints first; the run fails on anything wrong (a leak, lost or misordered text,
//! a wrong score band, an approval that changed the wrong paragraphs, an unreadable export,
//! a restore that differs, a failure not told in Hebrew).

// Every slice is at a position `find` returned, or at the end of what it found.
#![allow(clippy::string_slice)]

mod cases;
mod make;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::*;
use cases::{Block, Case, Doc, Format};

/// A step she waits for, on this machine, that is not the network: longer is flagged.
const SLOW: Duration = Duration::from_secs(1);

struct Row {
    stage: &'static str,
    item: String,
    took: Option<Duration>,
    info: String,
    verdict: String,
}

#[derive(Default)]
struct Bench {
    rows: Vec<Row>,
    /// Wrong results: the run fails on any.
    problems: Vec<String>,
    /// Worth knowing, not wrong: printed.
    notes: Vec<String>,
}

impl Bench {
    fn row(&mut self, stage: &'static str, item: &str, took: Option<Duration>, info: String) {
        let verdict = match took {
            Some(t) if t > SLOW => "SLOW".to_owned(),
            _ => "ok".to_owned(),
        };
        self.rows.push(Row {
            stage,
            item: item.to_owned(),
            took,
            info,
            verdict,
        });
    }

    fn check(&mut self, ok: bool, what: impl FnOnce() -> String) {
        if !ok {
            let w = what();
            if let Some(r) = self.rows.last_mut() {
                r.verdict = "WRONG".to_owned();
            }
            self.problems.push(w);
        }
    }

    fn print(&self) {
        let mut out = String::from("\n=== real corpus bench ===\n");
        for r in &self.rows {
            let took = r.took.map_or("-".to_owned(), |t| {
                format!("{:.0} ms", t.as_secs_f64() * 1000.0)
            });
            out.push_str(&format!(
                "{:<12} | {:<44} | {:>9} | {:<5} | {}\n",
                r.stage, r.item, took, r.verdict, r.info
            ));
        }
        out.push_str("\n--- notes ---\n");
        for n in &self.notes {
            out.push_str(&format!("- {n}\n"));
        }
        out.push_str("\n--- problems ---\n");
        for p in &self.problems {
            out.push_str(&format!("- {p}\n"));
        }
        let _ = std::io::Write::write_all(&mut std::io::stderr(), out.as_bytes());
    }
}

fn ms(t: Duration) -> String {
    format!("{:.0} ms", t.as_secs_f64() * 1000.0)
}

/// Spaces collapsed, direction marks dropped: the text as words in order.
fn norm(t: &str) -> String {
    t.chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{feff}'
            )
        })
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn words(t: &str) -> Vec<String> {
    t.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Of the expected words, the share found in the text at all, and the share found in order
/// (each looked for a little after the previous one).
fn recall(expected: &str, got: &str) -> (f64, f64) {
    let want = words(expected);
    let have = words(got);
    if want.is_empty() {
        return (1.0, 1.0);
    }
    let mut bag: HashMap<&str, i32> = HashMap::new();
    for w in &have {
        *bag.entry(w.as_str()).or_default() += 1;
    }
    let mut found = 0usize;
    for w in &want {
        if let Some(n) = bag.get_mut(w.as_str()) {
            if *n > 0 {
                *n -= 1;
                found += 1;
            }
        }
    }
    let mut at = 0usize;
    let mut ordered = 0usize;
    for w in &want {
        if let Some(i) = have.iter().skip(at).take(60).position(|h| h == w) {
            ordered += 1;
            at += i + 1;
        }
    }
    let n = want.len() as f64;
    (found as f64 / n, ordered as f64 / n)
}

/// Where `secret` stands in `text` as a word of its own (after up to three Hebrew prefix
/// letters: "ולשקד"), not inside another word ("שקדים", "תנועה" for "נועה"). Numbers, mails
/// and several words are looked for as they are.
fn find_word(text: &str, secret: &str) -> Option<usize> {
    if secret.contains([' ', '@', '/', '.']) || secret.chars().any(|c| c.is_ascii_digit()) {
        return text.find(secret);
    }
    let mut from = 0;
    while let Some(i) = text[from..].find(secret) {
        let at = from + i;
        let end = at + secret.len();
        let before: String = text[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric())
            .collect();
        let after_ok = text[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if after_ok && before.chars().count() <= 3 && before.chars().all(|c| "והבלמשכ".contains(c))
        {
            return Some(at);
        }
        from = end;
    }
    None
}

fn hebrew(t: &str) -> bool {
    t.chars().any(|c| ('\u{5d0}'..='\u{5ea}').contains(&c))
}

/// Every string in a request body, joined: what would leave, without JSON escaping.
fn strings_of(v: &Value, out: &mut String) {
    match v {
        Value::String(s) => {
            out.push_str(s);
            out.push('\n');
        }
        Value::Array(a) => a.iter().for_each(|x| strings_of(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings_of(x, out)),
        _ => {}
    }
}

fn sent_text(fake: &FakeTransport, from: usize) -> (String, usize, usize) {
    let sent = fake.sent.lock().unwrap();
    let mut text = String::new();
    let mut bytes = 0;
    for body in sent.iter().skip(from) {
        bytes += body.len();
        strings_of(&serde_json::from_str(body).unwrap(), &mut text);
    }
    (text, bytes, sent.len().saturating_sub(from))
}

fn outgoing(p: &Prepared) -> String {
    p.parts
        .iter()
        .flat_map(|part| part.outgoing.iter().map(|s| s.text.as_str()))
        .collect()
}

fn person(p: &cases::Person) -> IdentityInput {
    IdentityInput {
        id: None,
        role: p.role,
        value: p.value.into(),
        aliases: p.aliases.iter().map(|a| (*a).to_owned()).collect(),
    }
}

/// The OCR engine of this machine, when it reads Hebrew.
fn ocr_engine() -> Option<dv_ingest::ocr::Engine> {
    for data in [
        "/usr/share/tesseract-ocr/5/tessdata",
        "/usr/share/tesseract-ocr/4.00/tessdata",
    ] {
        let data = std::path::PathBuf::from(data);
        let exe = std::path::PathBuf::from("/usr/bin/tesseract");
        if exe.is_file() && data.join("heb.traineddata").is_file() {
            return Some(dv_ingest::ocr::Engine::new(exe, data));
        }
    }
    None
}

/// The isolated reader next to the test binary (`cargo build -p dv-ingest --bins`), as the
/// app reads her files; else the documents are read in this process.
fn ingest_worker() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("DV_INGEST_WORKER") {
        return Some(p.into());
    }
    let exe = std::env::current_exe().ok()?;
    let w = exe.parent()?.parent()?.join("dv-ingest-worker");
    w.is_file().then_some(w)
}

/// Import: read (isolated reader or in process, OCR for scans) and preview, timed; the text
/// checked against what the page says; then saved as material.
fn import_case(
    b: &mut Bench,
    core: &mut Core,
    case: &str,
    c: &Case,
    files: &[(Doc, Vec<u8>)],
) -> usize {
    let mut out = 0;
    for (doc, bytes) in files {
        let t = Instant::now();
        let read = core
            .begin_import_document(case, &doc.file, bytes.clone())
            .unwrap()
            .run();
        let read_took = t.elapsed();
        let t = Instant::now();
        let preview = core.finish_import_document(case, read);
        let preview_took = t.elapsed();
        let p = match preview {
            Ok(p) => p,
            Err(e) => {
                let ui = e.to_ui();
                b.row(
                    "import",
                    &format!("{} [{}]", doc.file, doc.format.label()),
                    Some(read_took),
                    format!("FAILED: {} / {:?}", ui.message, ui.details),
                );
                b.check(false, || {
                    format!("{}: import failed: {}", doc.file, ui.message)
                });
                continue;
            }
        };
        let truth = doc.lines().join("\n");
        let body = norm(&p.body);
        let lines = doc.lines();
        let found: Vec<Option<usize>> = lines.iter().map(|l| body.find(&norm(l))).collect();
        // In order: each line looked for after the one before it (a line can repeat).
        let notes: HashSet<String> = doc
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Note(n) => Some(norm(n)),
                _ => None,
            })
            .chain(doc.textbox.iter().map(|t| norm(t)))
            .collect();
        let mut at = 0usize;
        let mut seq: Vec<Option<usize>> = Vec::new();
        for l in &lines {
            let l = norm(l);
            // A footnote is read where the format puts it (Word: after the body).
            if notes.contains(&l) {
                seq.push(body.find(&l));
                continue;
            }
            match body[at..].find(&l) {
                Some(i) => {
                    seq.push(Some(at + i));
                    at += i + l.len();
                }
                None => seq.push(None),
            }
        }
        let whole = found.iter().filter(|f| f.is_some()).count();
        // Words in order, the footnotes and text boxes apart (each format puts them elsewhere).
        let flowing: String = lines
            .iter()
            .filter(|l| !notes.contains(&norm(l)))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let (bag, _) = recall(&truth, &p.body);
        let (_, ordered) = recall(&flowing, &p.body);
        let out_of_order: Vec<&String> = lines
            .iter()
            .zip(found.iter().zip(&seq))
            .filter(|(l, _)| !notes.contains(&norm(l)))
            .filter(|(_, (f, q))| f.is_some() && q.is_none())
            .map(|(l, _)| l)
            .collect();
        let in_order = out_of_order.is_empty();
        b.row(
            "import",
            &format!("{} [{}]", doc.file, doc.format.label()),
            Some(read_took + preview_took),
            format!(
                "read {} + preview {}; {} chars, {} pages; lines whole {whole}/{}; words {:.0}% / in order {:.0}%; {} hidden; warnings: {}",
                ms(read_took),
                ms(preview_took),
                p.body.chars().count(),
                p.pages,
                lines.len(),
                bag * 100.0,
                ordered * 100.0,
                p.auto_hidden.len() + p.hidden.len(),
                p.warnings.join(" | ")
            ),
        );
        if read_took > SLOW || preview_took > SLOW {
            b.notes.push(format!(
                "{}: read {} preview {} ({} chars)",
                doc.file,
                ms(read_took),
                ms(preview_took),
                p.body.chars().count()
            ));
        }
        // What the page says is all there, in order, for the formats that keep text whole.
        if !doc.format.ocr() && doc.format != Format::Pdf {
            b.check(whole == lines.len(), || {
                let missing: Vec<&String> = lines
                    .iter()
                    .zip(&found)
                    .filter(|(_, f)| f.is_none())
                    .map(|(l, _)| l)
                    .collect();
                format!("{}: lines missing or broken: {missing:?}", doc.file)
            });
            b.check(in_order, || {
                format!("{}: lines out of order: {out_of_order:?}", doc.file)
            });
        } else if doc.format == Format::Pdf {
            b.check(bag > 0.97 && ordered > 0.9, || {
                format!(
                    "{}: PDF text: {:.1}% of words, {:.1}% in order",
                    doc.file,
                    bag * 100.0,
                    ordered * 100.0
                )
            });
        } else {
            b.check(bag > 0.85, || {
                format!("{}: OCR read {:.1}% of the words", doc.file, bag * 100.0)
            });
            // Every name must come through whole or be learned as a misreading.
            let kept: Vec<String> = core
                .case_detail(case)
                .unwrap()
                .identities
                .iter()
                .flat_map(|i| std::iter::once(i.value.clone()).chain(i.aliases.clone()))
                .collect();
            for person in &c.people {
                for part in person.value.split(' ').filter(|w| w.chars().count() > 2) {
                    if truth.contains(part) && !p.body.contains(part) {
                        b.notes.push(format!(
                            "{}: OCR misread the name part {part:?} (learned spellings: {})",
                            doc.file,
                            kept.iter()
                                .filter(|k| k.chars().count() + 1 >= part.chars().count()
                                    && k.chars().count() <= part.chars().count() + 1)
                                .count()
                        ));
                    }
                }
            }
        }
        // What never prints is never imported.
        let never: Vec<&String> = [&doc.comment, &doc.deleted].into_iter().flatten().collect();
        for n in never {
            b.check(!p.body.contains(n.as_str()), || {
                format!("{}: imported what was not on the page: {n}", doc.file)
            });
        }
        if matches!(doc.format, Format::Docx | Format::Docx2) {
            b.check(!p.body.contains("טקסט נסתר"), || {
                format!("{}: hidden text imported", doc.file)
            });
        }
        if !matches!(doc.format, Format::Pdf) {
            for m in [&doc.header, &doc.footer].into_iter().flatten() {
                b.check(!p.body.contains(m.as_str()), || {
                    format!("{}: the margin is in the body: {m}", doc.file)
                });
            }
        }
        if let Some(t) = &doc.textbox {
            b.check(p.body.matches(t.as_str()).count() == 1, || {
                format!(
                    "{}: the text box is there {} times",
                    doc.file,
                    p.body.matches(t.as_str()).count()
                )
            });
        }
        let t = Instant::now();
        let input = core
            .add_input(case, p.suggested_kind, &p.title, &p.body)
            .unwrap();
        let took = t.elapsed();
        b.row(
            "save",
            &format!("{} as {:?}", doc.file, p.suggested_kind),
            Some(took),
            format!("title {:?}", p.title),
        );
        let _ = input;
        out += 1;
    }
    out
}

/// The score sheet she types: the text the sections read, with the ranges and percentiles
/// from the fixed tables, checked against the manuals' values computed by hand.
fn scores(b: &mut Bench, core: &mut Core, case: &str, c: &Case) {
    let t = Instant::now();
    let saved = core.save_scores(case, None, &c.sheet).unwrap();
    let took = t.elapsed();
    let text = saved.content.clone();
    let mut wrong = Vec::new();
    let instrument = dv_domain::instruments()
        .into_iter()
        .find(|i| i.key == c.sheet.instrument)
        .unwrap();
    for (key, value, band, pct) in &c.expected_bands {
        let m = instrument.measures.iter().find(|m| m.key == *key).unwrap();
        let got_band = dv_domain::scores::classify(m, *value).map(|b| b.label.clone());
        let got_pct = dv_domain::scores::percentile(m, *value);
        if got_band.as_deref() != Some(*band) || got_pct.as_deref() != Some(*pct) {
            wrong.push(format!(
                "{key} {value}: {got_band:?} {got_pct:?}, the manual says {band} {pct}"
            ));
        }
        let line = text
            .lines()
            .find(|l| l.contains(&m.name_he) && l.contains(&format!("{value}")))
            .unwrap_or("");
        if !line.contains(band) {
            wrong.push(format!(
                "{key}: the material's line lacks {band:?}: {line:?}"
            ));
        }
    }
    b.row(
        "scores",
        &format!("{} ({} entries)", c.sheet.instrument, c.sheet.entries.len()),
        Some(took),
        format!("{} chars of material", text.chars().count()),
    );
    b.check(wrong.is_empty(), || {
        format!("{}: scores: {wrong:?}", c.code)
    });
}

/// Sorting as the app runs it: one request per material, side by side (D-134).
fn sort(b: &mut Bench, core: &mut Core, fake: &FakeTransport, case: &str, c: &Case) {
    let t = Instant::now();
    let jobs = core.begin_sort_parts(case).unwrap();
    let begin = t.elapsed();
    let n = jobs.len();
    let t = Instant::now();
    let runs = run_requests(jobs).unwrap();
    let run = t.elapsed();
    let t = Instant::now();
    let (ready, again) = core.finish_requests(runs).unwrap();
    let finish = t.elapsed();
    let mut sizes = Vec::new();
    let from = fake.sent.lock().unwrap().len();
    let t = Instant::now();
    let mut blocked = 0;
    for (input, p) in &ready {
        sizes.push(outgoing(p).len());
        if !p.blocked.is_empty() {
            blocked += 1;
            b.notes.push(format!(
                "{}: sorting of material {input} blocked: {:?}",
                c.code, p.blocked
            ));
            continue;
        }
        core.send_sort(p.approval_id.as_deref().unwrap()).unwrap();
    }
    let send = t.elapsed();
    let (_, bytes, count) = sent_text(fake, from);
    b.row(
        "sort",
        &format!("{} ({n} materials)", c.code),
        Some(begin + run + finish),
        format!(
            "begin {} (locked) + filter {} (free) + land {} (locked); {} ready, {} again, {blocked} blocked; outgoing {}–{} chars per material; send+apply {} for {count} requests, {} KB",
            ms(begin),
            ms(run),
            ms(finish),
            ready.len(),
            again.len(),
            sizes.iter().min().unwrap_or(&0),
            sizes.iter().max().unwrap_or(&0),
            ms(send),
            bytes / 1024
        ),
    );
    b.check(again.is_empty() && blocked == 0, || {
        format!(
            "{}: sorting: {} again, {blocked} blocked",
            c.code,
            again.len()
        )
    });
    let detail = core.case_detail(case).unwrap();
    let unsorted = detail.routing.iter().filter(|r| r.needs_sorting).count();
    b.check(unsorted == 0, || {
        format!("{}: {unsorted} materials still need sorting", c.code)
    });
}

/// A realistic answer for one section: a paragraph that keeps the source's words and cites
/// it, one that rewrites with the tags, and one with a number found in no source (the check
/// must flag it, and "approve the clean ones" must leave it).
fn invented_number_answer(tagged_source: &str) -> Value {
    let first: String = tagged_source
        .split_inclusive('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    api_json(&json!({
        "reply": "ניסחתי שלוש פסקאות.",
        "paragraphs": [
            {"text": first, "source_refs": ["S1"], "guide": 0},
            {"text": "[ילד] שיתפה פעולה לאורך המפגשים, ועבדה בריכוז במטלות החזותיות.", "source_refs": ["S1"], "guide": 0},
            {"text": "בסולם ההסתגלות התקבל ציון 143, גבוה מאוד ביחס לבני גילה.", "source_refs": ["S1"], "guide": 0}
        ],
        "questions": [],
        "missing": [],
        "contradictions": []
    }))
}

fn statuses(core: &mut Core, case: &str) -> HashMap<String, (String, DraftStatus)> {
    core.case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .flat_map(|s| {
            let key = s.key.clone();
            s.paragraphs
                .into_iter()
                .map(move |p| (p.id, (key.clone(), p.status)))
        })
        .collect()
}

fn changed(
    before: &HashMap<String, (String, DraftStatus)>,
    after: &HashMap<String, (String, DraftStatus)>,
) -> Vec<String> {
    after
        .iter()
        .filter(|(id, (_, s))| before.get(*id).is_none_or(|(_, b)| b != s))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Drafting: each section alone, then the whole report the way the app runs it; the drafts
/// checked for their sources; then the three ways to approve.
fn draft(b: &mut Bench, core: &mut Core, fake: &FakeTransport, case: &str, c: &Case) {
    // One section alone, twice (the second turn is a reply in the same conversation).
    for (n, ask) in ["תנסחי טיוטה לסעיף", "תקצרי מעט ותשמרי על העובדות"]
        .iter()
        .enumerate()
    {
        let t = Instant::now();
        let p = core.prepare_section(case, "background", ask).unwrap();
        let took = t.elapsed();
        let from = fake.sent.lock().unwrap().len();
        let t = Instant::now();
        let r = core.send_section(p.approval_id.as_deref().unwrap());
        let send = t.elapsed();
        let (_, bytes, _) = sent_text(fake, from);
        b.row(
            "section",
            &format!("{} background #{}", c.code, n + 1),
            Some(took),
            format!(
                "prepare+gate {}; request {} KB; send+land {}; {} paragraphs",
                ms(took),
                bytes / 1024,
                ms(send),
                r.as_ref().map_or(0, |r| r.paragraphs.len())
            ),
        );
        b.check(r.is_ok(), || {
            format!("{}: background section: {:?}", c.code, r.err())
        });
    }
    // The whole report, as "לכתוב את הריקים" runs it (D-109).
    let t = Instant::now();
    let jobs = core.begin_full_draft(case).unwrap();
    let begin = t.elapsed();
    let sections = jobs.len();
    let t = Instant::now();
    let runs = run_requests(jobs).unwrap();
    let run = t.elapsed();
    let t = Instant::now();
    let (ready, again) = core.finish_requests(runs).unwrap();
    let finish = t.elapsed();
    let mut sizes: Vec<(String, usize)> = Vec::new();
    let mut warned_section = None;
    let from = fake.sent.lock().unwrap().len();
    let t = Instant::now();
    let mut paragraphs = 0;
    for (key, p) in &ready {
        sizes.push((key.clone(), outgoing(p).len()));
        b.check(p.blocked.is_empty(), || {
            format!("{}: section {key} blocked: {:?}", c.code, p.blocked)
        });
        if p.blocked.is_empty() {
            // One section gets a model answer with a number it invented.
            if warned_section.is_none() && key != "background" {
                let src = outgoing(p);
                let block = src
                    .split("id=\"S1\"")
                    .nth(1)
                    .and_then(|r| r.split_once(">\n"))
                    .map_or("", |(_, body)| body);
                *fake.answer.lock().unwrap() = Some(invented_number_answer(block));
                warned_section = Some(key.clone());
            }
            let r = core.send_section(p.approval_id.as_deref().unwrap());
            *fake.answer.lock().unwrap() = None;
            match r {
                Ok(r) => paragraphs += r.paragraphs.len(),
                Err(e) => b.check(false, || {
                    format!("{}: section {key}: {}", c.code, e.to_ui().message)
                }),
            }
        }
    }
    let send = t.elapsed();
    let (_, bytes, count) = sent_text(fake, from);
    let biggest = sizes
        .iter()
        .max_by_key(|(_, s)| *s)
        .cloned()
        .unwrap_or_default();
    b.row(
        "full draft",
        &format!("{} ({sections} sections)", c.code),
        Some(begin + run + finish),
        format!(
            "begin {} (locked) + filter {} (free) + land {} (locked); {} again; biggest request {} ({} chars); {count} sent, {} KB; send+land {}; {paragraphs} paragraphs",
            ms(begin),
            ms(run),
            ms(finish),
            again.len(),
            biggest.0,
            biggest.1,
            bytes / 1024,
            ms(send)
        ),
    );
    b.check(again.is_empty(), || {
        format!("{}: {} sections again", c.code, again.len())
    });

    // The drafts: each cites a source, and the demo's words are the sources' own.
    let detail = core.case_detail(case).unwrap();
    let materials: String = detail
        .inputs
        .iter()
        .map(|i| norm(&i.content))
        .collect::<Vec<_>>()
        .join("\n");
    let mut total = 0;
    let mut cited = 0;
    let mut kept = 0;
    let mut warned = 0;
    for s in &detail.sections {
        for p in s.paragraphs.iter().filter(|p| p.by_ai) {
            total += 1;
            if !p.sources.is_empty() {
                cited += 1;
            }
            if !p.warnings.is_empty() {
                warned += 1;
            }
            if p.text
                .split_inclusive('.')
                .map(norm)
                .filter(|x| x.chars().count() > 8)
                .all(|x| materials.contains(x.trim_end_matches('.')))
            {
                kept += 1;
            }
            b.check(!p.text.contains("[ילד"), || {
                format!("{}: a tag shown in a draft: {}", c.code, p.text)
            });
        }
    }
    if std::env::var_os("DV_BENCH_DEBUG").is_some() {
        for p in detail
            .sections
            .iter()
            .flat_map(|s| s.paragraphs.iter())
            .filter(|p| !p.warnings.is_empty())
        {
            b.notes.push(format!(
                "{}: warning {:?} on {:?}",
                c.code,
                p.warnings,
                p.text.chars().take(80).collect::<String>()
            ));
        }
    }
    b.row(
        "drafts",
        c.code,
        None,
        format!(
            "{total} AI paragraphs: {cited} cite a source, {kept} word for word from the materials, {warned} with a warning"
        ),
    );
    let invented = warned_section.as_ref().and_then(|k| {
        detail
            .sections
            .iter()
            .find(|s| &s.key == k)?
            .paragraphs
            .iter()
            .find(|p| p.text.contains("143"))
            .cloned()
    });
    b.check(
        invented.as_ref().is_some_and(|p| !p.warnings.is_empty()),
        || {
            format!(
                "{}: a number in no source was not flagged: {:?}",
                c.code,
                invented.as_ref().map(|p| (&p.text, &p.warnings))
            )
        },
    );

    // Approvals: one, the clean ones, a whole section: each changes exactly what it should.
    let before = statuses(core, case);
    let some = detail
        .sections
        .iter()
        .flat_map(|s| s.paragraphs.iter())
        .find(|p| p.status == DraftStatus::Proposed && p.warnings.is_empty())
        .map(|p| p.id.clone())
        .unwrap();
    let t = Instant::now();
    core.approve_paragraph(case, &some).unwrap();
    let one = t.elapsed();
    let after = statuses(core, case);
    let diff = changed(&before, &after);
    b.check(diff == vec![some.clone()], || {
        format!("{}: approving one changed {diff:?}", c.code)
    });
    let waiting: Vec<String> = after
        .iter()
        .filter(|(_, (_, s))| *s == DraftStatus::Proposed)
        .map(|(id, _)| id.clone())
        .collect();
    let detail = core.case_detail(case).unwrap();
    let flagged: HashSet<String> = detail
        .sections
        .iter()
        .flat_map(|s| s.paragraphs.iter())
        .filter(|p| {
            p.status == DraftStatus::Proposed && (!p.warnings.is_empty() || p.replaces.is_some())
        })
        .map(|p| p.id.clone())
        .collect();
    let t = Instant::now();
    let n = core.approve_clean_paragraphs(case, &waiting).unwrap();
    let clean = t.elapsed();
    let after2 = statuses(core, case);
    let diff = changed(&after, &after2);
    let expected: HashSet<String> = waiting
        .iter()
        .filter(|id| !flagged.contains(*id))
        .cloned()
        .collect();
    b.check(
        diff.iter().cloned().collect::<HashSet<_>>() == expected
            && usize::try_from(n).unwrap() == expected.len()
            && diff.iter().all(|id| after2[id].1 == DraftStatus::Approved),
        || {
            let left: Vec<(String, String, Vec<String>)> = expected
                .iter()
                .filter(|id| !diff.contains(*id))
                .filter_map(|id| {
                    detail.sections.iter().find_map(|s| {
                        s.paragraphs.iter().find(|p| &p.id == id).map(|p| {
                            (
                                s.key.clone(),
                                p.text.chars().take(60).collect(),
                                p.warnings.clone(),
                            )
                        })
                    })
                })
                .collect();
            format!(
                "{}: approve-clean changed {} of {} expected (flagged {}); left: {left:?}",
                c.code,
                diff.len(),
                expected.len(),
                flagged.len()
            )
        },
    );
    // The flagged ones, section by section.
    let sections: HashSet<String> = after2
        .iter()
        .filter(|(_, (_, s))| *s == DraftStatus::Proposed)
        .map(|(_, (k, _))| k.clone())
        .collect();
    let t = Instant::now();
    for k in &sections {
        let before = statuses(core, case);
        core.approve_section(case, k).unwrap();
        let after = statuses(core, case);
        let diff = changed(&before, &after);
        b.check(
            diff.iter()
                .all(|id| after[id].0 == *k && after[id].1 == DraftStatus::Approved)
                && after
                    .values()
                    .all(|(sk, s)| sk != k || *s != DraftStatus::Proposed),
            || format!("{}: approving section {k} changed {diff:?}", c.code),
        );
    }
    let section = t.elapsed();
    b.row(
        "approve",
        c.code,
        Some(one + clean + section),
        format!(
            "one {}; clean {} ({n} of {}, {} flagged left); {} sections {}",
            ms(one),
            ms(clean),
            waiting.len(),
            flagged.len(),
            sections.len(),
            ms(section)
        ),
    );

    // The summary, written from what she approved.
    let t = Instant::now();
    let p = core.prepare_section(case, "summary", "").unwrap();
    let took = t.elapsed();
    core.send_section(p.approval_id.as_deref().unwrap())
        .unwrap();
    core.approve_section(case, "summary").unwrap();
    b.row(
        "section",
        &format!("{} summary", c.code),
        Some(took),
        format!("prepare+gate {}", ms(took)),
    );
}

/// The finishing checks before the file: what blocks the export, the report check and the
/// check against the sources.
fn finish_checks(b: &mut Bench, core: &mut Core, case: &str, c: &Case) {
    let t = Instant::now();
    let check = core.check_export(case).unwrap();
    let took = t.elapsed();
    b.row(
        "check",
        &format!("{} export check", c.code),
        Some(took),
        format!(
            "{} blocking, {} to complete, {} sections, {} score tables, file {:?}",
            check.blocking.len(),
            check.to_complete.len(),
            check.included_sections,
            check.score_tables,
            check.file_name
        ),
    );
    b.check(check.blocking.is_empty(), || {
        format!("{}: export blocked: {:?}", c.code, check.blocking)
    });
    b.check(
        !check
            .file_name
            .contains(c.people[0].value.split(' ').next().unwrap()),
        || format!("{}: the child's name in the file name", c.code),
    );
    let t = Instant::now();
    let r = core.prepare_report_check(case);
    let took = t.elapsed();
    match r {
        Ok(p) => {
            let size = outgoing(&p).len();
            let sent = p
                .approval_id
                .as_deref()
                .map(|a| core.send_report_check(a).map(|r| r.notes.len()));
            b.row(
                "check",
                &format!("{} report check", c.code),
                Some(took),
                format!("prepare+gate {}; {size} chars; notes {sent:?}", ms(took)),
            );
        }
        Err(e) => b.row(
            "check",
            &format!("{} report check", c.code),
            Some(took),
            format!("not prepared: {}", e.to_ui().message),
        ),
    }
}

fn docx_body(bytes: &[u8]) -> String {
    dv_ingest::extract(bytes, "report.docx").map_or_else(|e| format!("UNREADABLE {e}"), |e| e.body)
}

/// Export: Word, PDF and the parents' versions; each file read back.
fn export(b: &mut Bench, core: &mut Core, case: &str, c: &Case) -> String {
    let detail = core.case_detail(case).unwrap();
    let approved: Vec<String> = detail
        .sections
        .iter()
        .filter(|s| !s.key.starts_with("letter_"))
        .flat_map(|s| s.paragraphs.iter())
        .filter(|p| p.status == DraftStatus::Approved)
        .map(|p| p.text.clone())
        .collect();
    let mut word_text = String::new();
    for (label, pdf, parents) in [
        ("Word", false, false),
        ("PDF", true, false),
        ("Word parents", false, true),
        ("PDF parents", true, true),
    ] {
        let opts = parents.then_some(ParentsOptions {
            show_numbers: false,
        });
        let t = Instant::now();
        let job = if pdf {
            core.begin_export_pdf_as(case, None, opts)
        } else {
            core.begin_export_report_as(case, None, opts)
        };
        let job = match job {
            Ok(j) => j,
            Err(e) => {
                b.row(
                    "export",
                    &format!("{} {label}", c.code),
                    None,
                    format!("FAILED {}", e.to_ui().message),
                );
                b.check(false, || {
                    format!("{}: export {label}: {}", c.code, e.to_ui().message)
                });
                continue;
            }
        };
        let begin = t.elapsed();
        let t = Instant::now();
        let ran = job.run();
        let render = t.elapsed();
        let t = Instant::now();
        let (bytes, _) = core.finish_export(ran).unwrap();
        let finish = t.elapsed();
        let text = if pdf {
            dv_ingest::extract(&bytes, "report.pdf")
                .map_or_else(|e| format!("UNREADABLE {e}"), |e| e.body)
        } else {
            docx_body(&bytes)
        };
        let all = approved.join("\n");
        let (bag, ordered) = recall(&all, &text);
        let whole = approved
            .iter()
            .filter(|p| norm(&text).contains(&norm(p)))
            .count();
        b.row(
            "export",
            &format!("{} {label}", c.code),
            Some(begin + render + finish),
            format!(
                "begin {} + render {} + finish {}; {} KB; read back: {whole}/{} paragraphs whole, words {:.1}% / in order {:.1}%",
                ms(begin),
                ms(render),
                ms(finish),
                bytes.len() / 1024,
                approved.len(),
                bag * 100.0,
                ordered * 100.0
            ),
        );
        if std::env::var_os("DV_BENCH_DEBUG").is_some() && !parents {
            for p in approved
                .iter()
                .filter(|p| !norm(&text).contains(&norm(p)))
                .take(2)
            {
                let first: String = words(p).into_iter().take(3).collect::<Vec<_>>().join(" ");
                let at = norm(&text).find(&words(p)[0]).unwrap_or(0);
                let around: String = norm(&text)[at..]
                    .chars()
                    .take(p.chars().count() + 40)
                    .collect();
                b.notes.push(format!(
                    "{} {label}: paragraph {first:?}…\n   approved: {p}\n   in file:  {around}",
                    c.code
                ));
            }
        }
        b.check(!text.starts_with("UNREADABLE"), || {
            format!("{}: {label} does not read: {text}", c.code)
        });
        b.check(
            !text.contains("[ילד") && !text.contains("[אם]"),
            || format!("{}: {label}: a tag in the file", c.code),
        );
        if !parents {
            b.check(bag > 0.98, || {
                format!(
                    "{}: {label}: only {:.1}% of the approved words read back",
                    c.code,
                    bag * 100.0
                )
            });
            if !pdf {
                b.check(whole == approved.len() && ordered > 0.98, || {
                    format!(
                        "{}: {label}: {whole}/{} approved paragraphs whole, {:.1}% in order",
                        c.code,
                        approved.len(),
                        ordered * 100.0
                    )
                });
                word_text = text;
            }
        }
    }
    word_text
}

/// The full backup, its check, and a restore on "another computer": the same cases, byte
/// for byte, and the same report.
fn backup_restore(
    b: &mut Bench,
    core: &mut Core,
    dir: &std::path::Path,
    ids: &[String],
    reports: &[String],
) {
    let backups = dir.join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    let t = Instant::now();
    let task = core.begin_backup(&backups.join("b.vaultbak")).unwrap();
    let begin = t.elapsed();
    let t = Instant::now();
    let run = task.run();
    let write = t.elapsed();
    let t = Instant::now();
    let done = core.finish_backup(run).unwrap();
    let finish = t.elapsed();
    b.row(
        "backup",
        "write + verify",
        Some(begin + write + finish),
        format!(
            "begin {} + write/read back {} + finish {}; {} KB",
            ms(begin),
            ms(write),
            ms(finish),
            done.bytes / 1024
        ),
    );
    let bytes = std::fs::read(&done.path).unwrap();
    core.stage_backup(bytes.clone(), "b.vaultbak".into())
        .unwrap();
    let t = Instant::now();
    let checked = core.check_staged_backup(PASSWORD);
    let took = t.elapsed();
    b.row(
        "backup",
        "drill (check)",
        Some(took),
        format!(
            "{:?}",
            checked
                .as_ref()
                .map(|_| "ok")
                .map_err(|e| e.to_ui().message)
        ),
    );
    b.check(checked.is_ok(), || "backup drill failed".into());

    let other = dir.join("other-computer");
    std::fs::create_dir_all(&other).unwrap();
    let mut restored = Core::for_tests(&other, None);
    restored.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    let t = Instant::now();
    let job = restored.begin_restore(Some(PASSWORD), None).unwrap();
    let ran = job.run();
    let status = restored.finish_restore(ran);
    let took = t.elapsed();
    b.row(
        "restore",
        "on another computer",
        Some(took),
        format!(
            "{:?}",
            status
                .as_ref()
                .map(|s| s.unlocked)
                .map_err(|e| e.to_ui().message)
        ),
    );
    b.check(status.is_ok(), || "restore failed".into());
    for (id, report) in ids.iter().zip(reports) {
        let a = serde_json::to_string(&core.case_detail(id).unwrap()).unwrap();
        let t = Instant::now();
        let r = serde_json::to_string(&restored.case_detail(id).unwrap()).unwrap();
        let took = t.elapsed();
        let same = a == r;
        let again = docx_body(&restored.export_report(id, None).unwrap());
        b.row(
            "restore",
            &format!("case {}", &id[..8.min(id.len())]),
            Some(took),
            format!(
                "detail identical: {same} ({} KB); report identical: {}",
                a.len() / 1024,
                norm(&again) == norm(report)
            ),
        );
        b.check(same, || format!("restored case {id} differs"));
        b.check(norm(&again) == norm(report), || {
            format!("restored report of {id} differs")
        });
    }
}

/// Her past report for the style profile: read, neutralized, kept, and the analysis request.
fn style(
    b: &mut Bench,
    core: &mut Core,
    fake: &FakeTransport,
    long: &(Doc, Vec<u8>),
    secrets: &[String],
) {
    let t = Instant::now();
    let read = core
        .begin_import_style_source(&long.0.file, long.1.clone())
        .unwrap()
        .run();
    let read_took = t.elapsed();
    let t = Instant::now();
    let preview = core.finish_import_style_source(read).unwrap();
    let preview_took = t.elapsed();
    let all: Vec<u32> = (0..u32::try_from(preview.parts.len()).unwrap()).collect();
    let t = Instant::now();
    let source = core.save_style_source(&preview.token, &all, None).unwrap();
    let save = t.elapsed();
    b.row(
        "style",
        &format!("import {}", long.0.file),
        Some(read_took + preview_took + save),
        format!(
            "read {} + neutralize {} + save {}; {} parts, {} hidden, {} numbers",
            ms(read_took),
            ms(preview_took),
            ms(save),
            preview.parts.len(),
            preview.hidden,
            preview.numbers
        ),
    );
    let t = Instant::now();
    let p = core.prepare_style_analysis(&source.id).unwrap();
    let took = t.elapsed();
    let out = outgoing(&p);
    let leaked: Vec<&String> = secrets
        .iter()
        .filter(|s| find_word(&out, s).is_some())
        .collect();
    let from = fake.sent.lock().unwrap().len();
    let r = p
        .approval_id
        .as_deref()
        .map(|a| core.send_style_analysis(a).map(|r| r.kept));
    let (_, bytes, _) = sent_text(fake, from);
    b.row(
        "style",
        "analysis prepare",
        Some(took),
        format!(
            "prepare+gate {}; {} chars; sent {} KB; result {:?}",
            ms(took),
            out.len(),
            bytes / 1024,
            r.map(|r| r.map_err(|e| e.to_ui().message))
        ),
    );
    b.check(leaked.is_empty(), || {
        format!("style analysis carries {leaked:?}")
    });
}

/// Opening a case as it grows: materials 1 → 30, then paragraphs 0 → 200.
fn growth(b: &mut Bench, core: &mut Core, sample: &str) {
    let meta = CaseMeta {
        code: "BENCH-GROW".into(),
        age: Some(Age {
            years: 6,
            months: 1,
        }),
        child_gender: Some(GrammaticalGender::Male),
        consent: Some(consent()),
        ..CaseMeta::default()
    };
    let case = core
        .create_case(
            meta,
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "יואב גדליה".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    let mut adds = Vec::new();
    for i in 1..=30usize {
        let t = Instant::now();
        core.add_input(
            &case,
            InputKind::Observation,
            &format!("תצפית {i}"),
            &format!("יואב בתצפית {i}. {sample}"),
        )
        .unwrap();
        adds.push(t.elapsed());
        if [1, 5, 10, 20, 30].contains(&i) {
            let t = Instant::now();
            let d = core.case_detail(&case).unwrap();
            let took = t.elapsed();
            let size = serde_json::to_vec(&d).unwrap().len();
            b.row(
                "open case",
                &format!("{i} materials"),
                Some(took),
                format!(
                    "{} KB to the screen; add_input #{i} {}",
                    size / 1024,
                    ms(*adds.last().unwrap())
                ),
            );
        }
    }
    let keys: Vec<String> = ReportStructure::load_default()
        .unwrap()
        .sections()
        .map(|s| s.key.clone())
        .collect();
    let mut added = 0usize;
    for target in [50usize, 100, 200] {
        let t0 = Instant::now();
        while added < target {
            core.add_own_paragraph(
                &case,
                &keys[added % keys.len()],
                &format!("פסקה {added}: יואב שיתף פעולה במטלות, ועבד בריכוז ובהתמדה לאורך המפגש."),
            )
            .unwrap();
            added += 1;
        }
        let add = t0.elapsed();
        let t = Instant::now();
        let d = core.case_detail(&case).unwrap();
        let took = t.elapsed();
        let size = serde_json::to_vec(&d).unwrap().len();
        b.row(
            "open case",
            &format!("30 materials, {target} paragraphs"),
            Some(took),
            format!(
                "{} KB; adding the last {} paragraphs took {}",
                size / 1024,
                target - (target / 2).min(target),
                ms(add)
            ),
        );
    }
}

/// Every way it fails tells her, in Hebrew, what to do next.
fn failures(b: &mut Bench, core: &mut Core, case: &str, scan: Option<&(Doc, Vec<u8>)>) {
    let docx = make::docx(&cases::case_a(&cases::secrets()).docs[1], "document.xml");
    let mut truncated = docx.clone();
    truncated.truncate(docx.len() / 2);
    let protected = dv_export::encrypt(&docx, "סיסמה-ארוכה-1").unwrap();
    let mut tries: Vec<(&str, Result<ImportPreview, CoreError>)> = vec![
        (
            "truncated .docx",
            core.import_document(case, "שאלון.docx", &truncated),
        ),
        (
            "password .docx",
            core.import_document(case, "מוגן.docx", &protected),
        ),
        ("empty .txt", core.import_document(case, "ריק.txt", b"")),
        (
            "HEIC photo",
            core.import_document(case, "IMG_1234.HEIC", b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"),
        ),
        (
            "not a PDF",
            core.import_document(case, "דוח.pdf", b"%PDF-1.7\n garbage"),
        ),
        (
            "zip, not Word",
            core.import_document(case, "קבצים.docx", &{
                let mut buf = std::io::Cursor::new(Vec::new());
                let mut z = zip::ZipWriter::new(&mut buf);
                z.start_file("a.txt", zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut z, b"x").unwrap();
                z.finish().unwrap();
                buf.into_inner()
            }),
        ),
        (
            "26 MB file",
            core.import_document(case, "גדול.pdf", &vec![b'a'; 26 * 1024 * 1024]),
        ),
    ];
    if let Some((d, bytes)) = scan {
        let ocr = core.ocr.take();
        tries.push((
            "scan, no OCR engine",
            core.import_document(case, &d.file, bytes),
        ));
        core.ocr = ocr;
    }
    let mut other: Vec<(&str, Result<(), CoreError>)> = Vec::new();
    // No consent.
    let nc = core
        .create_case(
            CaseMeta {
                code: "BENCH-NC".into(),
                age: Some(Age {
                    years: 4,
                    months: 0,
                }),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "מאיה".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.add_input(
        &nc,
        InputKind::Intake,
        "שיחה",
        "ההורים מספרים שמאיה ישנה היטב.",
    )
    .unwrap();
    other.push((
        "section without consent",
        core.prepare_section(&nc, "background", "").map(|_| ()),
    ));
    other.push((
        "export with nothing approved",
        core.export_report(&nc, None).map(|_| ()),
    ));
    other.push((
        "restore over a vault",
        core.begin_restore(Some(PASSWORD), None).map(|_| ()),
    ));
    other.push((
        "backup to a missing folder",
        core.write_backup(std::path::Path::new("/nonexistent/x/b.vaultbak"))
            .map(|_| ()),
    ));
    for (what, r) in tries {
        report_failure(b, what, r.map(|_| ()));
    }
    for (what, r) in other {
        report_failure(b, what, r);
    }
    // The service fails: each told in Hebrew, with the step that fits.
    for e in [
        EgressError::Offline,
        EgressError::RateLimited,
        EgressError::Overloaded,
        EgressError::Unavailable(503),
        EgressError::Unauthorized,
        EgressError::TooLarge,
    ] {
        report_failure(b, "service", Err(CoreError::Egress(e)));
    }
    core.lock();
    report_failure(b, "locked", core.case_detail(case).map(|_| ()));
    core.unlock(PASSWORD).unwrap();
}

fn report_failure(b: &mut Bench, what: &str, r: Result<(), CoreError>) {
    match r {
        Ok(()) => {
            b.row("failure", what, None, "no error".into());
            b.check(false, || format!("{what}: did not fail"));
        }
        Err(e) => {
            let ui = e.to_ui();
            b.row(
                "failure",
                what,
                None,
                format!("{:?} → {:?}: {}", ui.code, ui.action, ui.message),
            );
            b.check(hebrew(&ui.message) && !ui.message.is_empty(), || {
                format!("{what}: not told in Hebrew: {}", ui.message)
            });
            if ui.action == ErrorAction::Report {
                b.notes.push(format!(
                    "{what}: the button offered is 'report a problem' ({:?}): {}",
                    ui.code, ui.message
                ));
            }
        }
    }
}

/// The leak check over every request that left: no secret of any case, in any form.
fn leaks(b: &mut Bench, fake: &FakeTransport, all: &[(&Case, String)]) {
    let (text, bytes, count) = sent_text(fake, 0);
    let mut leaked = Vec::new();
    for (c, _) in all {
        for s in &c.secrets {
            if let Some(at) = find_word(&text, s) {
                let from = text[..at]
                    .char_indices()
                    .rev()
                    .nth(40)
                    .map_or(0, |(i, _)| i);
                let ctx: String = text[from..].chars().take(100).collect();
                leaked.push(format!("{}: {s:?} in …{ctx}…", c.code));
            }
        }
    }
    // Dates: no full date and no year goes out (the strictest rule: relative time only).
    let mut dates = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    for w in text.split(|c: char| c.is_whitespace() || c == '"') {
        let digits: String = w.chars().filter(char::is_ascii_digit).collect();
        let seps = w.chars().filter(|c| matches!(c, '.' | '/' | '-')).count();
        if seps >= 2
            && digits.len() >= 4
            && w.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | '/' | '-' | ',' | ')' | '(' | ':'))
        {
            dates.push(w.to_owned());
        }
        if digits.len() == 4
            && (digits.starts_with("20") || digits.starts_with("19"))
            && w.trim_matches(|c: char| !c.is_ascii_digit()).len() == 4
        {
            dates.push(w.to_owned());
        }
    }
    let _ = chars;
    dates.sort();
    dates.dedup();
    b.row(
        "leaks",
        &format!("{count} requests, {} KB", bytes / 1024),
        None,
        format!("{} secrets found; date-like words: {dates:?}", leaked.len()),
    );
    b.check(leaked.is_empty(), || format!("LEAK: {leaked:#?}"));
    for (c, keep_text) in all {
        let lost: Vec<&&str> = c.keep.iter().filter(|k| !keep_text.contains(**k)).collect();
        if !lost.is_empty() {
            b.notes.push(format!(
                "{}: ordinary words that did not reach the AI as written: {lost:?}",
                c.code
            ));
        }
    }
}

#[test]
#[ignore = "slow: OCR and LibreOffice; see the module comment for the command"]
fn real_corpus_end_to_end() {
    let mut b = Bench::default();
    let x = cases::secrets();
    let all_cases = [cases::case_a(&x), cases::case_b(&x), cases::case_c(&x)];
    let work = tempfile::tempdir().unwrap();
    let all_docs: Vec<Doc> = all_cases.iter().flat_map(|c| c.docs.clone()).collect();
    let made = make::make_all(work.path(), &all_docs);
    b.row(
        "corpus",
        &format!("{} files made", made.files.len()),
        Some(made.took),
        format!(
            "{} KB in all; skipped: {:?}",
            made.files.iter().map(|(_, f)| f.len()).sum::<usize>() / 1024,
            made.skipped
        ),
    );
    for (n, why) in &made.skipped {
        b.notes.push(format!("not made here: {n} ({why})"));
    }

    let fake = FakeTransport::default();
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(
        &dir.path().join("vault"),
        Some(Box::new(fake.clone()) as Box<dyn Transport>),
    );
    core.create_vault(PASSWORD).unwrap();
    core.set_practitioner(vec!["ד\"ר רותם אלמוג".into()])
        .unwrap();
    core.ocr = ocr_engine();
    core.ingest_exe = ingest_worker();
    b.notes.push(format!(
        "reader: {}; OCR: {}",
        core.ingest_exe.as_ref().map_or(
            "in this process (no worker built)".to_owned(),
            |p| format!("isolated worker {}", p.display())
        ),
        if core.ocr.is_some() {
            "Tesseract (heb)"
        } else {
            "none"
        }
    ));

    let mut ids = Vec::new();
    let mut reports = Vec::new();
    let mut keep_texts = Vec::new();
    for c in &all_cases {
        let meta = CaseMeta {
            code: c.code.into(),
            age: Some(c.age),
            child_gender: Some(c.gender),
            consent: Some(consent()),
            ..CaseMeta::default()
        };
        let case = core
            .create_case(meta, c.people.iter().map(person).collect())
            .unwrap();
        let files: Vec<(Doc, Vec<u8>)> = made
            .files
            .iter()
            .filter(|(d, _)| c.docs.iter().any(|x| x.file == d.file))
            .cloned()
            .collect();
        let from = fake.sent.lock().unwrap().len();
        let imported = import_case(&mut b, &mut core, &case, c, &files);
        b.check(imported == files.len(), || {
            format!("{}: {imported} of {} files imported", c.code, files.len())
        });
        scores(&mut b, &mut core, &case, c);
        sort(&mut b, &mut core, &fake, &case, c);
        draft(&mut b, &mut core, &fake, &case, c);
        finish_checks(&mut b, &mut core, &case, c);
        reports.push(export(&mut b, &mut core, &case, c));
        keep_texts.push(sent_text(&fake, from).0);
        ids.push(case);
    }
    if let Some(long) = made
        .files
        .iter()
        .find(|(d, _)| d.format == Format::Docx && d.file.starts_with("אבחון פסיכודידקטי"))
    {
        let secrets: Vec<String> = all_cases.iter().flat_map(|c| c.secrets.clone()).collect();
        style(&mut b, &mut core, &fake, long, &secrets);
    }
    let pairs: Vec<(&Case, String)> = all_cases.iter().zip(keep_texts).collect();
    leaks(&mut b, &fake, &pairs);
    backup_restore(&mut b, &mut core, dir.path(), &ids, &reports);
    let sample = "בתצפית בגן נראה כי הוא משחק בעיקר לבד בפינת הבנייה. הגננת מתארת קושי במעברים בין פעילויות ובפרידה בבוקר. בשיחה עם ההורים עלה כי השינה השתפרה בחודשים האחרונים.".repeat(4);
    growth(&mut b, &mut core, &sample);
    let scan = made
        .files
        .iter()
        .find(|(d, _)| d.format == Format::ScanPdf)
        .cloned();
    failures(&mut b, &mut core, &ids[0], scan.as_ref());
    b.print();
    assert!(
        b.problems.is_empty(),
        "{} problems, see the table",
        b.problems.len()
    );
}
