//! The gate is fail-closed (layer 7): every doubt blocks, no `ClearedPayload` is ever made
//! from a request that still holds an identifier, wherever in the JSON it sits, and what the
//! gate hands back (the payload, its hash, the audit codes) leaks nothing on its own.
//! All data is invented; identifiers are generated at run time (see `common`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::indexing_slicing
)]

mod common;

use std::collections::HashSet;

use common::*;
use dv_privacy::text::normalize;
use dv_privacy::{clear, filter, GateRequest, PrivacyContext, GENERIC_TAGS};
use proptest::prelude::*;

/// Every code the gate may write to the audit log. The log keeps only codes, so none of them
/// may carry text of the request.
const CODES: &[&str] = &[
    "unresolved_suspects",
    "serialize",
    "too_large",
    "identity",
    "unknown_name",
    "past_report_name",
    "pattern",
    "internal",
    "foreign_tag",
    "canary",
];

fn request<'a>(
    body: &'a serde_json::Value,
    ctx: &'a PrivacyContext<'a>,
    tags: &'a HashSet<String>,
) -> GateRequest<'a> {
    GateRequest {
        body,
        ctx,
        case_tags: tags,
        unresolved_suspects: 0,
        canaries: &[],
        max_bytes: 1_000_000,
    }
}

const CLEAN: &str = "[ילד] מתקשה במעברים בין פעילויות, ו[אם] תיארה שינה רצופה.";

#[test]
fn clean_tagged_text_is_cleared_and_the_payload_is_exactly_the_serialized_body() {
    let ids = identities();
    let body = serde_json::json!({
        "system": "עוזרת לכתיבת דוח.",
        "messages": [{"role": "user", "content": CLEAN}],
    });
    let payload = gate_body(&body, &ids, CASE).expect("clean text clears");
    assert_eq!(
        payload.body(),
        serde_json::to_vec(&body).unwrap().as_slice()
    );
    assert_eq!(payload.case_id(), CASE);
    // The approval is bound to a SHA-256 of exactly these bytes: 64 lowercase hex digits,
    // the same for the same body and different for a different one.
    let sha = payload.sha256();
    assert_eq!(sha.len(), 64);
    assert!(sha
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_eq!(gate_body(&body, &ids, CASE).unwrap().sha256(), sha);
    let other = serde_json::json!({"messages": [{"role": "user", "content": "[ילד] שיחק."}]});
    assert_ne!(gate_body(&other, &ids, CASE).unwrap().sha256(), sha);
    // The empty SHA-256 is a fixed, known value: the hash really covers the bytes.
    let empty = serde_json::json!("");
    assert_eq!(
        gate_body(&empty, &ids, CASE).unwrap().sha256(),
        "12ae32cb1ec02d01eda3581b127c1fee3b0dc53572ed6baf239721a03d82e126" // sha256("\"\"")
    );
}

#[test]
fn the_payloads_debug_output_never_shows_the_body() {
    let ids = identities();
    let marker = "משפט_ייחודי_לבדיקה";
    let payload = gate_text(&format!("[ילד] {marker}"), &ids, CASE).unwrap();
    let dbg = format!("{payload:?}");
    assert!(!dbg.contains(marker), "{dbg}");
    assert!(dbg.contains(payload.sha256()));
}

#[test]
fn a_declared_name_blocks_wherever_it_sits_in_the_request() {
    let ids = identities();
    let name = "אביטל";
    let bodies = [
        serde_json::json!({"system": format!("הגננת {name} מדווחת."), "messages": []}),
        serde_json::json!({"messages": [{"role": "user", "content": format!("שלום {name}")}]}),
        serde_json::json!({"messages": [{"role": "user", "content": [{"type": "text", "text": format!("ו{name} אמרה")}]}]}),
        serde_json::json!({"messages": [{"role": "assistant", "content": format!("{name}.")}]}),
        // Object keys are text that leaves too (a schema, a tool input).
        serde_json::json!({"metadata": {name: "x"}}),
        serde_json::json!({"deep": [[[{"a": [format!("({name})")]}]]]}),
    ];
    for body in bodies {
        let blocked = gate_body(&body, &ids, CASE).expect_err(&body.to_string());
        assert!(
            blocked.reasons.iter().any(|r| r.code == "identity"),
            "{body}: {:?}",
            blocked.reasons
        );
    }
}

#[test]
fn every_name_of_every_case_blocks_raw_with_any_prefix() {
    // The gate checks the identities of *all* cases, not only the one being worked on.
    let ids = identities();
    let names = [
        "נועם",
        "רותם",
        "עידו",
        "אביטל",
        "אבנר",
        "שטרן",
        "ינאי",
        "אסנת",
        "Noam",
    ];
    for name in names {
        for prefix in [
            "", "ו", "ה", "ב", "ל", "מ", "ש", "כ", "וכש", "ולכש", "שב", "ו-",
        ] {
            if name.is_ascii() && !prefix.is_empty() {
                continue;
            }
            let text = format!("כך אמר {prefix}{name} היום.");
            assert!(gate_text(&text, &ids, CASE).is_err(), "cleared «{text}»");
        }
    }
}

#[test]
fn unresolved_suspects_block_even_a_clean_request() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    let tags = case_tags(&ids, CASE);
    let body = serde_json::json!({"messages": [{"role": "user", "content": CLEAN}]});
    for n in [1, 2, usize::MAX] {
        let req = GateRequest {
            unresolved_suspects: n,
            ..request(&body, &c, &tags)
        };
        let blocked = clear(&req).unwrap_err();
        assert_eq!(blocked.reasons[0].code, "unresolved_suspects");
        assert_eq!(
            blocked.reasons[0].detail.as_deref(),
            Some(n.to_string().as_str())
        );
    }
}

#[test]
fn the_size_limit_is_exact_on_the_serialized_bytes() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    let tags = case_tags(&ids, CASE);
    let body = serde_json::json!({"messages": [{"role": "user", "content": CLEAN}]});
    let len = serde_json::to_vec(&body).unwrap().len();
    let at = GateRequest {
        max_bytes: len,
        ..request(&body, &c, &tags)
    };
    assert!(clear(&at).is_ok(), "exactly at the limit passes");
    let below = GateRequest {
        max_bytes: len - 1,
        ..request(&body, &c, &tags)
    };
    let blocked = clear(&below).unwrap_err();
    assert_eq!(blocked.reasons.len(), 1);
    assert_eq!(blocked.reasons[0].code, "too_large");
    let zero = GateRequest {
        max_bytes: 0,
        ..request(&body, &c, &tags)
    };
    assert!(clear(&zero).is_err());
}

#[test]
fn canaries_block_in_any_spelling_and_any_place() {
    let ids = identities();
    let c = ctx(&ids, CASE);
    let tags = case_tags(&ids, CASE);
    let canaries = vec!["זרובבלית".to_owned(), "Qx-Trap".to_owned()];
    for text in [
        "[ילד] פגש את זרובבלית.",
        "[ילד] פגש את ולזרובבלית.",
        "[ילד] פגש את זְרוּבָּבֶלִית.",
        "[ילד] פגש את זרובבלית\u{200F}.",
        "[ילד] and qx-trap",
        "[ילד] and QX TRAP", // the hyphen normalizes to a space
    ] {
        let body =
            serde_json::json!({"system": "x", "messages": [{"role": "user", "content": text}]});
        let req = GateRequest {
            canaries: &canaries,
            ..request(&body, &c, &tags)
        };
        let blocked = clear(&req).expect_err(text);
        assert!(
            blocked
                .reasons
                .iter()
                .any(|r| r.code == "canary" && r.detail.is_none()),
            "{text}: {:?}",
            blocked.reasons
        );
    }
    // A canary in a key blocks too.
    let body = serde_json::json!({"זרובבלית": 1});
    let req = GateRequest {
        canaries: &canaries,
        ..request(&body, &c, &tags)
    };
    assert!(clear(&req).is_err());
}

#[test]
fn only_this_cases_tags_generic_tags_and_missing_notes_pass() {
    let ids = identities();
    // Generic tags and "[חסר: …]" notes always pass.
    let mut generic = GENERIC_TAGS.join(" ");
    generic.push_str(" [חסר: תאריך האבחון]");
    assert!(gate_text(&generic, &ids, CASE).is_ok(), "{generic}");
    // A tag of this case passes; the same text under another case passes only if it has it.
    assert!(gate_text("[גננת] סיפרה.", &ids, CASE).is_ok());
    let blocked = gate_text("[גננת] סיפרה.", &ids, OTHER).unwrap_err();
    assert_eq!(blocked.reasons[0].code, "foreign_tag");
    assert_eq!(blocked.reasons[0].detail.as_deref(), Some("[גננת]"));
    // Unknown, numbered or made-up tags block.
    for tag in [
        "[גננת_2]",
        "[ילד_גן_1]",
        "[סוד]",
        "[ת.ז]",
        "[ ילד ]",
        "[ילד ]",
    ] {
        let text = format!("{tag} סיפרה.");
        let blocked = gate_text(&text, &ids, CASE).expect_err(&text);
        assert!(
            blocked.reasons.iter().any(|r| r.code == "foreign_tag"),
            "{text}: {:?}",
            blocked.reasons
        );
    }
}

#[test]
fn unknown_lexicon_names_and_past_report_names_block_raw() {
    let ids = identities();
    // A lexicon first name that no case declared.
    let blocked = gate_text("[ילד] שיחק עם יובל בחצר.", &ids, CASE).unwrap_err();
    assert!(blocked.reasons.iter().any(|r| r.code == "unknown_name"));
    // Allowlisted by the psychologist ("not a name"), it passes.
    let allow = |t: &str| t == normalize("יובל");
    let c = PrivacyContext {
        allowlisted: &allow,
        ..ctx(&ids, CASE)
    };
    let tags = case_tags(&ids, CASE);
    let body =
        serde_json::json!({"messages": [{"role": "user", "content": "[ילד] שיחק עם יובל."}]});
    assert!(clear(&request(&body, &c, &tags)).is_ok());
    // A name known only from a past report (by its normalized form), with a prefix.
    let past = |t: &str| t == normalize("זלמנוביץ");
    let c = PrivacyContext {
        past_names: &past,
        ..ctx(&ids, CASE)
    };
    let body = serde_json::json!({"messages": [{"role": "user", "content": "[ילד] ביקר אצל ולזלמנוביץ."}]});
    let blocked = clear(&request(&body, &c, &tags)).unwrap_err();
    assert!(blocked.reasons.iter().any(|r| r.code == "past_report_name"));
}

#[test]
fn the_audit_codes_and_messages_never_carry_request_text() {
    let ids = identities();
    let phone = mobile(4, "7654321");
    let mail = email("tester.x", "example.org");
    let idn = valid_id("31415926");
    let text =
        format!("אביטל ויובל, ת.ז. {idn}, טלפון {phone}, {mail}, ב-14.9.2025, ברחוב הרצל 12 [סוד]");
    let blocked = gate_text(&text, &ids, CASE).unwrap_err();
    let codes: HashSet<&str> = blocked.reasons.iter().map(|r| r.code.as_str()).collect();
    for want in ["identity", "unknown_name", "pattern", "foreign_tag"] {
        assert!(codes.contains(want), "{want} missing in {codes:?}");
    }
    for r in &blocked.reasons {
        assert!(
            CODES.contains(&r.code.as_str()),
            "unexpected code {}",
            r.code
        );
        for secret in ["אביטל", "יובל", &idn, &phone, &mail, "הרצל", "[סוד]"] {
            assert!(
                !r.code.contains(secret) && !r.message.contains(secret),
                "{r:?}"
            );
        }
    }
    // The same reason is listed once.
    let mut seen = HashSet::new();
    let doubled = gate_text("אביטל אביטל", &ids, CASE).unwrap_err();
    for r in &doubled.reasons {
        assert!(seen.insert((&r.code, &r.detail)), "{:?}", doubled.reasons);
    }
}

#[test]
fn numbers_with_no_identifier_shape_pass_the_gate() {
    // The gate is strict, not blind: scores, ages and short counts are clinical content.
    let ids = identities();
    for text in [
        "[ילד] בגיל 5:4, מנת משכל 104, מטריצות 13 12 9.",
        "ציון T של 62 ואחוזון 85.",
        "שלוש פגישות של 45 דקות.",
    ] {
        assert!(gate_text(text, &ids, CASE).is_ok(), "blocked «{text}»");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Any declared name of any case, with any prefix, anywhere among clean words, in any
    /// message of the request, never yields a payload.
    #[test]
    fn a_raw_declared_name_never_clears(
        name in prop::sample::select(vec!["נועם", "רותם", "עידו", "אביטל", "אבנר", "שטרן", "ינאי", "אסנת"]),
        prefix in prop::sample::select(vec!["", "ו", "ה", "ב", "ל", "מ", "ש", "כ", "וב", "של", "כש", "וכש"]),
        before in prop::collection::vec(prop::sample::select(vec!["[ילד]", "מתקשה", "בגן", "היום", "עם", ",", "(", "\"", "—"]), 0..6),
        after in prop::collection::vec(prop::sample::select(vec!["סיפרה", "בחצר", "[אם]", ".", ")", "!", "\n"]), 0..6),
        slot in 0usize..3,
    ) {
        let ids = identities();
        let text = format!("{} {prefix}{name} {}", before.join(" "), after.join(" "));
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": CLEAN}),
            serde_json::json!({"role": "assistant", "content": "[ילד] שיחק."}),
            serde_json::json!({"role": "user", "content": CLEAN}),
        ];
        messages[slot] = serde_json::json!({"role": "user", "content": text});
        let body = serde_json::json!({"system": "עוזרת.", "messages": messages});
        prop_assert!(gate_body(&body, &ids, CASE).is_err(), "cleared «{text}»");
    }

    /// Clean tagged text clears for any count of suspects = 0, and never for any count > 0.
    #[test]
    fn suspects_decide_alone_on_clean_text(n in 0usize..1000) {
        let ids = identities();
        let c = ctx(&ids, CASE);
        let tags = case_tags(&ids, CASE);
        let body = serde_json::json!({"messages": [{"role": "user", "content": CLEAN}]});
        let req = GateRequest { unresolved_suspects: n, ..request(&body, &c, &tags) };
        prop_assert_eq!(clear(&req).is_ok(), n == 0);
    }

    /// A random identifier (a valid ID, a mobile phone, an e-mail) among clean words blocks
    /// raw, and once filtered clears with none of its digits or its address left.
    #[test]
    fn identifiers_block_raw_and_leave_nothing_once_filtered(
        seed in 10_000_000u32..99_999_999,
        kind in 0u8..3,
        before in prop::sample::select(vec!["", "[ילד] סיפר:", "טלפון:", "מספר", "כתבו ל-", "בטופס"]),
        after in prop::sample::select(vec!["", ".", " בבוקר", ", תודה", " (הורה)"]),
    ) {
        let ids = identities();
        let seed = seed.to_string();
        let value = match kind {
            0 => valid_id(&seed),
            1 => mobile(seed.as_bytes()[0] - b'0', &seed[1..]),
            _ => email(&format!("p{seed}"), "example.org"),
        };
        let text = format!("{before} {value}{after}");
        prop_assert!(gate_text(&text, &ids, CASE).is_err(), "cleared raw «{text}»");
        let out = filter(&text, &ctx(&ids, CASE)).unwrap();
        prop_assert!(!out.tagged.contains(&value), "{} kept in {}", value, out.tagged);
        prop_assert!(!digits(&out.tagged).contains(&seed[1..]), "digits kept in {}", out.tagged);
        prop_assert!(gate_text(&out.tagged, &ids, CASE).is_ok(), "dead end on {}", out.tagged);
    }
}
