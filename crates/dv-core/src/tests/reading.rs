//! Round 31: the reading of the materials. One request sorts them and sums each one up, with a
//! record for a report by another professional; what no material feeds and the order of the
//! previous assessments are worked out here. Fabricated data only; nothing leaves the machine.

use super::*;

fn reports(core: &mut Core, case: &str) -> (String, String) {
    let community = core
        .add_input(
            case,
            InputKind::PriorReport,
            "אבחון בהתפתחות הילד",
            "תאריך לידה: 3.5.2020\nנבדק ב-14.2.2023 במכון להתפתחות הילד.\n\nאלון הגיע עם אמו. נמצא עיכוב קל בשפה.\n\nמומלץ על טיפול בקלינאית תקשורת.",
        )
        .unwrap()
        .id;
    let institute = core
        .add_input(
            case,
            InputKind::PriorReport,
            "אבחון קודם במכון",
            "סיכום מרץ 2024.\n\nאלון שיתף פעולה לאורך הבדיקה. תפקוד קוגניטיבי בטווח הממוצע.\n\nמומלץ על המשך מעקב.",
        )
        .unwrap()
        .id;
    (community, institute)
}

fn reading() -> Value {
    api_json(&json!({
        "sections": [
            {"section": "prior_assessments", "passages": ["S3P1", "S3P2", "S3P3", "S4P1", "S4P2", "S4P3"]},
            {"section": "kindergarten", "passages": ["S1P1"]}
        ],
        "materials": [
            {"material": 3, "summary": "אבחון של [ילד] בהתפתחות הילד: עיכוב קל בשפה, ציון 85.", "record": {
                "kind": "אבחון התפתחותי", "by": "רופאה התפתחותית", "setting": "community",
                "findings": "עיכוב קל בשפה.", "recommendations": "טיפול בקלינאית תקשורת."
            }},
            {"material": 4, "summary": "אבחון קודם במכון. אלון שיתף פעולה.", "record": {
                "kind": "אבחון פסיכולוגי", "by": "פסיכולוגית", "setting": "institute",
                "findings": "תפקוד בטווח הממוצע.", "recommendations": "המשך מעקב."
            }},
            {"material": 1, "summary": "שיחה עם הגננת.", "record": {
                "kind": "לא דוח", "by": "", "setting": "unknown", "findings": "x", "recommendations": ""
            }}
        ]
    }))
}

#[test]
fn one_reading_sorts_sums_up_and_records_and_the_order_is_fixed_here() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let (community, institute) = reports(&mut core, &case);

    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(reading());
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap()[0].clone();
    assert!(sent.contains("materials") && sent.contains("record"));
    assert!(!sent.contains("אלון"), "the reading goes out filtered");

    let detail = core.case_detail(&case).unwrap();
    let route = |id: &str| detail.routing.iter().find(|r| r.input_id == id).unwrap();
    let c = route(&community);
    // Shown with the names back, as plain text.
    assert!(c.summary.starts_with("אבחון של אלון"), "{}", c.summary);
    let rec = c.record.as_ref().unwrap();
    assert_eq!(rec.setting, dv_domain::Setting::Community);
    // The date is read here, from the document, and never the date of birth.
    assert_eq!(rec.date, "2/2023");
    assert!(
        c.summary_notes.iter().any(|n| n.contains("85")),
        "{:?}",
        c.summary_notes
    );
    // A real name in the answer is pointed out: the AI only ever saw tags.
    let i = route(&institute);
    assert!(
        i.summary_notes.iter().any(|n| n.contains("אלון")),
        "{:?}",
        i.summary_notes
    );
    assert_eq!(i.record.as_ref().unwrap().date, "3/2024");
    // A record for a material that is not a report is not kept.
    let kindergarten = &detail.routing[0];
    assert_eq!(kindergarten.summary, "שיחה עם הגננת.");
    assert!(kindergarten.record.is_none());
    // The institute first, then the community.
    assert_eq!(
        detail.prior_order,
        vec![institute.clone(), community.clone()]
    );
    // What no material feeds, by title; the sections that are fed are not listed.
    assert!(!detail.missing.is_empty());
    let fed: Vec<&str> = detail
        .sections
        .iter()
        .filter(|s| s.source_count > 0)
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        detail.missing.iter().all(|m| !fed.contains(&m.as_str())),
        "{:?} {fed:?}",
        detail.missing
    );
    // Everything was read: nothing is offered again.
    assert!(detail.routing.iter().all(|r| !r.needs_sorting));
    assert!(matches!(
        core.prepare_sort(&case),
        Err(CoreError::Refused(_))
    ));

    // The section goes out in the order fixed here, and says so.
    *fake.answer.lock().unwrap() = None;
    let p = core
        .prepare_section(&case, "prior_assessments", "טיוטה")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(sent.contains(dv_ai::ORDER_LINE));
    let at = |t: &str| sent.find(t).unwrap_or_else(|| panic!("{t} not sent"));
    assert!(at("תפקוד קוגניטיבי") < at("עיכוב קל"));

    // An edit makes the summary stale: it is read again.
    core.update_input(
        &case,
        &community,
        "אבחון בהתפתחות הילד",
        "נמצא עיכוב קל בשפה.",
    )
    .unwrap();
    let detail = core.case_detail(&case).unwrap();
    let c = detail
        .routing
        .iter()
        .find(|r| r.input_id == community)
        .unwrap();
    assert!(c.summary.is_empty() && c.record.is_none() && c.needs_sorting);
    assert_eq!(detail.prior_order, vec![institute]);
}

#[test]
fn demo_mode_sums_up_locally_too() {
    let (_dir, mut core, case) = setup(None);
    let (community, _) = reports(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    assert!(prepared.demo_mode);
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    let detail = core.case_detail(&case).unwrap();
    let c = detail
        .routing
        .iter()
        .find(|r| r.input_id == community)
        .unwrap();
    assert!(!c.summary.is_empty());
    assert!(c.record.is_some(), "{c:?}");
    assert!(
        c.summary_notes.is_empty(),
        "the filter's own words are no invention: {c:?}"
    );
    assert!(detail.routing.iter().all(|r| !r.needs_sorting));
}
