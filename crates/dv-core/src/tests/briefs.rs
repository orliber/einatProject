//! Her drafting wishes (D-140): recent ones and one kept per section, in the vault.

use super::*;

#[test]
fn recent_wishes_are_newest_first_unique_and_five_and_a_kept_one_is_per_section() {
    let (_dir, mut core, _case) = setup(None);
    for i in 0..7 {
        core.keep_draft_brief("background", &format!("בקשה {i}"), false, None)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    core.keep_draft_brief("background", "בקשה 3", true, None)
        .unwrap_or_else(|e| panic!("{e}"));
    let b = core.draft_briefs().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(b.recent, ["בקשה 3", "בקשה 6", "בקשה 5", "בקשה 4", "בקשה 2"]);
    assert_eq!(
        b.sections.get("background").map(String::as_str),
        Some("בקשה 3")
    );
    // Sent without keeping: the section's kept wish is forgotten, the recent one stays.
    let b = core
        .keep_draft_brief("background", "", false, None)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(b.sections.is_empty());
    assert_eq!(b.recent.len(), 5);
}

#[test]
fn an_unknown_section_or_a_long_wish_is_refused() {
    let (_dir, mut core, _case) = setup(None);
    assert!(core
        .keep_draft_brief("no-such-section", "קצר", true, None)
        .is_err());
    assert!(core
        .keep_draft_brief("background", &"א".repeat(1_001), false, None)
        .is_err());
    assert_eq!(
        core.draft_briefs().unwrap_or_else(|e| panic!("{e}")),
        Default::default()
    );
}

#[test]
fn an_untick_forgets_only_the_kept_wish_it_was_about() {
    let (_dir, mut core, _case) = setup(None);
    let kept = |core: &mut Core| {
        core.draft_briefs()
            .unwrap_or_else(|e| panic!("{e}"))
            .sections
            .get("background")
            .cloned()
    };
    core.keep_draft_brief("background", "המשאלה הישנה", true, None)
        .unwrap_or_else(|e| panic!("{e}"));
    // Another report kept a newer wish since she unticked "לזכור" against the old one: sending
    // her words unticked leaves the newer wish, and her words are still the newest recent one.
    core.keep_draft_brief("background", "המשאלה החדשה", true, None)
        .unwrap_or_else(|e| panic!("{e}"));
    let b = core
        .keep_draft_brief("background", "רק לילד הזה", false, Some("המשאלה הישנה"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(b.recent.first().map(String::as_str), Some("רק לילד הזה"));
    assert_eq!(kept(&mut core).as_deref(), Some("המשאלה החדשה"));
    // An empty box, ticked or not, is the same: no words to keep, and not her untick of it.
    core.keep_draft_brief("background", "", true, Some("המשאלה הישנה"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&mut core).as_deref(), Some("המשאלה החדשה"));
    // Unticked against the wish that is kept (as the box showed it, with its spaces and line
    // breaks): it is forgotten.
    core.keep_draft_brief("background", "אחרת", false, Some(" המשאלה החדשה\r\n"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&mut core), None);
    // Ticked, her words are the section's wish whichever wish she saw.
    core.keep_draft_brief("background", "המשאלה החדשה", true, None)
        .unwrap_or_else(|e| panic!("{e}"));
    core.keep_draft_brief("background", "שלי", true, Some("המשאלה הישנה"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&mut core).as_deref(), Some("שלי"));
    // No wish kept: unticked against any wish is nothing to forget.
    core.keep_draft_brief("background", "", false, None)
        .unwrap_or_else(|e| panic!("{e}"));
    core.keep_draft_brief("background", "עוד", false, Some("שלי"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&mut core), None);
}
