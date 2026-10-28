//! Her drafting wishes (D-140): recent ones and one kept per section, in the vault.

use super::*;

#[test]
fn recent_wishes_are_newest_first_unique_and_five_and_a_kept_one_is_per_section() {
    let (_dir, mut core, _case) = setup(None);
    for i in 0..7 {
        core.keep_draft_brief("background", &format!("בקשה {i}"), false)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    core.keep_draft_brief("background", "בקשה 3", true)
        .unwrap_or_else(|e| panic!("{e}"));
    let b = core.draft_briefs().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(b.recent, ["בקשה 3", "בקשה 6", "בקשה 5", "בקשה 4", "בקשה 2"]);
    assert_eq!(
        b.sections.get("background").map(String::as_str),
        Some("בקשה 3")
    );
    // Sent without keeping: the section's kept wish is forgotten, the recent one stays.
    let b = core
        .keep_draft_brief("background", "", false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(b.sections.is_empty());
    assert_eq!(b.recent.len(), 5);
}

#[test]
fn an_unknown_section_or_a_long_wish_is_refused() {
    let (_dir, mut core, _case) = setup(None);
    assert!(core
        .keep_draft_brief("no-such-section", "קצר", true)
        .is_err());
    assert!(core
        .keep_draft_brief("background", &"א".repeat(1_001), false)
        .is_err());
    assert_eq!(
        core.draft_briefs().unwrap_or_else(|e| panic!("{e}")),
        Default::default()
    );
}
