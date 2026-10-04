//! Hebrew-aware normalization, tokenization, prefixes and spelling variants.

/// Hebrew one-letter prefixes (ו ה ב ל מ ש כ), which attach to names: "ולנועם", "שמיכל".
pub const PREFIX_LETTERS: [char; 7] = ['ו', 'ה', 'ב', 'ל', 'מ', 'ש', 'כ'];
const MAX_PREFIX: usize = 4;

/// Hebrew prefixes stack in a fixed order: conjunction, relative, preposition, article
/// ("ו" → "ש/כש/לכש/מש" → "ב/ל/כ/מ" → "ה"). Only such sequences are split off, so
/// "בשאלון" is not read as ב+ש+"אלון" (בש is not a Hebrew prefix).
#[must_use]
pub fn is_valid_prefix(prefix: &str) -> bool {
    const CONJ: [&str; 2] = ["", "ו"];
    const REL: [&str; 5] = ["", "ש", "כש", "לכש", "מש"];
    const PREP: [&str; 5] = ["", "ב", "ל", "כ", "מ"];
    const ART: [&str; 2] = ["", "ה"];
    !prefix.is_empty()
        && CONJ.iter().any(|c| {
            REL.iter().any(|r| {
                PREP.iter()
                    .any(|p| ART.iter().any(|a| format!("{c}{r}{p}{a}") == prefix))
            })
        })
}

fn is_hebrew_letter(c: char) -> bool {
    ('\u{05D0}'..='\u{05EA}').contains(&c)
}

/// Points, cantillation and other marks that do not change the letters.
fn is_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0591}'
            ..='\u{05BD}'
                | '\u{05BF}'
                | '\u{05C1}'
                | '\u{05C2}'
                | '\u{05C4}'
                | '\u{05C5}'
                | '\u{05C7}'
                // Arabic vowel signs, Quranic marks and the tatweel.
                | '\u{0610}'..='\u{061A}'
                | '\u{064B}'..='\u{065F}'
                | '\u{0670}'
                | '\u{06D6}'..='\u{06ED}'
                | '\u{0640}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{200D}'
                | '\u{200C}'
                | '\u{FEFF}'
    )
}

fn is_word_char(c: char) -> bool {
    is_hebrew_letter(c) || c.is_ascii_alphanumeric() || c.is_alphanumeric()
}

fn is_inner_quote(c: char) -> bool {
    matches!(
        c,
        '\'' | '"'
            | '\u{05F3}'
            | '\u{05F4}'
            | '\u{2019}'
            | '\u{2018}'
            | '\u{201C}'
            | '\u{201D}'
            | '`'
    )
}

/// Map one character for comparison; `None` drops it.
#[must_use]
pub fn normalize_char(c: char) -> Option<char> {
    if is_mark(c) {
        return None;
    }
    Some(match c {
        'ך' => 'כ',
        'ם' => 'מ',
        'ן' => 'נ',
        'ף' => 'פ',
        'ץ' => 'צ',
        '\u{05F3}' | '\u{2019}' | '\u{2018}' | '`' => '\'',
        '\u{05F4}' | '\u{201C}' | '\u{201D}' => '"',
        // Arabic: one alef, one ya, ta marbuta as ha.
        'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
        'ى' | 'ئ' => 'ي',
        'ؤ' => 'و',
        'ة' => 'ه',
        c if c.is_ascii_uppercase() => c.to_ascii_lowercase(),
        c => c,
    })
}

/// Normalized form used for every comparison: no points, no final letters, one kind of
/// geresh/gershayim, lowercase Latin, single spaces.
#[must_use]
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars().filter_map(normalize_char) {
        if c.is_whitespace() || c == '\u{05BE}' || c == '-' {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim_end().to_owned()
}

/// A word in the original text (`start..end` are byte offsets) and its normalized form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub norm: String,
}

/// Split into words. Inner quotes stay inside a word (ד"ר, ג'ורג'); marks are skipped.
#[must_use]
pub fn tokenize(text: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if !is_word_char(c) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() {
            let c = chars[j].1;
            let inner_quote =
                is_inner_quote(c) && j + 1 < chars.len() && is_word_char(chars[j + 1].1);
            if is_word_char(c) || is_mark(c) || inner_quote {
                j += 1;
            } else if is_inner_quote(c) && c != '"' && c != '\u{05F4}' && j > i {
                // Trailing geresh as in ג'ורג' – keep it only after a Hebrew letter.
                if is_hebrew_letter(chars[j - 1].1)
                    && (j + 1 == chars.len() || !is_word_char(chars[j + 1].1))
                {
                    j += 1;
                }
                break;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
        let norm = normalize(&text[start..end]);
        if !norm.is_empty() {
            tokens.push(Token { start, end, norm });
        }
        i = j;
    }
    tokens
}

/// The word itself and the word with 1–3 leading prefix letters removed (remainder ≥ 2 letters).
/// Returns `(prefix letter count, remainder)`.
#[must_use]
pub fn prefix_splits(norm: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = norm.chars().collect();
    let mut out = vec![(0, norm.to_owned())];
    for n in 1..=MAX_PREFIX {
        if chars.len() < n + 2 || !chars[..n].iter().all(|c| PREFIX_LETTERS.contains(c)) {
            break;
        }
        if is_valid_prefix(&chars[..n].iter().collect::<String>()) {
            out.push((n, chars[n..].iter().collect()));
        }
    }
    out
}

/// Byte length in `original` of the first `letters` letters (points in between are skipped).
#[must_use]
pub fn byte_len_of_letters(original: &str, letters: usize) -> usize {
    let mut seen = 0;
    for (i, c) in original.char_indices() {
        if seen == letters {
            return i;
        }
        if !is_mark(c) {
            seen += 1;
        }
    }
    original.len()
}

/// Plene/defective spelling variants: drop an inner ו or י ("נועם" → "נעם", "מיכאל" stays).
#[must_use]
pub fn spelling_variants(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut out = vec![word.to_owned()];
    if chars.len() >= 4 && chars.iter().all(|c| is_hebrew_letter(*c)) {
        for i in 1..chars.len() - 1 {
            if matches!(chars[i], 'ו' | 'י') {
                let v: String = chars
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, c)| *c)
                    .collect();
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// Levenshtein distance over characters, capped (returns `cap + 1` once exceeded).
#[must_use]
pub fn edit_distance(a: &str, b: &str, cap: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        if cur.iter().min().copied().unwrap_or(0) > cap {
            return cap + 1;
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Letters Hebrew spellings of names vary in (matres lectionis, gutturals): א ה ו י ע.
const WEAK_LETTERS: [char; 5] = ['א', 'ה', 'ו', 'י', 'ע'];

/// One edit apart, and the edit touches only weak letters: "נואם"/"נועם", "נעם"/"נועם",
/// but not "הילד"/"הילה" (a real word, differing in ד).
#[must_use]
pub fn weak_near(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let weak = |c: &char| WEAK_LETTERS.contains(c);
    if a == b {
        return false;
    }
    if a.len() == b.len() {
        let diffs: Vec<(char, char)> = a
            .iter()
            .zip(&b)
            .filter(|(x, y)| x != y)
            .map(|(x, y)| (*x, *y))
            .collect();
        return diffs.len() == 1 && diffs.iter().all(|(x, y)| weak(x) && weak(y));
    }
    let (long, short) = if a.len() > b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if long.len() != short.len() + 1 {
        return false;
    }
    (0..long.len())
        .any(|i| weak(&long[i]) && long[..i].iter().chain(&long[i + 1..]).eq(short.iter()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_removes_points_and_final_letters() {
        assert_eq!(normalize("נוֹעַם"), "נועמ");
        assert_eq!(normalize("ד״ר  שטרן"), "ד\"ר שטרנ");
        assert_eq!(normalize("Noam"), "noam");
    }

    #[test]
    fn tokenizer_keeps_titles_and_geresh_inside_words() {
        let t: Vec<String> = tokenize("ד\"ר שטרן, ג'ורג' ו-נועם!")
            .into_iter()
            .map(|t| t.norm)
            .collect();
        assert_eq!(t, vec!["ד\"ר", "שטרנ", "ג'ורג'", "ו", "נועמ"]);
    }

    #[test]
    fn prefixes_are_split_off() {
        let splits = prefix_splits("ולנועמ");
        assert!(splits.contains(&(2, "נועמ".to_owned())));
        assert_eq!(prefix_splits("נועמ"), vec![(0, "נועמ".to_owned())]);
        assert!(prefix_splits("וכשנועמ").contains(&(3, "נועמ".to_owned())));
    }

    #[test]
    fn only_grammatical_prefix_sequences_are_split() {
        for ok in ["ו", "ש", "כש", "וכש", "ולכש", "של", "שב", "מה", "ובה"] {
            assert!(is_valid_prefix(ok), "{ok}");
        }
        for bad in ["בש", "שוכש", "הב", "לו", ""] {
            assert!(!is_valid_prefix(bad), "{bad}");
        }
        assert!(!prefix_splits("בשאלונ").iter().any(|(_, r)| r == "אלונ"));
    }

    #[test]
    fn defective_spelling_variants() {
        assert!(spelling_variants("נועמ").contains(&"נעמ".to_owned()));
        assert!(spelling_variants("מיכל").contains(&"מכל".to_owned()));
    }

    #[test]
    fn weak_letter_near_misses() {
        assert!(weak_near("נואמ", "נועמ"));
        assert!(weak_near("נעמ", "נועמ"));
        assert!(!weak_near("הילד", "הילה"));
        assert!(!weak_near("הילדה", "הילה"));
        assert!(!weak_near("נועמ", "נועמ"));
    }

    #[test]
    fn edit_distance_is_capped() {
        assert_eq!(edit_distance("נועמ", "נועה", 2), 1);
        assert_eq!(edit_distance("אבג", "דהוזח", 1), 2);
    }
}
