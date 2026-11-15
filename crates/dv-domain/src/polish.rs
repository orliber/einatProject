//! Proofreading ("הגהה", D-167): small things a careful reader would mark in a paragraph,
//! found in code on her computer (no AI). Each note is short, quotes at most three words of
//! the text, and only suggests: she decides. It is advice for the finish, never a block, and
//! never holds a paragraph back from being approved.
//!
//! Square-bracket tags (`[ילד]`, `[תוצאה 7 · ממוצע נמוך]`) are read as one word and never
//! noted, except a "[חסר" marker left to complete.

/// A tag in square brackets stands as this one character while the text is read.
const TAG: char = '\u{E000}';

/// A sentence longer than this many words is pointed out.
pub const LONG_SENTENCE_WORDS: usize = 45;

/// Words that are doubled on purpose ("לאט לאט", "אחד אחד", "צעד צעד").
const DOUBLED_ON_PURPOSE: &[&str] = &[
    "לאט",
    "אחד",
    "אחת",
    "צעד",
    "מעט",
    "יום",
    "שניים",
    "שתיים",
    "זוג",
    "מילה",
    "שלב",
];

/// Absolute words: rarely exact about a child.
const ABSOLUTE: &[&str] = &["תמיד"];
/// Judgmental or outdated words, as whole words: a description says more.
const JUDGMENTAL: &[&str] = &[
    "עצלן",
    "עצלנית",
    "עצלנות",
    "מפונק",
    "מפונקת",
    "גרוע",
    "גרועה",
    "טיפש",
    "טיפשה",
    "מפגר",
    "מפגרת",
    "פיגור",
];

/// Every proofreading note on a paragraph, in a fixed order, each once.
#[must_use]
pub fn polish_notes(text: &str) -> Vec<String> {
    let masked = mask_tags(text);
    let mut out: Vec<String> = Vec::new();
    let mut push = |n: String| {
        if !out.contains(&n) {
            out.push(n);
        }
    };
    for n in placeholders(text) {
        push(n);
    }
    for n in doubled_words(&masked) {
        push(n);
    }
    for n in spacing(&masked) {
        push(n);
    }
    for n in unbalanced(&masked) {
        push(n);
    }
    for n in long_sentences(&masked) {
        push(n);
    }
    if let Some(n) = unfinished(text) {
        push(n);
    }
    for n in wording(&masked) {
        push(n);
    }
    out
}

/// The text with every `[...]` on one line (up to 80 characters inside) as one [`TAG`].
fn mask_tags(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '[' {
            let close = chars
                .iter()
                .skip(i + 1)
                .take(81)
                .take_while(|c| **c != '\n' && **c != '[')
                .position(|c| *c == ']');
            if let Some(len) = close {
                out.push(TAG);
                i += len + 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A word as quoted in a note: a tag shown as "[…]".
fn shown(word: &str) -> String {
    word.replace(TAG, "[…]")
}

/// At most three words, quoted.
fn quote(words: &[&str]) -> String {
    let w: Vec<String> = words.iter().take(3).map(|w| shown(w)).collect();
    format!("\"{}\"", w.join(" "))
}

/// A word without the punctuation around it.
fn core(token: &str) -> &str {
    token
        .trim_matches(|c: char| {
            !(c.is_alphanumeric() || c == TAG || c == '-' || c == '"' || c == '\'')
        })
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '-')
}

/// `word` without one or two of the common prefix letters from `prefixes`, when the rest is
/// one of `stems`.
fn stem_of<'a>(word: &str, stems: &[&'a str], prefixes: &[char]) -> Option<&'a str> {
    stems.iter().copied().find(|s| {
        word.strip_suffix(*s).is_some_and(|pre| {
            pre.chars().count() <= 2 && pre.chars().all(|c| prefixes.contains(&c))
        })
    })
}

fn is_hebrew(c: char) -> bool {
    ('\u{05d0}'..='\u{05ea}').contains(&c)
}

/// "[חסר", a template slot "{...}", "XXX", "???", "TODO": something left to fill in.
fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("[חסר") {
        let rest = text.get(at..).unwrap_or_default();
        let marker = rest
            .split_inclusive([']', '\n'])
            .next()
            .unwrap_or(rest)
            .trim_end();
        let words: Vec<&str> = marker.split_whitespace().collect();
        out.push(format!("נשאר סימון להשלמה: {}", quote(&words)));
    }
    for line in text.lines() {
        let mut rest = line;
        while let Some(open) = rest.find('{') {
            let after = rest.get(open + 1..).unwrap_or_default();
            let Some(close) = after.find('}') else {
                break;
            };
            let inner = after.get(..close).unwrap_or_default();
            let words: Vec<&str> = inner.split_whitespace().collect();
            out.push(format!(
                "נשאר מקום למילוי מהתבנית: \"{{{}}}\"",
                words.iter().take(3).copied().collect::<Vec<_>>().join(" ")
            ));
            rest = after.get(close + 1..).unwrap_or_default();
        }
    }
    let marks = text.split_whitespace().map(core).any(|w| {
        (w.chars().count() >= 3 && w.chars().all(|c| c == 'X' || c == 'x')) || w == "TODO"
    });
    if marks {
        let w = text
            .split_whitespace()
            .map(core)
            .find(|w| {
                (w.chars().count() >= 3 && w.chars().all(|c| c == 'X' || c == 'x')) || *w == "TODO"
            })
            .unwrap_or_default();
        out.push(format!("נשאר סימון למילוי: \"{w}\""));
    }
    if text.contains("???") {
        out.push("נשאר סימון למילוי: \"???\"".to_owned());
    }
    out
}

/// The same word twice in a row ("של של"), not a number and not a pair said on purpose.
fn doubled_words(masked: &str) -> Vec<String> {
    let tokens: Vec<&str> = masked.split_whitespace().collect();
    let mut out = Vec::new();
    for pair in tokens.windows(2) {
        let [a, b] = pair else { continue };
        let (ca, cb) = (core(a), core(b));
        // Punctuation between them ("מאוד, מאוד") is a choice, not a slip.
        if ca != *a || !b.starts_with(cb) {
            continue;
        }
        let word = ca.to_lowercase();
        if word != cb.to_lowercase()
            || !word.chars().any(char::is_alphabetic)
            || word.chars().any(|c| c.is_ascii_digit())
            || word.contains(TAG)
            || DOUBLED_ON_PURPOSE.contains(&ca)
        {
            continue;
        }
        out.push(format!("מילה כפולה: {}", quote(&[ca, cb])));
    }
    out
}

/// The word that ends right before byte `at` of `line`, for a note.
fn word_before(line: &str, at: usize) -> Option<&str> {
    line.get(..at)?
        .split_whitespace()
        .next_back()
        .map(core)
        .filter(|w| !w.is_empty())
}

/// Spaces and punctuation: a double space, a space before a punctuation mark or after "(", no
/// space after "," or "." before a Hebrew word, and a mark doubled (",,", "..", but "..." is an
/// ellipsis).
fn spacing(masked: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in masked.lines() {
        let body = line.trim();
        let chars: Vec<(usize, char)> = body.char_indices().collect();
        for (k, &(at, c)) in chars.iter().enumerate() {
            let prev = k.checked_sub(1).and_then(|j| chars.get(j)).map(|(_, c)| *c);
            let next = chars.get(k + 1).map(|(_, c)| *c);
            let next2 = chars.get(k + 2).map(|(_, c)| *c);
            if c == ' ' && next == Some(' ') && prev != Some(' ') {
                if let Some(w) = word_before(body, at) {
                    out.push(format!("רווח כפול אחרי {}", quote(&[w])));
                }
            }
            if matches!(c, ',' | '.' | ';' | ':' | ')') && prev == Some(' ') {
                let ellipsis = c == '.' && next == Some('.');
                if !ellipsis {
                    if let Some(w) = word_before(body, at) {
                        out.push(format!("רווח לפני \"{c}\" אחרי {}", quote(&[w])));
                    }
                }
            }
            if c == '(' && next == Some(' ') {
                if let Some(w) = body.get(at + 1..).and_then(|r| r.split_whitespace().next()) {
                    out.push(format!("רווח אחרי \"(\" לפני {}", quote(&[core(w)])));
                }
            }
            if matches!(c, ',' | '.') && next.is_some_and(is_hebrew) && prev.is_some_and(is_hebrew)
            {
                // The letters before the mark: one letter is an abbreviation ("ת.ז").
                let before = body.get(..at).unwrap_or_default();
                let run = before.chars().rev().take_while(|c| is_hebrew(*c)).count();
                let after = body.get(at + 1..).unwrap_or_default();
                let next_run: Vec<char> = after.chars().take_while(|c| is_hebrew(*c)).collect();
                let abbreviation = run == 1
                    || (next_run.len() == 1
                        && after
                            .chars()
                            .nth(1)
                            .is_some_and(|c| matches!(c, '.' | '"' | '\'')));
                if !abbreviation {
                    let start = before.rfind(char::is_whitespace).map_or(0, |j| j + 1);
                    let end = after
                        .find(char::is_whitespace)
                        .map_or(body.len(), |j| at + 1 + j);
                    let glued = body.get(start..end).unwrap_or_default();
                    out.push(format!("חסר רווח אחרי \"{c}\" ב-\"{}\"", shown(glued)));
                }
            }
            if matches!(c, ',' | ';' | ':') && next == Some(c)
                || matches!((c, next), (',', Some('.')) | ('.', Some(',')))
                || (c == '.' && next == Some('.') && prev != Some('.') && next2 != Some('.'))
            {
                let mark: String = [Some(c), next].iter().flatten().collect();
                match word_before(body, at) {
                    Some(w) => out.push(format!("סימן פיסוק כפול \"{mark}\" אחרי {}", quote(&[w]))),
                    None => out.push(format!("סימן פיסוק כפול \"{mark}\"")),
                }
            }
        }
    }
    out
}

/// Brackets and quotation marks that do not close. A list mark ("1)", "א)") is not a
/// bracket, and a quote between two letters is the gershayim of an abbreviation ("ע\"י",
/// "ד\"ר"), not a quotation mark.
fn unbalanced(masked: &str) -> Vec<String> {
    let chars: Vec<char> = masked.chars().collect();
    let mut depth = 0usize;
    let mut stray_close = false;
    let mut quotes = 0usize;
    for (k, &c) in chars.iter().enumerate() {
        let prev = k.checked_sub(1).and_then(|j| chars.get(j)).copied();
        let next = chars.get(k + 1).copied();
        match c {
            '(' => depth += 1,
            ')' => {
                // "1)" or "א)" at the start of a word: a list mark.
                let token: Vec<char> = chars
                    .get(..k)
                    .unwrap_or_default()
                    .iter()
                    .rev()
                    .take_while(|c| !c.is_whitespace())
                    .copied()
                    .collect();
                let list_mark = !token.is_empty()
                    && token.len() <= 2
                    && token.iter().all(|c| c.is_alphanumeric())
                    && depth == 0;
                if list_mark {
                    continue;
                }
                match depth.checked_sub(1) {
                    Some(d) => depth = d,
                    None => stray_close = true,
                }
            }
            '"' => {
                // Gershayim stand before an abbreviation's last letter ("ע\"י", "צה\"ל"), or
                // inside a word that is more than prefix letters ("דו\"חות"). After a prefix
                // alone ("ה\"משחק\"") the mark opens a quotation.
                let letters_before: Vec<char> = chars
                    .get(..k)
                    .unwrap_or_default()
                    .iter()
                    .rev()
                    .take_while(|c| c.is_alphabetic())
                    .copied()
                    .collect();
                let letters_after = chars
                    .iter()
                    .skip(k + 1)
                    .take_while(|c| c.is_alphabetic())
                    .count();
                let only_prefix = letters_before
                    .iter()
                    .all(|c| matches!(c, 'ו' | 'ה' | 'ב' | 'ל' | 'מ' | 'ש' | 'כ'));
                let gershayim = prev.is_some_and(char::is_alphabetic)
                    && next.is_some_and(char::is_alphabetic)
                    && (letters_after == 1 || !only_prefix);
                if !gershayim {
                    quotes += 1;
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if depth > 0 {
        out.push("סוגריים: נפתחו ולא נסגרו".to_owned());
    }
    if stray_close {
        out.push("סוגריים: נסגרו בלי שנפתחו".to_owned());
    }
    if quotes % 2 == 1 {
        out.push("מירכאות: נפתחו ולא נסגרו".to_owned());
    }
    out
}

/// Sentences longer than [`LONG_SENTENCE_WORDS`] words.
fn long_sentences(masked: &str) -> Vec<String> {
    let chars: Vec<char> = masked.chars().collect();
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    for (k, &c) in chars.iter().enumerate() {
        let prev = k.checked_sub(1).and_then(|j| chars.get(j)).copied();
        let next = chars.get(k + 1).copied();
        // A decimal point ("5.4") does not end a sentence.
        let decimal = c == '.'
            && prev.is_some_and(|p| p.is_ascii_digit())
            && next.is_some_and(|n| n.is_ascii_digit());
        if matches!(c, '.' | '!' | '?' | '\n') && !decimal {
            sentences.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    sentences.push(current);
    sentences
        .iter()
        .filter_map(|s| {
            let words: Vec<&str> = s
                .split_whitespace()
                .filter(|w| w.chars().any(|c| c.is_alphanumeric() || c == TAG))
                .collect();
            (words.len() > LONG_SENTENCE_WORDS).then(|| {
                format!(
                    "משפט ארוך ({} מילים), שמתחיל ב-{}",
                    words.len(),
                    quote(&words.iter().map(|w| core(w)).collect::<Vec<_>>())
                )
            })
        })
        .collect()
}

/// A paragraph of one line and four words or more that does not end with a full stop, a
/// colon, a question or exclamation mark, or a closing bracket or quote.
fn unfinished(text: &str) -> Option<String> {
    let t = text.trim();
    if t.contains('\n') || t.split_whitespace().count() < 4 {
        return None;
    }
    let last = t.chars().next_back()?;
    if matches!(
        last,
        '.' | ':' | '?' | '!' | ')' | ']' | '"' | '\'' | '”' | '״' | '׳' | '»' | '…'
    ) {
        return None;
    }
    let word = t
        .split_whitespace()
        .next_back()
        .map(core)
        .unwrap_or_default();
    Some(format!(
        "הפסקה לא מסתיימת בסימן פיסוק (אחרי {})",
        quote(&[&shown(word)])
    ))
}

/// Absolute or judgmental words, as whole words: a suggestion to reconsider, never a rule.
fn wording(masked: &str) -> Vec<String> {
    let words: Vec<&str> = masked.split_whitespace().map(core).collect();
    let mut out = Vec::new();
    let absolute = |w: &str| format!("ניסוח מוחלט: \"{w}\" – כדאי לשקול אם זה מדויק");
    let judgmental = |w: &str| format!("ניסוח שיפוטי: \"{w}\" – כדאי לשקול ניסוח תיאורי");
    for (k, w) in words.iter().enumerate() {
        let prev = k.checked_sub(1).and_then(|j| words.get(j)).copied();
        let next = words.get(k + 1).copied();
        // "לא תמיד", "כמעט אף פעם": not absolute.
        let softened = prev.is_some_and(|p| matches!(p, "לא" | "כמעט" | "ולא"));
        if !softened {
            if stem_of(w, ABSOLUTE, &['ו', 'ש', 'כ']).is_some() {
                out.push(absolute(w));
            }
            if stem_of(w, &["אף"], &['ו', 'ש']).is_some() && next == Some("פעם") {
                out.push(absolute(&format!("{w} פעם")));
            }
            if stem_of(w, &["לעולם"], &['ו', 'ש']).is_some() && next == Some("לא") {
                out.push(absolute(&format!("{w} לא")));
            }
        }
        if stem_of(w, JUDGMENTAL, &['ו', 'ש', 'ה']).is_some() {
            out.push(judgmental(w));
        }
        let suffers = stem_of(w, &["סובל", "סובלת"], &['ו', 'ש', 'ה'])
            .is_some_and(|_| next.is_some_and(|n| n.starts_with('מ')));
        let afflicted = stem_of(w, &["לוקה"], &['ו', 'ש', 'ה'])
            .is_some_and(|_| next.is_some_and(|n| n.starts_with('ב')));
        if suffers || afflicted {
            let n = next.unwrap_or_default();
            out.push(judgmental(&format!("{w} {n}")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(text: &str, part: &str) -> bool {
        polish_notes(text).iter().any(|n| n.contains(part))
    }

    #[test]
    fn a_clean_paragraph_has_no_notes() {
        for clean in [
            "[ילד] הגיע לאבחון בשיתוף פעולה, והתמיד במשימות לאורך כל המפגש.",
            "במטריצות ([תוצאה 13 · ממוצע גבוה · חוזקה יחסית]) עבד לאט לאט ובדייקנות.",
            "לדברי ההורים (ע\"י שאלון), [ילד] נרדם בקושי. ד\"ר כהן המליצה על המשך מעקב.",
            "הציון 5.4 הושג בגיל 5:4, ב-12.3.2026: \"הוא אוהב פאזלים\".",
            "המשימות הוצגו אחת אחת, והתשובות נכתבו מילה מילה...",
            "א) המשך מעקב התפתחותי. ב) הדרכת הורים.",
            "הבדיקה כללה שלושה חלקים: שיחה, תצפית ומבחנים.",
            "לא תמיד נענה לפנייה, ובכל זאת שיתף פעולה במשימות.",
        ] {
            assert_eq!(polish_notes(clean), Vec::<String>::new(), "{clean}");
        }
    }

    #[test]
    fn tags_never_make_a_note() {
        // Two tags in a row, a tag at the end, a score tag with its own dots and words.
        assert!(polish_notes("שוחחתי עם [אם] [אב] על הקשיים בגן של [ילד].").is_empty());
        assert!(polish_notes("בקוביות התקבל [תוצאה 7 · ממוצע נמוך]").is_empty());
        assert!(polish_notes("[ילד] [ילד] הגיע עם [מטפלת] למפגש השני.").is_empty());
    }

    #[test]
    fn a_word_twice_in_a_row() {
        let n = polish_notes("הילד שיחק של של חברים בחצר הגן.");
        assert_eq!(n, vec!["מילה כפולה: \"של של\"".to_owned()]);
        assert!(has("הוא הגיע הגיע למפגש בזמן.", "\"הגיע הגיע\""));
        // Numbers, words said twice on purpose, and a comma between them are not a slip.
        assert!(!has("נבחנו 3 3 פעמים במהלך היום.", "מילה כפולה"));
        assert!(!has("התקדם צעד צעד לאורך המשימה.", "מילה כפולה"));
        assert!(!has("הוא מאוד, מאוד שמח לבוא.", "מילה כפולה"));
    }

    #[test]
    fn spaces_and_marks() {
        assert!(has(
            "הגיע  לאבחון בזמן ושיתף פעולה.",
            "רווח כפול אחרי \"הגיע\""
        ));
        assert!(has(
            "הגיע לאבחון , ושיתף פעולה.",
            "רווח לפני \",\" אחרי \"לאבחון\""
        ));
        assert!(has(
            "שיתף פעולה ( בעיקר בהתחלה) לאורך המפגש.",
            "רווח אחרי \"(\""
        ));
        assert!(has(
            "שיתף פעולה (בעיקר בהתחלה ) לאורך המפגש.",
            "רווח לפני \")\""
        ));
        assert!(has(
            "הגיע לאבחון,ושיתף פעולה לאורך המפגש.",
            "חסר רווח אחרי \",\" ב-\"לאבחון,ושיתף\""
        ));
        assert!(has(
            "הגיע לאבחון.שיתף פעולה לאורך המפגש.",
            "חסר רווח אחרי \".\""
        ));
        assert!(has(
            "הגיע לאבחון,, ושיתף פעולה במשימות.",
            "סימן פיסוק כפול \",,\""
        ));
        assert!(has(
            "הגיע לאבחון.. ושיתף פעולה במשימות.",
            "סימן פיסוק כפול \"..\""
        ));
        // An ellipsis, a decimal, an abbreviation with points and a space before one are fine.
        assert!(!has("הגיע לאבחון... ושיתף פעולה במשימות.", "פיסוק"));
        assert!(!has("ממוצע של 9.5 בתת-המבחנים, ת.ז. לא נמסרה.", "רווח"));
        assert!(!has("והמשימות הבאות ... לא הושלמו במלואן.", "רווח לפני"));
    }

    #[test]
    fn brackets_and_quotes_that_do_not_close() {
        assert!(has(
            "שיתף פעולה (בעיקר בהתחלה לאורך המפגש.",
            "נפתחו ולא נסגרו"
        ));
        assert!(has(
            "שיתף פעולה בעיקר בהתחלה) לאורך המפגש.",
            "נסגרו בלי שנפתחו"
        ));
        assert!(has("אמר \"אני לא רוצה לאורך כל המפגש.", "מירכאות"));
        // Gershayim inside abbreviations are not quotation marks.
        assert!(!has("נבדק ע\"י ד\"ר לוי בצה\"ל לאורך המפגש.", "מירכאות"));
        assert!(!has("אמר \"די\" ויצא, ע\"פ דברי ההורים.", "מירכאות"));
        assert!(!has(
            "בשלב ה\"משחק החופשי\" שיתף פעולה לפי דו\"חות קודמים.",
            "מירכאות"
        ));
        assert!(has(
            "בשלב ה\"משחק החופשי שיתף פעולה לאורך המפגש.",
            "מירכאות"
        ));
        // A list mark is not a bracket.
        assert!(!has("1) מעקב התפתחותי. 2) הדרכת הורים.", "סוגריים"));
    }

    #[test]
    fn a_long_sentence() {
        let long = vec!["מילה"; 46].join(" ") + ".";
        let n = polish_notes(&format!("בגן הוא {long}"));
        assert!(
            n.iter()
                .any(|x| x.contains("משפט ארוך (48 מילים)") && x.contains("\"בגן הוא מילה\"")),
            "{n:?}"
        );
        let fine = vec!["מילה"; 44].join(" ") + ".";
        assert!(!has(&fine, "משפט ארוך"));
        // A decimal point does not split a sentence (nor end it early).
        let two = format!(
            "{} 5.4 {}.",
            vec!["מילה"; 23].join(" "),
            vec!["מילה"; 23].join(" ")
        );
        assert!(has(&two, "משפט ארוך"));
    }

    #[test]
    fn a_paragraph_that_does_not_end() {
        assert!(has(
            "שיתף פעולה לאורך כל המפגש והתמיד",
            "הפסקה לא מסתיימת בסימן פיסוק (אחרי \"והתמיד\")"
        ));
        for ends in [
            "שיתף פעולה לאורך כל המפגש.",
            "ההמלצות הן כדלקמן:",
            "האם שיתף פעולה לאורך המפגש?",
            "שיתף פעולה לאורך כל המפגש (בעיקר בהתחלה)",
            "שיתף פעולה לאורך כל המפגש עם [מטפלת]",
            "לדבריו: \"אני אוהב לבנות מגדלים\"",
        ] {
            assert!(!has(ends, "לא מסתיימת"), "{ends}");
        }
        // A short line or a list is not a paragraph of prose.
        assert!(!has("המלצות לגן", "לא מסתיימת"));
        assert!(!has("1. מעקב התפתחותי\n2. הדרכת הורים", "לא מסתיימת"));
    }

    #[test]
    fn placeholders_left_in_the_text() {
        assert!(has(
            "[ילד] נבדק בתאריך [חסר: תאריך האבחון המלא] במכון.",
            "נשאר סימון להשלמה: \"[חסר: תאריך האבחון\""
        ));
        assert!(has("שם הגן: {שם הגן} בעיר.", "\"{שם הגן}\""));
        assert!(has("הציון בקוביות היה XXX בבדיקה.", "\"XXX\""));
        assert!(has("הציון בקוביות היה ??? בבדיקה.", "\"???\""));
        assert!(has("TODO להשלים את פרטי הגן.", "\"TODO\""));
        // A question mark or two, and a word with X in it, are not placeholders.
        assert!(!has("האם זה קשור?? כנראה שלא.", "סימון"));
        assert!(!has("הועבר מבחן WISC-V במלואו.", "סימון"));
    }

    #[test]
    fn absolute_and_judgmental_wording_is_only_suggested() {
        let n = polish_notes("הוא תמיד מסרב לשתף פעולה.");
        assert_eq!(
            n,
            vec!["ניסוח מוחלט: \"תמיד\" – כדאי לשקול אם זה מדויק".to_owned()]
        );
        assert!(has("הוא אף פעם לא מסיים משימה.", "\"אף פעם\""));
        assert!(has("ולעולם לא מבקש עזרה.", "\"ולעולם לא\""));
        assert!(has("הילד עצלן בשיעורים.", "ניסוח שיפוטי: \"עצלן\""));
        assert!(has("ילדה מפונקת ונוחה לכעוס.", "\"מפונקת\""));
        assert!(has("הוא סובל מקשיי קשב.", "\"סובל מקשיי\""));
        assert!(has("הילדה לוקה בשפה.", "\"לוקה בשפה\""));
        // Softened, inside other words, or other senses: nothing.
        for fine in [
            "לא תמיד נענה לפנייה.",
            "כמעט אף פעם לא ויתר.",
            "הוא התמיד במשימה עד הסוף.",
            "הוא מסרב לעיתים לעבור בין משימות.",
            "הוא סובל את הרעש בכיתה.",
            "האחוזונים חושבו לפי העקומה הנורמלית.",
        ] {
            assert!(
                polish_notes(fine).is_empty(),
                "{fine}: {:?}",
                polish_notes(fine)
            );
        }
    }

    #[test]
    fn notes_quote_at_most_three_words() {
        let text = format!(
            "[חסר: הרבה מאוד מילים כאן בפנים] ו-{{שם ארוך מאוד של משהו}} {} סוף",
            vec!["מילה"; 50].join(" ")
        );
        for n in polish_notes(&text) {
            let quoted: Vec<&str> = n.split('"').skip(1).step_by(2).collect();
            for q in quoted {
                assert!(q.split_whitespace().count() <= 3, "{n}");
            }
        }
    }
}
