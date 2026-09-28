//! Right-to-left lines from a PDF: visual (left-to-right on the page) → logical order.
//!
//! The PDF reader rebuilds each line from the characters' positions, so the input here is
//! the true visual order whatever the producer did.

#[must_use]
pub fn is_hebrew(c: char) -> bool {
    ('\u{0590}'..='\u{05FF}').contains(&c) || ('\u{FB1D}'..='\u{FB4F}').contains(&c)
}

/// A token that stays left-to-right inside Hebrew text: numbers, Latin, "5:4", "WISC-V".
fn is_ltr_token(t: &str) -> bool {
    !t.chars().any(is_hebrew) && t.chars().any(|c| c.is_ascii_alphanumeric())
}

/// Reverse a display-order line into logical order, keeping numbers and Latin readable.
#[must_use]
pub fn reverse_line(line: &str) -> String {
    let reversed: String = line.chars().rev().collect();
    // Split into words, keeping the separators (spaces and table tabs) as their own items.
    let mut items: Vec<String> = Vec::new();
    for c in reversed.chars() {
        let sep = c == ' ' || c == '\t';
        match items.last_mut() {
            Some(last) if !sep && !last.starts_with([' ', '\t']) => last.push(c),
            _ => items.push(c.to_string()),
        }
    }
    let is_sep = |s: &str| s == " " || s == "\t";
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < items.len() {
        if !is_ltr_token(&items[i]) {
            out.push_str(&items[i]);
            i += 1;
            continue;
        }
        // A group of LTR words joined by single spaces ("V CSIW" → "WISC V"); a tab (table
        // column) ends the group.
        let mut j = i;
        while j + 2 < items.len() && items[j + 1] == " " && is_ltr_token(&items[j + 2]) {
            j += 2;
        }
        // Only the span from the first to the last letter/digit: punctuation around it
        // ("5:4." / "(WISC") belongs to the right-to-left text and is already in place.
        let group: Vec<char> = items[i..=j].concat().chars().collect();
        let first = group
            .iter()
            .position(char::is_ascii_alphanumeric)
            .unwrap_or(0);
        let last = group
            .iter()
            .rposition(char::is_ascii_alphanumeric)
            .unwrap_or(0);
        out.extend(&group[..first]);
        out.extend(group[first..=last].iter().rev());
        out.extend(&group[last + 1..]);
        debug_assert!(items
            .get(j + 1)
            .is_none_or(|x| is_sep(x) || !is_ltr_token(x)));
        i = j + 1;
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
}
