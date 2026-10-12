//! Shared helpers for the export tests: a fabricated report, unzipping, and a strict XML
//! reading of every part. Fabricated data only.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Cursor, Read};

use dv_export::{ChartBar, InfoLine, Report, ReportPart, ReportSection, ScoreChart, ScoreTable};
use quick_xml::events::Event;
use quick_xml::Reader;

/// A report whose every text field is `f(label)`, so the same structure can be filled with
/// plain text or with hostile text and compared.
pub fn report_with(f: impl Fn(&str) -> String) -> Report {
    Report {
        title: f("title"),
        info: vec![InfoLine {
            label: f("info label"),
            value: f("info value"),
        }],
        parts: vec![ReportPart {
            title: f("part"),
            sections: vec![ReportSection {
                title: f("section"),
                paragraphs: vec![f("paragraph 1"), f("paragraph 2")],
            }],
        }],
        tables: vec![ScoreTable {
            title: f("table"),
            columns: vec![f("column 1"), f("column 2")],
            rows: vec![vec![f("cell 1"), f("cell 2")]],
            note: f("note"),
            charts: vec![ScoreChart {
                title: f("chart"),
                min: 40.0,
                max: 160.0,
                step: 5.0,
                mean: 100.0,
                sd: 15.0,
                bars: vec![ChartBar {
                    label: f("bar"),
                    value: 112.0,
                }],
                note: f("chart note"),
            }],
        }],
        signature: vec![f("signature")],
        confidentiality: f("confidentiality"),
        font: f("font"),
    }
}

/// A plain fabricated report in Hebrew.
pub fn sample() -> Report {
    let mut r = report_with(|_| "טקסט בדוי".to_owned());
    r.title = "דוח אבחון פסיכולוגי".into();
    r.info[0] = InfoLine {
        label: "שם הילד".into(),
        value: "אלון בדוי".into(),
    };
    r.parts[0].sections[0].paragraphs = vec![
        "ההורים פנו בשל קושי במעברים ובוויסות רגשי.".into(),
        "WISC-V (גרסה עברית): ציון מלא 102 (טווח ממוצע).".into(),
    ];
    r.font = "David".into();
    r
}

pub fn part_names(zip_bytes: &[u8]) -> Vec<String> {
    let zip = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
    zip.file_names().map(str::to_owned).collect()
}

pub fn part(zip_bytes: &[u8], name: &str) -> Option<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
    let mut s = String::new();
    zip.by_name(name).ok()?.read_to_string(&mut s).unwrap();
    Some(s)
}

/// XML 1.0 `Char`: what may appear in a document at all, even escaped.
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// One element event: start, empty or end, with its name and attribute names.
pub type Shape = Vec<(char, String, Vec<String>)>;

/// Read `xml` strictly (balanced tags, valid attributes, only XML characters) and return its
/// element structure. Panics, naming `what`, on anything a strict reader would refuse.
pub fn shape(what: &str, xml: &str) -> Shape {
    if let Some(bad) = xml.chars().find(|c| !is_xml_char(*c)) {
        panic!(
            "{what}: character U+{:04X} is not allowed in XML",
            u32::from(bad)
        );
    }
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut depth = 0i64;
    loop {
        let ev = reader
            .read_event()
            .unwrap_or_else(|e| panic!("{what}: not well-formed: {e}"));
        let mut push = |kind: char, e: &quick_xml::events::BytesStart<'_>| {
            let attrs = e
                .attributes()
                .map(|a| {
                    let a = a.unwrap_or_else(|e| panic!("{what}: bad attribute: {e}"));
                    a.key.as_ref().to_owned()
                })
                .collect();
            out.push((kind, e.name().as_ref().to_owned(), attrs));
        };
        match ev {
            Event::Start(e) => {
                depth += 1;
                push('<', &e);
            }
            Event::Empty(e) => push('/', &e),
            Event::End(e) => {
                depth -= 1;
                out.push(('>', e.name().as_ref().to_owned(), Vec::new()));
            }
            Event::DocType(_) => panic!("{what}: a DOCTYPE appeared"),
            Event::CData(_) => panic!("{what}: a CDATA section appeared"),
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(depth, 0, "{what}: unbalanced");
    out
}

/// Every XML part of a package, read strictly.
pub fn check_package(zip_bytes: &[u8]) {
    for name in part_names(zip_bytes) {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            shape(&name, &part(zip_bytes, &name).unwrap());
        }
    }
}

/// The text of each `w:p` (its `w:t` runs joined), in document order.
pub fn paragraph_texts(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    let mut out: Vec<String> = Vec::new();
    let mut in_t = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) if e.name().as_ref() == "w:p" => out.push(String::new()),
            Event::Start(e) if e.name().as_ref() == "w:t" => in_t = true,
            Event::End(e) if e.name().as_ref() == "w:t" => in_t = false,
            Event::Text(t) if in_t => {
                if let Some(p) = out.last_mut() {
                    p.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) if in_t => {
                let ch = match r.resolve_char_ref().unwrap() {
                    Some(c) => c,
                    None => match r.xml10_content().as_ref() {
                        "amp" => '&',
                        "lt" => '<',
                        "gt" => '>',
                        "quot" => '"',
                        "apos" => '\'',
                        other => panic!("unknown entity &{other};"),
                    },
                };
                if let Some(p) = out.last_mut() {
                    p.push(ch);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// A font with Hebrew letters from this computer, or `None` (PDF tests are then skipped, as
/// in the crate's own tests).
pub fn system_font() -> Option<Vec<u8>> {
    [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
    ]
    .iter()
    .find_map(|p| std::fs::read(p).ok())
}
