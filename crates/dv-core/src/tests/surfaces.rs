//! A made-up name is followed through every place the program writes or shows something
//! (S2): what is sent, the activity log, every error message, every status, `Debug` output,
//! the files in the vault folder and the temp folder. The name may appear only where it must
//! (inside the open vault and in the report she exports); nowhere else, ever.

use super::*;

/// Not a word in any list, so a hit is the child's name and nothing else.
const SECRET_CHILD: &str = "זרובבלית";
const SECRET_PARENT: &str = "פרנצ'סקה";

fn assert_clean(surface: &str, text: &str) {
    for secret in [SECRET_CHILD, SECRET_PARENT] {
        assert!(!text.contains(secret), "{secret} appeared in {surface}");
    }
}

#[test]
fn a_made_up_name_never_reaches_a_log_an_error_a_status_or_a_file() {
    let fake = FakeTransport::default();
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), Some(Box::new(fake.clone())));
    core.create_vault(PASSWORD).unwrap();
    let person = |role, value: &str| IdentityInput {
        id: None,
        role,
        value: value.into(),
        aliases: vec![],
    };
    let case = core
        .create_case(
            CaseMeta {
                code: "TEST-0900".into(),
                age: Some(Age {
                    years: 6,
                    months: 1,
                }),
                child_gender: Some(GrammaticalGender::Female),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![
                person(Role::Child, SECRET_CHILD),
                person(Role::Mother, SECRET_PARENT),
            ],
        )
        .unwrap();
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "שיחה עם הגננת",
        &format!("{SECRET_PARENT} מספרת ש{SECRET_CHILD} נרדמת מאוחר ומתעוררת בלילה."),
    )
    .unwrap();

    let approval = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap()
        .approval_id
        .unwrap();
    core.send_section(&approval).unwrap();

    // 1. What left the machine.
    for body in fake.sent.lock().unwrap().iter() {
        assert_clean("a request to the AI", body);
    }
    // 2. The activity log. Its case label shows the child's name inside the open vault, by
    //    design (the screen lists "which case"); the event texts never carry a name.
    for entry in core.activity(None).unwrap().entries {
        assert_clean("an activity text", &entry.text);
    }
    // 3. Status views and Debug output.
    assert_clean(
        "the app status",
        &serde_json::to_string(&core.status()).unwrap(),
    );
    assert_clean(
        "the security status",
        &serde_json::to_string(&core.security_status()).unwrap(),
    );
    assert_clean("Debug of the core", &format!("{core:?}"));
    // 4. Error messages: the same approval twice, a missing case, a wrong password.
    let errors = [
        core.send_section(&approval).unwrap_err(),
        core.case_detail("no-such-case").unwrap_err(),
        core.prepare_section(&case, "no-such-section", "x")
            .unwrap_err(),
    ];
    for e in &errors {
        assert_clean(
            "an error message",
            &format!("{} {:?}", e.to_ui().message, e),
        );
    }
    // 5. Files: the vault folder (names and bytes) and anything new in the temp folder.
    core.lock();
    for entry in std::fs::read_dir(dir.path()).unwrap().flatten() {
        assert_clean("a file name", &entry.file_name().to_string_lossy());
        let bytes = std::fs::read(entry.path()).unwrap_or_default();
        assert_clean("a file", &String::from_utf8_lossy(&bytes));
    }
    assert_clean(
        "a locked status",
        &serde_json::to_string(&core.status()).unwrap(),
    );
    let err = core.unlock("wrong password").unwrap_err();
    assert_clean("the wrong-password error", &err.to_ui().message);
}
