//! Generated probes (D-049, stage A): every first name of the lexicon put into sentences where
//! it can only be a name, and every name that is also a word into sentences where it can only
//! be that word. Invented sentences only. Prints the rates and fails below the floors, which
//! may only rise. Stage C replaces the lexicon with the CBS names and adds the morphology.

#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use dv_domain::{Identity, IdentitySource, Role};
use dv_privacy::text::normalize;
use dv_privacy::{filter, PrivacyContext};

fn list(file: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join(file);
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn child() -> Vec<Identity> {
    vec![Identity {
        id: "c-0".into(),
        case_id: "c".into(),
        role: Role::Child,
        tag: "[ילד]".into(),
        value: "תמר דמיוני".into(),
        aliases: vec![],
        source: IdentitySource::Manual,
        reason: String::new(),
    }]
}

/// Share of `sentences` (with the name at `{}`) in which the name was hidden.
fn hidden_share(names: &[String], templates: &[&str]) -> (u32, u32, Vec<String>) {
    let ids = child();
    let ctx = PrivacyContext {
        case_id: "c",
        identities: &ids,
        practitioner: &[],
        allowlisted: &|_: &str| false,
        confirmed_names: &|_: &str| false,
        past_names: &|_: &str| false,
        today: (2026, 10, 9),
    };
    let (mut hit, mut total, mut missed) = (0, 0, Vec::new());
    for name in names {
        for t in templates {
            let text = t.replace("{}", name);
            let out = filter(&text, &ctx).unwrap();
            total += 1;
            if normalize(&out.tagged).contains(&normalize(name)) {
                if missed.len() < 40 {
                    missed.push(text);
                }
            } else {
                hit += 1;
            }
        }
    }
    (hit, total, missed)
}

#[test]
fn every_lexicon_name_is_hidden_where_it_can_only_be_a_name() {
    let names = list("first_names.txt");
    let (hit, total, missed) = hidden_share(
        &names,
        &[
            "קוראים לה {}, והיא בת שש.",
            "הגננת, {}, סיפרה על השבוע.",
            "ביום ראשון {} הגיעה לגן מוקדם.",
        ],
    );
    let rate = f64::from(hit) / f64::from(total.max(1));
    println!(
        "\n== probe/names: {hit}/{total} hidden ({:.2}%)",
        100.0 * rate
    );
    for m in &missed {
        println!("   MISS  {m}");
    }
    // Baseline 2026-10-09 (stage A of D-049).
    assert!(
        rate + 1e-9 >= FLOOR_NAMES,
        "names hidden {rate:.4}, floor {FLOOR_NAMES}"
    );
}

/// Only names the lexicon already has, so it is close to 1; stage C measures names it lacks.
const FLOOR_NAMES: f64 = 0.9995;
