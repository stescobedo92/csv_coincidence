use crate::error::Result;
use std::path::Path;

pub const EMAIL_PATTERN: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}";
pub const URL_PATTERN: &str = r"https?://[^\s,;]+";
pub const IPV4_PATTERN: &str = r"\b(?:\d{1,3}\.){3}\d{1,3}\b";
pub const IBAN_PATTERN: &str = r"\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,3})?\b";
pub const CREDIT_CARD_PATTERN: &str = r"\b\d{4}[- ]?\d{4}[- ]?\d{4}[- ]?\d{4}\b";
pub const DNI_PATTERN: &str = r"\b\d{8}[A-HJ-NP-TV-Z]\b";
pub const NIE_PATTERN: &str = r"\b[XYZ]\d{7}[A-Z]\b";
pub const PHONE_ES_PATTERN: &str = r"\b(?:\+34\s?)?[6789]\d{8}\b";
pub const DATE_ISO_PATTERN: &str = r"\b\d{4}-\d{2}-\d{2}\b";
pub const UUID_PATTERN: &str =
    r"\b[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\b";

const BUILTINS: &[(&str, &str)] = &[
    ("email", EMAIL_PATTERN),
    ("url", URL_PATTERN),
    ("ipv4", IPV4_PATTERN),
    ("iban", IBAN_PATTERN),
    ("credit-card", CREDIT_CARD_PATTERN),
    ("dni", DNI_PATTERN),
    ("nie", NIE_PATTERN),
    ("phone-es", PHONE_ES_PATTERN),
    ("date-iso", DATE_ISO_PATTERN),
    ("uuid", UUID_PATTERN),
];

/// All built-in `(name, pattern)` pairs.
pub fn builtin_patterns() -> &'static [(&'static str, &'static str)] {
    BUILTINS
}

/// Look up a built-in pattern by name (case-insensitive, `_` equals `-`).
pub fn builtin_pattern(name: &str) -> Option<&'static str> {
    let key = name.trim().to_ascii_lowercase().replace('_', "-");
    BUILTINS
        .iter()
        .find(|(n, _)| *n == key)
        .map(|(_, pattern)| *pattern)
}

/// Parse pattern-file text: one pattern per line, blank lines and `#`
/// comments are ignored.
pub fn parse_patterns(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Read patterns from a file (one per line, `#` comments allowed).
pub fn read_pattern_file(path: impl AsRef<Path>) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path.as_ref())?;
    Ok(parse_patterns(&text))
}

/// Validate a credit-card number with the Luhn checksum.
///
/// Non-digit separators (spaces, `-`) are ignored. Requires 13–19 digits.
pub fn is_valid_credit_card(value: &str) -> bool {
    let digits: Vec<u32> = value.chars().filter_map(|c| c.to_digit(10)).collect();
    if value
        .chars()
        .any(|c| !c.is_ascii_digit() && c != ' ' && c != '-')
        || !(13..=19).contains(&digits.len())
    {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Validate an IBAN with the ISO 13616 modulo-97 check.
pub fn is_valid_iban(value: &str) -> bool {
    let iban: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    if iban.len() < 15
        || iban.len() > 34
        || !iban.chars().all(|c| c.is_ascii_alphanumeric())
        || !iban[..2].chars().all(|c| c.is_ascii_alphabetic())
        || !iban[2..4].chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    // Move the first 4 chars to the end and expand letters to numbers.
    let rearranged = format!("{}{}", &iban[4..], &iban[..4]);
    let mut remainder: u32 = 0;
    for c in rearranged.chars() {
        let digits = if c.is_ascii_digit() {
            (c as u32 - '0' as u32).to_string()
        } else {
            (c as u32 - 'A' as u32 + 10).to_string()
        };
        for d in digits.chars() {
            remainder = (remainder * 10 + (d as u32 - '0' as u32)) % 97;
        }
    }
    remainder == 1
}

const DNI_LETTERS: &str = "TRWAGMYFPDXBNJZSQVHLCKE";

fn dni_control_letter(number: u32) -> Option<char> {
    DNI_LETTERS.chars().nth((number % 23) as usize)
}

/// Validate a Spanish DNI (8 digits + control letter).
pub fn is_valid_dni(value: &str) -> bool {
    let chars: Vec<char> = value.trim().to_ascii_uppercase().chars().collect();
    if chars.len() != 9 || !chars[..8].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let digits: String = chars[..8].iter().collect();
    let Ok(number) = digits.parse::<u32>() else {
        return false;
    };
    dni_control_letter(number) == Some(chars[8])
}

/// Validate a Spanish NIE (`X`/`Y`/`Z` + 7 digits + control letter).
pub fn is_valid_nie(value: &str) -> bool {
    let chars: Vec<char> = value.trim().to_ascii_uppercase().chars().collect();
    if chars.len() != 9 || !chars[1..8].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let prefix = match chars[0] {
        'X' => 0,
        'Y' => 1,
        'Z' => 2,
        _ => return false,
    };
    let digits: String = chars[1..8].iter().collect();
    let Ok(rest) = digits.parse::<u32>() else {
        return false;
    };
    dni_control_letter(prefix * 10_000_000 + rest) == Some(chars[8])
}
