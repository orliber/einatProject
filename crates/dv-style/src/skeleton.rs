//! The order of a section (K3): which topics she writes about in it, in what order and at what
//! length, counted across her past reports. Each paragraph (or, in a section written as one
//! paragraph, each sentence) is given the topic whose words it uses most; a passage with none
//! of them continues the topic before it. A topic that appears in at least half of the reports
//! that have the section is part of its skeleton, in its usual place.
//!
//! The skeleton is counted separately for each age band (K13) when the band has enough
//! reports of its own: a report on a two-year-old is built differently from one on a
//! six-year-old. Only the topics' labels, places and lengths are kept, never text.

use std::collections::{BTreeMap, HashSet};

use dv_privacy::text::normalize;
use serde::{Deserialize, Serialize};

use crate::edits::percent;
use crate::phrases::ReportParts;
use crate::text::{sentences, words};

/// Fewest reports with the section before its order is learned.
const MIN_REPORTS: usize = 3;
/// A topic belongs to the skeleton when this share of the reports writes about it.
const MIN_SHARE: f64 = 0.5;
/// Hebrew prefix letters a keyword may follow (בגן, והמשחק).
const PREFIXES: &[char] = &['ו', 'ב', 'ל', 'ה', 'כ', 'מ', 'ש'];

/// The age band of a report or a case (K13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgeBand {
    /// Under three.
    Toddler,
    /// Three to six.
    Preschool,
    /// Six and over.
    School,
}

impl AgeBand {
    /// The band of an age in whole years.
    #[must_use]
    pub fn of_years(years: u8) -> Self {
        match years {
            0..=2 => Self::Toddler,
            3..=5 => Self::Preschool,
            _ => Self::School,
        }
    }

    /// In words, for the screen.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Toddler => "עד גיל שלוש",
            Self::Preschool => "גיל שלוש עד שש",
            Self::School => "גיל שש ומעלה",
        }
    }
}

/// The topics of the sections whose order is learned: (section, [(topic, its words)]). The
/// topics follow the institute's guide for the section.
pub const TOPICS: &[(&str, &[(&str, &str)])] = &[
    (
        "parents_view",
        &[
            (
                "התכונות הטובות ומה אוהב",
                "אוהב אוהבת נהנה נהנית חוזקות תכונות סקרן סקרנית הומור מתוק מתוקה חייכן חייכנית שמח שמחה",
            ),
            (
                "סדר היום והעצמאות",
                "בוקר ערב התלבשות מתלבש מתלבשת אכילה אוכל אוכלת שינה ישן ישנה נרדם נרדמת גמילה גמול גמולה מקלחת רחצה עצמאות עצמאי עצמאית",
            ),
            (
                "משחק",
                "משחק משחקת משחקים צעצוע צעצועים בובות דמיון לגו הרכבה פאזל פאזלים",
            ),
            (
                "רגשי",
                "רגשי רגשית רגשות כעס כועס כועסת תסכול מתוסכל התפרצות התפרצויות פחד פחדים חרדה עצב בוכה נרגע נרגעת",
            ),
            (
                "חברתי",
                "חברים חברות חבר חברה חברתי חברתית אחים אחיו אחותו קבוצה ילדים",
            ),
            (
                "תקשורתי",
                "מדבר מדברת דיבור משפטים מילים שפה מבין מבינה הבנה תקשורת מחוות מצביע מצביעה",
            ),
            (
                "חישה",
                "רעש רעשים מגע חושי חושית חישה חישתי מרקם מרקמים תחושתי תחושה ריחות",
            ),
            (
                "קשב",
                "קשב ריכוז מתרכז מתרכזת מוסח מוסחת הסחה פעלתנות פעלתן תנועתי תנועתית",
            ),
        ],
    ),
    (
        "kindergarten",
        &[
            (
                "המסגרות לפי הסדר",
                "מסגרת מסגרות משפחתון פעוטון מעון גן עבר עברה שנה",
            ),
            (
                "ההתאקלמות",
                "התאקלמות התאקלם התאקלמה הסתגלות הסתגל הסתגלה פרידה נפרד נפרדת",
            ),
            (
                "הדיווח של הצוות",
                "גננת סייעת צוות מדווחת מדווח מדווחים דיווח מורה מחנכת",
            ),
        ],
    ),
    (
        "appearance",
        &[
            (
                "ההגעה והקשר",
                "הגיע הגיעה נכנס נכנסה נפרד נפרדה פרידה חדר קשר התחבר התחברה ליווי",
            ),
            (
                "תקשורת מילולית ולא מילולית",
                "עין מבט דיבור מדבר מדברת שיח מחוות הצבעה חיוך משפטים מילולי מילולית",
            ),
            (
                "טמפרמנט וקשב",
                "טמפרמנט קשב ריכוז אנרגיה פעלתן שקט שקטה רגוע רגועה עירני עירנית ערני ערנית תנועתי תנועתית",
            ),
            (
                "שיתוף פעולה ותגובה לכישלון",
                "שיתוף שיתף שיתפה כישלון כישלונות תסכול הצלחה הצלחות עידוד ויתר ויתרה התמיד התמידה התמדה מוטיבציה",
            ),
        ],
    ),
    (
        "emotional",
        &[
            (
                "מה מעסיק והתמה",
                "מעסיק מעסיקה תמה תמות סמלי סיפור סיפורים דמויות ציור ציורים פנימי",
            ),
            (
                "ההורים כבסיס בטוח",
                "בטוח בטוחה ביטחון היקשרות קשר נעזר נעזרת",
            ),
            (
                "ההורים מהצד שלהם",
                "הורות מתמודדים מתמודדת עומס דאגה דאגות מתארים שיתפו",
            ),
            (
                "המלצה לטיפול",
                "טיפול טיפולי ממליצה מומלץ המלצה",
            ),
        ],
    ),
    (
        "summary",
        &[
            (
                "הד לסיבת ההפניה",
                "הופנה הופנתה הפניה סיבת שאלה",
            ),
            (
                "ההתרשמות בקצרה",
                "התרשמות התרשמתי נעים נעימה סקרן סקרנית מפגש",
            ),
            (
                "התוצאות העיקריות",
                "תוצאה תוצאות ציון ציונים טווח מדד מדדים מנת משכל",
            ),
            (
                "השאלונים",
                "שאלון שאלונים דירוג asrs brief abas vineland",
            ),
            (
                "התחום הרגשי",
                "רגשי רגשית רגשות",
            ),
            (
                "ההמלצה",
                "ממליצה מומלץ המלצה המלצות לאור",
            ),
        ],
    ),
];

/// One topic of a learned skeleton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkeletonTopic {
    pub label: String,
    /// Share of the reports with the section that write about it, 0 to 100.
    pub share: u32,
    /// The usual length, in words (the median).
    pub words: u32,
}

/// The usual order of a section, for every case or for one age band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skeleton {
    pub section: String,
    /// `None`: from all her reports.
    pub band: Option<AgeBand>,
    /// How many reports with the section it was learned from.
    pub reports: u32,
    /// In their usual order.
    pub topics: Vec<SkeletonTopic>,
}

impl Skeleton {
    /// The skeleton as a section guide: one line for each topic, in order (the screen numbers
    /// them), with its usual length in words ("פסקה קצרה"), never a figure from a report.
    #[must_use]
    pub fn guide(&self) -> String {
        self.topics
            .iter()
            .map(|t| format!("{} ({}).", t.label, length_words(t.words)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn length_words(words: u32) -> &'static str {
    match words {
        0..=29 => "משפט או שניים",
        30..=69 => "פסקה קצרה",
        70..=139 => "פסקה",
        _ => "פסקה ארוכה",
    }
}

/// The topic lists of `section`, normalized.
fn topics_of(section: &str) -> Option<Vec<(&'static str, HashSet<String>)>> {
    let (_, topics) = TOPICS.iter().find(|(k, _)| *k == section)?;
    Some(
        topics
            .iter()
            .map(|(label, list)| (*label, list.split_whitespace().map(normalize).collect()))
            .collect(),
    )
}

/// Whether the normalized word `w` is one of `keys`, alone or after one or two prefix letters.
fn is_key(w: &str, keys: &HashSet<String>) -> bool {
    if keys.contains(w) {
        return true;
    }
    let mut rest = w;
    for _ in 0..2 {
        let mut chars = rest.chars();
        match chars.next() {
            Some(c) if PREFIXES.contains(&c) => {
                rest = chars.as_str();
                if rest.chars().count() >= 2 && keys.contains(rest) {
                    return true;
                }
            }
            _ => break,
        }
    }
    false
}

/// The passages a section's text is read in: its paragraphs, or its sentences when it is one.
fn passages(text: &str) -> Vec<&str> {
    let paras: Vec<&str> = text
        .split('\n')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if paras.len() >= 2 {
        paras
    } else {
        sentences(text)
    }
}

/// The topics of one section of one report, in the order they first appear, with the words
/// written on each.
fn read_section(text: &str, topics: &[(&'static str, HashSet<String>)]) -> Vec<(usize, u32)> {
    let mut order: Vec<(usize, u32)> = Vec::new();
    let mut current: Option<usize> = None;
    for passage in passages(text) {
        let ws = words(passage);
        let hits = topics
            .iter()
            .map(|(_, keys)| ws.iter().filter(|w| is_key(&w.norm, keys)).count())
            .collect::<Vec<_>>();
        let best = hits
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            // Ties go to the earlier topic.
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)))
            .map(|(i, _)| i);
        if let Some(b) = best {
            current = Some(b);
        }
        let Some(t) = current else { continue };
        let n = u32::try_from(ws.len()).unwrap_or(u32::MAX);
        match order.iter_mut().find(|(i, _)| *i == t) {
            Some(e) => e.1 = e.1.saturating_add(n),
            None => order.push((t, n)),
        }
    }
    order
}

/// One report as read for one section: its topics in order, with their words.
type Reading = Vec<(usize, u32)>;

#[allow(clippy::cast_precision_loss)]
fn skeleton_of(
    section: &str,
    band: Option<AgeBand>,
    topics: &[(&'static str, HashSet<String>)],
    readings: &[&Reading],
) -> Option<Skeleton> {
    if readings.len() < MIN_REPORTS {
        return None;
    }
    let n = readings.len() as f64;
    let mut kept: Vec<(f64, SkeletonTopic)> = Vec::new();
    for (t, (label, _)) in topics.iter().enumerate() {
        let mut places = Vec::new();
        let mut lengths = Vec::new();
        for r in readings {
            if let Some(pos) = r.iter().position(|(i, _)| *i == t) {
                let last = r.len().saturating_sub(1).max(1) as f64;
                places.push(pos as f64 / last);
                lengths.push(r.get(pos).map_or(0, |(_, w)| *w));
            }
        }
        if (places.len() as f64) < MIN_SHARE * n {
            continue;
        }
        lengths.sort_unstable();
        let place = places.iter().sum::<f64>() / places.len() as f64;
        kept.push((
            place,
            SkeletonTopic {
                label: (*label).to_owned(),
                share: percent(places.len() as f64 / n),
                words: lengths.get(lengths.len() / 2).copied().unwrap_or(0),
            },
        ));
    }
    if kept.len() < 2 {
        return None;
    }
    kept.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(Skeleton {
        section: section.to_owned(),
        band,
        reports: u32::try_from(readings.len()).unwrap_or(u32::MAX),
        topics: kept.into_iter().map(|(_, t)| t).collect(),
    })
}

/// The skeleton of each section that has topics, from all her reports, and for each age band
/// with enough reports of its own. `bands` gives each report's band, when it is known.
#[must_use]
pub fn mine_skeletons(reports: &[ReportParts<'_>], bands: &[Option<AgeBand>]) -> Vec<Skeleton> {
    let mut out = Vec::new();
    for (section, _) in TOPICS {
        let Some(topics) = topics_of(section) else {
            continue;
        };
        // (band of the report, how it reads), for each report that has the section.
        let readings: Vec<(Option<AgeBand>, Reading)> = reports
            .iter()
            .enumerate()
            .filter_map(|(d, parts)| {
                let text: Vec<&str> = parts
                    .iter()
                    .filter(|(s, _)| *s == Some(*section))
                    .map(|(_, t)| *t)
                    .collect();
                if text.is_empty() {
                    return None;
                }
                let r = read_section(&text.join("\n"), &topics);
                (!r.is_empty()).then(|| (bands.get(d).copied().flatten(), r))
            })
            .collect();
        let all: Vec<&Reading> = readings.iter().map(|(_, r)| r).collect();
        out.extend(skeleton_of(section, None, &topics, &all));
        let by_band: BTreeMap<AgeBand, Vec<&Reading>> =
            readings.iter().fold(BTreeMap::new(), |mut m, (b, r)| {
                if let Some(b) = b {
                    m.entry(*b).or_insert_with(Vec::new).push(r);
                }
                m
            });
        for (band, rs) in by_band {
            out.extend(skeleton_of(section, Some(band), &topics, &rs));
        }
    }
    out
}

/// The skeleton that fits a case: its age band's when there is one, else the general one.
#[must_use]
pub fn skeleton_for<'a>(
    skeletons: &'a [Skeleton],
    section: &str,
    band: Option<AgeBand>,
) -> Option<&'a Skeleton> {
    let of = |b: Option<AgeBand>| {
        skeletons
            .iter()
            .find(|s| s.section == section && s.band == b)
    };
    band.and_then(|b| of(Some(b))).or_else(|| of(None))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// A fabricated "at home" section with the institute's seven topics in her order, each in a
    /// paragraph of its own; `skip` leaves one topic out.
    fn home(skip: Option<usize>, variant: usize) -> String {
        let paras = [
            "[ילד] ילד חייכן וסקרן, אוהב מוזיקה ונהנה מטיולים עם המשפחה.",
            "בבוקר מתלבש בעזרה, אוכל מגוון ונרדם בקלות. גמול ביום, ומתקלח בהנאה.",
            "במשחק בוחר בלגו ובפאזלים, ומשחק בדמיון עם בובות.",
            "כשהוא כועס מגיב בבכי, ומתקשה להירגע לאחר תסכול.",
            "עם האחים משתף פעולה, ובגינה פונה לילדים אחרים ומחפש חברים.",
            "מדבר במשפטים קצרים, מבין הוראות פשוטות ומצביע על מה שרוצה.",
            "רגיש לרעש חזק ונמנע ממגע של מרקמים מסוימים.",
            "מתקשה להתרכז לאורך זמן, ופעלתנות רבה ניכרת בבית.",
        ];
        paras
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != skip)
            .map(|(i, p)| {
                if i == variant % paras.len() {
                    format!("{p} כך תיארו ההורים.")
                } else {
                    (*p).to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_topics_of_the_home_section_are_found_in_her_order() {
        let texts: Vec<String> = (0..6).map(|i| home((i == 2).then_some(6), i)).collect();
        let reports: Vec<ReportParts<'_>> = texts
            .iter()
            .map(|t| vec![(Some("parents_view"), t.as_str())])
            .collect();
        let s = mine_skeletons(&reports, &[]);
        let home = s.iter().find(|s| s.section == "parents_view").unwrap();
        let labels: Vec<&str> = home.topics.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "התכונות הטובות ומה אוהב",
                "סדר היום והעצמאות",
                "משחק",
                "רגשי",
                "חברתי",
                "תקשורתי",
                "חישה",
                "קשב"
            ]
        );
        assert_eq!(home.reports, 6);
        let sensing = home.topics.iter().find(|t| t.label == "חישה").unwrap();
        assert_eq!(sensing.share, 83, "one report left it out");
        let guide = home.guide();
        assert!(guide.starts_with("התכונות הטובות ומה אוהב (משפט או שניים)."));
        assert_eq!(guide.lines().count(), 8);
        assert!(!guide.contains('['), "no tag in a guide");
        assert!(
            mine_skeletons(&reports[..2], &[]).is_empty(),
            "two reports are not a habit"
        );
    }

    #[test]
    fn a_section_written_as_one_paragraph_is_read_by_sentences() {
        let texts: Vec<String> = (0..4)
            .map(|i| {
                format!(
                    "[ילד] הגיע עם אמו ונפרד בקלות. שמר על קשר עין ודיבור שוטף {i}. היה רגוע וקשבו נשמר. שיתף פעולה גם לאחר כישלון."
                )
            })
            .collect();
        let reports: Vec<ReportParts<'_>> = texts
            .iter()
            .map(|t| vec![(Some("appearance"), t.as_str())])
            .collect();
        let s = mine_skeletons(&reports, &[]);
        let a = s.iter().find(|s| s.section == "appearance").unwrap();
        assert_eq!(a.topics.len(), 4);
        assert_eq!(a.topics.first().unwrap().label, "ההגעה והקשר");
        assert_eq!(a.topics.last().unwrap().label, "שיתוף פעולה ותגובה לכישלון");
    }

    #[test]
    fn each_age_band_with_enough_reports_gets_its_own_order() {
        // Toddlers: play before the day; school age: the day before play.
        let young = "[ילד] אוהב מוזיקה ונהנה מטיולים.\nבמשחק בוחר בלגו ובפאזלים.\nבבוקר מתלבש בעזרה ואוכל מגוון.";
        let old = "[ילד] אוהב מוזיקה ונהנה מטיולים.\nבבוקר מתלבש בעזרה ואוכל מגוון.\nבמשחק בוחר בלגו ובפאזלים.";
        let mut reports: Vec<ReportParts<'_>> = Vec::new();
        let mut bands = Vec::new();
        for _ in 0..3 {
            reports.push(vec![(Some("parents_view"), young)]);
            bands.push(Some(AgeBand::Toddler));
            reports.push(vec![(Some("parents_view"), old)]);
            bands.push(Some(AgeBand::School));
        }
        let s = mine_skeletons(&reports, &bands);
        let second = |b| {
            skeleton_for(&s, "parents_view", b)
                .unwrap()
                .topics
                .get(1)
                .unwrap()
                .label
                .clone()
        };
        assert_eq!(second(Some(AgeBand::Toddler)), "משחק");
        assert_eq!(second(Some(AgeBand::School)), "סדר היום והעצמאות");
        assert_eq!(
            skeleton_for(&s, "parents_view", Some(AgeBand::Preschool))
                .unwrap()
                .band,
            None,
            "a band without its own reports falls back to all of them"
        );
        assert_eq!(AgeBand::of_years(2), AgeBand::Toddler);
        assert_eq!(AgeBand::of_years(3), AgeBand::Preschool);
        assert_eq!(AgeBand::of_years(6), AgeBand::School);
    }
}
