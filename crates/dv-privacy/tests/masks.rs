//! The mask book (round 25): numbers, dates, addresses and places go out as numbered tags of
//! the case and come back as she wrote them. Nothing of them goes out, and the gate takes the
//! numbered tags like the fixed ones.

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
use dv_privacy::{filter, MaskBook, PrivacyContext};

fn with_book<'a>(ids: &'a [dv_domain::Identity], book: &'a MaskBook) -> PrivacyContext<'a> {
    PrivacyContext {
        masks: Some(book),
        ..ctx(ids, CASE)
    }
}

#[test]
fn identifiers_go_out_as_numbered_tags_and_come_back_as_written() {
    let ids = identities();
    let book = MaskBook::new(Vec::new());
    let id_no = valid_id("12345678");
    let phone = mobile(2, "4567890");
    let mail = email("dana.k", "example.co.il");
    let text = format!(
        "נועם גר ברחוב הרצל 12 בחיפה. ת.ז. {id_no}, טלפון האם {phone}, דוא\"ל {mail}. הטלפון שוב: {phone}."
    );
    let out = filter(&text, &with_book(&ids, &book)).unwrap();
    for secret in [&id_no, &phone, &mail] {
        assert!(
            !contains_norm(&out.tagged, secret),
            "{secret} left in {}",
            out.tagged
        );
        assert!(!digits(&out.tagged).contains(&digits(secret)) || digits(secret).is_empty());
    }
    assert!(
        !out.tagged.contains("[טלפון]") && !out.tagged.contains("[ת.ז.]"),
        "{}",
        out.tagged
    );
    // The same phone twice: one tag.
    let phone_tags: Vec<_> = book
        .entries()
        .into_iter()
        .filter(|e| e.kind == "טלפון")
        .collect();
    assert_eq!(phone_tags.len(), 1, "{phone_tags:?}");
    assert_eq!(
        out.tagged.matches(phone_tags[0].tag.as_str()).count(),
        2,
        "{}",
        out.tagged
    );
    // The gate takes it as it is.
    gate_text(&out.tagged, &ids, CASE).expect("the filter's own output passes the gate");
    // And the answer that uses the tags reads as she wrote it.
    let back = restore(&out.tagged, &ids, None, &book.entries());
    assert_eq!(back, text);
}

#[test]
fn another_request_of_the_case_gives_the_same_words_the_same_tag() {
    let ids = identities();
    let first = MaskBook::new(Vec::new());
    let phone = mobile(4, "1112223");
    let a = filter(&format!("טלפון {phone}"), &with_book(&ids, &first)).unwrap();
    let kept = first.added();
    let second = MaskBook::new(kept.clone());
    let b = filter(
        &format!("ושוב {phone}, ומספר נוסף {}", mobile(4, "9998887")),
        &with_book(&ids, &second),
    )
    .unwrap();
    let tag = &kept[0].tag;
    assert!(
        a.tagged.contains(tag.as_str()) && b.tagged.contains(tag.as_str()),
        "{} / {}",
        a.tagged,
        b.tagged
    );
    assert_eq!(second.added().len(), 1);
    assert_ne!(&second.added()[0].tag, tag);
}

#[test]
fn without_a_book_the_fixed_tags_stay() {
    let ids = identities();
    let phone = mobile(2, "4567890");
    let out = filter(&format!("טלפון {phone}"), &ctx(&ids, CASE)).unwrap();
    assert!(out.tagged.contains("[טלפון]"), "{}", out.tagged);
}

#[test]
fn a_numbered_tag_in_her_text_or_in_an_answer_is_never_read_as_a_pattern() {
    let ids = identities();
    for base in dv_privacy::MASK_BASES {
        for n in [1, 7, 12, 305] {
            let tag = format!("[{base}_{n}]");
            for prefix in ["", "ו", "ב", "ל", "מ", "ש", "וב", "ו-"] {
                for after in ["", " 2", " ו-3", " תשפ\"ה"] {
                    let text = format!("[ילד] ציין {prefix}{tag}{after} בשיחה.");
                    let out = filter(&text, &ctx(&ids, CASE)).unwrap();
                    assert!(out.tagged.contains(&tag), "{text} -> {}", out.tagged);
                    gate_text(&out.tagged, &ids, CASE).unwrap_or_else(|b| panic!("{text}: {b:?}"));
                }
            }
        }
    }
}

#[test]
fn a_restored_tag_is_only_the_case_books() {
    let ids = identities();
    let book = MaskBook::new(Vec::new());
    let out = filter(
        &format!("טלפון {}", mobile(2, "4567890")),
        &with_book(&ids, &book),
    )
    .unwrap();
    // A tag the book does not hold stays a tag: never someone else's words.
    let answer = format!("{} ו[טלפון_9]", out.tagged);
    let back = restore(&answer, &ids, None, &book.entries());
    assert!(back.ends_with("ו[טלפון_9]"), "{back}");
}

/// PRIV-4: a child's surname alone is hidden under a numbered tag of the book, so it comes
/// back as the surname, never as the child's first name ("משפחת נחמיאס" was shown and exported
/// as "משפחת הדר"). The full name still goes out as one child's tag, and nothing of the
/// surname goes out.
#[test]
fn a_childs_surname_alone_comes_back_as_the_surname() {
    use dv_domain::Role;
    for child in [
        id(CASE, Role::Child, "[ילד]", "הדר", &["נחמיאס"]),
        id(CASE, Role::Child, "[ילד]", "הדר נחמיאס", &[]),
        id(CASE, Role::Child, "[ילד]", "הדר", &["Hadar Nahmias"]),
    ] {
        let ids = vec![
            child,
            id(CASE, Role::Sister, "[אחות_1]", "שירה", &["שירה נחמיאס"]),
        ];
        let book = MaskBook::new(Vec::new());
        for text in [
            "משפחת נחמיאס פנתה בשל עיכוב בדיבור.",
            "אמו, גב' נחמיאס, דיווחה על שינה טובה.",
            "הדר נחמיאס הגיעה עם אמה, ומשפחת נחמיאס סיפרה על המעבר.",
            "הוריה של הדר מספרים: לנחמיאס יש כלב.",
            "ראיתי את שירה נחמיאס ואת הדר, ואת שירה לבד.",
        ] {
            let out = filter(text, &with_book(&ids, &book)).unwrap();
            for secret in ["נחמיאס", "הדר", "שירה", "nahmias"] {
                assert!(
                    !contains_norm(&out.tagged, secret),
                    "{secret} left in {}",
                    out.tagged
                );
            }
            gate_text(&out.tagged, &ids, CASE)
                .unwrap_or_else(|b| panic!("{} refused: {b:?}", out.tagged));
            // A full name may come back by first name, as a child is written (prose_name);
            // every surname alone comes back as the surname.
            let back = restore(&out.tagged, &ids, None, &book.entries());
            let as_written = |t: &str| {
                t.replace("הדר נחמיאס", "הדר")
                    .replace("שירה נחמיאס", "שירה")
            };
            assert_eq!(as_written(&back), as_written(text), "{}", out.tagged);
        }
        // Where the child's own name holds the surname, the full name is still the child's
        // one tag, and comes back by first name.
        if ids[0].value.contains("נחמיאס") || ids[0].aliases.iter().any(|a| a == "נחמיאס")
        {
            let out = filter("הדר נחמיאס הגיעה.", &with_book(&ids, &book)).unwrap();
            assert_eq!(out.tagged, "[ילד] הגיעה.", "{:?}", ids[0]);
        }
    }
}

/// An alias that is a form of the first name (another script, a misread one letter off)
/// keeps the child's tag, as before: it is the first name, not a surname.
#[test]
fn an_alias_that_is_the_first_name_keeps_the_childs_tag() {
    use dv_domain::Role;
    let ids = vec![id(CASE, Role::Child, "[ילד]", "אלון", &["Alon", "אלוו"])];
    let book = MaskBook::new(Vec::new());
    for (text, want) in [
        ("Alon came.", "[ילד] came."),
        ("אלוו הגיע.", "[ילד] הגיע."),
        ("אלון הגיע.", "[ילד] הגיע."),
    ] {
        let out = filter(text, &with_book(&ids, &book)).unwrap();
        assert_eq!(out.tagged, want, "{text}");
    }
    assert!(book.entries().is_empty(), "{:?}", book.entries());
}

/// PRIV-4, review: a name written surname first ("נחמיאס הדר", as forms and lists write it)
/// still names the child to the model by the child's tag, and comes back as written, never
/// as "הדר הדר".
#[test]
fn a_full_name_written_surname_first_keeps_the_childs_tag() {
    use dv_domain::Role;
    for child in [
        id(CASE, Role::Child, "[ילד]", "הדר", &["נחמיאס"]),
        id(CASE, Role::Child, "[ילד]", "הדר נחמיאס", &[]),
        id(CASE, Role::Child, "[ילד]", "הדר", &["הדר נחמיאס"]),
    ] {
        let ids = vec![child];
        let book = MaskBook::new(Vec::new());
        for text in ["נחמיאס הדר, בת 5, הופנתה לאבחון.", "שם: נחמיאס הדר."]
        {
            let out = filter(text, &with_book(&ids, &book)).unwrap();
            assert!(!contains_norm(&out.tagged, "נחמיאס"), "{}", out.tagged);
            assert!(!contains_norm(&out.tagged, "הדר"), "{}", out.tagged);
            assert!(out.tagged.contains("[ילד]"), "{:?}: {}", ids[0], out.tagged);
            gate_text(&out.tagged, &ids, CASE)
                .unwrap_or_else(|b| panic!("{} refused: {b:?}", out.tagged));
            // Back as written, or by first name when the whole name was one tag.
            let back = restore(&out.tagged, &ids, None, &book.entries());
            assert!(
                back == text || back == text.replace("נחמיאס הדר", "הדר"),
                "{:?}: {} -> {back}",
                ids[0],
                out.tagged
            );
        }
    }
}
