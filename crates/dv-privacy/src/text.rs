//! Hebrew-aware normalization, tokenization, prefixes and spelling variants.

/// Hebrew one-letter prefixes (ו ה ב ל מ ש כ), which attach to names: "ולנועם", "שמיכל".
pub const PREFIX_LETTERS: [char; 7] = ['ו', 'ה', 'ב', 'ל', 'מ', 'ש', 'כ'];
const MAX_PREFIX: usize = 4;

/// Hebrew prefixes stack in a fixed order: conjunction, relative, preposition, article
/// ("ו" → "ש/כש/לכש/מש" → "ב/ל/כ/מ" → "ה"). Only such sequences are split off, so
/// "בשאלון" is not read as ב+ש+"אלון" (בש is not a Hebrew prefix).
#[must_use]
pub fn is_valid_prefix(prefix: &str) -> bool {
    static VALID: std::sync::LazyLock<std::collections::HashSet<String>> =
        std::sync::LazyLock::new(|| {
            const CONJ: [&str; 2] = ["", "ו"];
            const REL: [&str; 5] = ["", "ש", "כש", "לכש", "מש"];
            const PREP: [&str; 5] = ["", "ב", "ל", "כ", "מ"];
            const ART: [&str; 2] = ["", "ה"];
            let mut all = std::collections::HashSet::new();
            for c in CONJ {
                for r in REL {
                    for p in PREP {
                        for a in ART {
                            all.insert(format!("{c}{r}{p}{a}"));
                        }
                    }
                }
            }
            all
        });
    !prefix.is_empty() && VALID.contains(prefix)
}

fn is_hebrew_letter(c: char) -> bool {
    ('\u{05D0}'..='\u{05EA}').contains(&c)
}

/// Points, cantillation and other marks that do not change the letters.
pub(crate) fn is_mark(c: char) -> bool {
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
                // Invisible format characters: a zero-width space, joiners, direction marks,
                // embeddings, overrides and isolates, the word joiner, a soft hyphen, the
                // combining grapheme joiner, the BOM. None shows on screen, so inside a word
                // they must not split it: "נו\u{200B}עם" reads as "נועם".
                | '\u{00AD}'
                | '\u{034F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
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

/// The plain character a compatibility form shows: full-width ASCII ("Ｎｏａｍ", "１２３") as
/// ASCII, and Hebrew presentation forms (wide letters, letters with a dagesh or a shin dot,
/// as PDF text often holds them: "ﬠ", "שׁ", "וֹ") as the bare letter.
#[must_use]
pub(crate) fn compat_char(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c),
        '\u{FB1D}' | '\u{FB39}' => 'י',
        '\u{FB20}' => 'ע',
        '\u{FB21}' | '\u{FB2E}' | '\u{FB2F}' | '\u{FB30}' => 'א',
        '\u{FB22}' | '\u{FB33}' => 'ד',
        '\u{FB23}' | '\u{FB34}' => 'ה',
        '\u{FB24}' | '\u{FB3B}' | '\u{FB4D}' => 'כ',
        '\u{FB25}' | '\u{FB3C}' => 'ל',
        '\u{FB26}' => 'ם',
        '\u{FB27}' | '\u{FB48}' => 'ר',
        '\u{FB28}' | '\u{FB4A}' => 'ת',
        '\u{FB29}' => '+',
        '\u{FB2A}'..='\u{FB2D}' | '\u{FB49}' => 'ש',
        '\u{FB31}' | '\u{FB4C}' => 'ב',
        '\u{FB32}' => 'ג',
        '\u{FB35}' | '\u{FB4B}' => 'ו',
        '\u{FB36}' => 'ז',
        '\u{FB38}' => 'ט',
        '\u{FB3A}' => 'ך',
        '\u{FB3E}' => 'מ',
        '\u{FB40}' => 'נ',
        '\u{FB41}' => 'ס',
        '\u{FB43}' => 'ף',
        '\u{FB44}' | '\u{FB4E}' => 'פ',
        '\u{FB46}' => 'צ',
        '\u{FB47}' => 'ק',
        c => lookalike(c),
    }
}

/// A Cyrillic or Greek letter that looks like a Latin one ("Nоam" with a Cyrillic о) reads
/// as that Latin letter, so a name typed with one does not slip past every rule.
fn lookalike(c: char) -> char {
    match c {
        'а' | 'α' => 'a',
        'А' | 'Α' => 'A',
        'В' | 'Β' => 'B',
        'с' | 'ϲ' => 'c',
        'С' | 'Ϲ' => 'C',
        'е' => 'e',
        'Е' | 'Ε' => 'E',
        'Н' | 'Η' => 'H',
        'і' | 'ι' => 'i',
        'І' | 'Ι' => 'I',
        'ј' => 'j',
        'Ј' => 'J',
        'К' | 'Κ' => 'K',
        'М' | 'Μ' => 'M',
        'Ν' => 'N',
        'о' | 'ο' => 'o',
        'О' | 'Ο' => 'O',
        'р' | 'ρ' => 'p',
        'Р' | 'Ρ' => 'P',
        'ѕ' => 's',
        'Ѕ' => 'S',
        'Т' | 'Τ' => 'T',
        'у' => 'y',
        'Υ' | 'Ү' => 'Y',
        'х' | 'χ' => 'x',
        'Х' | 'Χ' => 'X',
        'Ζ' => 'Z',
        c => c,
    }
}

/// Map one character for comparison; `None` drops it.
#[must_use]
pub fn normalize_char(c: char) -> Option<char> {
    if is_mark(c) {
        return None;
    }
    let c = compat_char(c);
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

/// `normalize(raw) == norm`, without building the normalized string: the hot comparison of
/// a fixed word against a token, which mostly fails on the first letter.
#[must_use]
pub fn norm_eq<T: AsRef<str> + ?Sized>(raw: &str, norm: &T) -> bool {
    let mut want = norm.as_ref().chars();
    let mut space = false;
    let mut started = false;
    for c in raw.chars().filter_map(normalize_char) {
        if c.is_whitespace() || c == '\u{05BE}' || c == '-' {
            space = started;
            continue;
        }
        if space && want.next() != Some(' ') {
            return false;
        }
        space = false;
        started = true;
        if want.next() != Some(c) {
            return false;
        }
    }
    want.next().is_none()
}

/// The normalized forms of a fixed word list, built once per list: `norm_set(L).contains(w)`
/// is `L.iter().any(|l| normalize(l) == w)` without normalizing the list on every word.
#[must_use]
pub fn norm_set(list: &'static [&'static str]) -> &'static std::collections::HashSet<String> {
    type Sets =
        std::collections::HashMap<(usize, usize), &'static std::collections::HashSet<String>>;
    static SETS: std::sync::LazyLock<std::sync::Mutex<Sets>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(Sets::new()));
    let key = (list.as_ptr() as usize, list.len());
    let mut sets = SETS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    sets.entry(key).or_insert_with(|| {
        // Once per list in the program, never freed: the lists are constants.
        Box::leak(Box::new(list.iter().map(|w| normalize(w)).collect()))
    })
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
    let char_at = |k: usize| chars.get(k).map(|&(_, c)| c);
    let mut i = 0;
    while let Some(&(start, c)) = chars.get(i) {
        if !is_word_char(c) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while let Some(c) = char_at(j) {
            let next_is_word = char_at(j + 1).is_some_and(is_word_char);
            let inner_quote = is_inner_quote(c) && next_is_word;
            if is_word_char(c) || is_mark(c) || inner_quote {
                j += 1;
            } else if is_inner_quote(c) && c != '"' && c != '\u{05F4}' && j > i {
                // Trailing geresh as in ג'ורג' – keep it only after a Hebrew letter.
                let after_hebrew = j
                    .checked_sub(1)
                    .and_then(char_at)
                    .is_some_and(is_hebrew_letter);
                if after_hebrew && !next_is_word {
                    j += 1;
                }
                break;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
        // `start` and `end` come from `char_indices`; were the cut ever refused, the same
        // characters are read one by one, so no word is lost.
        let norm = match text.get(start..end) {
            Some(word) => normalize(word),
            None => normalize(
                &chars
                    .iter()
                    .skip(i)
                    .take(j.saturating_sub(i))
                    .map(|&(_, c)| c)
                    .collect::<String>(),
            ),
        };
        if !norm.is_empty() {
            tokens.push(Token { start, end, norm });
        }
        i = j;
    }
    join_spaced_letters(text, tokens)
}

/// Fewest single letters in a row that are read as one word.
const SPACED_MIN: usize = 3;

/// A word typed with a space between its letters ("נ ו ע ם", "N o a m") is one word: three
/// or more single letters, each one space (or two) from the next, on one line. Without this
/// a name spelled out that way reads as letters and passes every rule.
fn join_spaced_letters(text: &str, tokens: Vec<Token>) -> Vec<Token> {
    let single = |t: &Token| {
        let mut chars = t.norm.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if c.is_alphabetic())
    };
    // A gap that cannot be read (never expected) joins: the letters are read as one word.
    let close = |a: &Token, b: &Token| {
        text.get(a.end..b.start).is_none_or(|gap| {
            (1..=2).contains(&gap.len()) && gap.chars().all(|c| c == ' ' || c == '\u{00A0}')
        })
    };
    // The first letter may carry a prefix: "שנ ו ע ם" is ש + "נועם".
    let starter = |t: &Token| {
        let chars: Vec<char> = t.norm.chars().collect();
        single(t)
            || ((2..=MAX_PREFIX + 1).contains(&chars.len())
                && chars.split_last().is_some_and(|(last, head)| {
                    last.is_alphabetic() && is_valid_prefix(&head.iter().collect::<String>())
                }))
    };
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut rest = tokens.into_iter().peekable();
    while let Some(first) = rest.next() {
        let joins = starter(&first);
        let mut run = vec![first];
        while let Some(next) =
            rest.next_if(|t| joins && single(t) && run.last().is_some_and(|prev| close(prev, t)))
        {
            run.push(next);
        }
        match (run.first(), run.last()) {
            (Some(a), Some(b)) if run.len() >= SPACED_MIN => {
                let joined = Token {
                    start: a.start,
                    end: b.end,
                    norm: run.iter().map(|t| t.norm.as_str()).collect(),
                };
                out.push(joined);
            }
            _ => out.extend(run),
        }
    }
    out
}

/// The word itself and the word with 1–3 leading prefix letters removed (remainder ≥ 2 letters).
/// Returns `(prefix letter count, remainder)`.
#[must_use]
pub fn prefix_splits(norm: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = norm.chars().collect();
    let mut out = vec![(0, norm.to_owned())];
    for n in 1..=MAX_PREFIX {
        let Some((head, rest)) = chars.split_at_checked(n) else {
            break;
        };
        if rest.len() < 2 || !head.iter().all(|c| PREFIX_LETTERS.contains(c)) {
            break;
        }
        if is_valid_prefix(&head.iter().collect::<String>()) {
            out.push((n, rest.iter().collect()));
            // A prefix before a quoted word: ל"נועם", ב״גן הדקל״, ו'מיכל'. The tokenizer keeps
            // the opening quote inside the word, and the quote counts as one more character
            // split off, so the remainder still starts where the name does.
            if let Some((&('"' | '\''), after)) = rest.split_first() {
                if rest.len() >= 3 {
                    out.push((n + 1, after.iter().collect()));
                }
            }
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
        let inner = chars.iter().enumerate().take(chars.len() - 1).skip(1);
        for (i, c) in inner {
            if matches!(c, 'ו' | 'י') {
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
    // The distance from all of `a` so far to all of `b`: the last cell of the last row.
    let mut last = b.len();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = Vec::with_capacity(b.len() + 1);
        let mut left = i + 1;
        cur.push(left);
        // (diagonal, above) = (prev[j], prev[j + 1]) for each letter b[j].
        for (cb, (diag, up)) in b.iter().zip(prev.iter().zip(prev.iter().skip(1))) {
            left = (diag + usize::from(ca != cb)).min(up + 1).min(left + 1);
            cur.push(left);
        }
        if cur.iter().min().copied().unwrap_or(0) > cap {
            return cap + 1;
        }
        last = left;
        prev = cur;
    }
    last
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
    long.iter().enumerate().any(|(i, c)| {
        weak(c)
            && long
                .iter()
                .enumerate()
                .filter(|&(k, _)| k != i)
                .map(|(_, c)| c)
                .eq(short.iter())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `norm_eq` is exactly `normalize(a) == b`, for every pair of these texts.
    #[test]
    fn norm_eq_agrees_with_normalize() {
        let texts = [
            "",
            " ",
            "  ",
            "נוֹעַם",
            "נועמ",
            "נועם",
            "ד״ר  שטרן",
            "ד\"ר שטרנ",
            "בית ספר",
            " בית\u{00A0}ספר ",
            "בית-ספר",
            "בית\u{05BE}ספר",
            "בית ספר ",
            "ביתספר",
            "ABC",
            "abc",
            "גן\u{200B}",
            "גנ",
            "ג",
            "ג ן",
            "\tגן\n",
            "ע״ש",
            "ע\"ש",
            "עש",
            "ב'",
            "ב׳",
            "ب",
            "أحمد",
            "احمد",
            "ז'ורז'",
            "ז׳ורז׳",
        ];
        for a in texts {
            for b in texts {
                let want = normalize(a) == b;
                assert_eq!(norm_eq(a, b), want, "{a:?} vs {b:?}");
                let nb = normalize(b);
                assert_eq!(
                    norm_eq(a, &nb),
                    normalize(a) == nb,
                    "{a:?} vs normalized {b:?}"
                );
            }
        }
        for list in [
            crate::lexicon::PERSON_WORDS,
            crate::lexicon::TITLES,
            crate::lexicon::NAME_CONTEXT_PREV,
            crate::lexicon::GENERIC_PLACE_WORDS,
        ] {
            for w in list {
                assert!(norm_eq(w, &normalize(w)), "{w}");
            }
        }
    }

    /// `norm_set(L).contains(w)` is exactly `L.iter().any(|l| normalize(l) == w)`.
    #[test]
    fn norm_set_agrees_with_normalizing_the_list() {
        for list in [
            crate::lexicon::PERSON_WORDS,
            crate::lexicon::TITLES,
            crate::lexicon::GENERIC_PLACE_WORDS,
        ] {
            let set = norm_set(list);
            assert!(std::ptr::eq(set, norm_set(list)), "built once");
            for w in list
                .iter()
                .chain(["", "נועם", "בית", "גננת", "הגננת", "ד״ר"].iter())
            {
                let n = normalize(w);
                assert_eq!(
                    set.contains(&n),
                    list.iter().any(|l| normalize(l) == n),
                    "{w}"
                );
                assert_eq!(
                    set.contains(*w),
                    list.iter().any(|l| normalize(l) == *w),
                    "{w}"
                );
            }
        }
    }

    #[test]
    fn valid_prefixes_are_the_ordered_stacks() {
        for p in [
            "ו",
            "ה",
            "ב",
            "ל",
            "כ",
            "מ",
            "ש",
            "וה",
            "וב",
            "ושב",
            "וכשה",
            "ולכשבה",
            "מה",
            "שה",
        ] {
            assert!(is_valid_prefix(p), "{p}");
        }
        for p in ["", "בש", "הב", "בו", "ששש", "x"] {
            assert!(!is_valid_prefix(p), "{p}");
        }
    }

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
