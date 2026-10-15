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
