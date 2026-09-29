//! Israeli ID number (ת"ז) check digit (Luhn variant with weights 1,2,1,2,…).

/// True when `digits` is exactly nine ASCII digits with a valid check digit.
pub fn is_valid(digits: &str) -> bool {
    if digits.len() != 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let sum: u32 = digits
        .bytes()
        .enumerate()
        .map(|(i, b)| {
            let d = u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 2 };
            if d > 9 {
                d - 9
            } else {
                d
            }
        })
        .sum();
    sum % 10 == 0
}

#[cfg(test)]
mod tests {
    use super::is_valid;

    #[test]
    fn accepts_valid_and_rejects_invalid_check_digits() {
        assert!(is_valid("000000018"));
        assert!(is_valid("000000026"));
        assert!(!is_valid("000000019"));
        assert!(!is_valid("12345678"));
        assert!(!is_valid("12345678a"));
    }
}
