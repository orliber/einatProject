//! Score entry and interpretation: deterministic tables, never the model.
//!
//! The psychologist enters scores; the range ("ממוצע גבוה") comes from a fixed table here, and
//! Claude receives the finished interpretation. The tables are a starting point marked for the
//! psychologist's approval (knowledge/instruments.md); norms themselves stay in the manuals.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How a measure's numbers are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScoreScale {
    /// Composite / index score: mean 100, SD 15 (FSIQ, VCI, GAC, Bayley composites).
    Standard,
    /// Subtest scaled score: mean 10, SD 3.
    Scaled,
    /// Questionnaire T score, higher = more concern, instrument-specific cut-offs.
    TConcern,
    /// A raw total compared with a cut-off the psychologist enters (ADOS-2).
    RawCutoff,
    /// CARS-2 standard-version total.
    Cars,
    /// ADOS-2 comparison score (1–10).
    Comparison,
}

/// One band of a table: `min..=max` gets `label`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Band {
    pub min: f64,
    pub max: f64,
    pub label: String,
    /// 0 = typical, 1 = mild, 2 = marked, 3 = severe (for colour in the UI only).
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Measure {
    pub key: String,
    pub name_he: String,
    /// Abbreviation as used in reports ("VCI", "GAC").
    pub abbr: String,
    pub group: String,
    pub scale: ScoreScale,
    pub min: f64,
    pub max: f64,
    /// The table used for this measure.
    pub bands: Vec<Band>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Instrument {
    pub key: String,
    pub name: String,
    pub description_he: String,
    /// The sentence that explains the scale in the report ("ציון תקן: ממוצע 100…").
    pub scale_note_he: String,
    /// The ages the instrument is normed for, in months (the UI warns outside them).
    pub min_age_months: u32,
    pub max_age_months: u32,
    pub measures: Vec<Measure>,
}

fn band(min: f64, max: f64, label: &str, level: u8) -> Band {
    Band {
        min,
        max,
        label: label.to_owned(),
        level,
    }
}

/// Wechsler-style descriptors for composites (mean 100, SD 15).
fn standard_bands() -> Vec<Band> {
    vec![
        band(130.0, 160.0, "גבוה מאוד", 0),
        band(120.0, 129.0, "גבוה", 0),
        band(110.0, 119.0, "ממוצע גבוה", 0),
        band(90.0, 109.0, "ממוצע", 0),
        band(80.0, 89.0, "ממוצע נמוך", 1),
        band(70.0, 79.0, "גבולי", 2),
        band(40.0, 69.0, "נמוך מאוד", 3),
    ]
}

/// Wechsler-style descriptors for subtests (mean 10, SD 3).
fn scaled_bands() -> Vec<Band> {
    vec![
        band(16.0, 19.0, "גבוה מאוד", 0),
        band(13.0, 15.0, "ממוצע גבוה", 0),
        band(8.0, 12.0, "ממוצע", 0),
        band(6.0, 7.0, "ממוצע נמוך", 1),
        band(4.0, 5.0, "נמוך", 2),
        band(1.0, 3.0, "נמוך מאוד", 3),
    ]
}

/// ASRS and similar rating scales: T ≥ 60 raises concern.
fn asrs_bands() -> Vec<Band> {
    vec![
        band(20.0, 59.0, "בטווח התקין", 0),
        band(60.0, 64.0, "מעט מעל הטווח התקין", 1),
        band(65.0, 69.0, "מוגבר", 2),
        band(70.0, 100.0, "מוגבר מאוד", 3),
    ]
}

/// Achenbach syndrome scales.
fn cbcl_syndrome_bands() -> Vec<Band> {
    vec![
        band(50.0, 64.0, "בטווח התקין", 0),
        band(65.0, 69.0, "בטווח הגבולי", 1),
        band(70.0, 100.0, "בטווח הקליני", 3),
    ]
}

/// Achenbach broad-band scales (internalizing, externalizing, total).
fn cbcl_broad_bands() -> Vec<Band> {
    vec![
        band(20.0, 59.0, "בטווח התקין", 0),
        band(60.0, 63.0, "בטווח הגבולי", 1),
        band(64.0, 100.0, "בטווח הקליני", 3),
    ]
}

fn cars_bands() -> Vec<Band> {
    vec![
        band(15.0, 29.5, "מעט או ללא תסמינים של הספקטרום", 0),
        band(30.0, 36.5, "תסמינים בדרגה קלה-בינונית", 2),
        band(37.0, 60.0, "תסמינים בדרגה חמורה", 3),
    ]
}

/// ADOS-2 comparison score: level of autism-spectrum-related symptoms.
fn comparison_bands() -> Vec<Band> {
    vec![
        band(1.0, 2.0, "רמה מינימלית עד היעדר עדות לתסמינים", 0),
        band(3.0, 4.0, "רמה נמוכה של תסמינים", 1),
        band(5.0, 7.0, "רמה בינונית של תסמינים", 2),
        band(8.0, 10.0, "רמה גבוהה של תסמינים", 3),
    ]
}

fn m(key: &str, name_he: &str, abbr: &str, group: &str, scale: ScoreScale) -> Measure {
    let (min, max, bands) = match scale {
        ScoreScale::Standard => (40.0, 160.0, standard_bands()),
        ScoreScale::Scaled => (1.0, 19.0, scaled_bands()),
        ScoreScale::TConcern => (20.0, 100.0, asrs_bands()),
        ScoreScale::RawCutoff => (0.0, 30.0, Vec::new()),
        ScoreScale::Cars => (15.0, 60.0, cars_bands()),
        ScoreScale::Comparison => (1.0, 10.0, comparison_bands()),
    };
    Measure {
        key: key.to_owned(),
        name_he: name_he.to_owned(),
        abbr: abbr.to_owned(),
        group: group.to_owned(),
        scale,
        min,
        max,
        bands,
    }
}

const STANDARD_NOTE: &str = "ציוני המדדים הם ציוני תקן (ממוצע 100, סטיית תקן 15); ציוני תת-המבחנים הם ציונים מותאמים (ממוצע 10, סטיית תקן 3).";

/// The instruments the psychologist uses (docs/REPORT_STRUCTURE.md).
#[must_use]
pub fn instruments() -> Vec<Instrument> {
    use ScoreScale::{Cars, Comparison, RawCutoff, Scaled, Standard, TConcern};
    let mut cbcl: Vec<Measure> = [
        ("emotionally_reactive", "תגובתיות רגשית"),
        ("anxious_depressed", "חרדה/דיכאון"),
        ("somatic", "תלונות סומטיות"),
        ("withdrawn", "הסתגרות"),
        ("sleep", "בעיות שינה"),
        ("attention", "בעיות קשב"),
        ("aggressive", "התנהגות תוקפנית"),
    ]
    .iter()
    .map(|(k, n)| {
        let mut x = m(k, n, "", "סולמות תסמונת", TConcern);
        x.bands = cbcl_syndrome_bands();
        x
    })
    .collect();
    for (k, n) in [
        ("internalizing", "מופנם (Internalizing)"),
        ("externalizing", "מוחצן (Externalizing)"),
        ("total", "ציון כולל"),
    ] {
        let mut x = m(k, n, "", "סולמות רחבים", TConcern);
        x.bands = cbcl_broad_bands();
        cbcl.push(x);
    }

    vec![
        Instrument {
            key: "wppsi_iv".into(),
            name: "WPPSI-IV".into(),
            description_he: "מבחן וקסלר לגיל הרך (2:6–7:7)".into(),
            scale_note_he: STANDARD_NOTE.into(),
            min_age_months: 30,
            max_age_months: 91,
            measures: vec![
                m("fsiq", "מנת משכל כללית", "FSIQ", "מדדים", Standard),
                m("vci", "הבנה מילולית", "VCI", "מדדים", Standard),
                m("vsi", "חזותי-מרחבי", "VSI", "מדדים", Standard),
                m("fri", "חשיבה פלואידית", "FRI", "מדדים", Standard),
                m("wmi", "זיכרון עבודה", "WMI", "מדדים", Standard),
                m("psi", "מהירות עיבוד", "PSI", "מדדים", Standard),
                m("information", "מידע", "IN", "תת-מבחנים", Scaled),
                m("similarities", "שיתופיות", "SI", "תת-מבחנים", Scaled),
                m("vocabulary", "אוצר מילים", "VC", "תת-מבחנים", Scaled),
                m("comprehension", "הבנה", "CO", "תת-מבחנים", Scaled),
                m("block_design", "קוביות", "BD", "תת-מבחנים", Scaled),
                m("object_assembly", "הרכבת עצמים", "OA", "תת-מבחנים", Scaled),
                m("matrix", "מטריצות", "MR", "תת-מבחנים", Scaled),
                m("picture_concepts", "מושגים בתמונות", "PC", "תת-מבחנים", Scaled),
                m("picture_memory", "זיכרון תמונות", "PM", "תת-מבחנים", Scaled),
                m("zoo_locations", "מיקומים בגן החיות", "ZL", "תת-מבחנים", Scaled),
                m("bug_search", "חיפוש חרקים", "BS", "תת-מבחנים", Scaled),
                m("cancellation", "מחיקה", "CA", "תת-מבחנים", Scaled),
            ],
        },
        Instrument {
            key: "wisc_v".into(),
            name: "WISC-V".into(),
            description_he: "מבחן וקסלר לילדים (6:0–16:11)".into(),
            scale_note_he: STANDARD_NOTE.into(),
            min_age_months: 72,
            max_age_months: 203,
            measures: vec![
                m("fsiq", "מנת משכל כללית", "FSIQ", "מדדים", Standard),
                m("vci", "הבנה מילולית", "VCI", "מדדים", Standard),
                m("vsi", "חזותי-מרחבי", "VSI", "מדדים", Standard),
                m("fri", "חשיבה פלואידית", "FRI", "מדדים", Standard),
                m("wmi", "זיכרון עבודה", "WMI", "מדדים", Standard),
                m("psi", "מהירות עיבוד", "PSI", "מדדים", Standard),
                m("similarities", "שיתופיות", "SI", "תת-מבחנים", Scaled),
                m("vocabulary", "אוצר מילים", "VC", "תת-מבחנים", Scaled),
                m("information", "מידע", "IN", "תת-מבחנים", Scaled),
                m("comprehension", "הבנה", "CO", "תת-מבחנים", Scaled),
                m("block_design", "קוביות", "BD", "תת-מבחנים", Scaled),
                m("visual_puzzles", "פאזלים חזותיים", "VP", "תת-מבחנים", Scaled),
                m("matrix", "מטריצות", "MR", "תת-מבחנים", Scaled),
                m("figure_weights", "משקלות", "FW", "תת-מבחנים", Scaled),
                m("picture_concepts", "מושגים בתמונות", "PC", "תת-מבחנים", Scaled),
                m("arithmetic", "חשבון", "AR", "תת-מבחנים", Scaled),
                m("digit_span", "זכירת ספרות", "DS", "תת-מבחנים", Scaled),
                m("picture_span", "זכירת תמונות", "PS", "תת-מבחנים", Scaled),
                m("letter_number", "רצף אותיות ומספרים", "LN", "תת-מבחנים", Scaled),
                m("coding", "צפנים", "CD", "תת-מבחנים", Scaled),
                m("symbol_search", "חיפוש סמלים", "SS", "תת-מבחנים", Scaled),
                m("cancellation", "מחיקה", "CA", "תת-מבחנים", Scaled),
            ],
        },
        Instrument {
            key: "bayley_4".into(),
            name: "Bayley-4".into(),
            description_he: "סולמות ביילי להתפתחות תינוקות ופעוטות (עד 42 חודשים)".into(),
            scale_note_he: STANDARD_NOTE.into(),
            min_age_months: 0,
            max_age_months: 42,
            measures: vec![
                m("cognitive", "קוגניטיבי", "COG", "מדדים", Standard),
                m("language", "שפה", "LANG", "מדדים", Standard),
                m("motor", "מוטורי", "MOT", "מדדים", Standard),
                m("receptive", "שפה קולטת", "RC", "תת-מבחנים", Scaled),
                m("expressive", "שפה מבטאת", "EC", "תת-מבחנים", Scaled),
                m("fine_motor", "מוטוריקה עדינה", "FM", "תת-מבחנים", Scaled),
                m("gross_motor", "מוטוריקה גסה", "GM", "תת-מבחנים", Scaled),
            ],
        },
        Instrument {
            key: "abas_3".into(),
            name: "ABAS-3".into(),
            description_he: "סולם התנהגות מסתגלת (דיווח הורים או צוות המסגרת)".into(),
            scale_note_he: "הציון המסתגל הכללי והאשכולות הם ציוני תקן (ממוצע 100, סטיית תקן 15); תחומי המיומנות הם ציונים מותאמים (ממוצע 10, סטיית תקן 3).".into(),
            min_age_months: 0,
            max_age_months: 1079,
            measures: vec![
                m("gac", "ציון מסתגל כללי", "GAC", "מדדים", Standard),
                m("conceptual", "מושגי", "CON", "מדדים", Standard),
                m("social", "חברתי", "SOC", "מדדים", Standard),
                m("practical", "מעשי", "PRA", "מדדים", Standard),
                m("communication", "תקשורת", "COM", "תחומי מיומנות", Scaled),
                m("functional_pre_academics", "מושגים טרום-אקדמיים", "FA", "תחומי מיומנות", Scaled),
                m("self_direction", "הכוונה עצמית", "SD", "תחומי מיומנות", Scaled),
                m("leisure", "פנאי", "LS", "תחומי מיומנות", Scaled),
                m("social_skill", "חברתי", "SO", "תחומי מיומנות", Scaled),
                m("community_use", "שימוש בקהילה", "CU", "תחומי מיומנות", Scaled),
                m("home_living", "חיים בבית / במסגרת", "HL", "תחומי מיומנות", Scaled),
                m("health_safety", "בריאות ובטיחות", "HS", "תחומי מיומנות", Scaled),
                m("self_care", "טיפול עצמי", "SC", "תחומי מיומנות", Scaled),
                m("motor", "מוטורי", "MO", "תחומי מיומנות", Scaled),
            ],
        },
        Instrument {
            key: "asrs".into(),
            name: "ASRS".into(),
            description_he: "סולם דירוג לספקטרום האוטיסטי (הורים / גננת)".into(),
            scale_note_he: "ציוני T (ממוצע 50, סטיית תקן 10). ציון T של 60 ומעלה מעלה חשד לקשיים בתחום.".into(),
            min_age_months: 24,
            max_age_months: 227,
            measures: vec![
                m("total", "ציון כולל", "T", "סולמות", TConcern),
                m("social_communication", "תקשורת חברתית", "SC", "סולמות", TConcern),
                m("unusual_behaviors", "התנהגויות חריגות", "UB", "סולמות", TConcern),
                m("dsm_scale", "סולם DSM-5", "DSM", "סולמות", TConcern),
            ],
        },
        Instrument {
            key: "cbcl_1_5".into(),
            name: "CBCL 1½–5 (אכנבך)".into(),
            description_he: "רשימת התנהגויות לילד, דיווח הורים".into(),
            scale_note_he: "ציוני T. בסולמות התסמונת: 65–69 גבולי, 70 ומעלה קליני. בסולמות הרחבים: 60–63 גבולי, 64 ומעלה קליני.".into(),
            min_age_months: 18,
            max_age_months: 71,
            measures: cbcl,
        },
        Instrument {
            key: "ados_2".into(),
            name: "ADOS-2".into(),
            description_he: "תצפית אבחנתית לספקטרום האוטיסטי; נקודת החתך לפי המודול".into(),
            scale_note_he: "הציון מושווה לנקודת החתך של המודול שהועבר.".into(),
            min_age_months: 12,
            max_age_months: 1200,
            measures: vec![
                m("social_affect", "אפקט חברתי", "SA", "ציונים", RawCutoff),
                m("rrb", "התנהגויות מוגבלות וחזרתיות", "RRB", "ציונים", RawCutoff),
                m("total", "ציון כולל", "Total", "ציונים", RawCutoff),
                m("comparison", "ציון השוואה", "CSS", "ציונים", Comparison),
            ],
        },
        Instrument {
            key: "cars_2".into(),
            name: "CARS-2".into(),
            description_he: "סולם דירוג לאוטיזם בילדות (גרסה סטנדרטית)".into(),
            scale_note_he: "ציון כולל: מתחת ל-30 מעט או ללא תסמינים, 30 עד 36.5 קל-בינוני, 37 ומעלה חמור.".into(),
            min_age_months: 24,
            max_age_months: 1200,
            measures: vec![m("total", "ציון כולל", "Total", "ציונים", Cars)],
        },
    ]
}

/// The band a value falls in; `None` for a value outside the scale or between bands.
#[must_use]
pub fn classify(measure: &Measure, value: f64) -> Option<&Band> {
    if value < measure.min || value > measure.max {
        return None;
    }
    measure
        .bands
        .iter()
        .find(|b| value >= b.min && value <= b.max + f64::EPSILON)
}

/// Standard normal cumulative distribution (Abramowitz–Stegun 7.1.26, error < 1.5e-7).
fn normal_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    if z >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

/// Percentile rank of a standard or scaled score on the normal curve, formatted the way the
/// Wechsler manuals print it ("79", "0.4", ">99.9"). `None` for other scales.
#[must_use]
pub fn percentile(measure: &Measure, value: f64) -> Option<String> {
    let z = match measure.scale {
        ScoreScale::Standard => (value - 100.0) / 15.0,
        ScoreScale::Scaled => (value - 10.0) / 3.0,
        _ => return None,
    };
    let p = normal_cdf(z) * 100.0;
    Some(if p < 0.1 {
        "<0.1".to_owned()
    } else if p > 99.9 {
        ">99.9".to_owned()
    } else if !(1.0..=99.0).contains(&p) {
        fmt_value((p * 10.0).round() / 10.0)
    } else {
        format!("{:.0}", p.round())
    })
}

/// One entered score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScoreEntry {
    pub measure: String,
    pub value: f64,
    pub note: String,
}

/// What the psychologist entered for one instrument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScoreSheet {
    pub instrument: String,
    /// ADOS-2 module ("1", "2", "T").
    pub module: String,
    /// ADOS-2: the module's cut-off for the total (from the protocol).
    pub cutoff: Option<f64>,
    pub entries: Vec<ScoreEntry>,
    pub notes: String,
}

fn fmt_value(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// The text stored as a "test scores" material: every score with its range from the table,
/// the scale sentence, and a note on large gaps between indexes (to check in the manual).
pub fn format_sheet(sheet: &ScoreSheet) -> Result<String, String> {
    let all = instruments();
    let inst = all
        .iter()
        .find(|i| i.key == sheet.instrument)
        .ok_or("כלי לא מוכר")?;
    let mut out = format!("{} – {}\n", inst.name, inst.description_he);
    if !sheet.module.trim().is_empty() {
        out.push_str(&format!("מודול: {}\n", sheet.module.trim()));
    }
    let mut group = String::new();
    let mut indexes: Vec<(String, f64)> = Vec::new();
    let mut any_percentile = false;
    for e in &sheet.entries {
        let Some(measure) = inst.measures.iter().find(|x| x.key == e.measure) else {
            return Err(format!("מדד לא מוכר: {}", e.measure));
        };
        if e.value < measure.min || e.value > measure.max {
            return Err(format!(
                "{}: הציון {} מחוץ לטווח האפשרי ({}–{})",
                measure.name_he,
                fmt_value(e.value),
                fmt_value(measure.min),
                fmt_value(measure.max)
            ));
        }
        if measure.group != group {
            group.clone_from(&measure.group);
            out.push_str(&format!("{group}:\n"));
        }
        let name = if measure.abbr.is_empty() {
            measure.name_he.clone()
        } else {
            format!("{} ({})", measure.name_he, measure.abbr)
        };
        let range = match measure.scale {
            ScoreScale::RawCutoff => match sheet.cutoff {
                Some(c) if measure.key == "total" && e.value >= c => {
                    format!("בנקודת החתך או מעליה ({})", fmt_value(c))
                }
                Some(c) if measure.key == "total" => format!("מתחת לנקודת החתך ({})", fmt_value(c)),
                _ => String::new(),
            },
            _ => classify(measure, e.value)
                .map(|b| b.label.clone())
                .unwrap_or_default(),
        };
        // "ציון" before the number: the privacy filter reads "29.5" alone as a date (29 May).
        let value = match percentile(measure, e.value) {
            Some(p) => {
                any_percentile = true;
                format!("ציון {}, אחוזון {p}", fmt_value(e.value))
            }
            None => format!("ציון {}", fmt_value(e.value)),
        };
        let note = if e.note.trim().is_empty() {
            String::new()
        } else {
            format!(". {}", e.note.trim())
        };
        if range.is_empty() {
            out.push_str(&format!("- {name}: {value}{note}\n"));
        } else {
            out.push_str(&format!("- {name}: {value} – {range}{note}\n"));
        }
        if measure.scale == ScoreScale::Standard
            && measure.group == "מדדים"
            && !matches!(measure.key.as_str(), "fsiq" | "gac")
        {
            indexes.push((measure.name_he.clone(), e.value));
        }
    }
    if let (Some(hi), Some(lo)) = (
        indexes.iter().max_by(|a, b| a.1.total_cmp(&b.1)),
        indexes.iter().min_by(|a, b| a.1.total_cmp(&b.1)),
    ) {
        if hi.1 - lo.1 >= 15.0 {
            out.push_str(&format!(
                "פער של {} נקודות בין {} ({}) לבין {} ({}), סטיית תקן אחת או יותר. מובהקות הפער נבדקת לפי טבלאות המבחן.\n",
                fmt_value(hi.1 - lo.1), hi.0, fmt_value(hi.1), lo.0, fmt_value(lo.1)
            ));
        }
    }
    out.push_str(&inst.scale_note_he);
    out.push('\n');
    if any_percentile {
        out.push_str("האחוזונים חושבו לפי העקומה הנורמלית.\n");
    }
    out.push_str("הטווחים חושבו בתוכנה לפי טבלה קבועה, לא על ידי מודל השפה.\n");
    if !sheet.notes.trim().is_empty() {
        out.push_str(&format!("הערות המאבחנת: {}\n", sheet.notes.trim()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(inst: &str, key: &str) -> Measure {
        instruments()
            .into_iter()
            .find(|i| i.key == inst)
            .and_then(|i| i.measures.into_iter().find(|m| m.key == key))
            .unwrap_or_else(|| panic!("{inst}/{key}"))
    }

    #[test]
    fn wechsler_bands_cover_every_integer_without_gaps() {
        let idx = measure("wppsi_iv", "vci");
        for v in 40..=160 {
            assert!(classify(&idx, f64::from(v)).is_some(), "standard {v}");
        }
        let sub = measure("wppsi_iv", "matrix");
        for v in 1..=19 {
            assert!(classify(&sub, f64::from(v)).is_some(), "scaled {v}");
        }
        assert!(classify(&sub, 20.0).is_none());
    }

    #[test]
    fn labels_match_the_report_style() {
        assert_eq!(
            classify(&measure("wppsi_iv", "vci"), 111.0).map(|b| b.label.as_str()),
            Some("ממוצע גבוה")
        );
        assert_eq!(
            classify(&measure("wppsi_iv", "matrix"), 9.0).map(|b| b.label.as_str()),
            Some("ממוצע")
        );
        assert_eq!(
            classify(&measure("wppsi_iv", "psi"), 74.0).map(|b| b.label.as_str()),
            Some("גבולי")
        );
        assert_eq!(
            classify(&measure("asrs", "total"), 62.0).map(|b| b.label.as_str()),
            Some("מעט מעל הטווח התקין")
        );
        assert_eq!(
            classify(&measure("cbcl_1_5", "total"), 64.0).map(|b| b.label.as_str()),
            Some("בטווח הקליני")
        );
        assert_eq!(
            classify(&measure("cbcl_1_5", "withdrawn"), 64.0).map(|b| b.label.as_str()),
            Some("בטווח התקין")
        );
        assert_eq!(
            classify(&measure("cars_2", "total"), 30.0).map(|b| b.label.as_str()),
            Some("תסמינים בדרגה קלה-בינונית")
        );
    }

    #[test]
    fn sheet_text_with_ranges_gap_note_and_cutoff() {
        let sheet = ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                ScoreEntry {
                    measure: "vci".into(),
                    value: 112.0,
                    note: String::new(),
                },
                ScoreEntry {
                    measure: "psi".into(),
                    value: 88.0,
                    note: "עבד לאט ובדייקנות".into(),
                },
                ScoreEntry {
                    measure: "matrix".into(),
                    value: 13.0,
                    note: String::new(),
                },
            ],
            notes: "שיתף פעולה לאורך כל ההעברה".into(),
        };
        let text = format_sheet(&sheet).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            text.contains("- הבנה מילולית (VCI): ציון 112, אחוזון 79 – ממוצע גבוה"),
            "{text}"
        );
        assert!(
            text.contains(
                "- מהירות עיבוד (PSI): ציון 88, אחוזון 21 – ממוצע נמוך. עבד לאט ובדייקנות"
            ),
            "{text}"
        );
        assert!(
            text.contains("- מטריצות (MR): ציון 13, אחוזון 84 – ממוצע גבוה"),
            "{text}"
        );
        assert!(text.contains("פער של 24 נקודות"), "{text}");
        assert!(text.contains("לא על ידי מודל השפה"));

        let ados = ScoreSheet {
            instrument: "ados_2".into(),
            module: "1".into(),
            cutoff: Some(11.0),
            entries: vec![ScoreEntry {
                measure: "total".into(),
                value: 14.0,
                note: String::new(),
            }],
            notes: String::new(),
        };
        let text = format_sheet(&ados).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            text.contains("מודול: 1") && text.contains("ציון 14 – בנקודת החתך או מעליה (11)"),
            "{text}"
        );
    }

    #[test]
    fn impossible_scores_are_refused() {
        let sheet = ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![ScoreEntry {
                measure: "matrix".into(),
                value: 25.0,
                note: String::new(),
            }],
            notes: String::new(),
        };
        assert!(format_sheet(&sheet).is_err_and(|e| e.contains("מחוץ לטווח")));
    }

    #[test]
    fn percentiles_match_the_printed_wechsler_tables() {
        let idx = measure("wppsi_iv", "vci");
        let sub = measure("wppsi_iv", "matrix");
        for (v, want) in [
            (100.0, "50"),
            (91.0, "27"),
            (69.0, "2"),
            (130.0, "98"),
            (60.0, "0.4"),
            (65.0, "1"),
            (135.0, "99"),
            (140.0, "99.6"),
            (145.0, "99.9"),
            (55.0, "0.1"),
            (40.0, "<0.1"),
            (160.0, ">99.9"),
        ] {
            assert_eq!(percentile(&idx, v).as_deref(), Some(want), "standard {v}");
        }
        for (v, want) in [
            (10.0, "50"),
            (7.0, "16"),
            (13.0, "84"),
            (4.0, "2"),
            (1.0, "0.1"),
            (19.0, "99.9"),
        ] {
            assert_eq!(percentile(&sub, v).as_deref(), Some(want), "scaled {v}");
        }
        assert_eq!(percentile(&measure("asrs", "total"), 65.0), None);
    }

    #[test]
    fn every_instrument_is_well_formed() {
        let all = instruments();
        let mut keys: Vec<&str> = all.iter().map(|i| i.key.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), all.len(), "instrument keys are unique");
        for inst in &all {
            assert!(inst.min_age_months < inst.max_age_months, "{}", inst.key);
            let mut measures: Vec<&str> = inst.measures.iter().map(|m| m.key.as_str()).collect();
            measures.sort_unstable();
            measures.dedup();
            assert_eq!(
                measures.len(),
                inst.measures.len(),
                "{}: measure keys are unique",
                inst.key
            );
            for m in &inst.measures {
                for b in &m.bands {
                    assert!(
                        b.min <= b.max && b.min >= m.min && b.max <= m.max,
                        "{}/{}: {}",
                        inst.key,
                        m.key,
                        b.label
                    );
                }
            }
        }
        assert_eq!(
            classify(&measure("ados_2", "comparison"), 6.0).map(|b| b.level),
            Some(2)
        );
    }
}
