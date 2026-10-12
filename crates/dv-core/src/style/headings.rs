//! Template sections of a past report: the words of its headings, and cutting it by them.

use super::*;

/// Words in a heading that point to a template section. Checked in this order ("סיכום
/// והמלצות" is the summary; "אבחנה על-פי DSM" is the DSM section, not the diagnoses).
pub(super) const HEADING_WORDS: &[(&str, &[&str])] = &[
    ("dsm", &["dsm"]),
    ("summary", &["סיכום"]),
    ("recommendations", &["המלצות", "המלצה"]),
    ("referral", &["סיבת הפניה", "סיבת ההפניה", "הפניה"]),
    (
        "parents_view",
        &[
            "לפי ההורים",
            "דיווח ההורים",
            "תיאור ההורים",
            "תפקוד והתנהגות",
        ],
    ),
    (
        "background",
        &[
            "רקע",
            "הריון",
            "לידה",
            "אבני דרך",
            "היסטוריה התפתחותית",
            "התפתחות מוקדמת",
        ],
    ),
    (
        "prior_assessments",
        &["אבחונים", "טיפולים", "אבחון קודם", "בדיקות קודמות"],
    ),
    (
        "kindergarten",
        &["מסגרת", "גן", "בית ספר", "בית הספר", "דיווח הצוות", "גננת"],
    ),
    (
        "tools",
        &["כלי האבחון", "כלי אבחון", "כלים", "מבחנים שהועברו"],
    ),
    ("adaptive", &["הסתגלות", "abas", "תפקוד מסתגל"]),
    (
        "cognitive",
        &[
            "קוגניטיבי",
            "קוגניטיבית",
            "אינטליגנציה",
            "wppsi",
            "wisc",
            "וכסלר",
            "וקסלר",
            "bayley",
            "ביילי",
            "יכולות חשיבה",
        ],
    ),
    (
        "communication",
        &["תקשורת", "ados", "asrs", "cars", "שפה ודיבור"],
    ),
    ("emotional", &["רגשי", "רגשית", "משחק", "השלכתי", "htp"]),
    (
        "appearance",
        &["הופעה", "התרשמות", "תצפית", "התנהגות במהלך"],
    ),
    ("diagnoses", &["אבחנות", "אבחנה"]),
];

/// Words of a report's title line, which is not a section heading.
pub(super) const REPORT_TITLES: &[&str] = &[
    "סיכום אבחון",
    "דוח אבחון",
    "דו\"ח אבחון",
    "חוות דעת",
    "אבחון פסיכולוגי",
];

/// A heading's words, normalized, with the common one-letter prefixes as separate readings.
pub(super) fn heading_matches(norm: &str, keyword: &str) -> bool {
    let keyword = normalize(keyword).to_lowercase();
    let keyword = keyword.as_str();
    if keyword.contains(' ') {
        return norm.contains(keyword);
    }
    norm.split(|c: char| !c.is_alphanumeric()).any(|w| {
        let mut forms = vec![w];
        let mut rest = w;
        for _ in 0..2 {
            match rest.chars().next() {
                Some(p @ ('ו' | 'ה' | 'ב' | 'ל' | 'ש' | 'מ')) => {
                    rest = &rest[p.len_utf8()..];
                    forms.push(rest);
                }
                _ => break,
            }
        }
        forms
            .iter()
            .any(|f| *f == keyword || (keyword.chars().count() >= 4 && f.starts_with(keyword)))
    })
}

/// `Some(Some(key))`: a heading of that section. `Some(None)`: a heading of something else.
/// `None`: not a heading.
pub(super) fn heading_key(line: &str, structure: &ReportStructure) -> Option<Option<String>> {
    let line = line.trim();
    let n = line.chars().count();
    let colon = line.ends_with(':');
    let word_count = line.split_whitespace().count();
    if !(2..=70).contains(&n) || line.ends_with('.') || word_count > if colon { 9 } else { 6 } {
        return None;
    }
    let norm = normalize(line.trim_end_matches(':')).to_lowercase();
    // The report's own title ("סיכום אבחון פסיכולוגי") is not its summary section.
    if REPORT_TITLES
        .iter()
        .any(|t| norm.contains(&normalize(t).to_lowercase()))
    {
        return None;
    }
    for s in structure.sections() {
        if normalize(&s.title).to_lowercase() == norm {
            return Some(Some(s.key.clone()));
        }
    }
    for (key, keywords) in HEADING_WORDS {
        if keywords.iter().any(|k| heading_matches(&norm, k)) {
            return Some(Some((*key).to_owned()));
        }
    }
    colon.then_some(None)
}

/// Cut a report into parts by its headings. Text before the first heading is its own part
/// (usually the header: names and dates), not included by default.
pub(super) fn split_parts(body: &str, structure: &ReportStructure) -> Vec<StoredPart> {
    let mut parts: Vec<StoredPart> = Vec::new();
    let mut current = StoredPart {
        section: None,
        heading: "פתיחת הדוח".to_owned(),
        text: String::new(),
    };
    for line in body.lines() {
        if let Some(key) = heading_key(line, structure) {
            if !current.text.trim().is_empty() {
                parts.push(current);
            }
            current = StoredPart {
                section: key,
                heading: line.trim().trim_end_matches(':').trim().to_owned(),
                text: String::new(),
            };
        } else {
            current.text.push_str(line);
            current.text.push('\n');
        }
    }
    if !current.text.trim().is_empty() {
        parts.push(current);
    }
    // Two parts of the same section in a row (a sub-heading) are one excerpt.
    let mut merged: Vec<StoredPart> = Vec::new();
    for p in parts {
        match merged.last_mut() {
            Some(last) if p.section.is_some() && last.section == p.section => {
                last.text.push_str(&p.text);
            }
            _ => merged.push(p),
        }
    }
    for p in &mut merged {
        p.text = p.text.trim().to_owned();
    }
    merged
}

/// Every template key the profile can describe (the signature has no prose).
pub(super) fn section_keys(structure: &ReportStructure) -> Vec<String> {
    structure
        .sections()
        .filter(|s| s.key != "signature")
        .map(|s| s.key.clone())
        .collect()
}
