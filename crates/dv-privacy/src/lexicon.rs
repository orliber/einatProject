//! Built-in lexicons (data/*.txt): names, localities, allow-listed English terms.
//! Data, not code – extend the files to improve recall (D-015).

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::matcher::PhraseIndex;
use crate::text::normalize;

fn lines(data: &str) -> impl Iterator<Item = &str> {
    data.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// What an automatically recognized place becomes in the outgoing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceKind {
    Locality,
    Hospital,
}

#[derive(Debug)]
pub struct Lexicon {
    pub first_names: HashSet<String>,
    pub word_names: HashSet<String>,
    pub places: PhraseIndex<PlaceKind>,
    pub latin_allow: HashSet<String>,
    /// Ordinary words that only look like prefix + name; never suspects.
    pub common_words: HashSet<String>,
}

/// Hospitals that identify a region; hidden as [בית_חולים].
const HOSPITALS: &[&str] = &[
    "שניידר",
    "איכילוב",
    "סורוקה",
    "רמב\"ם",
    "שיבא",
    "תל השומר",
    "וולפסון",
    "בילינסון",
    "ברזילי",
    "לניאדו",
    "שערי צדק",
    "הדסה עין כרם",
    "הדסה הר הצופים",
    "אסף הרופא",
    "שמיר",
    "קפלן",
    "הלל יפה",
    "מעייני הישועה",
    "בני ציון",
    "פוריה",
    "בית לוינשטיין",
    "אלין",
    "דנה-דואק",
    "רבין",
    "גהה",
    "אברבנאל",
    "שלוותה",
    "איתנים",
    "כפר שאול",
];

pub static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| {
    let mut places = PhraseIndex::default();
    for l in lines(include_str!("../data/localities.txt")) {
        places.insert(l, PlaceKind::Locality);
    }
    for h in HOSPITALS {
        places.insert(h, PlaceKind::Hospital);
    }
    Lexicon {
        first_names: lines(include_str!("../data/first_names.txt"))
            .map(normalize)
            .collect(),
        word_names: lines(include_str!("../data/word_names.txt"))
            .map(normalize)
            .collect(),
        places,
        latin_allow: lines(include_str!("../data/latin_allowlist.txt"))
            .map(normalize)
            .collect(),
        common_words: lines(include_str!("../data/common_words.txt"))
            .map(normalize)
            .collect(),
    }
});

/// Words before a name: prepositions, relations, titles, reporting verbs (VSO order).
pub const NAME_CONTEXT_PREV: &[&str] = &[
    "עם",
    "של",
    "את",
    "כמו",
    "אצל",
    "בין",
    "ואת",
    "ועם",
    "הגננת",
    "הסייעת",
    "הילד",
    "הילדה",
    "החבר",
    "החברה",
    "חברו",
    "חברתו",
    "אחיו",
    "אחותו",
    "אחיה",
    "אחותה",
    "אמו",
    "אביו",
    "אמה",
    "אביה",
    "בנו",
    "בתו",
    "ד\"ר",
    "דר",
    "גב'",
    "מר",
    "הפסיכולוגית",
    "הקלינאית",
    "המטפלת",
    "הרופא",
    "הרופאה",
    "המורה",
    "אמר",
    "אמרה",
    "סיפר",
    "סיפרה",
    "מספר",
    "מספרת",
    "ציין",
    "ציינה",
    "משתף",
    "משתפת",
    "שיתף",
    "שיתפה",
    "אומר",
    "אומרת",
    "טוען",
    "טוענת",
    "מתאר",
    "מתארת",
    "פנה",
    "פנתה",
    "ביקש",
    "ביקשה",
    "כשהגיע",
    "כשהגיעה",
    "שמו",
    "שמה",
    "בשם",
    "ששמו",
    "ששמה",
    "הנקרא",
    "הנקראת",
    "ע\"י",
    "ידי",
];

/// Words after a name: reporting and action verbs in third person.
pub const NAME_CONTEXT_NEXT: &[&str] = &[
    "אמר",
    "אמרה",
    "סיפר",
    "סיפרה",
    "ביקש",
    "ביקשה",
    "שיחק",
    "שיחקה",
    "הגיע",
    "הגיעה",
    "מספר",
    "מספרת",
    "מתאר",
    "מתארת",
    "משתף",
    "משתפת",
    "טוען",
    "טוענת",
    "רצה",
    "רצתה",
    "ציין",
    "ציינה",
    "פנה",
    "פנתה",
    "בכה",
    "בכתה",
    "צחק",
    "צחקה",
    "דחף",
    "דחפה",
    "הרביץ",
    "הרביצה",
    "לקח",
    "לקחה",
    "נתן",
    "נתנה",
    "המליץ",
    "המליצה",
    "אבחן",
    "אבחנה",
    "הפנה",
    "הפנתה",
    "טיפל",
    "טיפלה",
    "מטפל",
    "מטפלת",
    "בן",
    "בת",
    "אחיו",
    "אחותו",
    "וחבריו",
];

#[must_use]
pub fn is_name_context(prev: Option<&str>, next: Option<&str>) -> bool {
    let norm_in = |w: &str, list: &[&str]| list.iter().any(|l| normalize(l) == w);
    prev.is_some_and(|p| norm_in(p, NAME_CONTEXT_PREV))
        || next.is_some_and(|n| norm_in(n, NAME_CONTEXT_NEXT))
}

/// Generic words that never identify on their own ("גן" in "גן הדקל").
pub const GENERIC_PLACE_WORDS: &[&str] = &[
    "גן",
    "גני",
    "בית",
    "ספר",
    "בית ספר",
    "מכון",
    "מרכז",
    "כפר",
    "קריית",
    "קרית",
    "מושב",
    "קיבוץ",
    "עיר",
    "שכונת",
    "רחוב",
    "מעון",
    "צהרון",
    "טרום",
    "חובה",
    "יסודי",
    "תיכון",
    "בית חולים",
    "מרפאה",
    "מרפאת",
    "המכון",
    "המרכז",
    "הגן",
    "יום",
];

/// Titles that are not part of the name ("ד\"ר", "גב'").
pub const TITLES: &[&str] = &[
    "ד\"ר",
    "דר",
    "דוקטור",
    "פרופ'",
    "פרופסור",
    "מר",
    "גב'",
    "גברת",
    "הרב",
    "עו\"ד",
];
