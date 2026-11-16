//! The clinic (beta, D-170), through `Core::clinic` only, on an unlocked test vault.
//! Every family, name, phone, date and sum here is invented.

use dv_domain::clinic::{
    self as rules, AlertKind, CaseLink, Charge, ClinicReply, ClinicRequest, ClinicSettings,
    ClinicTask, Contact, ContactRole, DiaryMonth, HistoryEntry, IncomeYear, MessageKind, Overview,
    Patient, PatientCard, PatientRow, PatientStatus, PayMethod, Payment, Repeat, SavedVisits,
    Scope, SessionNote, StepState, TaskKind, TaskView, Today, Visit, VisitKind, VisitStatus,
    VisitView,
};
use dv_domain::IdentitySource;

use super::*;

/// The day the window calls "today" in these tests: a Sunday.
const TODAY: &str = "2026-11-15";

fn now() -> Today {
    Today {
        date: TODAY.to_owned(),
        minute: 12 * 60,
    }
}

fn open_clinic() -> (tempfile::TempDir, Core) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    core.set_practitioner(vec!["ד\"ר רותם אלמוג".into(), "רותם".into()])
        .unwrap();
    (dir, core)
}

fn ask(core: &mut Core, request: ClinicRequest) -> ClinicReply {
    core.clinic(request).unwrap()
}

/// What every text she sees has to be: Hebrew, and without an em-dash.
fn hebrew(text: &str) {
    assert!(
        text.chars().any(|c| ('א'..='ת').contains(&c)),
        "not Hebrew: {text}"
    );
    assert!(!text.contains('—'), "an em-dash in: {text}");
}

/// The refusal's sentence; panics when the request went through.
fn refused(core: &mut Core, request: ClinicRequest) -> String {
    match core.clinic(request) {
        Err(CoreError::Refused(text)) => {
            hebrew(&text);
            text
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// What was not found, as she is told.
fn not_found(core: &mut Core, request: ClinicRequest) -> String {
    match core.clinic(request) {
        Err(e @ CoreError::NotFound(_)) => {
            let ui = e.to_ui();
            assert_eq!(ui.code, "not_found");
            hebrew(&ui.message);
            ui.message
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

fn contact(role: ContactRole, name: &str) -> Contact {
    Contact {
        role,
        name: name.to_owned(),
        phone: "000-0000".to_owned(),
        email: String::new(),
    }
}

fn child(name: &str) -> Patient {
    Patient {
        name: name.to_owned(),
        birth_date: Some("2021-03-10".to_owned()),
        contacts: vec![contact(ContactRole::Mother, "דנה")],
        status: PatientStatus::Active,
        created_on: "2026-09-01".to_owned(),
        ..Patient::default()
    }
}

fn save_patient(core: &mut Core, patient: Patient) -> Patient {
    match ask(core, ClinicRequest::SavePatient { patient }) {
        ClinicReply::Patient(p) => p,
        other => panic!("{other:?}"),
    }
}

fn new_patient(core: &mut Core, name: &str) -> Patient {
    save_patient(core, child(name))
}

fn a_visit(patient: &Patient, kind: VisitKind, date: &str, start: u16) -> Visit {
    Visit {
        patient_id: Some(patient.id.clone()),
        kind,
        date: date.to_owned(),
        start,
        minutes: 60,
        place: "הקליניקה".to_owned(),
        ..Visit::default()
    }
}

fn save_visits(core: &mut Core, visit: Visit, repeat: Option<Repeat>, scope: Scope) -> SavedVisits {
    let request = ClinicRequest::SaveVisit {
        visit,
        repeat,
        scope,
    };
    match ask(core, request) {
        ClinicReply::Saved(saved) => saved,
        other => panic!("{other:?}"),
    }
}

fn save_visit(core: &mut Core, visit: Visit) -> Visit {
    let mut saved = save_visits(core, visit, None, Scope::One).visits;
    assert_eq!(saved.len(), 1);
    saved.remove(0).visit
}

fn mark(core: &mut Core, visit: &Visit, status: VisitStatus) -> Visit {
    save_visit(
        core,
        Visit {
            status,
            ..visit.clone()
        },
    )
}

fn save_note(core: &mut Core, note: SessionNote) -> SessionNote {
    match ask(core, ClinicRequest::SaveNote { note }) {
        ClinicReply::Note(n) => n,
        other => panic!("{other:?}"),
    }
}

fn a_note(patient: &Patient, visit: Option<&Visit>, record: &str) -> SessionNote {
    SessionNote {
        patient_id: patient.id.clone(),
        visit_id: visit.map(|v| v.id.clone()),
        date: visit.map_or_else(|| TODAY.to_owned(), |v| v.date.clone()),
        record: record.to_owned(),
        ..SessionNote::default()
    }
}

fn a_payment(patient: &Patient, date: &str, amount: i32) -> Payment {
    Payment {
        patient_id: patient.id.clone(),
        date: date.to_owned(),
        amount,
        method: PayMethod::Bit,
        receipt: "101".to_owned(),
        ..Payment::default()
    }
}

fn save_payment(core: &mut Core, payment: Payment) -> Payment {
    match ask(core, ClinicRequest::SavePayment { payment }) {
        ClinicReply::Payment(p) => p,
        other => panic!("{other:?}"),
    }
}

fn a_charge(patient: &Patient, date: &str, amount: i32) -> Charge {
    Charge {
        patient_id: patient.id.clone(),
        date: date.to_owned(),
        amount,
        what: "חבילת אבחון".to_owned(),
        ..Charge::default()
    }
}

fn save_charge(core: &mut Core, charge: Charge) -> Charge {
    match ask(core, ClinicRequest::SaveCharge { charge }) {
        ClinicReply::Charge(c) => c,
        other => panic!("{other:?}"),
    }
}

fn save_task(core: &mut Core, task: ClinicTask) -> ClinicTask {
    match ask(core, ClinicRequest::SaveTask { task }) {
        ClinicReply::Task(t) => t,
        other => panic!("{other:?}"),
    }
}

fn card(core: &mut Core, id: &str) -> PatientCard {
    let request = ClinicRequest::PatientCard {
        id: id.to_owned(),
        today: now(),
    };
    match ask(core, request) {
        ClinicReply::Card(card) => *card,
        other => panic!("{other:?}"),
    }
}

fn overview(core: &mut Core) -> Overview {
    match ask(core, ClinicRequest::Overview { today: now() }) {
        ClinicReply::Overview(o) => o,
        other => panic!("{other:?}"),
    }
}

fn rows(core: &mut Core) -> Vec<PatientRow> {
    match ask(core, ClinicRequest::Patients { today: now() }) {
        ClinicReply::Patients(rows) => rows,
        other => panic!("{other:?}"),
    }
}

fn calendar(core: &mut Core, from: &str, to: &str) -> Vec<VisitView> {
    let request = ClinicRequest::Calendar {
        from: from.to_owned(),
        to: to.to_owned(),
    };
    match ask(core, request) {
        ClinicReply::Visits(v) => v,
        other => panic!("{other:?}"),
    }
}

fn history(core: &mut Core, record_id: &str) -> Vec<HistoryEntry> {
    let request = ClinicRequest::History {
        record_id: record_id.to_owned(),
    };
    match ask(core, request) {
        ClinicReply::History(h) => h,
        other => panic!("{other:?}"),
    }
}

fn actions(core: &mut Core, record_id: &str) -> Vec<String> {
    history(core, record_id)
        .into_iter()
        .map(|entry| entry.action)
        .collect()
}

fn diary(core: &mut Core, month: &str) -> DiaryMonth {
    let request = ClinicRequest::Diary {
        month: month.to_owned(),
    };
    match ask(core, request) {
        ClinicReply::Diary(d) => d,
        other => panic!("{other:?}"),
    }
}

fn income(core: &mut Core, year: u16) -> IncomeYear {
    match ask(core, ClinicRequest::Income { year }) {
        ClinicReply::Income(i) => i,
        other => panic!("{other:?}"),
    }
}

fn tasks(core: &mut Core) -> Vec<TaskView> {
    match ask(core, ClinicRequest::Tasks { today: now() }) {
        ClinicReply::Tasks(t) => t,
        other => panic!("{other:?}"),
    }
}

fn create_case(core: &mut Core, patient: &Patient) -> CaseLink {
    let request = ClinicRequest::CreateCase {
        patient_id: patient.id.clone(),
        today: now(),
    };
    match ask(core, request) {
        ClinicReply::Case(link) => link,
        other => panic!("{other:?}"),
    }
}

fn link_case(core: &mut Core, patient: &Patient, case_id: Option<&str>) -> Patient {
    let request = ClinicRequest::LinkCase {
        patient_id: patient.id.clone(),
        case_id: case_id.map(str::to_owned),
    };
    match ask(core, request) {
        ClinicReply::Patient(p) => p,
        other => panic!("{other:?}"),
    }
}

fn note_to_case(core: &mut Core, note: &SessionNote) -> SessionNote {
    match ask(
        core,
        ClinicRequest::NoteToCase {
            id: note.id.clone(),
        },
    ) {
        ClinicReply::Note(n) => n,
        other => panic!("{other:?}"),
    }
}

fn audit_count(core: &mut Core, event: &str) -> usize {
    core.activity(None)
        .unwrap()
        .entries
        .iter()
        .filter(|e| e.event == event)
        .count()
}

// ------------------------------------------------------------------ the door

#[test]
fn a_locked_vault_answers_locked_to_everything() {
    let (_dir, mut core) = open_clinic();
    let patient = new_patient(&mut core, "נועם");
    core.lock();
    let requests = vec![
        ClinicRequest::Overview { today: now() },
        ClinicRequest::Settings,
        ClinicRequest::Patients { today: now() },
        ClinicRequest::SavePatient {
            patient: child("תמר"),
        },
        // Not even a request that would be refused says anything else.
        ClinicRequest::Diary {
            month: "לא חודש".to_owned(),
        },
        ClinicRequest::ErasePatient {
            id: patient.id.clone(),
            name: "נועם".to_owned(),
        },
        ClinicRequest::CaseChoices,
    ];
    for request in requests {
        assert!(
            matches!(core.clinic(request.clone()), Err(CoreError::Locked)),
            "{request:?}"
        );
    }
    core.unlock(PASSWORD).unwrap();
    assert_eq!(rows(&mut core).len(), 1);
}

#[test]
fn the_clinic_goes_idle_like_every_other_command() {
    let (_dir, mut core) = open_clinic();
    new_patient(&mut core, "נועם");
    core.last_activity = Instant::now() - Duration::from_secs(61 * 60);
    assert!(matches!(
        core.clinic(ClinicRequest::Patients { today: now() }),
        Err(CoreError::Locked)
    ));
}

/// D-170, rule 2: nothing of the clinic is sent anywhere. The service knows the vault and
/// the rules only; a name of the network, the model or the filter's gate in it is a fault.
#[test]
fn the_service_names_nothing_that_sends() {
    let source = include_str!("../clinic.rs");
    for forbidden in [
        "dv_egress",
        "dv_ai",
        "dv_privacy",
        "dv_ner",
        "gate::",
        "ClearedPayload",
        "Transport",
        "transmit",
        "prepare_",
        "send_",
        "http",
    ] {
        assert!(
            !source.contains(forbidden),
            "crates/dv-core/src/clinic.rs names {forbidden}"
        );
    }
    // The one way out of the clinic is into a case, through the core's own two functions.
    // A record that goes for the first time is written together with the note that points to
    // it (D-177), and learned from as `add_input` learns; a second send is `update_input`.
    assert!(source.contains(".clinic_note_into_case(") && source.contains("self.learn_input("));
    assert!(source.contains("self.update_input("));
}

// ------------------------------------------------------------------ patients

#[test]
fn a_patient_card_is_saved_cleaned_and_keeps_what_is_not_the_windows() {
    let (_dir, mut core) = open_clinic();
    let sent = Patient {
        name: "  נועם   לביא ".to_owned(),
        status: PatientStatus::Waiting,
        // Never the window's to set.
        case_id: Some("לא-תיק".to_owned()),
        contacts: vec![
            contact(ContactRole::Mother, " דנה  לביא"),
            Contact::default(),
        ],
        ..child("")
    };
    let saved = save_patient(&mut core, sent);
    assert!(!saved.id.is_empty());
    assert_eq!(saved.name, "נועם לביא");
    assert_eq!(saved.case_id, None);
    assert_eq!(saved.contacts.len(), 1);
    assert_eq!(saved.contacts[0].name, "דנה לביא");
    // A family on the list waits since the day the card was opened.
    assert_eq!(saved.waiting_since.as_deref(), Some("2026-09-01"));
    assert_eq!(audit_count(&mut core, "clinic_patient_added"), 1);

    // An edit keeps the day the card was opened, whatever is sent.
    let edited = save_patient(
        &mut core,
        Patient {
            framework: "גן הדקל".to_owned(),
            created_on: "2020-01-01".to_owned(),
            case_id: Some("אחר".to_owned()),
            ..saved.clone()
        },
    );
    assert_eq!(edited.created_on, "2026-09-01");
    assert_eq!(edited.case_id, None);
    assert_eq!(edited.framework, "גן הדקל");
    assert_eq!(card(&mut core, &saved.id).patient, edited);
    assert_eq!(audit_count(&mut core, "clinic_patient_added"), 1);

    // A new card without its day gets the core's own date.
    let undated = save_patient(
        &mut core,
        Patient {
            created_on: String::new(),
            ..child("תמר")
        },
    );
    assert_eq!(undated.created_on, dates::iso(dates::today()));

    let text = refused(
        &mut core,
        ClinicRequest::SavePatient {
            patient: child("   "),
        },
    );
    assert!(text.contains("שם"), "{text}");
    refused(
        &mut core,
        ClinicRequest::SavePatient {
            patient: Patient {
                birth_date: Some("2021-02-30".to_owned()),
                ..child("רון")
            },
        },
    );
    let missing = not_found(
        &mut core,
        ClinicRequest::SavePatient {
            patient: Patient {
                id: "אין-כזה".to_owned(),
                ..child("רון")
            },
        },
    );
    assert_eq!(missing, "לא נמצא: המטופל");
    assert_eq!(rows(&mut core).len(), 2);
}

#[test]
fn the_patient_list_shows_each_family_at_a_glance() {
    let (_dir, mut core) = open_clinic();
    let tamar = new_patient(&mut core, "תמר");
    let noam = new_patient(&mut core, "נועם");
    let held = save_visit(
        &mut core,
        Visit {
            fee: 40_000,
            ..a_visit(&noam, VisitKind::Intake, "2026-11-10", 540)
        },
    );
    mark(&mut core, &held, VisitStatus::Held);
    save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Assessment, "2026-11-20", 600),
    );
    save_payment(&mut core, a_payment(&noam, "2026-11-10", 15_000));
    save_task(
        &mut core,
        ClinicTask {
            patient_id: Some(noam.id.clone()),
            title: "לשלוח שאלון".to_owned(),
            ..ClinicTask::default()
        },
    );
    let rows = rows(&mut core);
    let names: Vec<&str> = rows.iter().map(|r| r.patient.name.as_str()).collect();
    assert_eq!(names, ["נועם", "תמר"]);
    let row = &rows[0];
    assert_eq!(row.due, 25_000);
    assert_eq!(row.open_tasks, 1);
    assert_eq!(row.last_visit.as_deref(), Some("2026-11-10"));
    let next = row.next_visit.clone().unwrap();
    assert_eq!(
        (next.date.as_str(), next.start, next.kind),
        ("2026-11-20", 600, VisitKind::Assessment)
    );
    let age = row.age.unwrap();
    assert_eq!((age.years, age.months), (5, 8));
    assert_eq!(rows[1].patient.id, tamar.id);
    assert_eq!((rows[1].due, rows[1].open_tasks), (0, 0));
    assert!(rows[1].next_visit.is_none() && rows[1].last_visit.is_none());

    refused(
        &mut core,
        ClinicRequest::Patients {
            today: Today {
                date: "היום".to_owned(),
                minute: 0,
            },
        },
    );
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::PatientCard {
                id: "אין".to_owned(),
                today: now()
            }
        ),
        "לא נמצא: המטופל"
    );
}

// ------------------------------------------------------------------ visits

#[test]
fn a_new_visit_is_planned_and_a_waiting_family_becomes_active() {
    let (_dir, mut core) = open_clinic();
    let waiting = save_patient(
        &mut core,
        Patient {
            status: PatientStatus::Waiting,
            ..child("נועם")
        },
    );
    assert_eq!(overview(&mut core).waiting, 1);
    let saved = save_visit(
        &mut core,
        Visit {
            // Whatever is sent, a new visit is planned and belongs to no series.
            status: VisitStatus::Held,
            series_id: Some("לא-סדרה".to_owned()),
            place: "  הקליניקה  ".to_owned(),
            ..a_visit(&waiting, VisitKind::Intake, "2026-11-17", 540)
        },
    );
    assert!(!saved.id.is_empty());
    assert_eq!(saved.status, VisitStatus::Planned);
    assert_eq!(saved.series_id, None);
    assert_eq!(saved.place, "הקליניקה");
    let after = card(&mut core, &waiting.id);
    assert_eq!(after.patient.status, PatientStatus::Active);
    let o = overview(&mut core);
    assert_eq!((o.waiting, o.active), (0, 1));
    assert_eq!(actions(&mut core, &saved.id), ["created"]);

    // Saving it again as it is writes nothing.
    assert_eq!(save_visit(&mut core, saved.clone()), saved);
    assert_eq!(actions(&mut core, &saved.id), ["created"]);

    // Marking it is a change, kept in its history, in her words.
    let held = mark(&mut core, &saved, VisitStatus::Held);
    assert_eq!(held.status, VisitStatus::Held);
    let entries = history(&mut core, &saved.id);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].action, "changed");
    assert!(entries[0].lines.contains(&"מצב: מתוכננת".to_owned()));
    assert!(entries[1].lines.contains(&"מצב: התקיימה".to_owned()));
    assert!(entries[1].lines.contains(&"מטופל: נועם".to_owned()));
    assert!(entries[0].at > 1_700_000_000);
}

#[test]
fn a_visit_is_checked_before_it_is_saved() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let request = |visit: Visit| ClinicRequest::SaveVisit {
        visit,
        repeat: None,
        scope: Scope::One,
    };
    // A block has no patient and no fee; every other kind needs a patient.
    let block = Visit {
        kind: VisitKind::Block,
        title: "כתיבת דוחות".to_owned(),
        date: "2026-11-17".to_owned(),
        start: 480,
        minutes: 120,
        ..Visit::default()
    };
    let saved = save_visit(&mut core, block.clone());
    assert_eq!((saved.patient_id.clone(), saved.fee), (None, 0));
    refused(
        &mut core,
        request(Visit {
            patient_id: Some(noam.id.clone()),
            ..block.clone()
        }),
    );
    refused(
        &mut core,
        request(Visit {
            fee: 100,
            ..block.clone()
        }),
    );
    refused(
        &mut core,
        request(Visit {
            patient_id: None,
            ..a_visit(&noam, VisitKind::Therapy, TODAY, 600)
        }),
    );
    // The patient has to be there.
    let ghost = Patient {
        id: "אין-מטופל-כזה".to_owned(),
        ..child("רפאים")
    };
    let text = refused(
        &mut core,
        request(a_visit(&ghost, VisitKind::Therapy, TODAY, 600)),
    );
    assert!(text.contains("המטופל"), "{text}");
    // Limits come from the rules, in their words.
    refused(
        &mut core,
        request(Visit {
            minutes: 4,
            ..a_visit(&noam, VisitKind::Therapy, TODAY, 600)
        }),
    );
    refused(
        &mut core,
        request(a_visit(&noam, VisitKind::Therapy, "2026-13-01", 600)),
    );
    // An id the vault does not know, or one of another kind of record.
    let missing = not_found(
        &mut core,
        request(Visit {
            id: "אין".to_owned(),
            ..a_visit(&noam, VisitKind::Therapy, TODAY, 600)
        }),
    );
    assert_eq!(missing, "לא נמצא: הפגישה");
    let payment = save_payment(&mut core, a_payment(&noam, TODAY, 100));
    not_found(
        &mut core,
        request(Visit {
            id: payment.id,
            ..a_visit(&noam, VisitKind::Therapy, TODAY, 600)
        }),
    );
    // Only the block was written.
    assert_eq!(calendar(&mut core, "2026-11-01", "2026-11-30").len(), 1);
}

#[test]
fn a_repeat_makes_one_series_in_one_go() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let first = Visit {
        fee: 45_000,
        ..a_visit(&noam, VisitKind::Therapy, "2026-12-21", 960)
    };
    let repeat = Repeat {
        every_weeks: 2,
        count: 4,
    };
    let saved = save_visits(&mut core, first.clone(), Some(repeat), Scope::One);
    let dates: Vec<&str> = saved.visits.iter().map(|v| v.visit.date.as_str()).collect();
    assert_eq!(
        dates,
        ["2026-12-21", "2027-01-04", "2027-01-18", "2027-02-01"]
    );
    let series = saved.visits[0].visit.series_id.clone().unwrap();
    assert!(!series.is_empty());
    for view in &saved.visits {
        assert_eq!(view.visit.series_id.as_deref(), Some(series.as_str()));
        assert_eq!(view.visit.status, VisitStatus::Planned);
        assert_eq!((view.visit.start, view.visit.fee), (960, 45_000));
        assert_eq!(view.patient_name.as_deref(), Some("נועם"));
        assert!(!view.has_note && !view.overlaps);
        assert_eq!(actions(&mut core, &view.visit.id), ["created"]);
    }
    assert_eq!(saved.overlapping, 0);
    // Another series has its own id.
    let other = save_visits(&mut core, first.clone(), Some(repeat), Scope::One);
    assert_ne!(
        other.visits[0].visit.series_id.as_deref(),
        Some(series.as_str())
    );
    assert_eq!(other.overlapping, 4);

    // A series that cannot be is refused whole: nothing of it is written.
    let before = calendar(&mut core, "2026-12-01", "2027-03-01").len();
    for bad in [
        Repeat {
            every_weeks: 0,
            count: 4,
        },
        Repeat {
            every_weeks: 1,
            count: 1,
        },
        Repeat {
            every_weeks: 9,
            count: 4,
        },
        Repeat {
            every_weeks: 1,
            count: 53,
        },
    ] {
        refused(
            &mut core,
            ClinicRequest::SaveVisit {
                visit: first.clone(),
                repeat: Some(bad),
                scope: Scope::One,
            },
        );
    }
    assert_eq!(
        calendar(&mut core, "2026-12-01", "2027-03-01").len(),
        before
    );
    // A repeat is for a new visit only.
    refused(
        &mut core,
        ClinicRequest::SaveVisit {
            visit: saved.visits[0].visit.clone(),
            repeat: Some(repeat),
            scope: Scope::One,
        },
    );
}

#[test]
fn this_one_and_the_following_changes_the_later_planned_visits_together() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let repeat = Repeat {
        every_weeks: 1,
        count: 5,
    };
    let series = save_visits(
        &mut core,
        Visit {
            fee: 40_000,
            ..a_visit(&noam, VisitKind::Therapy, "2026-12-07", 600)
        },
        Some(repeat),
        Scope::One,
    )
    .visits;
    let ids: Vec<String> = series.iter().map(|v| v.visit.id.clone()).collect();
    // The first was held; the fourth she moved by herself, to the afternoon.
    mark(&mut core, &series[0].visit, VisitStatus::Held);
    let fourth = save_visit(
        &mut core,
        Visit {
            date: "2026-12-29".to_owned(),
            start: 900,
            ..series[3].visit.clone()
        },
    );
    assert_eq!(fourth.date, "2026-12-29");
    assert_eq!(actions(&mut core, &ids[4]), ["created"]);

    // The second moves two days later, to 11:00, at a new fee: this one and the following.
    let edited = Visit {
        date: "2026-12-16".to_owned(),
        start: 660,
        fee: 42_000,
        // Never the window's: the series stays the one it was made in.
        series_id: Some("אחרת".to_owned()),
        ..series[1].visit.clone()
    };
    let saved = save_visits(&mut core, edited, None, Scope::Following);
    let written: Vec<(&str, &str, u16, i32)> = saved
        .visits
        .iter()
        .map(|v| {
            (
                v.visit.id.as_str(),
                v.visit.date.as_str(),
                v.visit.start,
                v.visit.fee,
            )
        })
        .collect();
    assert_eq!(
        written,
        [
            (ids[1].as_str(), "2026-12-16", 660, 42_000),
            (ids[2].as_str(), "2026-12-23", 660, 42_000),
            (ids[3].as_str(), "2026-12-31", 660, 42_000),
            (ids[4].as_str(), "2027-01-06", 660, 42_000),
        ]
    );
    let all = calendar(&mut core, "2026-12-01", "2027-01-31");
    assert_eq!(all.len(), 5);
    let series_id = series[0].visit.series_id.clone();
    assert!(all.iter().all(|v| v.visit.series_id == series_id));
    // The one that was held stays where it was, as it was.
    let first = all.iter().find(|v| v.visit.id == ids[0]).unwrap();
    assert_eq!(
        (
            first.visit.date.as_str(),
            first.visit.start,
            first.visit.fee
        ),
        ("2026-12-07", 600, 40_000)
    );
    assert_eq!(actions(&mut core, &ids[0]), ["created", "changed"]);
    assert_eq!(actions(&mut core, &ids[2]), ["created", "changed"]);
    assert_eq!(
        actions(&mut core, &ids[3]),
        ["created", "changed", "changed"]
    );

    // A status change with "the following" marks this one only, and writes only it.
    let held = save_visits(
        &mut core,
        Visit {
            status: VisitStatus::Held,
            ..saved.visits[0].visit.clone()
        },
        None,
        Scope::Following,
    );
    assert_eq!(held.visits.len(), 1);
    assert_eq!(held.visits[0].visit.status, VisitStatus::Held);
    let all = calendar(&mut core, "2026-12-01", "2027-01-31");
    let planned = all
        .iter()
        .filter(|v| v.visit.status == VisitStatus::Planned)
        .count();
    assert_eq!(planned, 3);
    assert_eq!(actions(&mut core, &ids[4]), ["created", "changed"]);

    // "Only this one" touches only this one.
    let only = save_visits(
        &mut core,
        Visit {
            start: 720,
            ..saved.visits[1].visit.clone()
        },
        None,
        Scope::One,
    );
    assert_eq!(only.visits.len(), 1);
    assert_eq!(actions(&mut core, &ids[4]), ["created", "changed"]);

    // A move that pushes a later visit out of what a date may be writes nothing at all.
    let far = save_visits(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, "2200-12-10", 600),
        Some(Repeat {
            every_weeks: 1,
            count: 3,
        }),
        Scope::One,
    )
    .visits;
    refused(
        &mut core,
        ClinicRequest::SaveVisit {
            visit: Visit {
                date: "2200-12-20".to_owned(),
                ..far[0].visit.clone()
            },
            repeat: None,
            scope: Scope::Following,
        },
    );
    assert_eq!(actions(&mut core, &far[0].visit.id), ["created"]);
    assert_eq!(actions(&mut core, &far[2].visit.id), ["created"]);
}

#[test]
fn overlapping_visits_are_flagged_wherever_they_are_shown() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let tamar = new_patient(&mut core, "תמר");
    let first = save_visits(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, TODAY, 600),
        None,
        Scope::One,
    );
    assert_eq!(first.overlapping, 0);
    // Half an hour into the first one.
    let second = save_visits(
        &mut core,
        a_visit(&tamar, VisitKind::Assessment, TODAY, 630),
        None,
        Scope::One,
    );
    assert_eq!(second.overlapping, 1);
    assert!(second.visits[0].overlaps);
    // Starting exactly when the first ends is not an overlap.
    let third = save_visits(
        &mut core,
        a_visit(&noam, VisitKind::Guidance, TODAY, 690),
        None,
        Scope::One,
    );
    assert_eq!(third.overlapping, 0);

    let day = calendar(&mut core, TODAY, TODAY);
    let flags: Vec<(u16, bool)> = day.iter().map(|v| (v.visit.start, v.overlaps)).collect();
    assert_eq!(flags, [(600, true), (630, true), (690, false)]);
    let o = overview(&mut core);
    assert_eq!(o.visits.len(), 3);
    assert!(o.visits[0].overlaps && o.visits[1].overlaps && !o.visits[2].overlaps);
    let shown = card(&mut core, &noam.id).visits;
    assert_eq!(shown.len(), 2);
    // Newest first, and the overlap with another family's visit is seen from the card too.
    assert_eq!((shown[0].visit.start, shown[0].overlaps), (690, false));
    assert_eq!((shown[1].visit.start, shown[1].overlaps), (600, true));

    // A cancelled visit overlaps nothing.
    mark(&mut core, &second.visits[0].visit, VisitStatus::Cancelled);
    assert!(calendar(&mut core, TODAY, TODAY)
        .iter()
        .all(|v| !v.overlaps));

    // The calendar asks for a stretch that makes sense.
    for (from, to) in [
        ("2026-11-15", "2026-11-14"),
        ("אתמול", "2026-11-14"),
        ("2026-01-01", "2026-12-31"),
    ] {
        refused(
            &mut core,
            ClinicRequest::Calendar {
                from: from.to_owned(),
                to: to.to_owned(),
            },
        );
    }
}

#[test]
fn only_a_planned_visit_without_a_note_can_be_removed() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let delete = |id: &str, scope: Scope| ClinicRequest::DeleteVisit {
        id: id.to_owned(),
        scope,
    };
    let planned = save_visit(&mut core, a_visit(&noam, VisitKind::Intake, TODAY, 540));
    ask(&mut core, delete(&planned.id, Scope::One));
    assert!(calendar(&mut core, TODAY, TODAY).is_empty());
    // What was removed can still be seen.
    let entries = history(&mut core, &planned.id);
    let seen: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(seen, ["created", "deleted"]);
    assert!(entries[1].lines.contains(&"סוג: פגישת היכרות".to_owned()));
    assert_eq!(
        not_found(&mut core, delete(&planned.id, Scope::One)),
        "לא נמצא: הפגישה"
    );

    // One that happened is cancelled, not removed.
    let visit = save_visit(&mut core, a_visit(&noam, VisitKind::Intake, TODAY, 540));
    for status in [
        VisitStatus::Held,
        VisitStatus::Cancelled,
        VisitStatus::NoShow,
    ] {
        mark(&mut core, &visit, status);
        let text = refused(&mut core, delete(&visit.id, Scope::One));
        assert!(text.contains("מבוטלת"), "{text}");
    }
    // A planned one that already has a note stays too.
    let noted = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, "2026-11-16", 540),
    );
    save_note(&mut core, a_note(&noam, Some(&noted), "נכתב מראש."));
    let text = refused(&mut core, delete(&noted.id, Scope::One));
    assert!(text.contains("סיכום") && text.contains("מבוטלת"), "{text}");

    // "This one and the following" removes the later planned ones of the series.
    let series = save_visits(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, "2027-01-04", 600),
        Some(Repeat {
            every_weeks: 1,
            count: 5,
        }),
        Scope::One,
    )
    .visits;
    mark(&mut core, &series[3].visit, VisitStatus::Cancelled);
    save_note(
        &mut core,
        a_note(&noam, Some(&series[4].visit), "נכתב מראש."),
    );
    ask(&mut core, delete(&series[1].visit.id, Scope::Following));
    let left: Vec<String> = calendar(&mut core, "2027-01-01", "2027-02-28")
        .into_iter()
        .map(|v| v.visit.date)
        .collect();
    // The first (earlier), the cancelled one and the one with a note stay.
    assert_eq!(left, ["2027-01-04", "2027-01-25", "2027-02-01"]);
    assert_eq!(
        actions(&mut core, &series[2].visit.id),
        ["created", "deleted"]
    );
    // "Only this one" on a visit of a series removes it alone.
    ask(&mut core, delete(&series[0].visit.id, Scope::One));
    assert_eq!(calendar(&mut core, "2027-01-01", "2027-02-28").len(), 2);
}

// ------------------------------------------------------------------ session notes

#[test]
fn a_note_is_the_record_one_for_a_visit_and_never_removed() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let tamar = new_patient(&mut core, "תמר");
    let visit = save_visit(&mut core, a_visit(&noam, VisitKind::Intake, TODAY, 540));
    let theirs = save_visit(&mut core, a_visit(&tamar, VisitKind::Intake, TODAY, 700));
    let request = |note: SessionNote| ClinicRequest::SaveNote { note };

    let note = save_note(
        &mut core,
        SessionNote {
            record: "  ההורים תיארו קושי בהתארגנות.  ".to_owned(),
            personal: "מחשבה-פרטית-בדויה".to_owned(),
            // Never the window's to say.
            case_input_id: Some("חומר".to_owned()),
            ..a_note(&noam, Some(&visit), "")
        },
    );
    assert!(!note.id.is_empty());
    assert_eq!(note.record, "ההורים תיארו קושי בהתארגנות.");
    assert_eq!(note.case_input_id, None);
    assert!(calendar(&mut core, TODAY, TODAY)
        .iter()
        .any(|v| v.visit.id == visit.id && v.has_note));

    // A second note for the same visit is refused: the first one is edited.
    let text = refused(
        &mut core,
        request(a_note(&noam, Some(&visit), "סיכום נוסף.")),
    );
    assert!(text.contains("כבר יש סיכום"), "{text}");
    // A note on its own (a phone call) needs no visit, and there may be many.
    let call = save_note(&mut core, a_note(&noam, None, "שיחת טלפון עם האם."));
    save_note(&mut core, a_note(&noam, None, "שיחה נוספת."));
    assert_eq!(call.visit_id, None);

    // The visit has to be this patient's, and has to be a visit.
    refused(&mut core, request(a_note(&noam, Some(&theirs), "טעות.")));
    assert_eq!(
        not_found(
            &mut core,
            request(SessionNote {
                visit_id: Some("אין".to_owned()),
                ..a_note(&noam, None, "טעות.")
            })
        ),
        "לא נמצא: הפגישה"
    );
    let ghost = Patient {
        id: "אין".to_owned(),
        ..child("רפאים")
    };
    refused(&mut core, request(a_note(&ghost, None, "טעות.")));
    refused(&mut core, request(a_note(&noam, None, "   ")));
    assert_eq!(
        not_found(
            &mut core,
            request(SessionNote {
                id: "אין".to_owned(),
                ..a_note(&noam, None, "טעות.")
            })
        ),
        "לא נמצא: הסיכום"
    );
    // A note does not move to another family.
    refused(
        &mut core,
        request(SessionNote {
            patient_id: tamar.id.clone(),
            visit_id: None,
            ..note.clone()
        }),
    );

    // An edit is kept in the history: the record in full, the personal note never.
    let edited = save_note(
        &mut core,
        SessionNote {
            record: "ההורים תיארו קושי בהתארגנות בבוקר.".to_owned(),
            ..note.clone()
        },
    );
    save_note(
        &mut core,
        SessionNote {
            personal: "מחשבה-אחרת-בדויה".to_owned(),
            ..edited.clone()
        },
    );
    // Saving it as it is writes nothing.
    let again = card(&mut core, &noam.id)
        .notes
        .into_iter()
        .find(|n| n.id == note.id)
        .unwrap();
    assert_eq!(again.personal, "מחשבה-אחרת-בדויה");
    save_note(&mut core, again);
    let entries = history(&mut core, &note.id);
    let seen: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(seen, ["created", "changed", "changed"]);
    let about: Vec<&str> = entries
        .iter()
        .map(|e| {
            e.lines
                .iter()
                .find(|l| l.contains("תרשומת אישית"))
                .unwrap()
                .as_str()
        })
        .collect();
    assert_eq!(
        about,
        [
            "תרשומת אישית: נכתבה",
            "תרשומת אישית: בלי שינוי",
            "תרשומת אישית: השתנתה"
        ]
    );
    assert!(entries[1]
        .lines
        .contains(&"ההורים תיארו קושי בהתארגנות בבוקר.".to_owned()));
    for entry in &entries {
        assert!(entry.lines.iter().all(|l| !l.contains("בדויה")));
    }

    // The clinical record is never removed.
    let text = refused(
        &mut core,
        ClinicRequest::DeleteNote {
            id: note.id.clone(),
        },
    );
    assert!(text.contains("לא מוחקים"), "{text}");
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::DeleteNote {
                id: "אין".to_owned()
            }
        ),
        "לא נמצא: הסיכום"
    );
    // Not even through the id of another kind of record.
    not_found(
        &mut core,
        ClinicRequest::DeleteNote {
            id: visit.id.clone(),
        },
    );
    assert_eq!(card(&mut core, &noam.id).notes.len(), 3);
    // A visit that has a note does not move to another family either.
    refused(
        &mut core,
        ClinicRequest::SaveVisit {
            visit: Visit {
                patient_id: Some(tamar.id.clone()),
                ..visit.clone()
            },
            repeat: None,
            scope: Scope::One,
        },
    );
}

// ------------------------------------------------------------------ money

#[test]
fn payments_and_charges_are_voided_with_a_reason_and_never_removed() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let payment = save_payment(
        &mut core,
        Payment {
            // Voiding has its own request: a save never voids.
            voided: true,
            void_reason: "ניסיון".to_owned(),
            ..a_payment(&noam, "2026-11-03", 45_000)
        },
    );
    assert!(!payment.voided && payment.void_reason.is_empty());
    let charge = save_charge(&mut core, a_charge(&noam, "2026-11-02", 300_000));
    assert_eq!(card(&mut core, &noam.id).balance.due, 255_000);

    // An edit is a change, and the diary says the line was changed.
    let fixed = save_payment(
        &mut core,
        Payment {
            amount: 50_000,
            ..payment.clone()
        },
    );
    assert_eq!(fixed.amount, 50_000);
    // Saving it as it is writes nothing.
    save_payment(&mut core, fixed.clone());
    assert_eq!(actions(&mut core, &payment.id), ["created", "changed"]);
    let month = diary(&mut core, "2026-11");
    let line = |month: &DiaryMonth, id: &str| {
        month
            .days
            .iter()
            .flat_map(|d| d.lines.iter())
            .find(|l| l.record_id == id)
            .cloned()
            .unwrap()
    };
    assert!(line(&month, &payment.id).changed);
    assert!(!line(&month, &charge.id).changed);
    assert_eq!((month.charged, month.received), (300_000, 50_000));

    // Voiding needs a reason.
    let void = |id: &str, reason: &str| ClinicRequest::VoidPayment {
        id: id.to_owned(),
        reason: reason.to_owned(),
    };
    refused(&mut core, void(&payment.id, "   "));
    let voided = match ask(&mut core, void(&payment.id, " נרשם   פעמיים ")) {
        ClinicReply::Payment(p) => p,
        other => panic!("{other:?}"),
    };
    assert!(voided.voided);
    assert_eq!(voided.void_reason, "נרשם פעמיים");
    assert_eq!(
        actions(&mut core, &payment.id),
        ["created", "changed", "voided"]
    );
    let last = history(&mut core, &payment.id).pop().unwrap();
    assert!(last.lines.contains(&"בוטל: כן".to_owned()));
    assert!(last.lines.contains(&"סיבת הביטול: נרשם פעמיים".to_owned()));
    // A voided one is not voided again and not edited.
    refused(&mut core, void(&payment.id, "שוב"));
    refused(
        &mut core,
        ClinicRequest::SavePayment {
            payment: Payment {
                amount: 1,
                ..voided.clone()
            },
        },
    );
    assert_eq!(not_found(&mut core, void("אין", "סיבה")), "לא נמצא: התקבול");
    // It stays in the diary, shown as voided, and is counted nowhere.
    let month = diary(&mut core, "2026-11");
    let shown = line(&month, &payment.id);
    assert!(shown.voided && shown.changed);
    assert_eq!(shown.received, 50_000);
    assert_eq!(month.received, 0);
    assert_eq!(card(&mut core, &noam.id).balance.paid, 0);
    assert_eq!(card(&mut core, &noam.id).payments.len(), 1);

    // The same for a charge.
    let void = |id: &str, reason: &str| ClinicRequest::VoidCharge {
        id: id.to_owned(),
        reason: reason.to_owned(),
    };
    refused(&mut core, void(&charge.id, ""));
    let voided = match ask(&mut core, void(&charge.id, "סוכם אחרת")) {
        ClinicReply::Charge(c) => c,
        other => panic!("{other:?}"),
    };
    assert!(voided.voided);
    refused(&mut core, void(&charge.id, "שוב"));
    refused(
        &mut core,
        ClinicRequest::SaveCharge {
            charge: Charge {
                amount: 1,
                ..voided
            },
        },
    );
    assert_eq!(
        not_found(&mut core, void(&payment.id, "סיבה")),
        "לא נמצא: החיוב"
    );
    assert_eq!(actions(&mut core, &charge.id), ["created", "voided"]);
    let balance = card(&mut core, &noam.id).balance;
    assert_eq!((balance.charged, balance.paid, balance.due), (0, 0, 0));

    // What a record needs before it is written.
    let ghost = Patient {
        id: "אין".to_owned(),
        ..child("רפאים")
    };
    for request in [
        ClinicRequest::SavePayment {
            payment: a_payment(&noam, TODAY, 0),
        },
        ClinicRequest::SavePayment {
            payment: a_payment(&noam, "2026-02-30", 100),
        },
        ClinicRequest::SavePayment {
            payment: a_payment(&ghost, TODAY, 100),
        },
        ClinicRequest::SaveCharge {
            charge: Charge {
                what: String::new(),
                ..a_charge(&noam, TODAY, 100)
            },
        },
        ClinicRequest::SaveCharge {
            charge: a_charge(&ghost, TODAY, 100),
        },
        ClinicRequest::Diary {
            month: "2026-13".to_owned(),
        },
        ClinicRequest::Income { year: 1999 },
    ] {
        refused(&mut core, request);
    }
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::SavePayment {
                payment: Payment {
                    id: charge.id.clone(),
                    ..a_payment(&noam, TODAY, 100)
                }
            }
        ),
        "לא נמצא: התקבול"
    );
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::History {
                record_id: "אין".to_owned()
            }
        ),
        "לא נמצא: הרישום"
    );
}

#[test]
fn the_diary_and_the_year_add_up() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let tamar = save_patient(
        &mut core,
        Patient {
            created_on: "2025-12-30".to_owned(),
            ..child("תמר")
        },
    );
    let held = save_visit(
        &mut core,
        Visit {
            fee: 40_000,
            ..a_visit(&noam, VisitKind::Assessment, "2026-10-12", 540)
        },
    );
    mark(&mut core, &held, VisitStatus::Held);
    let missed = save_visit(
        &mut core,
        Visit {
            fee: 40_000,
            ..a_visit(&tamar, VisitKind::Therapy, "2026-10-12", 660)
        },
    );
    save_visit(
        &mut core,
        Visit {
            status: VisitStatus::NoShow,
            charge_missed: true,
            ..missed
        },
    );
    let cancelled = save_visit(
        &mut core,
        a_visit(&tamar, VisitKind::Therapy, "2026-11-02", 660),
    );
    mark(&mut core, &cancelled, VisitStatus::Cancelled);
    save_charge(&mut core, a_charge(&noam, "2026-10-20", 250_000));
    save_payment(
        &mut core,
        Payment {
            receipt: String::new(),
            method: PayMethod::Cash,
            ..a_payment(&noam, "2026-10-12", 40_000)
        },
    );
    save_payment(&mut core, a_payment(&tamar, "2026-11-02", 40_000));
    // Time she blocks for herself is not in the diary.
    save_visit(
        &mut core,
        Visit {
            kind: VisitKind::Block,
            date: "2026-10-12".to_owned(),
            start: 900,
            minutes: 60,
            ..Visit::default()
        },
    );

    let october = diary(&mut core, "2026-10");
    assert_eq!(october.month, "2026-10");
    let dates: Vec<&str> = october.days.iter().map(|d| d.date.as_str()).collect();
    assert_eq!(dates, ["2026-10-12", "2026-10-20"]);
    let day = &october.days[0];
    let who: Vec<(&str, i32, i32)> = day
        .lines
        .iter()
        .map(|l| (l.patient_name.as_str(), l.charged, l.received))
        .collect();
    assert_eq!(
        who,
        [("נועם", 40_000, 0), ("תמר", 40_000, 0), ("נועם", 0, 40_000)]
    );
    assert_eq!((day.charged, day.received), (80_000, 40_000));
    assert_eq!((october.charged, october.received), (330_000, 40_000));
    assert_eq!(october.missing_receipts, 1);
    assert_eq!(october.by_method.len(), 1);
    assert_eq!(october.by_method[0].method, PayMethod::Cash);
    assert!(diary(&mut core, "2026-09").days.is_empty());

    let year = income(&mut core, 2026);
    assert_eq!(year.months.len(), 12);
    let october = &year.months[9];
    assert_eq!(
        (october.held, october.no_show, october.cancelled),
        (1, 1, 0)
    );
    assert_eq!(year.months[10].cancelled, 1);
    assert_eq!((year.charged, year.received), (330_000, 80_000));
    // Opened in 2026: one of the two cards.
    assert_eq!(year.new_patients, 1);
    assert_eq!(year.by_kind.len(), 1);
    assert_eq!(year.by_kind[0].kind, VisitKind::Assessment);
    let before = income(&mut core, 2025);
    assert_eq!((before.charged, before.new_patients), (0, 1));

    let o = overview(&mut core);
    assert_eq!(o.month_received, 40_000);
    // What each family owes: 250,000 and 0. A family's credit never hides another's debt.
    assert_eq!(o.total_due, 250_000);
}

// ------------------------------------------------------------------ tasks, settings, messages

#[test]
fn tasks_are_saved_listed_in_order_and_removed() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let task = |title: &str, due: Option<&str>, patient: Option<&Patient>| ClinicTask {
        patient_id: patient.map(|p| p.id.clone()),
        kind: TaskKind::Call,
        title: title.to_owned(),
        due: due.map(str::to_owned),
        ..ClinicTask::default()
    };
    let undated = save_task(&mut core, task("לסדר את הארון", None, None));
    let late = save_task(
        &mut core,
        task("לחזור לגננת", Some("2026-11-10"), Some(&noam)),
    );
    let ahead = save_task(
        &mut core,
        task("לשלוח שאלון", Some("2026-11-20"), Some(&noam)),
    );
    let list = tasks(&mut core);
    let titles: Vec<(&str, bool, Option<&str>)> = list
        .iter()
        .map(|t| (t.task.title.as_str(), t.overdue, t.patient_name.as_deref()))
        .collect();
    assert_eq!(
        titles,
        [
            ("לחזור לגננת", true, Some("נועם")),
            ("לשלוח שאלון", false, Some("נועם")),
            ("לסדר את הארון", false, None),
        ]
    );
    let o = overview(&mut core);
    assert_eq!(o.tasks.len(), 3);
    assert!(o.alerts.iter().any(
        |a| a.kind == AlertKind::TaskOverdue && a.task_id.as_deref() == Some(late.id.as_str())
    ));

    // Done: off the front page, still in the list, last.
    let done = save_task(
        &mut core,
        ClinicTask {
            done_on: Some(TODAY.to_owned()),
            ..late.clone()
        },
    );
    assert_eq!(done.id, late.id);
    assert_eq!(overview(&mut core).tasks.len(), 2);
    let list = tasks(&mut core);
    assert_eq!(list.len(), 3);
    assert_eq!(list[2].task.id, late.id);
    assert!(!list[2].overdue);
    // A task has no history: it is not part of the record.
    assert!(history(&mut core, &late.id).is_empty());

    ask(
        &mut core,
        ClinicRequest::DeleteTask {
            id: ahead.id.clone(),
        },
    );
    assert_eq!(tasks(&mut core).len(), 2);
    assert_eq!(
        not_found(&mut core, ClinicRequest::DeleteTask { id: ahead.id }),
        "לא נמצא: המשימה"
    );
    // The id of another kind of record removes nothing.
    let payment = save_payment(&mut core, a_payment(&noam, TODAY, 100));
    not_found(&mut core, ClinicRequest::DeleteTask { id: payment.id });
    assert_eq!(card(&mut core, &noam.id).payments.len(), 1);

    refused(
        &mut core,
        ClinicRequest::SaveTask {
            task: task("  ", None, None),
        },
    );
    let ghost = Patient {
        id: "אין".to_owned(),
        ..child("רפאים")
    };
    refused(
        &mut core,
        ClinicRequest::SaveTask {
            task: task("להתקשר", None, Some(&ghost)),
        },
    );
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::SaveTask {
                task: ClinicTask {
                    id: "אין".to_owned(),
                    ..undated
                }
            }
        ),
        "לא נמצא: המשימה"
    );
}

#[test]
fn the_settings_start_from_the_defaults_and_are_checked() {
    let (_dir, mut core) = open_clinic();
    let read = |core: &mut Core| match ask(core, ClinicRequest::Settings) {
        ClinicReply::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(read(&mut core), ClinicSettings::default());
    let mine = ClinicSettings {
        places: vec![" הקליניקה ".to_owned(), "זום".to_owned(), "זום".to_owned()],
        work_days: vec![4, 0, 0, 2],
        debt_days: 14,
        payment_text: "שלום {הורה}, נותרו {סכום} ₪ לתשלום עבור {ילד}. {שמי}".to_owned(),
        ..ClinicSettings::default()
    };
    let saved = match ask(
        &mut core,
        ClinicRequest::SaveSettings {
            settings: mine.clone(),
        },
    ) {
        ClinicReply::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(saved.places, ["הקליניקה", "זום"]);
    assert_eq!(saved.work_days, [0, 2, 4]);
    assert_eq!(read(&mut core), saved);
    // Saved twice: still one record.
    ask(
        &mut core,
        ClinicRequest::SaveSettings {
            settings: saved.clone(),
        },
    );
    assert_eq!(read(&mut core), saved);

    let bad = ClinicSettings {
        work_days: Vec::new(),
        ..ClinicSettings::default()
    };
    refused(&mut core, ClinicRequest::SaveSettings { settings: bad });
    let mut bad = ClinicSettings::default();
    bad.defaults.pop();
    refused(&mut core, ClinicRequest::SaveSettings { settings: bad });
    assert_eq!(read(&mut core), saved);
    // The settings are not a record with a history, and cannot be reached as one.
    assert!(history(&mut core, "settings").is_empty());
    not_found(
        &mut core,
        ClinicRequest::DeleteTask {
            id: "settings".to_owned(),
        },
    );
}

#[test]
fn a_message_is_filled_for_her_to_copy() {
    let (_dir, mut core) = open_clinic();
    let noam = save_patient(
        &mut core,
        Patient {
            contacts: vec![
                Contact {
                    phone: "000-0000".to_owned(),
                    ..Contact::default()
                },
                contact(ContactRole::Father, "יואב"),
                contact(ContactRole::Mother, "דנה"),
            ],
            ..child("נועם")
        },
    );
    let tamar = save_patient(
        &mut core,
        Patient {
            contacts: Vec::new(),
            ..child("תמר")
        },
    );
    let visit = save_visit(
        &mut core,
        Visit {
            fee: 45_050,
            ..a_visit(&noam, VisitKind::Assessment, "2026-11-17", 570)
        },
    );
    let message = |kind, patient: &Patient, visit: Option<&Visit>| ClinicRequest::Message {
        kind,
        patient_id: patient.id.clone(),
        visit_id: visit.map(|v| v.id.clone()),
    };
    let text = |core: &mut Core, request| match ask(core, request) {
        ClinicReply::Text(t) => t,
        other => panic!("{other:?}"),
    };
    // The first contact that has a name; her first name as she signs; no child's name.
    let reminder = text(
        &mut core,
        message(MessageKind::Reminder, &noam, Some(&visit)),
    );
    assert_eq!(
        reminder,
        "שלום יואב, תזכורת לפגישה ביום שלישי, 17.11.2026, בשעה 09:30. להתראות, ד\"ר רותם אלמוג"
    );
    assert!(!reminder.contains("נועם"));

    // A reminder needs a visit, and the visit has to be this family's.
    refused(&mut core, message(MessageKind::Reminder, &noam, None));
    refused(
        &mut core,
        message(MessageKind::Reminder, &tamar, Some(&visit)),
    );
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::Message {
                kind: MessageKind::Reminder,
                patient_id: noam.id.clone(),
                visit_id: Some("אין".to_owned()),
            }
        ),
        "לא נמצא: הפגישה"
    );
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::Message {
                kind: MessageKind::Payment,
                patient_id: "אין".to_owned(),
                visit_id: None,
            }
        ),
        "לא נמצא: המטופל"
    );

    // Nothing owed: nothing to ask for.
    let text_refused = refused(&mut core, message(MessageKind::Payment, &noam, None));
    assert!(text_refused.contains("יתרה"), "{text_refused}");
    mark(&mut core, &visit, VisitStatus::Held);
    save_payment(&mut core, a_payment(&noam, "2026-11-17", 20_000));
    let payment = text(&mut core, message(MessageKind::Payment, &noam, None));
    assert_eq!(
        payment,
        "שלום יואב, היתרה לתשלום היא 250.50 ₪. תודה רבה, ד\"ר רותם אלמוג"
    );
    // No parent's name on the card: "שלום רב".
    save_charge(&mut core, a_charge(&tamar, TODAY, 10_000));
    let payment = text(&mut core, message(MessageKind::Payment, &tamar, None));
    assert!(
        payment.starts_with("שלום רב, היתרה לתשלום היא 100 ₪"),
        "{payment}"
    );
}

// ------------------------------------------------------------------ a whole assessment

/// Every section of the case's report written and approved, as she would at the end.
fn finish_report(core: &mut Core, case_id: &str) {
    let keys: Vec<String> = core
        .case_detail(case_id)
        .unwrap()
        .sections
        .into_iter()
        .map(|s| s.key)
        .filter(|key| key != "signature")
        .collect();
    for key in keys {
        core.add_own_paragraph(case_id, &key, "פסקה בדויה לבדיקה.")
            .unwrap();
    }
}

fn step<'a>(card: &'a PatientCard, key: &str) -> &'a rules::ProcessStep {
    card.process.iter().find(|s| s.key == key).unwrap()
}

#[test]
fn an_assessment_from_the_waiting_list_to_the_last_payment() {
    let (_dir, mut core) = open_clinic();
    // A family asks for an assessment and waits.
    let family = save_patient(
        &mut core,
        Patient {
            status: PatientStatus::Waiting,
            waiting_since: Some("2026-08-01".to_owned()),
            urgent: true,
            ..child("נועם")
        },
    );
    let o = overview(&mut core);
    assert_eq!((o.waiting, o.active), (1, 0));
    // Fifteen weeks on the list: pointed out.
    assert!(o.alerts.iter().any(|a| a.kind == AlertKind::Waiting
        && a.patient_id.as_deref() == Some(family.id.as_str())
        && !a.text.contains("נועם")));
    let first = card(&mut core, &family.id);
    assert_eq!(step(&first, "intake").state, StepState::Now);
    assert!(first.process.iter().all(|s| s.key != "observation"));

    // The intake is set: the family is in the process.
    let intake = save_visit(
        &mut core,
        Visit {
            fee: 45_000,
            ..a_visit(&family, VisitKind::Intake, "2026-10-12", 540)
        },
    );
    let c = card(&mut core, &family.id);
    assert_eq!(c.patient.status, PatientStatus::Active);
    // Its time has passed and it is still "planned".
    assert!(c.alerts.iter().any(
        |a| a.kind == AlertKind::Unmarked && a.visit_id.as_deref() == Some(intake.id.as_str())
    ));
    let intake = mark(&mut core, &intake, VisitStatus::Held);
    // Held, and no record yet.
    assert!(card(&mut core, &family.id)
        .alerts
        .iter()
        .any(|a| a.kind == AlertKind::NoNote));
    save_note(
        &mut core,
        a_note(&family, Some(&intake), "ההורים תיארו קושי בהתארגנות."),
    );

    // Three testing sessions, as one series.
    let sessions = save_visits(
        &mut core,
        Visit {
            fee: 50_000,
            ..a_visit(&family, VisitKind::Assessment, "2026-10-19", 600)
        },
        Some(Repeat {
            every_weeks: 1,
            count: 3,
        }),
        Scope::One,
    )
    .visits;
    assert_eq!(sessions.len(), 3);
    let c = card(&mut core, &family.id);
    assert_eq!(step(&c, "intake").state, StepState::Done);
    assert_eq!(step(&c, "sessions").state, StepState::Now);
    for (n, session) in sessions.iter().enumerate() {
        let held = mark(&mut core, &session.visit, VisitStatus::Held);
        save_note(
            &mut core,
            a_note(
                &family,
                Some(&held),
                &format!("מפגש {}: עבד בריכוז.", n + 1),
            ),
        );
    }
    let c = card(&mut core, &family.id);
    assert_eq!(step(&c, "sessions").state, StepState::Done);
    assert_eq!(step(&c, "sessions").detail, "3 מפגשים התקיימו");
    assert_eq!(step(&c, "report").state, StepState::Now);
    assert_eq!(c.notes.len(), 4);
    assert_eq!(c.visits.len(), 4);
    assert!(c.visits.iter().all(|v| v.has_note));

    // The report as a package, and what the family paid so far.
    save_charge(&mut core, a_charge(&family, "2026-11-02", 150_000));
    save_payment(&mut core, a_payment(&family, "2026-10-12", 45_000));
    save_payment(&mut core, a_payment(&family, "2026-11-02", 150_000));
    let c = card(&mut core, &family.id);
    assert_eq!(
        (c.balance.charged, c.balance.paid, c.balance.due),
        (345_000, 195_000, 150_000)
    );
    assert_eq!(step(&c, "payment").state, StepState::Now);

    // The case of the report, opened from the card.
    let link = create_case(&mut core, &family);
    assert_eq!(link.approved, 0);
    assert!(link.total > 0 && !link.in_trash);
    // The feedback meeting is two days away and the report is not finished.
    let feedback = save_visit(
        &mut core,
        a_visit(&family, VisitKind::Feedback, "2026-11-17", 540),
    );
    let c = card(&mut core, &family.id);
    assert!(c.alerts.iter().any(|a| a.kind == AlertKind::FeedbackSoon));
    assert_eq!(step(&c, "feedback").state, StepState::Now);

    finish_report(&mut core, &link.id);
    let c = card(&mut core, &family.id);
    let case = c.case.clone().unwrap();
    assert_eq!(case.approved, case.total);
    assert_eq!(step(&c, "report").state, StepState::Done);
    assert!(c.alerts.iter().all(|a| a.kind != AlertKind::FeedbackSoon));

    // Two days later: the feedback was given, its record written, the rest paid.
    let later = Today {
        date: "2026-11-17".to_owned(),
        minute: 14 * 60,
    };
    let feedback = mark(&mut core, &feedback, VisitStatus::Held);
    save_note(
        &mut core,
        a_note(&family, Some(&feedback), "נמסרו הממצאים וההמלצות."),
    );
    save_payment(&mut core, a_payment(&family, "2026-11-17", 150_000));
    let done = match ask(
        &mut core,
        ClinicRequest::PatientCard {
            id: family.id.clone(),
            today: later.clone(),
        },
    ) {
        ClinicReply::Card(card) => *card,
        other => panic!("{other:?}"),
    };
    assert_eq!(done.balance.due, 0);
    let states: Vec<(&str, StepState)> = done
        .process
        .iter()
        .map(|s| (s.key.as_str(), s.state))
        .collect();
    assert_eq!(
        states,
        [
            ("intake", StepState::Done),
            ("sessions", StepState::Done),
            ("report", StepState::Done),
            ("feedback", StepState::Done),
            ("payment", StepState::Done),
        ]
    );
    for s in &done.process {
        hebrew(&s.label);
        hebrew(&s.detail);
    }
    assert!(done.alerts.is_empty(), "{:?}", done.alerts);
    let o = match ask(&mut core, ClinicRequest::Overview { today: later }) {
        ClinicReply::Overview(o) => o,
        other => panic!("{other:?}"),
    };
    assert!(o.alerts.is_empty(), "{:?}", o.alerts);
    assert_eq!(o.total_due, 0);
    assert_eq!(o.month_received, 300_000);
    assert_eq!(o.visits.len(), 1);
    assert_eq!(o.week_visits, 1);
    assert_eq!(o.next_day, None);

    // The year saw it all.
    let year = income(&mut core, 2026);
    assert_eq!((year.charged, year.received), (345_000, 345_000));
    assert_eq!(year.new_patients, 1);
    // And a closed card stays, with everything in it.
    let closed = save_patient(
        &mut core,
        Patient {
            status: PatientStatus::Closed,
            ..done.patient.clone()
        },
    );
    assert_eq!(closed.case_id.as_deref(), Some(link.id.as_str()));
    assert_eq!(card(&mut core, &family.id).visits.len(), 5);
}

#[test]
fn the_front_page_shows_today_and_the_next_day_with_visits() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    // This morning's visit has ended; the next ones are on Tuesday.
    save_visit(&mut core, a_visit(&noam, VisitKind::Therapy, TODAY, 540));
    save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, "2026-11-17", 600),
    );
    save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Guidance, "2026-11-17", 480),
    );
    save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, "2026-11-24", 600),
    );
    let o = overview(&mut core);
    assert_eq!(o.date, TODAY);
    assert_eq!(o.visits.len(), 1);
    assert_eq!(o.next_day.as_deref(), Some("2026-11-17"));
    let starts: Vec<u16> = o.next_visits.iter().map(|v| v.visit.start).collect();
    assert_eq!(starts, [480, 600]);
    assert_eq!(o.week_visits, 3);
    // While a visit of today is still ahead, there is no "next day".
    let morning = Today {
        date: TODAY.to_owned(),
        minute: 8 * 60,
    };
    let o = match ask(&mut core, ClinicRequest::Overview { today: morning }) {
        ClinicReply::Overview(o) => o,
        other => panic!("{other:?}"),
    };
    assert_eq!(o.next_day, None);
    assert!(o.next_visits.is_empty());
    refused(
        &mut core,
        ClinicRequest::Overview {
            today: Today {
                date: TODAY.to_owned(),
                minute: 1_440,
            },
        },
    );
}

// ------------------------------------------------------------------ erasing a patient (D-174)

fn erase(patient: &Patient, name: &str) -> ClinicRequest {
    ClinicRequest::ErasePatient {
        id: patient.id.clone(),
        name: name.to_owned(),
    }
}

#[test]
fn a_card_where_nothing_happened_is_erased_with_its_name_typed() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם לביא");
    let tamar = new_patient(&mut core, "תמר");
    // Plans only: two visits ahead and a task.
    let planned = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Intake, "2026-11-20", 540),
    );
    save_visit(
        &mut core,
        Visit {
            fee: 40_000,
            ..a_visit(&noam, VisitKind::Intake, "2026-11-27", 540)
        },
    );
    save_task(
        &mut core,
        ClinicTask {
            patient_id: Some(noam.id.clone()),
            title: "לחזור להורים".to_owned(),
            ..ClinicTask::default()
        },
    );
    let theirs = save_visit(
        &mut core,
        a_visit(&tamar, VisitKind::Intake, "2026-11-20", 700),
    );
    let link = create_case(&mut core, &noam);

    // The name has to be typed as it is on the card.
    for wrong in ["נועם", "", "נועם לביאא", "תמר"] {
        let text = refused(&mut core, erase(&noam, wrong));
        assert!(text.contains("השם"), "{text}");
    }
    assert_eq!(rows(&mut core).len(), 2);
    // Spaces around and between are not a difference.
    ask(&mut core, erase(&noam, "  נועם   לביא "));

    let left = rows(&mut core);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].patient.id, tamar.id);
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::PatientCard {
                id: noam.id.clone(),
                today: now()
            }
        ),
        "לא נמצא: המטופל"
    );
    // Their visits, their task and the history of their records are gone; the other
    // family's are all there.
    let month = calendar(&mut core, "2026-11-01", "2026-11-30");
    assert_eq!(month.len(), 1);
    assert_eq!(month[0].visit.id, theirs.id);
    assert!(tasks(&mut core).is_empty());
    not_found(
        &mut core,
        ClinicRequest::History {
            record_id: planned.id.clone(),
        },
    );
    assert_eq!(actions(&mut core, &theirs.id), ["created"]);
    // The case is never touched.
    assert!(core.list_cases().unwrap().iter().any(|c| c.id == link.id));
    assert_eq!(
        not_found(&mut core, erase(&noam, "נועם לביא")),
        "לא נמצא: המטופל"
    );

    // In the log: that a card was erased and how many records, never who.
    let log = core.activity(None).unwrap();
    let entry = log
        .entries
        .iter()
        .find(|e| e.event == "clinic_patient_erased")
        .unwrap();
    assert_eq!(entry.text, "כרטיס מטופל נמחק מהקליניקה, ואיתו 3 רישומים");
    assert!(entry.warn && entry.case.is_none());
    assert!(log.entries.iter().all(|e| !e.text.contains("נועם")));
    assert!(log.intact);
}

#[test]
fn a_card_where_something_happened_is_closed_not_erased() {
    let (_dir, mut core) = open_clinic();
    // Seven years from the core's own today; the children here are 25 long before that.
    let from = rules::display_date(&dates::iso(dates::add_years(
        dates::today(),
        KEEP_YEARS_AFTER_LAST_CHANGE,
    )));
    let grown = |name: &str| Patient {
        birth_date: Some("2000-05-05".to_owned()),
        created_on: "2015-01-01".to_owned(),
        ..child(name)
    };
    let check = |core: &mut Core, patient: &Patient, what: &str| {
        let text = refused(core, erase(patient, &patient.name));
        assert!(text.contains("לסגור את הכרטיס"), "{what}: {text}");
        assert!(text.contains(&from), "{what}: {text} (expected {from})");
        assert_eq!(card(core, &patient.id).patient.id, patient.id, "{what}");
    };

    let held = save_patient(&mut core, grown("אורי"));
    let visit = save_visit(
        &mut core,
        a_visit(&held, VisitKind::Intake, "2015-02-01", 540),
    );
    mark(&mut core, &visit, VisitStatus::Held);
    check(&mut core, &held, "a held visit");

    let charged = save_patient(&mut core, grown("גיל"));
    let visit = save_visit(
        &mut core,
        Visit {
            fee: 30_000,
            ..a_visit(&charged, VisitKind::Therapy, "2015-02-01", 540)
        },
    );
    save_visit(
        &mut core,
        Visit {
            status: VisitStatus::Cancelled,
            charge_missed: true,
            ..visit
        },
    );
    check(&mut core, &charged, "a cancelled visit that is charged");

    let noted = save_patient(&mut core, grown("מיכל"));
    save_note(&mut core, a_note(&noted, None, "שיחת טלפון."));
    check(&mut core, &noted, "a note");

    let paid = save_patient(&mut core, grown("רון"));
    let payment = save_payment(&mut core, a_payment(&paid, "2015-02-01", 10_000));
    check(&mut core, &paid, "a payment");
    // Voided is still something that happened: the diary keeps it.
    ask(
        &mut core,
        ClinicRequest::VoidPayment {
            id: payment.id,
            reason: "טעות".to_owned(),
        },
    );
    check(&mut core, &paid, "a voided payment");

    let billed = save_patient(&mut core, grown("שי"));
    let charge = save_charge(&mut core, a_charge(&billed, "2015-02-01", 10_000));
    ask(
        &mut core,
        ClinicRequest::VoidCharge {
            id: charge.id,
            reason: "טעות".to_owned(),
        },
    );
    check(&mut core, &billed, "a voided charge");

    // A young child: not before the 25th birthday, when that is later.
    let young = save_patient(
        &mut core,
        Patient {
            birth_date: Some("2021-03-10".to_owned()),
            ..child("נועה")
        },
    );
    save_note(&mut core, a_note(&young, None, "שיחת טלפון."));
    let text = refused(&mut core, erase(&young, "נועה"));
    assert!(text.contains("10.03.2046"), "{text}");

    assert_eq!(rows(&mut core).len(), 6);
    assert_eq!(audit_count(&mut core, "clinic_patient_erased"), 0);
    // What the rule offers instead works: the card is closed and everything stays.
    let closed = save_patient(
        &mut core,
        Patient {
            status: PatientStatus::Closed,
            ..young.clone()
        },
    );
    assert_eq!(closed.status, PatientStatus::Closed);
    assert_eq!(card(&mut core, &young.id).notes.len(), 1);
}

// ------------------------------------------------------------------ the link to cases

fn family_with_three_contacts(core: &mut Core) -> Patient {
    save_patient(
        core,
        Patient {
            name: "אביתר שגב".to_owned(),
            birth_date: Some("2021-03-10".to_owned()),
            gender: Some(GrammaticalGender::Male),
            contacts: vec![
                contact(ContactRole::Mother, "מורן שגב"),
                contact(ContactRole::Father, "עידו שגב"),
                contact(ContactRole::Guardian, "ציפורה לנדאו"),
                // A contact with a phone and no name is no one to register.
                Contact {
                    role: ContactRole::Other,
                    phone: "000-0000".to_owned(),
                    ..Contact::default()
                },
            ],
            ..child("")
        },
    )
}

#[test]
fn a_case_is_opened_from_the_card_with_everyone_named_in_it() {
    let (_dir, mut core) = open_clinic();
    let family = family_with_three_contacts(&mut core);
    let link = create_case(&mut core, &family);
    assert_eq!(link.code, "תיק-2026-1115");
    assert_eq!((link.approved, link.in_trash), (0, false));
    assert!(link.total >= 15);
    let detail = core.case_detail(&link.id).unwrap();
    assert_eq!(detail.meta.consent, None);
    let age = detail.meta.age.unwrap();
    assert_eq!((age.years, age.months), (5, 8));
    assert_eq!(detail.meta.child_gender, Some(GrammaticalGender::Male));
    let people: Vec<(Role, &str)> = detail
        .identities
        .iter()
        .map(|i| (i.role, i.value.as_str()))
        .collect();
    assert_eq!(
        people,
        [
            (Role::Child, "אביתר שגב"),
            (Role::Mother, "מורן שגב"),
            (Role::Father, "עידו שגב"),
            (Role::Other, "ציפורה לנדאו"),
        ]
    );
    assert!(detail
        .identities
        .iter()
        .all(|i| i.source == IdentitySource::Manual));
    // The card keeps the case, and shows it.
    let c = card(&mut core, &family.id);
    assert_eq!(c.patient.case_id.as_deref(), Some(link.id.as_str()));
    assert_eq!(c.case, Some(link.clone()));
    // Saving the card never changes the link.
    let saved = save_patient(
        &mut core,
        Patient {
            case_id: None,
            notes: "עודכן".to_owned(),
            ..c.patient.clone()
        },
    );
    assert_eq!(saved.case_id.as_deref(), Some(link.id.as_str()));

    // One case for a patient.
    let text = refused(
        &mut core,
        ClinicRequest::CreateCase {
            patient_id: family.id.clone(),
            today: now(),
        },
    );
    assert!(text.contains("כבר יש תיק"), "{text}");
    // A second case on the same day takes the next code.
    let other = new_patient(&mut core, "תמר");
    assert_eq!(create_case(&mut core, &other).code, "תיק-2026-1115-2");
    let third = new_patient(&mut core, "גיל");
    assert_eq!(create_case(&mut core, &third).code, "תיק-2026-1115-3");

    // An adult on a card: the case takes no age it would refuse.
    let adult = save_patient(
        &mut core,
        Patient {
            birth_date: Some("2000-06-01".to_owned()),
            contacts: Vec::new(),
            ..child("בוגר")
        },
    );
    let link = create_case(&mut core, &adult);
    assert_eq!(core.case_detail(&link.id).unwrap().meta.age, None);

    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::CreateCase {
                patient_id: "אין".to_owned(),
                today: now()
            }
        ),
        "לא נמצא: המטופל"
    );
    refused(
        &mut core,
        ClinicRequest::CreateCase {
            patient_id: family.id,
            today: Today {
                date: "היום".to_owned(),
                minute: 0,
            },
        },
    );
}

#[test]
fn a_case_is_linked_to_one_patient_and_can_be_unlinked() {
    let (_dir, mut core, existing) = setup(None);
    let noam = new_patient(&mut core, "נועם");
    let tamar = new_patient(&mut core, "תמר");
    let choices = |core: &mut Core| match ask(core, ClinicRequest::CaseChoices) {
        ClinicReply::Cases(c) => c,
        other => panic!("{other:?}"),
    };
    let offered = choices(&mut core);
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].id, existing);
    assert_eq!(offered[0].label, "אלון · TEST-0002");

    let linked = link_case(&mut core, &noam, Some(&existing));
    assert_eq!(linked.case_id.as_deref(), Some(existing.as_str()));
    let shown = card(&mut core, &noam.id).case.unwrap();
    assert_eq!(
        (shown.id.as_str(), shown.code.as_str()),
        (existing.as_str(), "TEST-0002")
    );
    // Linked: not offered again, and not to be had by another patient.
    assert!(choices(&mut core).is_empty());
    let text = refused(
        &mut core,
        ClinicRequest::LinkCase {
            patient_id: tamar.id.clone(),
            case_id: Some(existing.clone()),
        },
    );
    assert!(text.contains("מטופל אחר"), "{text}");
    // Linking it again to the same patient changes nothing.
    link_case(&mut core, &noam, Some(&existing));

    let unlinked = link_case(&mut core, &noam, None);
    assert_eq!(unlinked.case_id, None);
    assert_eq!(card(&mut core, &noam.id).case, None);
    assert_eq!(choices(&mut core).len(), 1);
    link_case(&mut core, &tamar, Some(&existing));

    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::LinkCase {
                patient_id: noam.id.clone(),
                case_id: Some("אין".to_owned()),
            }
        ),
        "לא נמצא: התיק"
    );
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::LinkCase {
                patient_id: "אין".to_owned(),
                case_id: None,
            }
        ),
        "לא נמצא: המטופל"
    );
    // The case's materials were never changed by any of it. Its people only grew (D-177):
    // the names of each card that was linked are names its filter knows, and stay so.
    let detail = core.case_detail(&existing).unwrap();
    assert_eq!(detail.inputs.len(), 2);
    let child_of_case = detail
        .identities
        .iter()
        .find(|i| i.role == Role::Child)
        .unwrap();
    assert_eq!(child_of_case.value, "אלון");
    assert_eq!(child_of_case.aliases, ["נועם", "תמר"]);
    assert!(detail
        .identities
        .iter()
        .any(|i| i.role == Role::Mother && i.value == "דנה"));
    assert_eq!(detail.identities.len(), 3);
}

#[test]
fn a_case_in_the_bin_is_shown_as_such_and_a_purged_one_reads_as_no_case() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let tamar = new_patient(&mut core, "תמר");
    let visit = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Assessment, "2026-10-01", 540),
    );
    let visit = mark(&mut core, &visit, VisitStatus::Held);
    let note = save_note(&mut core, a_note(&noam, Some(&visit), "עבד בריכוז."));
    let link = create_case(&mut core, &noam);
    let note = note_to_case(&mut core, &note);
    assert!(note.case_input_id.is_some());

    // Into the recycle bin: the card says so, and nothing new goes into the case.
    core.delete_case(&link.id).unwrap();
    let c = card(&mut core, &noam.id);
    let shown = c.case.clone().unwrap();
    assert!(shown.in_trash);
    assert_eq!(shown.id, link.id);
    let text = refused(
        &mut core,
        ClinicRequest::NoteToCase {
            id: note.id.clone(),
        },
    );
    assert!(text.contains("סל המחזור"), "{text}");
    // Not offered, and not linked to anyone while it is there.
    assert!(matches!(
        ask(&mut core, ClinicRequest::CaseChoices),
        ClinicReply::Cases(c) if c.is_empty()
    ));
    let unlinked = link_case(&mut core, &noam, None);
    assert_eq!(unlinked.case_id, None);
    let text = refused(
        &mut core,
        ClinicRequest::LinkCase {
            patient_id: tamar.id.clone(),
            case_id: Some(link.id.clone()),
        },
    );
    assert!(text.contains("סל המחזור"), "{text}");
    // The card still holds its case while it is in the bin, and has no second one.
    core.restore_case(&link.id).unwrap();
    link_case(&mut core, &noam, Some(&link.id));
    core.delete_case(&link.id).unwrap();
    refused(
        &mut core,
        ClinicRequest::CreateCase {
            patient_id: noam.id.clone(),
            today: now(),
        },
    );

    // Erased for good: the card's link points nowhere, and that is "no case" everywhere.
    core.purge_case(&link.id, PASSWORD).unwrap();
    let c = card(&mut core, &noam.id);
    assert_eq!(c.case, None);
    assert_eq!(c.patient.case_id.as_deref(), Some(link.id.as_str()));
    assert!(step(&c, "report").detail.contains("עוד לא נפתח תיק"));
    overview(&mut core);
    assert_eq!(rows(&mut core).len(), 2);
    assert!(matches!(
        ask(&mut core, ClinicRequest::CaseChoices),
        ClinicReply::Cases(c) if c.is_empty()
    ));
    let text = refused(
        &mut core,
        ClinicRequest::NoteToCase {
            id: note.id.clone(),
        },
    );
    assert!(text.contains("אין תיק"), "{text}");
    not_found(
        &mut core,
        ClinicRequest::LinkCase {
            patient_id: tamar.id.clone(),
            case_id: Some(link.id.clone()),
        },
    );
    // The card offers to open another, and the record goes into the new one.
    let fresh = create_case(&mut core, &noam);
    assert_ne!(fresh.id, link.id);
    assert_eq!(card(&mut core, &noam.id).case, Some(fresh.clone()));
    let again = note_to_case(&mut core, &note);
    assert_ne!(again.case_input_id, note.case_input_id);
    assert_eq!(core.case_detail(&fresh.id).unwrap().inputs.len(), 1);
}

// ------------------------------------------------------------------ a note into a case

/// Put only in the personal note: it must never be seen outside the clinic.
const PERSONAL_CANARY: &str = "קנרית-אישית-בדויה-7731";

#[test]
fn only_the_formal_record_goes_into_the_case_and_a_second_send_replaces_it() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let intake = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Intake, "2026-10-12", 540),
    );
    let observed = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Observation, "2026-10-14", 540),
    );
    let session = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Assessment, "2026-10-19", 540),
    );
    let note = save_note(
        &mut core,
        SessionNote {
            personal: format!("רק בשבילי: {PERSONAL_CANARY}"),
            ..a_note(&noam, Some(&intake), "ההורים תיארו קושי בהתארגנות בבוקר.")
        },
    );

    // No case yet: nowhere to put it.
    let text = refused(
        &mut core,
        ClinicRequest::NoteToCase {
            id: note.id.clone(),
        },
    );
    assert!(text.contains("אין תיק"), "{text}");
    let link = create_case(&mut core, &noam);

    let sent = note_to_case(&mut core, &note);
    let input_id = sent.case_input_id.clone().unwrap();
    assert_eq!(sent.personal, note.personal);
    let inputs = core.case_detail(&link.id).unwrap().inputs;
    assert_eq!(inputs.len(), 1);
    let material = &inputs[0];
    assert_eq!(material.id, input_id);
    assert_eq!(material.kind, InputKind::Intake);
    assert_eq!(material.title, "סיכום פגישת היכרות · 12.10.2026");
    assert_eq!(material.content, "ההורים תיארו קושי בהתארגנות בבוקר.");
    assert!(!material.content.contains(PERSONAL_CANARY));
    assert!(!material.title.contains(PERSONAL_CANARY));

    // She corrects the record and sends again: the same material, replaced.
    let edited = save_note(
        &mut core,
        SessionNote {
            record: "ההורים תיארו קושי בהתארגנות בבוקר ובשעת השינה.".to_owned(),
            ..sent.clone()
        },
    );
    // The link to the material is the core's: an edit keeps it whatever is sent.
    assert_eq!(edited.case_input_id.as_deref(), Some(input_id.as_str()));
    let again = note_to_case(&mut core, &edited);
    assert_eq!(again.case_input_id.as_deref(), Some(input_id.as_str()));
    let inputs = core.case_detail(&link.id).unwrap().inputs;
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].content,
        "ההורים תיארו קושי בהתארגנות בבוקר ובשעת השינה."
    );

    // An observation, a testing session and a note on its own: each of its kind.
    let observation = save_note(&mut core, a_note(&noam, Some(&observed), "שיחק לבד בחצר."));
    let testing = save_note(&mut core, a_note(&noam, Some(&session), "עבד בריכוז."));
    let call = save_note(
        &mut core,
        SessionNote {
            date: "2026-10-20".to_owned(),
            ..a_note(&noam, None, "שיחה עם האם.")
        },
    );
    for n in [&observation, &testing, &call] {
        note_to_case(&mut core, n);
    }
    let inputs = core.case_detail(&link.id).unwrap().inputs;
    let mut seen: Vec<(InputKind, &str)> =
        inputs.iter().map(|i| (i.kind, i.title.as_str())).collect();
    seen.sort_by_key(|(_, title)| title.to_owned());
    assert_eq!(
        seen,
        [
            (InputKind::SessionNote, "סיכום מפגש אבחון · 19.10.2026"),
            (InputKind::Intake, "סיכום פגישת היכרות · 12.10.2026"),
            (InputKind::SessionNote, "סיכום שיחה · 20.10.2026"),
            (InputKind::Observation, "סיכום תצפית · 14.10.2026"),
        ]
    );
    // Nothing of the personal note is anywhere in the case.
    for input in &inputs {
        assert!(!input.content.contains(PERSONAL_CANARY));
        assert!(!input.content.contains("רק בשבילי"));
    }

    // She deleted the material in the case: the next send adds it anew.
    core.delete_input(&link.id, &input_id).unwrap();
    let anew = note_to_case(&mut core, &again);
    assert_ne!(anew.case_input_id.as_deref(), Some(input_id.as_str()));
    assert_eq!(core.case_detail(&link.id).unwrap().inputs.len(), 4);

    // A note with a personal text only has nothing to send.
    let personal_only = save_note(
        &mut core,
        SessionNote {
            personal: PERSONAL_CANARY.to_owned(),
            ..a_note(&noam, None, "")
        },
    );
    let text = refused(
        &mut core,
        ClinicRequest::NoteToCase {
            id: personal_only.id,
        },
    );
    assert!(text.contains("הרשומה ריקה"), "{text}");
    assert_eq!(
        not_found(
            &mut core,
            ClinicRequest::NoteToCase {
                id: "אין".to_owned()
            }
        ),
        "לא נמצא: הסיכום"
    );
    assert_eq!(core.case_detail(&link.id).unwrap().inputs.len(), 4);

    // In the log: each send, with the case, and nothing of the text.
    let log = core.activity(None).unwrap();
    let sends: Vec<_> = log
        .entries
        .iter()
        .filter(|e| e.event == "clinic_note_to_case")
        .collect();
    assert_eq!(sends.len(), 6);
    for entry in sends {
        assert_eq!(entry.text, "סיכום פגישה מהקליניקה הועבר לחומרי התיק");
        assert!(entry.case.is_some());
    }
    assert!(log.intact);
    // The note's own history says it was sent, and still never the personal text.
    let entries = history(&mut core, &note.id);
    assert!(entries
        .last()
        .unwrap()
        .lines
        .contains(&"הועבר לחומרי התיק: כן".to_owned()));
    assert!(entries
        .iter()
        .all(|e| e.lines.iter().all(|l| !l.contains(PERSONAL_CANARY))));
}

/// The reviewer's canary: a record sent to a case names the child and every contact of the
/// card. Before anything of that case goes to the AI, the case's own filter hides them all,
/// because opening the case from the card registered every one of them.
#[test]
fn a_record_sent_to_a_case_goes_out_with_every_name_of_the_card_hidden() {
    let fake = FakeTransport::default();
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), Some(Box::new(fake.clone())));
    core.create_vault(PASSWORD).unwrap();
    core.set_practitioner(vec!["ד\"ר רותם אלמוג".into()])
        .unwrap();
    let family = family_with_three_contacts(&mut core);
    let link = create_case(&mut core, &family);
    let visit = save_visit(
        &mut core,
        a_visit(&family, VisitKind::Intake, "2026-10-12", 540),
    );
    let record = "אביתר שגב הגיע לפגישה עם מורן שגב. עידו שגב הצטרף בהמשך, \
                  וציפורה לנדאו חיכתה בחוץ. מורן שגב סיפרה כי אביתר שגב מתקשה בהתארגנות בבוקר. \
                  לדברי עידו שגב הקושי בולט בעיקר בימי ראשון, וציפורה לנדאו מסכימה איתו.";
    let note = save_note(
        &mut core,
        SessionNote {
            personal: format!("{PERSONAL_CANARY}: להתייעץ על המשפחה"),
            ..a_note(&family, Some(&visit), record)
        },
    );
    note_to_case(&mut core, &note);
    let detail = core.case_detail(&link.id).unwrap();
    assert_eq!(detail.inputs.len(), 1);
    let material = detail.inputs[0].clone();
    assert_eq!(material.content, record);

    let registered = ["אביתר שגב", "מורן שגב", "עידו שגב", "ציפורה לנדאו"];
    let parts = ["אביתר", "מורן", "עידו", "ציפורה", "לנדאו", "שגב"];
    let clean = |what: &str, text: &str| {
        for name in registered.iter().chain(&parts) {
            assert!(!text.contains(name), "{name} is in {what}: {text}");
        }
        assert!(!text.contains(PERSONAL_CANARY), "{what}");
    };

    // The filter the review screen and every request use, on the material itself.
    let filtered = core.preview_filter(&link.id, &material.content).unwrap();
    clean("the filtered material", &filtered.tagged);
    assert!(filtered.tagged.contains("[ילד]"), "{}", filtered.tagged);
    assert!(filtered.tagged.contains("[אם]"), "{}", filtered.tagged);
    assert!(filtered.tagged.contains("[אב]"), "{}", filtered.tagged);
    assert!(filtered.tagged.contains("מתקשה בהתארגנות בבוקר"));

    // And the whole way: her consent, a request for every section that has material, sent.
    let mut meta = detail.meta.clone();
    meta.consent = Some(consent());
    core.update_case(&link.id, meta).unwrap();
    let drafts = core.prepare_full_draft(&link.id).unwrap();
    assert!(!drafts.is_empty());
    let mut went_out = String::new();
    for (key, prepared) in drafts {
        let shown = outgoing(&prepared);
        clean(&format!("the review of section {key}"), &shown);
        went_out.push_str(&shown);
        if let Some(approval) = prepared.approval_id {
            core.send_section(&approval).unwrap();
        }
    }
    assert!(
        went_out.contains("מתקשה בהתארגנות בבוקר"),
        "the record was in no request"
    );
    let sent = fake.sent.lock().unwrap().clone();
    assert!(!sent.is_empty());
    for payload in &sent {
        clean("what left the machine", payload);
    }
    assert!(sent.iter().any(|p| p.contains("מתקשה בהתארגנות בבוקר")));
}

// ------------------------------------------------------------------ staying in the vault

/// A small clinic with one of everything, and the ids to find it again.
fn seed(core: &mut Core) -> (Patient, Visit, SessionNote, Payment, Charge, ClinicTask) {
    let noam = new_patient(core, "נועם");
    let visit = save_visit(
        core,
        Visit {
            fee: 40_000,
            ..a_visit(&noam, VisitKind::Assessment, "2026-11-10", 540)
        },
    );
    let visit = mark(core, &visit, VisitStatus::Held);
    let note = save_note(
        core,
        SessionNote {
            personal: "מחשבה-פרטית-בדויה".to_owned(),
            ..a_note(&noam, Some(&visit), "עבד בריכוז.")
        },
    );
    let payment = save_payment(core, a_payment(&noam, "2026-11-10", 40_000));
    let charge = save_charge(core, a_charge(&noam, "2026-11-11", 150_000));
    let task = save_task(
        core,
        ClinicTask {
            patient_id: Some(noam.id.clone()),
            title: "לשלוח שאלון".to_owned(),
            due: Some("2026-11-20".to_owned()),
            ..ClinicTask::default()
        },
    );
    let mine = ClinicSettings {
        report_days: 30,
        ..ClinicSettings::default()
    };
    ask(core, ClinicRequest::SaveSettings { settings: mine });
    (noam, visit, note, payment, charge, task)
}

fn seeded_is_all_there(
    core: &mut Core,
    seeded: &(Patient, Visit, SessionNote, Payment, Charge, ClinicTask),
) {
    let (noam, visit, note, payment, charge, task) = seeded;
    let c = card(core, &noam.id);
    assert_eq!(&c.patient, noam);
    assert_eq!(c.visits.len(), 1);
    assert_eq!(&c.visits[0].visit, visit);
    assert!(c.visits[0].has_note);
    assert_eq!(c.notes, std::slice::from_ref(note));
    assert_eq!(c.payments, std::slice::from_ref(payment));
    assert_eq!(c.charges, std::slice::from_ref(charge));
    assert_eq!(c.tasks, std::slice::from_ref(task));
    assert_eq!(c.balance.due, 150_000);
    assert_eq!(actions(core, &visit.id), ["created", "changed"]);
    assert_eq!(actions(core, &payment.id), ["created"]);
    match ask(core, ClinicRequest::Settings) {
        ClinicReply::Settings(s) => assert_eq!(s.report_days, 30),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_clinic_survives_a_lock_and_an_unlock() {
    let (dir, mut core) = open_clinic();
    let seeded = seed(&mut core);
    core.lock();
    assert!(matches!(
        core.clinic(ClinicRequest::Settings),
        Err(CoreError::Locked)
    ));
    core.unlock(PASSWORD).unwrap();
    seeded_is_all_there(&mut core, &seeded);
    // And a restart of the program.
    core.lock();
    drop(core);
    let mut core = Core::for_tests(dir.path(), None);
    core.unlock(PASSWORD).unwrap();
    seeded_is_all_there(&mut core, &seeded);

    // Nothing of it is readable in the vault's files.
    core.lock();
    let mut stack = vec![dir.path().to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            for needle in ["נועם", "עבד בריכוז", "מחשבה-פרטית-בדויה", "לשלוח שאלון"]
            {
                let needle = needle.as_bytes();
                assert!(
                    !bytes.windows(needle.len()).any(|w| w == needle),
                    "{path:?}"
                );
            }
        }
    }
}

#[test]
fn the_clinic_is_in_the_backup_and_comes_back_with_it() {
    let (_dir, mut core) = open_clinic();
    let seeded = seed(&mut core);
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b")).unwrap();
    let bytes = std::fs::read(&done.path).unwrap();

    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    other.stage_backup(bytes, "b.vaultbak".into()).unwrap();
    other.restore_staged_backup(Some(PASSWORD), None).unwrap();
    seeded_is_all_there(&mut other, &seeded);
    assert!(other.activity(None).unwrap().intact);
}

// ------------------------------------------------------------------ the log

#[test]
fn the_log_tells_of_the_clinic_in_her_words_and_names_no_one() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    ask(&mut core, erase(&noam, "נועם"));
    let log = core.activity(None).unwrap();
    let said: Vec<(&str, &str)> = log
        .entries
        .iter()
        .filter(|e| e.event.starts_with("clinic_"))
        .map(|e| (e.event.as_str(), e.text.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("clinic_patient_erased", "כרטיס מטופל נמחק מהקליניקה"),
            ("clinic_patient_added", "נפתח כרטיס מטופל בקליניקה"),
        ]
    );
    for entry in &log.entries {
        assert!(!entry.text.contains("נועם"));
        assert!(!entry.text.contains('_'), "{}", entry.text);
    }
}

// ------------------------------------------------------------------ the review of the beta

fn try_save_visit(core: &mut Core, visit: Visit) -> ClinicRequest {
    let _ = core;
    ClinicRequest::SaveVisit {
        visit,
        repeat: None,
        scope: Scope::One,
    }
}

fn delete_visit(visit: &Visit, scope: Scope) -> ClinicRequest {
    ClinicRequest::DeleteVisit {
        id: visit.id.clone(),
        scope,
    }
}

/// D-176 (the reviewer's first finding): a visit that was held and charged, put back to
/// "planned", could be deleted, and its card erased at once as if nothing had happened.
#[test]
fn a_visit_that_was_ever_marked_is_never_deleted_and_its_card_is_not_empty() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let visit = save_visit(
        &mut core,
        Visit {
            fee: 40_000,
            ..a_visit(&noam, VisitKind::Therapy, "2026-11-10", 540)
        },
    );
    let held = mark(&mut core, &visit, VisitStatus::Held);
    assert_eq!(card(&mut core, &noam.id).balance.due, 40_000);
    // A wrong click is corrected: no note, nothing paid. The history keeps both states.
    let back = mark(&mut core, &held, VisitStatus::Planned);
    assert_eq!(back.status, VisitStatus::Planned);
    assert_eq!(card(&mut core, &noam.id).balance.due, 0);
    assert_eq!(
        actions(&mut core, &visit.id),
        ["created", "changed", "changed"]
    );
    // It says "planned" again, and still it stays: it was held once.
    for scope in [Scope::One, Scope::Following] {
        let text = refused(&mut core, delete_visit(&back, scope));
        assert!(text.contains("כבר סומנה פעם"), "{text}");
    }
    assert_eq!(calendar(&mut core, "2026-11-10", "2026-11-10").len(), 1);
    // And the card is one where something happened.
    let text = refused(&mut core, erase(&noam, "נועם"));
    assert!(text.contains("לסגור את הכרטיס"), "{text}");
    assert_eq!(rows(&mut core).len(), 1);
    assert_eq!(audit_count(&mut core, "clinic_patient_erased"), 0);

    // A visit that was cancelled without a charge, or missed, happened too.
    for (name, status) in [
        ("תמר", VisitStatus::Cancelled),
        ("גיל", VisitStatus::NoShow),
    ] {
        let patient = new_patient(&mut core, name);
        let visit = save_visit(
            &mut core,
            a_visit(&patient, VisitKind::Intake, "2026-11-11", 600),
        );
        mark(&mut core, &visit, status);
        let text = refused(&mut core, erase(&patient, name));
        assert!(text.contains("לסגור את הכרטיס"), "{status:?}: {text}");
    }
}

#[test]
fn a_mark_is_taken_back_only_while_nothing_leans_on_it() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let priced = |date: &str| Visit {
        fee: 40_000,
        ..a_visit(&noam, VisitKind::Therapy, date, 540)
    };
    // With a session note: the note says the session happened.
    let noted = save_visit(&mut core, priced("2026-11-03"));
    let noted = mark(&mut core, &noted, VisitStatus::Held);
    save_note(&mut core, a_note(&noam, Some(&noted), "התקיימה פגישה."));
    let planned = Visit {
        status: VisitStatus::Planned,
        ..noted.clone()
    };
    let request = try_save_visit(&mut core, planned);
    let text = refused(&mut core, request);
    assert!(
        text.contains("יש סיכום") && text.contains("כמבוטלת"),
        "{text}"
    );
    // Paid: the money received would be left with no session behind it.
    let paid = save_visit(&mut core, priced("2026-11-04"));
    let paid = mark(&mut core, &paid, VisitStatus::Held);
    let payment = save_payment(&mut core, a_payment(&noam, "2026-11-04", 80_000));
    let planned = Visit {
        status: VisitStatus::Planned,
        ..paid.clone()
    };
    let request = try_save_visit(&mut core, planned.clone());
    let text = refused(&mut core, request);
    assert!(text.contains("כבר נרשם תשלום"), "{text}");
    assert!(text.contains("לבטל קודם את התקבול"), "{text}");
    let before = diary(&mut core, "2026-11");
    assert_eq!((before.charged, before.received), (80_000, 80_000));
    // Any other change of a mark is always possible, and the history keeps it.
    let cancelled = save_visit(
        &mut core,
        Visit {
            status: VisitStatus::Cancelled,
            status_note: "סומנה בטעות".to_owned(),
            ..paid.clone()
        },
    );
    assert_eq!(cancelled.status, VisitStatus::Cancelled);
    mark(&mut core, &cancelled, VisitStatus::Held);
    // Once the payment is voided, the wrong click can be corrected.
    ask(
        &mut core,
        ClinicRequest::VoidPayment {
            id: payment.id,
            reason: "נרשם בטעות".to_owned(),
        },
    );
    let request = try_save_visit(&mut core, planned);
    assert!(matches!(core.clinic(request), Ok(ClinicReply::Saved(_))));
    // A cancelled visit that cost nothing goes back freely, and still is never deleted.
    let free = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Meeting, "2026-11-05", 540),
    );
    let free = mark(&mut core, &free, VisitStatus::Cancelled);
    let free = mark(&mut core, &free, VisitStatus::Planned);
    refused(&mut core, delete_visit(&free, Scope::One));
}

#[test]
fn deleting_the_rest_of_a_series_leaves_a_visit_that_was_ever_marked() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let series = save_visits(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, "2026-12-01", 540),
        Some(Repeat {
            every_weeks: 1,
            count: 4,
        }),
        Scope::One,
    )
    .visits;
    let third = series[2].visit.clone();
    let marked = mark(&mut core, &third, VisitStatus::NoShow);
    mark(&mut core, &marked, VisitStatus::Planned);
    ask(&mut core, delete_visit(&series[0].visit, Scope::Following));
    let left = calendar(&mut core, "2026-12-01", "2026-12-31");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].visit.id, third.id);
}

/// D-177 (the reviewer's second finding): a record never moves to another card, so one
/// card's erase or change never reaches another card's money or its history.
#[test]
fn a_payment_a_charge_and_a_visit_never_move_to_another_card() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let tamar = new_patient(&mut core, "תמר");
    let payment = save_payment(&mut core, a_payment(&noam, "2026-11-02", 10_000));
    let text = refused(
        &mut core,
        ClinicRequest::SavePayment {
            payment: Payment {
                patient_id: tamar.id.clone(),
                ..payment.clone()
            },
        },
    );
    assert!(
        text.contains("אי אפשר להעביר תקבול") && text.contains("לבטל"),
        "{text}"
    );
    let charge = save_charge(&mut core, a_charge(&noam, "2026-11-02", 10_000));
    let text = refused(
        &mut core,
        ClinicRequest::SaveCharge {
            charge: Charge {
                patient_id: tamar.id.clone(),
                ..charge.clone()
            },
        },
    );
    assert!(
        text.contains("אי אפשר להעביר חיוב") && text.contains("לבטל"),
        "{text}"
    );
    let visit = save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Intake, "2026-11-20", 540),
    );
    let moved = Visit {
        patient_id: Some(tamar.id.clone()),
        ..visit.clone()
    };
    let request = try_save_visit(&mut core, moved);
    let text = refused(&mut core, request);
    assert!(text.contains("אי אפשר להעביר פגישה"), "{text}");
    // Nor does a patient's visit become time she blocked for herself.
    let blocked = Visit {
        patient_id: None,
        kind: VisitKind::Block,
        ..visit.clone()
    };
    let request = try_save_visit(&mut core, blocked);
    let text = refused(&mut core, request);
    assert!(text.contains("לזמן שמור"), "{text}");
    // Nothing moved, and nothing was written.
    let c = card(&mut core, &noam.id);
    assert_eq!(
        (c.payments.len(), c.charges.len(), c.visits.len()),
        (1, 1, 1)
    );
    let c = card(&mut core, &tamar.id);
    assert_eq!(
        (c.payments.len(), c.charges.len(), c.visits.len()),
        (0, 0, 0)
    );
    for id in [&payment.id, &charge.id, &visit.id] {
        assert_eq!(actions(&mut core, id), ["created"]);
    }
    // Time she blocked for herself can still become a visit with a patient.
    let block = save_visit(
        &mut core,
        Visit {
            patient_id: None,
            ..a_visit(&noam, VisitKind::Block, "2026-11-21", 540)
        },
    );
    let given = save_visit(
        &mut core,
        Visit {
            patient_id: Some(tamar.id.clone()),
            kind: VisitKind::Intake,
            ..block
        },
    );
    assert_eq!(given.patient_id.as_deref(), Some(tamar.id.as_str()));
}

fn raw_put(core: &mut Core, kind: &str, patient: &Patient, date: &str, json: &str) -> String {
    core.vault_mut()
        .unwrap()
        .clinic_put(
            kind,
            None,
            Some(&patient.id),
            rules::day_number(date).unwrap(),
            json,
            Some("created"),
        )
        .unwrap()
}

/// D-178 (the reviewer's third finding): a value of a later version is shown as "unknown"
/// and never turned into something else.
#[test]
fn a_value_this_version_does_not_know_is_shown_and_never_rewritten() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let json = format!(
        r#"{{"patient_id":"{}","date":"2026-11-02","amount":12000,"method":"crypto_coin","receipt":"7"}}"#,
        noam.id
    );
    let id = raw_put(&mut core, "payment", &noam, "2026-11-02", &json);
    let c = card(&mut core, &noam.id);
    assert_eq!(c.payments.len(), 1);
    assert_eq!(c.payments[0].method, PayMethod::Unknown);
    assert_eq!(c.balance.paid, 12_000);
    // The diary counts it, under a method of its own, so the methods still add up.
    let month = diary(&mut core, "2026-11");
    assert_eq!(month.received, 12_000);
    assert_eq!(month.by_method.len(), 1);
    assert_eq!(month.by_method[0].method, PayMethod::Unknown);
    let lines = history(&mut core, &id).remove(0).lines;
    assert!(
        lines.iter().any(|l| l.contains("אמצעי לא מוכר")),
        "{lines:?}"
    );
    // Nothing writes it back as it is: not a save, not a void.
    let text = refused(
        &mut core,
        ClinicRequest::SavePayment {
            payment: Payment {
                note: "הערה".to_owned(),
                ..c.payments[0].clone()
            },
        },
    );
    assert!(
        text.contains("אמצעי התשלום") && text.contains("לא מכירה"),
        "{text}"
    );
    let text = refused(
        &mut core,
        ClinicRequest::VoidPayment {
            id: id.clone(),
            reason: "טעות".to_owned(),
        },
    );
    assert!(text.contains("לא מכירה"), "{text}");
    let stored = core.vault_ref().unwrap().clinic_get(&id).unwrap().unwrap();
    assert_eq!(stored.json, json);
    // She picks a value herself: then it is saved.
    let saved = save_payment(
        &mut core,
        Payment {
            method: PayMethod::Transfer,
            ..c.payments[0].clone()
        },
    );
    assert_eq!(saved.method, PayMethod::Transfer);

    // The same for a visit's kind and status, a task's kind, a card's status and a role.
    let json = format!(
        r#"{{"patient_id":"{}","kind":"home_visit","date":"2026-11-03","start":540,"minutes":60,"status":"postponed"}}"#,
        noam.id
    );
    let visit_id = raw_put(&mut core, "visit", &noam, "2026-11-03", &json);
    let shown = calendar(&mut core, "2026-11-03", "2026-11-03")
        .remove(0)
        .visit;
    assert_eq!(shown.id, visit_id);
    assert_eq!(
        (shown.kind, shown.status),
        (VisitKind::Unknown, VisitStatus::Unknown)
    );
    let request = try_save_visit(&mut core, shown.clone());
    let text = refused(&mut core, request);
    assert!(text.contains("סוג הפגישה"), "{text}");
    let request = try_save_visit(
        &mut core,
        Visit {
            kind: VisitKind::Other,
            ..shown
        },
    );
    let text = refused(&mut core, request);
    assert!(text.contains("מצב הפגישה"), "{text}");
    // No screen fell over any of it.
    overview(&mut core);
    income(&mut core, 2026);
    rows(&mut core);
}

/// D-178: one row that cannot be read does not take the clinic with it. It is left out,
/// counted, said on the front page and in the log (a number only), and never touched.
#[test]
fn a_row_that_cannot_be_read_is_left_out_counted_and_reported() {
    const CANARY: &str = "תוכן-בדוי-שלא-נקרא";
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    save_payment(&mut core, a_payment(&noam, "2026-11-02", 10_000));
    assert!(overview(&mut core)
        .alerts
        .iter()
        .all(|a| a.kind != AlertKind::Unreadable));
    assert_eq!(audit_count(&mut core, "clinic_unreadable"), 0);
    // A payment whose amount is not a number, and a visit that is not an object at all.
    let broken = format!(r#"{{"amount":"{CANARY}","note":"{CANARY}"}}"#);
    let bad = raw_put(&mut core, "payment", &noam, "2026-11-03", &broken);
    let first = overview(&mut core);
    let alert = first
        .alerts
        .first()
        .filter(|a| a.kind == AlertKind::Unreadable)
        .unwrap();
    hebrew(&alert.text);
    assert!(
        alert.text.starts_with("רישום אחד בקליניקה לא נקרא"),
        "{}",
        alert.text
    );
    assert!(alert.patient_id.is_none() && alert.patient_name.is_none());
    assert_eq!(first.month_received, 10_000);
    raw_put(&mut core, "visit", &noam, "2026-11-04", "[1,2]");
    let second = overview(&mut core);
    assert!(second.alerts[0]
        .text
        .starts_with("2 רישומים בקליניקה לא נקראו"));
    overview(&mut core);
    // Every screen works, without the rows.
    let c = card(&mut core, &noam.id);
    assert_eq!((c.payments.len(), c.visits.len()), (1, 0));
    assert_eq!(diary(&mut core, "2026-11").received, 10_000);
    assert_eq!(income(&mut core, 2026).received, 10_000);
    assert_eq!(rows(&mut core).len(), 1);
    assert!(calendar(&mut core, "2026-11-01", "2026-11-30").is_empty());
    // The log: one line for each count, a number only.
    let log = core.activity(None).unwrap();
    let said: Vec<&str> = log
        .entries
        .iter()
        .filter(|e| e.event == "clinic_unreadable")
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(
        said,
        [
            "2 רישומים בקליניקה לא נקראו, והם לא מוצגים במסכים",
            "רישום אחד בקליניקה לא נקרא, והוא לא מוצג במסכים",
        ]
    );
    for entry in &log.entries {
        assert!(!entry.text.contains(CANARY) && !entry.text.contains("נועם"));
    }
    assert!(format!("{first:?}{second:?}").find(CANARY).is_none());
    // The row is as it was.
    let stored = core.vault_ref().unwrap().clinic_get(&bad).unwrap().unwrap();
    assert_eq!(stored.json, broken);
    // What is decided from all of a card's records is not decided without one of them.
    assert!(matches!(
        core.clinic(erase(&noam, "נועם")),
        Err(CoreError::Internal(_))
    ));
    assert!(matches!(
        core.clinic(ClinicRequest::Message {
            kind: MessageKind::Payment,
            patient_id: noam.id.clone(),
            visit_id: None,
        }),
        Err(CoreError::Internal(_))
    ));
    assert_eq!(rows(&mut core).len(), 1);
}

#[test]
fn settings_written_before_a_field_was_added_still_load() {
    let (_dir, mut core) = open_clinic();
    // A default with no fee, a template with no text, and a kind of a later version.
    let json = r#"{"defaults":[{"kind":"intake","minutes":45},{"kind":"hologram","minutes":30}],
                   "note_templates":[{"name":"תבנית ישנה"}]}"#;
    core.vault_mut()
        .unwrap()
        .clinic_put("settings", None, None, 0, json, None)
        .unwrap();
    let settings = match ask(&mut core, ClinicRequest::Settings) {
        ClinicReply::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(settings.defaults.len(), 2);
    assert_eq!(
        (
            settings.defaults[0].kind,
            settings.defaults[0].minutes,
            settings.defaults[0].fee
        ),
        (VisitKind::Intake, 45, 0)
    );
    assert_eq!(settings.defaults[1].kind, VisitKind::Unknown);
    assert_eq!(settings.note_templates[0].text, "");
    assert_eq!(settings.report_days, ClinicSettings::default().report_days);
    overview(&mut core);
    // Saved as they are they are refused; nothing is rewritten by itself.
    refused(&mut core, ClinicRequest::SaveSettings { settings });
}

fn identities_of(core: &mut Core, case_id: &str) -> Vec<(Role, String, Vec<String>)> {
    core.case_detail(case_id)
        .unwrap()
        .identities
        .into_iter()
        .map(|i| (i.role, i.value, i.aliases))
        .collect()
}

/// D-177 (the reviewer's fourth finding, the one about privacy): the names of a card are
/// names its case's filter knows, however the card came to the case and whenever a name
/// was added.
#[test]
fn the_names_of_a_card_are_known_to_its_case_whenever_they_are_linked_or_change() {
    let (_dir, mut core, existing) = setup(None);
    let before = identities_of(&mut core, &existing);
    assert!(before.iter().all(|(_, value, _)| value != "יהלי בן חיים"));
    let family = save_patient(
        &mut core,
        Patient {
            name: "יהלי בן חיים".to_owned(),
            contacts: vec![contact(ContactRole::Mother, "סיגל בן חיים")],
            ..child("")
        },
    );
    // Linking a case she made by hand registers the child and the contact.
    link_case(&mut core, &family, Some(&existing));
    let linked = identities_of(&mut core, &existing);
    assert_eq!(linked.len(), before.len() + 1);
    // The case already had its child: the card's name is another way that child is written.
    let child_of_case = linked
        .iter()
        .find(|(role, _, _)| *role == Role::Child)
        .unwrap();
    assert_eq!(child_of_case.1, "אלון");
    assert_eq!(child_of_case.2, ["יהלי בן חיים"]);
    assert!(linked.contains(&(Role::Mother, "סיגל בן חיים".to_owned(), Vec::new())));
    // Everything the case had is as it was.
    for known in &before {
        assert!(linked
            .iter()
            .any(|now| now.0 == known.0 && now.1 == known.1));
    }
    // A contact added later, and a child's name that was corrected.
    let family = save_patient(
        &mut core,
        Patient {
            name: "יהלי בן-חיים".to_owned(),
            contacts: vec![
                contact(ContactRole::Mother, "סיגל בן חיים"),
                contact(ContactRole::Father, "אלעד בן חיים"),
                contact(ContactRole::Guardian, "נחמה וייס"),
            ],
            ..family
        },
    );
    let later = identities_of(&mut core, &existing);
    assert!(later.contains(&(Role::Father, "אלעד בן חיים".to_owned(), Vec::new())));
    assert!(later.contains(&(Role::Other, "נחמה וייס".to_owned(), Vec::new())));
    let child_of_case = later
        .iter()
        .find(|(role, _, _)| *role == Role::Child)
        .unwrap();
    assert_eq!(child_of_case.2, ["יהלי בן חיים", "יהלי בן-חיים"]);
    // A contact taken off the card stays a name the case hides.
    let family = save_patient(
        &mut core,
        Patient {
            contacts: vec![contact(ContactRole::Mother, "סיגל בן חיים")],
            ..family
        },
    );
    let after = identities_of(&mut core, &existing);
    assert_eq!(after, later);
    // Saving again adds nothing twice.
    save_patient(&mut core, family);
    assert_eq!(identities_of(&mut core, &existing), later);
}

#[test]
fn a_record_sent_to_a_hand_made_case_has_a_later_contact_hidden_by_the_filter() {
    let (_dir, mut core, existing) = setup(None);
    let family = save_patient(
        &mut core,
        Patient {
            name: "יהלי בן חיים".to_owned(),
            contacts: vec![contact(ContactRole::Mother, "סיגל בן חיים")],
            ..child("")
        },
    );
    link_case(&mut core, &family, Some(&existing));
    let family = save_patient(
        &mut core,
        Patient {
            contacts: vec![
                contact(ContactRole::Mother, "סיגל בן חיים"),
                contact(ContactRole::Other, "זהבית קורן"),
            ],
            ..family
        },
    );
    // A name the filter would have no other reason to hide: no title, no cue before it.
    let record =
        "בפגישה השתתפה גם זהבית קורן. יהלי בן חיים שיתף פעולה, וסיגל בן חיים הצטרפה בסוף. \
                  לדברי זהבית קורן יש שיפור בהתארגנות.";
    let note = save_note(&mut core, a_note(&family, None, record));
    // Even if the case lost the names in between (she removed them there), the send itself
    // puts them back before the record goes.
    let kept: Vec<IdentityInput> = core
        .case_detail(&existing)
        .unwrap()
        .identities
        .into_iter()
        .filter(|i| i.value != "זהבית קורן")
        .map(|i| IdentityInput {
            id: Some(i.id),
            role: i.role,
            value: i.value,
            aliases: i.aliases,
        })
        .collect();
    core.set_identities(&existing, kept).unwrap();
    assert!(identities_of(&mut core, &existing)
        .iter()
        .all(|(_, value, _)| value != "זהבית קורן"));
    let sent = note_to_case(&mut core, &note);
    assert!(identities_of(&mut core, &existing)
        .iter()
        .any(|(role, value, _)| *role == Role::Other && value == "זהבית קורן"));
    let detail = core.case_detail(&existing).unwrap();
    let material = detail
        .inputs
        .iter()
        .find(|i| Some(&i.id) == sent.case_input_id.as_ref())
        .unwrap();
    assert_eq!(material.content, record);
    let filtered = core.preview_filter(&existing, &material.content).unwrap();
    for name in ["זהבית", "קורן", "יהלי", "סיגל", "בן חיים"] {
        assert!(
            !filtered.tagged.contains(name),
            "{name}: {}",
            filtered.tagged
        );
    }
    assert!(filtered.tagged.contains("שיפור בהתארגנות"));
}

/// D-177 (the fifth finding): the first send writes the material and the note together, and
/// a send asked again finds the note already pointing at its material.
#[test]
fn a_record_is_never_added_to_a_case_twice() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let link = create_case(&mut core, &noam);
    let note = save_note(&mut core, a_note(&noam, None, "שיחת טלפון עם ההורים."));
    let sent = note_to_case(&mut core, &note);
    let input_id = sent.case_input_id.clone().unwrap();
    // The note as kept names the material, and the material is in the case.
    let kept = card(&mut core, &noam.id).notes.remove(0);
    assert_eq!(kept.case_input_id.as_deref(), Some(input_id.as_str()));
    let inputs = core.case_detail(&link.id).unwrap().inputs;
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].id, input_id);
    assert_eq!(actions(&mut core, &note.id), ["created", "changed"]);
    // Asked again, with the note as the window still has it (no material id): replaced.
    for _ in 0..2 {
        let again = note_to_case(&mut core, &note);
        assert_eq!(again.case_input_id.as_deref(), Some(input_id.as_str()));
    }
    assert_eq!(core.case_detail(&link.id).unwrap().inputs.len(), 1);
    assert_eq!(actions(&mut core, &note.id), ["created", "changed"]);
}

/// D-176 (the seventh finding): "this one and every later one" never moves a visit that has
/// a session note, or one that is no longer planned.
#[test]
fn a_series_change_leaves_a_later_visit_with_a_note_where_it_is() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    let series = save_visits(
        &mut core,
        a_visit(&noam, VisitKind::Therapy, "2026-12-01", 540),
        Some(Repeat {
            every_weeks: 1,
            count: 5,
        }),
        Scope::One,
    );
    assert_eq!(series.left_alone, 0);
    let visits: Vec<Visit> = series.visits.into_iter().map(|v| v.visit).collect();
    // The third has a note written ahead; the fourth was cancelled.
    let noted = visits[2].clone();
    save_note(&mut core, a_note(&noam, Some(&noted), "הכנה לפגישה."));
    let cancelled = mark(&mut core, &visits[3], VisitStatus::Cancelled);
    let saved = save_visits(
        &mut core,
        Visit {
            date: "2026-12-02".to_owned(),
            start: 600,
            place: "זום".to_owned(),
            ..visits[0].clone()
        },
        None,
        Scope::Following,
    );
    assert_eq!(saved.left_alone, 2);
    let moved: Vec<&str> = saved.visits.iter().map(|v| v.visit.date.as_str()).collect();
    assert_eq!(moved, ["2026-12-02", "2026-12-09", "2026-12-30"]);
    let all = calendar(&mut core, "2026-12-01", "2026-12-31");
    let now_noted = all.iter().find(|v| v.visit.id == noted.id).unwrap();
    assert_eq!(now_noted.visit, noted);
    assert!(now_noted.has_note);
    let now_cancelled = all.iter().find(|v| v.visit.id == cancelled.id).unwrap();
    assert_eq!(now_cancelled.visit, cancelled);
    assert_eq!(actions(&mut core, &noted.id), ["created"]);
    // One visit alone says nothing was left.
    let alone = save_visits(
        &mut core,
        Visit {
            start: 660,
            ..visits[1].clone()
        },
        None,
        Scope::One,
    );
    assert_eq!(alone.left_alone, 0);
}

fn erase_on(core: &mut Core, patient: &Patient, day: &str) {
    assert_eq!(
        core.clinic_erase_on(&patient.id, &patient.name, day)
            .unwrap(),
        ClinicReply::Done
    );
}

/// A family whose retention period can pass: an adult child, and everything long ago.
fn an_old_family(core: &mut Core, name: &str) -> Patient {
    let patient = save_patient(
        core,
        Patient {
            birth_date: Some("2000-05-05".to_owned()),
            created_on: "2026-01-20".to_owned(),
            ..child(name)
        },
    );
    let priced = |kind, date: &str, start| Visit {
        fee: 40_000,
        ..a_visit(&patient, kind, date, start)
    };
    let intake = save_visit(core, priced(VisitKind::Intake, "2026-02-03", 540));
    mark(core, &intake, VisitStatus::Held);
    for date in ["2026-02-10", "2026-03-03"] {
        let visit = save_visit(core, priced(VisitKind::Assessment, date, 540));
        mark(core, &visit, VisitStatus::Held);
    }
    let missed = save_visit(core, priced(VisitKind::Assessment, "2026-03-10", 540));
    save_visit(
        core,
        Visit {
            status: VisitStatus::NoShow,
            charge_missed: true,
            ..missed
        },
    );
    let cancelled = save_visit(core, priced(VisitKind::Feedback, "2026-03-17", 540));
    mark(core, &cancelled, VisitStatus::Cancelled);
    save_visit(core, priced(VisitKind::Feedback, "2026-03-24", 540));
    save_charge(core, a_charge(&patient, "2026-03-05", 150_000));
    let wrong = save_charge(core, a_charge(&patient, "2026-03-06", 99_000));
    ask(
        core,
        ClinicRequest::VoidCharge {
            id: wrong.id,
            reason: "נרשם פעמיים".to_owned(),
        },
    );
    save_payment(core, a_payment(&patient, "2026-02-03", 40_000));
    save_payment(
        core,
        Payment {
            method: PayMethod::Transfer,
            receipt: String::new(),
            ..a_payment(&patient, "2026-03-05", 150_000)
        },
    );
    let wrong = save_payment(core, a_payment(&patient, "2026-03-06", 77_000));
    ask(
        core,
        ClinicRequest::VoidPayment {
            id: wrong.id,
            reason: "נרשם פעמיים".to_owned(),
        },
    );
    save_note(core, a_note(&patient, Some(&intake), "פגישת היכרות."));
    patient
}

/// D-179: erasing a card takes the names and the records, and the summaries of the years
/// that passed stay exactly as they were.
#[test]
fn the_yearly_summary_and_the_diary_totals_are_the_same_after_an_erase() {
    const DAY: &str = "2100-01-01";
    let (_dir, mut core) = open_clinic();
    let stays = new_patient(&mut core, "תמר");
    let visit = save_visit(
        &mut core,
        Visit {
            fee: 35_000,
            ..a_visit(&stays, VisitKind::Therapy, "2026-03-04", 700)
        },
    );
    mark(&mut core, &visit, VisitStatus::Held);
    save_payment(&mut core, a_payment(&stays, "2026-03-04", 35_000));
    let first = an_old_family(&mut core, "אלמוג דוידי");
    let second = an_old_family(&mut core, "שחר אביטל");

    let year_before = income(&mut core, 2026);
    assert_eq!(year_before.new_patients, 3);
    let months = ["2026-01", "2026-02", "2026-03", "2026-04"];
    let diaries_before: Vec<DiaryMonth> = months.iter().map(|m| diary(&mut core, m)).collect();
    let totals = |d: &DiaryMonth| {
        (
            d.charged,
            d.received,
            d.by_method.clone(),
            d.missing_receipts,
        )
    };
    assert!(diaries_before.iter().all(|d| d.erased.is_none()));

    erase_on(&mut core, &first, DAY);
    assert_eq!(income(&mut core, 2026), year_before);
    erase_on(&mut core, &second, DAY);
    let year_after = income(&mut core, 2026);
    assert_eq!(year_after, year_before);
    assert_eq!(income(&mut core, 2025).new_patients, 0);
    for (month, before) in months.iter().zip(&diaries_before) {
        let after = diary(&mut core, month);
        assert_eq!(totals(&after), totals(before), "{month}");
    }
    // March: the family that stays has its lines; the two that went are one line.
    let march = diary(&mut core, "2026-03");
    let lines: Vec<&str> = march
        .days
        .iter()
        .flat_map(|d| &d.lines)
        .map(|l| l.patient_name.as_str())
        .collect();
    assert_eq!(lines, ["תמר", "תמר"]);
    let erased = march.erased.clone().unwrap();
    assert_eq!(erased.label, "רשומות של כרטיסים שנמחקו");
    hebrew(&erased.label);
    assert_eq!((erased.held, erased.cancelled, erased.no_show), (2, 2, 2));
    assert_eq!(
        (erased.charged, erased.received),
        (2 * (40_000 + 40_000 + 150_000), 2 * 150_000)
    );
    assert_eq!(march.missing_receipts, 2);
    // A month in which the erased cards had nothing has no such line.
    assert!(diary(&mut core, "2026-04").erased.is_none());
    assert!(diary(&mut core, "2026-01").erased.is_none());

    // What is kept: months, counts and sums. There is no field for a name, an id or a text,
    // and the kept JSON holds none of the erased families' words.
    let v = core.vault_ref().unwrap();
    let row = v.clinic_get("tally").unwrap().unwrap();
    assert_eq!(
        (row.kind.as_str(), row.patient_id.clone(), row.day),
        ("tally", None, 0)
    );
    let tally: rules::ErasedTally = serde_json::from_str(&row.json).unwrap();
    assert_eq!(
        tally.months.keys().collect::<Vec<_>>(),
        ["2026-02", "2026-03"]
    );
    let february = &tally.months["2026-02"];
    assert_eq!(
        february.held,
        [
            rules::KindCount {
                kind: VisitKind::Intake,
                count: 2
            },
            rules::KindCount {
                kind: VisitKind::Assessment,
                count: 2
            },
        ]
    );
    assert_eq!((february.cancelled, february.no_show), (0, 0));
    assert_eq!(february.charged, 160_000);
    assert_eq!(
        february.received,
        [rules::ErasedMethod {
            method: PayMethod::Bit,
            amount: 80_000
        }]
    );
    let cards_json = v.clinic_erased_cards().unwrap().unwrap();
    let cards: rules::ErasedCards = serde_json::from_str(&cards_json).unwrap();
    assert_eq!(
        cards.months.into_iter().collect::<Vec<_>>(),
        [("2026-01".to_owned(), 2)]
    );
    for kept in [&row.json, &cards_json] {
        for word in [
            "אלמוג",
            "דוידי",
            "שחר",
            "אביטל",
            "דנה",
            "פגישת היכרות",
            "חבילת אבחון",
            "נרשם פעמיים",
            "101",
            &first.id,
            &second.id,
        ] {
            assert!(!kept.contains(word), "{word} is in {kept}");
        }
    }
    // The records and the cards are gone, and the other family is whole.
    assert!(v.clinic_of_patient(&first.id).unwrap().is_empty());
    assert!(v.clinic_patient(&second.id).unwrap().is_none());
    assert_eq!(rows(&mut core).len(), 1);
    assert_eq!(card(&mut core, &stays.id).visits.len(), 1);
    // The log: that a card went and how many records, nothing of the sums or the months.
    let log = core.activity(None).unwrap();
    let said: Vec<&str> = log
        .entries
        .iter()
        .filter(|e| e.event == "clinic_patient_erased")
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(said, ["כרטיס מטופל נמחק מהקליניקה, ואיתו 12 רישומים"; 2]);
    assert!(log.intact);
    // The front page counts what was received this month with the erased cards' part.
    let march_day = Today {
        date: "2026-03-20".to_owned(),
        minute: 600,
    };
    match ask(&mut core, ClinicRequest::Overview { today: march_day }) {
        ClinicReply::Overview(o) => assert_eq!(o.month_received, 35_000 + 2 * 150_000),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_card_where_nothing_happened_leaves_nothing_behind() {
    let (_dir, mut core) = open_clinic();
    let noam = new_patient(&mut core, "נועם");
    save_visit(
        &mut core,
        a_visit(&noam, VisitKind::Intake, "2026-11-20", 540),
    );
    assert_eq!(income(&mut core, 2026).new_patients, 1);
    ask(&mut core, erase(&noam, "נועם"));
    // A card opened by mistake was never a new patient.
    assert_eq!(income(&mut core, 2026).new_patients, 0);
    let v = core.vault_ref().unwrap();
    assert!(v.clinic_get("tally").unwrap().is_none());
    assert!(v.clinic_erased_cards().unwrap().is_none());
}

#[test]
fn a_birth_date_that_has_not_come_is_refused_when_a_card_is_saved() {
    let (_dir, mut core) = open_clinic();
    let day = |offset: i64| rules::add_days(&dates::iso(dates::today()), offset).unwrap();
    // Her evening can be the core's next day: one day ahead is still "today" somewhere.
    for offset in [-1, 0, 1] {
        let saved = save_patient(
            &mut core,
            Patient {
                birth_date: Some(day(offset)),
                ..child("נועם")
            },
        );
        assert_eq!(saved.birth_date, Some(day(offset)));
    }
    for offset in [2, 400] {
        let text = refused(
            &mut core,
            ClinicRequest::SavePatient {
                patient: Patient {
                    birth_date: Some(day(offset)),
                    ..child("תמר")
                },
            },
        );
        assert!(text.contains("תאריך הלידה עוד לא הגיע"), "{text}");
    }
    assert_eq!(rows(&mut core).len(), 3);
    // An existing card is held to the same rule.
    let noam = rows(&mut core).remove(0).patient;
    refused(
        &mut core,
        ClinicRequest::SavePatient {
            patient: Patient {
                birth_date: Some(day(30)),
                ..noam
            },
        },
    );
}
