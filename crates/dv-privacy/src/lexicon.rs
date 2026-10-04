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
    /// Common surnames: flagged next to a name, after "משפחת" or after a name label.
    pub surnames: HashSet<String>,
    pub places: PhraseIndex<PlaceKind>,
    pub latin_allow: HashSet<String>,
    /// Ordinary words that only look like prefix + name; never suspects.
    pub common_words: HashSet<String>,
    /// A first name → the other members of its nickname group ("שמעון" → "שימי").
    pub nicknames: std::collections::HashMap<String, Vec<String>>,
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
        surnames: lines(include_str!("../data/surnames.txt"))
            .map(normalize)
            .collect(),
        places,
        latin_allow: lines(include_str!("../data/latin_allowlist.txt"))
            .map(normalize)
            .collect(),
        common_words: lines(include_str!("../data/common_words.txt"))
            .map(normalize)
            .collect(),
        nicknames: nickname_groups(include_str!("../data/nicknames.txt")),
    }
});

fn nickname_groups(data: &str) -> std::collections::HashMap<String, Vec<String>> {
    let mut out: std::collections::HashMap<String, Vec<String>> = Default::default();
    for line in lines(data) {
        let group: Vec<String> = line.split_whitespace().map(normalize).collect();
        for member in &group {
            let others = out.entry(member.clone()).or_default();
            for other in &group {
                if other != member && !others.contains(other) {
                    others.push(other.clone());
                }
            }
        }
    }
    out
}

/// A sorted `key<TAB>value` table embedded in the binary (data/*.tsv), searched in place:
/// only the line offsets are kept, so 300,000 word forms cost about 2 MB of memory.
#[derive(Debug)]
pub struct SortedTable {
    data: &'static str,
    /// (start of the line, end of the key) for every data line, in key order.
    lines: Vec<(u32, u32)>,
}

impl SortedTable {
    fn new(data: &'static str) -> Self {
        let mut lines = Vec::new();
        let mut start = 0usize;
        for line in data.split_inclusive('\n') {
            if !line.starts_with('#') && !line.trim().is_empty() {
                let key_len = line.find('\t').unwrap_or(line.trim_end().len());
                if let (Ok(s), Ok(e)) = (u32::try_from(start), u32::try_from(start + key_len)) {
                    lines.push((s, e));
                }
            }
            start += line.len();
        }
        Self { data, lines }
    }

    /// The value column(s) for `key`, if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&'static str> {
        let data = self.data;
        let i = self
            .lines
            .binary_search_by(|&(s, e)| data[s as usize..e as usize].cmp(key))
            .ok()?;
        let (_, e) = self.lines[i];
        let rest = &self.data[e as usize..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        Some(rest[..line_end].trim_start_matches('\t'))
    }
}

/// Hebrew word forms from open corpora with how common they are (scripts/build_lexicons.py),
/// in sorted shards of under 1 MB each (the repository's file-size limit).
pub static WORDS: LazyLock<[SortedTable; 6]> = LazyLock::new(|| {
    [
        SortedTable::new(include_str!("../data/words-1.tsv")),
        SortedTable::new(include_str!("../data/words-2.tsv")),
        SortedTable::new(include_str!("../data/words-3.tsv")),
        SortedTable::new(include_str!("../data/words-4.tsv")),
        SortedTable::new(include_str!("../data/words-5.tsv")),
        SortedTable::new(include_str!("../data/words-6.tsv")),
    ]
});

/// How common a normalized form is as a written word: 0 = never seen, else 1 + log2(count)
/// in a corpus of about 200 million words (≥ 12 is an everyday word).
#[must_use]
pub fn word_bucket(norm: &str) -> u8 {
    WORDS
        .iter()
        .find_map(|t| t.get(norm))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

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
    "עו\"ס",
    "פרופ",
];

/// Roles and professions in a salutation ("לכבוד הפסיכולוגית ההתפתחותית"): skipped after a
/// label like a title, so the name that may follow them is still flagged.
pub const ROLE_WORDS: &[&str] = &[
    "פסיכולוג",
    "פסיכולוגית",
    "התפתחותי",
    "התפתחותית",
    "קליני",
    "קלינית",
    "חינוכי",
    "חינוכית",
    "רופא",
    "רופאת",
    "רופאה",
    "ילדים",
    "גננת",
    "הגננת",
    "מורה",
    "מחנכת",
    "מחנך",
    "יועצת",
    "יועץ",
    "מנהלת",
    "מנהל",
    "קלינאית",
    "תקשורת",
    "מרפאה",
    "מרפא",
    "בעיסוק",
    "פיזיותרפיסטית",
    "עובדת",
    "סוציאלית",
    "צוות",
    "הורי",
    "ההורים",
];

/// Words for a person that a name may follow, set off by commas or a dash ("הגננת, ברכה,").
pub const PERSON_WORDS: &[&str] = &[
    "גננת",
    "גננת משלימה",
    "סייעת",
    "מורה",
    "מחנכת",
    "מחנך",
    "יועצת",
    "יועץ",
    "מנהלת",
    "מנהל",
    "קלינאית",
    "מרפאה",
    "מטפלת",
    "מטפל",
    "רופא",
    "רופאה",
    "פסיכולוגית",
    "פסיכולוג",
    "עובדת",
    "פיזיותרפיסטית",
    "אח",
    "אחות",
    "אחיו",
    "אחותו",
    "אחיה",
    "אחותה",
    "אמא",
    "אבא",
    "סבא",
    "סבתא",
    "דוד",
    "דודה",
    "בן דוד",
    "בת דודה",
    "חבר",
    "חברה",
    "חברו",
    "חברתו",
    "שכן",
    "שכנה",
    "בן זוג",
    "בת זוג",
    "תאום",
    "תאומה",
    "סייע",
    "מדריך",
    "מדריכה",
    "ילד",
    "ילדה",
    "נכד",
    "נכדה",
    "אחיין",
    "אחיינית",
    "מאבחנת",
    "מאבחן",
    "כלב",
    "כלבה",
    "חתול",
    "חתולה",
    "חברתה",
    "אחותי",
    "אחי",
    "בעלה",
    "אשתו",
    "נהג",
    "נהגת",
    "בבושקה",
    "סבתוש",
    "דדושקה",
    "ג'דו",
    "ג'דה",
    "סיתו",
    "תיתה",
];

/// Labels that introduce a person's name in forms and letters ("שם הילד: …").
pub const NAME_LABELS: &[&str] = &[
    "שם",
    "שם הילד",
    "שם הילדה",
    "שם הילד/ה",
    "שם המטופל",
    "שם המטופלת",
    "המטופל",
    "המטופלת",
    "מטופל",
    "מטופלת",
    "שם התלמיד",
    "שם התלמידה",
    "התלמיד",
    "התלמידה",
    "הנבדק",
    "הנבדקת",
    "נבדק",
    "נבדקת",
    "שם האם",
    "שם האב",
    "שם ההורים",
    "הורים",
    "ההורים",
    "שם פרטי",
    "שם משפחה",
    "שם הגננת",
    "הגננת",
    "המחנכת",
    "הרופא",
    "הרופאה",
    "רופא מטפל",
    "רופאה מטפלת",
    "המטפלת",
    "המטפל",
    "מפנה",
    "הפונה",
    "לכבוד",
    "חתימה",
    "בברכה",
];

/// Words that end a name after a label ("שם הילד: נועם, גיל 5").
pub const NAME_STOP: &[&str] = &[
    "גיל",
    "בן",
    "בת",
    "תאריך",
    "כיתה",
    "גן",
    "ת",
    "ז",
    "מספר",
    "טלפון",
    "כתובת",
];

/// Typical surname endings (Ashkenazi patronymics and similar).
pub const SURNAME_ENDINGS: &[&str] = &[
    "וביץ",
    "וביץ'",
    "ביץ",
    "וויץ",
    "סקי",
    "צקי",
    "סקה",
    "שטיין",
    "שטין",
    "בוים",
    "בלאט",
    "זון",
    "ברג",
    "ינסקי",
    "יאן",
    "ונוב",
    "ייב",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_table_finds_keys_and_values() {
        let t = SortedTable::new("# comment\nא\t1\nבית\t12\nזמנ\t15\n");
        assert_eq!(t.get("א"), Some("1"));
        assert_eq!(t.get("בית"), Some("12"));
        assert_eq!(t.get("זמנ"), Some("15"));
        assert_eq!(t.get("ב"), None);
        assert_eq!(t.get("זמן"), None);
    }

    #[test]
    fn word_statistics_know_ordinary_words() {
        // Everyday words, including ones that look like prefix + name or like a name.
        for w in ["זמנ", "מתנ", "אליה", "איתה", "מלאה", "דמיונ", "שאלונ"]
        {
            assert!(word_bucket(w) >= 8, "{w}: {}", word_bucket(w));
        }
        assert_eq!(word_bucket("קסלפטרונ"), 0);
    }
}
