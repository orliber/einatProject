//! The Word report: well-formed, right to left, and report text (written by the model or
//! taken from documents) is only ever text, never markup. Fabricated data only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::io::{Cursor, Write};

use common::{check_package, paragraph_texts, part, part_names, report_with, sample, shape};
use dv_export::{render, render_with_template, ChartBar, ExportError, ScoreChart};
use proptest::prelude::*;

/// Text that would become markup if it were not escaped: closing the run and paragraph,
/// opening new elements, ending a CDATA section, entities, a DOCTYPE, attribute breakouts.
const HOSTILE: &[&str] = &[
    "</w:t></w:r></w:p><w:p><w:r><w:t>הוזרק</w:t></w:r></w:p><w:p><w:r><w:t>",
    "]]><![CDATA[<w:b/>]]>",
    "&amp; &lt;w:p&gt; &#x3C; & < > \" '",
    "<!DOCTYPE d [<!ENTITY e \"x\">]>&e;",
    "\"/><w:b/><w:t x=\"",
    "<?xml version=\"1.0\"?><w:document>",
];

/// The body paragraphs of the report in `doc`, as they were given (without the bold labels).
fn body_texts(doc: &str) -> Vec<String> {
    paragraph_texts(doc)
}

#[test]
fn every_part_of_the_package_is_well_formed() {
    let bytes = render(&sample()).unwrap();
    let names = part_names(&bytes);
    for expected in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/_rels/document.xml.rels",
        "word/document.xml",
        "word/styles.xml",
        "word/settings.xml",
        "word/header1.xml",
        "word/footer1.xml",
        "docProps/core.xml",
    ] {
        assert!(names.iter().any(|n| n == expected), "{expected} missing");
    }
    check_package(&bytes);
}

/// Hostile text in every field of the report leaves the package's structure exactly as it
/// is with harmless text (no element or attribute added, none closed early), and every
/// string comes back, character for character, as the text of its paragraph.
#[test]
fn markup_in_any_field_stays_text_and_never_changes_the_structure() {
    let plain = render(&report_with(|_| "x".to_owned())).unwrap();
    for payload in HOSTILE {
        let hostile = render(&report_with(|label| format!("{label}: {payload}"))).unwrap();
        for name in part_names(&plain) {
            if !(name.ends_with(".xml") || name.ends_with(".rels")) {
                continue;
            }
            let a = shape(&name, &part(&plain, &name).unwrap());
            let b = shape(&name, &part(&hostile, &name).unwrap());
            assert_eq!(a, b, "{name} changed shape with {payload:?}");
        }
        let doc = part(&hostile, "word/document.xml").unwrap();
        let texts = body_texts(&doc);
        for label in [
            "title",
            "part",
            "section",
            "paragraph 1",
            "signature",
            "cell 2",
        ] {
            let expected = format!("{label}: {payload}");
            assert!(
                texts.contains(&expected),
                "{expected:?} not found as a paragraph's text in {texts:?}"
            );
        }
        assert!(
            texts.contains(&format!("info label: {payload}: info value: {payload}")),
            "{texts:?}"
        );
        let header = paragraph_texts(&part(&hostile, "word/header1.xml").unwrap());
        assert_eq!(header, vec![format!("confidentiality: {payload}")]);
    }
}

/// Characters XML 1.0 does not allow at all (most C0 controls, U+FFFE, U+FFFF) can come in
/// with a pasted document; they are dropped, and tabs and line breaks inside a line become
/// spaces, so the file always opens.
#[test]
fn characters_xml_forbids_are_dropped() {
    let mut r = sample();
    r.parts[0].sections[0].paragraphs = vec![
        "א\u{0}ב\u{1}ג\u{8}ד\u{b}ה\u{c}ו\u{1f}ז\u{7f}ח".into(),
        "ט\u{FFFE}י\u{FFFF}כ".into(),
        "ל\tמ\rנ".into(),
    ];
    r.title = "כותרת\u{FFFF}".into();
    let bytes = render(&r).unwrap();
    check_package(&bytes);
    let texts = paragraph_texts(&part(&bytes, "word/document.xml").unwrap());
    assert!(texts.contains(&"אבגדהוזח".to_owned()), "{texts:?}");
    assert!(texts.contains(&"טיכ".to_owned()), "{texts:?}");
    assert!(texts.contains(&"ל מ נ".to_owned()), "{texts:?}");
    assert!(texts.contains(&"כותרת".to_owned()), "{texts:?}");
}

/// What a paragraph is expected to read as, given the text the caller passed: one Word
/// paragraph per non-blank line, trimmed, with forbidden characters dropped.
fn expected_paragraphs(text: &str) -> Vec<String> {
    text.split('\n')
        .filter(|t| !t.trim().is_empty())
        .map(|t| {
            t.trim()
                .chars()
                .filter_map(|c| match c {
                    '\t' | '\r' => Some(' '),
                    c if c.is_control() || c == '\u{FFFE}' || c == '\u{FFFF}' => None,
                    c => Some(c),
                })
                .collect()
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Any text at all (any Unicode scalar, controls and markup included) gives a well-formed
    /// document whose paragraphs read back exactly as the text that was passed.
    #[test]
    fn any_text_round_trips_into_well_formed_paragraphs(
        text in prop_oneof![
            any::<String>(),
            "[<>&\"'\\]\\[!/a-z\u{05D0}-\u{05EA} \n\t\r\u{0}\u{FFFE}\u{FFFF}\u{200F}]{0,80}",
        ],
    ) {
        let mut r = report_with(|_| "x".to_owned());
        r.parts[0].sections[0].paragraphs = vec![text.clone()];
        let bytes = render(&r).unwrap();
        let doc = part(&bytes, "word/document.xml").unwrap();
        shape("document.xml", &doc);
        let texts = paragraph_texts(&doc);
        // title, info, part, section, then the paragraph's lines.
        let got: Vec<String> = texts[4..4 + expected_paragraphs(&text).len()].to_vec();
        prop_assert_eq!(got, expected_paragraphs(&text));
    }

    /// Score charts come from fixed scales in code, but the export takes any numbers (it is
    /// also deserialized): odd axes, a zero or negative step, NaN or infinities never panic
    /// and never break the document.
    #[test]
    fn any_chart_numbers_give_a_well_formed_document(
        min in prop_oneof![any::<f64>(), -200.0f64..200.0],
        max in prop_oneof![any::<f64>(), -200.0f64..200.0],
        step in prop_oneof![any::<f64>(), -5.0f64..20.0],
        mean in -200.0f64..200.0,
        sd in prop_oneof![any::<f64>(), 0.0f64..30.0],
        value in prop_oneof![any::<f64>(), -200.0f64..200.0],
    ) {
        let mut r = sample();
        r.tables[0].charts = vec![ScoreChart {
            title: "פרופיל".into(),
            min,
            max,
            step,
            mean,
            sd,
            bars: vec![ChartBar { label: "מדד".into(), value }],
            note: String::new(),
        }];
        let bytes = render(&r).unwrap();
        shape("document.xml", &part(&bytes, "word/document.xml").unwrap());
    }
}

/// Right to left everywhere: every paragraph is a bidi paragraph, every text run is an RTL
/// run, tables are laid out right to left, the page is RTL, and Hebrew (with niqqud and the
/// direction marks used in mixed lines) is kept exactly.
#[test]
fn hebrew_and_right_to_left_are_preserved() {
    let mixed = "הַיֶּלֶד קִבֵּל ציון 112 \u{200F}(VCI)\u{200F} ו\u{200E}WISC-V\u{200E}.";
    let mut r = sample();
    r.parts[0].sections[0].paragraphs = vec![mixed.into()];
    let bytes = render(&r).unwrap();
    let doc = part(&bytes, "word/document.xml").unwrap();
    assert!(paragraph_texts(&doc).contains(&mixed.to_owned()));

    let paragraphs = doc.matches("<w:p>").count();
    let bidi_paragraphs = doc.matches("<w:p><w:pPr>").count();
    // The page break paragraph is the only one without paragraph properties.
    assert_eq!(paragraphs, bidi_paragraphs + 1, "{doc}");
    for p in doc.split("<w:p>").skip(1) {
        let p = p.split("</w:p>").next().unwrap();
        if p.contains("<w:pPr>") {
            assert!(p.contains("<w:bidi/>"), "paragraph without bidi: {p}");
        }
        for run in p.split("<w:r>").skip(1) {
            if run.contains("<w:t") {
                assert!(run.contains("<w:rtl/>"), "run without rtl: {run}");
            }
        }
    }
    assert!(doc.contains("<w:bidiVisual/>"), "tables right to left");
    assert!(doc.contains("<w:bidi/></w:sectPr>"), "page right to left");
    let settings = part(&bytes, "word/settings.xml").unwrap();
    assert!(settings.contains("w:bidi=\"he-IL\""));
    let styles = part(&bytes, "word/styles.xml").unwrap();
    assert!(styles.contains("<w:lang w:val=\"he-IL\" w:bidi=\"he-IL\"/>"));
}

/// The child's name and the other report text are only in the document itself: not in the
/// file properties, the header, the styles or the file names, and every part carries the
/// same fixed timestamp.
#[test]
fn report_text_stays_in_the_body_and_the_package_has_no_dates() {
    let bytes = render(&sample()).unwrap();
    for name in part_names(&bytes) {
        if name != "word/document.xml" {
            let content = part(&bytes, &name).unwrap();
            assert!(!content.contains("אלון"), "the name is in {name}");
            assert!(!name.contains("אלון"));
        }
    }
    let core = part(&bytes, "docProps/core.xml").unwrap();
    for field in [
        "creator",
        "lastModifiedBy",
        "created",
        "modified",
        "revision",
    ] {
        assert!(!core.contains(field), "{field} in core properties");
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    for i in 0..zip.len() {
        let f = zip.by_index(i).unwrap();
        assert_eq!(
            f.last_modified(),
            Some(zip::DateTime::default()),
            "{} carries a time",
            f.name()
        );
    }
}

// ---------------------------------------------------------------- her template

fn fixture_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/report_template")
}

/// The fictional template, zipped, with `edit` applied to each part (None drops it) and
/// `extra` parts added.
fn template(edit: impl Fn(&str, String) -> Option<String>, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let root = fixture_root();
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for p in files {
            let name = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if let Some(c) = edit(&name, std::fs::read_to_string(&p).unwrap()) {
                zip.start_file(name, opts).unwrap();
                zip.write_all(c.as_bytes()).unwrap();
            }
        }
        for (name, bytes) in extra {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

fn template_error(bytes: &[u8]) -> String {
    match render_with_template(&sample(), bytes) {
        Err(ExportError::Template(m)) => m,
        other => panic!("expected a template refusal, got {other:?}"),
    }
}

/// Hostile report text written into her template leaves the package's structure as it is
/// with plain text, and the result is well-formed throughout.
#[test]
fn markup_in_report_text_stays_text_inside_her_template() {
    let tpl = template(|_, c| Some(c), &[]);
    let plain = render_with_template(&report_with(|_| "x".to_owned()), &tpl).unwrap();
    for payload in HOSTILE {
        let out = render_with_template(&report_with(|l| format!("{l}: {payload}")), &tpl).unwrap();
        check_package(&out);
        for name in ["word/document.xml", "word/header1.xml", "word/styles.xml"] {
            assert_eq!(
                shape(name, &part(&plain, name).unwrap()),
                shape(name, &part(&out, name).unwrap()),
                "{name} changed shape with {payload:?}"
            );
        }
        let texts = paragraph_texts(&part(&out, "word/document.xml").unwrap());
        assert!(
            texts.contains(&format!("paragraph 1: {payload}")),
            "{texts:?}"
        );
    }
}

/// The English marker works like the Hebrew one.
#[test]
fn the_english_marker_marks_the_place_too() {
    let tpl = template(
        |name, c| {
            Some(if name == "word/document.xml" {
                c.replace("הדוח}}", "report}}")
            } else {
                c
            })
        },
        &[],
    );
    assert!(dv_export::check_template(&tpl).unwrap().has_marker);
    let doc = part(
        &render_with_template(&sample(), &tpl).unwrap(),
        "word/document.xml",
    )
    .unwrap();
    assert!(!doc.contains("report}}") && doc.contains("לכבוד ההורים"));
}

/// A template without the parts a document needs is refused with a sentence she can act on.
#[test]
fn a_template_missing_its_body_or_content_types_is_refused_clearly() {
    let no_body = template(|n, c| (n != "word/document.xml").then_some(c), &[]);
    assert!(template_error(&no_body).contains("אין בו גוף מסמך"));
    let no_types = template(|n, c| (n != "[Content_Types].xml").then_some(c), &[]);
    assert!(template_error(&no_types).contains("פגום"));
    let no_body_element = template(
        |n, c| {
            Some(if n == "word/document.xml" {
                c.replace("<w:body>", "<w:x>")
                    .replace("</w:body>", "</w:x>")
            } else {
                c
            })
        },
        &[],
    );
    assert!(template_error(&no_body_element).contains("פגום"));
    let broken_xml = template(
        |n, c| {
            Some(if n == "word/document.xml" {
                c.replace("</w:p>", "</w:q>")
            } else {
                c
            })
        },
        &[],
    );
    assert!(render_with_template(&sample(), &broken_xml).is_err());
}

/// Part names that climb out of the package, are absolute, or use backslashes are refused.
#[test]
fn a_template_with_escaping_part_names_is_refused() {
    for name in [
        "../evil.xml",
        "word/../../evil.xml",
        "/abs.xml",
        "word\\x.xml",
    ] {
        let tpl = template(|_, c| Some(c), &[(name, b"x")]);
        assert!(template_error(&tpl).contains("פגום"), "{name}");
    }
}

/// Macros, embedded objects and ActiveX controls are refused whatever the file is called.
#[test]
fn a_template_with_active_content_is_refused() {
    for name in [
        "word/vbaProject.bin",
        "word/embeddings/oleObject1.bin",
        "word/activeX/activeX1.xml",
    ] {
        let tpl = template(|_, c| Some(c), &[(name, b"x")]);
        assert!(template_error(&tpl).contains("מאקרו"), "{name}");
    }
    let commented = template(
        |n, c| {
            Some(if n == "word/document.xml" {
                c.replace("<w:r>", "<w:commentRangeStart w:id=\"0\"/><w:r>")
            } else {
                c
            })
        },
        &[],
    );
    assert!(template_error(&commented).contains("הערות"));
}

/// Size limits: a large file, too many parts, or a small file that unpacks to a huge one.
#[test]
fn oversized_templates_are_refused() {
    let big = vec![0u8; dv_export::MAX_TEMPLATE_BYTES + 1];
    assert!(template_error(&big).contains("גדולה מדי"));

    let many: Vec<(String, Vec<u8>)> = (0..401)
        .map(|i| (format!("word/media/p{i}.png"), vec![0u8]))
        .collect();
    let many_refs: Vec<(&str, &[u8])> = many
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    assert!(template_error(&template(|_, c| Some(c), &many_refs)).contains("פגום"));

    // 41 MB of zeros packs to a few dozen kilobytes.
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/media/bomb.png", opts).unwrap();
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..41 {
            zip.write_all(&chunk).unwrap();
        }
        zip.finish().unwrap();
    }
    let bomb = buf.into_inner();
    assert!(bomb.len() < dv_export::MAX_TEMPLATE_BYTES);
    assert!(template_error(&bomb).contains("גדולה מדי כשהיא פתוחה"));
}
