//! Right-to-left lines from a PDF: visual (left-to-right on the page) → logical order.
//!
//! The PDF reader rebuilds each line from the characters' positions, so the input here is
//! the true visual order whatever the producer did.

#[must_use]
pub fn is_hebrew(c: char) -> bool {
    ('\u{0590}'..='\u{05FF}').contains(&c) || ('\u{FB1D}'..='\u{FB4F}').contains(&c)
}

/// A character that may join two groups of digits into one number: "5:4", "12.3.2019",
/// "1,000", "10:30-11:00". Only one of them between digits, as on the page.
fn joins_digits(c: char) -> bool {
    matches!(c, '.' | ',' | ':' | '/' | '-' | '+')
}

/// Put back the left-to-right runs of a stretch that has no Hebrew letter and no tab. The
/// stretch arrives with its characters reversed.
///
/// With a Latin letter in it, the stretch is one run, from its first to its last letter or
/// digit ("V CSIW" → "WISC V", "4 CSIW" → "WISC 4"): a Latin word pulls the numbers and the
/// punctuation next to it into its own direction. Without one, every number is a run of its
/// own, and the numbers follow each other right to left like the words around them
/// ("001 29 58" → "100 92 85"). Punctuation around a run ("5:4." / "(WISC" / "ב-12") belongs
/// to the right-to-left text and is already in place.
fn restore_ltr(stretch: &[char], out: &mut String) {
    if stretch.iter().any(char::is_ascii_alphabetic) {
        let first = stretch
            .iter()
            .position(char::is_ascii_alphanumeric)
            .unwrap_or(0);
        let last = stretch
            .iter()
            .rposition(char::is_ascii_alphanumeric)
            .unwrap_or(0);
        out.extend(stretch.iter().take(first));
        out.extend(stretch.iter().take(last + 1).skip(first).rev());
        out.extend(stretch.iter().skip(last + 1));
        return;
    }
    let mut rest = stretch;
    while let Some((&c, after)) = rest.split_first() {
        if !c.is_ascii_digit() {
            out.push(c);
            rest = after;
            continue;
        }
        // The number: digits, and single joiners that have a digit on both sides.
        let mut len = 1;
        loop {
            match (rest.get(len), rest.get(len + 1)) {
                (Some(d), _) if d.is_ascii_digit() => len += 1,
                (Some(j), Some(d)) if joins_digits(*j) && d.is_ascii_digit() => len += 2,
                _ => break,
            }
        }
        out.extend(rest.iter().take(len).rev());
        rest = rest.get(len..).unwrap_or_default();
    }
}

/// Reverse a display-order line into logical order, keeping numbers and Latin readable.
#[must_use]
pub fn reverse_line(line: &str) -> String {
    let reversed: Vec<char> = line.chars().rev().collect();
    let mut out = String::with_capacity(line.len());
    // Hebrew letters and table tabs are in place after the reversal; what lies between them
    // (also inside a word: "ב-12", "ה-WISC") may hold left-to-right runs.
    for part in reversed.split_inclusive(|c| is_hebrew(*c) || *c == '\t') {
        match part.split_last() {
            Some((&end, stretch)) if is_hebrew(end) || end == '\t' => {
                restore_ltr(stretch, &mut out);
                out.push(end);
            }
            _ => restore_ltr(part, &mut out),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_line_becomes_logical() {
        // As the characters sit on the page, left to right.
        assert_eq!(reverse_line("הנפוה .5:4 ליג"), "גיל 5:4. הופנה");
        assert_eq!(
            reverse_line(".)עצוממ חווט( 102 ןויצ"),
            "ציון 102 (טווח ממוצע)."
        );
        assert_eq!(reverse_line("לבקתה WISC V ןחבמב"), "במבחן WISC V התקבל");
        assert_eq!(reverse_line("85\tהנבה"), "הבנה\t85");
        assert_eq!(reverse_line("92\t85\tעוצקמ"), "מקצוע\t85\t92");
    }

    #[test]
    fn round_trip() {
        let logical = "בגיל 5:4 נבדק במבחן WISC-V (גרסה עברית) והתקבל ציון 102.";
        assert_eq!(reverse_line(&reverse_line(logical)), logical);
    }

    /// A number or a Latin word glued to a Hebrew prefix ("ב-12", "ה-WISC") stays readable.
    #[test]
    fn ltr_run_glued_to_a_hebrew_prefix() {
        assert_eq!(reverse_line("םישדוח 12-ב"), "ב-12 חודשים");
        assert_eq!(reverse_line("12.3.2019-ב דלונ"), "נולד ב-12.3.2019");
        assert_eq!(reverse_line("לבקתה WISC-ה ןחבמב"), "במבחן ה-WISC התקבל");
        assert_eq!(reverse_line("85:ןויצ"), "ציון:85");
        assert_eq!(reverse_line("תוקד 15-כ"), "כ-15 דקות");
    }

    /// Numbers separated by spaces are separate runs, laid out right to left on the page.
    #[test]
    fn numbers_in_a_row_keep_their_order() {
        assert_eq!(reverse_line("100 92 85 םינויצ"), "ציונים 85 92 100");
        assert_eq!(reverse_line("10:30-11:00 העשב"), "בשעה 10:30-11:00");
        // A Latin name pulls the numbers after it into its own left-to-right run.
        assert_eq!(reverse_line("לבקתה WISC 4 ןחבמב"), "במבחן WISC 4 התקבל");
    }
}
