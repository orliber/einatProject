//! The gate checks itself at run time, for the security page (L6, D-078). With an invented
//! child, three requests go to the real gate: the child's name in the clear, and a trap
//! string, must each be refused; the same text with the name hidden must pass (so a gate that
//! refuses everything is not shown as working). Nothing is sent anywhere.

use std::collections::HashSet;

use dv_domain::dates::Ymd;
use dv_domain::{Identity, IdentitySource, Role};

use crate::gate::{clear, GateRequest};
use crate::pipeline::PrivacyContext;

/// An invented child, in a case that does not exist.
const CASE: &str = "self-check";
const NAME: &str = "נועם";
const TRAP: &str = "DV-SELF-CHECK-TRAP";

fn body(text: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "claude-opus-5",
        "system": "בדיקה עצמית של השער. שום דבר לא נשלח.",
        "messages": [{"role": "user", "content": text}],
    })
}

/// True when the gate refuses a name in the clear and a trap string, and lets the same text
/// through with the name hidden.
#[must_use]
pub fn gate_self_check(today: Ymd) -> bool {
    let ids = [Identity {
        id: "self-check-child".to_owned(),
        case_id: CASE.to_owned(),
        role: Role::Child,
        tag: "[ילד]".to_owned(),
        value: NAME.to_owned(),
        aliases: Vec::new(),
        source: IdentitySource::Manual,
        reason: String::new(),
    }];
    let practitioner: Vec<String> = Vec::new();
    let ctx = PrivacyContext {
        case_id: CASE,
        identities: &ids,
        practitioner: &practitioner,
        allowlisted: &|_: &str| false,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today,
        masks: None,
    };
    let case_tags: HashSet<String> = ids.iter().map(|i| i.tag.clone()).collect();
    let canaries = vec![TRAP.to_owned()];
    let passes = |text: &str| {
        let b = body(text);
        clear(&GateRequest {
            body: &b,
            ctx: &ctx,
            case_tags: &case_tags,
            unresolved_suspects: 0,
            canaries: &canaries,
            max_bytes: 200_000,
        })
        .is_ok()
    };
    !passes(&format!("{NAME} שיחק בחצר עם חברים."))
        && !passes(&format!("[ילד] שיחק בחצר עם חברים. {TRAP}"))
        && passes("[ילד] שיחק בחצר עם חברים.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_passes_its_own_check() {
        assert!(gate_self_check((2026, 10, 15)));
    }
}
