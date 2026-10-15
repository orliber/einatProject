//! L6: the security page as ten layers checked now, and the same list on paper. Fabricated
//! data only; nothing is sent.

use super::*;

#[test]
fn the_page_shows_ten_layers_checked_now_and_what_depends_on_her() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    let s = core.security_status();
    assert_eq!(s.layers.len(), 10);
    let gate = s.layers.iter().find(|l| l.key == "privacy_gate").unwrap();
    assert_eq!(
        gate.state, "ok",
        "the gate refused the invented name just now"
    );
    assert!(gate.now.starts_with("נבדק עכשיו"));
    let keys: Vec<_> = s.yours.iter().map(|l| l.key.as_str()).collect();
    assert!(keys.contains(&"recovery_kit") && keys.contains(&"backup"));
    assert!(
        !s.send_stopped,
        "a gate that passes its check does not stop sending"
    );
}

#[test]
fn the_paper_opens_without_a_password_and_holds_no_case() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    let case = core
        .create_case(
            CaseMeta {
                code: "TEST-0004".into(),
                ..CaseMeta::default()
            },
            vec![],
        )
        .unwrap();
    assert!(!case.is_empty());
    let pdf = match core.security_layers_pdf() {
        Ok(pdf) => pdf,
        // No Hebrew font on this computer.
        Err(CoreError::Refused(_)) => return,
        Err(e) => panic!("{e:?}"),
    };
    assert!(pdf.starts_with(b"%PDF-2.0"));
    let raw = String::from_utf8_lossy(&pdf);
    assert!(!raw.contains("TEST-0004"));
    if let Ok(out) = std::env::var("DV_PDF_SAMPLE") {
        std::fs::write(std::path::Path::new(&out).join("layers.pdf"), &pdf).unwrap();
    }
}
