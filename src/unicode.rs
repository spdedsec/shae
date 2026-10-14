use unicode_segmentation::UnicodeSegmentation;

/// Return all extended grapheme clusters in a UTF-8 string according to UAX #29.
pub fn graphemes(s: &str) -> Vec<&str> {
    s.graphemes(true).collect()
}

/// Return the count of user-perceived grapheme clusters in a UTF-8 string.
pub fn grapheme_len(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Safely slice a UTF-8 string by grapheme cluster indices.
///
/// Prevents splitting multi-byte UTF-8 codepoints, combining accents,
/// skin-tone modifiers, flags, and zero-width joiner (ZWJ) emoji sequences.
pub fn slice_graphemes(s: &str, start: i64, end: Option<i64>) -> String {
    let clusters: Vec<&str> = s.graphemes(true).collect();
    let total = clusters.len() as i64;
    if total == 0 {
        return String::new();
    }

    let actual_start = if start < 0 {
        (total + start).clamp(0, total) as usize
    } else {
        start.clamp(0, total) as usize
    };

    let actual_end = match end {
        Some(e) => {
            if e < 0 {
                (total + e).clamp(0, total) as usize
            } else {
                e.clamp(0, total) as usize
            }
        }
        None => total as usize,
    };

    if actual_start >= actual_end {
        return String::new();
    }

    clusters[actual_start..actual_end].concat()
}

/// Return an array of Unicode codepoint characters as individual strings.
pub fn runes(s: &str) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}

/// Return the numeric Unicode codepoint values (as i64).
pub fn codepoints(s: &str) -> Vec<i64> {
    s.chars().map(|c| c as u32 as i64).collect()
}

/// Reconstruct a UTF-8 string from a list of integer codepoints.
pub fn from_codepoints(cps: &[i64]) -> Result<String, String> {
    let mut out = String::with_capacity(cps.len());
    for &cp in cps {
        if cp < 0 || cp > 0x10FFFF {
            return Err(format!(
                "Unicode codepoint {:#x} out of valid range (0..0x10FFFF)",
                cp
            ));
        }
        match char::from_u32(cp as u32) {
            Some(c) => out.push(c),
            None => {
                return Err(format!(
                    "Invalid Unicode scalar value {:#x} (surrogate or reserved)",
                    cp
                ));
            }
        }
    }
    Ok(out)
}

/// Check if all characters in the string are ASCII.
pub fn is_ascii(s: &str) -> bool {
    s.is_ascii()
}

/// Return the general Unicode category of a character or first character of a string.
pub fn unicode_category(c: char) -> &'static str {
    if c.is_alphabetic() {
        "Letter"
    } else if c.is_numeric() {
        "Number"
    } else if c.is_whitespace() {
        "Separator"
    } else if c.is_ascii_punctuation()
        || matches!(c, '\u{2000}'..='\u{206F}' | '\u{3000}'..='\u{303F}')
    {
        "Punctuation"
    } else if matches!(c, '\u{0000}'..='\u{001F}' | '\u{007F}'..='\u{009F}') {
        "Control"
    } else if matches!(c, '\u{0300}'..='\u{036F}' | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}' | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
    {
        "Mark"
    } else {
        "Symbol"
    }
}
