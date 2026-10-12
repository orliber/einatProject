//! What the local style learning keeps, and how it behaves on odd input. The design (D-052):
//! the style model keeps numbers only, never text; phrases and edits are learned from text that
//! is already neutralized (names are tags), and never hold a tag or a number. Every name and
//! number below is generated at run time from a fixed seed: nothing here is a real person.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dv_style::edits::{changes, wilson_lower, MIN_CHANGES};
use dv_style::retrieve::{best, bm25};
use dv_style::{mine_phrases, EditModel, ReportParts, StyleModel};
use proptest::prelude::*;

/// xorshift64*: deterministic invented names and numbers, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// An invented Hebrew name of three syllables, unlike any ordinary report word.
fn invented_name(rng: &mut Rng) -> String {
    const SYL: &[&str] = &["זב", "קפ", "טר", "גל", "פצ", "שק", "נח", "דמ", "צפ", "רז"];
    (0..3).map(|_| SYL[rng.below(SYL.len())]).collect()
}

/// A nine-digit number, as an identity number would be written (validity does not matter:
/// the point is that no digit string is ever learned).
fn invented_number(rng: &mut Rng) -> String {
    format!("{:09}", rng.next() % 1_000_000_000)
}

/// A paragraph in her steady voice (at least 12 words, a few sentences), mentioning a name.
fn paragraph(i: usize, name: &str, number: &str) -> String {
    let topics = ["המשחק", "השפה", "הקשב", "הציור", "התנועה", "החברה"];
    let t = topics[i % topics.len()];
    format!(
        "בתחום {t} נראה כי {name} פועל בהתאם לגילו, עם זאת ניכר קושי קל במעברים. \
         במהלך המפגש התרשמתי מילד סקרן ומשתף פעולה (מספר תיק {number}), אשר נעזר בתיווך. \
         כאשר המשימה התארכה הוא ביקש הפסקה קצרה וחזר לעבודה."
    )
}

/// Every string in a JSON document: keys and values.
fn strings(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        serde_json::Value::Object(o) => {
            for (k, x) in o {
                out.push(k.clone());
                strings(x, out);
            }
        }
        _ => {}
    }
}

#[test]
fn the_style_model_keeps_numbers_never_words() {
    let mut rng = Rng(0x5157_1e5e_ed00_0001);
    let people: Vec<(String, String)> = (0..20)
        .map(|_| (invented_name(&mut rng), invented_number(&mut rng)))
        .collect();
    let texts: Vec<String> = people
        .iter()
        .enumerate()
        .map(|(i, (n, id))| paragraph(i, n, id))
        .collect();
    let mut m = StyleModel::default();
    m.fit_reports(texts.iter().map(|t| (Some("observation"), t.as_str())));
    m.observe_edit("cognitive", &paragraph(7, &people[0].0, &people[0].1));
    assert!(m.paragraphs() > 20.0, "the model did learn");

    let json = serde_json::to_value(&m).unwrap();
    let mut found = Vec::new();
    strings(&json, &mut found);
    // The only strings in it are field names and the section keys it was given.
    let allowed = [
        "reports",
        "edits",
        "global",
        "sections",
        "function",
        "n",
        "mean",
        "m2",
        "observation",
        "cognitive",
    ];
    for s in &found {
        assert!(allowed.contains(&s.as_str()), "the model holds text: {s:?}");
    }
    let raw = json.to_string();
    for (name, id) in &people {
        assert!(!raw.contains(name.as_str()) && !raw.contains(id.as_str()));
    }
}

#[test]
fn the_style_model_is_deterministic_and_order_free() {
    let mut rng = Rng(7);
    let texts: Vec<String> = (0..24)
        .map(|i| {
            let (n, id) = (invented_name(&mut rng), invented_number(&mut rng));
            paragraph(i, &n, &id)
        })
        .collect();
    let fit = |order: &[String]| {
        let mut m = StyleModel::default();
        m.fit_reports(order.iter().map(|t| (Some("observation"), t.as_str())));
        m
    };
    let a = fit(&texts);
    assert_eq!(a, fit(&texts), "same input, same model");
    let mut reversed = texts.clone();
    reversed.reverse();
    let b = fit(&reversed);
    // Welford's running figures agree whatever the order, up to rounding.
    for (x, y) in a.reports.global.iter().zip(&b.reports.global) {
        assert!((x.n - y.n).abs() < 1e-9);
        assert!((x.mean - y.mean).abs() < 1e-9, "{x:?} vs {y:?}");
        assert!((x.m2 - y.m2).abs() < 1e-6, "{x:?} vs {y:?}");
    }
    let long = "הילד הציג תפקוד שהוא בהחלט תואם את גילו הכרונולוגי ברוב התחומים שנבדקו במהלך האבחון הנוכחי וגם בתחומים נוספים שעלו בשיחה עם ההורים ועם הצוות החינוכי של המסגרת בה הוא לומד כעת ובמסגרות קודמות. \
                בנוסף לכך הוא הפגין יכולת להתמיד במשימות לאורך זמן ממושך יחסית גם כאשר הן היו קשות עבורו ודרשו ממנו מאמץ רב ושימוש באסטרטגיות שונות ומגוונות של פתרון בעיות.";
    assert_eq!(a.note("observation", long), b.note("observation", long));
    assert!(a.note("observation", long).is_some());
}

#[test]
fn empty_blank_and_fragmentary_text_teaches_nothing() {
    let mut m = StyleModel::default();
    m.fit_reports(std::iter::empty());
    assert_eq!(m.paragraphs(), 0.0);
    m.fit_reports([(Some("observation"), ""), (None, "   \n\n\t")]);
    assert_eq!(m.paragraphs(), 0.0);
    let before = m.clone();
    for t in ["", "   ", "\n\n", "[ילד].", "12 34. 78!", "כן. לא. אולי?"] {
        m.observe_edit("observation", t);
    }
    // Nothing with a sentence of three words or more: nothing to measure, nothing learned.
    assert_eq!(m.paragraphs(), before.paragraphs());
    assert_eq!(m.note("observation", ""), None);
    assert_eq!(m.note("observation", "[ילד]"), None);
}

#[test]
fn huge_input_is_measured_without_trouble() {
    // One "paragraph" of 60,000 words with no full stop, and one of very many sentences.
    let endless = "הילד משחק בחצר עם חברים ".repeat(12_000);
    let many = "הילד משחק בחצר עם חברים בגן. ".repeat(10_000);
    let mut m = StyleModel::default();
    m.fit_reports([
        (Some("observation"), endless.as_str()),
        (None, many.as_str()),
    ]);
    m.observe_edit("observation", &many);
    for x in m
        .reports
        .global
        .iter()
        .chain(&m.edits.global)
        .chain(&m.reports.function)
    {
        assert!(
            x.n.is_finite() && x.mean.is_finite() && x.m2.is_finite(),
            "{x:?}"
        );
    }
    let _ = m.note("observation", &endless);
}

// ---------------------------------------------------------------- edits

const AI: &str = "חשוב לציין כי הילד מפגין סקרנות רבה ומשתף פעולה עם הבודקת לאורך המפגש.";
const HERS: &str = "הילד מראה סקרנות רבה ומשתף פעולה עם הבודקת לאורך המפגש.";

#[test]
fn edits_never_learn_a_number_or_a_tag() {
    let mut rng = Rng(0xED17);
    let mut m = EditModel::default();
    for i in 0..30 {
        let id = invented_number(&mut rng);
        let section = ["observation", "cognitive"][i % 2];
        // Numbers and tags replaced, deleted and inserted, around ordinary changes.
        let before = format!("[ילד] קיבל {id} נקודות ו[אם] סיפרה. {AI}");
        let after = format!("[ילד_2] קיבל {} נקודות. {HERS}", invented_number(&mut rng));
        m.observe_edit(section, &before, &after);
        m.observe_edit(section, &format!("מספר {id} נמסר"), "נמסר");
        m.observe_edit(section, "נמסר", &format!("מספר {id} נמסר"));
    }
    assert!(!m.pairs.is_empty(), "ordinary changes were learned");
    for p in &m.pairs {
        for s in [&p.from, &p.to] {
            assert!(
                !s.chars().any(|c| c.is_ascii_digit()) && !s.contains('[') && !s.contains(']'),
                "learned {p:?}"
            );
        }
    }
    for s in m.suggestions() {
        assert!(!s.from.chars().any(|c| c.is_ascii_digit()), "{s:?}");
    }
}

#[test]
fn edits_are_deterministic_and_survive_a_round_trip() {
    let run = || {
        let mut m = EditModel::default();
        for i in 0..6 {
            m.observe_edit(["observation", "cognitive"][i % 2], AI, HERS);
            m.observe_approval(AI);
        }
        m
    };
    let a = run();
    assert_eq!(a, run());
    assert_eq!(a.suggestions(), run().suggestions());
    let back: EditModel = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert_eq!(back, a);
}

#[test]
fn empty_and_oversized_paragraphs_are_not_aligned() {
    assert_eq!(changes("", HERS), (Vec::new(), None));
    assert_eq!(changes("   ", ""), (Vec::new(), None));
    // Past 400 words the quadratic alignment is skipped altogether: no learning, no figure.
    let long = "מילה ".repeat(401);
    assert_eq!(changes(&long, "מילה"), (Vec::new(), None));
    assert_eq!(changes("מילה", &long), (Vec::new(), None));
    let mut m = EditModel::default();
    m.observe_edit("observation", &long, HERS);
    assert!(m.pairs.is_empty());
    assert_eq!(m.fit.edits, 0);
    // An unchanged paragraph keeps everything and teaches no change.
    let (found, kept) = changes(AI, AI);
    assert!(found.is_empty());
    assert_eq!(kept, Some(1.0));
    // Deleting everything keeps nothing.
    assert_eq!(changes(AI, "").1, Some(0.0));
}

#[test]
fn the_learned_list_stays_bounded() {
    // 700 different one-word replacements: the list never grows past its ceiling, and what
    // she already decided on is kept.
    let mut rng = Rng(0xB0B);
    let mut m = EditModel::default();
    m.observe_edit("observation", "הילד מפגין סקרנות", "הילד מראה סקרנות");
    m.pairs[0].accepted = true;
    for _ in 0..700 {
        let (a, b) = (invented_name(&mut rng), invented_name(&mut rng));
        m.observe_edit(
            "observation",
            &format!("הילד {a} סקרנות"),
            &format!("הילד {b} סקרנות"),
        );
    }
    assert!(m.pairs.len() <= 500, "{}", m.pairs.len());
    assert!(m.pairs.iter().any(|p| p.accepted && p.from == "מפגין"));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// The Wilson lower bound is a cautious estimate: between 0 and the observed share, higher
    /// with more changes, and with the same share it grows as evidence grows.
    #[test]
    fn wilson_lower_bound_is_cautious_and_monotonic(n in 1u32..500, k in 0u32..500, extra in 1u32..10) {
        let k = k % (n + 1);
        let w = wilson_lower(k, n);
        prop_assert!((0.0..=1.0).contains(&w));
        prop_assert!(w <= f64::from(k) / f64::from(n) + 1e-12, "{k}/{n} → {w}");
        if k < n {
            prop_assert!(wilson_lower(k + 1, n) >= w);
        }
        prop_assert!(wilson_lower(k * extra, n * extra) >= w - 1e-12);
    }

    /// Too few changes never become a suggestion, however sure the rate looks.
    #[test]
    fn fewer_than_the_minimum_changes_never_suggest(times in 0u32..MIN_CHANGES) {
        let mut m = EditModel::default();
        for _ in 0..times {
            m.observe_edit("observation", AI, HERS);
        }
        prop_assert!(m.suggestions().is_empty());
    }

    /// Ranking is a pure function: the same documents and query give the same scores and the
    /// same picks, every pick is a real index, in order, within the budget.
    #[test]
    fn retrieval_is_deterministic_and_within_budget(
        docs in prop::collection::vec(
            prop::collection::vec(
                prop::sample::select(vec!["משחק", "ציור", "גזירה", "שפה", "דיבור", "ריצה", "[ילד]", "12", "של"]),
                0..8,
            ).prop_map(|w| w.join(" ")),
            0..12,
        ),
        query in prop::collection::vec(prop::sample::select(vec!["משחק", "ציור", "דיבור", "בגן"]), 0..5)
            .prop_map(|w| w.join(" ")),
        budget in 0usize..15,
    ) {
        let refs: Vec<&str> = docs.iter().map(String::as_str).collect();
        let s = bm25(&refs, &query);
        prop_assert_eq!(&s, &bm25(&refs, &query));
        prop_assert_eq!(s.len(), refs.len());
        prop_assert!(s.iter().all(|x| x.is_finite() && *x >= 0.0), "{s:?}");
        let picked = best(&refs, &query, budget);
        prop_assert_eq!(&picked, &best(&refs, &query, budget));
        prop_assert_eq!(picked.len(), budget.min(refs.len()));
        prop_assert!(picked.windows(2).all(|w| w[0] < w[1]));
        prop_assert!(picked.iter().all(|i| *i < refs.len()));
    }
}

// ---------------------------------------------------------------- phrases

/// Invented reports in neutralized form: the child is a tag, each report has its own case
/// number and a few words about that child, and two habits of hers recur in all of them.
fn reports(rng: &mut Rng, n: usize) -> Vec<Vec<(Option<&'static str>, String)>> {
    (0..n)
        .map(|_| {
            let id = invented_number(rng);
            let one_off = invented_name(rng);
            vec![
                (
                    Some("observation"),
                    format!(
                        "ניכר כי [ילד] פועל בהתאם לגילו. תיק {id}. הוא שיחק עם {one_off} בחצר."
                    ),
                ),
                (
                    Some("recommendations"),
                    format!("מומלץ להמשיך במעקב התפתחותי. [ילד] [אם] {id}."),
                ),
            ]
        })
        .collect()
}

fn parts<'a>(r: &'a [Vec<(Option<&'static str>, String)>]) -> Vec<ReportParts<'a>> {
    r.iter()
        .map(|p| p.iter().map(|(s, t)| (*s, t.as_str())).collect())
        .collect()
}

#[test]
fn phrases_never_hold_a_tag_a_number_or_what_one_report_says() {
    let mut rng = Rng(0xF00D);
    let r = reports(&mut rng, 8);
    let found = mine_phrases(&parts(&r));
    assert!(
        found
            .iter()
            .any(|p| p.text == "מומלץ להמשיך במעקב התפתחותי"),
        "{found:?}"
    );
    for p in &found {
        assert!(
            !p.text.contains('[') && !p.text.chars().any(|c| c.is_ascii_digit()),
            "{p:?}"
        );
        assert!(p.reports >= 3, "{p:?}");
    }
    // The names that appeared once each (invented, one per report) are never offered.
    let mut rng = Rng(0xF00D);
    for _ in 0..8 {
        let _ = invented_number(&mut rng);
        let one_off = invented_name(&mut rng);
        assert!(
            !found.iter().any(|p| p.text.contains(&one_off)),
            "{one_off}"
        );
    }
}

#[test]
fn phrases_are_deterministic_and_do_not_depend_on_report_order() {
    let mut rng = Rng(0xABCD);
    let r = reports(&mut rng, 7);
    let a = mine_phrases(&parts(&r));
    assert_eq!(a, mine_phrases(&parts(&r)));
    let mut shuffled = r.clone();
    shuffled.rotate_left(3);
    shuffled.swap(0, 4);
    assert_eq!(a, mine_phrases(&parts(&shuffled)));
}

#[test]
fn phrases_from_empty_or_huge_input_stay_within_limits() {
    assert!(mine_phrases(&[]).is_empty());
    let blank: Vec<ReportParts<'_>> = (0..5)
        .map(|_| vec![(None, ""), (Some("x"), "  ")])
        .collect();
    assert!(mine_phrases(&blank).is_empty());
    // Forty different recurring phrases in every report: at most 8 per section, 30 in all.
    let mut rng = Rng(0x1234);
    let vocab: Vec<String> = (0..80).map(|_| invented_name(&mut rng)).collect();
    let text: String = (0..40)
        .map(|i| format!("{} {} תמיד.", vocab[2 * i], vocab[2 * i + 1]))
        .collect::<Vec<_>>()
        .join(" ");
    let sections = ["a", "b", "c", "d", "e", "f"];
    let reports: Vec<Vec<(Option<&str>, String)>> = (0..6)
        .map(|_| sections.iter().map(|s| (Some(*s), text.clone())).collect())
        .collect();
    let refs: Vec<ReportParts<'_>> = reports
        .iter()
        .map(|p| p.iter().map(|(s, t)| (*s, t.as_str())).collect())
        .collect();
    let found = mine_phrases(&refs);
    assert!(!found.is_empty() && found.len() <= 30, "{}", found.len());
    let mut per: std::collections::BTreeMap<Option<String>, usize> = Default::default();
    for p in &found {
        *per.entry(p.section.clone()).or_default() += 1;
    }
    assert!(per.values().all(|n| *n <= 8), "{per:?}");
}
