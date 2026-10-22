//! The logo and the parents' version (round 55, D-095). Fabricated data only; nothing is sent:
//! the test has no transport at all.

use super::import_export::read_docx_text;
use super::*;

/// A 4×2 PNG with an "Author" text chunk that must not reach a report.
const PNG_HEX: &str = "89504e470d0a1a0a0000000d4948445200000004000000020802000000f0caea340000000c74455874417574686f720042647579613e1c90560000001f49444154789c05c1010100300cc3a008bb922a41d8041e2a2f72a979636e3e569607f9a590a9020000000049454e44ae426082";

fn part(docx: &[u8], name: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(docx)).unwrap();
    let mut f = zip.by_name(name).ok()?;
    let mut out = Vec::new();
    f.read_to_end(&mut out).unwrap();
    Some(out)
}

/// The words of the Word file, without its markup (attributes hold numbers too).
fn words(docx: &[u8]) -> String {
    let xml = read_docx_text(docx);
    let mut out = String::new();
    let mut inside = false;
    for c in xml.chars() {
        match c {
            '<' => {
                inside = true;
                out.push(' ');
            }
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

fn export(core: &mut Core, case: &str, parents: Option<ParentsOptions>) -> (Vec<u8>, ExportCheck) {
    let job = core.begin_export_report_as(case, None, parents).unwrap();
    core.finish_export(job.run()).unwrap()
}

#[test]
fn her_logo_is_checked_by_its_bytes_and_printed_without_its_metadata() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "referral", "ההורים של אלון פנו בשל קושי במעברים.")
        .unwrap();
    for (bytes, says) in [
        (
            &b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>"[..],
            "SVG",
        ),
        (&b"GIF89a\x01\x00\x01\x00"[..], "PNG או JPEG"),
        (&b""[..], "ריק"),
    ] {
        match core.set_report_logo(bytes) {
            Err(CoreError::Refused(m)) => assert!(m.contains(says), "{m}"),
            other => panic!("not refused: {other:?}"),
        }
    }
    let mut big = dv_vault::crypto::unhex(PNG_HEX).unwrap();
    big.resize(dv_export::MAX_LOGO_BYTES + 1, 0);
    assert!(matches!(
        core.set_report_logo(&big),
        Err(CoreError::Refused(_))
    ));
    assert!(core.report_logo().unwrap().is_none(), "nothing kept");

    let view = core
        .set_report_logo(&dv_vault::crypto::unhex(PNG_HEX).unwrap())
        .unwrap();
    assert_eq!(
        (view.format.as_str(), view.width, view.height),
        ("PNG", 4, 2)
    );
    assert_eq!(core.report_logo().unwrap(), Some(view));
    let (docx, _) = export(&mut core, &case, None);
    let logo = part(&docx, "word/media/logo.png").expect("the logo in the Word file");
    assert!(logo.starts_with(b"\x89PNG"));
    assert!(!logo.windows(5).any(|w| w == b"Bduya"), "metadata left out");
    // The parents' version carries it too.
    let (docx, _) = export(&mut core, &case, Some(ParentsOptions::default()));
    assert!(part(&docx, "word/media/logo.png").is_some());

    core.clear_report_logo().unwrap();
    assert!(core.report_logo().unwrap().is_none());
    let (docx, _) = export(&mut core, &case, None);
    assert!(part(&docx, "word/media/logo.png").is_none());
}

#[test]
fn the_parents_version_has_the_chart_her_sentence_and_no_numbers_by_default() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "referral", "ההורים של אלון פנו בשל קושי במעברים.")
        .unwrap();
    core.add_own_paragraph(
        &case,
        "kindergarten",
        "בהבנה המילולית קיבל ציון 112. אלון נהנה מהמשחק ושיתף פעולה.",
    )
    .unwrap();
    let entry = |measure: &str, value: f64| dv_domain::ScoreEntry {
        measure: measure.into(),
        value,
        note: String::new(),
    };
    core.save_scores(
        &case,
        None,
        &dv_domain::ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                entry("vci", 112.0),
                entry("fsiq", 100.0),
                entry("wmi", 84.0),
            ],
            notes: String::new(),
        },
    )
    .unwrap();

    let (full, full_check) = export(&mut core, &case, None);
    assert!(read_docx_text(&full).contains("נספח: טבלאות ציונים"));

    let (bytes, check) = export(&mut core, &case, Some(ParentsOptions::default()));
    let doc = words(&bytes);
    assert!(
        doc.contains("גרסה להורים") && doc.contains("להורים שלום"),
        "her opening"
    );
    assert!(
        doc.contains("במבט אחד") && doc.contains("בטווח הגיל") && doc.contains("קשה יותר"),
        "the chart"
    );
    assert!(doc.contains("הבנה מילולית") && doc.contains("זיכרון עבודה"));
    assert!(!doc.contains("נספח: טבלאות ציונים"), "no score tables");
    for n in ["112", "100", "84", "79"] {
        assert!(!doc.contains(n), "{n} in the parents' version");
    }
    assert!(doc.contains("אלון נהנה מהמשחק") && doc.contains("ההורים של אלון פנו"));
    assert!(check.file_name.contains("להורים") && !check.file_name.contains("אלון"));
    assert_ne!(check.file_name, full_check.file_name);

    // With numbers, the scores are back, still without the full tables.
    let (bytes, _) = export(
        &mut core,
        &case,
        Some(ParentsOptions { show_numbers: true }),
    );
    let doc = words(&bytes);
    assert!(doc.contains(" 112 ") && doc.contains("ציון 112"));
    assert!(!doc.contains("נספח: טבלאות ציונים"));

    // Her own sentence, from the settings.
    let mut settings = core.report_settings().unwrap();
    settings.parents_opening = "שלום רב, כאן מה שראיתי בקצרה.".into();
    core.set_report_settings(&settings).unwrap();
    let (bytes, _) = export(&mut core, &case, Some(ParentsOptions::default()));
    assert!(read_docx_text(&bytes).contains("שלום רב, כאן מה שראיתי בקצרה."));

    // Each export is in the activity log, the parents' version marked.
    let marked = core
        .vault_ref()
        .unwrap()
        .audit_find("export", "\"parents\":true")
        .unwrap();
    assert!(marked.is_some());
}

#[test]
fn settings_kept_before_round_55_read_with_the_default_opening() {
    let old =
        r#"{"title":"דוח","font":"David","confidentiality":"חסוי","signature":[],"supervisor":[]}"#;
    let s: ReportSettings = serde_json::from_str(old).unwrap();
    assert_eq!(s.parents_opening, crate::views::default_parents_opening());
}

/// One chart for each test, under its name (two tests can share a measure's name); a test she
/// does not give is not charted (D-098) but its numbers still leave the text; the dialog knows
/// how many sentences go and whether there is a chart at all.
#[test]
fn each_test_gets_its_own_chart_and_the_dialog_knows_what_goes() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(
        &case,
        "kindergarten",
        "בהבנה המילולית קיבל ציון 112. אלון נהנה מהמשחק. ציוניו בקוביות היו 7 ו-9.",
    )
    .unwrap();
    let sheet = |instrument: &str, entries: &[(&str, f64)]| dv_domain::ScoreSheet {
        instrument: instrument.into(),
        module: String::new(),
        cutoff: None,
        entries: entries
            .iter()
            .map(|(m, v)| dv_domain::ScoreEntry {
                measure: (*m).into(),
                value: *v,
                note: String::new(),
            })
            .collect(),
        notes: String::new(),
    };
    let check = core.check_export(&case).unwrap();
    assert_eq!(check.parents_chart_rows, 0, "no table, no chart");
    assert_eq!(check.parents_left_out, 2);

    core.save_scores(&case, None, &sheet("wppsi_iv", &[("vci", 112.0)]))
        .unwrap();
    core.save_scores(
        &case,
        None,
        &sheet("wisc_v", &[("vci", 95.0), ("wmi", 88.0)]),
    )
    .unwrap();
    // A questionnaire only: still no row of its own.
    core.save_scores(&case, None, &sheet("brief_p", &[("inhibit", 71.0)]))
        .unwrap();
    let check = core.check_export(&case).unwrap();
    assert_eq!(check.parents_chart_rows, 3);
    assert!(
        !check.file_name.contains("להורים"),
        "the full report's name"
    );

    let (bytes, _) = export(&mut core, &case, Some(ParentsOptions::default()));
    let doc = words(&bytes);
    assert!(doc.contains("במבט אחד · WPPSI-IV") && doc.contains("במבט אחד · WISC-V"));
    assert_eq!(
        doc.matches("הבנה מילולית").count(),
        2,
        "one row in each chart"
    );

    core.set_instrument_status("wisc_v", crate::InstrumentStatus::NotUsed)
        .unwrap();
    assert_eq!(core.check_export(&case).unwrap().parents_chart_rows, 1);
    let (bytes, _) = export(&mut core, &case, Some(ParentsOptions::default()));
    let doc = words(&bytes);
    assert!(doc.contains("במבט אחד · WPPSI-IV") && !doc.contains("WISC-V"));
    for n in ["112", "95", "88", "71"] {
        assert!(!doc.contains(n), "{n} in the parents' version");
    }
    assert!(doc.contains("אלון נהנה מהמשחק"));
}
