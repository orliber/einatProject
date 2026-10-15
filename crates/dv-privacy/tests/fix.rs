//! The automatic fix loop's step (round 25): for every code a fix answers, the refused words
//! are not in the fixed request, the fixed request passes the unchanged gate, and the words
//! come back from the case's book. What a fix cannot answer stays refused.

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
use dv_privacy::{clear, fix_refused, GateRequest, MaskBook};
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
        // The words come back from the book (a foreign tag comes back as its word).
        if code != "foreign_tag" {
            let back = restore(&out, &ids, None, &book.entries());
            assert!(contains_norm(&back, &secret), "{text}: {back}");
        }
    }
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
    let (next, _) = fix_refused(&req(&body), &book).unwrap();
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
        }
    }
}
