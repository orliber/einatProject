//! ST13: does the closeness figure measure what it claims? Two invented writers, generated from
//! a fixed seed: "her" (short sentences, a careful "נראה", first person, three sentences to a
//! paragraph) and a generic drafting voice (long sentences, contrasts and commas, no first
//! person). The model learns from her reports only; the figure must then tell held-out
//! paragraphs of hers from the generic ones, and a generic paragraph rewritten toward the
//! figures `closer_ask` names must score higher than before. Every word here is invented.
//!
//! The numbers are printed (`cargo test -p dv-style --test fidelity -- --nocapture`), so a
//! change to the style model can be weighed by them, not by feel.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::indexing_slicing
)]

use dv_style::StyleModel;

/// xorshift64*: deterministic, no dependency.
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
    fn between(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }
}

const WORDS: &[&str] = &[
    "הילד",
    "המשחק",
    "המשימה",
    "הקשב",
    "השפה",
    "התנועה",
    "בחדר",
    "המבוגר",
    "הציור",
    "הקוביות",
    "בתחילת",
    "המפגש",
    "לאחר",
    "מכן",
    "בעבודה",
    "עם",
    "החומרים",
    "בזמן",
    "הפעילות",
    "בקצב",
    "שלו",
    "באופן",
    "עקבי",
    "בהדרכה",
    "קצרה",
    "בתורו",
    "ברגע",
    "בסוף",
    "הסיפור",
    "התמונה",
];

fn words(rng: &mut Rng, n: usize) -> Vec<String> {
    (0..n)
        .map(|_| WORDS[rng.below(WORDS.len())].to_owned())
        .collect()
}

/// Her voice: 7–11 words, often a hedge or "התרשמתי", three sentences.
fn hers(rng: &mut Rng) -> String {
    (0..3)
        .map(|i| {
            let n = rng.between(6, 10);
            let mut w = words(rng, n);
            if i == 0 {
                w.insert(0, "נראה כי".into());
            }
            if i == 1 {
                w.insert(0, "התרשמתי כי".into());
            }
            format!("{}.", w.join(" "))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The generic drafting voice: 18–26 words, contrasts and commas, five sentences.
fn generic(rng: &mut Rng) -> String {
    (0..5)
        .map(|i| {
            let n = rng.between(16, 23);
            let mut w = words(rng, n);
            let mid = w.len() / 2;
            w[mid] = format!("{},", w[mid]);
            if i % 2 == 0 {
                w.insert(mid + 1, "עם זאת".into());
            }
            format!("{}.", w.join(" "))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A voice close to hers with two things off: somewhat longer sentences, no first person.
fn near(rng: &mut Rng) -> String {
    (0..3)
        .map(|i| {
            let n = rng.between(11, 14);
            let mut w = words(rng, n);
            if i == 0 {
                w.insert(0, "נראה כי".into());
            }
            format!("{}.", w.join(" "))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The share of (hers, generic) pairs in which hers scores higher: the area under the ROC
/// curve, ties counting half.
fn auc(hers: &[u32], others: &[u32]) -> f64 {
    let mut wins = 0.0;
    for h in hers {
        for o in others {
            wins += match h.cmp(o) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Less => 0.0,
            };
        }
    }
    wins / (hers.len() * others.len()) as f64
}

fn mean(xs: &[u32]) -> f64 {
    xs.iter().map(|x| f64::from(*x)).sum::<f64>() / xs.len() as f64
}

#[test]
fn the_closeness_figure_tells_her_writing_from_a_generic_voice() {
    let mut rng = Rng(0x5EED_57E1);
    let reports: Vec<String> = (0..40).map(|_| hers(&mut rng)).collect();
    let mut model = StyleModel::default();
    model.fit_reports(reports.iter().map(|t| (Some("observation"), t.as_str())));

    let held_out: Vec<u32> = (0..30)
        .map(|_| model.fidelity("observation", &hers(&mut rng)).unwrap())
        .collect();
    let others: Vec<u32> = (0..30)
        .map(|_| model.fidelity("observation", &generic(&mut rng)).unwrap())
        .collect();
    let a = auc(&held_out, &others);
    println!(
        "fidelity: hers {:.1}, generic {:.1}, AUC {a:.3}",
        mean(&held_out),
        mean(&others)
    );
    assert!(a >= 0.95, "AUC {a}");
    assert!(mean(&held_out) - mean(&others) >= 30.0);

    // The harder case: a voice that is almost hers. Still told apart most of the time.
    let near_scores: Vec<u32> = (0..30)
        .map(|_| model.fidelity("observation", &near(&mut rng)).unwrap())
        .collect();
    let b = auc(&held_out, &near_scores);
    println!(
        "fidelity: near {:.1}, AUC against hers {b:.3}",
        mean(&near_scores)
    );
    assert!(b >= 0.75, "AUC {b}");
    assert!(
        mean(&near_scores) > mean(&others),
        "almost hers is closer than generic"
    );

    // What "לקרב לסגנון שלי" asks for points the right way: the generic paragraph is told to
    // shorten its sentences, and a paragraph that does so scores higher.
    let far = generic(&mut rng);
    let ask = model.closer_ask("observation", &far).unwrap();
    println!("asked: {ask}");
    assert!(ask.contains("משפטים קצרים יותר"), "{ask}");
    let closer = hers(&mut rng);
    assert!(model.fidelity("observation", &closer) > model.fidelity("observation", &far));
}
