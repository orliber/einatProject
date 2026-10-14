//! Importing her documents and exporting the report.

use super::*;

pub(super) fn docx(document_body: &str, header: &str, creator: &str) -> Vec<u8> {
    use std::io::Write;
    let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let p = |t: &str| format!("<w:p><w:r><w:t xml:space=\"preserve\">{t}</w:t></w:r></w:p>");
    let parts = [
        ("word/document.xml", format!("<w:document {w}><w:body>{}</w:body></w:document>", p(document_body))),
        ("word/header1.xml", format!("<w:hdr {w}>{}</w:hdr>", p(header))),
        ("docProps/core.xml", format!("<cp:coreProperties xmlns:cp=\"c\" xmlns:dc=\"d\"><dc:creator>{creator}</dc:creator></cp:coreProperties>")),
    ];
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    for (name, content) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    buf.into_inner()
}

#[test]
fn import_shows_body_hides_names_and_keeps_names_from_margins() {
    let (_dir, mut core, case) = setup(None);
    let bytes = docx(
        "סיכום ביקור: אלון הגיע עם אמו. בבדיקה נצפה קושי במעברים.",
        "מרכז בדוי · לידי ד\"ר רונית",
        "יעל בדויה",
    );
    let p = core
        .import_document(&case, "סיכום ביקור.docx", &bytes)
        .unwrap();
    assert_eq!(p.format, "docx");
    assert_eq!(p.suggested_kind, InputKind::PriorReport);
    assert!(
        p.body.contains("אלון הגיע"),
        "the stored body keeps the real text"
    );
    assert!(!p.body.contains("מרכז בדוי"), "the header is not imported");
    assert!(p.left_out.iter().any(|l| l.contains("מרכז בדוי")));
    assert!(
        p.preview
            .iter()
            .any(|s| s.mark.is_some() && s.text.contains("אלון")),
        "{:?}",
        p.preview
    );
    // Names in the file's properties and margins are kept with the case without asking.
    assert!(p.name_suggestions.is_empty());
    let values: Vec<&str> = p.auto_hidden.iter().map(|a| a.token.as_str()).collect();
    assert!(values.contains(&"יעל בדויה"), "{values:?}");
    assert!(values.iter().any(|v| v.contains("רונית")), "{values:?}");
    let kept = core.case_detail(&case).unwrap().identities;
    assert!(kept
        .iter()
        .any(|i| i.value == "יעל בדויה" && i.source == IdentitySource::Metadata));
    assert!(p.warnings.iter().any(|w| w.contains("הכותרת")));

    // Confirming stores the (reviewed) body as case material.
    core.add_input(&case, p.suggested_kind, &p.title, &p.body)
        .unwrap();
    assert!(core
        .case_detail(&case)
        .unwrap()
        .inputs
        .iter()
        .any(|i| i.title == "סיכום ביקור"));
}

#[test]
fn import_refuses_what_it_cannot_read() {
    let (_dir, mut core, case) = setup(None);
    let err = core
        .import_document(&case, "IMG_0001.HEIC", b"\0\0\0\x18ftypheic\0\0\0\0")
        .unwrap_err();
    assert!(
        err.to_ui().message.contains("JPG"),
        "{}",
        err.to_ui().message
    );
}

pub(super) fn read_docx_text(bytes: &[u8]) -> String {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

#[test]
fn export_needs_approved_paragraphs_and_restores_names() {
    let (_dir, mut core, case) = setup(None);
    let check = core.check_export(&case).unwrap();
    assert!(!check.blocking.is_empty(), "nothing approved yet");
    assert!(matches!(
        core.export_report(&case, None),
        Err(CoreError::Refused(_))
    ));

    // A demo draft for the kindergarten section, then approve it.
    let p = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let para = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs
        .remove(0);
    core.approve_paragraph(&case, &para.id).unwrap();
    core.add_own_paragraph(&case, "referral", "ההורים של אלון פנו בשל קושי במעברים.")
        .unwrap();

    let check = core.check_export(&case).unwrap();
    assert!(check.blocking.is_empty(), "{:?}", check.blocking);
    assert_eq!(check.included_sections, 2);
    assert!(
        !check.file_name.contains("אלון"),
        "no child name in the file name: {}",
        check.file_name
    );
    assert!(check.file_name.contains("TEST-0002"));

    assert_eq!(check.score_tables, 0);
    // A score table entered in the case becomes an appendix table in the report.
    let sheet = dv_domain::ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![dv_domain::ScoreEntry {
            measure: "vci".into(),
            value: 112.0,
            note: String::new(),
        }],
        notes: String::new(),
    };
    core.save_scores(&case, None, &sheet).unwrap();
    assert_eq!(core.check_export(&case).unwrap().score_tables, 1);
    let bytes = core.export_report(&case, None).unwrap();
    let doc = read_docx_text(&bytes);
    assert!(
        doc.contains("נספח: טבלאות ציונים") && doc.contains("הבנה מילולית (VCI)"),
        "score table"
    );
    assert!(
        doc.contains("ממוצע גבוה") && doc.contains("79"),
        "range and percentile in the table"
    );
    assert!(doc.contains("פרופיל המדדים"), "score chart under the table");
    assert!(doc.contains("ההורים של אלון פנו"), "names restored");
    assert!(doc.contains("שם הילד"), "info line");
    assert!(!doc.contains("[ילד]") && !doc.contains("[גננת]"));

    // Her template: refused with a sentence she can act on, else every export goes into it.
    assert!(matches!(
        core.set_report_template(b"not a docx"),
        Err(CoreError::Refused(_))
    ));
    assert!(core.report_template().unwrap().is_none());
    let template = {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, xml) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
            ),
            (
                "word/document.xml",
                r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p/><w:sectPr><w:headerReference w:type="default" r:id="rId1"/></w:sectPr></w:body></w:document>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#,
            ),
            (
                "word/header1.xml",
                r#"<?xml version="1.0"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>נייר מכתבים בדוי</w:t></w:r></w:p></w:hdr>"#,
            ),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        buf.into_inner()
    };
    assert_eq!(core.set_report_template(&template).unwrap().headers, 1);
    let bytes = core.export_report(&case, None).unwrap();
    let mut header = String::new();
    std::io::Read::read_to_string(
        &mut zip::ZipArchive::new(std::io::Cursor::new(&bytes))
            .unwrap()
            .by_name("word/header1.xml")
            .unwrap(),
        &mut header,
    )
    .unwrap();
    assert!(
        header.contains("נייר מכתבים בדוי") && header.contains("חסוי"),
        "{header}"
    );
    assert!(read_docx_text(&bytes).contains("ההורים של אלון פנו"));
    core.clear_report_template().unwrap();
    assert!(core.report_template().unwrap().is_none());

    // Protected export: a Compound File, not a readable zip.
    let protected = core.export_report(&case, Some("סיסמה-לקובץ-1")).unwrap();
    assert!(protected.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]));
    assert!(matches!(
        core.export_report(&case, Some("קצר")),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn missing_information_marker_is_pointed_out_and_bracketed_words_are_plain() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(
        &case,
        "background",
        "ההריון תקין. [חסר: גיל ההליכה] לדברי [מחנכת] הוא משתלב.",
    )
    .unwrap();
    let check = core.check_export(&case).unwrap();
    assert!(check.blocking.is_empty(), "{:?}", check.blocking);
    assert_eq!(check.to_complete.len(), 1, "{:?}", check.to_complete);
    assert!(check.to_complete[0].contains("[חסר: גיל ההליכה]"));
    // The file goes out; the marker stays in it, the model's brackets become plain text.
    let bytes = core.export_report(&case, None).unwrap();
    let text = read_docx_text(&bytes);
    assert!(
        text.contains("[חסר: גיל ההליכה]") && text.contains("(מחנכת)"),
        "{text}"
    );
}
