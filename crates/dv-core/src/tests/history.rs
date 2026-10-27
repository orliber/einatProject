//! "מה יצא מהמחשב" (round 27): what the history shows is what left, byte for byte, with
//! what was hidden in it read back from the case. Fabricated data only.

use super::*;

/// The first page of the history.
fn page(core: &mut Core, case: Option<&str>, place: Option<&str>, query: &str) -> Vec<SentItem> {
    core.sent_history(case, place, query, None).unwrap().items
}

fn sent_section(core: &mut Core, case: &str) {
    let id = core
        .prepare_section(case, "kindergarten", "תנסח פסקה")
        .unwrap()
        .approval_id
        .expect("a cleared request has an approval");
    core.send_section(&id).unwrap();
}

#[test]
fn the_history_shows_what_left_and_what_was_hidden_in_it() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    sent_section(&mut core, &case);
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);

    let all = page(&mut core, None, None, "");
    assert_eq!(all.len(), 1);
    let item = &all[0];
    assert_eq!(item.case_code, "TEST-0002");
    assert_eq!(item.place, "kindergarten");
    assert_eq!(item.ai, "Claude");
    assert!(!item.demo);
    // The child and the kindergarten teacher were in the material: both went out as tags.
    let words: Vec<&str> = item.hidden.iter().map(|h| h.word.as_str()).collect();
    assert!(words.contains(&"אלון"), "{:?}", item.hidden);
    assert!(words.contains(&"שירה"), "{:?}", item.hidden);

    let full = core.sent_text(&case, &item.id).unwrap();
    assert_eq!(full.exact, sent[0], "the exact bytes that left");
    assert!(full.hash_ok);
    assert_eq!(full.item, *item);
    assert_eq!(full.parts[0].label, "הוראות קבועות");
    let asked: String = full.parts[1..].iter().map(|p| p.text.as_str()).collect();
    assert!(asked.contains("מתקשה במעברים"));
    for h in &item.hidden {
        assert!(asked.contains(&h.tag), "{} is in the request", h.tag);
        assert!(!full.exact.contains(&h.word), "{} never left", h.word);
    }
}

#[test]
fn a_search_finds_the_text_that_left_and_says_when_the_word_was_hidden() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake));
    sent_section(&mut core, &case);

    let in_text = page(&mut core, Some(&case), None, "מעברים");
    assert_eq!(in_text.len(), 1);
    assert!(!in_text[0].hidden_match);

    // The child's name did not leave: found, and marked as hidden there.
    let hidden = page(&mut core, Some(&case), None, "אלון");
    assert_eq!(hidden.len(), 1);
    assert!(hidden[0].hidden_match);

    assert!(page(&mut core, Some(&case), None, "מילה שלא הייתה").is_empty());
    assert!(page(&mut core, Some(&case), Some("intake"), "").is_empty());
    assert_eq!(
        page(&mut core, Some(&case), Some("kindergarten"), "").len(),
        1
    );
}

#[test]
fn a_question_from_inside_a_case_is_kept_with_it_and_one_without_a_case_is_not() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let ask = core
        .prepare_consult(Some(&case), None, "איך מנסחים המלצה על לוח מעברים?")
        .unwrap()
        .approval_id
        .unwrap();
    core.send_consult(&ask).unwrap();
    let general = core
        .prepare_consult(None, None, "איך מנסחים המלצה כללית?")
        .unwrap()
        .approval_id
        .unwrap();
    core.send_consult(&general).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 2);

    let kept = page(&mut core, None, None, "");
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].place, "consult");
    assert_eq!(kept[0].case_id, case);
}

#[test]
fn newest_first_across_cases_each_with_its_own_names() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake));
    sent_section(&mut core, &case);
    let other = core
        .create_case(
            CaseMeta {
                code: "TEST-0003".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "יובל".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.add_input(
        &other,
        InputKind::Kindergarten,
        "שיחה עם הגננת",
        "יובל משחק בחצר ומשתף פעולה עם חברים.",
    )
    .unwrap();
    sent_section(&mut core, &other);

    let all = page(&mut core, None, None, "");
    assert_eq!(all.len(), 2);
    assert!(all[0].at >= all[1].at);
    let codes: HashSet<&str> = all.iter().map(|i| i.case_code.as_str()).collect();
    assert_eq!(codes, HashSet::from(["TEST-0002", "TEST-0003"]));
    // Each case's names are its own: the other child's name is not listed under this case.
    let mine = page(&mut core, Some(&case), None, "");
    assert!(mine[0].hidden.iter().all(|h| h.word != "יובל"));

    // A wrong id is not found, also through another case.
    assert!(matches!(
        core.sent_text(&case, &all.iter().find(|i| i.case_id == other).unwrap().id),
        Err(CoreError::NotFound(_))
    ));
}

/// The review of stage 2: a send whose answer failed after it reached the service (busy, a
/// connection lost on the way) is in the history too; a refusal on this computer is not.
#[test]
fn a_send_without_an_answer_is_in_the_history_when_it_may_have_left() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake));
    let id = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap()
        .approval_id
        .unwrap();
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    assert!(core
        .finish_section(out, Err(CoreError::Egress(EgressError::Unavailable(503))))
        .is_err());
    assert_eq!(page(&mut core, Some(&case), None, "").len(), 1);
    // Sending is switched off here: nothing left, nothing recorded.
    let out = core.begin_send(&id, SendKind::Section).unwrap();
    assert!(core
        .finish_section(out, Err(CoreError::Egress(EgressError::Stopped)))
        .is_err());
    assert_eq!(page(&mut core, Some(&case), None, "").len(), 1);
    // The retry goes, and is recorded again: the same bytes left twice.
    core.send_section(&id).unwrap();
    assert_eq!(page(&mut core, Some(&case), None, "").len(), 2);
}

/// Every page from the first to the last, and how many reads it took.
fn all_pages(
    core: &mut Core,
    case: Option<&str>,
    place: Option<&str>,
    query: &str,
) -> (Vec<SentItem>, usize) {
    let mut out = Vec::new();
    let mut before = None;
    let mut calls = 0;
    loop {
        let page = core.sent_history(case, place, query, before).unwrap();
        calls += 1;
        assert!(page.items.len() <= SENT_PAGE);
        out.extend(page.items);
        match page.next {
            Some(next) => before = Some(next),
            None => return (out, calls),
        }
    }
}

/// D-123: a year of work (2,000 fabricated requests over two cases) is read a page at a time,
/// newest first, every request exactly once; the case and place narrow in the vault; a search
/// reads a bounded number of requests per page. The time budgets are loose (a debug build on a
/// slow CI machine): they catch a return to reading everything, not a few milliseconds.
#[test]
fn a_year_of_requests_is_read_a_page_at_a_time() {
    const SENT: usize = 2_000;
    let (_dir, mut core, case) = setup(None);
    let other = core
        .create_case(
            CaseMeta {
                code: "TEST-0003".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "יובל".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    let child_tag = |core: &mut Core, c: &str| {
        core.vault_ref()
            .unwrap()
            .identities(c)
            .unwrap()
            .into_iter()
            .find(|i| i.role == Role::Child)
            .unwrap()
            .tag
    };
    let tags = [child_tag(&mut core, &case), child_tag(&mut core, &other)];
    let places = ["kindergarten", "intake", "consult", "sorting"];
    let v = core.vault_mut().unwrap();
    for i in 0..SENT {
        let (c, tag) = if i % 2 == 0 {
            (case.as_str(), &tags[0])
        } else {
            (other.as_str(), &tags[1])
        };
        // A word only three requests hold, to search for.
        let rare = if i % 700 == 3 {
            " ציור-נדיר"
        } else {
            ""
        };
        let body = json!({
            "system": "כללים קבועים",
            "messages": [{"role": "user", "content": format!("בקשה {i}: {tag} מתקשה במעברים.{rare}")}],
        })
        .to_string();
        let sha = dv_vault::crypto::sha256_hex(body.as_bytes());
        v.add_transmission(
            c,
            places[i % places.len()],
            &sha,
            "claude-sonnet-4-5",
            &body,
        )
        .unwrap();
    }

    // The first page: a page's worth, not the year.
    let t = Instant::now();
    let first = core.sent_history(None, None, "", None).unwrap();
    let took = t.elapsed();
    assert_eq!(first.items.len(), SENT_PAGE);
    assert!(first.next.is_some());
    assert!(
        took < Duration::from_secs(2),
        "the first page took {took:?}"
    );
    assert!(first.items.iter().any(|i| i.case_code == "TEST-0003"));
    assert!(first.items.iter().any(|i| i.case_code == "TEST-0002"));

    // Every page: each request once, newest first (time, then id), each with its own case's
    // names.
    let t = Instant::now();
    let (all, calls) = all_pages(&mut core, None, None, "");
    let took = t.elapsed();
    assert_eq!(all.len(), SENT);
    assert_eq!(calls, SENT / SENT_PAGE);
    assert!(took < Duration::from_secs(30), "every page took {took:?}");
    let ids: HashSet<&str> = all.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids.len(), SENT, "no request twice");
    assert!(all
        .windows(2)
        .all(|w| (w[0].at, &w[0].id) > (w[1].at, &w[1].id)));
    for item in &all {
        let (code, child) = if item.case_id == case {
            ("TEST-0002", "אלון")
        } else {
            ("TEST-0003", "יובל")
        };
        assert_eq!(item.case_code, code);
        assert_eq!(item.hidden.len(), 1, "{:?}", item.hidden);
        assert_eq!(item.hidden[0].word, child);
    }

    // The privacy page's count: counted, not read.
    let t = Instant::now();
    assert_eq!(core.sent_count(0).unwrap(), SENT as u32);
    assert!(t.elapsed() < Duration::from_secs(1));
    assert_eq!(core.sent_count(all[0].at + 1).unwrap(), 0);

    // One case, one place: narrowed in the vault, still paged.
    let (mine, _) = all_pages(&mut core, Some(&other), None, "");
    assert_eq!(mine.len(), SENT / 2);
    assert!(mine.iter().all(|i| i.case_id == other));
    let (one_place, _) = all_pages(&mut core, Some(&case), Some("intake"), "");
    assert!(
        one_place.is_empty(),
        "the even requests are never \"intake\""
    );
    let (one_place, _) = all_pages(&mut core, None, Some("intake"), "");
    assert_eq!(one_place.len(), SENT / 4);
    assert!(one_place.iter().all(|i| i.place == "intake"));

    // A search: each page reads at most SENT_SCAN requests, so a rare word takes several
    // short reads instead of one long one, and still finds every request that holds it.
    let t = Instant::now();
    let page = core.sent_history(None, None, "ציור-נדיר", None).unwrap();
    let took = t.elapsed();
    assert!(
        took < Duration::from_secs(5),
        "a searched page took {took:?}"
    );
    assert!(page.next.is_some(), "stopped after {SENT_SCAN} requests");
    let (found, calls) = all_pages(&mut core, None, None, "ציור-נדיר");
    assert_eq!(found.len(), 3);
    assert!(found.iter().all(|i| !i.hidden_match));
    assert_eq!(calls, SENT.div_ceil(SENT_SCAN));
    // A name that did not leave is found as hidden, in its own case only.
    let (hidden, _) = all_pages(&mut core, None, None, "יובל");
    assert_eq!(hidden.len(), SENT / 2);
    assert!(hidden.iter().all(|i| i.hidden_match && i.case_id == other));

    // A case in the recycle bin leaves the history of every case, and stays in its own.
    core.delete_case(&other).unwrap();
    let (left, _) = all_pages(&mut core, None, None, "");
    assert_eq!(left.len(), SENT / 2);
    assert_eq!(core.sent_count(0).unwrap(), (SENT / 2) as u32);
    assert!(left.iter().all(|i| i.case_id == case));
    let (own, _) = all_pages(&mut core, Some(&other), None, "");
    assert_eq!(own.len(), SENT / 2);
}

/// A page that ends exactly at the last request says there is nothing older.
#[test]
fn the_last_full_page_has_no_next() {
    let (_dir, mut core, case) = setup(None);
    let v = core.vault_mut().unwrap();
    for i in 0..SENT_PAGE {
        let body =
            json!({"messages": [{"role": "user", "content": format!("בקשה {i}")}]}).to_string();
        let sha = dv_vault::crypto::sha256_hex(body.as_bytes());
        v.add_transmission(&case, "intake", &sha, "demo", &body)
            .unwrap();
    }
    let page = core.sent_history(None, None, "", None).unwrap();
    assert_eq!(page.items.len(), SENT_PAGE);
    assert_eq!(page.next, None);
    assert!(page.items.iter().all(|i| i.demo && i.ai.is_empty()));
    // A demo answer never left: not counted as a request that did.
    assert_eq!(core.sent_count(0).unwrap(), 0);
}
