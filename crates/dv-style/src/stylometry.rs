//! A statistical model of how Einat writes, per report section.
//!
//! Each paragraph becomes a short vector of features that can be explained in words. Their
//! mean and variance are learned with Welford's running algorithm, so only numbers are kept,
//! never the paragraphs. A section with few paragraphs leans on her whole-report figures
//! (empirical-Bayes shrinkage). The function-word profile is compared with Burrows' Delta, the
//! classic measure of authorship.
//!
//! Three sources feed the model separately and are merged when it is used: her past reports
//! (re-measured whenever they change), her final wording of the paragraphs she edits
//! (learned one edit at a time, half weight, because part of it may still be the AI's), and
//! the reports she finished in the program (full weight, one entry per case, replaced when
//! the case is exported again).

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
/// ...and worth a line under the closeness figure, or a request to the AI, from this many.
const Z_HINT: f64 = 1.0;
/// Weight of her final wording of an edited paragraph.
pub const EDIT_WEIGHT: f64 = 0.5;
/// Gradual forgetting (ST5). Every new edit leaves the earlier ones at this share (half after
/// about 140 edits); every newly finished report leaves the earlier ones at this share (half
/// after about 7 reports).
const EDIT_FADE: f64 = 0.995;
const FINAL_FADE: f64 = 0.9;
/// A past report counts this much less for each report she uploaded after it, and never less
/// than [`REPORT_FLOOR`].
const REPORT_FADE: f64 = 0.9;
const REPORT_FLOOR: f64 = 0.4;

/// The weight of a past report with `newer` of her reports uploaded after it (ST5).
#[must_use]
pub fn report_weight(newer: usize) -> f64 {
    let newer = i32::try_from(newer).unwrap_or(i32::MAX);
    REPORT_FADE.powi(newer).max(REPORT_FLOOR)
}

struct Feature {
    /// Smallest standard deviation used (a feature she never varies would flag everything).
    floor: f64,
    /// (more than usual, less than usual); `{x}` is her usual figure, `{v}` this paragraph's.
    more: &'static str,
    less: Option<&'static str>,
    /// What to ask the AI for when it is (more, less) than usual; `{x}` is her usual figure.
    ask_more: Option<&'static str>,
    ask_less: Option<&'static str>,
}

const FEATURES: [Feature; N] = [
    Feature {
        floor: 2.0,
        more: "משפטים ארוכים מהרגיל אצלך (בממוצע {v} מילים במשפט; אצלך בדרך כלל כ-{x})",
        less: Some("משפטים קצרים מהרגיל אצלך (בממוצע {v} מילים במשפט; אצלך בדרך כלל כ-{x})"),
        ask_more: Some("משפטים קצרים יותר, כ-{x} מילים במשפט"),
        ask_less: Some("משפטים מעט ארוכים יותר, כ-{x} מילים במשפט"),
    },
    Feature {
        floor: 1.5,
        more: "אורך המשפטים משתנה יותר מהרגיל אצלך",
        less: None,
        ask_more: Some("משפטים באורך אחיד יותר"),
        ask_less: None,
    },
    Feature {
        floor: 0.8,
        more: "יותר משפטים בפסקה מהרגיל אצלך ({v}; אצלך בדרך כלל כ-{x})",
        less: Some("פחות משפטים בפסקה מהרגיל אצלך ({v}; אצלך בדרך כלל כ-{x})"),
        ask_more: Some("פחות משפטים בפסקה, כ-{x}"),
        ask_less: Some("יותר משפטים בפסקה, כ-{x}"),
    },
    Feature {
        floor: 0.25,
        more: "ניסוח כבד מהרגיל אצלך (מילים ארוכות)",
        less: None,
        ask_more: Some("מילים פשוטות וקצרות יותר"),
        ask_less: None,
    },
    Feature {
        floor: 0.4,
        more: "יותר פסיקים מהרגיל אצלך",
        less: None,
        ask_more: Some("פחות פסיקים ומשפטים פשוטים יותר"),
        ask_less: None,
    },
    Feature {
        floor: 1.0,
        more: "יותר סוגריים מהרגיל אצלך",
        less: None,
        ask_more: Some("בלי סוגריים, או מעט מאוד"),
        ask_less: None,
    },
    Feature {
        floor: 0.15,
        more: "יותר מילות ניגוד («אך», «עם זאת») מהרגיל אצלך",
        less: None,
        ask_more: Some("פחות מילות ניגוד כמו «אך» ו«עם זאת»"),
        ask_less: None,
    },
    Feature {
        floor: 0.15,
        more: "יותר הסתייגויות («נראה», «ניכר») מהרגיל אצלך",
        less: Some("פחות הסתייגויות («נראה», «ניכר») מהרגיל אצלך"),
        ask_more: Some("פחות הסתייגויות כמו «נראה» ו«ניכר»"),
        ask_less: Some("יותר ניסוח זהיר, כמו «נראה» ו«ניכר»"),
    },
    Feature {
        floor: 0.12,
        more: "יותר גוף ראשון («התרשמתי») מהרגיל אצלך",
        less: Some("פחות גוף ראשון («התרשמתי») מהרגיל אצלך"),
        ask_more: Some("פחות גוף ראשון"),
        ask_less: Some("יותר גוף ראשון, כמו «התרשמתי»"),
    },
    Feature {
        floor: 0.1,
        more: "בחירת המילים הקטנות («של», «כי», «אשר») שונה מהרגיל אצלך",
        less: None,
        ask_more: None,
        ask_less: None,
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

    /// The same mean and spread, worth `f` times as much: older observations fade (ST5).
    pub fn fade(&mut self, f: f64) {
        self.n *= f;
        self.m2 *= f;
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

    fn fade(&mut self, f: f64) {
        for m in self
            .global
            .iter_mut()
            .chain(self.function.iter_mut())
            .chain(self.sections.values_mut().flatten())
        {
            m.fade(f);
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

/// Her style model: what her past reports, her edits and her finished reports taught, kept
/// apart.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StyleModel {
    #[serde(default)]
    pub reports: Part,
    #[serde(default)]
    pub edits: Part,
    /// One entry per finished report, by a keyed hash of its case (numbers only).
    #[serde(default)]
    pub finals: BTreeMap<String, Part>,
}

/// Her usual figures, for the screen that shows what the program learned. `None` for a
/// figure with too little behind it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Measures {
    /// Paragraphs learned from: past reports, her edits (weighted), finished reports.
    pub from_reports: f64,
    pub from_edits: f64,
    pub from_finals: f64,
    pub finished_reports: usize,
    pub words_per_sentence: Option<f64>,
    pub sentences_per_paragraph: Option<f64>,
    pub commas_per_sentence: Option<f64>,
    /// Out of 100 sentences.
    pub hedges: Option<f64>,
    pub contrasts: Option<f64>,
    pub first_person: Option<f64>,
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
        .filter(|p| matches!(p, [a, b] if a.norm.ends_with("עמ") && b.norm == "זאת"))
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

/// Measures every paragraph of `parts` into one source's figures, at full weight. Delta is
/// taken against `against` when given, else against the parts' own function-word profile.
fn fit<'a>(
    parts: impl IntoIterator<Item = (Option<&'a str>, &'a str)>,
    against: Option<&[Moments]>,
) -> Part {
    fit_weighted(parts.into_iter().map(|(s, t)| (s, t, 1.0)), against)
}

/// One measured paragraph: (section, features, function-word rates, weight).
type Measured<'a> = (Option<&'a str>, [f64; N], Vec<f64>, f64);

fn fit_weighted<'a>(
    parts: impl IntoIterator<Item = (Option<&'a str>, &'a str, f64)>,
    against: Option<&[Moments]>,
) -> Part {
    let measured: Vec<Measured<'_>> = parts
        .into_iter()
        .flat_map(|(s, t, w)| paragraphs(t).into_iter().map(move |p| (s, p, w)))
        .filter_map(|(s, p, w)| measure(p).map(|(f, r)| (s, f, r, w)))
        .collect();
    let mut part = Part::default();
    for (s, f, r, w) in &measured {
        add(&mut part, *s, f, r, *w);
    }
    // Second pass: how far each paragraph is from her function-word profile.
    let function = match against {
        Some(f) if f.iter().any(|m| m.n >= 2.0) => f.to_vec(),
        _ => part.function.clone(),
    };
    for (s, _, r, w) in measured {
        if let Some(d) = delta(&r, &function) {
            let key = s.unwrap_or(GENERAL).to_owned();
            // `add` made room for every feature, Delta among them, before this pass.
            if let Some(m) = part.global.get_mut(DELTA) {
                m.add(d, w);
            }
            if let Some(m) = part
                .sections
                .get_mut(&key)
                .and_then(|sec| sec.get_mut(DELTA))
            {
                m.add(d, w);
            }
        }
    }
    part
}

fn add(part: &mut Part, section: Option<&str>, f: &[f64; N], rates: &[f64], w: f64) {
    part.ensure();
    let key = section.unwrap_or(GENERAL).to_owned();
    let sec = part
        .sections
        .entry(key)
        .or_insert_with(|| vec![Moments::default(); N]);
    sec.resize(N, Moments::default());
    // `ensure` and `resize` made both N long, as `f` is.
    for ((g, s), x) in part.global.iter_mut().zip(sec.iter_mut()).zip(f) {
        g.add(*x, w);
        s.add(*x, w);
    }
    for (m, r) in part.function.iter_mut().zip(rates) {
        m.add(*r, w);
    }
}

impl StyleModel {
    /// Re-learn from her past reports: `(section, text)` for every part she kept.
    pub fn fit_reports<'a>(&mut self, parts: impl IntoIterator<Item = (Option<&'a str>, &'a str)>) {
        self.reports = fit(parts, None);
    }

    /// Re-learn from her past reports, each part with the weight of its report
    /// ([`report_weight`]): the newer reports count more, so the model follows her as her
    /// writing changes (ST5).
    pub fn fit_reports_weighted<'a>(
        &mut self,
        parts: impl IntoIterator<Item = (Option<&'a str>, &'a str, f64)>,
    ) {
        self.reports = fit_weighted(parts, None);
    }

    /// Learn from a report she finished in the program, at full weight: `(section, text)` for
    /// every approved paragraph. `key` names the case (a keyed hash); exporting the same case
    /// again replaces what it taught, so a report never counts twice.
    pub fn observe_final<'a>(
        &mut self,
        key: &str,
        parts: impl IntoIterator<Item = (Option<&'a str>, &'a str)>,
    ) {
        self.finals.remove(key);
        // Earlier finished reports fade a little with every new one (ST5).
        for f in self.finals.values_mut() {
            f.fade(FINAL_FADE);
        }
        // Delta against what she wrote elsewhere: the report itself would pull it to zero.
        let function = self.combined().function;
        let part = fit(parts, Some(&function));
        if part.global.first().is_some_and(|m| m.n > 0.0) {
            self.finals.insert(key.to_owned(), part);
        }
    }

    /// The finished report of a case that was erased is no longer learned from.
    pub fn forget_final(&mut self, key: &str) -> bool {
        self.finals.remove(key).is_some()
    }

    /// Everything learned from her edits and finished reports; her past reports stay.
    pub fn forget_learning(&mut self) {
        self.edits = Part::default();
        self.finals.clear();
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
        // What she edited long ago fades, a little with every new edit (ST5).
        self.edits.fade(EDIT_FADE);
        add(&mut self.edits, Some(section), &f, &r, EDIT_WEIGHT);
    }

    fn combined(&self) -> Part {
        let mut p = self.reports.merge(&self.edits);
        for f in self.finals.values() {
            p = p.merge(f);
        }
        p.ensure();
        p
    }

    /// Her usual figures, in words she can check.
    #[must_use]
    pub fn measures(&self) -> Measures {
        let part = self.combined();
        let n = |p: &Part| p.global.first().map_or(0.0, |m| m.n);
        let usual = |i: usize, scale: f64| {
            let m = part.global.get(i)?;
            (m.n >= MIN_PARAGRAPHS).then_some(m.mean * scale)
        };
        Measures {
            from_reports: n(&self.reports),
            from_edits: n(&self.edits),
            from_finals: self.finals.values().map(n).sum(),
            finished_reports: self.finals.len(),
            words_per_sentence: usual(0, 1.0),
            sentences_per_paragraph: usual(2, 1.0),
            commas_per_sentence: usual(4, 1.0),
            hedges: usual(7, 100.0),
            contrasts: usual(6, 100.0),
            first_person: usual(8, 100.0),
        }
    }

    /// How close a paragraph is to her writing, 0–100: for every feature, how many of her
    /// usual spreads away it is (0 at her usual figure, nothing left at twice the distance of
    /// a note), averaged. `None` when there is too little to go on. The same figures as
    /// [`StyleModel::note`], as one number that can follow the drafts over time.
    #[must_use]
    pub fn fidelity(&self, section: &str, text: &str) -> Option<u32> {
        let zs = self.zs(section, text)?;
        if zs.is_empty() {
            return None;
        }
        let close: f64 = zs
            .iter()
            .map(|(_, z, _, _)| (1.0 - z.abs() / (2.0 * Z_NOTE)).max(0.0))
            .sum::<f64>()
            / zs.len() as f64;
        Some((close * 100.0).round() as u32)
    }

    /// For each feature of a paragraph: (feature, z, her usual figure, this paragraph's).
    fn zs(&self, section: &str, text: &str) -> Option<Vec<(&'static Feature, f64, f64, f64)>> {
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
        let mut out = Vec::new();
        // `combined` made the whole report's figures and every section's N long, as `f` is.
        for (i, ((spec, g), x)) in FEATURES.iter().zip(&part.global).zip(f).enumerate() {
            let section_figures = sec.and_then(|s| s.get(i)).filter(|s| s.n > 0.0);
            let (mean, var) = match section_figures {
                // Empirical Bayes: a section's figures, pulled toward the whole report's.
                Some(s) => (
                    (s.n * s.mean + PRIOR * g.mean) / (s.n + PRIOR),
                    (s.n * s.var() + PRIOR * g.var()) / (s.n + PRIOR),
                ),
                None => (g.mean, g.var()),
            };
            let sd = var.sqrt().max(spec.floor);
            if !x.is_finite() || g.n < 2.0 {
                continue;
            }
            out.push((spec, (x - mean) / sd, mean, x));
        }
        Some(out)
    }

    /// How many paragraphs (weighted) the model has learned from.
    #[must_use]
    pub fn paragraphs(&self) -> f64 {
        self.combined().global.first().map_or(0.0, |m| m.n)
    }

    /// Features of a paragraph at least `z_min` of her usual spreads away, furthest first:
    /// (feature, z, her usual figure, this paragraph's).
    fn unusual(
        &self,
        section: &str,
        text: &str,
        z_min: f64,
    ) -> Vec<(&'static Feature, f64, f64, f64)> {
        let mut found: Vec<_> = self
            .zs(section, text)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, z, _, _)| z.abs() >= z_min)
            .collect();
        found.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        found
    }

    /// The features of a paragraph at least `z_min` away, in words (her figures filled in).
    fn described(&self, section: &str, text: &str, z_min: f64) -> Vec<String> {
        self.unusual(section, text, z_min)
            .into_iter()
            .filter_map(|(spec, z, mean, x)| {
                let t = if z > 0.0 { Some(spec.more) } else { spec.less }?;
                Some(
                    t.replace("{v}", &format!("{x:.0}"))
                        .replace("{x}", &format!("{mean:.0}")),
                )
            })
            .collect()
    }

    /// A gentle note on a drafted paragraph in `section` whose writing is unlike hers: the one
    /// or two features furthest from her usual figures, in words. `None` when it reads like her
    /// or when there is too little to go on.
    #[must_use]
    pub fn note(&self, section: &str, text: &str) -> Option<String> {
        let mut found = self.described(section, text, Z_NOTE);
        found.truncate(2);
        (!found.is_empty()).then(|| found.join("; "))
    }

    /// Everything in a paragraph that is noticeably unlike her (one of her usual spreads away),
    /// furthest first, in words: what she reads under the closeness figure.
    #[must_use]
    pub fn differences(&self, section: &str, text: &str) -> Vec<String> {
        self.described(section, text, Z_HINT)
    }

    /// What to ask the AI so the paragraph reads more like her, as one line of requests, or
    /// `None` when nothing is far enough off. Only her usual figures go into it, never text.
    #[must_use]
    pub fn closer_ask(&self, section: &str, text: &str) -> Option<String> {
        let asks: Vec<String> = self
            .unusual(section, text, Z_HINT)
            .into_iter()
            .filter_map(|(spec, z, mean, _)| {
                let t = if z > 0.0 {
                    spec.ask_more
                } else {
                    spec.ask_less
                }?;
                Some(t.replace("{x}", &format!("{mean:.0}")))
            })
            .take(3)
            .collect();
        (!asks.is_empty()).then(|| asks.join("; "))
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

    #[test]
    fn older_writing_fades_but_keeps_its_figures() {
        let mut m = Moments::default();
        for x in [10.0, 12.0, 14.0] {
            m.add(x, 1.0);
        }
        let (mean, var) = (m.mean, m.var());
        m.fade(0.5);
        assert!((m.mean - mean).abs() < 1e-12 && (m.n - 1.5).abs() < 1e-12);
        assert!((m.m2 - var * 2.0 * 0.5).abs() < 1e-9, "the spread is kept");
        assert!((report_weight(0) - 1.0).abs() < 1e-12);
        assert!(report_weight(1) < 1.0 && report_weight(1) > report_weight(3));
        assert!((report_weight(100) - REPORT_FLOOR).abs() < 1e-12);

        // Her newer edits pull her figures further than her older ones.
        let words = |n: usize| vec!["מילה"; n].join(" ");
        let para = |n: usize| format!("{}. {}. {}.", words(n), words(n), words(n));
        let mut m = StyleModel::default();
        for _ in 0..300 {
            m.observe_edit("observation", &para(20));
        }
        for _ in 0..300 {
            m.observe_edit("observation", &para(8));
        }
        let wps = m.edits.global[0].mean;
        assert!(wps < 14.0, "the newer, shorter sentences weigh more: {wps}");
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
    fn im_zot_counts_as_a_contrast_only_as_two_words_in_a_row() {
        let contrast = |text: &str| measure(text).unwrap().0[6];
        let with = contrast("הילד שיחק בחצר עם זאת התעייף מהר מאוד.");
        let apart = contrast("הילד שיחק עם החברים ואת זאת אהב מאוד.");
        assert!(with > apart, "{with} {apart}");
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

    #[test]
    fn her_writing_is_closer_than_a_stranger_s() {
        let m = model();
        let close = m.fidelity("observation", &hers(4)).unwrap();
        let other = "הילד הציג תפקוד שהוא בהחלט תואם את גילו הכרונולוגי ברוב התחומים שנבדקו במהלך האבחון הנוכחי וגם בתחומים נוספים שעלו בשיחה עם ההורים ועם הצוות החינוכי של המסגרת. \
                     בנוסף לכך הוא הפגין יכולת להתמיד במשימות לאורך זמן ממושך יחסית גם כאשר הן היו קשות עבורו ודרשו ממנו מאמץ רב ושימוש באסטרטגיות שונות ומגוונות של פתרון בעיות.";
        let theirs = m.fidelity("observation", other).unwrap();
        assert!(close >= 80, "{close}");
        assert!(theirs + 20 < close, "{theirs} vs {close}");
        assert_eq!(
            StyleModel::default().fidelity("observation", &hers(0)),
            None
        );
    }

    #[test]
    fn a_finished_report_counts_once_and_can_be_forgotten() {
        let mut m = model();
        let base = m.paragraphs();
        let report: Vec<String> = (0..3).map(hers).collect();
        let parts = || report.iter().map(|t| (Some("observation"), t.as_str()));
        m.observe_final("case-a", parts());
        assert!((m.paragraphs() - base - 3.0).abs() < 1e-9);
        // Exported again: replaced, not added.
        m.observe_final("case-a", parts());
        assert!((m.paragraphs() - base - 3.0).abs() < 1e-9);
        assert_eq!(m.measures().finished_reports, 1);
        assert!(m.forget_final("case-a"));
        assert!((m.paragraphs() - base).abs() < 1e-9);
        m.observe_edit("observation", &hers(2));
        m.observe_final("case-b", parts());
        m.forget_learning();
        assert!((m.paragraphs() - base).abs() < 1e-9);
    }

    #[test]
    fn measures_say_nothing_on_too_little() {
        let m = model();
        let usual = m.measures();
        assert!((usual.from_reports - 20.0).abs() < 1e-9);
        assert!(usual
            .words_per_sentence
            .is_some_and(|w| w > 8.0 && w < 20.0));
        assert!(usual.hedges.is_some());
        assert_eq!(StyleModel::default().measures().words_per_sentence, None);
    }

    #[test]
    fn an_older_model_without_finished_reports_still_reads() {
        let old = serde_json::json!({"reports": Part::default(), "edits": Part::default()});
        let m: StyleModel = serde_json::from_value(old).unwrap();
        assert!(m.finals.is_empty());
    }
}
