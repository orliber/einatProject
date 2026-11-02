//! D-158: the background part is written one paragraph per material, in the order they are
//! sent; the sections of the assessment itself are written by topic, as before.

use super::sort_flows::add_home_intake;
use super::*;

fn request_for(core: &mut Core, fake: &FakeTransport, case: &str, key: &str) -> String {
    *fake.answer.lock().unwrap() = None;
    let prepared = core.prepare_section(case, key, "טיוטה").unwrap();
    let _ = core.send_section(&prepared.approval_id.expect("clears the gate"));
    fake.sent.lock().unwrap().last().unwrap().clone()
}

#[test]
fn the_background_part_is_written_a_paragraph_per_material() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    add_home_intake(&mut core, &case);
    core.add_input(
        &case,
        InputKind::Observation,
        "תצפית",
        "בחדר הטיפול בנה מגדל וספר בקול את הקוביות.\n\nבמבחן עבד בריכוז.",
    )
    .unwrap();

    let structure = crate::structure_for(core.vault_ref().unwrap(), &case).unwrap();
    for key in [
        "referral",
        "background",
        "parents_view",
        "kindergarten",
        "prior_assessments",
    ] {
        assert!(structure.section(key).unwrap().per_source, "{key}");
    }
    assert!(!structure.section("cognitive").unwrap().per_source);

    // Two intakes feed the background: one paragraph for each, in order.
    let sent = request_for(&mut core, &fake, &case, "background");
    assert!(
        sent.contains("פסקה לכל מקור"),
        "{}",
        sent.chars().take(3000).collect::<String>()
    );
    // A section of the assessment itself stays by topic.
    let sent = request_for(&mut core, &fake, &case, "cognitive");
    assert!(!sent.contains("פסקה לכל מקור"));
}
