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
    /// Vineland v-scale score: mean 15, SD 3 (round 32).
    VScale,
    /// An ability T score, higher = better: mean 50, SD 10 (Mullen scales, round 32).
    TAbility,
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
    /// Other names it goes by in reports ("קידוד" for WISC-V coding, round 32).
    #[serde(default)]
    pub aliases: Vec<String>,
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

/// Vineland-3 adaptive levels for the composite and the domains (mean 100, SD 15).
fn vineland_bands() -> Vec<Band> {
    vec![
        band(130.0, 140.0, "גבוה", 0),
        band(115.0, 129.0, "גבוה במידה מתונה", 0),
        band(86.0, 114.0, "תקין", 0),
        band(71.0, 85.0, "נמוך במידה מתונה", 1),
        band(20.0, 70.0, "נמוך", 3),
    ]
}

/// Vineland-3 adaptive levels for the subdomains (v-scale, mean 15, SD 3).
fn vscale_bands() -> Vec<Band> {
    vec![
        band(21.0, 24.0, "גבוה", 0),
        band(18.0, 20.0, "גבוה במידה מתונה", 0),
        band(13.0, 17.0, "תקין", 0),
        band(10.0, 12.0, "נמוך במידה מתונה", 1),
        band(1.0, 9.0, "נמוך", 3),
    ]
}

/// Mullen descriptive categories for the scales (T, mean 50, SD 10).
fn mullen_t_bands() -> Vec<Band> {
    vec![
        band(70.0, 80.0, "גבוה מאוד", 0),
        band(60.0, 69.0, "מעל הממוצע", 0),
        band(40.0, 59.0, "ממוצע", 0),
        band(30.0, 39.0, "מתחת לממוצע", 2),
        band(20.0, 29.0, "נמוך מאוד", 3),
    ]
}

/// Mullen descriptive categories for the Early Learning Composite (mean 100, SD 15).
fn mullen_elc_bands() -> Vec<Band> {
    vec![
        band(130.0, 155.0, "גבוה מאוד", 0),
        band(115.0, 129.0, "מעל הממוצע", 0),
        band(85.0, 114.0, "ממוצע", 0),
        band(70.0, 84.0, "מתחת לממוצע", 2),
        band(49.0, 69.0, "נמוך מאוד", 3),
    ]
}

/// BRIEF-P: T ≥ 65 is clinically elevated; 60–64 mildly elevated (pending her approval).
fn brief_bands() -> Vec<Band> {
    vec![
        band(20.0, 59.0, "בטווח התקין", 0),
        band(60.0, 64.0, "מוגבר במידה קלה", 1),
        band(65.0, 120.0, "מוגבר באופן מובהק קלינית", 3),
    ]
}

/// A measure with other names it goes by in reports.
fn with_aliases(mut x: Measure, aliases: &[&str]) -> Measure {
    x.aliases = aliases.iter().map(|a| (*a).to_owned()).collect();
    x
}

/// A measure with its own range and table.
fn with_table(mut x: Measure, min: f64, max: f64, bands: Vec<Band>) -> Measure {
    x.min = min;
    x.max = max;
    x.bands = bands;
    x
}

fn m(key: &str, name_he: &str, abbr: &str, group: &str, scale: ScoreScale) -> Measure {
    let (min, max, bands) = match scale {
        ScoreScale::Standard => (40.0, 160.0, standard_bands()),
        ScoreScale::Scaled => (1.0, 19.0, scaled_bands()),
        ScoreScale::VScale => (1.0, 24.0, vscale_bands()),
        ScoreScale::TAbility => (20.0, 80.0, mullen_t_bands()),
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
        aliases: Vec::new(),
    }
}

const STANDARD_NOTE: &str = "ציוני המדדים הם ציוני תקן (ממוצע 100, סטיית תקן 15); ציוני תת-המבחנים הם ציונים מותאמים (ממוצע 10, סטיית תקן 3).";

/// The instruments the psychologist uses (docs/REPORT_STRUCTURE.md).
#[must_use]
pub fn instruments() -> Vec<Instrument> {
    use ScoreScale::{Cars, Comparison, RawCutoff, Scaled, Standard, TAbility, TConcern, VScale};
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
        // Achenbach syndrome T scores start at 50: a lower one cannot exist and is refused.
        x.min = 50.0;
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
            description_he: "מבחן וקסלר לילדים בגן ובתחילת בית הספר (2:6–7:7)".into(),
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
                with_aliases(
                    m("animal_coding", "קידוד חיות", "AC", "תת-מבחנים", Scaled),
                    &["קידוד"],
                ),
                m("receptive_vocabulary", "אוצר מילים רצפטיבי", "RV", "תת-מבחנים", Scaled),
                m("picture_naming", "שיום תמונות", "PN", "תת-מבחנים", Scaled),
                // Ancillary indexes: not in the gap between the primary indexes.
                m("vai", "רכישת אוצר מילים", "VAI", "מדדים נוספים", Standard),
                m("nvi", "מדד לא מילולי", "NVI", "מדדים נוספים", Standard),
                m("gai", "מדד יכולת כללית", "GAI", "מדדים נוספים", Standard),
                m("cpi", "מדד יעילות קוגניטיבית", "CPI", "מדדים נוספים", Standard),
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
                with_aliases(
                    m("coding", "צפנים", "CD", "תת-מבחנים", Scaled),
                    &["קידוד"],
                ),
                m("symbol_search", "חיפוש סמלים", "SS", "תת-מבחנים", Scaled),
                m("cancellation", "מחיקה", "CA", "תת-מבחנים", Scaled),
            ],
        },
        Instrument {
            key: "wppsi_iii".into(),
            name: "WPPSI-III".into(),
            description_he: "מבחן וקסלר לגיל הרך, מהדורה שלישית (2:6–7:3)".into(),
            scale_note_he: STANDARD_NOTE.into(),
            min_age_months: 30,
            max_age_months: 87,
            measures: vec![
                m("fsiq", "מנת משכל כללית", "FSIQ", "מדדים", Standard),
                m("viq", "מנת משכל מילולית", "VIQ", "מדדים", Standard),
                m("piq", "מנת משכל ביצועית", "PIQ", "מדדים", Standard),
                m("psq", "מנת מהירות עיבוד", "PSQ", "מדדים", Standard),
                m("glc", "שפה כללית", "GLC", "מדדים", Standard),
                m("information", "מידע", "IN", "תת-מבחנים", Scaled),
                m("vocabulary", "אוצר מילים", "VC", "תת-מבחנים", Scaled),
                m("word_reasoning", "היסק מילולי", "WR", "תת-מבחנים", Scaled),
                m("comprehension", "הבנה", "CO", "תת-מבחנים", Scaled),
                m("similarities", "שיתופיות", "SI", "תת-מבחנים", Scaled),
                m("block_design", "קוביות", "BD", "תת-מבחנים", Scaled),
                m("matrix", "מטריצות", "MR", "תת-מבחנים", Scaled),
                m("picture_concepts", "מושגים בתמונות", "PCn", "תת-מבחנים", Scaled),
                m("picture_completion", "השלמת תמונות", "PCm", "תת-מבחנים", Scaled),
                m("object_assembly", "הרכבת עצמים", "OA", "תת-מבחנים", Scaled),
                with_aliases(
                    m("coding", "צפנים", "CD", "תת-מבחנים", Scaled),
                    &["קידוד"],
                ),
                m("symbol_search", "חיפוש סמלים", "SS", "תת-מבחנים", Scaled),
                m("receptive_vocabulary", "אוצר מילים רצפטיבי", "RV", "תת-מבחנים", Scaled),
                m("picture_naming", "שיום תמונות", "PN", "תת-מבחנים", Scaled),
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
                with_aliases(
                    m("cognitive", "קוגניטיבי", "COG", "מדדים", Standard),
                    &["קוגניטיבית"],
                ),
                m("language", "שפה", "LANG", "מדדים", Standard),
                with_aliases(m("motor", "מוטורי", "MOT", "מדדים", Standard), &["מוטורית"]),
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
                m("home_living", "תפקוד בבית / במסגרת", "HL", "תחומי מיומנות", Scaled),
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
            key: "brief_p".into(),
            name: "BRIEF-P".into(),
            description_he: "סולם דירוג לתפקודים ניהוליים בגיל הרך (הורים / גננת)".into(),
            scale_note_he: "ציוני T (ממוצע 50, סטיית תקן 10); ככל שהערך גבוה יותר, הקושי רב יותר. ציון T של 65 ומעלה מוגבר באופן מובהק קלינית.".into(),
            min_age_months: 24,
            max_age_months: 71,
            measures: [
                ("inhibit", "עכבה", "INH", "סולמות"),
                ("shift", "גמישות", "SHF", "סולמות"),
                ("emotional_control", "שליטה רגשית", "EC", "סולמות"),
                ("working_memory", "זיכרון עבודה", "WM", "סולמות"),
                ("plan_organize", "תכנון וארגון", "PO", "סולמות"),
                ("isci", "שליטה עצמית מעכבת", "ISCI", "מדדים"),
                ("fi", "מדד גמישות", "FI", "מדדים"),
                ("emi", "מטה-קוגניציה מתהווה", "EMI", "מדדים"),
                ("gec", "מדד ניהולי כללי", "GEC", "מדדים"),
            ]
            .iter()
            .map(|(k, n, a, g)| with_table(m(k, n, a, g, TConcern), 20.0, 120.0, brief_bands()))
            // A report may call it a score; that name never goes out in a table ("ציון" is
            // also a name).
            .map(|x| if x.key == "gec" { with_aliases(x, &["ציון ניהולי כללי"]) } else { x })
            .collect(),
        },
        Instrument {
            key: "vineland_3".into(),
            name: "Vineland-3".into(),
            description_he: "סולמות וינלנד להתנהגות מסתגלת (ראיון עם ההורים או דירוג של ההורים / הצוות)".into(),
            scale_note_he: "המדד הכללי והתחומים הם ציוני תקן (ממוצע 100, סטיית תקן 15); תת-התחומים הם ציוני v (ממוצע 15, סטיית תקן 3).".into(),
            min_age_months: 0,
            max_age_months: 1079,
            measures: [
                ("abc", "מדד התנהגות מסתגלת", "ABC"),
                ("communication", "תקשורת", "COM"),
                ("daily_living", "מיומנויות יומיום", "DLS"),
                ("socialization", "חברות", "SOC"),
                ("motor", "מוטוריקה", "MOT"),
            ]
            .iter()
            .map(|(k, n, a)| with_table(m(k, n, a, "תחומים", Standard), 20.0, 140.0, vineland_bands()))
            .chain(
                [
                    ("receptive", "קליטה", "REC"),
                    ("expressive", "הבעה", "EXP"),
                    ("written", "קריאה וכתיבה", "WRN"),
                    ("personal", "אישי", "PER"),
                    ("domestic", "ביתי", "DOM"),
                    ("community", "קהילתי", "CMM"),
                    ("interpersonal", "יחסים בין-אישיים", "IPR"),
                    ("play_leisure", "משחק ופנאי", "PLY"),
                    ("coping", "התמודדות", "CPS"),
                    ("fine_motor", "מוטוריקה עדינה", "FMS"),
                    ("gross_motor", "מוטוריקה גסה", "GMS"),
                ]
                .iter()
                .map(|(k, n, a)| m(k, n, a, "תת-תחומים", VScale)),
            )
            .collect(),
        },
        Instrument {
            key: "mullen".into(),
            name: "MSEL".into(),
            description_he: "סולמות מולן ללמידה מוקדמת (לידה עד 68 חודשים)".into(),
            scale_note_he: "המדד המשולב ללמידה מוקדמת הוא ציון תקן (ממוצע 100, סטיית תקן 15); הסולמות הם ציוני T (ממוצע 50, סטיית תקן 10).".into(),
            min_age_months: 0,
            max_age_months: 68,
            measures: vec![
                with_table(
                    m("elc", "מדד משולב ללמידה מוקדמת", "ELC", "מדדים", Standard),
                    49.0,
                    155.0,
                    mullen_elc_bands(),
                ),
                m("gross_motor", "מוטוריקה גסה", "GM", "סולמות", TAbility),
                m("visual_reception", "קליטה חזותית", "VR", "סולמות", TAbility),
                m("fine_motor", "מוטוריקה עדינה", "FM", "סולמות", TAbility),
                m("receptive_language", "שפה קולטת", "RL", "סולמות", TAbility),
                m("expressive_language", "שפה מבטאת", "EL", "סולמות", TAbility),
            ],
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

/// The totals of the instruments: a composite over the indexes or domains of a test (FSIQ,
/// GAC, Vineland's ABC, Mullen's ELC) or a questionnaire's total. Never one of the indexes a
/// gap or an order is read between.
pub(crate) const TOTALS: [&str; 5] = ["fsiq", "gac", "abc", "elc", "total"];

/// The smallest step a score of a scale is written in. Every scale here is in whole numbers
/// (standard, scaled, v-scale and T scores; ADOS-2 raw totals and comparison score), but
/// CARS-2 items are scored in halves. Exhaustive on purpose: a scale added later (an age
/// equivalent in months, a growth score) must say what its step is.
pub fn score_step(scale: ScoreScale) -> f64 {
    match scale {
        ScoreScale::Standard
        | ScoreScale::Scaled
        | ScoreScale::VScale
        | ScoreScale::TAbility
        | ScoreScale::TConcern
        | ScoreScale::RawCutoff
        | ScoreScale::Comparison => 1.0,
        ScoreScale::Cars => 0.5,
    }
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
    let (mean, sd) = ability_norm(measure.scale)?;
    let z = (value - mean) / sd;
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

/// The mean and standard deviation of an ability scale, where higher is better; `None` for
/// questionnaires and the other scales.
fn ability_norm(scale: ScoreScale) -> Option<(f64, f64)> {
    match scale {
        ScoreScale::Standard => Some((100.0, 15.0)),
        ScoreScale::Scaled => Some((10.0, 3.0)),
        ScoreScale::VScale => Some((15.0, 3.0)),
        ScoreScale::TAbility => Some((50.0, 10.0)),
        _ => None,
    }
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

/// The range a score falls in: from the table, or against the ADOS-2 cut-off entered.
fn range_of(measure: &Measure, sheet: &ScoreSheet, value: f64) -> String {
    match measure.scale {
        ScoreScale::RawCutoff => match sheet.cutoff {
            Some(c) if measure.key == "total" && value >= c => {
                format!("בנקודת החתך או מעליה ({})", fmt_value(c))
            }
            Some(c) if measure.key == "total" => format!("מתחת לנקודת החתך ({})", fmt_value(c)),
            _ => String::new(),
        },
        _ => classify(measure, value)
            .map(|b| b.label.clone())
            .unwrap_or_default(),
    }
}

/// The sheet's instrument and each entry with its measure, checked once for both the text sent
/// to Claude and the Word table (so the two can never disagree).
/// An instrument and the sheet's entries, each with its measure.
type Checked<'a> = (Instrument, Vec<(Measure, &'a ScoreEntry)>);

fn checked(sheet: &ScoreSheet) -> Result<Checked<'_>, String> {
    let inst = instruments()
        .into_iter()
        .find(|i| i.key == sheet.instrument)
        .ok_or("כלי לא מוכר")?;
    let mut out = Vec::new();
    for e in &sheet.entries {
        let Some(measure) = inst.measures.iter().find(|x| x.key == e.measure) else {
            return Err(format!("מדד לא מוכר: {}", e.measure));
        };
        // Written as "not inside" so that NaN, which compares false with everything, is refused.
        if !(measure.min..=measure.max).contains(&e.value) {
            // The value as she typed it (f64's shortest round-trip), never rounded to a number
            // she never entered (30.25 is not "30.2").
            return Err(format!(
                "{}: הציון {} מחוץ לטווח האפשרי ({}–{})",
                measure.name_he,
                e.value,
                fmt_value(measure.min),
                fmt_value(measure.max)
            ));
        }
        // A score that is not on the scale's grid (109.5 on a composite, 29.3 on CARS-2) is
        // not a score that can exist: refused, never written without a range.
        let step = score_step(measure.scale);
        if ((e.value / step).round() * step - e.value).abs() > 1e-9 {
            return Err(format!(
                "{}: הציון {} לא אפשרי בסולם הזה ({})",
                measure.name_he,
                e.value,
                if measure.scale == ScoreScale::Cars {
                    "ציונים בקפיצות של חצי נקודה"
                } else {
                    "ציונים שלמים בלבד"
                }
            ));
        }
        out.push((measure.clone(), e));
    }
    Ok((inst, out))
}

/// The sheet's entries with their measures, without the gate of [`checked`]: a sheet an older
/// version saved with a value `checked` now refuses (109.5 on a composite) still has every
/// other score she entered, and each is still held against the model's text and still has its
/// cases. An entry with no measure is left out; an unknown instrument is `None`. The gate for
/// the text, the table and the export stays [`checked`].
fn entries_of(sheet: &ScoreSheet) -> Option<Checked<'_>> {
    let inst = instruments()
        .into_iter()
        .find(|i| i.key == sheet.instrument)?;
    let entries = sheet
        .entries
        .iter()
        .filter_map(|e| {
            inst.measures
                .iter()
                .find(|x| x.key == e.measure)
                .map(|m| (m.clone(), e))
        })
        .collect();
    Some((inst, entries))
}

/// "מנת משכל כללית (FSIQ)".
fn measure_label(measure: &Measure) -> String {
    if measure.abbr.is_empty() {
        measure.name_he.clone()
    } else {
        format!("{} ({})", measure.name_he, measure.abbr)
    }
}

/// One row of the score table in the Word report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreRow {
    pub measure: String,
    pub score: String,
    /// "—" where a percentile does not apply (questionnaires, raw scores).
    pub percentile: String,
    pub range: String,
}

/// A sheet as a table for the report: title, rows, and the scale sentence under it.
pub fn sheet_table(sheet: &ScoreSheet) -> Result<(String, Vec<ScoreRow>, String), String> {
    let (inst, entries) = checked(sheet)?;
    let rows = entries
        .iter()
        .map(|(measure, e)| ScoreRow {
            measure: measure_label(measure),
            score: fmt_value(e.value),
            percentile: percentile(measure, e.value).unwrap_or_else(|| "—".to_owned()),
            range: range_of(measure, sheet, e.value),
        })
        .collect();
    let title = if sheet.module.trim().is_empty() {
        inst.name.clone()
    } else {
        format!("{} · מודול {}", inst.name, sheet.module.trim())
    };
    Ok((title, rows, inst.scale_note_he.clone()))
}

/// A bar profile for the report (EX-2): one bar per score on a fixed axis.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreProfile {
    pub title: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub mean: f64,
    pub sd: f64,
    /// (label, score), in the sheet's order.
    pub bars: Vec<(String, f64)>,
    pub note: String,
}

/// Profiles of a sheet: index scores (mean 100, SD 15) and subtest scaled scores (mean 10,
/// SD 3), each on its own axis. Other scales (T scores, cut-offs) are not drawn: a bar there
/// would suggest a comparison the scale does not support.
pub fn sheet_profiles(sheet: &ScoreSheet) -> Result<Vec<ScoreProfile>, String> {
    let (_, entries) = checked(sheet)?;
    let mut out = Vec::new();
    for (scale, title, min, max, step, mean, sd, note) in [
        (
            ScoreScale::Standard,
            "פרופיל המדדים",
            40.0,
            160.0,
            5.0,
            100.0,
            15.0,
            "הרקע הבהיר: טווח של סטיית תקן אחת סביב הממוצע (85–115).",
        ),
        (
            ScoreScale::Scaled,
            "פרופיל תת-המבחנים",
            1.0,
            20.0,
            1.0,
            10.0,
            3.0,
            "הרקע הבהיר: טווח של סטיית תקן אחת סביב הממוצע (7–13).",
        ),
    ] {
        let bars: Vec<(String, f64)> = entries
            .iter()
            .filter(|(m, _)| m.scale == scale)
            .map(|(m, e)| (measure_label(m), e.value))
            .collect();
        if !bars.is_empty() {
            out.push(ScoreProfile {
                title: title.to_owned(),
                min,
                max,
                step,
                mean,
                sd,
                bars,
                note: note.to_owned(),
            });
        }
    }
    Ok(out)
}

/// Where a score falls in the parents' chart (round 55, D-095).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentGroup {
    /// "חזק": a typical band that starts above the scale's mean ("ממוצע גבוה", "מעל הממוצע").
    Strong,
    /// "בטווח הגיל": the typical band around the mean.
    InRange,
    /// "קשה יותר": a band marked mild or worse ("ממוצע נמוך" and below).
    Harder,
}

/// One measure in the parents' chart.
#[derive(Debug, Clone, PartialEq)]
pub struct ParentBand {
    /// The measure's Hebrew name, without its abbreviation.
    pub label: String,
    pub group: ParentGroup,
    pub value: f64,
}

/// The parents' chart of a sheet, from the same fixed tables as the report (never the model).
/// Only ability scales (index, subtest, v-scale and ability T scores), where higher is better:
/// questionnaires and autism measures say nothing a "strong" column could hold, and are left
/// to her words in the text.
pub fn sheet_bands(sheet: &ScoreSheet) -> Result<Vec<ParentBand>, String> {
    let (_, entries) = checked(sheet)?;
    Ok(entries
        .iter()
        .filter_map(|(measure, e)| {
            let (mean, _) = ability_norm(measure.scale)?;
            let band = classify(measure, e.value)?;
            let group = if band.level >= 1 {
                ParentGroup::Harder
            } else if band.min > mean {
                ParentGroup::Strong
            } else {
                ParentGroup::InRange
            };
            Some(ParentBand {
                label: measure.name_he.clone(),
                group,
                value: e.value,
            })
        })
        .collect())
}

/// Said under every list of scores that goes out: where the ranges come from.
const RANGES_NOTE: &str = "הטווחים חושבו בתוכנה לפי טבלה קבועה, לא על ידי מודל השפה.";

/// One score as the sheet words it ("- name: ציון 98, אחוזון 45 – ממוצע. her note"), and
/// whether a percentile is in it. The one wording of a score that goes out: the material
/// ([`format_sheet_with`]) and the key scores of the closing sections ([`key_scores`]).
fn score_line(
    measure: &Measure,
    sheet: &ScoreSheet,
    e: &ScoreEntry,
    habits: SheetHabits,
) -> (String, bool) {
    let name = measure_label(measure);
    let range = range_of(measure, sheet, e.value);
    // "ציון" before the number: the privacy filter reads "29.5" alone as a date (29 May).
    let omit = if matches!(measure.scale, ScoreScale::Scaled | ScoreScale::VScale) {
        habits.no_subtest_percentiles
    } else {
        habits.no_index_percentiles
    };
    let percentile = percentile(measure, e.value).filter(|_| !omit);
    let value = match &percentile {
        Some(p) => format!("ציון {}, אחוזון {p}", fmt_value(e.value)),
        None => format!("ציון {}", fmt_value(e.value)),
    };
    let note = if e.note.trim().is_empty() {
        String::new()
    } else {
        format!(". {}", e.note.trim())
    };
    let line = if range.is_empty() {
        format!("- {name}: {value}{note}")
    } else {
        format!("- {name}: {value} – {range}{note}")
    };
    (line, percentile.is_some())
}

/// A total, an index or a questionnaire scale: what a summary quotes. Not a subtest (a scaled
/// score, a v-scale score, an ability T score of one scale).
fn is_key_measure(measure: &Measure) -> bool {
    !matches!(
        measure.scale,
        ScoreScale::Scaled | ScoreScale::VScale | ScoreScale::TAbility
    )
}

/// The key scores of a case, for the sections written from the approved ones (D-162): for each
/// sheet its totals, indexes and scales, each on the very line the score material has it on
/// ([`format_sheet_with`]: the value, the percentile unless she leaves it out, the range from
/// the fixed table, her note), and then the patterns of the scores ([`crate::sheet_patterns`]).
/// Nothing is worded here that the tables do not hold. A sheet that does not pass the checks
/// of the score table is left out; `None` when no sheet has a key score.
#[must_use]
pub fn key_scores(sheets: &[ScoreSheet], habits: SheetHabits) -> Option<String> {
    let mut out = String::new();
    for sheet in sheets {
        let Ok((inst, entries)) = checked(sheet) else {
            continue;
        };
        let lines: Vec<String> = entries
            .iter()
            .filter(|(measure, _)| is_key_measure(measure))
            .map(|(measure, e)| score_line(measure, sheet, e, habits).0)
            .collect();
        if lines.is_empty() {
            continue;
        }
        if sheet.module.trim().is_empty() {
            out.push_str(&format!("{}:\n", inst.name));
        } else {
            out.push_str(&format!("{} · מודול {}:\n", inst.name, sheet.module.trim()));
        }
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    if out.is_empty() {
        return None;
    }
    let patterns = crate::sheet_patterns(sheets);
    if !patterns.is_empty() {
        let labels: Vec<&str> = patterns.iter().map(|p| p.label.as_str()).collect();
        out.push_str(&format!(
            "דפוסי התוצאות, כפי שחושבו בתוכנה מהטבלאות: {}.\n",
            labels.join(" · ")
        ));
    }
    out.push_str(RANGES_NOTE);
    out.push('\n');
    Some(out)
}

/// The text stored as a "test scores" material: every score with its range from the table,
/// the scale sentence, and a note on large gaps between indexes (to check in the manual).
pub fn format_sheet(sheet: &ScoreSheet) -> Result<String, String> {
    format_sheet_with(sheet, SheetHabits::default())
}

/// A group of the sheet on one scale, and its (label, score) rows, for the order line.
type OrderGroup<'a> = (&'a str, ScoreScale, Vec<(String, f64)>);

/// [`format_sheet`], leaving out what she does not write beside scores (K12).
pub fn format_sheet_with(sheet: &ScoreSheet, habits: SheetHabits) -> Result<String, String> {
    let (inst, entries) = checked(sheet)?;
    let mut out = format!("{} – {}\n", inst.name, inst.description_he);
    if !sheet.module.trim().is_empty() {
        out.push_str(&format!("מודול: {}\n", sheet.module.trim()));
    }
    let mut group = String::new();
    let mut indexes: Vec<(String, f64)> = Vec::new();
    let mut any_percentile = false;
    for (measure, e) in &entries {
        if measure.group != group {
            group.clone_from(&measure.group);
            out.push_str(&format!("{group}:\n"));
        }
        let (line, with_percentile) = score_line(measure, sheet, e, habits);
        any_percentile |= with_percentile;
        out.push_str(&line);
        out.push('\n');
        if measure.scale == ScoreScale::Standard && measure.group == "מדדים" && !is_total(measure)
        {
            indexes.push((measure.name_he.clone(), e.value));
        }
    }
    if let (Some(hi), Some(lo)) = (
        indexes.iter().max_by(|a, b| a.1.total_cmp(&b.1)),
        indexes.iter().min_by(|a, b| a.1.total_cmp(&b.1)),
    ) {
        if hi.1 - lo.1 >= crate::patterns::INDEX_GAP {
            out.push_str(&format!(
                "פער של {} נקודות בין {} ({}) לבין {} ({}), סטיית תקן אחת או יותר. מובהקות הפער נבדקת לפי טבלאות המבחן.\n",
                fmt_value(hi.1 - lo.1), hi.0, fmt_value(hi.1), lo.0, fmt_value(lo.1)
            ));
        }
    }
    // The order the report gives them in (the institute's: from the highest to the lowest),
    // worked out here, never by the model. A total or composite is not one of the domains it
    // orders, and only scores on one scale are ordered together (D-165): raw scores of
    // different domains (ADOS-2 SA and RRB) have no common scale at all.
    let mut groups: Vec<OrderGroup<'_>> = Vec::new();
    for (measure, e) in &entries {
        if is_total(measure) || measure.scale == ScoreScale::RawCutoff {
            continue;
        }
        let row = (measure_label(measure), e.value);
        match groups
            .iter_mut()
            .find(|(g, s, _)| *g == measure.group && *s == measure.scale)
        {
            Some((_, _, rows)) => rows.push(row),
            None => groups.push((&measure.group, measure.scale, vec![row])),
        }
    }
    for (group, _, mut rows) in groups.into_iter().filter(|(_, _, r)| r.len() >= 2) {
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        let order: Vec<String> = rows
            .iter()
            .map(|(name, v)| format!("{name} – ציון {}", fmt_value(*v)))
            .collect();
        out.push_str(&format!("{group} מהגבוה לנמוך: {}\n", order.join("; ")));
    }
    if let Some(line) = relative_line(sheet, &entries) {
        out.push_str(&line);
    }
    if let Some(line) = elevated_line(&entries) {
        out.push_str(&line);
    }
    out.push_str(&inst.scale_note_he);
    out.push('\n');
    if any_percentile {
        out.push_str("האחוזונים חושבו לפי העקומה הנורמלית.\n");
    }
    out.push_str(RANGES_NOTE);
    out.push('\n');
    if !sheet.notes.trim().is_empty() {
        out.push_str(&format!("הערות המאבחנת: {}\n", sheet.notes.trim()));
    }
    Ok(out)
}

/// A total or composite of a test (FSIQ, GAC, Vineland ABC, Mullen ELC, a questionnaire's
/// total, BRIEF-P GEC): never one of the domains it sums up.
fn is_total(measure: &Measure) -> bool {
    TOTALS.contains(&measure.key.as_str()) || measure.key == "gec"
}

/// The sheet's relative strengths and weaknesses (D-165): exactly the computation of
/// [`sheet_cases`] (three subtests or more, [`RELATIVE_POINTS`] from their own mean). `None`
/// when there is none.
fn relative_line(sheet: &ScoreSheet, entries: &[(Measure, &ScoreEntry)]) -> Option<String> {
    let cases = sheet_cases(sheet);
    let (mut strong, mut weak) = (Vec::new(), Vec::new());
    let mut groups: Vec<&str> = Vec::new();
    for ((m, e), c) in entries.iter().zip(&cases) {
        if matches!(m.scale, ScoreScale::Scaled | ScoreScale::VScale)
            && !groups.contains(&m.group.as_str())
        {
            groups.push(&m.group);
        }
        let item = format!("{} – ציון {}", measure_label(m), fmt_value(e.value));
        if c.cases.iter().any(|x| x == Relative::Strength.label_he()) {
            strong.push(item);
        } else if c.cases.iter().any(|x| x == Relative::Weakness.label_he()) {
            weak.push(item);
        }
    }
    if strong.is_empty() && weak.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !strong.is_empty() {
        parts.push(format!(
            "{}: {}",
            Relative::Strength.label_he(),
            strong.join("; ")
        ));
    }
    if !weak.is_empty() {
        parts.push(format!(
            "{}: {}",
            Relative::Weakness.label_he(),
            weak.join("; ")
        ));
    }
    Some(format!(
        "חוזקות וחולשות יחסיות לפי כלל אצבע, שחושב בתוכנה: הפרש של {1} נקודות ומעלה מהממוצע של הציונים ב{0} שהוזנו בטבלה. מובהקות ההפרש נבדקת לפי טבלאות מדריך המבחן. {2}\n",
        groups.join(" וב"),
        fmt_value(RELATIVE_POINTS),
        parts.join(". ")
    ))
}

/// The questionnaire scales (higher = more concern) whose score falls in a band of its own
/// table that is not the typical one (level 1 and up), with that band (D-165). Ability scales
/// are left out: a low band there is not "elevated", and the score's line already says it.
fn elevated_line(entries: &[(Measure, &ScoreEntry)]) -> Option<String> {
    let elevated: Vec<String> = entries
        .iter()
        .filter(|(m, _)| m.scale == ScoreScale::TConcern)
        .filter_map(|(m, e)| {
            classify(m, e.value).filter(|b| b.level >= 1).map(|b| {
                format!(
                    "{} – ציון {}, {}",
                    measure_label(m),
                    fmt_value(e.value),
                    b.label
                )
            })
        })
        .collect();
    (!elevated.is_empty())
        .then(|| format!("סולמות בטווח חריג לפי הטבלה: {}\n", elevated.join("; ")))
}

/// What one entered score may be written as: its value, and its percentile when it has one.
struct Known {
    label: String,
    value: String,
    percentile: Option<String>,
    /// The score's range in the measure's own table.
    band: Option<String>,
}

/// Where a measure is named in a sentence: by its abbreviation ("VCI"), or by a Hebrew name of
/// two words or five letters and more, as a whole word (with one prefix letter, "ובאוצר").
/// Short names ("מידע", "הבנה", "שפה") are ordinary words too, so only their abbreviation counts.
fn mentions(sentence: &str, measure: &Measure) -> bool {
    let words = sentence_words(sentence);
    if measure.abbr.chars().count() >= 2 && words.iter().any(|w| *w == measure.abbr) {
        return true;
    }
    std::iter::once(measure.name_he.as_str())
        .chain(measure.aliases.iter().map(String::as_str))
        .any(|name| named(&words, name))
}

/// Whether a sentence names `measure` in any way, short names too ("שפה (68)"). Never enough
/// to check a sentence, only to know that a number in it is that measure's own score.
fn mentioned_at_all(sentence: &str, measure: &Measure) -> bool {
    let words = sentence_words(sentence);
    (measure.abbr.chars().count() >= 2 && words.iter().any(|w| *w == measure.abbr))
        || std::iter::once(measure.name_he.as_str())
            .chain(measure.aliases.iter().map(String::as_str))
            .any(|name| named_at(&words, name).next().is_some())
}

fn sentence_words(sentence: &str) -> Vec<&str> {
    sentence
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '"' || c == '\''))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether `words` hold the Hebrew name `name` (see [`mentions`]).
fn named(words: &[&str], name_he: &str) -> bool {
    let long = name_he.split_whitespace().count() >= 2 || name_he.chars().count() >= 5;
    long && named_at(words, name_he).next().is_some()
}

/// Where in `words` the Hebrew name `name` starts, with or without a short name's length
/// rule. A first word may take a prefix ("ובמטריצות"), the next ones the article
/// ("ההבנה המילולית").
fn named_at<'a>(words: &'a [&'a str], name_he: &'a str) -> impl Iterator<Item = usize> + 'a {
    let name: Vec<&str> = name_he.split_whitespace().collect();
    let hebrew = !name.is_empty()
        && name.iter().all(|w| {
            w.chars()
                .all(|c| ('\u{05d0}'..='\u{05ea}').contains(&c) || c == '-')
        });
    let n = name.len().max(1);
    (0..words.len().saturating_sub(n - 1)).filter(move |&k| {
        let (Some((&head, tail)), Some(win)) = (name.split_first(), words.get(k..k + n)) else {
            return false;
        };
        let first_ok = |w: &str| {
            w == head
                || w.strip_prefix(['ו', 'ב', 'ה', 'ל', 'מ', 'ש', 'כ'])
                    .is_some_and(|rest| {
                        rest == head || rest.strip_prefix(['ב', 'ה', 'ל', 'מ']) == Some(head)
                    })
        };
        hebrew
            && win.split_first().is_some_and(|(first, rest)| {
                first_ok(first)
                    && rest
                        .iter()
                        .zip(tail)
                        .all(|(w, t)| w == t || w.strip_prefix('ה') == Some(t))
            })
    })
}

/// A number that says how old or how long, not a score (D-145): after "בן", "בת" or "בגיל",
/// or before "שנים", "חודשים" and the like ("בן 5 שנים ו-4 חודשים"). An age that is not the
/// child's is still said, by `check_age`.
fn age_or_span(chars: &[char], start: usize, end: usize) -> bool {
    const BEFORE: &[&str] = &["בן", "בת", "כבן", "כבת", "גיל", "בגיל", "לגיל"];
    const AFTER: &[&str] = &[
        "שנים",
        "שנה",
        "שנות",
        "חודשים",
        "חודש",
        "שבועות",
        "שבוע",
        "ימים",
    ];
    let word = |w: &str| w.trim_matches(|c: char| !c.is_alphabetic()).to_owned();
    let before: String = chars.get(..start).unwrap_or_default().iter().collect();
    let after: String = chars.get(end..).unwrap_or_default().iter().collect();
    let prev = before
        .split_whitespace()
        .last()
        .map(word)
        .unwrap_or_default();
    let next = after
        .split_whitespace()
        .next()
        .map(word)
        .unwrap_or_default();
    BEFORE.contains(&prev.as_str()) || AFTER.contains(&next.as_str())
}

/// A number that is the distance between two scores, not a score: right after "פער של",
/// "הפרש של" or "הבדל של" (or the word alone, "פער 29"). Found in a report's own words: "פער של
/// 29 נקודות בין מוטורי (97) לבין שפה (68)".
fn difference(chars: &[char], start: usize) -> bool {
    const GAP: &[&str] = &["פער", "הפרש", "הבדל", "בפער", "בהפרש"];
    let before: String = chars.get(..start).unwrap_or_default().iter().collect();
    let mut words = before
        .split_whitespace()
        .rev()
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()));
    match words.next() {
        Some("של") => words.next().is_some_and(|w| GAP.contains(&w)),
        Some(w) => GAP.contains(&w),
        None => false,
    }
}

/// Numbers in a sentence that could be scores: not part of a test's name ("ADOS-2",
/// "WPPSI-IV"), not an age or a time ("5:4"), not inside a word.
fn score_numbers(sentence: &str) -> Vec<String> {
    let chars: Vec<char> = sentence.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(c) = chars.get(i) {
        if !c.is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while chars.get(i).is_some_and(|c| {
            c.is_ascii_digit() || (*c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        }) {
            i += 1;
        }
        let at = |j: Option<usize>| j.and_then(|j| chars.get(j)).copied();
        let before = at(start.checked_sub(1));
        let after = chars.get(i).copied();
        let in_name = before == Some('-')
            && at(start.checked_sub(2)).is_some_and(|c| c.is_ascii_alphabetic());
        let glued =
            before.is_some_and(char::is_alphabetic) || after.is_some_and(char::is_alphabetic);
        let time = after == Some(':') || before == Some(':');
        // A date ("12.3.2026") has two points; a score never has more than one (D-145).
        let date = chars
            .get(start..i)
            .is_some_and(|run| run.iter().filter(|c| **c == '.').count() >= 2);
        if !(in_name
            || glued
            || time
            || date
            || age_or_span(&chars, start, i)
            || difference(&chars, start))
        {
            if let Some(run) = chars.get(start..i) {
                out.push(run.iter().collect());
            }
        }
    }
    out
}

/// Every score written in `text` checked against the score sheets she entered, in code (not by
/// the model). A sentence that names a measure and holds a number that is none of the named
/// measures' scores or percentiles gets one line: what is written and what was entered.
#[must_use]
pub fn check_scores(text: &str, sheets: &[ScoreSheet]) -> Vec<String> {
    check_scores_tagged(text, sheets)
        .into_iter()
        .map(|(_, line)| line)
        .collect()
}

/// What a line of [`check_scores`] is about, said where the line is made (D-161), so the
/// page can word and weigh each kind without reading the sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreNoteKind {
    /// A number beside a measure that is not its score in the table.
    Mismatch,
    /// A range word that is not the table's range for the score.
    BandWord,
    /// A percentile that is not the computed one.
    Percentile,
    /// A cut-off that is not the sheet's.
    Cutoff,
}

/// [`check_scores`], each line with its kind.
#[must_use]
pub fn check_scores_tagged(text: &str, sheets: &[ScoreSheet]) -> Vec<(ScoreNoteKind, String)> {
    let mut entered: Vec<(Measure, Known)> = Vec::new();
    for sheet in sheets {
        // Not `checked`: one value an older version saved off the grid must not silence the
        // check of every other score in the sheet.
        let Some((_, entries)) = entries_of(sheet) else {
            continue;
        };
        for (measure, e) in entries {
            entered.push((
                measure.clone(),
                Known {
                    label: measure_label(&measure),
                    value: fmt_value(e.value),
                    percentile: percentile(&measure, e.value),
                    band: classify(&measure, e.value).map(|b| b.label.clone()),
                },
            ));
        }
    }
    // Tests whose sheet has a cut-off (ADOS-2), with it.
    let cutoffs: Vec<(Instrument, Vec<Measure>, f64)> = sheets
        .iter()
        .filter_map(|s| {
            let c = s.cutoff?;
            let (inst, entries) = checked(s).ok()?;
            Some((inst, entries.into_iter().map(|(m, _)| m).collect(), c))
        })
        .collect();
    // The measures of the sheets without a cut-off: "ציון כולל" is CARS-2's and CBCL's too.
    let others: Vec<Measure> = sheets
        .iter()
        .filter(|s| s.cutoff.is_none())
        .filter_map(entries_of)
        .flat_map(|(_, e)| e.into_iter().map(|(m, _)| m))
        .collect();
    let all = instruments();
    let mut out: Vec<(ScoreNoteKind, String)> = Vec::new();
    let mut push = |kind: ScoreNoteKind, line: String| {
        if !out.iter().any(|(_, l)| l == &line) {
            out.push((kind, line));
        }
    };
    for sentence in text
        .split(['\n', '!', '?', ';'])
        .flat_map(|s| s.split(". "))
    {
        let named: Vec<&(Measure, Known)> = entered
            .iter()
            .filter(|(m, _)| mentions(sentence, m))
            .collect();
        // A number of another measure named in the same sentence, by its short name too, is
        // that measure's ("בין מוטורי (97) לבין שפה (68)").
        let also: Vec<&(Measure, Known)> = entered
            .iter()
            .filter(|(m, _)| mentioned_at_all(sentence, m))
            .collect();
        // D-165: the sharper checks, each only where the sentence is about one thing.
        // The cut-off written in a sentence about a test whose sheet has one.
        let cut_written = cutoff_written(sentence);
        let cut_entered = cutoff_about(sentence, &cutoffs, &others, &all);
        if let (Some(n), Some((name, c))) = (&cut_written, &cut_entered) {
            if !same_number(n, &fmt_value(*c)) {
                push(
                    ScoreNoteKind::Cutoff,
                    format!(
                        "כתוב נקודת חתך {n}, ובטבלת הציונים של {name}: {}",
                        fmt_value(*c)
                    ),
                );
            }
        }
        let one = sole(&named, &also);
        let percentiles = percentiles_written(sentence);
        // Exactly one measure named, and named plainly, in a clause ("..., בעוד ש..." starts
        // another): its percentile and its range, read in that clause only.
        let mut percentiles_checked: Vec<String> = Vec::new();
        for (clause, next) in clauses(sentence) {
            let named_here: Vec<&(Measure, Known)> = entered
                .iter()
                .filter(|(m, _)| mentions(clause, m))
                .collect();
            let also_here: Vec<&(Measure, Known)> = entered
                .iter()
                .filter(|(m, _)| mentioned_at_all(clause, m))
                .collect();
            let Some((m, k)) = sole(&named_here, &also_here) else {
                continue;
            };
            if let Some(p) = &k.percentile {
                for n in percentiles_written(clause) {
                    if !same_number(&n, p) {
                        push(
                            ScoreNoteKind::Percentile,
                            format!(
                                "כתוב אחוזון {n} ליד {} ({}), ולפי החישוב: אחוזון {p}",
                                m.name_he, k.value
                            ),
                        );
                    }
                    percentiles_checked.push(n);
                }
            }
            if let Some(line) = band_word_note(clause, next, m, k) {
                push(ScoreNoteKind::BandWord, line);
            }
        }
        if named.is_empty() {
            continue;
        }
        for n in score_numbers(sentence) {
            // The cut-off and a percentile beside one measure had their own check above. A
            // number written as a cut-off is never a score, whichever test it is about.
            if cut_written.as_ref() == Some(&n) {
                continue;
            }
            if percentiles_checked.contains(&n)
                || one.is_some_and(|(_, k)| k.percentile.is_some()) && percentiles.contains(&n)
            {
                continue;
            }
            // A year beside a measure ("נבדק ב-2026") is never a score, so it is not held
            // against the table. Any other number is: an impossible score the model writes
            // ("ציון 950") is pointed out, not passed over.
            if n.len() == 4 && n.parse::<u32>().is_ok_and(|v| (1900..=2100).contains(&v)) {
                continue;
            }
            let fits = named
                .iter()
                .chain(&also)
                .any(|(_, k)| k.value == n || k.percentile.as_deref() == Some(n.as_str()));
            if fits {
                continue;
            }
            let entered: Vec<String> = named
                .iter()
                .map(|(_, k)| match &k.percentile {
                    Some(p) => format!("{} {} (אחוזון {p})", k.label, k.value),
                    None => format!("{} {}", k.label, k.value),
                })
                .collect();
            push(
                ScoreNoteKind::Mismatch,
                format!(
                    "המספר {n} לא תואם את טבלת הציונים: {} – לבדוק",
                    entered.join(", ")
                ),
            );
        }
    }
    out
}

/// The one measure a sentence or clause is about, named plainly and no other named at all.
fn sole<'a>(
    named: &[&'a (Measure, Known)],
    also: &[&'a (Measure, Known)],
) -> Option<&'a (Measure, Known)> {
    match (named, also) {
        ([n], [a]) if n.0.key == a.0.key && n.1.label == a.1.label => Some(*n),
        _ => None,
    }
}

/// Whole words that start a new clause about something else: "ציון 88, בעוד ששאר היכולות
/// בטווח הממוצע". The band-word and percentile checks read each clause on its own.
const CLAUSE_WORDS: &[&str] = &[
    "בעוד",
    "ובעוד",
    "ואילו",
    "לעומת",
    "ולעומת",
    "בעודו",
    "בעודה",
];

/// A sentence split before each of [`CLAUSE_WORDS`], each clause with the word that starts the
/// next one (so a label right before "לעומת" is still read as a comparison).
fn clauses(sentence: &str) -> Vec<(&str, Option<&str>)> {
    let cuts: Vec<(usize, &str)> = word_spans(sentence)
        .into_iter()
        .filter(|(s, _, w)| *s > 0 && CLAUSE_WORDS.contains(w))
        .map(|(s, _, w)| (s, w))
        .collect();
    let mut out = Vec::new();
    let mut start = 0;
    for (at, word) in &cuts {
        out.push((sentence.get(start..*at).unwrap_or_default(), Some(*word)));
        start = *at;
    }
    out.push((sentence.get(start..).unwrap_or_default(), None));
    out
}

/// Two numbers as written ("37", "0.4", ">99.9") that are the same number.
fn same_number(a: &str, b: &str) -> bool {
    let num = |s: &str| s.trim_start_matches(['<', '>']).parse::<f64>().ok();
    match (num(a), num(b)) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

/// Whether `word` is `stem` with at most two of the common prefix letters before it
/// ("באחוזון", "ובאחוזון", "לנקודת").
fn with_prefixes(word: &str, stem: &str) -> bool {
    word.strip_suffix(stem).is_some_and(|pre| {
        pre.chars().count() <= 2
            && pre
                .chars()
                .all(|c| matches!(c, 'ו' | 'ב' | 'ה' | 'ל' | 'מ'))
    })
}

/// The number at the start of `rest`, after spaces, an opening bracket or a dash: "37",
/// "0.4", "<0.1". `None` when something else comes first.
fn leading_number(rest: &str) -> Option<String> {
    let rest =
        rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '(' | ':' | '–' | '='));
    // "ה-37", "ה־37" (maqaf).
    let rest = ["ה-", "ה\u{05be}"]
        .iter()
        .find_map(|p| rest.strip_prefix(p))
        .unwrap_or(rest);
    let sign: String = rest
        .chars()
        .take_while(|c| matches!(c, '<' | '>'))
        .collect();
    let body = rest.get(sign.len()..)?;
    let chars: Vec<char> = body.chars().collect();
    let mut n = String::new();
    let mut i = 0;
    while let Some(c) = chars.get(i) {
        let decimal = *c == '.'
            && !n.is_empty()
            && !n.contains('.')
            && chars.get(i + 1).is_some_and(char::is_ascii_digit);
        if c.is_ascii_digit() || decimal {
            n.push(*c);
            i += 1;
        } else {
            break;
        }
    }
    // "5:4" is an age or a time, "12.3.2026" a date.
    let after = chars.get(i).copied();
    let date_or_time = matches!(after, Some(':' | '/' | '.'))
        && chars.get(i + 1).is_some_and(char::is_ascii_digit);
    if n.is_empty() || date_or_time {
        return None;
    }
    if after.is_some_and(char::is_alphabetic) {
        return None;
    }
    Some(format!("{sign}{n}"))
}

/// Every number written right after the word "אחוזון" ("אחוזון 37", "באחוזון ה-37").
fn percentiles_written(sentence: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = sentence
        .get(at..)
        .and_then(|s| s.find("אחוזון"))
        .map(|i| at + i)
    {
        let end = i + "אחוזון".len();
        at = end;
        let start = sentence
            .get(..i)
            .and_then(|b| {
                b.char_indices()
                    .rev()
                    .find(|(_, c)| !c.is_alphabetic())
                    .map(|(j, c)| j + c.len_utf8())
            })
            .unwrap_or(0);
        let word = sentence.get(start..end).unwrap_or_default();
        if !with_prefixes(word, "אחוזון") {
            continue;
        }
        let rest = sentence.get(end..).unwrap_or_default();
        if rest.starts_with(char::is_alphabetic) {
            continue; // "אחוזונים"
        }
        if let Some(n) = leading_number(rest) {
            out.push(n);
        }
    }
    out
}

/// The number written right after "נקודת החתך" or "נקודת חתך" (also "של", "היא" or "עומדת
/// על" between them). `None` when the words go on otherwise ("נקודת החתך למודול 1").
fn cutoff_written(sentence: &str) -> Option<String> {
    for phrase in ["נקודת החתך", "נקודת חתך"] {
        let Some(i) = sentence.find(phrase) else {
            continue;
        };
        let start = sentence
            .get(..i)
            .and_then(|b| b.rfind(char::is_whitespace))
            .map_or(0, |j| j + 1);
        let word = sentence.get(start..i + "נקודת".len()).unwrap_or_default();
        if !with_prefixes(word, "נקודת") {
            continue;
        }
        let mut rest = sentence.get(i + phrase.len()..).unwrap_or_default();
        if rest.starts_with(char::is_alphabetic) {
            continue;
        }
        // Small words that may come between the phrase and its number.
        for _ in 0..3 {
            let trimmed = rest.trim_start_matches(|c: char| {
                c.is_whitespace() || matches!(c, '(' | ':' | '–' | '=')
            });
            let skip = ["של ", "היא ", "הוא ", "עומדת ", "על "]
                .iter()
                .find_map(|w| trimmed.strip_prefix(w));
            match skip {
                Some(r) => rest = r,
                None => break,
            }
        }
        return leading_number(rest);
    }
    None
}

/// A text upper-cased with spaces and dashes as "-", to find a test's name in it.
fn flat_upper(text: &str) -> String {
    text.to_ascii_uppercase()
        .chars()
        .map(|c| if c == ' ' || c == '–' { '-' } else { c })
        .collect()
}

/// Whether `flat` (see [`flat_upper`]) names the test: its name ("ADOS-2"), its family of four
/// letters or more ("ADOS"), or its key ("ADOS_2").
fn instrument_named(flat: &str, inst: &Instrument) -> bool {
    let name = inst.name.to_ascii_uppercase().replace(' ', "-");
    let family = name.split(['-', '(']).next().unwrap_or_default();
    let key = inst.key.to_ascii_uppercase();
    flat.contains(&name)
        || (family.chars().count() >= 4 && flat.contains(family))
        || flat.contains(&key)
        || flat.contains(&key.replace('_', "-"))
}

/// The test a sentence is about, among those with a cut-off, and that cut-off. A sentence
/// that names tests is about the named ones only, and only when every test it names has a
/// cut-off sheet; a sentence that names none is about the one cut-off sheet of the case, when
/// there is exactly one, the sentence names one of its scores, and none of another sheet's
/// ("הציון הכולל" is CARS-2's too). `None` otherwise, or for two with different cut-offs.
fn cutoff_about(
    sentence: &str,
    cutoffs: &[(Instrument, Vec<Measure>, f64)],
    others: &[Measure],
    all: &[Instrument],
) -> Option<(String, f64)> {
    let flat = flat_upper(sentence);
    let named: Vec<&Instrument> = all.iter().filter(|i| instrument_named(&flat, i)).collect();
    let about: Vec<(String, f64)> = if named.is_empty() {
        match cutoffs {
            [(inst, measures, c)]
                if measures.iter().any(|m| mentioned_at_all(sentence, m))
                    && !others.iter().any(|m| mentioned_at_all(sentence, m)) =>
            {
                vec![(inst.name.clone(), *c)]
            }
            _ => Vec::new(),
        }
    } else {
        if named
            .iter()
            .any(|n| !cutoffs.iter().any(|(i, _, _)| i.key == n.key))
        {
            return None;
        }
        cutoffs
            .iter()
            .filter(|(i, _, _)| named.iter().any(|n| n.key == i.key))
            .map(|(inst, _, c)| (inst.name.clone(), *c))
            .collect()
    };
    match about.as_slice() {
        [first, rest @ ..] if rest.iter().all(|(_, c)| (c - first.1).abs() < 1e-9) => {
            Some(first.clone())
        }
        _ => None,
    }
}

/// Words that negate a range label a few words after them: "לא בטווח הממוצע".
const NEGATIONS: &[&str] = &["לא", "ולא", "אינו", "אינה", "אינם", "אינן", "אין"];
/// Words right before a range label that make it a comparison: "מתחת לממוצע".
const NOT_A_RANGE_BEFORE: &[&str] = &[
    "מתחת",
    "מעל",
    "סביב",
    "קרוב",
    "קרובה",
    "בגבול",
    "גבול",
    "לעומת",
    "יותר",
    "פחות",
    // "ביחס לממוצע", "בהשוואה לממוצע", "יחסית לממוצע": the sheet's own wording for a
    // relative strength.
    "ביחס",
    "וביחס",
    "בהשוואה",
    "ובהשוואה",
    "השוואה",
    "יחסית",
    "בניגוד",
    "מעבר",
];
/// Words after a range label that make it a comparison: "גבוה יחסית", "ממוצע של".
const NOT_A_RANGE_AFTER: &[&str] = &[
    "יחסית",
    "יותר",
    "של",
    "מאשר",
    "לעומת",
    "ולעומת",
    "ביחס",
    "בהשוואה",
];

/// A label word and its feminine and plural forms ("ממוצע", "ממוצעת", "ממוצעים"; "נמוך",
/// "נמוכה"): the same range word.
fn label_forms(word: &str) -> Vec<String> {
    let mut chars: Vec<char> = word.chars().collect();
    let mut out = vec![word.to_owned()];
    let Some(last) = chars.last_mut() else {
        return out;
    };
    *last = match *last {
        'ך' => 'כ',
        'ם' => 'מ',
        'ן' => 'נ',
        'ף' => 'פ',
        'ץ' => 'צ',
        c => c,
    };
    let base: String = chars.into_iter().collect();
    out.extend(["ה", "ת", "ים", "ות"].iter().map(|s| format!("{base}{s}")));
    out
}

/// The words of a label or a sentence, a hyphen or a maqaf between two words being a space
/// ("ממוצע-נמוך", "ממוצע־נמוך" is "ממוצע נמוך").
fn label_words(text: &str) -> Vec<&str> {
    sentence_words(text)
        .into_iter()
        .flat_map(|w| w.split('-'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Range words that give a direction: a label that adds one to the written one ("ממוצע" for
/// "ממוצע נמוך") says another range, one that adds only a degree ("גבוה" for "גבוה מאוד",
/// "נמוך" for "נמוך במידה מתונה") says the same range more briefly.
const DIRECTION_WORDS: &[&str] = &["נמוך", "גבוה", "ממוצע", "גבולי", "תקין", "מוגבר"];

/// Whether `written` is a shorter wording of `band`: the band's words start with the written
/// ones and what follows only says how much.
fn shorter_wording(written: &str, band: &str) -> bool {
    let (w, b) = (label_words(written), label_words(band));
    b.len() > w.len()
        && b.iter().zip(&w).all(|(x, y)| x == y)
        && b.iter().skip(w.len()).all(|x| !DIRECTION_WORDS.contains(x))
}

/// A range label of the measure's own table written in a sentence (or clause) about that one
/// measure and its score, other than the score's range in the table (D-165): one line, or
/// `None`. Only when exactly one label is written, read longest first ("ממוצע נמוך" is not
/// "ממוצע"), in any of its forms ("ממוצעת", "ממוצע-נמוך"), with the common prefixes ("בטווח
/// הממוצע", "ולממוצע"); a label in a comparison ("גבוה מהצפוי", "מתחת לממוצע", "ביחס
/// לממוצע") is not one, nor a shorter wording of the right one ("גבוה" for "גבוה מאוד").
/// `next` is the word that starts the next clause, if any.
fn band_word_note(sentence: &str, next: Option<&str>, m: &Measure, k: &Known) -> Option<String> {
    let band = k.band.as_deref()?;
    if !score_numbers(sentence).contains(&k.value) {
        return None;
    }
    let words = label_words(sentence);
    let mut labels: Vec<&str> = m.bands.iter().map(|b| b.label.as_str()).collect();
    labels.sort_by_key(|l| {
        (
            std::cmp::Reverse(label_words(l).len()),
            std::cmp::Reverse(l.chars().count()),
        )
    });
    let mut taken = vec![false; words.len()];
    let mut found: Vec<&str> = Vec::new();
    for label in labels {
        let parts: Vec<Vec<String>> = label_words(label).into_iter().map(label_forms).collect();
        let n = parts.len();
        if n == 0 {
            continue;
        }
        for start in 0..words.len().saturating_sub(n - 1) {
            let Some(win) = words.get(start..start + n) else {
                continue;
            };
            if taken
                .get(start..start + n)
                .is_some_and(|t| t.iter().any(|x| *x))
            {
                continue;
            }
            let fits = win.iter().zip(&parts).enumerate().all(|(j, (w, forms))| {
                forms.iter().any(|p| {
                    if j == 0 {
                        *w == p || with_prefixes(w, p) && !w.starts_with('מ')
                    } else {
                        *w == p || w.strip_prefix('ה') == Some(p.as_str())
                    }
                })
            });
            if !fits {
                continue;
            }
            for t in taken.iter_mut().skip(start).take(n) {
                *t = true;
            }
            // A negation up to three words before ("לא בטווח הממוצע"), a comparison right
            // before or after.
            let before = start.checked_sub(1).and_then(|j| words.get(j)).copied();
            let after = words.get(start + n).copied().or(next);
            let negated = words
                .get(start.saturating_sub(3)..start)
                .is_some_and(|w| w.iter().any(|w| NEGATIONS.contains(w)));
            let comparison = negated
                || before.is_some_and(|b| NOT_A_RANGE_BEFORE.contains(&b))
                || after.is_some_and(|a| {
                    NOT_A_RANGE_AFTER.contains(&a)
                        || (a.starts_with('מ') && !a.chars().all(|c| c.is_ascii_digit()))
                });
            if !comparison && !found.contains(&label) {
                found.push(label);
            }
        }
    }
    match found.as_slice() {
        [written] if *written != band && !shorter_wording(written, band) => Some(format!(
            "כתוב \"{written}\" ליד {} ({}), ולפי הטבלה: {band}",
            m.name_he, k.value
        )),
        _ => None,
    }
}

// ------------------------------------------------------------ scores in a past report (round 32)

/// Where a score stands among the child's own subtest scores in the same report: three
/// scaled-score points or more from their mean (the Wechsler convention, pending her approval).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Relative {
    Strength,
    Weakness,
}

impl Relative {
    #[must_use]
    pub fn label_he(self) -> &'static str {
        match self {
            Relative::Strength => "חוזקה יחסית",
            Relative::Weakness => "חולשה יחסית",
        }
    }
}

/// The distance from the child's subtest mean that counts as a relative strength or weakness.
pub const RELATIVE_POINTS: f64 = 3.0;

/// A score found in a text, beside the name of the test or subtest it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreMention {
    pub instrument: String,
    pub measure: String,
    pub value: f64,
    /// Byte range of the number in the text.
    pub start: usize,
    pub end: usize,
    pub band: Option<String>,
    pub relative: Option<Relative>,
}

/// Scales whose scores are tagged: the others' range labels are long sentences, and a raw
/// cut-off score means nothing without its module.
fn taggable(scale: ScoreScale) -> bool {
    matches!(
        scale,
        ScoreScale::Standard
            | ScoreScale::Scaled
            | ScoreScale::VScale
            | ScoreScale::TAbility
            | ScoreScale::TConcern
    )
}

/// The instruments a text names, best first: the exact name ("WPPSI-IV") before its family
/// ("WPPSI"). All of them when it names none.
fn named_instruments(text: &str, all: &[Instrument]) -> Vec<Instrument> {
    let flat = flat_upper(text);
    let mut scored: Vec<(u8, &Instrument)> = all
        .iter()
        .map(|i| {
            let name = i.name.to_ascii_uppercase().replace(' ', "-");
            let family = name.split(['-', '(']).next().unwrap_or_default().to_owned();
            let score = if flat.contains(&name) {
                2
            } else if family.chars().count() >= 4 && flat.contains(&family) {
                1
            } else {
                0
            };
            (score, i)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    if scored.is_empty() {
        return all.to_vec();
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, i)| i.clone()).collect()
}

/// Abbreviations of measures that reports also use to mark a score's kind ("SS 7" for a
/// scaled score, "T 63" for a T score): beside a Hebrew name they are not the measure.
const SCORE_MARKER_ABBRS: &[&str] = &["SS", "T", "IQ", "SD", "Z"];

/// Byte ranges of the sentences of a text, split as [`check_scores`] splits them.
fn sentence_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in bytes.iter().enumerate() {
        let next = bytes.get(k + 1).map(|(_, c)| *c);
        let ends = matches!(c, '\n' | '!' | '?' | ';') || (c == '.' && next == Some(' '));
        if ends {
            out.push((start, i));
            start = i + c.len_utf8();
        }
    }
    out.push((start, text.len()));
    out
}

/// Words of a sentence with their byte ranges, as [`mentions`] reads them.
fn word_spans(sentence: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let keep = |c: char| c.is_alphanumeric() || c == '-' || c == '"' || c == '\'';
    for (i, c) in sentence
        .char_indices()
        .chain(std::iter::once((sentence.len(), ' ')))
    {
        match (keep(c) && i < sentence.len(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if let Some(w) = sentence.get(s..i) {
                    out.push((s, i, w));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Where in `words` a measure is named: the byte range of each mention.
fn mention_spans(words: &[(usize, usize, &str)], measure: &Measure) -> Vec<(usize, usize)> {
    let plain: Vec<&str> = words.iter().map(|(_, _, w)| *w).collect();
    let mut out = Vec::new();
    if measure.abbr.chars().count() >= 2 {
        for (s, e, w) in words {
            if *w == measure.abbr {
                out.push((*s, *e));
            }
        }
    }
    for name in
        std::iter::once(measure.name_he.as_str()).chain(measure.aliases.iter().map(String::as_str))
    {
        let n = name.split_whitespace().count();
        let long = n >= 2 || name.chars().count() >= 5;
        for k in named_at(&plain, name) {
            // A short name ("מידע", "שפה") is a common word too: only right before a number.
            let next_is_number = plain
                .get(k + n)
                .is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()));
            if !long && !next_is_number {
                continue;
            }
            if let (Some(a), Some(b)) = (words.get(k), words.get(k + n - 1)) {
                out.push((a.0, b.1));
            }
        }
    }
    out
}

/// Numbers in a sentence that could be scores, with their byte ranges (see [`score_numbers`]),
/// leaving out a percentile ("אחוזון 79", "79%").
fn score_number_spans(sentence: &str) -> Vec<(usize, usize, f64)> {
    let words = word_spans(sentence);
    let mut out = Vec::new();
    for (k, (s, e, w)) in words.iter().enumerate() {
        let digits = w.chars().all(|c| c.is_ascii_digit());
        if !digits {
            continue;
        }
        let rest = sentence.get(*e..).unwrap_or_default();
        let before = sentence.get(..*s).unwrap_or_default();
        // A decimal: "29.5".
        let (end, text) = match rest.strip_prefix('.').and_then(|r| {
            let d: String = r.chars().take_while(char::is_ascii_digit).collect();
            (!d.is_empty()).then_some(d)
        }) {
            Some(d) => (*e + 1 + d.len(), format!("{w}.{d}")),
            None => (*e, (*w).to_owned()),
        };
        let after = sentence.get(end..).unwrap_or_default();
        let percent = after.starts_with('%')
            || k.checked_sub(1)
                .and_then(|j| words.get(j))
                .is_some_and(|(_, _, p)| p.contains("אחוזון"));
        let time = before.ends_with(':') || after.starts_with(':') || after.starts_with('/');
        // "28.1.2019", "27.6.2018": a date, not a score of 28.1 (the review of stage 4).
        let next_digit = |sep: char| {
            after
                .strip_prefix(sep)
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        };
        let date =
            before.ends_with('/') || before.ends_with('.') || next_digit('.') || next_digit('/');
        if percent || time || date {
            continue;
        }
        if let Ok(v) = text.parse::<f64>() {
            out.push((*s, end, v));
        }
    }
    out
}

/// Every score a past report gives beside the name of a test or subtest: the instrument it
/// names (or the first that has the measure), the measure, the value within its scale, its
/// range from the table, and where it stands among the same report's subtest scores. Local
/// only; the text is never sent for this.
#[must_use]
pub fn find_score_mentions(text: &str) -> Vec<ScoreMention> {
    let all = instruments();
    let candidates = named_instruments(text, &all);
    let mut found: Vec<(ScoreMention, ScoreScale)> = Vec::new();
    for (from, to) in sentence_spans(text) {
        let Some(sentence) = text.get(from..to) else {
            continue;
        };
        let words = word_spans(sentence);
        // Every mention in the sentence: (where it ends, instrument, measure).
        let mut named_here: Vec<(usize, usize, &Instrument, &Measure)> = Vec::new();
        for inst in &candidates {
            for m in inst.measures.iter().filter(|m| taggable(m.scale)) {
                for (s, e) in mention_spans(&words, m) {
                    // The same name in two instruments: the first one named wins.
                    if !named_here
                        .iter()
                        .any(|(a, b, _, x)| (*a, *b) == (s, e) && x.key == m.key)
                    {
                        named_here.push((s, e, inst, m));
                    }
                }
            }
        }
        if named_here.is_empty() {
            continue;
        }
        // A longer name inside which a shorter one was found wins ("אוצר מילים רצפטיבי").
        let inner: Vec<bool> = named_here
            .iter()
            .map(|(s, e, _, _)| {
                named_here
                    .iter()
                    .any(|(s2, e2, _, _)| s2 <= s && e2 >= e && (e2 - s2) > (e - s))
            })
            .collect();
        let named_here: Vec<_> = named_here
            .into_iter()
            .zip(inner)
            .filter(|(_, inside)| !inside)
            .map(|(n, _)| n)
            .collect();
        for (ns, ne, v) in score_number_spans(sentence) {
            // An abbreviation that is also how a score is marked ("SS 7", "T 63"), right
            // before the number: a Hebrew name before it names the measure ("בקוביות SS 7").
            let marker = |s: usize, e: usize, m: &Measure| {
                SCORE_MARKER_ABBRS.contains(&m.abbr.as_str())
                    && sentence.get(s..e) == Some(m.abbr.as_str())
                    && sentence.get(e..ns).is_some_and(|gap| gap.trim().is_empty())
            };
            // The nearest mention before the number; after a score word, the one after it.
            let before = named_here
                .iter()
                .filter(|(s, e, _, m)| *e <= ns && !marker(*s, *e, m))
                .max_by_key(|(_, e, _, _)| *e)
                .or_else(|| {
                    named_here
                        .iter()
                        .filter(|(_, e, _, _)| *e <= ns)
                        .max_by_key(|(_, e, _, _)| *e)
                });
            // After it only as "ציון 7 בקוביות": a number before any name ("בן 6, קוביות")
            // is something else.
            let said_score = words
                .iter()
                .rev()
                .find(|(_, e, _)| *e <= ns)
                .is_some_and(|(_, _, w)| w.ends_with("ציון") || *w == "ציונו" || *w == "ציונה");
            let after = named_here
                .iter()
                .filter(|(s, _, _, _)| *s >= ne && said_score)
                .min_by_key(|(s, _, _, _)| *s);
            // "ציון 13 במושגים בתמונות": after a score word, a name right after the number is
            // its own ("קיבל ציון 7 ובמטריצות 13" keeps 7 with the name before it).
            // "ובמטריצות" opens the next item.
            let next_word = words
                .iter()
                .find(|(s, _, _)| *s >= ne)
                .filter(|(_, _, w)| !w.starts_with('ו'))
                .map(|(s, _, _)| *s);
            let right_after = after.filter(|(s, _, _, _)| Some(*s) == next_word);
            let nearest = right_after.or(before).or(after);
            let Some(&(ms, me, inst, m)) = nearest else {
                continue;
            };
            // Every candidate at that same place, the first whose scale holds the value.
            // Every scale that becomes a tag is in whole numbers: "28.1" is never one of them.
            let fits = |m: &Measure| v >= m.min && v <= m.max && (v - v.round()).abs() < 1e-9;
            let pick = named_here
                .iter()
                .filter(|(s, e, _, _)| (*s, *e) == (ms, me))
                .find(|(_, _, _, x)| fits(x))
                .map(|(_, _, i, x)| (*i, *x))
                .or_else(|| fits(m).then_some((inst, m)));
            let Some((inst, m)) = pick else { continue };
            found.push((
                ScoreMention {
                    instrument: inst.key.clone(),
                    measure: m.key.clone(),
                    value: v,
                    start: from + ns,
                    end: from + ne,
                    band: classify(m, v).map(|b| b.label.clone()),
                    relative: None,
                },
                m.scale,
            ));
        }
    }
    // Relative place among the report's subtests of one instrument, three or more of them.
    let groups: Vec<String> = found.iter().map(|(f, _)| f.instrument.clone()).collect();
    for inst in groups {
        let mut seen: Vec<(&str, f64)> = Vec::new();
        for (f, scale) in &found {
            if f.instrument == inst
                && matches!(scale, ScoreScale::Scaled | ScoreScale::VScale)
                && !seen.iter().any(|(k, _)| *k == f.measure)
            {
                seen.push((&f.measure, f.value));
            }
        }
        if seen.len() < 3 {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let mean = seen.iter().map(|(_, v)| v).sum::<f64>() / seen.len() as f64;
        for (f, scale) in &mut found {
            if f.instrument == inst && matches!(scale, ScoreScale::Scaled | ScoreScale::VScale) {
                f.relative = if f.value - mean >= RELATIVE_POINTS {
                    Some(Relative::Strength)
                } else if mean - f.value >= RELATIVE_POINTS {
                    Some(Relative::Weakness)
                } else {
                    None
                };
            }
        }
    }
    found.into_iter().map(|(f, _)| f).collect()
}

/// The tag a score becomes in a neutralized past report: `[תוצאה 9 · ממוצע · חולשה יחסית]`.
#[must_use]
pub fn score_tag(m: &ScoreMention) -> String {
    let mut parts = vec![format!("תוצאה {}", fmt_value(m.value))];
    parts.extend(m.band.clone());
    parts.extend(m.relative.map(|r| r.label_he().to_owned()));
    let tag = format!("[{}]", parts.join(" · "));
    // The gate reads a tag as at most 40 characters inside the brackets.
    if tag.chars().count() <= 42 {
        tag
    } else {
        format!("[תוצאה {}]", fmt_value(m.value))
    }
}

/// Every range label of the fixed tables, once.
static BAND_LABELS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
    let mut out: Vec<String> = instruments()
        .into_iter()
        .flat_map(|i| i.measures)
        .flat_map(|m| m.bands)
        .map(|b| b.label)
        .collect();
    out.sort();
    out.dedup();
    out
});

/// A score tag as [`score_tag`] writes it, and nothing else: a score from 0 to 200, then only
/// a range label of the fixed tables and a relative place. Free words never pass, so the
/// tag cannot carry a name.
#[must_use]
pub fn is_score_tag(tag: &str) -> bool {
    parse_score_tag(tag).is_some()
}

/// The ceiling of a score tag: no scale or percentile reaches beyond 200 (BRIEF-P's 120).
const MAX_SCORE: f64 = 200.0;

/// What a score tag holds: the value, the range label and the relative place. `None` for
/// anything that is not a score tag (see [`is_score_tag`]).
#[must_use]
pub fn parse_score_tag(tag: &str) -> Option<(f64, Option<String>, Option<Relative>)> {
    let inner = tag.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = inner.split(" · ");
    let value = parts.next()?.strip_prefix("תוצאה ")?;
    let v = value.parse::<f64>().ok()?;
    if !(0.0..=MAX_SCORE).contains(&v) || fmt_value(v) != value {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let labels = &*BAND_LABELS;
    let band = |p: &str| labels.iter().any(|l| l == p).then(|| p.to_owned());
    let rel = |p: &str| {
        [Relative::Strength, Relative::Weakness]
            .into_iter()
            .find(|r| r.label_he() == p)
    };
    match rest.as_slice() {
        [] => Some((v, None, None)),
        [a] => match (band(a), rel(a)) {
            (Some(b), _) => Some((v, Some(b), None)),
            (None, Some(r)) => Some((v, None, Some(r))),
            (None, None) => None,
        },
        [a, b] => Some((v, Some(band(a)?), Some(rel(b)?))),
        _ => None,
    }
}

/// A score tag in a neutralized report, read back with the test or subtest named beside it
/// (round 33): what the interpretation of her past reports is grouped by.
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedScore {
    pub instrument: String,
    pub measure: String,
    /// Byte range of the whole tag.
    pub start: usize,
    pub end: usize,
    /// The score itself, for the patterns of the report's profile (D-100).
    pub value: f64,
    pub band: Option<String>,
    pub relative: Option<Relative>,
}

/// Every score tag in `text` whose test or subtest is named beside it, the way
/// [`find_score_mentions`] read the number before it became a tag.
#[must_use]
pub fn find_tagged_scores(text: &str) -> Vec<TaggedScore> {
    let mut tags = Vec::new();
    let mut at = 0;
    while let Some(open) = text.get(at..).and_then(|t| t.find('[')).map(|o| at + o) {
        let Some(close) = text
            .get(open..)
            .and_then(|t| t.find(']'))
            .map(|c| open + c + 1)
        else {
            break;
        };
        if let Some(parsed) = text.get(open..close).and_then(parse_score_tag) {
            tags.push((open, close, parsed));
        }
        at = close;
    }
    if tags.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<TaggedScore> = Vec::new();
    for m in find_score_mentions(text) {
        let Some((s, e, (value, band, relative))) =
            tags.iter().find(|(s, e, _)| *s <= m.start && m.end <= *e)
        else {
            continue;
        };
        if out.iter().any(|t| t.start == *s) {
            continue;
        }
        out.push(TaggedScore {
            instrument: m.instrument,
            measure: m.measure,
            start: *s,
            end: *e,
            value: *value,
            band: band.clone(),
            relative: *relative,
        });
    }
    out
}

/// What an interpretation of a measure can be about: each range label of its table, a
/// relative strength or weakness (for subtests), or the measure in general.
#[must_use]
pub fn interpretation_cases(instrument: &str, measure: &str) -> Vec<String> {
    let Some(m) = instruments()
        .into_iter()
        .find(|i| i.key == instrument)
        .and_then(|i| i.measures.into_iter().find(|m| m.key == measure))
    else {
        return Vec::new();
    };
    let mut out = vec![GENERAL_CASE.to_owned()];
    out.extend(m.bands.iter().map(|b| b.label.clone()));
    if matches!(m.scale, ScoreScale::Scaled | ScoreScale::VScale) {
        out.push(Relative::Strength.label_he().to_owned());
        out.push(Relative::Weakness.label_he().to_owned());
    }
    out
}

/// The case of an interpretation that holds whatever the score.
pub const GENERAL_CASE: &str = "כללי";

/// One score of a case and the interpretation cases it fits: the measure in general, its range
/// in the table, and for a subtest a relative strength or weakness against the child's own
/// subtest mean (three subtests or more, [`RELATIVE_POINTS`] away).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreCases {
    pub instrument: String,
    pub measure: String,
    /// "WPPSI-IV · קוביות".
    pub label: String,
    /// The range label, when the table has one.
    pub band: Option<String>,
    pub cases: Vec<String>,
}

/// The interpretation cases each score of `sheet` fits. A sheet of an unknown instrument is
/// none; an entry of an unknown measure is left out (one value off the grid in a sheet an
/// older version saved does not take the cases away from the others).
#[must_use]
pub fn sheet_cases(sheet: &ScoreSheet) -> Vec<ScoreCases> {
    let Some((inst, entries)) = entries_of(sheet) else {
        return Vec::new();
    };
    let scaled: Vec<f64> = entries
        .iter()
        .filter(|(m, _)| matches!(m.scale, ScoreScale::Scaled | ScoreScale::VScale))
        .map(|(_, e)| e.value)
        .collect();
    #[allow(clippy::cast_precision_loss)]
    let mean = (scaled.len() >= 3).then(|| scaled.iter().sum::<f64>() / scaled.len() as f64);
    entries
        .iter()
        .map(|(m, e)| {
            let band = classify(m, e.value).map(|b| b.label.clone());
            let mut cases = vec![GENERAL_CASE.to_owned()];
            cases.extend(band.clone());
            // A sheet is one instrument, so its subtests share one scale (as in a report).
            if let (ScoreScale::Scaled | ScoreScale::VScale, Some(mean)) = (m.scale, mean) {
                if e.value - mean >= RELATIVE_POINTS {
                    cases.push(Relative::Strength.label_he().to_owned());
                } else if mean - e.value >= RELATIVE_POINTS {
                    cases.push(Relative::Weakness.label_he().to_owned());
                }
            }
            ScoreCases {
                instrument: inst.key.clone(),
                measure: m.key.clone(),
                label: format!("{} · {}", inst.name, m.name_he),
                band,
                cases,
            }
        })
        .collect()
}

/// What she leaves out beside scores in her reports (K12), so the scores material does not
/// send what she does not write. Learned from the score tags of her past reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetHabits {
    /// No percentile beside a subtest's score (scaled scores).
    pub no_subtest_percentiles: bool,
    /// No percentile beside an index's score (standard and T scores).
    pub no_index_percentiles: bool,
}

/// Fewest reports with tagged scores of a kind before what she leaves out is learned.
const HABIT_REPORTS: usize = 3;
/// … and fewest tagged scores of the kind in all.
const HABIT_SCORES: usize = 8;
/// A percentile she writes beside less than this share of the scores is one she leaves out.
const HABIT_SHARE: f64 = 0.1;

/// Whether the sentence around the tag at `start..end` of `text` has a percentile in it.
fn percentile_near(text: &str, start: usize, end: usize) -> bool {
    let is_end = |c: char| matches!(c, '.' | '\n' | ';' | '!' | '?');
    let from = text
        .get(..start)
        .and_then(|t| t.rfind(is_end))
        .map_or(0, |i| i + 1);
    let to = text
        .get(end..)
        .and_then(|t| t.find(is_end))
        .map_or(text.len(), |i| end + i);
    text.get(from..to).is_some_and(|s| s.contains("אחוזון"))
}

/// What she leaves out beside scores, from the text of her past reports (with score tags).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn sheet_habits<'a>(reports: impl IntoIterator<Item = &'a str>) -> SheetHabits {
    let all = instruments();
    let scale = |inst: &str, measure: &str| {
        all.iter()
            .find(|i| i.key == inst)
            .and_then(|i| i.measures.iter().find(|m| m.key == measure))
            .map(|m| m.scale)
    };
    // (reports, scores, with a percentile) for subtests and for indexes.
    let mut sub = (0usize, 0usize, 0usize);
    let mut idx = (0usize, 0usize, 0usize);
    for text in reports {
        let (mut in_sub, mut in_idx) = (false, false);
        for t in find_tagged_scores(text) {
            let near = percentile_near(text, t.start, t.end);
            let kind = match scale(&t.instrument, &t.measure) {
                Some(ScoreScale::Scaled | ScoreScale::VScale) => {
                    in_sub = true;
                    &mut sub
                }
                Some(ScoreScale::Standard | ScoreScale::TAbility) => {
                    in_idx = true;
                    &mut idx
                }
                _ => continue,
            };
            kind.1 += 1;
            kind.2 += usize::from(near);
        }
        sub.0 += usize::from(in_sub);
        idx.0 += usize::from(in_idx);
    }
    let leaves_out = |(reports, scores, with): (usize, usize, usize)| {
        reports >= HABIT_REPORTS
            && scores >= HABIT_SCORES
            && (with as f64) < HABIT_SHARE * scores as f64
    };
    SheetHabits {
        no_subtest_percentiles: leaves_out(sub),
        no_index_percentiles: leaves_out(idx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents_groups_come_from_the_fixed_tables() {
        let entry = |measure: &str, value: f64| ScoreEntry {
            measure: measure.into(),
            value,
            note: String::new(),
        };
        let sheet = ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                entry("fsiq", 110.0),
                entry("fsiq", 109.0),
                entry("fsiq", 89.0),
                entry("block_design", 13.0),
                entry("block_design", 8.0),
                entry("block_design", 7.0),
            ],
            notes: String::new(),
        };
        let groups: Vec<ParentGroup> = sheet_bands(&sheet)
            .unwrap()
            .iter()
            .map(|b| b.group)
            .collect();
        use ParentGroup::{Harder, InRange, Strong};
        assert_eq!(groups, [Strong, InRange, Harder, Strong, InRange, Harder]);
        // Every ability table: the band that holds the mean is "in range", no strong band
        // holds it, and every band marked mild or worse is "harder".
        for inst in instruments() {
            for m in inst.measures {
                let mean = match m.scale {
                    ScoreScale::Standard => 100.0,
                    ScoreScale::Scaled => 10.0,
                    ScoreScale::VScale => 15.0,
                    ScoreScale::TAbility => 50.0,
                    _ => continue,
                };
                let at_mean = classify(&m, mean).unwrap();
                assert!(at_mean.level == 0 && at_mean.min <= mean, "{}", m.key);
            }
        }
        // Questionnaires are left out.
        let asrs = ScoreSheet {
            instrument: "cars_2".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![entry("total", 32.0)],
            notes: String::new(),
        };
        assert!(sheet_bands(&asrs).unwrap().is_empty());
    }

    /// "SS" marks a scaled score as often as it names Symbol Search: beside a Hebrew name the
    /// name is the measure; alone it is still the subtest.
    #[test]
    fn a_score_marker_beside_a_hebrew_name_does_not_take_the_score() {
        let got = |t: &str| -> Vec<(String, f64)> {
            find_score_mentions(t)
                .into_iter()
                .map(|m| (m.measure, m.value))
                .collect()
        };
        assert_eq!(
            got("בקוביות SS 7, נמוך מהצפוי."),
            vec![("block_design".to_owned(), 7.0)]
        );
        assert_eq!(
            got("במטריצות T 13 ובזיכרון תמונות SS 9."),
            vec![
                ("matrix".to_owned(), 13.0),
                ("picture_memory".to_owned(), 9.0)
            ]
        );
        assert_eq!(got("SS 7 בלבד."), vec![("symbol_search".to_owned(), 7.0)]);
    }

    #[test]
    fn scores_in_a_past_report_are_found_beside_their_subtest_with_their_place() {
        let text = "במבחן WPPSI-IV הושגה מנת משכל כללית של 98. בקוביות (7) ובמטריצות (13) \
                    הופיע פער. זיכרון תמונות 10, חיפוש חרקים 11.\nקידוד חיות: 6, אחוזון 9. \
                    בגיל 5:4 נבדק ב-12.3.2023. שיחק עם 3 בובות.";
        let found = find_score_mentions(text);
        let got: Vec<(&str, f64, Option<Relative>)> = found
            .iter()
            .map(|f| (f.measure.as_str(), f.value, f.relative))
            .collect();
        assert_eq!(
            got,
            vec![
                ("fsiq", 98.0, None),
                ("block_design", 7.0, None),
                ("matrix", 13.0, Some(Relative::Strength)),
                ("picture_memory", 10.0, None),
                ("bug_search", 11.0, None),
                ("animal_coding", 6.0, Some(Relative::Weakness)),
            ]
        );
        assert!(found.iter().all(|f| f.instrument == "wppsi_iv"));
        let six = found.iter().find(|f| f.value == 6.0).unwrap();
        assert_eq!(text.get(six.start..six.end), Some("6"));
        assert_eq!(score_tag(six), "[תוצאה 6 · ממוצע נמוך · חולשה יחסית]");
        assert!(is_score_tag(&score_tag(six)));
    }

    #[test]
    fn a_score_out_of_its_scale_or_named_otherwise_is_not_a_score() {
        // 25 is no scaled score; WISC-V coding goes by "קידוד" too.
        let found = find_score_mentions("ב-WISC-V: קוביות 25. קידוד 8.");
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].instrument.as_str(), found[0].measure.as_str()),
            ("wisc_v", "coding")
        );
        assert!(find_score_mentions("הילד בן 6 ואוהב קוביות").is_empty());
        let after = find_score_mentions("הושג ציון 7 בקוביות.");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].measure, "block_design");
        let both = find_score_mentions("בקוביות קיבל ציון 7 ובמטריצות 13.");
        let keys: Vec<&str> = both.iter().map(|m| m.measure.as_str()).collect();
        assert_eq!(keys, ["block_design", "matrix"]);
        // A short name is a common word: only right before its number.
        let short = find_score_mentions("WPPSI-IV: מידע 11. היה לו מידע רב על 12 חיות.");
        assert_eq!(short.len(), 1);
        assert_eq!(short[0].value, 11.0);
    }

    #[test]
    fn a_score_tag_is_read_back_with_its_subtest() {
        let text = "WPPSI-IV. בקוביות ([תוצאה 7 · ממוצע נמוך]) עבד לאט, ובמטריצות \
                    [תוצאה 13 · ממוצע גבוה · חוזקה יחסית]. [תוצאה 9] בלי שם. בגיל [מספר].";
        let got: Vec<(String, Option<String>, Option<Relative>)> = find_tagged_scores(text)
            .into_iter()
            .map(|t| (t.measure, t.band, t.relative))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "block_design".to_owned(),
                    Some("ממוצע נמוך".to_owned()),
                    None
                ),
                (
                    "matrix".to_owned(),
                    Some("ממוצע גבוה".to_owned()),
                    Some(Relative::Strength)
                ),
            ]
        );
        let t = &find_tagged_scores(text)[0];
        assert_eq!(text.get(t.start..t.end), Some("[תוצאה 7 · ממוצע נמוך]"));
        assert_eq!(
            parse_score_tag("[תוצאה 6 · חולשה יחסית]"),
            Some((6.0, None, Some(Relative::Weakness)))
        );
        let cases = interpretation_cases("wppsi_iv", "block_design");
        assert!(
            cases.contains(&GENERAL_CASE.to_owned()) && cases.contains(&"ממוצע נמוך".to_owned())
        );
        assert!(cases.contains(&"חולשה יחסית".to_owned()));
        assert!(!interpretation_cases("wppsi_iv", "fsiq").contains(&"חולשה יחסית".to_owned()));
        assert!(interpretation_cases("wppsi_iv", "nothing").is_empty());
    }

    #[test]
    fn a_percentile_she_never_writes_beside_subtests_is_left_out_of_the_sheet() {
        // Subtests without percentiles; the index with one.
        let report = "WPPSI-IV. בקוביות [תוצאה 7 · ממוצע נמוך], ובמטריצות [תוצאה 13 · ממוצע גבוה]. \
                      בזיכרון תמונות [תוצאה 10 · ממוצע]. מנת משכל כללית [תוצאה 98 · ממוצע], אחוזון [מספר].";
        let h = sheet_habits([report; 3]);
        assert!(h.no_subtest_percentiles);
        assert!(!h.no_index_percentiles, "she writes it beside the index");
        assert_eq!(
            sheet_habits([report; 2]),
            SheetHabits::default(),
            "two reports are not a habit"
        );
        let sheet = ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                ScoreEntry {
                    measure: "block_design".into(),
                    value: 7.0,
                    note: String::new(),
                },
                ScoreEntry {
                    measure: "fsiq".into(),
                    value: 98.0,
                    note: String::new(),
                },
            ],
            notes: String::new(),
        };
        let full = format_sheet(&sheet).unwrap_or_default();
        let hers = format_sheet_with(&sheet, h).unwrap_or_default();
        assert!(full.contains("ציון 7, אחוזון"), "{full}");
        assert!(
            hers.contains("ציון 7 –") && !hers.contains("ציון 7, אחוזון"),
            "{hers}"
        );
        assert!(hers.contains("ציון 98, אחוזון"), "{hers}");
    }

    #[test]
    fn each_score_fits_its_range_and_a_relative_place_among_the_childs_subtests() {
        let entry = |measure: &str, value: f64| ScoreEntry {
            measure: measure.into(),
            value,
            note: String::new(),
        };
        let sheet = ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                entry("block_design", 6.0),
                entry("matrix", 13.0),
                entry("information", 10.0),
                entry("fsiq", 98.0),
            ],
            notes: String::new(),
        };
        let cases = sheet_cases(&sheet);
        let of = |m: &str| cases.iter().find(|c| c.measure == m).unwrap().cases.clone();
        assert!(of("block_design").contains(&"חולשה יחסית".to_owned()));
        assert!(of("matrix").contains(&"חוזקה יחסית".to_owned()));
        assert_eq!(of("information").len(), 2, "general and its range");
        assert!(
            !of("fsiq").iter().any(|c| c.ends_with("יחסית")),
            "an index is not relative"
        );
        assert!(cases
            .iter()
            .all(|c| c.cases.first().map(String::as_str) == Some(GENERAL_CASE)));
        assert_eq!(
            cases.first().map(|c| c.label.as_str()),
            Some("WPPSI-IV · קוביות")
        );
    }

    #[test]
    fn a_vineland_subdomain_has_a_relative_place_too() {
        let entry = |measure: &str, value: f64| ScoreEntry {
            measure: measure.into(),
            value,
            note: String::new(),
        };
        let sheet = ScoreSheet {
            instrument: "vineland_3".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                entry("receptive", 10.0),
                entry("personal", 15.0),
                entry("coping", 19.0),
            ],
            notes: String::new(),
        };
        let cases = sheet_cases(&sheet);
        let of = |m: &str| cases.iter().find(|c| c.measure == m).unwrap().cases.clone();
        assert!(of("receptive").contains(&"חולשה יחסית".to_owned()));
        assert!(of("coping").contains(&"חוזקה יחסית".to_owned()));
        assert!(!of("personal").iter().any(|c| c.ends_with("יחסית")));
    }

    #[test]
    fn only_a_score_tag_of_the_fixed_tables_passes() {
        for ok in [
            "[תוצאה 9]",
            "[תוצאה 112 · ממוצע גבוה]",
            "[תוצאה 13 · חוזקה יחסית]",
            "[תוצאה 29.5]",
        ] {
            assert!(is_score_tag(ok), "{ok}");
        }
        for bad in [
            "[תוצאה 9 · אלון]",
            "[תוצאה 012]",
            "[תוצאה 300]",
            "[תוצאה 9 · ממוצע · ממוצע]",
            "[תוצאה 9 · חולשה יחסית · ממוצע]",
            "[תוצאה 052-1]",
            "[תוצאה]",
            "תוצאה 9",
        ] {
            assert!(!is_score_tag(bad), "{bad}");
        }
    }

    #[test]
    fn the_new_tables_read_their_scores() {
        let v = measure("vineland_3", "receptive");
        assert_eq!(percentile(&v, 15.0).as_deref(), Some("50"));
        assert_eq!(
            classify(&v, 11.0).map(|b| b.label.as_str()),
            Some("נמוך במידה מתונה")
        );
        let t = measure("mullen", "visual_reception");
        assert_eq!(percentile(&t, 60.0).as_deref(), Some("84"));
        let b = measure("brief_p", "gec");
        assert_eq!(classify(&b, 66.0).map(|b| b.level), Some(3));
        // The ancillary indexes are not part of the gap between the primary ones.
        assert_eq!(measure("wppsi_iv", "gai").group, "מדדים נוספים");
    }

    fn measure(inst: &str, key: &str) -> Measure {
        instruments()
            .into_iter()
            .find(|i| i.key == inst)
            .and_then(|i| i.measures.into_iter().find(|m| m.key == key))
            .unwrap_or_else(|| panic!("{inst}/{key}"))
    }

    fn wppsi(entries: &[(&str, f64)]) -> ScoreSheet {
        ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: entries
                .iter()
                .map(|(k, v)| ScoreEntry {
                    measure: (*k).into(),
                    value: *v,
                    note: String::new(),
                })
                .collect(),
            notes: String::new(),
        }
    }

    #[test]
    fn written_scores_are_checked_against_the_sheet() {
        let sheets = [wppsi(&[
            ("vci", 95.0),
            ("matrix", 13.0),
            ("vocabulary", 9.0),
        ])];
        // Right score, right percentile, the measure by name or abbreviation: nothing to say.
        assert!(check_scores(
            "ההבנה המילולית (VCI) בטווח הממוצע (95, אחוזון 37).",
            &sheets
        )
        .is_empty());
        assert!(check_scores("[ילד] הציג תפקוד ממוצע (9) במטלת אוצר מילים.", &sheets).is_empty());
        assert!(check_scores("במטריצות הושג ציון ממוצע גבוה (13).", &sheets).is_empty());
        // A wrong number next to a named measure.
        let w = check_scores("הבנה מילולית (VCI) ממוצעת (97).", &sheets);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("97") && w[0].contains("95"), "{w:?}");
        // Numbers that are not scores: a test's name, an age, a sentence with no measure.
        assert!(check_scores("הועבר WPPSI-4 ו-ADOS-2 בגיל 5:4.", &sheets).is_empty());
        assert!(check_scores("[ילד] בן 5 ומגיע לגן 3 פעמים בשבוע.", &sheets).is_empty());
        // D-145: an age in words and a session's date beside a measure are not scores…
        assert!(check_scores(
            "במטריצות, בגיל 5 שנים ו-4 חודשים, ב-12.3.2026, הושג ציון ממוצע גבוה (13).",
            &sheets
        )
        .is_empty());
        assert!(check_scores("[ילד] (בן 5) הציג באוצר מילים ציון 9.", &sheets).is_empty());
        // …and a wrong score beside them still is.
        let w = check_scores(
            "במטריצות, בגיל 5 שנים ו-4 חודשים, ב-12.3.2026, הושג ציון (11).",
            &sheets,
        );
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("11"), "{w:?}");
        // A gap between two scores is not a score, and a score beside another measure's short
        // name is that measure's (a made-up report, 0.9.9).
        let mut bayley = wppsi(&[("cognitive", 85.0), ("language", 68.0), ("motor", 97.0)]);
        bayley.instrument = "bayley_4".into();
        assert!(check_scores(
            "פער של 29 נקודות בין מוטורי (97) לבין שפה (68), סטיית תקן אחת או יותר.",
            std::slice::from_ref(&bayley)
        )
        .is_empty());
        // …and a wrong one there still is.
        let w = check_scores("פער של 29 נקודות בין מוטורי (79) לבין שפה (68).", &[bayley]);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("79"), "{w:?}");
        // Short names are ordinary words: "מידע" alone is not a measure.
        assert!(
            check_scores("נמסר מידע מ-2 מקורות.", &[wppsi(&[("information", 8.0)])]).is_empty()
        );
        // A year beside a measure is no score: nothing to hold against the table…
        assert!(check_scores(
            "ההבנה המילולית (VCI) נבדקה בספטמבר 2026 והושג ציון 95.",
            &sheets
        )
        .is_empty());
        assert!(check_scores("במטריצות, בבדיקה של 2026, 13.", &sheets).is_empty());
        // …and a wrong score in the same sentence still is.
        let w = check_scores(
            "ההבנה המילולית (VCI) נבדקה בספטמבר 2026 והושג ציון 97.",
            &sheets,
        );
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("97") && !w[0].contains("2026"), "{w:?}");
        // An impossible score beside a measure is no year: it is pointed out, not passed over.
        let w = check_scores("ההבנה המילולית (VCI) – ציון 950.", &sheets);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("950") && w[0].contains("95"), "{w:?}");
    }

    /// Every scale's step, so a scale added later (an age equivalent, a growth score) is
    /// checked on its own grid and not refused as "not a whole number".
    #[test]
    fn every_scale_declares_its_step_and_every_table_sits_on_it() {
        let step_of = |m: &Measure| score_step(m.scale);
        for inst in instruments() {
            for m in &inst.measures {
                let step = step_of(m);
                assert_eq!(
                    step,
                    if m.scale == ScoreScale::Cars {
                        0.5
                    } else {
                        1.0
                    },
                    "{}/{}",
                    inst.key,
                    m.key
                );
                for edge in [m.min, m.max]
                    .into_iter()
                    .chain(m.bands.iter().flat_map(|b| [b.min, b.max]))
                {
                    assert!(
                        ((edge / step).round() * step - edge).abs() < 1e-9,
                        "{}/{}: {edge} is off the scale's step {step}",
                        inst.key,
                        m.key
                    );
                }
            }
        }
    }

    #[test]
    fn a_range_word_unlike_the_table_is_noted() {
        let sheets = [wppsi(&[("vci", 88.0), ("matrix", 13.0), ("psi", 112.0)])];
        let w = check_scores("הבנה מילולית (88) בטווח הממוצע.", &sheets);
        assert_eq!(
            w,
            vec!["כתוב \"ממוצע\" ליד הבנה מילולית (88), ולפי הטבלה: ממוצע נמוך".to_owned()]
        );
        // With prefixes, and the longest label first: "הממוצע הגבוה" is not "ממוצע".
        let w = check_scores("במטריצות (13) הושג ציון בטווח הממוצע הנמוך.", &sheets);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("\"ממוצע נמוך\"") && w[0].ends_with("ממוצע גבוה"),
            "{w:?}"
        );
        assert!(check_scores("במטריצות (13) הושג ציון בטווח הממוצע הגבוה.", &sheets).is_empty());
        assert!(check_scores("הבנה מילולית (VCI): ממוצע נמוך (88).", &sheets).is_empty());
        // Comparisons and negations are not a range.
        for fine in [
            "הבנה מילולית (88) מתחת לממוצע הכללי.",
            "הבנה מילולית (88) נמוכה מהממוצע של תת-המבחנים.",
            "הבנה מילולית (88) לא בטווח הממוצע.",
            "מהירות עיבוד (112) גבוה יחסית לשאר.",
        ] {
            assert!(check_scores(fine, &sheets).is_empty(), "{fine}");
        }
        // Two measures in one sentence, two labels, no score, or no sheet: nothing.
        assert!(check_scores(
            "הבנה מילולית (88) ומהירות עיבוד (112) בטווח הממוצע.",
            &sheets
        )
        .is_empty());
        assert!(check_scores("הבנה מילולית (88) בין ממוצע לממוצע גבוה.", &sheets).is_empty());
        assert!(check_scores("ההבנה המילולית בטווח הממוצע.", &sheets).is_empty());
        assert!(check_scores("הבנה מילולית (88) בטווח הממוצע.", &[]).is_empty());
    }

    #[test]
    fn a_percentile_unlike_the_computed_one_is_noted() {
        let sheets = [wppsi(&[("vci", 95.0), ("matrix", 13.0)])];
        assert!(check_scores("הבנה מילולית (VCI) 95, אחוזון 37.", &sheets).is_empty());
        assert!(check_scores("הבנה מילולית (VCI) 95, באחוזון ה-37.", &sheets).is_empty());
        let w = check_scores("הבנה מילולית (VCI) 95, אחוזון 40.", &sheets);
        assert_eq!(
            w,
            vec!["כתוב אחוזון 40 ליד הבנה מילולית (95), ולפי החישוב: אחוזון 37".to_owned()]
        );
        // The score itself in place of the percentile.
        let w = check_scores("הבנה מילולית 95 (אחוזון 95).", &sheets);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("אחוזון 95") && w[0].ends_with("אחוזון 37"),
            "{w:?}"
        );
        // Two measures: the general check only (40 is neither one's).
        let w = check_scores("הבנה מילולית 95 ומטריצות 13, אחוזון 40.", &sheets);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].starts_with("המספר 40"), "{w:?}");
        assert!(check_scores("הבנה מילולית 95, אחוזון 40.", &[]).is_empty());
    }

    #[test]
    fn each_line_of_the_score_check_says_what_it_is_about() {
        // D-161: the kind is given where the line is made, never read back from its words.
        let sheets = [wppsi(&[("vci", 95.0), ("matrix", 13.0)])];
        let kinds = |text: &str, sheets: &[ScoreSheet]| -> Vec<ScoreNoteKind> {
            let tagged = check_scores_tagged(text, sheets);
            // The plain check is the same lines in the same order.
            assert_eq!(
                tagged.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>(),
                check_scores(text, sheets)
            );
            tagged.into_iter().map(|(k, _)| k).collect()
        };
        assert_eq!(
            kinds("הבנה מילולית (VCI) 95, אחוזון 40.", &sheets),
            [ScoreNoteKind::Percentile]
        );
        assert_eq!(
            kinds("הבנה מילולית (95) בטווח הממוצע הגבוה.", &sheets),
            [ScoreNoteKind::BandWord]
        );
        assert_eq!(
            kinds("הבנה מילולית 95 ומטריצות 13, אחוזון 40.", &sheets),
            [ScoreNoteKind::Mismatch]
        );
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
        assert_eq!(
            kinds(
                "הציון הכולל (14) מעל נקודת החתך (12).",
                std::slice::from_ref(&ados)
            ),
            [ScoreNoteKind::Cutoff]
        );
        assert!(kinds("הבנה מילולית (VCI) 95, אחוזון 37.", &sheets).is_empty());
    }

    #[test]
    fn a_cutoff_unlike_the_sheets_is_noted() {
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
        let sheets = std::slice::from_ref(&ados);
        // The right cut-off is not "a number unlike the table" either.
        assert!(check_scores("הציון הכולל (14) מעל נקודת החתך (11).", sheets).is_empty());
        assert!(check_scores("ב-ADOS-2 נקודת החתך היא 11.", sheets).is_empty());
        let w = check_scores("הציון הכולל (14) מעל נקודת החתך (12).", sheets);
        assert_eq!(
            w,
            vec!["כתוב נקודת חתך 12, ובטבלת הציונים של ADOS-2: 11".to_owned()]
        );
        assert_eq!(check_scores("ב-ADOS נקודת חתך של 10.", sheets).len(), 1);
        // The module's number is not the cut-off; another test or no cut-off: nothing.
        assert!(check_scores("ב-ADOS-2 נקודת החתך למודול 1 נמוכה.", sheets).is_empty());
        assert!(check_scores("בשאלון אחר נקודת החתך היא 60.", sheets).is_empty());
        let no_cut = ScoreSheet {
            cutoff: None,
            ..ados.clone()
        };
        assert!(check_scores("ב-ADOS-2 נקודת החתך היא 12.", &[no_cut]).is_empty());
    }

    #[test]
    fn a_comparison_with_the_mean_is_not_a_range() {
        let sheets = [wppsi(&[("vci", 88.0), ("psi", 112.0)])];
        // Still a range: the hit stays.
        assert_eq!(
            check_scores("הבנה מילולית (88) בטווח הממוצע.", &sheets).len(),
            1
        );
        // The sheet's own wording for a relative place, and other comparisons: nothing.
        for fine in [
            "הבנה מילולית (88) נמוכה ביחס לממוצע של תת-המבחנים.",
            "הבנה מילולית (88) בולטת בהשוואה לממוצע האישי.",
            "הבנה מילולית (88) נמוכה יחסית לממוצע.",
            "הבנה מילולית (88) מתחת לממוצע.",
            "הבנה מילולית (88) רחוקה מהממוצע.",
            "מהירות עיבוד (112) מעל הממוצע.",
            "מהירות עיבוד (112) וביחס לממוצע בולטת.",
        ] {
            assert!(check_scores(fine, &sheets).is_empty(), "{fine}");
        }
    }

    #[test]
    fn a_range_in_another_clause_is_about_something_else() {
        let sheets = [wppsi(&[("vci", 88.0), ("matrix", 13.0)])];
        // The hit in the measure's own clause stays.
        let w = check_scores(
            "הבנה מילולית (88) בטווח הממוצע, בעוד ששאר היכולות גבוהות.",
            &sheets,
        );
        assert_eq!(
            w,
            vec!["כתוב \"ממוצע\" ליד הבנה מילולית (88), ולפי הטבלה: ממוצע נמוך".to_owned()]
        );
        for fine in [
            "בהבנה מילולית (VCI) התקבל ציון 88, בעוד ששאר היכולות בטווח הממוצע.",
            "בהבנה מילולית (VCI) התקבל ציון 88, ואילו שאר היכולות בטווח הממוצע.",
            "בהבנה מילולית (VCI) התקבל ציון 88 לעומת יכולות בטווח הממוצע בתחומים אחרים.",
            "בהבנה מילולית (VCI) התקבל ציון 88; שאר היכולות בטווח הממוצע.",
            "בהבנה מילולית (VCI) התקבל ציון 88\nשאר היכולות בטווח הממוצע.",
        ] {
            assert!(check_scores(fine, &sheets).is_empty(), "{fine}");
        }
        // A percentile: read in the measure's clause only.
        let sheets = [wppsi(&[("vci", 95.0)])];
        assert_eq!(
            check_scores("הבנה מילולית 95 באחוזון 40, בעוד ש...", &sheets),
            vec!["כתוב אחוזון 40 ליד הבנה מילולית (95), ולפי החישוב: אחוזון 37".to_owned()]
        );
        assert!(check_scores(
            "הבנה מילולית 95, בעוד שבני גילו באחוזון 50 בממוצע.",
            &sheets
        )
        .is_empty());
    }

    #[test]
    fn a_shorter_wording_of_the_right_range_is_not_another_range() {
        let sheets = [wppsi(&[("fsiq", 135.0), ("vci", 88.0)])];
        for fine in [
            "מנת משכל כללית (135) בטווח הגבוה.",
            "מנת משכל כללית (135) גבוה במיוחד.",
            "מנת משכל כללית (135) בטווח הגבוה מאוד.",
        ] {
            assert!(check_scores(fine, &sheets).is_empty(), "{fine}");
        }
        // Another range is still noted, and "ממוצע" is not a shorter "ממוצע נמוך".
        assert_eq!(
            check_scores("מנת משכל כללית (135) בטווח הממוצע.", &sheets),
            vec!["כתוב \"ממוצע\" ליד מנת משכל כללית (135), ולפי הטבלה: גבוה מאוד".to_owned()]
        );
        assert_eq!(
            check_scores("הבנה מילולית (88) בטווח הממוצע.", &sheets).len(),
            1
        );
        assert!(shorter_wording("גבוה", "גבוה מאוד"));
        assert!(shorter_wording("נמוך", "נמוך במידה מתונה"));
        assert!(!shorter_wording("ממוצע", "ממוצע נמוך"));
        assert!(!shorter_wording("גבוה", "ממוצע גבוה"));
    }

    #[test]
    fn range_words_in_their_forms_and_with_a_hyphen() {
        let sheets = [wppsi(&[("vci", 88.0), ("matrix", 13.0), ("psi", 100.0)])];
        // The right range, in another form: nothing.
        for fine in [
            "הבנה מילולית (88) ממוצע-נמוך.",
            "הבנה מילולית (88) ממוצע\u{05be}נמוך.",
            "הבנה מילולית (88) בטווח הממוצע־הנמוך.",
            "יכולת מהירות עיבוד (100) ממוצעת.",
            "הבנה מילולית (88) ממוצעת-נמוכה.",
        ] {
            assert!(check_scores(fine, &sheets).is_empty(), "{fine}");
        }
        // Another range, in another form: noted as the table's label.
        assert_eq!(
            check_scores("הבנה מילולית (88) ממוצעת.", &sheets),
            vec!["כתוב \"ממוצע\" ליד הבנה מילולית (88), ולפי הטבלה: ממוצע נמוך".to_owned()]
        );
        let w = check_scores("במטריצות (13) הושג ציון ממוצע-נמוך.", &sheets);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("\"ממוצע נמוך\""), "{w:?}");
        // A percentile after "ה־" with a maqaf, or "ה-".
        let sheets = [wppsi(&[("vci", 95.0)])];
        assert!(check_scores("הבנה מילולית (VCI) 95, באחוזון ה\u{05be}37.", &sheets).is_empty());
        assert_eq!(
            check_scores("הבנה מילולית (VCI) 95, באחוזון ה\u{05be}40.", &sheets),
            vec!["כתוב אחוזון 40 ליד הבנה מילולית (95), ולפי החישוב: אחוזון 37".to_owned()]
        );
        assert_eq!(
            check_scores("הבנה מילולית (VCI) 95, באחוזון ה-40.", &sheets).len(),
            1
        );
    }

    #[test]
    fn a_cutoff_is_held_against_the_test_the_sentence_is_about() {
        let sheet = |instrument: &str, cutoff: Option<f64>, total: f64| ScoreSheet {
            instrument: instrument.into(),
            module: "1".into(),
            cutoff,
            entries: vec![ScoreEntry {
                measure: "total".into(),
                value: total,
                note: String::new(),
            }],
            notes: String::new(),
        };
        let ados = sheet("ados_2", Some(11.0), 14.0);
        let cars = sheet("cars_2", None, 32.0);
        let both = [ados.clone(), cars];
        // ADOS named: its cut-off.
        assert_eq!(
            check_scores("ב-ADOS-2 הציון הכולל (14) מעל נקודת החתך (12).", &both),
            vec!["כתוב נקודת חתך 12, ובטבלת הציונים של ADOS-2: 11".to_owned()]
        );
        assert!(check_scores("ב-ADOS-2 הציון הכולל (14) מעל נקודת החתך (11).", &both).is_empty());
        // CARS-2 named, or "הציון הכולל" of both sheets with no test named: nothing.
        for fine in [
            "ב-CARS-2 הציון הכולל (32) מעל נקודת החתך (30).",
            "הציון הכולל (32) מעל נקודת החתך (30).",
            "ב-ADOS-2 וב-CARS-2 נקודת החתך היא 30.",
        ] {
            assert!(check_scores(fine, &both).is_empty(), "{fine}");
        }
        // One cut-off sheet and no other: the unnamed total is its own.
        assert_eq!(
            check_scores(
                "הציון הכולל (14) מעל נקודת החתך (12).",
                std::slice::from_ref(&ados)
            )
            .len(),
            1
        );
        // Another test named, even without its sheet: nothing.
        assert!(
            check_scores("ב-CARS-2 נקודת החתך היא 30.", std::slice::from_ref(&ados)).is_empty()
        );
    }

    #[test]
    fn the_sheet_gives_relative_places_and_elevated_scales() {
        let text = format_sheet(&wppsi(&[
            ("block_design", 6.0),
            ("matrix", 13.0),
            ("information", 10.0),
            ("fsiq", 98.0),
        ]))
        .unwrap_or_else(|e| panic!("{e}"));
        let line = text
            .lines()
            .find(|l| l.starts_with("חוזקות וחולשות יחסיות"))
            .unwrap_or_else(|| panic!("{text}"));
        // The same line as the browser preview's (apps/desktop/src/web/scores.test.ts).
        assert_eq!(
            line,
            "חוזקות וחולשות יחסיות לפי כלל אצבע, שחושב בתוכנה: הפרש של 3 נקודות ומעלה מהממוצע \
             של הציונים בתת-מבחנים שהוזנו בטבלה. מובהקות ההפרש נבדקת לפי טבלאות מדריך המבחן. \
             חוזקה יחסית: מטריצות (MR) – ציון 13. חולשה יחסית: קוביות (BD) – ציון 6"
        );
        // It says plainly what it is: a rule of thumb of the software, over the subtests she
        // entered, not the manual's significance tables.
        assert!(line.contains("כלל אצבע, שחושב בתוכנה"), "{line}");
        assert!(
            line.contains("מהממוצע של הציונים בתת-מבחנים שהוזנו בטבלה"),
            "{line}"
        );
        assert!(line.contains("הפרש של 3 נקודות ומעלה"), "{line}");
        assert!(line.contains("לפי טבלאות מדריך המבחן"), "{line}");
        assert!(
            line.contains("חוזקה יחסית: מטריצות (MR) – ציון 13"),
            "{line}"
        );
        assert!(line.contains("חולשה יחסית: קוביות (BD) – ציון 6"), "{line}");
        assert!(!line.contains("מידע (IN)"), "{line}");
        // Two subtests are not enough, and an even profile has none.
        let two = format_sheet(&wppsi(&[("block_design", 6.0), ("matrix", 13.0)])).unwrap();
        assert!(!two.contains("יחסיות"), "{two}");
        let even = format_sheet(&wppsi(&[
            ("block_design", 9.0),
            ("matrix", 10.0),
            ("information", 11.0),
        ]))
        .unwrap();
        assert!(!even.contains("יחסיות"), "{even}");

        let brief = ScoreSheet {
            instrument: "brief_p".into(),
            module: String::new(),
            cutoff: None,
            entries: [
                ("inhibit", 66.0),
                ("shift", 61.0),
                ("emotional_control", 50.0),
            ]
            .iter()
            .map(|(k, v)| ScoreEntry {
                measure: (*k).into(),
                value: *v,
                note: String::new(),
            })
            .collect(),
            notes: String::new(),
        };
        let text = format_sheet(&brief).unwrap();
        assert!(
            text.contains(
                "סולמות בטווח חריג לפי הטבלה: עכבה (INH) – ציון 66, מוגבר באופן מובהק קלינית; \
                 גמישות (SHF) – ציון 61, מוגבר במידה קלה\n"
            ),
            "{text}"
        );
        // Ability scales never have an "elevated" line, even with low scores.
        let low = format_sheet(&wppsi(&[("vci", 70.0), ("psi", 72.0)])).unwrap();
        assert!(!low.contains("בטווח חריג"), "{low}");
        // Every questionnaire table says which band is the typical one: level 0 only.
        for inst in instruments() {
            for m in inst
                .measures
                .iter()
                .filter(|m| m.scale == ScoreScale::TConcern)
            {
                for b in &m.bands {
                    assert_eq!(
                        b.level == 0,
                        b.label == "בטווח התקין",
                        "{}/{}",
                        inst.key,
                        m.key
                    );
                }
            }
        }
    }

    #[test]
    fn composites_and_other_scales_are_not_ordered_with_the_domains() {
        let vineland = ScoreSheet {
            instrument: "vineland_3".into(),
            module: String::new(),
            cutoff: None,
            entries: [
                ("abc", 80.0),
                ("communication", 75.0),
                ("socialization", 90.0),
            ]
            .iter()
            .map(|(k, v)| ScoreEntry {
                measure: (*k).into(),
                value: *v,
                note: String::new(),
            })
            .collect(),
            notes: String::new(),
        };
        let text = format_sheet(&vineland).unwrap();
        let line = text.lines().find(|l| l.contains("מהגבוה לנמוך")).unwrap();
        assert!(!line.contains("ABC"), "{line}");
        assert!(line.contains("SOC") && line.contains("COM"), "{line}");
        let ados = ScoreSheet {
            instrument: "ados_2".into(),
            module: "1".into(),
            cutoff: Some(11.0),
            entries: [
                ("social_affect", 10.0),
                ("rrb", 4.0),
                ("total", 14.0),
                ("comparison", 7.0),
            ]
            .iter()
            .map(|(k, v)| ScoreEntry {
                measure: (*k).into(),
                value: *v,
                note: String::new(),
            })
            .collect(),
            notes: String::new(),
        };
        let text = format_sheet(&ados).unwrap();
        assert!(!text.contains("מהגבוה לנמוך"), "{text}");
        let mut brief = ScoreSheet {
            instrument: "brief_p".into(),
            ..ados
        };
        brief.cutoff = None;
        brief.entries = [("isci", 70.0), ("fi", 55.0), ("gec", 66.0)]
            .iter()
            .map(|(k, v)| ScoreEntry {
                measure: (*k).into(),
                value: *v,
                note: String::new(),
            })
            .collect();
        let text = format_sheet(&brief).unwrap();
        let line = text.lines().find(|l| l.contains("מהגבוה לנמוך")).unwrap();
        assert!(!line.contains("GEC") && line.contains("ISCI"), "{line}");
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
        assert!(
            text.contains(
                "מדדים מהגבוה לנמוך: הבנה מילולית (VCI) – ציון 112; מהירות עיבוד (PSI) – ציון 88"
            ),
            "{text}"
        );
        // A group of one score (the one subtest) has no order to give.
        assert_eq!(text.matches("מהגבוה לנמוך").count(), 1, "{text}");

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
    fn the_domains_are_given_from_the_highest_to_the_lowest_in_code() {
        let entry = |measure: &str, value: f64| ScoreEntry {
            measure: measure.into(),
            value,
            note: String::new(),
        };
        let sheet = ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                entry("fsiq", 99.0),
                entry("psi", 88.0),
                entry("vci", 112.0),
                entry("vsi", 100.0),
                entry("matrix", 13.0),
            ],
            notes: String::new(),
        };
        let text = format_sheet(&sheet).unwrap_or_else(|e| panic!("{e}"));
        let line = text
            .lines()
            .find(|l| l.contains("מהגבוה לנמוך"))
            .unwrap_or_else(|| panic!("{text}"));
        let at = |abbr: &str| line.find(abbr).unwrap_or_else(|| panic!("{line}"));
        assert!(at("VCI") < at("VSI") && at("VSI") < at("PSI"), "{line}");
        assert!(line.contains("(VCI) – ציון 112"), "{line}");
        // The full scale is not one of the domains.
        assert!(!line.contains("FSIQ"), "{line}");
        // One subtest alone is not ordered.
        assert_eq!(text.matches("מהגבוה לנמוך").count(), 1, "{text}");
        // Nor is Vineland's composite one of its domains, nor Mullen's of its scales: a
        // composite sits in the same group as what it sums up.
        let vineland = ScoreSheet {
            instrument: "vineland_3".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                entry("abc", 92.0),
                entry("communication", 98.0),
                entry("daily_living", 85.0),
                entry("socialization", 90.0),
            ],
            notes: String::new(),
        };
        let text = format_sheet(&vineland).unwrap_or_else(|e| panic!("{e}"));
        let line = text
            .lines()
            .find(|l| l.contains("מהגבוה לנמוך"))
            .unwrap_or_else(|| panic!("{text}"));
        assert!(!line.contains("ABC"), "{line}");
        assert!(
            line.contains("(COM) – ציון 98") && line.ends_with("(DLS) – ציון 85"),
            "{line}"
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

    /// The browser preview reads the same tables (apps/desktop/src/web/instruments.json).
    #[test]
    fn the_browser_preview_uses_these_tables() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/desktop/src/web/instruments.json"
        );
        let committed: Vec<Instrument> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_default())
                .unwrap_or_default();
        assert_eq!(
            committed,
            instruments(),
            "run: cargo run -p dv-domain --example instruments_json -- apps/desktop/src/web/instruments.json"
        );
    }

    #[test]
    fn a_sheet_becomes_a_table_for_the_report() {
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
                    note: String::new(),
                },
            ],
            notes: String::new(),
        };
        let (title, rows, note) = sheet_table(&sheet).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(title, "WPPSI-IV");
        assert_eq!(
            rows[0],
            ScoreRow {
                measure: "הבנה מילולית (VCI)".into(),
                score: "112".into(),
                percentile: "79".into(),
                range: "ממוצע גבוה".into()
            }
        );
        assert_eq!(rows[1].range, "ממוצע נמוך");
        assert!(note.contains("ממוצע 100"));
        let ados = ScoreSheet {
            instrument: "ados_2".into(),
            module: "2".into(),
            cutoff: Some(8.0),
            entries: vec![ScoreEntry {
                measure: "total".into(),
                value: 9.0,
                note: String::new(),
            }],
            notes: String::new(),
        };
        let (title, rows, _) = sheet_table(&ados).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(title, "ADOS-2 · מודול 2");
        assert_eq!(
            (rows[0].percentile.as_str(), rows[0].range.as_str()),
            ("—", "בנקודת החתך או מעליה (8)")
        );
    }

    #[test]
    fn profiles_split_indexes_from_subtests() {
        let sheet = wppsi(&[("fsiq", 102.0), ("vci", 112.0), ("block_design", 9.0)]);
        let p = sheet_profiles(&sheet).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].mean, p[0].sd), (100.0, 15.0));
        assert_eq!(p[0].bars.len(), 2);
        assert_eq!(p[0].bars[1], ("הבנה מילולית (VCI)".to_owned(), 112.0));
        assert_eq!((p[1].mean, p[1].bars.len()), (10.0, 1));
        assert!(sheet_profiles(&wppsi(&[])).unwrap().is_empty());
    }

    /// D-162: the closing sections get the totals and indexes on the very lines of the score
    /// material, and the patterns of the scores; a subtest stays in its own section.
    #[test]
    fn the_key_scores_are_the_sheets_own_lines_for_totals_and_indexes_with_the_patterns() {
        let entry = |measure: &str, value: f64, note: &str| ScoreEntry {
            measure: measure.into(),
            value,
            note: note.into(),
        };
        let cognitive = ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![
                entry("fsiq", 98.0, "פער בין המדדים"),
                entry("vci", 112.0, ""),
                entry("psi", 82.0, ""),
                entry("block_design", 13.0, ""),
            ],
            notes: String::new(),
        };
        let ados = ScoreSheet {
            instrument: "ados_2".into(),
            module: "2".into(),
            cutoff: Some(8.0),
            entries: vec![entry("total", 11.0, "")],
            notes: String::new(),
        };
        let only_subtests = ScoreSheet {
            instrument: "wisc_v".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![entry("matrix", 9.0, "")],
            notes: String::new(),
        };
        let unknown = ScoreSheet {
            instrument: "no_such_test".into(),
            module: String::new(),
            cutoff: None,
            entries: vec![entry("fsiq", 100.0, "")],
            notes: String::new(),
        };
        let sheets = [
            cognitive.clone(),
            ados.clone(),
            only_subtests.clone(),
            unknown,
        ];
        let text = key_scores(&sheets, SheetHabits::default()).unwrap();
        let material = format_sheet(&cognitive).unwrap();
        let ados_material = format_sheet(&ados).unwrap();
        // Every score line of the block is a line of the material, word for word.
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("- ")).collect();
        assert_eq!(lines.len(), 4, "{text}");
        for line in &lines {
            assert!(
                material.lines().any(|m| m == *line) || ados_material.lines().any(|m| m == *line),
                "{line}"
            );
        }
        assert!(
            text.contains("- מנת משכל כללית (FSIQ): ציון 98, אחוזון 45 – ממוצע. פער בין המדדים")
        );
        assert!(text.contains("WPPSI-IV:\n") && text.contains("ADOS-2 · מודול 2:\n"));
        assert!(text.contains("בנקודת החתך או מעליה (8)"));
        assert!(
            !text.contains("קוביות") && !text.contains("WISC-V"),
            "{text}"
        );
        assert!(!text.contains("no_such_test"));
        assert!(text.contains("דפוסי התוצאות, כפי שחושבו בתוכנה מהטבלאות: פער של סטיית תקן"));
        assert!(text.ends_with("לא על ידי מודל השפה.\n"));
        // What she leaves out beside an index is left out here too.
        let habits = SheetHabits {
            no_subtest_percentiles: false,
            no_index_percentiles: true,
        };
        let bare = key_scores(&sheets, habits).unwrap();
        assert!(!bare.contains("אחוזון"), "{bare}");
        assert_eq!(key_scores(&[only_subtests], SheetHabits::default()), None);
        assert_eq!(key_scores(&[], SheetHabits::default()), None);
    }
}
