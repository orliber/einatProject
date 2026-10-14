//! The Zero-Data-Retention policy checked on every outgoing body, and the type boundary that
//! makes the privacy gate the only source of sendable bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::marker::PhantomData;

use dv_egress::{check_policy, transport_for, EgressError, Provider, ALLOWED_MODELS};
use dv_privacy::ClearedPayload;
use proptest::prelude::*;
use serde_json::{json, Value};

const FIELDS: &[&str] = &[
    "model",
    "max_tokens",
    "system",
    "messages",
    "output_config",
    "inference_geo",
    "tools",
    "thinking",
    "stream",
];

fn ok_body() -> Value {
    json!({"model": "claude-opus-5", "max_tokens": 100, "inference_geo": "us", "messages": []})
}

fn search(max_uses: Value, domains: Value) -> Value {
    json!({"type": "web_search_20250305", "name": "web_search", "max_uses": max_uses, "allowed_domains": domains})
}

// ---------------------------------------------------------------- the type boundary

/// Asks at compile time whether a type has a trait, answering at run time (autoref
/// specialization: the first method found wins, and the fallback needs one more `&`).
macro_rules! has {
    ($t:ty, $($bound:tt)+) => {{
        struct Probe<T>(PhantomData<T>);
        // One of the two is always unused: which one is the answer.
        #[allow(dead_code)]
        trait Yes { fn get(&self) -> bool { true } }
        impl<T: $($bound)+> Yes for Probe<T> {}
        #[allow(dead_code)]
        trait No { fn get(&self) -> bool { false } }
        impl<T> No for &Probe<T> {}
        (&Probe::<$t>(PhantomData)).get()
    }};
}

#[test]
fn a_cleared_payload_cannot_be_made_outside_the_gate() {
    // The probe works: a type that has these traits says so.
    assert!(has!(String, Default));
    assert!(has!(String, for<'de> serde::Deserialize<'de>));
    assert!(has!(String, From<&'static str>));
    assert!(has!(Vec<u8>, From<Vec<u8>>));

    // No way to conjure sendable bytes: no default, no deserializing (from IPC, a file or the
    // UI), no conversion from raw bytes, text or JSON. Fields are private, and CI checks that
    // only the gate constructs one.
    assert!(!has!(ClearedPayload, Default));
    assert!(!has!(ClearedPayload, for<'de> serde::Deserialize<'de>));
    assert!(!has!(ClearedPayload, From<Vec<u8>>));
    assert!(!has!(ClearedPayload, From<String>));
    assert!(!has!(ClearedPayload, From<&'static str>));
    assert!(!has!(ClearedPayload, From<Value>));
    assert!(!has!(ClearedPayload, std::str::FromStr));
    // Nor can it be serialized somewhere else: its bytes leave only through `body()`.
    assert!(!has!(ClearedPayload, serde::Serialize));
}

// ---------------------------------------------------------------- the ZDR policy

#[test]
fn every_allowed_model_passes_and_the_zdr_list_is_exact() {
    for m in ALLOWED_MODELS {
        let mut b = ok_body();
        b["model"] = json!(m);
        assert!(check_policy(&b).is_ok(), "{m}");
        assert_eq!(Provider::of_model(m), Some(Provider::Anthropic));
    }
    // Models that keep data for 30 days, other companies' models, and near misses.
    for m in [
        "claude-fable-5-1",
        "claude-mythos-1",
        "claude-opus-4",
        "claude-opus-5-latest",
        "CLAUDE-OPUS-5",
        " claude-opus-5",
        "claude-opus-5\n",
        "claude-opus-5\u{200b}",
        "gpt-5-1",
        "gemini-2-5-pro",
        "local-gemma",
    ] {
        let mut b = ok_body();
        b["model"] = json!(m);
        assert!(
            matches!(check_policy(&b), Err(EgressError::Policy(_))),
            "{m:?}"
        );
    }
    for m in [
        json!(null),
        json!(5),
        json!(["claude-opus-5"]),
        json!({"id": "claude-opus-5"}),
    ] {
        let mut b = ok_body();
        b["model"] = m.clone();
        assert!(check_policy(&b).is_err(), "{m}");
    }
}

#[test]
fn the_body_must_be_an_object_pinned_to_the_us() {
    for body in [json!([]), json!("x"), json!(null), json!(1)] {
        assert!(check_policy(&body).is_err(), "{body}");
    }
    for geo in [json!("US"), json!("eu"), json!(null), json!(["us"])] {
        let mut b = ok_body();
        b["inference_geo"] = geo.clone();
        assert!(check_policy(&b).is_err(), "{geo}");
    }
}

#[test]
fn web_search_needs_domains_and_a_small_use_limit() {
    let refused = [
        search(json!(0), json!(["apa.org"])),
        search(json!(9), json!(["apa.org"])),
        search(json!(-1), json!(["apa.org"])),
        search(json!("3"), json!(["apa.org"])),
        search(json!(null), json!(["apa.org"])),
        search(json!(3), json!([])),
        search(json!(3), json!("apa.org")),
        search(json!(3), json!(null)),
        // Newer search versions with dynamic filtering are not ZDR-eligible.
        json!({"type": "web_search_20260209", "name": "web_search", "max_uses": 3, "allowed_domains": ["apa.org"]}),
    ];
    for tool in refused {
        let mut b = ok_body();
        b["tools"] = json!([tool]);
        assert!(check_policy(&b).is_err(), "{tool}");
    }
    for n in 1..=8 {
        let mut b = ok_body();
        b["tools"] = json!([search(json!(n), json!(["apa.org", "who.int"]))]);
        assert!(check_policy(&b).is_ok(), "{n}");
    }
    // One bad tool among good ones refuses the whole body.
    let mut b = ok_body();
    b["tools"] = json!([
        search(json!(3), json!(["apa.org"])),
        {"type": "code_execution_20250825", "name": "code_execution"}
    ]);
    assert!(check_policy(&b).is_err());
}

#[test]
fn no_transport_exists_for_a_model_off_every_list() {
    for m in ["claude-fable-5-1", "gpt-4o", "gemini-2.5-pro", "", "local"] {
        assert!(
            matches!(
                transport_for(m, "test-key-not-real"),
                Err(EgressError::Policy(_))
            ),
            "{m}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Any model name that is not on the list is refused.
    #[test]
    fn any_other_model_name_is_refused(model in "\\PC{0,30}") {
        prop_assume!(!ALLOWED_MODELS.contains(&model.as_str()));
        let mut b = ok_body();
        b["model"] = json!(model);
        prop_assert!(matches!(check_policy(&b), Err(EgressError::Policy(_))));
    }

    /// Any top-level field that is not on the list is refused, whatever its value.
    #[test]
    fn any_other_top_level_field_is_refused(field in "[a-z_]{1,20}", value in "\\PC{0,10}") {
        prop_assume!(!FIELDS.contains(&field.as_str()));
        let mut b = ok_body();
        b[field.as_str()] = json!(value);
        prop_assert!(matches!(check_policy(&b), Err(EgressError::Policy(_))));
    }

    /// Any tool type other than the basic web search is refused, even with the right shape.
    #[test]
    fn any_other_tool_type_is_refused(kind in "[a-z_0-9]{1,30}") {
        prop_assume!(kind != "web_search_20250305");
        let mut b = ok_body();
        b["tools"] = json!([{"type": kind, "name": "web_search", "max_uses": 3, "allowed_domains": ["apa.org"]}]);
        prop_assert!(check_policy(&b).is_err());
    }
}
