//! Tests of the clinic rules. Every name, date and sum here is invented.

use std::collections::{BTreeSet, HashMap, HashSet};

use proptest::prelude::*;

use super::*;
use crate::records::Age;
use crate::DomainError;

// ------------------------------------------------------------------ invented records

/// The day most tests call "today": a Sunday.
const TODAY: &str = "2026-11-15";

fn patient(id: &str, name: &str) -> Patient {
    Patient {
        id: id.to_owned(),
        name: name.to_owned(),
        birth_date: Some("2020-07-20".to_owned()),
        gender: None,
        contacts: vec![Contact {
            role: ContactRole::Mother,
            name: "דנה".to_owned(),
            phone: "000-0000".to_owned(),
            email: "dana@example.com".to_owned(),
        }],
        framework: "גן הדקל".to_owned(),
        referred_by: String::new(),
        reason: String::new(),
        notes: String::new(),
        status: PatientStatus::Active,
        waiting_since: None,
        urgent: false,
        session_fee: None,
        case_id: None,
        created_on: "2026-09-01".to_owned(),
    }
}

/// An hour-long visit with no fee, so a test that is not about money stays out of debt.
fn visit(id: &str, kind: VisitKind, date: &str, start: u16, status: VisitStatus) -> Visit {
    Visit {
        id: id.to_owned(),
        patient_id: (kind != VisitKind::Block).then(|| "p1".to_owned()),
        kind,
        title: String::new(),
        date: date.to_owned(),
        start,
        minutes: 60,
        place: String::new(),
        status,
        status_note: String::new(),
        fee: 0,
        charge_missed: false,
        series_id: None,
        note: String::new(),
    }
}

fn paid_visit(id: &str, date: &str, status: VisitStatus, fee: Agorot) -> Visit {
    Visit {
        fee,
        ..visit(id, VisitKind::Assessment, date, 540, status)
    }
}

fn note(id: &str, visit_id: Option<&str>, date: &str) -> SessionNote {
    SessionNote {
        id: id.to_owned(),
        patient_id: "p1".to_owned(),
        visit_id: visit_id.map(str::to_owned),
        date: date.to_owned(),
        record: "הילד שיתף פעולה לאורך המפגש.".to_owned(),
        personal: String::new(),
        case_input_id: None,
    }
}

fn payment(id: &str, date: &str, amount: Agorot) -> Payment {
    Payment {
        id: id.to_owned(),
        patient_id: "p1".to_owned(),
        date: date.to_owned(),
        amount,
        method: PayMethod::Bit,
        receipt: "101".to_owned(),
        payer: String::new(),
        note: String::new(),
        voided: false,
        void_reason: String::new(),
    }
}

fn charge(id: &str, date: &str, amount: Agorot) -> Charge {
    Charge {
        id: id.to_owned(),
        patient_id: "p1".to_owned(),
        date: date.to_owned(),
        amount,
        what: "חבילת אבחון".to_owned(),
        voided: false,
        void_reason: String::new(),
    }
}

fn task(id: &str, patient_id: Option<&str>, due: Option<&str>) -> ClinicTask {
    ClinicTask {
        id: id.to_owned(),
        patient_id: patient_id.map(str::to_owned),
        kind: TaskKind::Call,
        title: "לחזור לגננת".to_owned(),
        due: due.map(str::to_owned),
        done_on: None,
        note: String::new(),
    }
}

fn today() -> Today {
    Today {
        date: TODAY.to_owned(),
        minute: 12 * 60,
    }
}

/// The refusal's sentence; panics when the rule let the record through.
fn refusal<T: std::fmt::Debug>(result: Result<T, DomainError>) -> String {
    match result {
        Err(DomainError::Invalid(text)) => {
            assert_hebrew_sentence(&text);
            text
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// What every text she sees has to be: Hebrew, and without an em-dash.
fn assert_hebrew_sentence(text: &str) {
    assert!(
        text.chars().any(|c| ('א'..='ת').contains(&c)),
        "not Hebrew: {text}"
    );
    assert!(!text.contains('—'), "an em-dash in: {text}");
}

fn hebrew(chars: usize) -> String {
    "א".repeat(chars)
}

// ------------------------------------------------------------------ dates

#[test]
fn weekdays_and_day_names() {
    assert_eq!(weekday("1970-01-01"), None); // before the years a date may have
    assert_eq!(weekday("2000-01-01"), Some(6));
    assert_eq!(weekday(TODAY), Some(0));
    assert_eq!(weekday("2024-02-29"), Some(4));
    assert_eq!(weekday("2026-12-31"), Some(4));
    assert_eq!(weekday("2027-01-01"), Some(5));
    assert_eq!(weekday("2026-02-30"), None);
    let names: Vec<&str> = (0..7).map(day_name).collect();
    assert_eq!(
        names,
        ["ראשון", "שני", "שלישי", "רביעי", "חמישי", "שישי", "שבת"]
    );
    assert_eq!(day_name(7), "");
    assert_eq!(day_name_of(TODAY), "ראשון");
    assert_eq!(day_name_of("2026-11-21"), "שבת");
    assert_eq!(day_name_of("אתמול"), "");
}

#[test]
fn dates_and_times_as_she_reads_them() {
    assert_eq!(display_date(TODAY), "15.11.2026");
    assert_eq!(display_date("2027-01-05"), "05.01.2027");
    assert_eq!(display_date("לא תאריך"), "לא תאריך");
    assert_eq!(display_time(0), "00:00");
    assert_eq!(display_time(570), "09:30");
    assert_eq!(display_time(1_439), "23:59");
}

#[test]
fn strict_dates_only() {
    assert_eq!(parse_date("2026-11-15"), Some((2026, 11, 15)));
    for bad in [
        "",
        "2026-11-15 ",
        " 2026-11-15",
        "2026-+9-+9",
        "2026-9-15",
        "2026-02-29",
        "2026-13-01",
        "1999-12-31",
        "2201-01-01",
        "15.11.2026",
        "２０２６-11-15",
    ] {
        assert_eq!(parse_date(bad), None, "{bad}");
    }
}

#[test]
fn adding_days_crosses_months_years_and_leap_days() {
    assert_eq!(add_days("2026-12-31", 1).as_deref(), Some("2027-01-01"));
    assert_eq!(add_days("2027-01-01", -1).as_deref(), Some("2026-12-31"));
    assert_eq!(add_days("2024-02-28", 1).as_deref(), Some("2024-02-29"));
    assert_eq!(add_days("2025-02-28", 1).as_deref(), Some("2025-03-01"));
    assert_eq!(add_days("2100-02-28", 1).as_deref(), Some("2100-03-01")); // not a leap year
    assert_eq!(add_days("2026-04-30", 1).as_deref(), Some("2026-05-01"));
    assert_eq!(add_days(TODAY, 0).as_deref(), Some(TODAY));
    assert_eq!(add_days("2200-12-31", 1), None);
    assert_eq!(add_days("2000-01-01", -1), None);
    assert_eq!(add_days(TODAY, i64::MAX), None);
    assert_eq!(add_days(TODAY, i64::MIN), None);
    assert_eq!(add_days("מחר", 1), None);
}

#[test]
fn days_between_dates() {
    assert_eq!(days_between("2026-01-01", "2027-01-01"), Some(365));
    assert_eq!(days_between("2024-01-01", "2025-01-01"), Some(366));
    assert_eq!(days_between(TODAY, TODAY), Some(0));
    assert_eq!(days_between(TODAY, "2026-11-08"), Some(-7));
    assert_eq!(days_between(TODAY, "x"), None);
    assert_eq!(days_between("x", TODAY), None);
}

#[test]
fn the_week_runs_sunday_to_saturday() {
    let week = |date: &str| week_of(date).unwrap();
    assert_eq!(week(TODAY), (TODAY.to_owned(), "2026-11-21".to_owned()));
    assert_eq!(week("2026-11-21"), week(TODAY));
    assert_eq!(week("2026-11-18"), week(TODAY));
    assert_eq!(
        week("2026-12-31"),
        ("2026-12-27".to_owned(), "2027-01-02".to_owned())
    );
    assert_eq!(
        week("2024-02-29"),
        ("2024-02-25".to_owned(), "2024-03-02".to_owned())
    );
    // The first supported day is a Saturday: its week starts just before the range.
    assert_eq!(
        week("2000-01-01"),
        ("1999-12-26".to_owned(), "2000-01-01".to_owned())
    );
    assert_eq!(week_of("2026-11-31"), None);
}

#[test]
fn months_have_a_first_and_a_last_day() {
    let bounds = |month: &str| month_bounds(month).unwrap();
    assert_eq!(
        bounds("2024-02"),
        ("2024-02-01".to_owned(), "2024-02-29".to_owned())
    );
    assert_eq!(
        bounds("2026-02"),
        ("2026-02-01".to_owned(), "2026-02-28".to_owned())
    );
    assert_eq!(
        bounds("2026-12"),
        ("2026-12-01".to_owned(), "2026-12-31".to_owned())
    );
    assert_eq!(
        bounds("2026-04"),
        ("2026-04-01".to_owned(), "2026-04-30".to_owned())
    );
    for bad in [
        "2026-13", "2026-00", "2026-1", "26-01", "2026-+1", "2026/01", "1999-12", "",
    ] {
        assert_eq!(month_bounds(bad), None, "{bad}");
        refusal(validate_month(bad));
    }
    assert!(validate_month("2026-11").is_ok());
    assert_eq!(month_of(TODAY).as_deref(), Some("2026-11"));
    assert_eq!(month_of("x"), None);
}

#[test]
fn age_counts_whole_years_and_months() {
    let age = |birth: &str, on: &str| age_on(Some(birth), on);
    let is = |years, months| Some(Age { years, months });
    assert_eq!(age("2020-07-20", TODAY), is(6, 3));
    assert_eq!(age("2020-11-15", TODAY), is(6, 0));
    assert_eq!(age("2020-11-16", TODAY), is(5, 11));
    assert_eq!(age("2026-11-15", TODAY), is(0, 0));
    assert_eq!(age("2026-10-16", TODAY), is(0, 0));
    assert_eq!(age("2026-10-15", TODAY), is(0, 1));
    // Across a year end.
    assert_eq!(age("2025-12-31", "2026-01-30"), is(0, 0));
    assert_eq!(age("2025-12-31", "2026-01-31"), is(0, 1));
    // Born on a leap day: a year old on 28 February, as `dates::add_years` has it.
    assert_eq!(age("2020-02-29", "2021-02-27"), is(0, 11));
    assert_eq!(age("2020-02-29", "2021-02-28"), is(1, 0));
    assert_eq!(age("2020-02-29", "2024-02-29"), is(4, 0));
    // Born on the 31st: the month is complete on the last day of a shorter month.
    assert_eq!(age("2020-01-31", "2020-04-29"), is(0, 2));
    assert_eq!(age("2020-01-31", "2020-04-30"), is(0, 3));
    assert_eq!(age("2000-01-01", "2200-12-31"), is(200, 11));
}

#[test]
fn no_age_without_a_usable_birth_date() {
    assert_eq!(age_on(None, TODAY), None);
    assert_eq!(age_on(Some(""), TODAY), None);
    assert_eq!(age_on(Some("2020-02-30"), TODAY), None);
    assert_eq!(age_on(Some("2026-11-16"), TODAY), None); // tomorrow
    assert_eq!(age_on(Some("2020-07-20"), "היום"), None);
}

// ------------------------------------------------------------------ normalizing

#[test]
fn a_card_is_stored_clean() {
    let mut raw = patient("p1", "  נועם   לוי \n");
    raw.birth_date = Some("  ".to_owned());
    raw.waiting_since = Some(" 2026-10-01 ".to_owned());
    raw.case_id = Some(String::new());
    raw.framework = " גן   הדקל ".to_owned();
    raw.reason = "\n קושי בשפה.\nשורה שנייה \n".to_owned();
    raw.contacts = vec![
        Contact {
            role: ContactRole::Father,
            name: " יואב ".to_owned(),
            phone: " 000  0000 ".to_owned(),
            email: " yoav@example.com ".to_owned(),
        },
        Contact {
            role: ContactRole::Other,
            name: "  ".to_owned(),
            phone: String::new(),
            email: "\t".to_owned(),
        },
    ];
    let clean = normalize_patient(raw);
    assert_eq!(clean.name, "נועם לוי");
    assert_eq!(clean.birth_date, None);
    assert_eq!(clean.waiting_since.as_deref(), Some("2026-10-01"));
    assert_eq!(clean.case_id, None);
    assert_eq!(clean.framework, "גן הדקל");
    assert_eq!(clean.reason, "קושי בשפה.\nשורה שנייה");
    assert_eq!(clean.contacts.len(), 1);
    assert_eq!(clean.contacts[0].name, "יואב");
    assert_eq!(clean.contacts[0].phone, "000 0000");
    assert_eq!(clean.contacts[0].email, "yoav@example.com");
    assert!(validate_patient(&clean).is_ok());
    // Normalizing twice changes nothing.
    assert_eq!(normalize_patient(clean.clone()), clean);
}

#[test]
fn records_are_stored_clean() {
    let mut v = visit("v1", VisitKind::Block, TODAY, 600, VisitStatus::Planned);
    v.patient_id = Some(" ".to_owned());
    v.series_id = Some(String::new());
    v.title = "  כתיבת   דוחות ".to_owned();
    v.note = " להביא את השאלונים \n".to_owned();
    let v = normalize_visit(v);
    assert_eq!(
        (v.patient_id, v.series_id, v.title.as_str(), v.note.as_str()),
        (None, None, "כתיבת דוחות", "להביא את השאלונים")
    );

    let mut n = note("n1", Some(" "), " 2026-11-10 ");
    n.record = "\nשורה ראשונה\n\nשורה שנייה\n".to_owned();
    n.case_input_id = Some(" ".to_owned());
    let n = normalize_note(n);
    assert_eq!((n.visit_id, n.case_input_id), (None, None));
    assert_eq!(n.date, "2026-11-10");
    assert_eq!(n.record, "שורה ראשונה\n\nשורה שנייה");

    let mut p = payment("pay1", " 2026-11-10 ", 100);
    p.receipt = " 101 ".to_owned();
    p.payer = " קרן   בדויה ".to_owned();
    let p = normalize_payment(p);
    assert_eq!(
        (p.date.as_str(), p.receipt.as_str(), p.payer.as_str()),
        ("2026-11-10", "101", "קרן בדויה")
    );

    let mut c = charge("c1", "2026-11-10", 100);
    c.what = "  כתיבת   הדוח ".to_owned();
    assert_eq!(normalize_charge(c).what, "כתיבת הדוח");

    let mut t = task("t1", Some(""), Some(" "));
    t.title = " לשלוח   שאלון ".to_owned();
    t.done_on = Some(String::new());
    let t = normalize_task(t);
    assert_eq!(
        (t.patient_id, t.due, t.done_on, t.title.as_str()),
        (None, None, None, "לשלוח שאלון")
    );

    let mut s = ClinicSettings {
        work_days: vec![4, 0, 4, 2],
        ..ClinicSettings::default()
    };
    s.places = vec![
        " הקליניקה ".to_owned(),
        String::new(),
        "הקליניקה".to_owned(),
        "גן".to_owned(),
    ];
    s.note_templates[0].name = "  היכרות ".to_owned();
    let s = normalize_settings(s);
    assert_eq!(s.work_days, [0, 2, 4]);
    assert_eq!(s.places, ["הקליניקה", "גן"]);
    assert_eq!(s.note_templates[0].name, "היכרות");
    assert!(validate_settings(&s).is_ok());
}

// ------------------------------------------------------------------ validation

#[test]
fn a_patient_needs_a_name_of_a_sane_length() {
    assert!(validate_patient(&patient("p1", "נועם")).is_ok());
    refusal(validate_patient(&patient("p1", "")));
    refusal(validate_patient(&patient("p1", "  \n ")));
    // Chars, not bytes: 120 Hebrew letters are 240 bytes and still a name.
    assert!(validate_patient(&patient("p1", &hebrew(NAME_MAX))).is_ok());
    refusal(validate_patient(&patient("p1", &hebrew(NAME_MAX + 1))));
}

#[test]
fn a_patient_card_within_its_limits() {
    let ok = patient("p1", "תמר");
    let broken = |change: fn(&mut Patient)| {
        let mut p = patient("p1", "תמר");
        change(&mut p);
        refusal(validate_patient(&p))
    };
    broken(|p| p.birth_date = Some("2020-02-30".to_owned()));
    broken(|p| p.birth_date = Some(String::new()));
    broken(|p| p.waiting_since = Some("אתמול".to_owned()));
    broken(|p| p.created_on = String::new());
    broken(|p| p.framework = hebrew(SHORT_MAX + 1));
    broken(|p| p.referred_by = hebrew(SHORT_MAX + 1));
    broken(|p| p.reason = hebrew(LONG_MAX + 1));
    broken(|p| p.notes = hebrew(LONG_MAX + 1));
    broken(|p| p.session_fee = Some(-1));
    broken(|p| p.session_fee = Some(MAX_AMOUNT + 1));
    broken(|p| p.contacts[0].phone = hebrew(SHORT_MAX + 1));
    broken(|p| p.contacts[0].email = hebrew(SHORT_MAX + 1));
    broken(|p| p.contacts[0].name = hebrew(NAME_MAX + 1));
    broken(|p| {
        p.contacts[0] = Contact {
            role: ContactRole::Other,
            name: " ".to_owned(),
            phone: String::new(),
            email: String::new(),
        };
    });
    broken(|p| p.contacts = vec![p.contacts[0].clone(); MAX_CONTACTS + 1]);

    let mut full = ok.clone();
    full.contacts = vec![ok.contacts[0].clone(); MAX_CONTACTS];
    full.reason = hebrew(LONG_MAX);
    full.notes = hebrew(LONG_MAX);
    full.framework = hebrew(SHORT_MAX);
    full.session_fee = Some(MAX_AMOUNT);
    full.birth_date = None;
    assert!(validate_patient(&full).is_ok());
    full.session_fee = Some(0);
    full.contacts.clear();
    assert!(validate_patient(&full).is_ok());
    // A contact may be a phone alone.
    full.contacts.push(Contact {
        role: ContactRole::Guardian,
        name: String::new(),
        phone: "000-0000".to_owned(),
        email: String::new(),
    });
    assert!(validate_patient(&full).is_ok());
}

#[test]
fn a_block_has_no_patient_and_no_fee_and_the_rest_need_a_patient() {
    let block = visit("v1", VisitKind::Block, TODAY, 600, VisitStatus::Planned);
    assert!(validate_visit(&block).is_ok());
    let mut with_patient = block.clone();
    with_patient.patient_id = Some("p1".to_owned());
    refusal(validate_visit(&with_patient));
    let mut with_fee = block;
    with_fee.fee = 100;
    refusal(validate_visit(&with_fee));

    for kind in VISIT_KINDS.into_iter().filter(|k| *k != VisitKind::Block) {
        let ok = visit("v1", kind, TODAY, 600, VisitStatus::Planned);
        assert!(validate_visit(&ok).is_ok());
        let mut nobody = ok.clone();
        nobody.patient_id = None;
        refusal(validate_visit(&nobody));
        nobody.patient_id = Some("  ".to_owned());
        refusal(validate_visit(&nobody));
    }
}

#[test]
fn a_visit_fits_in_its_day() {
    let with = |start: u16, minutes: u16| Visit {
        minutes,
        ..visit("v1", VisitKind::Therapy, TODAY, start, VisitStatus::Planned)
    };
    assert!(validate_visit(&with(0, MIN_MINUTES)).is_ok());
    assert!(validate_visit(&with(0, MAX_MINUTES)).is_ok());
    refusal(validate_visit(&with(0, MIN_MINUTES - 1)));
    refusal(validate_visit(&with(0, 0)));
    refusal(validate_visit(&with(0, MAX_MINUTES + 1)));
    // Ending exactly at midnight is the last moment allowed.
    assert!(validate_visit(&with(1_380, 60)).is_ok());
    refusal(validate_visit(&with(1_381, 60)));
    refusal(validate_visit(&with(1_440, 5)));
    refusal(validate_visit(&with(u16::MAX, 5)));
    refusal(validate_visit(&with(600, u16::MAX)));
}

#[test]
fn a_visit_within_its_limits() {
    let broken = |change: fn(&mut Visit)| {
        let mut v = visit("v1", VisitKind::Intake, TODAY, 600, VisitStatus::Planned);
        change(&mut v);
        refusal(validate_visit(&v))
    };
    broken(|v| v.date = "2026-11-31".to_owned());
    broken(|v| v.date = String::new());
    broken(|v| v.fee = -1);
    broken(|v| v.fee = MAX_AMOUNT + 1);
    broken(|v| v.title = hebrew(SHORT_MAX + 1));
    broken(|v| v.place = hebrew(SHORT_MAX + 1));
    broken(|v| v.status_note = hebrew(SHORT_MAX + 1));
    broken(|v| v.note = hebrew(LONG_MAX + 1));

    let mut full = visit("v1", VisitKind::Intake, TODAY, 600, VisitStatus::Held);
    full.fee = MAX_AMOUNT;
    full.title = hebrew(SHORT_MAX);
    full.place = hebrew(SHORT_MAX);
    full.status_note = hebrew(SHORT_MAX);
    full.note = hebrew(LONG_MAX);
    assert!(validate_visit(&full).is_ok());
}

#[test]
fn a_repeat_within_its_limits() {
    let repeat = |every_weeks, count| Repeat { every_weeks, count };
    for (every, count) in [(1, 2), (8, 52), (1, 52), (8, 2), (2, 10)] {
        assert!(validate_repeat(repeat(every, count)).is_ok());
    }
    for (every, count) in [(0, 2), (9, 2), (1, 1), (1, 0), (1, 53), (255, 255)] {
        refusal(validate_repeat(repeat(every, count)));
    }
}

#[test]
fn a_session_note_says_something_and_fits() {
    let ok = note("n1", Some("v1"), TODAY);
    assert!(validate_note(&ok).is_ok());
    let broken = |change: fn(&mut SessionNote)| {
        let mut n = note("n1", Some("v1"), TODAY);
        change(&mut n);
        refusal(validate_note(&n))
    };
    broken(|n| n.patient_id = String::new());
    broken(|n| n.date = "2026-00-10".to_owned());
    broken(|n| n.record = "  \n".to_owned());
    broken(|n| n.record = hebrew(NOTE_MAX + 1));
    broken(|n| n.personal = hebrew(NOTE_MAX + 1));

    // The personal note alone is a note, and both may be as long as allowed.
    let mut personal_only = ok.clone();
    personal_only.record = String::new();
    personal_only.personal = hebrew(NOTE_MAX);
    assert!(validate_note(&personal_only).is_ok());
    let mut full = ok;
    full.record = hebrew(NOTE_MAX);
    full.visit_id = None;
    assert!(validate_note(&full).is_ok());
}

#[test]
fn payments_and_charges_within_their_limits() {
    for amount in [1, 45_050, MAX_AMOUNT] {
        assert!(validate_payment(&payment("pay1", TODAY, amount)).is_ok());
        assert!(validate_charge(&charge("c1", TODAY, amount)).is_ok());
    }
    for amount in [0, -1, MAX_AMOUNT + 1, i32::MIN, i32::MAX] {
        refusal(validate_payment(&payment("pay1", TODAY, amount)));
        refusal(validate_charge(&charge("c1", TODAY, amount)));
    }
    let bad_payment = |change: fn(&mut Payment)| {
        let mut p = payment("pay1", TODAY, 100);
        change(&mut p);
        refusal(validate_payment(&p))
    };
    bad_payment(|p| p.patient_id = " ".to_owned());
    bad_payment(|p| p.date = "2025-02-29".to_owned());
    bad_payment(|p| p.receipt = hebrew(SHORT_MAX + 1));
    bad_payment(|p| p.payer = hebrew(SHORT_MAX + 1));
    bad_payment(|p| p.note = hebrew(LONG_MAX + 1));
    bad_payment(|p| p.void_reason = hebrew(SHORT_MAX + 1));
    bad_payment(|p| p.voided = true); // voided, and no reason given

    let bad_charge = |change: fn(&mut Charge)| {
        let mut c = charge("c1", TODAY, 100);
        change(&mut c);
        refusal(validate_charge(&c))
    };
    bad_charge(|c| c.patient_id = String::new());
    bad_charge(|c| c.date = "x".to_owned());
    bad_charge(|c| c.what = " ".to_owned());
    bad_charge(|c| c.what = hebrew(SHORT_MAX + 1));
    bad_charge(|c| c.voided = true);

    let mut voided = payment("pay1", TODAY, 100);
    voided.voided = true;
    voided.void_reason = "נרשם פעמיים".to_owned();
    assert!(validate_payment(&voided).is_ok());
    // A payment with no receipt number is allowed: the diary counts it as missing one.
    let mut no_receipt = payment("pay1", TODAY, 100);
    no_receipt.receipt = String::new();
    assert!(validate_payment(&no_receipt).is_ok());

    assert!(validate_void_reason("נרשם בטעות").is_ok());
    refusal(validate_void_reason("  "));
    refusal(validate_void_reason(&hebrew(SHORT_MAX + 1)));
}

#[test]
fn a_task_within_its_limits() {
    assert!(validate_task(&task("t1", None, None)).is_ok());
    assert!(validate_task(&task("t1", Some("p1"), Some(TODAY))).is_ok());
    let broken = |change: fn(&mut ClinicTask)| {
        let mut t = task("t1", None, Some(TODAY));
        change(&mut t);
        refusal(validate_task(&t))
    };
    broken(|t| t.title = " ".to_owned());
    broken(|t| t.title = hebrew(SHORT_MAX + 1));
    broken(|t| t.due = Some("2026-11-32".to_owned()));
    broken(|t| t.done_on = Some("אתמול".to_owned()));
    broken(|t| t.note = hebrew(LONG_MAX + 1));
}

#[test]
fn the_default_settings_are_valid_and_each_limit_holds() {
    assert!(validate_settings(&ClinicSettings::default()).is_ok());
    let broken = |change: fn(&mut ClinicSettings)| {
        let mut s = ClinicSettings::default();
        change(&mut s);
        refusal(validate_settings(&s))
    };
    broken(|s| s.day_end = s.day_start);
    broken(|s| s.day_end = DAY_MINUTES + 1);
    broken(|s| s.work_days.clear());
    broken(|s| s.work_days = vec![0, 7]);
    broken(|s| s.work_days = vec![1, 1]);
    broken(|s| {
        s.defaults.pop();
    });
    broken(|s| s.defaults[0].kind = VisitKind::Assessment); // two of one kind, none of another
    broken(|s| s.defaults.push(s.defaults[0]));
    broken(|s| s.defaults[1].minutes = MIN_MINUTES - 1);
    broken(|s| s.defaults[1].minutes = MAX_MINUTES + 1);
    broken(|s| s.defaults[1].fee = -1);
    broken(|s| s.defaults[1].fee = MAX_AMOUNT + 1);
    broken(|s| s.defaults[8].fee = 100); // the block
    broken(|s| s.places = vec!["חדר".to_owned(); MAX_PLACES + 1]);
    broken(|s| s.places = vec![" ".to_owned()]);
    broken(|s| s.places = vec![hebrew(SHORT_MAX + 1)]);
    broken(|s| s.reminder_text = hebrew(MESSAGE_MAX + 1));
    broken(|s| s.payment_text = hebrew(MESSAGE_MAX + 1));
    broken(|s| s.note_templates = vec![s.note_templates[0].clone(); MAX_TEMPLATES + 1]);
    broken(|s| s.note_templates[0].name = String::new());
    broken(|s| s.note_templates[0].name = hebrew(SHORT_MAX + 1));
    broken(|s| s.note_templates[0].text = hebrew(TEMPLATE_MAX + 1));

    let mut full = ClinicSettings {
        day_start: 0,
        ..ClinicSettings::default()
    };
    full.day_end = DAY_MINUTES;
    full.work_days = vec![0, 1, 2, 3, 4, 5, 6];
    full.places = vec!["חדר".to_owned(); MAX_PLACES];
    full.reminder_text = hebrew(MESSAGE_MAX);
    full.payment_text = String::new();
    full.note_templates = vec![
        NoteTemplate {
            name: "תבנית".to_owned(),
            text: hebrew(TEMPLATE_MAX),
        };
        MAX_TEMPLATES
    ];
    full.defaults[1].fee = MAX_AMOUNT;
    assert!(validate_settings(&full).is_ok());
    full.places.clear();
    full.note_templates.clear();
    assert!(validate_settings(&full).is_ok());
}

#[test]
fn the_calendar_shows_at_most_a_hundred_days() {
    assert!(validate_range(TODAY, TODAY).is_ok());
    assert!(validate_range("2026-01-01", "2026-04-10").is_ok()); // 100 days, both ends in
    refusal(validate_range("2026-01-01", "2026-04-11"));
    assert!(validate_range("2026-12-01", "2027-01-11").is_ok());
    refusal(validate_range("2026-11-16", TODAY));
    refusal(validate_range("x", TODAY));
    refusal(validate_range(TODAY, "2026-11-31"));
    refusal(validate_range("2000-01-01", "2200-12-31"));
}

#[test]
fn today_and_the_year_are_checked() {
    assert!(validate_today(&today()).is_ok());
    assert!(validate_today(&Today {
        date: TODAY.to_owned(),
        minute: 1_439
    })
    .is_ok());
    refusal(validate_today(&Today {
        date: TODAY.to_owned(),
        minute: 1_440,
    }));
    refusal(validate_today(&Today {
        date: "היום".to_owned(),
        minute: 0,
    }));
    assert!(validate_year(2000).is_ok());
    assert!(validate_year(2200).is_ok());
    refusal(validate_year(1999));
    refusal(validate_year(2201));
}

#[test]
fn only_a_planned_visit_is_deleted() {
    let with = |status| visit("v1", VisitKind::Therapy, TODAY, 600, status);
    assert!(validate_delete(&with(VisitStatus::Planned)).is_ok());
    for status in [
        VisitStatus::Held,
        VisitStatus::Cancelled,
        VisitStatus::NoShow,
    ] {
        refusal(validate_delete(&with(status)));
    }
}

// ------------------------------------------------------------------ series

#[test]
fn a_series_runs_in_whole_weeks_across_a_year_end() {
    let dates = |first: &str, every_weeks, count| {
        series_dates(first, Repeat { every_weeks, count }).unwrap()
    };
    assert_eq!(
        dates("2026-12-24", 1, 3),
        ["2026-12-24", "2026-12-31", "2027-01-07"]
    );
    assert_eq!(dates("2024-02-15", 2, 2), ["2024-02-15", "2024-02-29"]);
    assert_eq!(
        dates("2026-11-15", 8, 3),
        ["2026-11-15", "2027-01-10", "2027-03-07"]
    );
    let year = dates(TODAY, 1, 52);
    assert_eq!(year.len(), 52);
    assert_eq!(year.last().map(String::as_str), Some("2027-11-07"));
    assert!(year.iter().all(|d| weekday(d) == Some(0)));
}

#[test]
fn a_series_is_refused_when_it_cannot_be() {
    let repeat = Repeat {
        every_weeks: 1,
        count: 2,
    };
    refusal(series_dates("2026-02-29", repeat));
    refusal(series_dates("", repeat));
    refusal(series_dates(
        TODAY,
        Repeat {
            every_weeks: 0,
            count: 2,
        },
    ));
    refusal(series_dates(
        TODAY,
        Repeat {
            every_weeks: 1,
            count: 1,
        },
    ));
    // The last day a date may be: the second visit would fall outside.
    refusal(series_dates("2200-12-31", repeat));
    assert!(series_dates("2200-12-24", repeat).is_ok());
}

fn series_visit(id: &str, date: &str, status: VisitStatus) -> Visit {
    Visit {
        series_id: Some("s1".to_owned()),
        fee: 40_000,
        place: "הקליניקה".to_owned(),
        ..visit(id, VisitKind::Therapy, date, 600, status)
    }
}

fn weekly_series() -> Vec<Visit> {
    vec![
        series_visit("v1", "2026-11-01", VisitStatus::Held),
        series_visit("v2", "2026-11-08", VisitStatus::Planned),
        series_visit("v3", "2026-11-15", VisitStatus::Held),
        series_visit("v4", "2026-11-22", VisitStatus::Planned),
        series_visit("v5", "2026-11-29", VisitStatus::Cancelled),
        series_visit("v6", "2026-12-06", VisitStatus::Planned),
    ]
}

#[test]
fn following_changes_the_later_planned_visits_and_moves_them_alike() {
    let mut series = weekly_series();
    series[3].status_note = "נשאר כמו שהיה".to_owned();
    series[3].charge_missed = true;
    // A visit of another series and one without a series are not this series.
    series.push(Visit {
        series_id: Some("s2".to_owned()),
        ..series_visit("other", "2026-11-23", VisitStatus::Planned)
    });
    series.push(visit(
        "single",
        VisitKind::Therapy,
        "2026-11-24",
        600,
        VisitStatus::Planned,
    ));
    let previous = series[1].clone();
    let edited = Visit {
        kind: VisitKind::Guidance,
        title: "הדרכה".to_owned(),
        date: "2026-11-10".to_owned(), // two days later
        start: 660,
        minutes: 45,
        place: "זום".to_owned(),
        fee: 35_000,
        note: "להביא את השאלונים".to_owned(),
        status: VisitStatus::Held, // a status change is for this one only
        status_note: "התקיימה".to_owned(),
        charge_missed: false,
        ..previous.clone()
    };

    let later = apply_following(&edited, &previous, &series).unwrap();
    let ids: Vec<&str> = later.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["v4", "v6"]); // not v1 (earlier), v3 (held), v5 (cancelled), v2 itself
    assert_eq!(later[0].date, "2026-11-24");
    assert_eq!(later[1].date, "2026-12-08");
    for v in &later {
        assert_eq!(v.kind, VisitKind::Guidance);
        assert_eq!(v.title, "הדרכה");
        assert_eq!((v.start, v.minutes), (660, 45));
        assert_eq!(v.place, "זום");
        assert_eq!(v.fee, 35_000);
        assert_eq!(v.note, "להביא את השאלונים");
        assert_eq!(v.status, VisitStatus::Planned);
        assert_eq!(v.series_id.as_deref(), Some("s1"));
        assert_eq!(v.patient_id.as_deref(), Some("p1"));
    }
    assert_eq!(later[0].status_note, "נשאר כמו שהיה");
    assert!(later[0].charge_missed);
    assert_eq!(later[1].status_note, "");
}

#[test]
fn following_moves_backwards_and_across_a_year_end() {
    let series = vec![
        series_visit("a", "2026-12-28", VisitStatus::Planned),
        series_visit("b", "2027-01-04", VisitStatus::Planned),
        series_visit("c", "2027-01-11", VisitStatus::Planned),
    ];
    let previous = series[1].clone();
    let edited = Visit {
        date: "2026-12-30".to_owned(), // five days earlier
        ..previous.clone()
    };
    let later = apply_following(&edited, &previous, &series).unwrap();
    assert_eq!(later.len(), 1);
    assert_eq!(
        (later[0].id.as_str(), later[0].date.as_str()),
        ("c", "2027-01-06")
    );

    // Nothing moved and nothing changed: the later ones come back as they are.
    let same = apply_following(&previous, &previous, &series).unwrap();
    assert_eq!(same, [series[2].clone()]);
}

#[test]
fn following_on_a_single_visit_touches_nothing() {
    let lone = visit("v1", VisitKind::Therapy, TODAY, 600, VisitStatus::Planned);
    let edited = Visit {
        start: 700,
        ..lone.clone()
    };
    assert_eq!(
        apply_following(&edited, &lone, &weekly_series()).unwrap(),
        []
    );
}

#[test]
fn following_is_refused_when_a_later_visit_cannot_be_saved() {
    let series = vec![
        series_visit("a", "2200-12-17", VisitStatus::Planned),
        series_visit("b", "2200-12-24", VisitStatus::Planned),
    ];
    let previous = series[0].clone();
    let pushed_out = Visit {
        date: "2200-12-27".to_owned(),
        ..previous.clone()
    };
    refusal(apply_following(&pushed_out, &previous, &series));

    let bad_date = Visit {
        date: "מחר".to_owned(),
        ..previous.clone()
    };
    refusal(apply_following(&bad_date, &previous, &series));

    // The later visits keep their patient, and a block cannot have one.
    let to_block = Visit {
        kind: VisitKind::Block,
        patient_id: None,
        fee: 0,
        ..previous.clone()
    };
    refusal(apply_following(&to_block, &previous, &series));

    let too_long = Visit {
        start: 1_400,
        ..previous.clone()
    };
    refusal(apply_following(&too_long, &previous, &series));
}

// ------------------------------------------------------------------ overlaps

#[test]
fn visits_overlap_only_when_their_times_intersect() {
    let at = |id: &str, start: u16, minutes: u16| Visit {
        minutes,
        ..visit(id, VisitKind::Therapy, TODAY, start, VisitStatus::Planned)
    };
    let nine = at("a", 540, 60);
    assert!(visits_overlap(&nine, &at("b", 570, 60)));
    assert!(visits_overlap(&nine, &at("b", 550, 10))); // inside
    assert!(visits_overlap(&nine, &at("b", 500, 200))); // around
    assert!(visits_overlap(&nine, &at("b", 540, 60))); // the same time
    assert!(visits_overlap(&nine, &at("b", 599, 5)));
    // Touching end to start is not an overlap, either way round.
    assert!(!visits_overlap(&nine, &at("b", 600, 60)));
    assert!(!visits_overlap(&at("b", 600, 60), &nine));
    assert!(!visits_overlap(&nine, &at("b", 480, 60)));

    let mut other_day = at("b", 540, 60);
    other_day.date = "2026-11-16".to_owned();
    assert!(!visits_overlap(&nine, &other_day));
    let mut cancelled = at("b", 540, 60);
    cancelled.status = VisitStatus::Cancelled;
    assert!(!visits_overlap(&nine, &cancelled));
    assert!(!visits_overlap(&cancelled, &nine));
    // A block and a missed session still take the time.
    let mut missed = at("b", 540, 60);
    missed.status = VisitStatus::NoShow;
    assert!(visits_overlap(&nine, &missed));
    let block = visit("blk", VisitKind::Block, TODAY, 560, VisitStatus::Planned);
    assert!(visits_overlap(&nine, &block));
}

#[test]
fn the_overlapping_ids_of_a_set() {
    let at = |id: &str, date: &str, start: u16, minutes: u16, status| Visit {
        minutes,
        ..visit(id, VisitKind::Therapy, date, start, status)
    };
    let planned = VisitStatus::Planned;
    let visits = vec![
        at("long", TODAY, 480, 240, planned), // 08:00–12:00
        at("in1", TODAY, 500, 30, planned),
        at("in2", TODAY, 660, 30, planned),
        at("touch", TODAY, 720, 60, planned), // starts when "long" ends
        at("gone", TODAY, 730, 30, VisitStatus::Cancelled),
        at("alone", TODAY, 900, 60, planned),
        at("next-day", "2026-11-16", 480, 240, planned),
    ];
    let ids: Vec<String> = overlapping_ids(&visits).into_iter().collect();
    assert_eq!(ids, ["in1", "in2", "long"]);
    assert!(overlapping_ids(&[]).is_empty());
    assert!(overlapping_ids(&visits[5..]).is_empty());
}

// ------------------------------------------------------------------ money

#[test]
fn what_a_visit_costs() {
    let with = |status, charge_missed| Visit {
        charge_missed,
        ..paid_visit("v1", TODAY, status, 45_000)
    };
    assert_eq!(visit_charge(&with(VisitStatus::Held, false)), 45_000);
    assert_eq!(visit_charge(&with(VisitStatus::Held, true)), 45_000);
    assert_eq!(visit_charge(&with(VisitStatus::Planned, false)), 0);
    assert_eq!(visit_charge(&with(VisitStatus::Planned, true)), 0);
    assert_eq!(visit_charge(&with(VisitStatus::Cancelled, false)), 0);
    assert_eq!(visit_charge(&with(VisitStatus::Cancelled, true)), 45_000);
    assert_eq!(visit_charge(&with(VisitStatus::NoShow, false)), 0);
    assert_eq!(visit_charge(&with(VisitStatus::NoShow, true)), 45_000);
    assert_eq!(
        visit_charge(&paid_visit("v1", TODAY, VisitStatus::Held, 0)),
        0
    );
}

#[test]
fn the_balance_counts_what_is_not_voided() {
    let visits = vec![
        paid_visit("v1", "2026-10-01", VisitStatus::Held, 40_000),
        paid_visit("v2", "2026-10-08", VisitStatus::Cancelled, 40_000),
        Visit {
            charge_missed: true,
            ..paid_visit("v3", "2026-10-15", VisitStatus::NoShow, 40_000)
        },
        paid_visit("v4", "2026-12-01", VisitStatus::Planned, 40_000),
    ];
    let charges = vec![
        charge("c1", "2026-10-01", 250_000),
        Charge {
            voided: true,
            void_reason: "נרשם פעמיים".to_owned(),
            ..charge("c2", "2026-10-01", 250_000)
        },
    ];
    let payments = vec![
        payment("pay1", "2026-10-02", 100_000),
        Payment {
            voided: true,
            void_reason: "הצ'ק חזר".to_owned(),
            ..payment("pay2", "2026-10-03", 999_000)
        },
    ];
    assert_eq!(
        balance(&visits, &charges, &payments).unwrap(),
        Balance {
            charged: 330_000,
            paid: 100_000,
            due: 230_000
        }
    );
    assert_eq!(balance(&[], &[], &[]).unwrap(), Balance::default());
}

#[test]
fn a_family_that_paid_ahead_has_a_negative_due() {
    let b = balance(
        &[paid_visit("v1", TODAY, VisitStatus::Planned, 40_000)],
        &[],
        &[payment("pay1", TODAY, 40_000)],
    )
    .unwrap();
    assert_eq!(
        b,
        Balance {
            charged: 0,
            paid: 40_000,
            due: -40_000
        }
    );
}

#[test]
fn a_sum_that_does_not_fit_is_refused_not_wrapped() {
    // 215 × 100,000 ₪ is more than an i32 of agorot holds.
    let charges: Vec<Charge> = (0..215)
        .map(|n| charge(&format!("c{n}"), TODAY, MAX_AMOUNT))
        .collect();
    let payments: Vec<Payment> = (0..215)
        .map(|n| payment(&format!("pay{n}"), TODAY, MAX_AMOUNT))
        .collect();
    refusal(balance(&[], &charges, &[]));
    refusal(balance(&[], &[], &payments));
    assert!(balance(&[], &charges[..214], &payments[..214]).is_ok());
    refusal(month_received(&payments, TODAY));
    let names = HashMap::new();
    refusal(diary_month(
        "2026-11",
        &[],
        &charges,
        &[],
        &names,
        &HashSet::new(),
    ));
    refusal(income_year(2026, &[], &[], &payments, &[]));
    let owed = Balance {
        charged: i32::MAX,
        paid: 0,
        due: i32::MAX,
    };
    refusal(total_due(&[owed, owed]));
}

#[test]
fn the_oldest_charges_are_paid_first() {
    let visits = vec![paid_visit("v1", "2026-10-10", VisitStatus::Held, 40_000)];
    let charges = vec![
        charge("c2", "2026-11-01", 40_000),
        charge("c1", "2026-10-01", 40_000),
        Charge {
            voided: true,
            void_reason: "טעות".to_owned(),
            ..charge("c0", "2026-09-01", 40_000)
        },
    ];
    let oldest = |paid: &[Agorot]| {
        let payments: Vec<Payment> = paid
            .iter()
            .enumerate()
            .map(|(n, amount)| payment(&format!("pay{n}"), "2026-11-10", *amount))
            .collect();
        oldest_uncovered(&visits, &charges, &payments)
    };
    assert_eq!(oldest(&[]).as_deref(), Some("2026-10-01"));
    assert_eq!(oldest(&[39_999]).as_deref(), Some("2026-10-01"));
    assert_eq!(oldest(&[40_000]).as_deref(), Some("2026-10-10"));
    assert_eq!(oldest(&[25_000, 25_000]).as_deref(), Some("2026-10-10"));
    assert_eq!(oldest(&[80_000]).as_deref(), Some("2026-11-01"));
    assert_eq!(oldest(&[119_999]).as_deref(), Some("2026-11-01"));
    assert_eq!(oldest(&[120_000]), None);
    assert_eq!(oldest(&[500_000]), None);

    // A voided payment covers nothing.
    let voided = Payment {
        voided: true,
        void_reason: "טעות".to_owned(),
        ..payment("pay", "2026-11-10", 500_000)
    };
    assert_eq!(
        oldest_uncovered(&visits, &charges, &[voided]).as_deref(),
        Some("2026-10-01")
    );
    assert_eq!(oldest_uncovered(&[], &[], &[]), None);
    // A visit that costs nothing is never "uncovered".
    let free = [visit(
        "v",
        VisitKind::Intake,
        "2026-01-01",
        600,
        VisitStatus::Held,
    )];
    assert_eq!(oldest_uncovered(&free, &[], &[]), None);
}

#[test]
fn money_as_she_reads_it() {
    assert_eq!(money(45_000), "450");
    assert_eq!(money(45_050), "450.50");
    assert_eq!(money(45_005), "450.05");
    assert_eq!(money(5), "0.05");
    assert_eq!(money(0), "0");
    assert_eq!(money(MAX_AMOUNT), "100000");
    assert_eq!(money(-45_050), "-450.50");
    assert_eq!(money(-100), "-1");
    assert_eq!(money(i32::MAX), "21474836.47");
    assert_eq!(money(i32::MIN), "-21474836.48");
}

#[test]
fn the_clinic_totals() {
    let due = |due| Balance {
        charged: 0,
        paid: 0,
        due,
    };
    assert_eq!(
        total_due(&[due(30_000), due(-50_000), due(0), due(1)]).unwrap(),
        30_001
    );
    assert_eq!(total_due(&[]).unwrap(), 0);

    let payments = vec![
        payment("a", "2026-11-01", 10_000),
        payment("b", "2026-11-30", 20_000),
        payment("c", "2026-10-31", 40_000),
        payment("d", "2026-12-01", 80_000),
        payment("e", "2025-11-15", 160_000),
        Payment {
            voided: true,
            void_reason: "טעות".to_owned(),
            ..payment("f", "2026-11-15", 320_000)
        },
    ];
    assert_eq!(month_received(&payments, TODAY).unwrap(), 30_000);
    assert_eq!(month_received(&payments, "2026-12-31").unwrap(), 80_000);
    assert_eq!(month_received(&payments, "2027-01-01").unwrap(), 0);
    refusal(month_received(&payments, "2026-11"));
}

// ------------------------------------------------------------------ labels

#[test]
fn every_label_is_hebrew_and_its_own() {
    fn check(labels: &[&str]) {
        let distinct: BTreeSet<&str> = labels.iter().copied().collect();
        assert_eq!(distinct.len(), labels.len(), "{labels:?}");
        for label in labels {
            assert_hebrew_sentence(label);
        }
    }
    check(&VISIT_KINDS.map(kind_label));
    check(&PAY_METHODS.map(method_label));
    check(
        &[
            VisitStatus::Planned,
            VisitStatus::Held,
            VisitStatus::Cancelled,
            VisitStatus::NoShow,
        ]
        .map(status_label),
    );
    check(
        &[
            TaskKind::Call,
            TaskKind::Questionnaire,
            TaskKind::Document,
            TaskKind::Report,
            TaskKind::Payment,
            TaskKind::FollowUp,
            TaskKind::Other,
        ]
        .map(task_kind_label),
    );
    check(
        &[
            PatientStatus::Waiting,
            PatientStatus::Active,
            PatientStatus::Closed,
        ]
        .map(patient_status_label),
    );
    check(
        &[
            ContactRole::Mother,
            ContactRole::Father,
            ContactRole::Guardian,
            ContactRole::Other,
        ]
        .map(contact_role_label),
    );
    assert_eq!(kind_label(VisitKind::Assessment), "מפגש אבחון");
}

#[test]
fn a_visit_is_called_by_her_words_or_by_its_kind() {
    let mut v = visit("v1", VisitKind::Feedback, TODAY, 600, VisitStatus::Planned);
    assert_eq!(visit_title(&v), "שיחת משוב");
    v.title = "  ".to_owned();
    assert_eq!(visit_title(&v), "שיחת משוב");
    v.title = " משוב להורים ".to_owned();
    assert_eq!(visit_title(&v), "משוב להורים");
    assert_eq!(
        note_case_title(VisitKind::Intake, TODAY),
        "סיכום פגישת היכרות · 15.11.2026"
    );
}

// ------------------------------------------------------------------ alerts

/// One patient's world, for the alert tests.
#[derive(Default)]
struct World {
    patient: Option<Patient>,
    visits: Vec<Visit>,
    notes: Vec<SessionNote>,
    charges: Vec<Charge>,
    payments: Vec<Payment>,
    tasks: Vec<ClinicTask>,
    report: Option<(u32, u32)>,
    settings: Option<ClinicSettings>,
}

impl World {
    fn alerts_at(&self, now: &Today) -> Vec<Alert> {
        let patient = self
            .patient
            .clone()
            .unwrap_or_else(|| patient("p1", "נועם"));
        let settings = self.settings.clone().unwrap_or_default();
        let records = PatientRecords {
            patient: &patient,
            visits: &self.visits,
            notes: &self.notes,
            charges: &self.charges,
            payments: &self.payments,
            tasks: &self.tasks,
            report: self.report,
        };
        let alerts = patient_alerts(&records, now, &settings);
        for alert in &alerts {
            assert_hebrew_sentence(&alert.text);
            assert!(!alert.text.contains(&patient.name), "{}", alert.text);
            assert_eq!(alert.patient_id.as_deref(), Some(patient.id.as_str()));
            assert_eq!(alert.patient_name.as_deref(), Some(patient.name.as_str()));
        }
        // The clinic as a whole sees the same thing for a clinic of one.
        let mut reports = HashMap::new();
        if let Some(report) = self.report {
            reports.insert(patient.id.clone(), report);
        }
        let clinic = ClinicRecords {
            patients: std::slice::from_ref(&patient),
            visits: &self.visits,
            notes: &self.notes,
            charges: &self.charges,
            payments: &self.payments,
            tasks: &self.tasks,
            reports: &reports,
        };
        assert_eq!(clinic_alerts(&clinic, now, &settings), alerts);
        alerts
    }

    fn alerts(&self) -> Vec<Alert> {
        self.alerts_at(&today())
    }

    fn of_kind(&self, kind: AlertKind) -> Vec<Alert> {
        self.alerts()
            .into_iter()
            .filter(|a| a.kind == kind)
            .collect()
    }

    fn fires(&self, kind: AlertKind) -> bool {
        !self.of_kind(kind).is_empty()
    }
}

fn with_visits(visits: Vec<Visit>) -> World {
    World {
        visits,
        ..World::default()
    }
}

#[test]
fn a_quiet_patient_has_no_alerts() {
    assert_eq!(World::default().alerts(), []);
    let calm = World {
        visits: vec![
            visit(
                "v1",
                VisitKind::Intake,
                "2026-11-10",
                600,
                VisitStatus::Held,
            ),
            visit(
                "v2",
                VisitKind::Assessment,
                "2026-11-17",
                600,
                VisitStatus::Planned,
            ),
        ],
        notes: vec![note("n1", Some("v1"), "2026-11-10")],
        ..World::default()
    };
    assert_eq!(calm.alerts(), []);
    // A date that is not one gives no alerts rather than wrong ones.
    let nowhere = Today {
        date: "היום".to_owned(),
        minute: 0,
    };
    assert_eq!(calm.alerts_at(&nowhere), []);
}

#[test]
fn unmarked_fires_once_the_planned_visit_has_ended() {
    let planned = |date: &str, start| {
        with_visits(vec![visit(
            "v1",
            VisitKind::Therapy,
            date,
            start,
            VisitStatus::Planned,
        )])
    };
    let yesterday = planned("2026-11-14", 600);
    let fired = yesterday.of_kind(AlertKind::Unmarked);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].visit_id.as_deref(), Some("v1"));
    assert_eq!(fired[0].since, "2026-11-14");
    assert!(fired[0].text.contains("14.11.2026"));
    assert!(fired[0].text.contains("10:00"));

    // Today 10:00–11:00: not at 10:59, yes from 11:00.
    let this_morning = planned(TODAY, 600);
    let at = |minute| Today {
        date: TODAY.to_owned(),
        minute,
    };
    let unmarked = |alerts: Vec<Alert>| alerts.iter().any(|a| a.kind == AlertKind::Unmarked);
    assert!(!unmarked(this_morning.alerts_at(&at(659))));
    assert!(unmarked(this_morning.alerts_at(&at(660))));
    assert!(!planned("2026-11-16", 0).fires(AlertKind::Unmarked));

    for status in [
        VisitStatus::Held,
        VisitStatus::Cancelled,
        VisitStatus::NoShow,
    ] {
        let marked = with_visits(vec![visit(
            "v1",
            VisitKind::Meeting,
            "2026-11-14",
            600,
            status,
        )]);
        assert!(!marked.fires(AlertKind::Unmarked));
    }
}

#[test]
fn a_block_is_never_unmarked() {
    let p = patient("p1", "נועם");
    let block = visit(
        "b1",
        VisitKind::Block,
        "2026-11-01",
        600,
        VisitStatus::Planned,
    );
    let reports = HashMap::new();
    let clinic = ClinicRecords {
        patients: std::slice::from_ref(&p),
        visits: std::slice::from_ref(&block),
        notes: &[],
        charges: &[],
        payments: &[],
        tasks: &[],
        reports: &reports,
    };
    assert_eq!(
        clinic_alerts(&clinic, &today(), &ClinicSettings::default()),
        []
    );
}

#[test]
fn no_note_fires_for_a_held_session_of_the_last_sixty_days() {
    let held =
        |kind, date: &str| with_visits(vec![visit("v1", kind, date, 600, VisitStatus::Held)]);
    let fired = held(VisitKind::Assessment, "2026-11-10").of_kind(AlertKind::NoNote);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].visit_id.as_deref(), Some("v1"));
    assert_eq!(fired[0].since, "2026-11-10");
    assert_eq!(
        fired[0].text,
        "עדיין אין סיכום למפגש אבחון מיום 10.11.2026."
    );

    for kind in [
        VisitKind::Intake,
        VisitKind::Assessment,
        VisitKind::Observation,
        VisitKind::Feedback,
        VisitKind::Therapy,
        VisitKind::Guidance,
    ] {
        assert!(held(kind, TODAY).fires(AlertKind::NoNote), "{kind:?}");
        assert!(held(kind, "2026-09-16").fires(AlertKind::NoNote)); // 60 days ago
        assert!(!held(kind, "2026-09-15").fires(AlertKind::NoNote)); // 61 days ago
    }
    for kind in [VisitKind::Meeting, VisitKind::Other] {
        assert!(
            !held(kind, "2026-11-10").fires(AlertKind::NoNote),
            "{kind:?}"
        );
    }
    for status in [
        VisitStatus::Planned,
        VisitStatus::Cancelled,
        VisitStatus::NoShow,
    ] {
        let not_held = with_visits(vec![visit(
            "v1",
            VisitKind::Assessment,
            "2026-11-10",
            600,
            status,
        )]);
        assert!(!not_held.fires(AlertKind::NoNote));
    }

    let mut documented = held(VisitKind::Assessment, "2026-11-10");
    documented.notes = vec![
        note("n1", Some("other"), "2026-11-10"),
        note("n2", None, "2026-11-10"),
    ];
    assert!(documented.fires(AlertKind::NoNote)); // notes of something else
    documented.notes.push(note("n3", Some("v1"), "2026-11-11"));
    assert!(!documented.fires(AlertKind::NoNote));
}

#[test]
fn debt_fires_when_the_oldest_uncovered_charge_is_old_enough() {
    let owing = |date: &str| World {
        charges: vec![charge("c1", date, 40_000)],
        ..World::default()
    };
    let fired = owing("2026-10-16").of_kind(AlertKind::Debt); // 30 days
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].since, "2026-10-16");
    assert_eq!(fired[0].text, "יתרה פתוחה של 400 ₪, מאז 16.10.2026.");
    assert_eq!((&fired[0].visit_id, &fired[0].task_id), (&None, &None));
    assert!(!owing("2026-10-17").fires(AlertKind::Debt)); // 29 days

    // A held session that was not paid is a debt too; a planned one is not.
    let session = |status| with_visits(vec![paid_visit("v1", "2026-10-01", status, 40_000)]);
    assert!(session(VisitStatus::Held).fires(AlertKind::Debt));
    assert!(!session(VisitStatus::Planned).fires(AlertKind::Debt));
    assert!(!session(VisitStatus::Cancelled).fires(AlertKind::Debt));

    let mut paid = owing("2026-09-01");
    paid.payments = vec![payment("pay1", "2026-11-01", 40_000)];
    assert!(!paid.fires(AlertKind::Debt));
    paid.payments[0].amount = 50_000; // paid ahead
    assert!(!paid.fires(AlertKind::Debt));
    paid.payments[0].amount = 39_950;
    let partly = paid.of_kind(AlertKind::Debt);
    assert_eq!(partly[0].text, "יתרה פתוחה של 0.50 ₪, מאז 01.09.2026.");
    paid.payments[0].voided = true;
    assert!(paid.fires(AlertKind::Debt));

    let mut voided = owing("2026-09-01");
    voided.charges[0].voided = true;
    assert!(!voided.fires(AlertKind::Debt));
}

#[test]
fn debt_pays_the_oldest_first_so_a_new_charge_is_not_an_old_debt() {
    let mut world = World {
        charges: vec![
            charge("old", "2026-08-01", 40_000),
            charge("new", "2026-11-01", 40_000),
        ],
        // Paid after the new charge was written, and still it covers the old one.
        payments: vec![payment("pay1", "2026-11-05", 40_000)],
        ..World::default()
    };
    assert!(!world.fires(AlertKind::Debt));
    world.payments[0].amount = 30_000;
    let fired = world.of_kind(AlertKind::Debt);
    assert_eq!(fired[0].since, "2026-08-01");
    assert_eq!(fired[0].text, "יתרה פתוחה של 500 ₪, מאז 01.08.2026.");

    // Her own setting decides how old is old.
    let mut patient_owes = World {
        charges: vec![charge("c1", "2026-11-08", 40_000)],
        ..World::default()
    };
    assert!(!patient_owes.fires(AlertKind::Debt));
    patient_owes.settings = Some(ClinicSettings {
        debt_days: 7,
        ..ClinicSettings::default()
    });
    assert!(patient_owes.fires(AlertKind::Debt));
}

#[test]
fn waiting_fires_after_the_weeks_she_set() {
    let waiting = |since: Option<&str>| World {
        patient: Some(Patient {
            status: PatientStatus::Waiting,
            waiting_since: since.map(str::to_owned),
            created_on: "2026-09-06".to_owned(),
            ..patient("p1", "תמר")
        }),
        ..World::default()
    };
    let fired = waiting(Some("2026-10-04")).of_kind(AlertKind::Waiting); // 42 days
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].since, "2026-10-04");
    assert_eq!(fired[0].text, "ברשימת ההמתנה כבר 6 שבועות, מאז 04.10.2026.");
    assert!(!waiting(Some("2026-10-05")).fires(AlertKind::Waiting)); // 41 days

    // Without the day it joined the list, the day the card was opened counts.
    let by_card = waiting(None).of_kind(AlertKind::Waiting);
    assert_eq!(by_card[0].since, "2026-09-06");
    assert!(by_card[0].text.contains("10 שבועות"));
    assert_eq!(
        waiting(Some("לא תאריך")).of_kind(AlertKind::Waiting)[0].since,
        "2026-09-06"
    );

    for status in [PatientStatus::Active, PatientStatus::Closed] {
        let mut not_waiting = waiting(Some("2026-01-01"));
        if let Some(p) = &mut not_waiting.patient {
            p.status = status;
        }
        assert!(!not_waiting.fires(AlertKind::Waiting));
    }

    let words = |weeks: u16, since: &str| {
        let mut world = waiting(Some(since));
        world.settings = Some(ClinicSettings {
            waiting_weeks: weeks,
            ..ClinicSettings::default()
        });
        world.of_kind(AlertKind::Waiting)[0].text.clone()
    };
    assert_eq!(
        words(1, "2026-11-08"),
        "ברשימת ההמתנה כבר שבוע, מאז 08.11.2026."
    );
    assert_eq!(
        words(2, "2026-11-01"),
        "ברשימת ההמתנה כבר שבועיים, מאז 01.11.2026."
    );
    assert_eq!(words(0, TODAY), "ברשימת ההמתנה מאז 15.11.2026.");
}

fn assessed(last: &str) -> World {
    with_visits(vec![
        visit(
            "a1",
            VisitKind::Assessment,
            "2026-10-01",
            600,
            VisitStatus::Held,
        ),
        visit("a2", VisitKind::Assessment, last, 600, VisitStatus::Held),
    ])
}

#[test]
fn report_due_fires_when_the_testing_ended_and_the_report_is_late() {
    // 21 days after 24 October is 14 November: before today.
    let late = assessed("2026-10-24");
    let fired = late.of_kind(AlertKind::ReportDue);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].since, "2026-10-24");
    assert_eq!(
        fired[0].text,
        "מפגש האבחון האחרון היה ביום 24.10.2026, ועדיין לא נפתח תיק לדוח."
    );
    // 21 days after 25 October is today: not before today.
    assert!(!assessed("2026-10-25").fires(AlertKind::ReportDue));

    let mut writing = assessed("2026-10-24");
    writing.report = Some((9, 16));
    assert_eq!(
        writing.of_kind(AlertKind::ReportDue)[0].text,
        "מפגש האבחון האחרון היה ביום 24.10.2026, והדוח עדיין לא גמור (9 מתוך 16 סעיפים)."
    );
    writing.report = Some((16, 16));
    assert!(!writing.fires(AlertKind::ReportDue));
    writing.report = Some((0, 0)); // a case with no sections is not a finished report
    assert!(writing.fires(AlertKind::ReportDue));
}

#[test]
fn report_due_does_not_fire_while_the_work_goes_on_or_is_over() {
    let mut more_ahead = assessed("2026-10-24");
    more_ahead.visits.push(visit(
        "a3",
        VisitKind::Assessment,
        TODAY,
        900,
        VisitStatus::Planned,
    ));
    assert!(!more_ahead.fires(AlertKind::ReportDue));
    // One that was planned for the past and never marked is not "ahead".
    more_ahead.visits[2].date = "2026-11-14".to_owned();
    assert!(more_ahead.fires(AlertKind::ReportDue));
    // Another kind of visit ahead does not hold the report back.
    more_ahead.visits[2] = visit(
        "t1",
        VisitKind::Guidance,
        "2026-11-20",
        900,
        VisitStatus::Planned,
    );
    assert!(more_ahead.fires(AlertKind::ReportDue));

    let mut given = assessed("2026-10-24");
    given.visits.push(visit(
        "f1",
        VisitKind::Feedback,
        "2026-11-10",
        600,
        VisitStatus::Held,
    ));
    assert!(!given.fires(AlertKind::ReportDue));
    given.visits[2].status = VisitStatus::Planned; // planned is not held
    assert!(given.fires(AlertKind::ReportDue));

    for status in [PatientStatus::Waiting, PatientStatus::Closed] {
        let mut world = assessed("2026-10-24");
        world.patient = Some(Patient {
            status,
            ..patient("p1", "נועם")
        });
        assert!(!world.fires(AlertKind::ReportDue));
    }
    // No testing session was held at all.
    let untested = with_visits(vec![visit(
        "i1",
        VisitKind::Intake,
        "2026-09-01",
        600,
        VisitStatus::Held,
    )]);
    assert!(!untested.fires(AlertKind::ReportDue));

    let mut patient_setting = assessed("2026-10-24");
    patient_setting.settings = Some(ClinicSettings {
        report_days: 30,
        ..ClinicSettings::default()
    });
    assert!(!patient_setting.fires(AlertKind::ReportDue));
}

#[test]
fn feedback_soon_fires_in_the_week_before_while_the_report_is_open() {
    let feedback = |date: &str, start, report| World {
        visits: vec![visit(
            "f1",
            VisitKind::Feedback,
            date,
            start,
            VisitStatus::Planned,
        )],
        report,
        ..World::default()
    };
    let fired = feedback("2026-11-22", 600, Some((9, 16))).of_kind(AlertKind::FeedbackSoon);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].visit_id.as_deref(), Some("f1"));
    assert_eq!(fired[0].since, "2026-11-22");
    assert_eq!(
        fired[0].text,
        "שיחת המשוב נקבעה ליום 22.11.2026 בשעה 10:00, והדוח עדיין לא גמור (9 מתוך 16 סעיפים)."
    );
    assert!(feedback(TODAY, 900, None).fires(AlertKind::FeedbackSoon));
    assert!(
        feedback("2026-11-22", 600, None).of_kind(AlertKind::FeedbackSoon)[0]
            .text
            .ends_with("ועדיין לא נפתח תיק לדוח.")
    );
    assert!(!feedback("2026-11-23", 600, None).fires(AlertKind::FeedbackSoon)); // 8 days
    assert!(!feedback("2026-11-14", 600, None).fires(AlertKind::FeedbackSoon)); // passed
    assert!(!feedback("2026-11-22", 600, Some((16, 16))).fires(AlertKind::FeedbackSoon));

    for status in [
        VisitStatus::Held,
        VisitStatus::Cancelled,
        VisitStatus::NoShow,
    ] {
        let mut world = feedback("2026-11-16", 600, None);
        world.visits[0].status = status;
        assert!(!world.fires(AlertKind::FeedbackSoon));
    }
    let mut other_kind = feedback("2026-11-16", 600, None);
    other_kind.visits[0].kind = VisitKind::Guidance;
    assert!(!other_kind.fires(AlertKind::FeedbackSoon));
}

#[test]
fn no_next_visit_fires_for_an_active_patient_left_with_nothing_ahead() {
    let last_on = |date: &str| {
        with_visits(vec![
            visit(
                "v0",
                VisitKind::Therapy,
                "2026-09-01",
                600,
                VisitStatus::Held,
            ),
            visit("v1", VisitKind::Therapy, date, 600, VisitStatus::Held),
        ])
    };
    let fired = last_on("2026-10-31").of_kind(AlertKind::NoNextVisit); // 15 days
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].since, "2026-10-31");
    assert_eq!(
        fired[0].text,
        "הפגישה האחרונה הייתה ביום 31.10.2026, ואין פגישה הבאה ביומן."
    );
    assert!(!last_on("2026-11-01").fires(AlertKind::NoNextVisit)); // 14 days
    assert!(!last_on(TODAY).fires(AlertKind::NoNextVisit));
    assert!(!World::default().fires(AlertKind::NoNextVisit)); // never came

    let mut booked = last_on("2026-10-31");
    booked.visits.push(visit(
        "v2",
        VisitKind::Therapy,
        "2027-01-10",
        600,
        VisitStatus::Planned,
    ));
    assert!(!booked.fires(AlertKind::NoNextVisit));
    booked.visits[2].status = VisitStatus::Cancelled; // a cancelled one is not ahead
    assert!(booked.fires(AlertKind::NoNextVisit));

    let mut has_task = last_on("2026-10-31");
    has_task.tasks = vec![task("t1", Some("p1"), None)];
    assert!(!has_task.fires(AlertKind::NoNextVisit));
    has_task.tasks[0].done_on = Some("2026-11-01".to_owned());
    assert!(has_task.fires(AlertKind::NoNextVisit));

    for status in [PatientStatus::Waiting, PatientStatus::Closed] {
        let mut world = last_on("2026-10-31");
        world.patient = Some(Patient {
            status,
            ..patient("p1", "נועם")
        });
        assert!(!world.fires(AlertKind::NoNextVisit));
    }
}

#[test]
fn no_next_visit_is_quiet_once_the_assessment_is_over_and_paid() {
    let mut done = with_visits(vec![Visit {
        fee: 40_000,
        ..visit(
            "f1",
            VisitKind::Feedback,
            "2026-10-20",
            600,
            VisitStatus::Held,
        )
    }]);
    // The feedback was given and money is still owed: she is not finished with them.
    assert!(done.fires(AlertKind::NoNextVisit));
    done.payments = vec![payment("pay1", "2026-10-20", 40_000)];
    assert!(!done.fires(AlertKind::NoNextVisit));
    done.payments[0].amount = 50_000; // paid ahead is "nothing owed" too
    assert!(!done.fires(AlertKind::NoNextVisit));
}

#[test]
fn task_overdue_fires_for_an_open_task_whose_date_passed() {
    let with_task = |due: Option<&str>| World {
        tasks: vec![task("t1", Some("p1"), due)],
        ..World::default()
    };
    let fired = with_task(Some("2026-11-14")).of_kind(AlertKind::TaskOverdue);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].task_id.as_deref(), Some("t1"));
    assert_eq!(fired[0].since, "2026-11-14");
    assert_eq!(
        fired[0].text,
        "המשימה \"לחזור לגננת\" הייתה אמורה להיעשות עד 14.11.2026."
    );
    assert!(!with_task(Some(TODAY)).fires(AlertKind::TaskOverdue));
    assert!(!with_task(Some("2026-11-16")).fires(AlertKind::TaskOverdue));
    assert!(!with_task(None).fires(AlertKind::TaskOverdue));
    let mut done = with_task(Some("2026-11-01"));
    done.tasks[0].done_on = Some("2026-11-10".to_owned());
    assert!(!done.fires(AlertKind::TaskOverdue));
}

#[test]
fn the_clinic_sees_every_patient_and_the_tasks_of_no_one() {
    let noam = patient("p1", "נועם");
    let tamar = Patient {
        status: PatientStatus::Waiting,
        waiting_since: Some("2026-08-02".to_owned()),
        ..patient("p2", "תמר")
    };
    let patients = vec![noam.clone(), tamar.clone()];
    let visits = vec![
        visit(
            "v1",
            VisitKind::Therapy,
            "2026-11-12",
            600,
            VisitStatus::Planned,
        ),
        Visit {
            patient_id: Some("gone".to_owned()), // a patient that is not in the clinic
            ..visit(
                "v2",
                VisitKind::Therapy,
                "2026-11-01",
                600,
                VisitStatus::Planned,
            )
        },
    ];
    let charges = vec![Charge {
        patient_id: "p2".to_owned(),
        ..charge("c1", "2026-09-01", 40_000)
    }];
    let tasks = vec![
        task("mine", None, Some("2026-11-05")),
        task("orphan", Some("gone"), Some("2026-11-06")),
        task("noam", Some("p1"), Some("2026-11-13")),
        task("later", None, Some("2026-12-01")),
    ];
    let reports = HashMap::new();
    let clinic = ClinicRecords {
        patients: &patients,
        visits: &visits,
        notes: &[],
        charges: &charges,
        payments: &[],
        tasks: &tasks,
        reports: &reports,
    };
    let settings = ClinicSettings::default();
    let alerts = clinic_alerts(&clinic, &today(), &settings);
    let seen: Vec<(AlertKind, &str, Option<&str>)> = alerts
        .iter()
        .map(|a| (a.kind, a.since.as_str(), a.patient_name.as_deref()))
        .collect();
    // Oldest first.
    assert_eq!(
        seen,
        [
            (AlertKind::Waiting, "2026-08-02", Some("תמר")),
            (AlertKind::Debt, "2026-09-01", Some("תמר")),
            (AlertKind::TaskOverdue, "2026-11-05", None),
            (AlertKind::TaskOverdue, "2026-11-06", None),
            (AlertKind::Unmarked, "2026-11-12", Some("נועם")),
            (AlertKind::TaskOverdue, "2026-11-13", Some("נועם")),
        ]
    );
    for alert in &alerts {
        assert_hebrew_sentence(&alert.text);
        assert!(!alert.text.contains("נועם") && !alert.text.contains("תמר"));
    }

    // Each patient's own view is their part of it, picked out of everyone's records.
    for p in &patients {
        let own = patient_alerts(
            &PatientRecords {
                patient: p,
                visits: &visits,
                notes: &[],
                charges: &charges,
                payments: &[],
                tasks: &tasks,
                report: None,
            },
            &today(),
            &settings,
        );
        let expected: Vec<Alert> = alerts
            .iter()
            .filter(|a| a.patient_id.as_deref() == Some(p.id.as_str()))
            .cloned()
            .collect();
        assert_eq!(own, expected);
    }
}

#[test]
fn alerts_of_one_day_come_in_a_steady_order() {
    let world = World {
        visits: vec![
            visit(
                "b",
                VisitKind::Therapy,
                "2026-11-10",
                700,
                VisitStatus::Planned,
            ),
            visit(
                "a",
                VisitKind::Therapy,
                "2026-11-10",
                600,
                VisitStatus::Held,
            ),
        ],
        tasks: vec![task("t", Some("p1"), Some("2026-11-10"))],
        ..World::default()
    };
    let kinds: Vec<AlertKind> = world.alerts().iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [
            AlertKind::Unmarked,
            AlertKind::NoNote,
            AlertKind::TaskOverdue
        ]
    );
    assert!(report_finished(Some((3, 3))));
    assert!(report_finished(Some((4, 3))));
    assert!(!report_finished(Some((2, 3))));
    assert!(!report_finished(Some((0, 0))));
    assert!(!report_finished(None));
}

// ------------------------------------------------------------------ the assessment process

fn states(steps: &[ProcessStep]) -> Vec<(&str, StepState)> {
    steps.iter().map(|s| (s.key.as_str(), s.state)).collect()
}

fn detail<'a>(steps: &'a [ProcessStep], key: &str) -> &'a str {
    steps
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.detail.as_str())
        .unwrap()
}

#[test]
fn a_new_patient_starts_at_the_intake() {
    let steps = process(&[], &[], &[], None);
    assert_eq!(
        states(&steps),
        [
            ("intake", StepState::Now), // nothing is under way: the first thing to do
            ("sessions", StepState::Todo),
            ("report", StepState::Todo),
            ("feedback", StepState::Todo),
            ("payment", StepState::Todo),
        ]
    );
    for s in &steps {
        assert_hebrew_sentence(&s.label);
        assert_hebrew_sentence(&s.detail);
    }
    assert_eq!(detail(&steps, "intake"), "עוד לא נקבעה");
    assert_eq!(detail(&steps, "sessions"), "עוד לא נקבעו מפגשים");
    assert_eq!(detail(&steps, "report"), "עוד לא נפתח תיק לדוח");
    assert_eq!(detail(&steps, "payment"), "עוד לא נרשם חיוב");
}

#[test]
fn the_process_in_the_middle_of_an_assessment() {
    let held = VisitStatus::Held;
    let planned = VisitStatus::Planned;
    let visits = vec![
        visit("i", VisitKind::Intake, "2026-10-01", 600, held),
        paid_visit("a1", "2026-10-10", held, 60_000),
        paid_visit("a2", "2026-10-17", held, 60_000),
        paid_visit("a3", "2026-11-20", planned, 60_000),
        visit("o", VisitKind::Observation, "2026-11-25", 600, planned),
        visit(
            "x",
            VisitKind::Assessment,
            "2026-10-12",
            600,
            VisitStatus::NoShow,
        ),
    ];
    let payments = vec![payment("pay", "2026-10-10", 45_000)];
    let steps = process(&visits, &[], &payments, Some((9, 16)));
    assert_eq!(
        states(&steps),
        [
            ("intake", StepState::Done),
            ("sessions", StepState::Now),
            ("observation", StepState::Now),
            ("report", StepState::Now), // she started writing before the testing ended
            ("feedback", StepState::Todo),
            ("payment", StepState::Now),
        ]
    );
    assert_eq!(detail(&steps, "intake"), "התקיימה ביום 01.10.2026");
    assert_eq!(detail(&steps, "sessions"), "2 מתוך 3 התקיימו");
    assert_eq!(detail(&steps, "observation"), "נקבעה ליום 25.11.2026");
    assert_eq!(detail(&steps, "report"), "הדוח: 9 מתוך 16 סעיפים");
    assert_eq!(detail(&steps, "feedback"), "עוד לא נקבעה");
    assert_eq!(detail(&steps, "payment"), "נותרו 750 ₪ לתשלום");
}

#[test]
fn each_step_of_the_process_in_each_state() {
    let v = |id: &str, kind, date: &str, status| visit(id, kind, date, 600, status);
    let state_of = |visits: &[Visit], report, key: &str| {
        let steps = process(visits, &[], &[], report);
        steps
            .iter()
            .find(|s| s.key == key)
            .map(|s| (s.state, s.detail.clone()))
    };
    let planned = VisitStatus::Planned;
    let held = VisitStatus::Held;

    // intake
    let intake = [v("i", VisitKind::Intake, "2026-11-20", planned)];
    assert_eq!(
        state_of(&intake, None, "intake"),
        Some((StepState::Now, "נקבעה ליום 20.11.2026".to_owned()))
    );

    // sessions
    let first_ahead = [
        v("a2", VisitKind::Assessment, "2026-11-27", planned),
        v("a1", VisitKind::Assessment, "2026-11-20", planned),
    ];
    assert_eq!(
        state_of(&first_ahead, None, "sessions"),
        Some((
            StepState::Now,
            "המפגש הראשון נקבע ליום 20.11.2026".to_owned()
        ))
    );
    let one = [v("a1", VisitKind::Assessment, "2026-11-01", held)];
    assert_eq!(
        state_of(&one, None, "sessions"),
        Some((StepState::Done, "מפגש אחד התקיים".to_owned()))
    );
    let three: Vec<Visit> = (1..=3)
        .map(|n| v(&format!("a{n}"), VisitKind::Assessment, "2026-11-01", held))
        .collect();
    assert_eq!(
        state_of(&three, None, "sessions"),
        Some((StepState::Done, "3 מפגשים התקיימו".to_owned()))
    );

    // observation: there only when one exists, and a cancelled one is not one
    assert_eq!(state_of(&one, None, "observation"), None);
    let cancelled = [v(
        "o",
        VisitKind::Observation,
        "2026-11-01",
        VisitStatus::Cancelled,
    )];
    assert_eq!(state_of(&cancelled, None, "observation"), None);
    let observed = [v("o", VisitKind::Observation, "2026-11-01", held)];
    assert_eq!(
        state_of(&observed, None, "observation"),
        Some((StepState::Done, "התקיימה ביום 01.11.2026".to_owned()))
    );
    let missed = [
        v("i", VisitKind::Intake, "2026-11-20", planned),
        v(
            "o",
            VisitKind::Observation,
            "2026-11-01",
            VisitStatus::NoShow,
        ),
    ];
    assert_eq!(
        state_of(&missed, None, "observation"),
        Some((StepState::Todo, "לא התקיימה".to_owned()))
    );

    // report
    assert_eq!(
        state_of(&one, None, "report"),
        Some((StepState::Now, "עוד לא נפתח תיק לדוח".to_owned()))
    );
    assert_eq!(
        state_of(&one, Some((0, 16)), "report"),
        Some((StepState::Now, "הדוח: 0 מתוך 16 סעיפים".to_owned()))
    );
    assert_eq!(
        state_of(&intake, Some((0, 16)), "report"),
        Some((StepState::Todo, "הדוח: 0 מתוך 16 סעיפים".to_owned()))
    );
    assert_eq!(
        state_of(&intake, Some((16, 16)), "report"),
        Some((StepState::Done, "הדוח: 16 מתוך 16 סעיפים".to_owned()))
    );

    // feedback
    let feedback_ahead = [v("f", VisitKind::Feedback, "2026-11-22", planned)];
    assert_eq!(
        state_of(&feedback_ahead, None, "feedback"),
        Some((StepState::Now, "נקבעה ליום 22.11.2026".to_owned()))
    );
    let feedback_held = [v("f", VisitKind::Feedback, "2026-11-10", held)];
    assert_eq!(
        state_of(&feedback_held, None, "feedback"),
        Some((StepState::Done, "התקיימה ביום 10.11.2026".to_owned()))
    );
}

#[test]
fn the_payment_step_follows_the_balance() {
    let payment_of = |charges: &[Charge], payments: &[Payment]| {
        let steps = process(&[], charges, payments, None);
        steps
            .iter()
            .find(|s| s.key == "payment")
            .map(|s| (s.state, s.detail.clone()))
            .unwrap()
    };
    let package = [charge("c", "2026-10-01", 250_000)];
    assert_eq!(
        payment_of(&package, &[]),
        (StepState::Now, "נותרו 2500 ₪ לתשלום".to_owned())
    );
    assert_eq!(
        payment_of(&package, &[payment("p", "2026-10-02", 250_000)]),
        (StepState::Done, "שולם במלואו".to_owned())
    );
    assert_eq!(
        payment_of(&package, &[payment("p", "2026-10-02", 260_050)]),
        (
            StepState::Done,
            "שולם, ויש יתרת זכות של 100.50 ₪".to_owned()
        )
    );
    // Money came in before anything was charged.
    assert_eq!(
        payment_of(&[], &[payment("p", "2026-10-02", 10_000)]),
        (StepState::Done, "שולם, ויש יתרת זכות של 100 ₪".to_owned())
    );
}

#[test]
fn a_finished_assessment_is_done_all_the_way() {
    let held = VisitStatus::Held;
    let visits = vec![
        visit("i", VisitKind::Intake, "2026-09-01", 600, held),
        paid_visit("a1", "2026-09-10", held, 60_000),
        visit("o", VisitKind::Observation, "2026-09-15", 600, held),
        visit("f", VisitKind::Feedback, "2026-10-20", 600, held),
    ];
    let payments = vec![payment("pay", "2026-10-20", 60_000)];
    let steps = process(&visits, &[], &payments, Some((16, 16)));
    assert_eq!(steps.len(), 6);
    assert!(steps.iter().all(|s| s.state == StepState::Done));
    let keys: Vec<&str> = steps.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "intake",
            "sessions",
            "observation",
            "report",
            "feedback",
            "payment"
        ]
    );
}

// ------------------------------------------------------------------ the diary and income

fn names() -> HashMap<String, String> {
    HashMap::from([
        ("p1".to_owned(), "נועם".to_owned()),
        ("p2".to_owned(), "תמר".to_owned()),
    ])
}

#[test]
fn the_diary_of_a_month() {
    let visits = vec![
        Visit {
            start: 540,
            ..paid_visit("v-held", "2026-11-03", VisitStatus::Held, 40_000)
        },
        Visit {
            start: 480,
            status_note: "הילד חולה".to_owned(),
            ..paid_visit("v-cancelled", "2026-11-03", VisitStatus::Cancelled, 40_000)
        },
        visit(
            "block",
            VisitKind::Block,
            "2026-11-03",
            400,
            VisitStatus::Planned,
        ),
        paid_visit("before", "2026-10-31", VisitStatus::Held, 40_000),
        paid_visit("after", "2026-12-01", VisitStatus::Held, 40_000),
        paid_visit("last-day", "2026-11-30", VisitStatus::Planned, 40_000),
    ];
    let charges = vec![
        charge("c-live", "2026-11-03", 100_000),
        Charge {
            voided: true,
            void_reason: "נרשם פעמיים".to_owned(),
            ..charge("c-void", "2026-11-03", 100_000)
        },
        charge("c-out", "2025-11-03", 100_000),
    ];
    let payments = vec![
        payment("pay-bit", "2026-11-03", 50_000),
        Payment {
            method: PayMethod::Cash,
            receipt: " ".to_owned(),
            payer: "קרן בדויה".to_owned(),
            patient_id: "p2".to_owned(),
            ..payment("pay-cash", "2026-11-20", 30_000)
        },
        Payment {
            voided: true,
            void_reason: "הצ'ק חזר".to_owned(),
            method: PayMethod::Check,
            receipt: String::new(),
            note: "צ'ק דחוי".to_owned(),
            ..payment("pay-void", "2026-11-20", 90_000)
        },
        Payment {
            method: PayMethod::Cash,
            ..payment("pay-cash-2", "2026-11-01", 5_000)
        },
        payment("pay-out", "2026-12-01", 70_000),
    ];
    let changed = HashSet::from(["v-held".to_owned(), "pay-void".to_owned()]);
    let diary = diary_month("2026-11", &visits, &charges, &payments, &names(), &changed).unwrap();

    assert_eq!(diary.month, "2026-11");
    let dates: Vec<&str> = diary.days.iter().map(|d| d.date.as_str()).collect();
    assert_eq!(
        dates,
        ["2026-11-01", "2026-11-03", "2026-11-20", "2026-11-30"]
    );

    let third = &diary.days[1];
    let order: Vec<&str> = third.lines.iter().map(|l| l.record_id.as_str()).collect();
    // Visits by time, then charges, then payments; the block is not in the diary.
    assert_eq!(
        order,
        ["v-cancelled", "v-held", "c-live", "c-void", "pay-bit"]
    );
    assert_eq!((third.charged, third.received), (140_000, 50_000));

    let cancelled = &third.lines[0];
    assert_eq!(cancelled.time, Some(480));
    assert_eq!(cancelled.patient_name, "נועם");
    assert_eq!(cancelled.what, "מפגש אבחון");
    assert_eq!(cancelled.status, Some(VisitStatus::Cancelled));
    assert_eq!((cancelled.charged, cancelled.received), (0, 0));
    assert_eq!(cancelled.note, "הילד חולה");
    assert!(!cancelled.changed && !cancelled.voided);
    let held = &third.lines[1];
    assert_eq!((held.charged, held.changed), (40_000, true));
    let live = &third.lines[2];
    assert_eq!(
        (live.what.as_str(), live.charged, live.time),
        ("חבילת אבחון", 100_000, None)
    );
    assert_eq!((live.status, live.method), (None, None));
    let void = &third.lines[3];
    // Shown with its amount, marked, and counted nowhere.
    assert_eq!((void.charged, void.voided), (100_000, true));
    assert_eq!(void.note, "בוטל: נרשם פעמיים");
    let bit = &third.lines[4];
    assert_eq!((bit.what.as_str(), bit.received), ("תשלום", 50_000));
    assert_eq!(
        (bit.method, bit.receipt.as_str()),
        (Some(PayMethod::Bit), "101")
    );

    let twentieth = &diary.days[2];
    assert_eq!((twentieth.charged, twentieth.received), (0, 30_000));
    let cash = &twentieth.lines[0];
    assert_eq!(
        (cash.patient_name.as_str(), cash.payer.as_str()),
        ("תמר", "קרן בדויה")
    );
    assert_eq!(cash.receipt, "");
    let bounced = &twentieth.lines[1];
    assert_eq!(
        (bounced.received, bounced.voided, bounced.changed),
        (90_000, true, true)
    );
    assert_eq!(bounced.note, "צ'ק דחוי · בוטל: הצ'ק חזר");

    // A planned visit is in the diary and charges nothing yet.
    assert_eq!(diary.days[3].lines[0].status, Some(VisitStatus::Planned));
    assert_eq!(diary.days[3].charged, 0);

    assert_eq!((diary.charged, diary.received), (140_000, 85_000));
    assert_eq!(
        diary.by_method,
        [
            MethodTotal {
                method: PayMethod::Cash,
                amount: 35_000
            },
            MethodTotal {
                method: PayMethod::Bit,
                amount: 50_000
            },
        ]
    );
    assert_eq!(diary.missing_receipts, 1); // the voided one without a receipt is not missing one
}

#[test]
fn the_diary_of_february_and_of_nothing() {
    let visits = vec![
        paid_visit("leap", "2024-02-29", VisitStatus::Held, 10_000),
        paid_visit("march", "2024-03-01", VisitStatus::Held, 10_000),
        Visit {
            patient_id: Some("unknown".to_owned()),
            ..paid_visit("nameless", "2024-02-01", VisitStatus::Held, 10_000)
        },
    ];
    let diary = diary_month("2024-02", &visits, &[], &[], &names(), &HashSet::new()).unwrap();
    assert_eq!(diary.days.len(), 2);
    assert_eq!(diary.days[0].lines[0].patient_name, ""); // no name known for the id
    assert_eq!(diary.days[1].date, "2024-02-29");
    assert_eq!(diary.charged, 20_000);

    let empty = diary_month("2026-01", &visits, &[], &[], &names(), &HashSet::new()).unwrap();
    assert_eq!(
        empty,
        DiaryMonth {
            month: "2026-01".to_owned(),
            days: Vec::new(),
            charged: 0,
            received: 0,
            by_method: Vec::new(),
            missing_receipts: 0,
        }
    );
    for bad in ["2026-13", "2026", "2026-1", ""] {
        refusal(diary_month(bad, &[], &[], &[], &names(), &HashSet::new()));
    }
}

#[test]
fn a_year_of_income() {
    let v = |id: &str, kind, date: &str, status, fee| Visit {
        fee,
        ..visit(id, kind, date, 600, status)
    };
    let held = VisitStatus::Held;
    let visits = vec![
        v("1", VisitKind::Intake, "2026-01-01", held, 30_000),
        v("2", VisitKind::Assessment, "2026-01-31", held, 60_000),
        v("3", VisitKind::Assessment, "2026-02-28", held, 60_000),
        v(
            "4",
            VisitKind::Therapy,
            "2026-02-10",
            VisitStatus::Cancelled,
            40_000,
        ),
        Visit {
            charge_missed: true,
            ..v(
                "5",
                VisitKind::Therapy,
                "2026-02-11",
                VisitStatus::NoShow,
                40_000,
            )
        },
        v("6", VisitKind::Assessment, "2026-12-31", held, 60_000),
        v(
            "7",
            VisitKind::Feedback,
            "2026-12-31",
            VisitStatus::Planned,
            60_000,
        ),
        // Not this year, and not a patient visit.
        v("8", VisitKind::Assessment, "2025-12-31", held, 60_000),
        v("9", VisitKind::Assessment, "2027-01-01", held, 60_000),
        visit("10", VisitKind::Block, "2026-03-01", 600, held),
    ];
    let charges = vec![
        charge("c1", "2026-02-01", 250_000),
        Charge {
            voided: true,
            void_reason: "טעות".to_owned(),
            ..charge("c2", "2026-02-01", 250_000)
        },
        charge("c3", "2027-02-01", 250_000),
    ];
    let payments = vec![
        payment("p1", "2026-02-15", 300_000),
        payment("p2", "2026-12-31", 60_000),
        Payment {
            voided: true,
            void_reason: "טעות".to_owned(),
            ..payment("p3", "2026-06-01", 999_000)
        },
        payment("p4", "2025-12-31", 10_000),
    ];
    let patients = vec![
        Patient {
            created_on: "2026-01-01".to_owned(),
            ..patient("p1", "נועם")
        },
        Patient {
            created_on: "2026-12-31".to_owned(),
            ..patient("p2", "תמר")
        },
        Patient {
            created_on: "2025-12-31".to_owned(),
            ..patient("p3", "יובל")
        },
        Patient {
            created_on: "2027-01-01".to_owned(),
            ..patient("p4", "שירה")
        },
    ];
    let year = income_year(2026, &visits, &charges, &payments, &patients).unwrap();

    assert_eq!(year.year, 2026);
    assert_eq!(year.months.len(), 12);
    let months: Vec<&str> = year.months.iter().map(|m| m.month.as_str()).collect();
    assert_eq!(months[0], "2026-01");
    assert_eq!(months[11], "2026-12");
    assert_eq!(
        year.months[0],
        IncomeMonth {
            month: "2026-01".to_owned(),
            held: 2,
            cancelled: 0,
            no_show: 0,
            charged: 90_000,
            received: 0,
        }
    );
    assert_eq!(
        year.months[1],
        IncomeMonth {
            month: "2026-02".to_owned(),
            held: 1,
            cancelled: 1,
            no_show: 1,
            charged: 350_000, // the session, the missed one she charges for, the package
            received: 300_000,
        }
    );
    assert_eq!(year.months[2].held, 0); // the block in March is not a session
    assert_eq!(
        (
            year.months[11].held,
            year.months[11].charged,
            year.months[11].received
        ),
        (1, 60_000, 60_000)
    );
    assert!(year.months[2..11]
        .iter()
        .all(|m| m.charged == 0 && m.received == 0));
    assert_eq!((year.charged, year.received), (500_000, 360_000));
    assert_eq!(year.new_patients, 2);
    assert_eq!(
        year.by_kind,
        [
            KindCount {
                kind: VisitKind::Assessment,
                count: 3
            },
            KindCount {
                kind: VisitKind::Intake,
                count: 1
            },
        ]
    );

    let quiet = income_year(2030, &visits, &charges, &payments, &patients).unwrap();
    assert_eq!(
        (quiet.charged, quiet.received, quiet.new_patients),
        (0, 0, 0)
    );
    assert!(quiet.by_kind.is_empty());
    assert_eq!(quiet.months.len(), 12);
    refusal(income_year(1999, &[], &[], &[], &[]));
}

#[test]
fn kinds_that_are_as_frequent_keep_the_usual_order() {
    let held = |id: &str, kind| visit(id, kind, "2026-05-05", 600, VisitStatus::Held);
    let visits = vec![
        held("1", VisitKind::Guidance),
        held("2", VisitKind::Intake),
        held("3", VisitKind::Therapy),
        held("4", VisitKind::Therapy),
    ];
    let year = income_year(2026, &visits, &[], &[], &[]).unwrap();
    let kinds: Vec<VisitKind> = year.by_kind.iter().map(|k| k.kind).collect();
    assert_eq!(
        kinds,
        [VisitKind::Therapy, VisitKind::Intake, VisitKind::Guidance]
    );
}

// ------------------------------------------------------------------ messages

fn values() -> MessageValues {
    MessageValues {
        parent: "דנה".to_owned(),
        child: "נועם".to_owned(),
        date: Some(TODAY.to_owned()),
        start: Some(570),
        place: "הקליניקה".to_owned(),
        amount: Some(45_050),
        me: "מיכל".to_owned(),
    }
}

#[test]
fn the_default_messages_fill_in() {
    let settings = ClinicSettings::default();
    assert_eq!(
        fill_message(&settings.reminder_text, &values()),
        "שלום דנה, תזכורת לפגישה ביום ראשון, 15.11.2026, בשעה 09:30. להתראות, מיכל"
    );
    assert_eq!(
        fill_message(&settings.payment_text, &values()),
        "שלום דנה, היתרה לתשלום היא 450.50 ₪. תודה רבה, מיכל"
    );
}

#[test]
fn a_missing_parent_is_greeted_as_rav() {
    let mut v = values();
    v.parent = "  ".to_owned();
    assert_eq!(fill_message("שלום {הורה},", &v), "שלום רב,");
    assert_eq!(
        fill_message("שלום {הורה},", &MessageValues::default()),
        "שלום רב,"
    );
}

#[test]
fn every_placeholder_is_filled_and_nothing_else_is_touched() {
    let all = PLACEHOLDERS.join("|");
    assert_eq!(
        fill_message(&all, &values()),
        "דנה|נועם|ראשון|15.11.2026|09:30|הקליניקה|450.50|מיכל"
    );
    // What is missing is left empty, never a placeholder she would send by mistake.
    assert_eq!(fill_message(&all, &MessageValues::default()), "רב|||||||");
    let filled = fill_message(&format!("א{all}ב{all}"), &values());
    assert!(PLACEHOLDERS.iter().all(|p| !filled.contains(p)));

    assert_eq!(fill_message("", &values()), "");
    assert_eq!(fill_message("בלי כלום", &values()), "בלי כלום");
    assert_eq!(
        fill_message("{אחר} {} {{ילד}} {ילד", &values()),
        "{אחר} {} {נועם} {ילד"
    );
    assert_eq!(fill_message("סוף {", &values()), "סוף {");
    assert_eq!(fill_message("{ילד}{ילד}", &values()), "נועםנועם");
}

#[test]
fn a_value_is_never_read_again_as_a_placeholder() {
    let mut v = values();
    v.place = "{ילד}".to_owned();
    v.child = "{שמי}".to_owned();
    assert_eq!(fill_message("{מקום} {ילד} {שמי}", &v), "{ילד} {שמי} מיכל");
}

#[test]
fn the_parent_to_greet_is_the_first_contact_with_a_name() {
    let mut p = patient("p1", "נועם");
    assert_eq!(message_parent(&p), "דנה");
    p.contacts.insert(
        0,
        Contact {
            role: ContactRole::Father,
            name: "  ".to_owned(),
            phone: "000-0000".to_owned(),
            email: String::new(),
        },
    );
    assert_eq!(message_parent(&p), "דנה");
    p.contacts.clear();
    assert_eq!(message_parent(&p), "");
}

// ------------------------------------------------------------------ history lines

#[test]
fn a_visit_in_her_words() {
    let v = Visit {
        title: "מפגש שני".to_owned(),
        place: "הקליניקה".to_owned(),
        minutes: 90,
        status_note: "הגיע עם אמא".to_owned(),
        charge_missed: true,
        series_id: Some("s1".to_owned()),
        note: "להביא\nאת השאלונים".to_owned(),
        ..paid_visit("v1", TODAY, VisitStatus::Held, 45_000)
    };
    assert_eq!(
        visit_history_lines(&Visit { start: 570, ..v }, Some("נועם")),
        [
            "מטופל: נועם",
            "סוג: מפגש אבחון",
            "כותרת: מפגש שני",
            "תאריך: 15.11.2026",
            "שעה: 09:30",
            "משך: 90 דקות",
            "מקום: הקליניקה",
            "מצב: התקיימה",
            "מה קרה: הגיע עם אמא",
            "מחיר: 450 ₪",
            "חיוב גם אם לא התקיימה: כן",
            "פגישה חוזרת: כן",
            "הערה: להביא את השאלונים",
        ]
    );
    // Only what says something: a bare block has no patient, no place and no price.
    let block = visit("b", VisitKind::Block, "2026-12-31", 0, VisitStatus::Planned);
    assert_eq!(
        visit_history_lines(&block, None),
        [
            "סוג: זמן שמור",
            "תאריך: 31.12.2026",
            "שעה: 00:00",
            "משך: 60 דקות",
            "מצב: מתוכננת"
        ]
    );
    let free = visit("f", VisitKind::Intake, TODAY, 600, VisitStatus::Planned);
    assert!(visit_history_lines(&free, Some("תמר")).contains(&"מחיר: ללא חיוב".to_owned()));
}

#[test]
fn a_payment_and_a_charge_in_her_words() {
    let p = Payment {
        payer: "קרן בדויה".to_owned(),
        note: "תשלום ראשון".to_owned(),
        ..payment("pay1", TODAY, 45_050)
    };
    assert_eq!(
        payment_history_lines(&p, "נועם"),
        [
            "מטופל: נועם",
            "תאריך: 15.11.2026",
            "סכום: 450.50 ₪",
            "אמצעי תשלום: ביט",
            "מספר קבלה: 101",
            "מי שילם: קרן בדויה",
            "הערה: תשלום ראשון",
        ]
    );
    let voided = Payment {
        voided: true,
        void_reason: "נרשם פעמיים".to_owned(),
        receipt: String::new(),
        ..payment("pay1", TODAY, 45_000)
    };
    assert_eq!(
        payment_history_lines(&voided, "נועם"),
        [
            "מטופל: נועם",
            "תאריך: 15.11.2026",
            "סכום: 450 ₪",
            "אמצעי תשלום: ביט",
            "בוטל: כן",
            "סיבת הביטול: נרשם פעמיים",
        ]
    );
    assert_eq!(
        charge_history_lines(&charge("c1", TODAY, 250_000), "נועם"),
        [
            "מטופל: נועם",
            "תאריך: 15.11.2026",
            "סכום: 2500 ₪",
            "על מה: חבילת אבחון"
        ]
    );
    let void_charge = Charge {
        voided: true,
        void_reason: "טעות".to_owned(),
        ..charge("c1", TODAY, 100)
    };
    assert_eq!(
        charge_history_lines(&void_charge, ""),
        [
            "תאריך: 15.11.2026",
            "סכום: 1 ₪",
            "על מה: חבילת אבחון",
            "בוטל: כן",
            "סיבת הביטול: טעות"
        ]
    );
}

// ------------------------------------------------------------------ the overview

#[test]
fn tasks_come_overdue_first_then_by_date_then_undated_then_done() {
    let done = |id: &str, due: Option<&str>, on: &str| ClinicTask {
        done_on: Some(on.to_owned()),
        ..task(id, None, due)
    };
    let tasks = vec![
        task("undated", None, None),
        task("next-week", Some("p1"), Some("2026-11-22")),
        done("done-old", Some("2026-01-01"), "2026-11-01"),
        task("today", None, Some(TODAY)),
        task("late", Some("p2"), Some("2026-11-01")),
        done("done-new", None, "2026-11-14"),
        task("very-late", Some("gone"), Some("2025-12-31")),
    ];
    let views = task_views(&tasks, &names(), TODAY);
    let seen: Vec<(&str, bool, Option<&str>)> = views
        .iter()
        .map(|t| (t.task.id.as_str(), t.overdue, t.patient_name.as_deref()))
        .collect();
    assert_eq!(
        seen,
        [
            ("very-late", true, None),
            ("late", true, Some("תמר")),
            ("today", false, None),
            ("next-week", false, Some("נועם")),
            ("undated", false, None),
            ("done-new", false, None),
            ("done-old", false, None), // done: not overdue, whatever its date was
        ]
    );
    assert_eq!(open_task_count(&tasks), 5);
    assert_eq!(open_task_count(&[]), 0);
    assert!(task_views(&[], &names(), TODAY).is_empty());

    assert!(task_overdue(&task("t", None, Some("2026-11-14")), TODAY));
    assert!(!task_overdue(&task("t", None, Some(TODAY)), TODAY));
    assert!(!task_overdue(&task("t", None, None), TODAY));
    assert!(!task_overdue(&task("t", None, Some("2026-11-14")), "היום"));
    assert!(!task_overdue(&done("t", Some("2026-11-14"), TODAY), TODAY));
}

#[test]
fn the_weeks_visits_are_counted_sunday_to_saturday() {
    let v = |id: &str, date: &str, status| visit(id, VisitKind::Therapy, date, 600, status);
    let planned = VisitStatus::Planned;
    let visits = vec![
        v("sat-before", "2026-11-14", planned),
        v("sun", TODAY, VisitStatus::Held),
        v("wed", "2026-11-18", planned),
        v("missed", "2026-11-19", VisitStatus::NoShow),
        v("cancelled", "2026-11-19", VisitStatus::Cancelled),
        v("sat", "2026-11-21", planned),
        v("sun-after", "2026-11-22", planned),
        visit("block", VisitKind::Block, "2026-11-17", 600, planned),
    ];
    assert_eq!(week_visit_count(&visits, TODAY), 4);
    assert_eq!(week_visit_count(&visits, "2026-11-21"), 4);
    assert_eq!(week_visit_count(&visits, "2026-11-14"), 1);
    assert_eq!(week_visit_count(&visits, "2026-11-22"), 1);
    assert_eq!(week_visit_count(&visits, "2026-12-31"), 0);
    assert_eq!(week_visit_count(&visits, "x"), 0);
    // A week that spans a year end is one week.
    let new_year = vec![v("a", "2026-12-27", planned), v("b", "2027-01-02", planned)];
    assert_eq!(week_visit_count(&new_year, "2026-12-31"), 2);
    assert_eq!(week_visit_count(&new_year, "2027-01-01"), 2);
}

#[test]
fn the_next_day_with_visits_shows_when_today_has_none_left() {
    let v =
        |id: &str, date: &str, start, status| visit(id, VisitKind::Therapy, date, start, status);
    let planned = VisitStatus::Planned;
    let at = |minute| Today {
        date: TODAY.to_owned(),
        minute,
    };
    let visits = vec![
        v("morning", TODAY, 540, VisitStatus::Held), // 09:00–10:00
        v("past", "2026-11-10", 540, planned),
        v("cancelled", "2026-11-16", 540, VisitStatus::Cancelled),
        visit("block", VisitKind::Block, "2026-11-17", 540, planned),
        v("thursday", "2026-11-19", 900, planned),
        v("later", "2026-11-26", 540, planned),
    ];
    // While this morning's visit is not over, today still has one.
    assert_eq!(next_visit_day(&visits, &at(599)), None);
    // Once it ended: the next day that has a real visit, past the cancelled one and the block.
    assert_eq!(
        next_visit_day(&visits, &at(600)).as_deref(),
        Some("2026-11-19")
    );
    assert_eq!(
        next_visit_day(&visits, &at(1_439)).as_deref(),
        Some("2026-11-19")
    );
    assert_eq!(next_visit_day(&visits[..4], &at(600)), None); // nothing ahead
    assert_eq!(next_visit_day(&[], &at(0)), None);
    // Across a year end.
    let new_year = [v("n", "2027-01-03", 540, planned)];
    let last_day = Today {
        date: "2026-12-31".to_owned(),
        minute: 0,
    };
    assert_eq!(
        next_visit_day(&new_year, &last_day).as_deref(),
        Some("2027-01-03")
    );
}

#[test]
fn the_next_and_the_last_visit_of_a_patient() {
    let v = |id: &str, kind, date: &str, start, status| visit(id, kind, date, start, status);
    let visits = vec![
        v(
            "old",
            VisitKind::Intake,
            "2026-10-01",
            600,
            VisitStatus::Held,
        ),
        v(
            "held",
            VisitKind::Assessment,
            "2026-11-10",
            600,
            VisitStatus::Held,
        ),
        v(
            "missed",
            VisitKind::Assessment,
            "2026-11-12",
            600,
            VisitStatus::NoShow,
        ),
        v(
            "unmarked",
            VisitKind::Assessment,
            "2026-11-14",
            600,
            VisitStatus::Planned,
        ),
        v(
            "late",
            VisitKind::Feedback,
            "2026-11-20",
            900,
            VisitStatus::Planned,
        ),
        v(
            "early",
            VisitKind::Guidance,
            "2026-11-20",
            540,
            VisitStatus::Planned,
        ),
        v(
            "off",
            VisitKind::Therapy,
            "2026-11-16",
            540,
            VisitStatus::Cancelled,
        ),
    ];
    assert_eq!(
        next_visit(&visits, TODAY),
        Some(VisitWhen {
            date: "2026-11-20".to_owned(),
            start: 540,
            kind: VisitKind::Guidance
        })
    );
    assert_eq!(next_visit(&visits, "2026-11-21"), None);
    assert_eq!(
        next_visit(&visits, "2026-11-14").map(|w| w.date).as_deref(),
        Some("2026-11-14")
    );
    assert_eq!(last_held(&visits).as_deref(), Some("2026-11-10"));
    assert_eq!(last_held(&visits[2..]), None);
    assert_eq!(next_visit(&[], TODAY), None);
}

#[test]
fn visits_as_the_calendar_shows_them() {
    let visits = vec![
        visit("b", VisitKind::Therapy, TODAY, 600, VisitStatus::Planned),
        visit("blk", VisitKind::Block, TODAY, 500, VisitStatus::Planned),
        visit("a", VisitKind::Intake, "2026-11-14", 900, VisitStatus::Held),
    ];
    let noted = HashSet::from(["a".to_owned()]);
    let overlapping = BTreeSet::from(["b".to_owned()]);
    let views = visit_views(&visits, &names(), &noted, &overlapping);
    let seen: Vec<(&str, Option<&str>, bool, bool)> = views
        .iter()
        .map(|v| {
            (
                v.visit.id.as_str(),
                v.patient_name.as_deref(),
                v.has_note,
                v.overlaps,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("a", Some("נועם"), true, false),
            ("blk", None, false, false),
            ("b", Some("נועם"), false, true),
        ]
    );
}

#[test]
fn a_waiting_family_becomes_active_and_fees_have_defaults() {
    let waiting = Patient {
        status: PatientStatus::Waiting,
        waiting_since: Some("2026-10-01".to_owned()),
        ..patient("p1", "נועם")
    };
    let active = activated(&waiting).unwrap();
    assert_eq!(active.status, PatientStatus::Active);
    assert_eq!(active.waiting_since, waiting.waiting_since); // kept for the record
    assert_eq!(activated(&active), None);
    assert_eq!(
        activated(&Patient {
            status: PatientStatus::Closed,
            ..waiting.clone()
        }),
        None
    );

    let mut settings = ClinicSettings::default();
    for d in &mut settings.defaults {
        if d.kind == VisitKind::Assessment {
            d.fee = 60_000;
        }
    }
    assert_eq!(
        kind_default(&settings, VisitKind::Assessment).map(|d| (d.minutes, d.fee)),
        Some((90, 60_000))
    );
    settings.defaults.retain(|d| d.kind != VisitKind::Meeting);
    assert_eq!(kind_default(&settings, VisitKind::Meeting), None);

    let own_fee = Patient {
        session_fee: Some(35_000),
        ..waiting.clone()
    };
    assert_eq!(
        default_fee(Some(&own_fee), &settings, VisitKind::Assessment),
        35_000
    );
    assert_eq!(
        default_fee(Some(&waiting), &settings, VisitKind::Assessment),
        60_000
    );
    assert_eq!(default_fee(None, &settings, VisitKind::Assessment), 60_000);
    assert_eq!(default_fee(None, &settings, VisitKind::Meeting), 0);
    assert_eq!(default_fee(Some(&own_fee), &settings, VisitKind::Block), 0);
}

// ------------------------------------------------------------------ properties

/// A day number of a supported date (2000-01-01 … 2200-12-31).
fn any_day() -> impl Strategy<Value = i64> {
    10_957_i64..=84_370
}

fn any_status() -> impl Strategy<Value = VisitStatus> {
    prop_oneof![
        Just(VisitStatus::Planned),
        Just(VisitStatus::Held),
        Just(VisitStatus::Cancelled),
        Just(VisitStatus::NoShow),
    ]
}

/// Visits on a few neighbouring days, so they often overlap.
fn any_visits() -> impl Strategy<Value = Vec<Visit>> {
    prop::collection::vec(
        (
            0_i64..3,
            0_u16..600,
            5_u16..=240,
            any_status(),
            0..MAX_AMOUNT,
            any::<bool>(),
        ),
        0..12,
    )
    .prop_map(|rows| {
        rows.into_iter()
            .enumerate()
            .map(
                |(n, (day, start, minutes, status, fee, charge_missed))| Visit {
                    minutes,
                    fee,
                    charge_missed,
                    date: add_days(TODAY, day).unwrap(),
                    ..visit(&format!("v{n}"), VisitKind::Therapy, TODAY, start, status)
                },
            )
            .collect()
    })
}

fn any_charges() -> impl Strategy<Value = Vec<Charge>> {
    prop::collection::vec((0_i64..90, 1..=MAX_AMOUNT, any::<bool>()), 0..8).prop_map(|rows| {
        rows.into_iter()
            .enumerate()
            .map(|(n, (day, amount, voided))| Charge {
                voided,
                ..charge(
                    &format!("c{n}"),
                    &add_days("2026-08-01", day).unwrap(),
                    amount,
                )
            })
            .collect()
    })
}

fn any_payments() -> impl Strategy<Value = Vec<Payment>> {
    prop::collection::vec((0_i64..90, 1..=MAX_AMOUNT, any::<bool>()), 0..8).prop_map(|rows| {
        rows.into_iter()
            .enumerate()
            .map(|(n, (day, amount, voided))| Payment {
                voided,
                ..payment(
                    &format!("pay{n}"),
                    &add_days("2026-08-01", day).unwrap(),
                    amount,
                )
            })
            .collect()
    })
}

proptest! {
    #[test]
    fn day_numbers_and_dates_round_trip(day in any_day()) {
        let date = date_of_day(day).unwrap();
        prop_assert_eq!(day_number(&date), Some(day));
        prop_assert_eq!(parse_date(&date).map(crate::dates::iso), Some(date.clone()));
        // The display form is the same day, turned around.
        let shown = display_date(&date);
        let parts: Vec<&str> = shown.split('.').collect();
        prop_assert_eq!(format!("{}-{}-{}", parts[2], parts[1], parts[0]), date);
    }

    #[test]
    fn adding_days_is_undone_by_taking_them_away(day in any_day(), shift in -80_000_i64..80_000) {
        let date = date_of_day(day).unwrap();
        match add_days(&date, shift) {
            Some(moved) => {
                prop_assert_eq!(days_between(&date, &moved), Some(shift));
                prop_assert_eq!(add_days(&moved, -shift), Some(date.clone()));
                // ISO dates sort as text the way they sort as days.
                prop_assert_eq!(moved.cmp(&date), shift.cmp(&0));
            }
            None => prop_assert!(!(10_957..=84_370).contains(&(day + shift))),
        }
    }

    #[test]
    fn the_weekday_steps_with_the_days(day in any_day()) {
        let date = date_of_day(day).unwrap();
        let wd = weekday(&date).unwrap();
        prop_assert!(wd < 7);
        if let Some(next) = add_days(&date, 1) {
            prop_assert_eq!(weekday(&next), Some((wd + 1) % 7));
        }
        if let Some(next_week) = add_days(&date, 7) {
            prop_assert_eq!(weekday(&next_week), Some(wd));
        }
        let (sunday, saturday) = week_of(&date).unwrap();
        prop_assert!(sunday <= date && date <= saturday);
        prop_assert_eq!(crate::dates::parse_iso(&saturday).map(crate::dates::days_from_civil), Some(day - i64::from(wd) + 6));
        if day - i64::from(wd) >= 10_957 {
            prop_assert_eq!(weekday(&sunday), Some(0));
            prop_assert_eq!(days_between(&sunday, &date), Some(i64::from(wd)));
        }
    }

    #[test]
    fn a_month_holds_its_days(year in 2000_i32..=2200, month in 1_u32..=12, pick in 0_u32..31) {
        let text = format!("{year:04}-{month:02}");
        let (first, last) = month_bounds(&text).unwrap();
        prop_assert_eq!(month_of(&first), Some(text.clone()));
        prop_assert_eq!(month_of(&last), Some(text.clone()));
        let length = days_between(&first, &last).unwrap() + 1;
        prop_assert!((28..=31).contains(&length));
        // The day after the last is the first of the next month (or past the last year).
        if let Some(after) = add_days(&last, 1) {
            prop_assert!(after.ends_with("-01"));
            prop_assert_ne!(month_of(&after), Some(text.clone()));
        }
        let inside = add_days(&first, i64::from(pick) % length).unwrap();
        prop_assert_eq!(month_of(&inside), Some(text));
    }

    #[test]
    fn age_grows_with_the_days(birth in 10_957_i64..40_000, lived in 0_i64..9_000) {
        let born = date_of_day(birth).unwrap();
        let on = date_of_day(birth + lived).unwrap();
        let age = age_on(Some(&born), &on).unwrap();
        prop_assert!(age.months < 12);
        let months = i64::from(age.years) * 12 + i64::from(age.months);
        // A month is 28 to 31 days.
        prop_assert!(months * 28 <= lived, "{born} {on} {age:?}");
        prop_assert!(lived < (months + 1) * 31 + 1, "{born} {on} {age:?}");
        let tomorrow = age_on(Some(&born), &date_of_day(birth + lived + 1).unwrap()).unwrap();
        prop_assert!((tomorrow.years, tomorrow.months) >= (age.years, age.months));
        prop_assert_eq!(age_on(Some(&on), &born).is_some(), lived == 0);
    }

    #[test]
    fn a_series_has_its_count_and_its_spacing(
        day in 10_957_i64..80_000,
        every_weeks in MIN_EVERY_WEEKS..=MAX_EVERY_WEEKS,
        count in MIN_REPEAT_COUNT..=MAX_REPEAT_COUNT,
    ) {
        let first = date_of_day(day).unwrap();
        let dates = series_dates(&first, Repeat { every_weeks, count }).unwrap();
        prop_assert_eq!(dates.len(), usize::from(count));
        prop_assert_eq!(&dates[0], &first);
        for pair in dates.windows(2) {
            prop_assert_eq!(days_between(&pair[0], &pair[1]), Some(7 * i64::from(every_weeks)));
        }
        prop_assert!(dates.iter().all(|d| weekday(d) == weekday(&first)));
    }

    #[test]
    fn a_repeat_outside_its_limits_is_always_refused(every_weeks: u8, count: u8) {
        let inside = (MIN_EVERY_WEEKS..=MAX_EVERY_WEEKS).contains(&every_weeks)
            && (MIN_REPEAT_COUNT..=MAX_REPEAT_COUNT).contains(&count);
        prop_assert_eq!(validate_repeat(Repeat { every_weeks, count }).is_ok(), inside);
        prop_assert_eq!(series_dates(TODAY, Repeat { every_weeks, count }).is_ok(), inside);
    }

    #[test]
    fn following_keeps_the_spacing_of_the_series(
        every_weeks in 1_u8..=4,
        count in 2_u8..=12,
        edit_at in 0_usize..12,
        shift in -20_i64..20,
        start in 0_u16..1_000,
    ) {
        let dates = series_dates("2026-06-07", Repeat { every_weeks, count }).unwrap();
        let series: Vec<Visit> = dates
            .iter()
            .enumerate()
            .map(|(n, date)| series_visit(&format!("v{n:02}"), date, VisitStatus::Planned))
            .collect();
        let previous = series[edit_at % series.len()].clone();
        let edited = Visit {
            date: add_days(&previous.date, shift).unwrap(),
            start,
            ..previous.clone()
        };
        let later = apply_following(&edited, &previous, &series).unwrap();
        // Every later visit, and only those.
        prop_assert_eq!(later.len(), series.len() - 1 - edit_at % series.len());
        let mut before = &edited;
        for moved in &later {
            let was = series.iter().find(|v| v.id == moved.id).unwrap();
            prop_assert_eq!(days_between(&was.date, &moved.date), Some(shift));
            prop_assert_eq!(moved.start, start);
            prop_assert_eq!(days_between(&before.date, &moved.date), Some(7 * i64::from(every_weeks)));
            before = moved;
        }
    }

    #[test]
    fn the_balance_adds_up(visits in any_visits(), charges in any_charges(), payments in any_payments()) {
        // Small enough here to always fit.
        let b = balance(&visits, &charges, &payments).unwrap();
        prop_assert_eq!(i64::from(b.due), i64::from(b.charged) - i64::from(b.paid));
        prop_assert!(b.charged >= 0 && b.paid >= 0);
        let by_hand: i64 = visits.iter().map(|v| i64::from(visit_charge(v))).sum::<i64>()
            + charges.iter().filter(|c| !c.voided).map(|c| i64::from(c.amount)).sum::<i64>();
        prop_assert_eq!(i64::from(b.charged), by_hand);

        // The order of the records does not matter.
        let mut reversed = (visits.clone(), charges.clone(), payments.clone());
        reversed.0.reverse();
        reversed.1.reverse();
        reversed.2.reverse();
        prop_assert_eq!(balance(&reversed.0, &reversed.1, &reversed.2).unwrap(), b);

        // Voided records change nothing.
        let live_charges: Vec<Charge> = charges.iter().filter(|c| !c.voided).cloned().collect();
        let live_payments: Vec<Payment> = payments.iter().filter(|p| !p.voided).cloned().collect();
        prop_assert_eq!(balance(&visits, &live_charges, &live_payments).unwrap(), b);

        // One more payment lowers what is owed by exactly its amount.
        let mut more = payments.clone();
        more.push(payment("extra", TODAY, 12_345));
        let after = balance(&visits, &charges, &more).unwrap();
        prop_assert_eq!((after.charged, after.paid, after.due), (b.charged, b.paid + 12_345, b.due - 12_345));
    }

    #[test]
    fn something_is_uncovered_exactly_when_something_is_owed(
        visits in any_visits(),
        charges in any_charges(),
        payments in any_payments(),
    ) {
        let b = balance(&visits, &charges, &payments).unwrap();
        let oldest = oldest_uncovered(&visits, &charges, &payments);
        prop_assert_eq!(oldest.is_some(), b.due > 0);
        if let Some(date) = &oldest {
            // It is the date of a real charge, and everything older is covered by what was paid.
            let dated = visits.iter().filter(|v| visit_charge(v) > 0).map(|v| &v.date)
                .chain(charges.iter().filter(|c| !c.voided).map(|c| &c.date));
            prop_assert!(dated.clone().any(|d| d == date));
            let older: i64 = visits.iter().filter(|v| &v.date < date).map(|v| i64::from(visit_charge(v))).sum::<i64>()
                + charges.iter().filter(|c| !c.voided && &c.date < date).map(|c| i64::from(c.amount)).sum::<i64>();
            let through: i64 = visits.iter().filter(|v| &v.date <= date).map(|v| i64::from(visit_charge(v))).sum::<i64>()
                + charges.iter().filter(|c| !c.voided && &c.date <= date).map(|c| i64::from(c.amount)).sum::<i64>();
            prop_assert!(older <= i64::from(b.paid) && i64::from(b.paid) < through);
        }
        // More money never makes the uncovered charge older.
        let mut more = payments.clone();
        more.push(payment("extra", TODAY, 50_000));
        let after = oldest_uncovered(&visits, &charges, &more);
        match (&oldest, &after) {
            (Some(was), Some(now)) => prop_assert!(now >= was),
            (None, Some(_)) => prop_assert!(false, "a payment created a debt"),
            _ => {}
        }
    }

    #[test]
    fn overlaps_are_mutual_and_match_the_pairs(visits in any_visits()) {
        let mut by_pairs = BTreeSet::new();
        for (n, a) in visits.iter().enumerate() {
            prop_assert_eq!(visits_overlap(a, a), a.status != VisitStatus::Cancelled);
            for b in &visits[n + 1..] {
                prop_assert_eq!(visits_overlap(a, b), visits_overlap(b, a));
                if visits_overlap(a, b) {
                    by_pairs.insert(a.id.clone());
                    by_pairs.insert(b.id.clone());
                }
            }
        }
        let ids = overlapping_ids(&visits);
        prop_assert_eq!(&ids, &by_pairs);
        prop_assert!(visits.iter().all(|v| v.status != VisitStatus::Cancelled || !ids.contains(&v.id)));
        let mut reversed = visits.clone();
        reversed.reverse();
        prop_assert_eq!(overlapping_ids(&reversed), ids);
    }

    #[test]
    fn money_reads_back_to_the_same_agorot(amount: i32) {
        let text = money(amount);
        let (whole, part) = text.split_once('.').unwrap_or((text.as_str(), "00"));
        prop_assert_eq!(part.len(), 2);
        let negative = whole.starts_with('-');
        let whole: i64 = whole.trim_start_matches('-').parse().unwrap();
        let part: i64 = part.parse().unwrap();
        let back = (whole * 100 + part) * if negative { -1 } else { 1 };
        prop_assert_eq!(back, i64::from(amount));
        prop_assert_eq!(text.contains('.'), amount % 100 != 0);
    }

    #[test]
    fn the_time_reads_back_to_the_same_minute(minute in 0_u16..1_440) {
        let text = display_time(minute);
        prop_assert_eq!(text.len(), 5);
        let (hours, minutes) = text.split_once(':').unwrap();
        prop_assert_eq!(hours.parse::<u16>().unwrap() * 60 + minutes.parse::<u16>().unwrap(), minute);
    }

    #[test]
    fn a_text_without_placeholders_is_left_alone(text in "[א-ת a-z0-9.,!?\n}]{0,60}") {
        prop_assert_eq!(fill_message(&text, &values()), text);
    }

    #[test]
    fn a_filled_message_keeps_what_was_around_the_placeholders(
        before in "[א-ת ,.]{0,20}",
        after in "[א-ת ,.]{0,20}",
        pick in 0_usize..8,
    ) {
        let placeholder = PLACEHOLDERS[pick];
        let filled = fill_message(&format!("{before}{placeholder}{after}"), &values());
        prop_assert!(filled.starts_with(&before) && filled.ends_with(&after));
        let brace_left = filled.contains('{');
        prop_assert!(!brace_left);
        prop_assert!(filled.chars().count() > before.chars().count() + after.chars().count());
    }

    #[test]
    fn cleaning_a_line_is_stable(text in "[א-ת a-z\t\n ]{0,40}") {
        let clean = clean_line(&text);
        prop_assert_eq!(clean_line(&clean), clean.clone());
        prop_assert!(!clean.starts_with(' ') && !clean.ends_with(' ') && !clean.contains("  "));
        prop_assert!(!clean.contains('\n') && !clean.contains('\t'));
    }

    #[test]
    fn tasks_are_ordered_open_first_and_by_date(
        rows in prop::collection::vec((prop::option::of(0_i64..60), prop::option::of(0_i64..60)), 0..12),
    ) {
        let tasks: Vec<ClinicTask> = rows
            .iter()
            .enumerate()
            .map(|(n, (due, done))| ClinicTask {
                done_on: done.map(|d| add_days("2026-10-15", d).unwrap()),
                ..task(&format!("t{n}"), None, due.map(|d| add_days("2026-10-15", d).unwrap()).as_deref())
            })
            .collect();
        let views = task_views(&tasks, &HashMap::new(), TODAY);
        prop_assert_eq!(views.len(), tasks.len());
        for pair in views.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            // Open before done; overdue before the rest; dated by date; undated last.
            prop_assert!(a.task.done_on.is_none() || b.task.done_on.is_some());
            prop_assert!(a.overdue || !b.overdue);
            if a.task.done_on.is_none() && b.task.done_on.is_none() {
                match (&a.task.due, &b.task.due) {
                    (Some(x), Some(y)) => prop_assert!(x <= y),
                    (None, Some(_)) => prop_assert!(false, "an undated task before a dated one"),
                    _ => {}
                }
            }
        }
        for view in &views {
            let late = view.task.done_on.is_none()
                && view.task.due.as_deref().is_some_and(|due| due < TODAY);
            prop_assert_eq!(view.overdue, late);
        }
    }

    #[test]
    fn the_diary_and_the_year_agree_with_the_balance(
        visits in any_visits(),
        charges in any_charges(),
        payments in any_payments(),
    ) {
        // Everything generated falls in 2026, from August to November.
        let b = balance(&visits, &charges, &payments).unwrap();
        let year = income_year(2026, &visits, &charges, &payments, &[]).unwrap();
        prop_assert_eq!((year.charged, year.received), (b.charged, b.paid));
        prop_assert_eq!(year.months.iter().map(|m| i64::from(m.charged)).sum::<i64>(), i64::from(b.charged));
        prop_assert_eq!(year.months.iter().map(|m| i64::from(m.received)).sum::<i64>(), i64::from(b.paid));
        let held = visits.iter().filter(|v| v.status == VisitStatus::Held).count();
        prop_assert_eq!(year.months.iter().map(|m| m.held as usize).sum::<usize>(), held);
        prop_assert_eq!(year.by_kind.iter().map(|k| k.count as usize).sum::<usize>(), held);

        let (mut charged, mut received, mut lines) = (0_i64, 0_i64, 0_usize);
        for month in ["2026-08", "2026-09", "2026-10", "2026-11"] {
            let diary = diary_month(month, &visits, &charges, &payments, &names(), &HashSet::new()).unwrap();
            charged += i64::from(diary.charged);
            received += i64::from(diary.received);
            prop_assert_eq!(diary.days.iter().map(|d| i64::from(d.charged)).sum::<i64>(), i64::from(diary.charged));
            prop_assert_eq!(diary.days.iter().map(|d| i64::from(d.received)).sum::<i64>(), i64::from(diary.received));
            prop_assert_eq!(diary.by_method.iter().map(|m| i64::from(m.amount)).sum::<i64>(), i64::from(diary.received));
            prop_assert!(diary.days.windows(2).all(|d| d[0].date < d[1].date));
            prop_assert!(diary.days.iter().all(|d| !d.lines.is_empty()));
            lines += diary.days.iter().map(|d| d.lines.len()).sum::<usize>();
        }
        prop_assert_eq!((charged, received), (i64::from(b.charged), i64::from(b.paid)));
        // Every record is a line, the voided ones too.
        prop_assert_eq!(lines, visits.len() + charges.len() + payments.len());
    }

    #[test]
    fn alerts_come_oldest_first_and_never_name_the_patient(
        visits in any_visits(),
        charges in any_charges(),
        payments in any_payments(),
        minute in 0_u16..1_440,
        report in prop::option::of((0_u32..20, 0_u32..20)),
    ) {
        let p = patient("p1", "נועם");
        let records = PatientRecords {
            patient: &p,
            visits: &visits,
            notes: &[],
            charges: &charges,
            payments: &payments,
            tasks: &[],
            report,
        };
        let now = Today { date: "2026-11-16".to_owned(), minute };
        let alerts = patient_alerts(&records, &now, &ClinicSettings::default());
        prop_assert!(alerts.windows(2).all(|a| a[0].since <= a[1].since));
        for alert in &alerts {
            prop_assert!(!alert.text.contains("נועם"));
            prop_assert!(!alert.text.contains('—'));
            prop_assert!(parse_date(&alert.since).is_some());
        }
        let debt = alerts.iter().filter(|a| a.kind == AlertKind::Debt).count();
        prop_assert!(debt <= 1);
        if balance(&visits, &charges, &payments).unwrap().due <= 0 {
            prop_assert_eq!(debt, 0);
        }
    }
}

// ------------------------------------------------------------------ the history of a note

#[test]
fn a_notes_history_shows_the_record_and_never_the_personal_text() {
    let first = SessionNote {
        record: "הילד שיתף פעולה.\n\n  עבד   בריכוז. ".to_owned(),
        personal: "מחשבה-פרטית-בדויה".to_owned(),
        ..note("n1", Some("v1"), TODAY)
    };
    let lines = note_history_lines(&first, "נועם", None);
    assert_eq!(
        lines,
        [
            "מטופל: נועם",
            "תאריך: 15.11.2026",
            "רשומה:",
            "הילד שיתף פעולה.",
            "עבד בריכוז.",
            "תרשומת אישית: נכתבה",
        ]
    );

    let same_personal = SessionNote {
        record: "נוסח מתוקן.".to_owned(),
        ..first.clone()
    };
    let changed_personal = SessionNote {
        personal: "מחשבה-אחרת-בדויה".to_owned(),
        ..first.clone()
    };
    let no_personal = SessionNote {
        personal: String::new(),
        ..first.clone()
    };
    let sent = SessionNote {
        case_input_id: Some("i1".to_owned()),
        ..no_personal.clone()
    };
    let last = |note: &SessionNote, before: Option<&SessionNote>| {
        let lines = note_history_lines(note, "נועם", before);
        for line in &lines {
            assert!(!line.contains("בדויה"), "the personal text in: {line}");
            assert_hebrew_sentence(line);
        }
        lines.last().cloned().unwrap()
    };
    assert_eq!(
        last(&same_personal, Some(&first)),
        "תרשומת אישית: בלי שינוי"
    );
    assert_eq!(
        last(&changed_personal, Some(&first)),
        "תרשומת אישית: השתנתה"
    );
    assert_eq!(last(&no_personal, Some(&first)), "תרשומת אישית: נמחקה");
    assert_eq!(last(&first, Some(&no_personal)), "תרשומת אישית: נכתבה");
    assert_eq!(last(&no_personal, Some(&no_personal)), "תרשומת אישית: אין");
    assert_eq!(last(&no_personal, None), "תרשומת אישית: אין");
    assert_eq!(last(&sent, Some(&no_personal)), "הועבר לחומרי התיק: כן");
    // Exactly one line speaks of the personal note, whatever happened to it.
    for (state, before) in [
        (&first, None),
        (&changed_personal, Some(&first)),
        (&sent, Some(&first)),
    ] {
        let about = note_history_lines(state, "", before)
            .iter()
            .filter(|l| l.contains("תרשומת אישית"))
            .count();
        assert_eq!(about, 1);
    }

    let empty_record = SessionNote {
        record: String::new(),
        ..first
    };
    assert!(note_history_lines(&empty_record, "", None).contains(&"רשומה: ריקה".to_owned()));
}

#[test]
fn a_note_on_its_own_has_a_title_for_the_case_too() {
    assert_eq!(
        note_case_title_for(Some(VisitKind::Intake), TODAY),
        note_case_title(VisitKind::Intake, TODAY)
    );
    assert_eq!(note_case_title_for(None, TODAY), "סיכום שיחה · 15.11.2026");
}

// ------------------------------------------------------------------ erasing a patient (D-174)

const KEEP_YEARS: i32 = 7;
const KEEP_AGE: i32 = 25;

struct Card {
    patient: Patient,
    visits: Vec<Visit>,
    notes: Vec<SessionNote>,
    charges: Vec<Charge>,
    payments: Vec<Payment>,
    tasks: Vec<ClinicTask>,
}

impl Card {
    fn new() -> Self {
        Self {
            patient: patient("p1", "נועם"),
            visits: Vec::new(),
            notes: Vec::new(),
            charges: Vec::new(),
            payments: Vec::new(),
            tasks: Vec::new(),
        }
    }

    fn erase(&self, today: &str, last_saved: Option<&str>) -> Result<(), DomainError> {
        let records = PatientRecords {
            patient: &self.patient,
            visits: &self.visits,
            notes: &self.notes,
            charges: &self.charges,
            payments: &self.payments,
            tasks: &self.tasks,
            report: None,
        };
        can_erase(&records, today, last_saved, KEEP_YEARS, KEEP_AGE)
    }
}

#[test]
fn a_card_where_nothing_happened_can_be_erased_at_once() {
    let mut card = Card::new();
    card.erase(TODAY, None).unwrap();
    // Plans, a cancellation that is not charged, a missed visit that is not charged and a
    // task are not something that happened.
    card.visits = vec![
        paid_visit("v1", "2026-11-20", VisitStatus::Planned, 40_000),
        paid_visit("v2", "2026-11-10", VisitStatus::Cancelled, 40_000),
        paid_visit("v3", "2026-11-11", VisitStatus::NoShow, 40_000),
    ];
    card.tasks = vec![task("t1", Some("p1"), Some(TODAY))];
    card.erase(TODAY, Some(TODAY)).unwrap();
    // Even a date that cannot be read does not stand in the way of an empty card.
    card.erase("היום", None).unwrap();
    // Another patient's records are not this card's.
    card.payments = vec![Payment {
        patient_id: "p2".to_owned(),
        ..payment("pay", TODAY, 10_000)
    }];
    card.notes = vec![SessionNote {
        patient_id: "p2".to_owned(),
        ..note("n", None, TODAY)
    }];
    card.visits.push(Visit {
        patient_id: Some("p2".to_owned()),
        ..paid_visit("v4", TODAY, VisitStatus::Held, 40_000)
    });
    card.erase(TODAY, None).unwrap();
}

#[test]
fn anything_that_happened_keeps_the_card_voided_records_too() {
    type Fill = Box<dyn Fn(&mut Card)>;
    let cases: Vec<(&str, Fill)> = vec![
        (
            "a held visit, even one that cost nothing",
            Box::new(|c| c.visits = vec![paid_visit("v", TODAY, VisitStatus::Held, 0)]),
        ),
        (
            "a cancelled visit that is charged",
            Box::new(|c| {
                c.visits = vec![Visit {
                    charge_missed: true,
                    ..paid_visit("v", TODAY, VisitStatus::Cancelled, 40_000)
                }];
            }),
        ),
        (
            "a missed visit that is charged",
            Box::new(|c| {
                c.visits = vec![Visit {
                    charge_missed: true,
                    ..paid_visit("v", TODAY, VisitStatus::NoShow, 40_000)
                }];
            }),
        ),
        (
            "a note",
            Box::new(|c| c.notes = vec![note("n", None, TODAY)]),
        ),
        (
            "a charge",
            Box::new(|c| c.charges = vec![charge("c", TODAY, 10_000)]),
        ),
        (
            "a voided charge",
            Box::new(|c| {
                c.charges = vec![Charge {
                    voided: true,
                    void_reason: "טעות".to_owned(),
                    ..charge("c", TODAY, 10_000)
                }];
            }),
        ),
        (
            "a payment",
            Box::new(|c| c.payments = vec![payment("p", TODAY, 10_000)]),
        ),
        (
            "a voided payment",
            Box::new(|c| {
                c.payments = vec![Payment {
                    voided: true,
                    void_reason: "טעות".to_owned(),
                    ..payment("p", TODAY, 10_000)
                }];
            }),
        ),
    ];
    for (what, fill) in cases {
        let mut card = Card::new();
        fill(&mut card);
        let text = refusal(card.erase(TODAY, None));
        assert!(text.contains("לסגור את הכרטיס"), "{what}: {text}");
        // Born 2020-07-20: 25 on 2045-07-20, later than seven years from today.
        assert!(text.contains("20.07.2045"), "{what}: {text}");
    }
}

#[test]
fn a_card_can_be_erased_once_the_retention_period_has_passed() {
    let mut card = Card::new();
    card.patient.birth_date = None;
    card.patient.created_on = "2026-09-01".to_owned();
    card.visits = vec![paid_visit("v", "2026-11-15", VisitStatus::Held, 40_000)];
    // No birth date: seven years after the last activity.
    assert!(refusal(card.erase("2033-11-14", None)).contains("15.11.2033"));
    card.erase("2033-11-15", None).unwrap();
    card.erase("2040-01-01", None).unwrap();

    // The last activity is the latest date anything in the card carries.
    card.payments = vec![payment("p", "2027-02-01", 40_000)];
    assert!(refusal(card.erase("2033-11-15", None)).contains("01.02.2034"));
    card.tasks = vec![ClinicTask {
        done_on: Some("2027-03-10".to_owned()),
        ..task("t", Some("p1"), Some("2027-03-01"))
    }];
    assert!(refusal(card.erase("2034-02-01", None)).contains("10.03.2034"));
    // A record corrected later was active on the day it was written.
    assert!(refusal(card.erase("2034-03-10", Some("2030-06-30"))).contains("30.06.2037"));
    card.erase("2037-06-30", Some("2030-06-30")).unwrap();
    // A saved day that cannot be read is not counted, and does not break the rule.
    card.erase("2034-03-10", Some("לא תאריך")).unwrap();

    // A known birth date: not before the child is 25, when that is later.
    card.patient.birth_date = Some("2020-07-20".to_owned());
    assert!(refusal(card.erase("2045-07-19", None)).contains("20.07.2045"));
    card.erase("2045-07-20", None).unwrap();
    // An older child: seven years after the last activity is the later of the two.
    card.patient.birth_date = Some("2005-01-01".to_owned());
    assert!(refusal(card.erase("2034-03-09", None)).contains("10.03.2034"));
    card.erase("2034-03-10", None).unwrap();
    // Born on a leap day: 25 on the last day of that February.
    card.patient.birth_date = Some("2024-02-29".to_owned());
    assert!(refusal(card.erase("2049-02-27", None)).contains("28.02.2049"));
    card.erase("2049-02-28", None).unwrap();

    // Today itself has to be a date when something happened in the card.
    refusal(card.erase("מחר", None));
}

// ------------------------------------------------------------------ earlier shapes (D-175)

#[test]
fn a_record_saved_in_an_earlier_shape_still_loads() {
    // Every field missing, and one this version does not know.
    let bare = r#"{"added_in_a_later_version":{"x":[1,2]}}"#;
    assert_eq!(
        serde_json::from_str::<Patient>(bare).unwrap(),
        Patient::default()
    );
    assert_eq!(
        serde_json::from_str::<Contact>(bare).unwrap(),
        Contact::default()
    );
    assert_eq!(
        serde_json::from_str::<Visit>(bare).unwrap(),
        Visit::default()
    );
    assert_eq!(
        serde_json::from_str::<SessionNote>(bare).unwrap(),
        SessionNote::default()
    );
    assert_eq!(
        serde_json::from_str::<Payment>(bare).unwrap(),
        Payment::default()
    );
    assert_eq!(
        serde_json::from_str::<Charge>(bare).unwrap(),
        Charge::default()
    );
    assert_eq!(
        serde_json::from_str::<ClinicTask>(bare).unwrap(),
        ClinicTask::default()
    );
    assert_eq!(
        serde_json::from_str::<ClinicSettings>(bare).unwrap(),
        ClinicSettings::default()
    );
    assert_eq!(
        serde_json::from_str::<Patient>("{}").unwrap(),
        Patient::default()
    );

    // The defaults a missing field takes are the ones the rules expect.
    let visit = Visit::default();
    assert_eq!(visit.status, VisitStatus::Planned);
    assert_eq!((visit.fee, visit.charge_missed), (0, false));
    assert_eq!(Patient::default().status, PatientStatus::Active);
    assert!(!Payment::default().voided && !Charge::default().voided);
}

#[test]
fn a_record_with_some_fields_keeps_them_and_defaults_the_rest() {
    let patient: Patient = serde_json::from_str(
        r#"{"id":"p1","name":"נועם","contacts":[{"name":"דנה","nickname":"דני"}],"shoe_size":30}"#,
    )
    .unwrap();
    assert_eq!(
        patient,
        Patient {
            id: "p1".to_owned(),
            name: "נועם".to_owned(),
            contacts: vec![Contact {
                name: "דנה".to_owned(),
                ..Contact::default()
            }],
            ..Patient::default()
        }
    );
    let visit: Visit =
        serde_json::from_str(r#"{"id":"v1","kind":"assessment","date":"2026-11-15","start":540}"#)
            .unwrap();
    assert_eq!(
        (visit.kind, visit.start, visit.minutes, visit.status),
        (VisitKind::Assessment, 540, 0, VisitStatus::Planned)
    );
    let note: SessionNote =
        serde_json::from_str(r#"{"patient_id":"p1","record":"רשומה","mood":"טוב"}"#).unwrap();
    assert_eq!(
        (note.record.as_str(), note.personal.as_str(), note.visit_id),
        ("רשומה", "", None)
    );
    let payment: Payment = serde_json::from_str(r#"{"amount":45000,"currency":"ILS"}"#).unwrap();
    assert_eq!(
        (payment.amount, payment.method, payment.voided),
        (45_000, PayMethod::Other, false)
    );
    let charge: Charge = serde_json::from_str(r#"{"amount":1,"what":"דוח"}"#).unwrap();
    assert_eq!((charge.amount, charge.what.as_str()), (1, "דוח"));
    let task: ClinicTask = serde_json::from_str(r#"{"title":"להתקשר","priority":3}"#).unwrap();
    assert_eq!((task.kind, task.done_on), (TaskKind::Other, None));
    // Settings saved before a field existed take that field's default, not an empty one.
    let settings: ClinicSettings =
        serde_json::from_str(r#"{"report_days":30,"theme":"dark"}"#).unwrap();
    let defaults = ClinicSettings::default();
    assert_eq!(settings.report_days, 30);
    assert_eq!(settings.defaults, defaults.defaults);
    assert_eq!(settings.reminder_text, defaults.reminder_text);
    validate_settings(&settings).unwrap();

    // What is written today reads back as it is.
    let full = patient_full();
    let json = serde_json::to_string(&full).unwrap();
    assert_eq!(serde_json::from_str::<Patient>(&json).unwrap(), full);
}

fn patient_full() -> Patient {
    Patient {
        gender: Some(crate::records::GrammaticalGender::Female),
        waiting_since: Some("2026-09-01".to_owned()),
        urgent: true,
        session_fee: Some(45_000),
        case_id: Some("c1".to_owned()),
        status: PatientStatus::Waiting,
        ..patient("p1", "תמר")
    }
}

// ------------------------------------------------------------------ recurrence: the edges

#[test]
fn following_after_a_later_visit_was_moved_by_hand() {
    let mut series = vec![
        series_visit("a", "2026-11-01", VisitStatus::Planned),
        series_visit("b", "2026-11-08", VisitStatus::Planned),
        series_visit("c", "2026-11-15", VisitStatus::Planned),
        series_visit("d", "2026-11-22", VisitStatus::Planned),
    ];
    // She moved the third one by itself: two days later, to the afternoon.
    series[2].date = "2026-11-17".to_owned();
    series[2].start = 900;
    let previous = series[0].clone();
    let edited = Visit {
        date: "2026-11-02".to_owned(), // one day later
        start: 630,
        ..previous.clone()
    };
    let later = apply_following(&edited, &previous, &series).unwrap();
    let moved: Vec<(&str, &str, u16)> = later
        .iter()
        .map(|v| (v.id.as_str(), v.date.as_str(), v.start))
        .collect();
    // Each one moves by the same day from where it stands (her own move is kept), and all
    // of them take the new hour.
    assert_eq!(
        moved,
        [
            ("b", "2026-11-09", 630),
            ("c", "2026-11-18", 630),
            ("d", "2026-11-23", 630),
        ]
    );

    // A visit she moved to before the edited one is no longer "later", and stays.
    series[3].date = "2026-10-30".to_owned();
    let later = apply_following(&edited, &previous, &series).unwrap();
    let ids: Vec<&str> = later.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["b", "c"]);
}

#[test]
fn following_leaves_a_visit_that_was_already_held_as_it_was() {
    let series = vec![
        series_visit("a", "2026-11-01", VisitStatus::Planned),
        series_visit("b", "2026-11-08", VisitStatus::Held),
        series_visit("c", "2026-11-15", VisitStatus::NoShow),
        series_visit("d", "2026-11-22", VisitStatus::Planned),
    ];
    let previous = series[0].clone();
    let edited = Visit {
        fee: 50_000,
        minutes: 45,
        ..previous.clone()
    };
    let later = apply_following(&edited, &previous, &series).unwrap();
    assert_eq!(later.len(), 1);
    assert_eq!(
        (later[0].id.as_str(), later[0].fee, later[0].minutes),
        ("d", 50_000, 45)
    );
    // The edited one itself being held changes nothing for the later ones.
    let held = Visit {
        status: VisitStatus::Held,
        ..edited
    };
    assert_eq!(apply_following(&held, &previous, &series).unwrap(), later);
}

#[test]
fn following_moves_across_a_month_end_and_a_year_end() {
    let series = vec![
        series_visit("a", "2027-01-25", VisitStatus::Planned),
        series_visit("b", "2027-02-22", VisitStatus::Planned),
        series_visit("c", "2027-12-27", VisitStatus::Planned),
        // 2028 is a leap year.
        series_visit("d", "2028-02-23", VisitStatus::Planned),
    ];
    let previous = series[0].clone();
    let edited = Visit {
        date: "2027-02-01".to_owned(), // a week later, into the next month
        ..previous.clone()
    };
    let dates: Vec<String> = apply_following(&edited, &previous, &series)
        .unwrap()
        .into_iter()
        .map(|v| v.date)
        .collect();
    assert_eq!(dates, ["2027-03-01", "2028-01-03", "2028-03-01"]);

    // And backwards, into the year before.
    let series = vec![
        series_visit("a", "2027-01-04", VisitStatus::Planned),
        series_visit("b", "2027-01-05", VisitStatus::Planned),
    ];
    let edited = Visit {
        date: "2026-12-28".to_owned(),
        ..series[0].clone()
    };
    let later = apply_following(&edited, &series[0], &series).unwrap();
    assert_eq!(later[0].date, "2026-12-29");
}

#[test]
fn a_four_oclock_visit_stays_at_four_across_the_clock_changes() {
    // Israel moves the clock on Friday 2027-03-26 and on Sunday 2027-10-24. A visit is a
    // date and minutes from midnight, so a series that crosses either day keeps its hour.
    for (first, across) in [("2027-03-19", "2027-03-26"), ("2027-10-17", "2027-10-24")] {
        let dates = series_dates(
            first,
            Repeat {
                every_weeks: 1,
                count: 4,
            },
        )
        .unwrap();
        assert_eq!(dates[1], across);
        let series: Vec<Visit> = dates
            .iter()
            .enumerate()
            .map(|(n, date)| Visit {
                start: 16 * 60,
                ..series_visit(&format!("v{n}"), date, VisitStatus::Planned)
            })
            .collect();
        for (n, v) in series.iter().enumerate() {
            assert_eq!(display_time(v.start), "16:00");
            assert_eq!(days_between(first, &v.date), Some(7 * n as i64));
            assert_eq!(weekday(&v.date), weekday(first));
        }
        // Moving the series by a day keeps the hour on both sides of the change too.
        let edited = Visit {
            date: add_days(first, 1).unwrap(),
            ..series[0].clone()
        };
        let later = apply_following(&edited, &series[0], &series).unwrap();
        assert_eq!(later.len(), 3);
        assert!(later.iter().all(|v| v.start == 16 * 60 && v.minutes == 60));
        assert_eq!(later[0].date, add_days(across, 1).unwrap());
    }
}
