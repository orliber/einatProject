//! A statistical model of how Einat writes, per report section.
//!
//! Each paragraph becomes a short vector of features that can be explained in words. Their
//! mean and variance are learned with Welford's running algorithm, so only numbers are kept,
//! never the paragraphs. A section with few paragraphs leans on her whole-report figures
//! (empirical-Bayes shrinkage). The function-word profile is compared with Burrows' Delta, the
//! classic measure of authorship.
//!
//! Two sources feed the model separately and are merged when it is used: her past reports
//! (re-measured whenever they change) and her final wording of the paragraphs she edits
//! (learned one edit at a time, half weight, because part of it may still be the AI's).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::text::{has, paragraphs, sentences, words, CONTRAST, FIRST_PERSON, FUNCTION, HEDGE};
use crate::GENERAL;

/// Features of one paragraph. The order is the order of [`FEATURES`].
const N: usize = 10;
const DELTA: usize = 9;

/// Below this many paragraphs her model says nothing.
pub const MIN_PARAGRAPHS: f64 = 12.0;
/// How many paragraphs of her whole-report figures a section's figures are weighed against.
const PRIOR: f64 = 8.0;
/// A feature is worth a note from this many standard deviations away.
const Z_NOTE: f64 = 2.0;
/// Weight of her final wording of an edited paragraph.
pub const EDIT_WEIGHT: f64 = 0.5;

struct Feature {
    /// Smallest standard deviation used (a feature she never varies would flag everything).
    floor: f64,
    /// (more than usual, less than usual); `{x}` is her usual figure, `{v}` this paragraph's.
    more: &'static str,
    less: Option<&'static str>,
}

const FEATURES: [Feature; N] = [
    Feature {
        floor: 2.0,
        more: "משפטים ארוכים מהרגיל אצלך (בממוצע {v} מילים במשפט; אצלך בדרך כלל כ-{x})",
        less: Some("משפטים קצרים מהרגיל אצלך (בממוצע {v} מילים במשפט; אצלך בדרך כלל כ-{x})"),
    },
    Feature {
        floor: 1.5,
        more: "אורך המשפטים משתנה יותר מהרגיל אצלך",
        less: None,
    },
    Feature {
        floor: 0.8,
        more: "יותר משפטים בפסקה מהרגיל אצלך ({v}; אצלך בדרך כלל כ-{x})",
        less: Some("פחות משפטים בפסקה מהרגיל אצלך ({v}; אצלך בדרך כלל כ-{x})"),
    },
    Feature {
        floor: 0.25,
        more: "ניסוח כבד מהרגיל אצלך (מילים ארוכות)",
        less: None,
    },
    Feature {
        floor: 0.4,
        more: "יותר פסיקים מהרגיל אצלך",
        less: None,
    },
    Feature {
        floor: 1.0,
        more: "יותר סוגריים מהרגיל אצלך",
        less: None,
    },
    Feature {
        floor: 0.15,
        more: "יותר מילות ניגוד («אך», «עם זאת») מהרגיל אצלך",
        less: None,
    },
    Feature {
        floor: 0.15,
        more: "יותר הסתייגויות («נראה», «ניכר») מהרגיל אצלך",
        less: Some("פחות הסתייגויות («נראה», «ניכר») מהרגיל אצלך"),
    },
    Feature {
        floor: 0.12,
        more: "יותר גוף ראשון («התרשמתי») מהרגיל אצלך",
        less: Some("פחות גוף ראשון («התרשמתי») מהרגיל אצלך"),
    },
    Feature {
        floor: 0.1,
        more: "בחירת המילים הקטנות («של», «כי», «אשר») שונה מהרגיל אצלך",
        less: None,
    },
];

/// Running mean and variance (Welford), with weights.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Moments {
    pub n: f64,
    pub mean: f64,
    pub m2: f64,
}

impl Moments {
    pub fn add(&mut self, x: f64, w: f64) {
        if w <= 0.0 || !x.is_finite() {
            return;
        }
        self.n += w;
        let d = x - self.mean;
        self.mean += d * w / self.n;
        self.m2 += w * d * (x - self.mean);
    }

    /// Both sets of observations as one (Chan et al.).
    #[must_use]
    pub fn merge(self, o: Moments) -> Moments {
        let n = self.n + o.n;
        if n <= 0.0 {
            return Moments::default();
        }
        let d = o.mean - self.mean;
        Moments {
            n,
            mean: self.mean + d * o.n / n,
            m2: self.m2 + o.m2 + d * d * self.n * o.n / n,
        }
    }

    #[must_use]
    pub fn var(&self) -> f64 {
        if self.n > 1.0 {
            self.m2 / (self.n - 1.0)
        } else {
            0.0
        }
    }
}

/// One source's figures: whole report, by section, and the function-word rates.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Part {
    pub global: Vec<Moments>,
    pub sections: BTreeMap<String, Vec<Moments>>,
    /// Rate per 100 words of each of [`crate::text::FUNCTION_WORDS`].
    pub function: Vec<Moments>,
}

impl Part {
    fn ensure(&mut self) {
        self.global.resize(N, Moments::default());
        self.function.resize(FUNCTION.len(), Moments::default());
        for v in self.sections.values_mut() {
            v.resize(N, Moments::default());
        }
    }

    fn merge(&self, o: &Part) -> Part {
        let mut a = self.clone();
        let mut b = o.clone();
        a.ensure();
        b.ensure();
        let zip = |x: &[Moments], y: &[Moments]| -> Vec<Moments> {
            x.iter().zip(y).map(|(p, q)| p.merge(*q)).collect()
        };
        let mut sections = a.sections.clone();
        for (k, v) in &b.sections {
            let merged = match sections.get(k) {
                Some(mine) => zip(mine, v),
                None => v.clone(),
            };
            sections.insert(k.clone(), merged);
        }
        Part {
            global: zip(&a.global, &b.global),
            sections,
            function: zip(&a.function, &b.function),
        }
    }
}

/// Her style model: what her past reports and her edits taught, kept apart.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StyleModel {
    #[serde(default)]
    pub reports: Part,
    #[serde(default)]
    pub edits: Part,
}

/// The raw features of a paragraph, without Delta (which needs her figures).
fn measure(text: &str) -> Option<([f64; N], Vec<f64>)> {
    let sents: Vec<Vec<crate::text::Word>> = sentences(text)
        .into_iter()
        .map(words)
        .filter(|w| w.len() >= 3)
        .collect();
    if sents.is_empty() {
        return None;
    }
    let all: Vec<&crate::text::Word> = sents.iter().flatten().collect();
    let n_words = all.len() as f64;
    let n_sents = sents.len() as f64;
    let lens: Vec<f64> = sents.iter().map(|s| s.len() as f64).collect();
    let mean_len = n_words / n_sents;
    let sd_len = (lens.iter().map(|l| (l - mean_len).powi(2)).sum::<f64>() / n_sents).sqrt();
    let letters: usize = all
        .iter()
        .filter(|w| !w.is_masked())
        .map(|w| w.norm.chars().count())
        .sum();
    let plain = all.iter().filter(|w| !w.is_masked()).count().max(1) as f64;
    let count = |list| all.iter().filter(|w| has(list, &w.norm)).count() as f64;
    // "עם זאת": contrast in two words.
    let im_zot = all
        .windows(2)
        .filter(|p| p[0].norm.ends_with("עמ") && p[1].norm == "זאת")
        .count() as f64;
    let commas = text.matches(',').count() as f64;
    let parens = text.matches('(').count() as f64;
    let mut f = [0.0; N];
    f[0] = mean_len;
    f[1] = sd_len;
    f[2] = n_sents;
    f[3] = letters as f64 / plain;
    f[4] = commas / n_sents;
    f[5] = parens * 100.0 / n_words;
    f[6] = (count(&CONTRAST) + im_zot) / n_sents;
    f[7] = count(&HEDGE) / n_sents;
    f[8] = count(&FIRST_PERSON) / n_sents;
    // Filled in against her figures; until then it is not counted (`Moments::add` skips NaN).
    f[DELTA] = f64::NAN;
    let rates = FUNCTION
        .iter()
        .map(|fw| all.iter().filter(|w| &w.norm == fw).count() as f64 * 100.0 / n_words)
        .collect();
    Some((f, rates))
}

/// Burrows' Delta of one paragraph's function-word rates against hers.
fn delta(rates: &[f64], function: &[Moments]) -> Option<f64> {
    let mut sum = 0.0;
    let mut k = 0.0;
    for (r, m) in rates.iter().zip(function) {
        if m.n < 2.0 {
            continue;
        }
        // A word she uses at a steady rate still varies by half a use per 100 words.
        let sd = m.var().sqrt().max(0.5);
        sum += ((r - m.mean) / sd).abs();
        k += 1.0;
    }
    (k > 0.0).then(|| sum / k)
}

fn add(part: &mut Part, section: Option<&str>, f: &[f64; N], rates: &[f64], w: f64) {
    part.ensure();
    let key = section.unwrap_or(GENERAL).to_owned();
    let sec = part
        .sections
        .entry(key)
        .or_insert_with(|| vec![Moments::default(); N]);
    sec.resize(N, Moments::default());
    for (i, x) in f.iter().enumerate() {
        part.global[i].add(*x, w);
        sec[i].add(*x, w);
    }
    for (m, r) in part.function.iter_mut().zip(rates) {
        m.add(*r, w);
    }
}

impl StyleModel {
    /// Re-learn from her past reports: `(section, text)` for every part she kept.
    pub fn fit_reports<'a>(&mut self, parts: impl IntoIterator<Item = (Option<&'a str>, &'a str)>) {
        let measured: Vec<(Option<&str>, [f64; N], Vec<f64>)> = parts
            .into_iter()
            .flat_map(|(s, t)| paragraphs(t).into_iter().map(move |p| (s, p)))
            .filter_map(|(s, p)| measure(p).map(|(f, r)| (s, f, r)))
            .collect();
        let mut part = Part::default();
        for (s, f, r) in &measured {
            add(&mut part, *s, f, r, 1.0);
        }
        // Second pass: how far her own paragraphs are from her function-word profile.
        let function = part.function.clone();
        for (s, _, r) in measured {
            if let Some(d) = delta(&r, &function) {
                let key = s.unwrap_or(GENERAL).to_owned();
                part.global[DELTA].add(d, 1.0);
                if let Some(sec) = part.sections.get_mut(&key) {
                    sec[DELTA].add(d, 1.0);
                }
            }
        }
        self.reports = part;
    }

    /// Learn from her final wording of a paragraph in `section`.
    pub fn observe_edit(&mut self, section: &str, text: &str) {
        let combined = self.combined();
        let Some((mut f, r)) = measure(text) else {
            return;
        };
        if let Some(d) = delta(&r, &combined.function) {
            f[DELTA] = d;
        }
        add(&mut self.edits, Some(section), &f, &r, EDIT_WEIGHT);
    }

    fn combined(&self) -> Part {
        let mut p = self.reports.merge(&self.edits);
        p.ensure();
        p
    }

    /// How many paragraphs (weighted) the model has learned from.
    #[must_use]
    pub fn paragraphs(&self) -> f64 {
        self.combined().global.first().map_or(0.0, |m| m.n)
    }

    /// A gentle note on a drafted paragraph in `section` whose writing is unlike hers: the one
    /// or two features furthest from her usual figures, in words. `None` when it reads like her
    /// or when there is too little to go on.
    #[must_use]
    pub fn note(&self, section: &str, text: &str) -> Option<String> {
        let part = self.combined();
        if part.global.first().map_or(0.0, |m| m.n) < MIN_PARAGRAPHS {
            return None;
        }
        let (mut f, rates) = measure(text)?;
        if f[2] < 2.0 {
            return None;
        }
        f[DELTA] = delta(&rates, &part.function).unwrap_or(f64::NAN);
        let sec = part.sections.get(section);
        let mut found: Vec<(f64, String)> = Vec::new();
        for (i, spec) in FEATURES.iter().enumerate() {
            let g = part.global[i];
            let (mean, var) = match sec.map(|s| s[i]).filter(|s| s.n > 0.0) {
                // Empirical Bayes: a section's figures, pulled toward the whole report's.
                Some(s) => (
                    (s.n * s.mean + PRIOR * g.mean) / (s.n + PRIOR),
                    (s.n * s.var() + PRIOR * g.var()) / (s.n + PRIOR),
                ),
                None => (g.mean, g.var()),
            };
            let sd = var.sqrt().max(spec.floor);
            let x = f[i];
            if !x.is_finite() || g.n < 2.0 {
                continue;
            }
            let z = (x - mean) / sd;
            let text = if z >= Z_NOTE {
                Some(spec.more)
            } else if z <= -Z_NOTE {
                spec.less
            } else {
                None
            };
            if let Some(t) = text {
                found.push((
                    z.abs(),
                    t.replace("{v}", &format!("{x:.0}"))
                        .replace("{x}", &format!("{mean:.0}")),
                ));
            }
        }
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        found.truncate(2);
        (!found.is_empty()).then(|| {
            found
                .into_iter()
                .map(|(_, t)| t)
                .collect::<Vec<_>>()
                .join("; ")
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// An invented paragraph in a steady, measured voice: medium sentences, some hedging.
    fn hers(i: usize) -> String {
        let topics = ["המשחק", "השפה", "הקשב", "הציור", "התנועה", "החברה"];
        let t = topics[i % topics.len()];
        format!(
            "בתחום {t} נראה כי הילד פועל בהתאם לגילו, עם זאת ניכר קושי קל במעברים. \
             במהלך המפגש התרשמתי מילד סקרן ומשתף פעולה, אשר נעזר בתיווך מבוגר. \
             כאשר המשימה התארכה הוא ביקש הפסקה קצרה וחזר לעבודה."
        )
    }

    fn model() -> StyleModel {
        let texts: Vec<String> = (0..20).map(hers).collect();
        let mut m = StyleModel::default();
        m.fit_reports(texts.iter().map(|t| (Some("observation"), t.as_str())));
        m
    }

    #[test]
    fn welford_matches_the_direct_figures() {
        let xs = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let mut m = Moments::default();
        xs.iter().for_each(|x| m.add(*x, 1.0));
        assert!((m.mean - 5.0).abs() < 1e-9);
        assert!((m.var() - 32.0 / 7.0).abs() < 1e-9);
        let (mut a, mut b) = (Moments::default(), Moments::default());
        xs[..3].iter().for_each(|x| a.add(*x, 1.0));
        xs[3..].iter().for_each(|x| b.add(*x, 1.0));
        let c = a.merge(b);
        assert!((c.mean - m.mean).abs() < 1e-9 && (c.m2 - m.m2).abs() < 1e-9);
    }

    #[test]
    fn delta_slot_holds_only_real_values() {
        let m = model();
        let d = m.reports.global[DELTA];
        assert!((d.n - 20.0).abs() < 1e-9, "{d:?}");
        assert!(d.mean >= 0.0 && d.mean.is_finite());
    }

    #[test]
    fn her_own_writing_gets_no_note() {
        assert_eq!(model().note("observation", &hers(3)), None);
    }

    #[test]
    fn long_winding_sentences_get_a_note_in_words() {
        let long = "הילד הציג תפקוד שהוא בהחלט תואם את גילו הכרונולוגי ברוב התחומים שנבדקו במהלך האבחון הנוכחי וגם בתחומים נוספים שעלו בשיחה עם ההורים ועם הצוות החינוכי של המסגרת בה הוא לומד כעת ובמסגרות קודמות שבהן שהה בעבר במשך תקופה ארוכה למדי. \
                    בנוסף לכך הוא הפגין יכולת להתמיד במשימות לאורך זמן ממושך יחסית גם כאשר הן היו קשות עבורו ודרשו ממנו מאמץ רב ושימוש באסטרטגיות שונות ומגוונות של פתרון בעיות ושל ויסות רגשי עצמי ושל פנייה לעזרה מן המבוגר הנוכח בחדר.";
        let note = model().note("observation", long).unwrap();
        assert!(note.contains("משפטים ארוכים מהרגיל"), "{note}");
    }

    #[test]
    fn too_little_to_go_on_says_nothing() {
        let mut m = StyleModel::default();
        m.fit_reports([(Some("observation"), hers(1).as_str())]);
        assert_eq!(
            m.note("observation", "משפט אחד קצר מאוד כאן. ועוד אחד כזה בדיוק."),
            None
        );
    }

    #[test]
    fn her_edits_add_to_the_model_at_half_weight() {
        let mut m = model();
        let before = m.paragraphs();
        m.observe_edit("observation", &hers(2));
        assert!((m.paragraphs() - before - EDIT_WEIGHT).abs() < 1e-9);
    }
}
