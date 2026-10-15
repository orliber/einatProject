//! The automatic fix loop's step (round 25): for every code a fix answers, the refused words
//! are not in the fixed request, and the fixed request passes the unchanged gate. A pattern
//! comes back from the case's book; a line with a name or a foreign tag is left out whole,
//! so nothing next to the name goes out and nothing of it is kept. What a fix cannot answer
//! stays refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

mod common;

use common::*;
use dv_privacy::restore::restore;
use dv_privacy::{clear, fix_refused, GateRequest, MaskBook, LEFT_OUT};
use serde_json::{json, Value};

/// Text put in the request as is, the way a template, a key or a bug could: the filter never
/// saw it.
fn raw(text: &str) -> Value {
    json!({
        "model": "claude-test",
        "system": "עוזרת לכתיבת דוח.",
        "messages": [{"role": "user", "content": text}],
    })
}

/// Fix up to three times, as the core does; the final body and the book.
fn fixed(body: &Value) -> (Value, MaskBook, Vec<String>) {
    let ids = identities();
    let c = ctx(&ids, CASE);
    let tags = case_tags(&ids, CASE);
    let book = MaskBook::new(Vec::new());
    let mut body = body.clone();
    let mut codes = Vec::new();
    for _ in 0..3 {
        let req = GateRequest {
            body: &body,
            ctx: &c,
            case_tags: &tags,
            unresolved_suspects: 0,
            canaries: &[],
            max_bytes: 1_000_000,
        };
        let Err(blocked) = clear(&req) else { break };
        codes.extend(blocked.reasons.iter().map(|r| r.code.clone()));
        let Some((next, _)) = fix_refused(&req, &book) else {
            break;
        };
        body = next;
    }
    (body, book, codes)
}

#[test]
fn every_fixable_refusal_is_hidden_and_the_fixed_request_passes_the_gate() {
    let ids = identities();
    let phone = mobile(2, "4567890");
    let id_no = valid_id("23456789");
    for (text, code, secret) in [
        (
            "נועם מתקשה במעברים.".to_owned(),
            "identity",
            "נועם".to_owned(),
        ),
        (
            "רותם ועידו סיפרו על הבוקר.".to_owned(),
            "identity",
            "עידו".to_owned(),
        ),
        (
            "ינאי מתיק אחר הגיע.".to_owned(),
            "identity",
            "ינאי".to_owned(),
        ),
        (
            "שיחקה עם אביגיל בחצר.".to_owned(),
            "unknown_name",
            "אביגיל".to_owned(),
        ),
        (format!("טלפון {phone}"), "pattern", phone.clone()),
        (format!("ת.ז. {id_no}"), "pattern", id_no.clone()),
        (
            "נבדק ב-14/9/2025.".to_owned(),
            "pattern",
            "14/9/2025".to_owned(),
        ),
        (
            "פרטים\nכתובת: מסדה 33, חיפה\n".to_owned(),
            "pattern",
            "מסדה 33".to_owned(),
        ),
        (
            "למד ב[מוסד_7] עם [ילד].".to_owned(),
            "foreign_tag",
            "[מוסד_7]".to_owned(),
        ),
    ] {
        let (body, book, codes) = fixed(&raw(&text));
        assert!(
            codes.iter().any(|c| c == code),
            "{text}: refused with {codes:?}"
        );
        let out = body["messages"][0]["content"].as_str().unwrap().to_owned();
        assert!(
            !contains_norm(&out, &secret),
            "{text}: {secret} still in {out}"
        );
        gate_body(&body, &ids, CASE).unwrap_or_else(|b| panic!("{text} -> {out}: {b:?}"));
        if code == "pattern" {
            // A pattern comes back from the book.
            let back = restore(&out, &ids, None, &book.entries());
            assert!(contains_norm(&back, &secret), "{text}: {back}");
        } else {
            // A name or a foreign tag: its line is left out, and the book keeps none of it.
            assert!(out.contains(LEFT_OUT), "{text}: {out}");
            assert!(
                book.entries()
                    .iter()
                    .all(|e| !contains_norm(&e.original, &secret)),
                "{text}"
            );
        }
    }
}

/// The review of stage 2: a name hidden alone let the surname next to it out. Each of these
/// was refused before the fix loop, and must not go out in part now.
#[test]
fn a_surname_next_to_a_refused_name_never_goes_out() {
    let ids = identities();
    for (text, kept) in [
        ("שיחקה עם אביגיל שטרנברג בחצר.", "שטרנברג"),
        ("נועם ואחיו גלעדי הגיעו.", "גלעדי"),
        ("[דנה_שטרנברג] הגיעה.", "שטרנברג"),
        (
            "בבוקר הגיע.\nשיחקה עם אביגיל שטרנברג.\nבסוף נרגעה.",
            "שטרנברג",
        ),
    ] {
        let (body, book, _) = fixed(&raw(text));
        let out = body["messages"][0]["content"].as_str().unwrap().to_owned();
        assert!(!out.contains(kept), "{text} -> {out}");
        assert!(out.contains(LEFT_OUT), "{text} -> {out}");
        assert!(book.entries().is_empty(), "{text}: {:?}", book.entries());
        gate_body(&body, &ids, CASE).unwrap_or_else(|b| panic!("{text} -> {out}: {b:?}"));
    }
    // The other lines go as they were.
    let (body, _, _) = fixed(&raw("בבוקר הגיע.\nשיחקה עם אביגיל שטרנברג.\nבסוף נרגעה."));
    assert_eq!(
        body["messages"][0]["content"],
        format!("בבוקר הגיע.\n{LEFT_OUT}\nבסוף נרגעה.")
    );
}

#[test]
fn what_a_fix_cannot_answer_stays_refused() {
    let ids = identities();
    // A name in an object key: keys are never rewritten.
    let key = json!({"model": "claude-test", "messages": [{"role": "user", "content": "שלום"}], "נועם": 1});
    let (body, _, codes) = fixed(&key);
    assert!(codes.iter().any(|c| c == "identity"));
    assert!(gate_body(&body, &ids, CASE).is_err());
    // A canary is a test trap: nothing to fix, refused whatever else is fixed.
    let c = ctx(&ids, CASE);
    let tags = case_tags(&ids, CASE);
    let canaries = vec!["CANARY-7f3a".to_owned()];
    let body = raw("נועם CANARY-7f3a");
    let book = MaskBook::new(Vec::new());
    let req = |body| GateRequest {
        body,
        ctx: &c,
        case_tags: &tags,
        unresolved_suspects: 0,
        canaries: &canaries,
        max_bytes: 1_000_000,
    };
    // The line with the trap is never left out, so the name stays too and nothing is fixed.
    let next = fix_refused(&req(&body), &book).map_or(body.clone(), |(n, _)| n);
    let still = clear(&req(&next)).unwrap_err();
    assert!(still.reasons.iter().any(|r| r.code == "canary"));
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(200))]
    /// Whatever raw text: after at most three fixes, either the gate passes and no declared
    /// name of any case is in what goes out, or it is still refused. A fix never lets a word
    /// through that the gate refused.
    #[test]
    fn a_fix_never_lets_out_what_the_gate_refused(
        words in proptest::collection::vec(proptest::sample::select(vec![
            "נועם", "ונועם", "לרותם", "עידו", "אביטל", "ינאי", "אסנת", "אביגיל", "מתקשה",
            "בגן", "[מוסד_3]", "[ילד]", "14/9/2025", "רחוב", "הדקלים", "12", "שלום", "כתובת:",
            "\n", "ת.ז.", "000000018", "ושוב", "[טלפון_2]", "[שם_1]",
            // Surnames and second names next to a name (the review of stage 2).
            "שטרנברג", "גלעדי", "ואחיו", "[דנה_שטרנברג]", "בן-דוד",
        ]), 1..14)
    ) {
        let text = words.join(" ");
        let ids = identities();
        let (body, _, _) = fixed(&raw(&text));
        if let Ok(p) = gate_body(&body, &ids, CASE) {
            let sent = String::from_utf8_lossy(p.body()).into_owned();
            for name in ["נועם", "רותם", "עידו", "אביטל", "ינאי", "אסנת", "אביגיל", "000000018"] {
                proptest::prop_assert!(!contains_norm(&sent, name), "{} in {}", name, sent);
            }
            // A surname on a line with a name the gate refuses (or a bracket it does not
            // know) never goes out: the whole line is left out, not only the name.
            let refused = ["נועם", "רותם", "עידו", "אביטל", "ינאי", "אסנת", "אביגיל", "[דנה", "[מוסד"];
            for surname in ["שטרנברג", "גלעדי", "בן-דוד"] {
                let near = text.lines().any(|l| l.contains(surname) && refused.iter().any(|n| l.contains(n)));
                let alone = text.lines().any(|l| l.contains(surname) && !refused.iter().any(|n| l.contains(n)));
                proptest::prop_assert!(
                    !(near && !alone && sent.contains(surname)),
                    "{} next to a name in {}", surname, sent
                );
            }
        }
    }
}
