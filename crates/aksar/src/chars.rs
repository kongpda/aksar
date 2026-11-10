//! Khmer character classes.
//!
//! Covers the Khmer block (U+1780–U+17FF) and Khmer Symbols (U+19E0–U+19FF).
//! Classes follow the Unicode Standard (ch. 16.4) and the categories used by
//! the OpenType Khmer shaper.

/// U+17D2 KHMER SIGN COENG: the invisible stacker that makes the next
/// consonant a subscript.
pub const COENG: char = '\u{17D2}';
/// U+17CC KHMER SIGN ROBAT.
pub const ROBAT: char = '\u{17CC}';
/// U+200C ZERO WIDTH NON-JOINER.
pub const ZWNJ: char = '\u{200C}';
/// U+200D ZERO WIDTH JOINER.
pub const ZWJ: char = '\u{200D}';
/// U+200B ZERO WIDTH SPACE, the usual Khmer word separator.
pub const ZWSP: char = '\u{200B}';

/// The role a character plays inside a Khmer syllable.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// U+1780–U+17A2: consonants (can be a base or follow a coeng).
    Consonant,
    /// U+17A5–U+17B3: independent vowels (act as a base).
    IndependentVowel,
    /// U+17A3, U+17A4, U+17B4, U+17B5, U+17D3, U+17D8: deprecated / discouraged.
    Deprecated,
    /// U+17D2 coeng.
    Coeng,
    /// U+17C9 muusikatoan, U+17CA triisap.
    RegisterShifter,
    /// U+17CC robat.
    Robat,
    /// U+17B6–U+17C5: dependent vowel signs.
    DependentVowel,
    /// U+17C6–U+17C8, U+17CB, U+17CD–U+17D1, U+17DD: other combining signs.
    Sign,
    /// U+17E0–U+17E9 Khmer digits.
    Digit,
    /// U+17F0–U+17F9 lek attak (divination numerals).
    LekAttak,
    /// U+17D4–U+17DA, U+17DC: punctuation and the riel sign U+17DB.
    Punctuation,
    /// U+19E0–U+19FF lunar date symbols.
    LunarSymbol,
    /// ZWJ / ZWNJ (ligation control inside a cluster).
    Joiner,
    /// Anything outside Khmer.
    Other,
}

/// Classifies one character.
pub const fn class(c: char) -> Class {
    match c as u32 {
        0x1780..=0x17A2 => Class::Consonant,
        0x17A3 | 0x17A4 | 0x17B4 | 0x17B5 | 0x17D3 | 0x17D8 => Class::Deprecated,
        0x17A5..=0x17B3 => Class::IndependentVowel,
        0x17B6..=0x17C5 => Class::DependentVowel,
        0x17C9 | 0x17CA => Class::RegisterShifter,
        0x17CC => Class::Robat,
        0x17D2 => Class::Coeng,
        0x17C6..=0x17C8 | 0x17CB | 0x17CD..=0x17D1 | 0x17DD => Class::Sign,
        0x17D4..=0x17D7 | 0x17D9..=0x17DC => Class::Punctuation,
        0x17E0..=0x17E9 => Class::Digit,
        0x17F0..=0x17F9 => Class::LekAttak,
        0x19E0..=0x19FF => Class::LunarSymbol,
        0x200C | 0x200D => Class::Joiner,
        _ => Class::Other,
    }
}

/// True for any assigned code point in the Khmer or Khmer Symbols blocks.
pub const fn is_khmer(c: char) -> bool {
    matches!(c as u32, 0x1780..=0x17DD | 0x17E0..=0x17E9 | 0x17F0..=0x17F9 | 0x19E0..=0x19FF)
}

/// True for characters that can start a syllable (consonant or independent vowel),
/// including the deprecated independent vowels U+17A3 and U+17A4, which shapers
/// and [`crate::syllable::validate`] accept as bases.
pub const fn is_base(c: char) -> bool {
    matches!(class(c), Class::Consonant | Class::IndependentVowel) || matches!(c as u32, 0x17A3 | 0x17A4)
}

/// True for combining marks that need a base (coeng, shifters, robat, vowels, signs).
pub const fn is_mark(c: char) -> bool {
    matches!(
        class(c),
        Class::Coeng | Class::RegisterShifter | Class::Robat | Class::DependentVowel | Class::Sign
    ) || matches!(c as u32, 0x17D3) // deprecated BATHAMASAT is still a mark
}

/// Converts a Khmer digit (០–៩) to its value.
pub const fn digit_value(c: char) -> Option<u32> {
    match c as u32 {
        d @ 0x17E0..=0x17E9 => Some(d - 0x17E0),
        _ => None,
    }
}

/// Converts an ASCII digit string to Khmer digits; other characters pass through.
pub fn to_khmer_digits(s: &str) -> alloc::string::String {
    s.chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32(0x17E0 + (c as u32 - '0' as u32)).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_base` must agree with the validator: a character it rejects as a
    /// base must not carry marks in valid text.
    #[test]
    fn deprecated_independent_vowels_are_bases() {
        for c in ['ឣ', 'ឤ'] {
            assert!(is_base(c), "{c}");
            assert!(crate::syllable::validate(&alloc::format!("{c}ា")).is_ok(), "{c}");
        }
        assert!(!is_base('\u{17B4}') && !is_base('ា') && !is_base('a'));
    }

    #[test]
    fn classes() {
        assert_eq!(class('ក'), Class::Consonant);
        assert_eq!(class('អ'), Class::Consonant);
        assert_eq!(class('ឥ'), Class::IndependentVowel);
        assert_eq!(class('ា'), Class::DependentVowel);
        assert_eq!(class('ៅ'), Class::DependentVowel);
        assert_eq!(class('្'), Class::Coeng);
        assert_eq!(class('៉'), Class::RegisterShifter);
        assert_eq!(class('ំ'), Class::Sign);
        assert_eq!(class('៌'), Class::Robat);
        assert_eq!(class('៥'), Class::Digit);
        assert_eq!(class('៛'), Class::Punctuation);
        assert_eq!(class('\u{19E0}'), Class::LunarSymbol);
        assert_eq!(class('a'), Class::Other);
    }

    #[test]
    fn digits() {
        assert_eq!(digit_value('០'), Some(0));
        assert_eq!(digit_value('៩'), Some(9));
        assert_eq!(digit_value('9'), None);
        assert_eq!(to_khmer_digits("2026-10"), "២០២៦-១០");
    }
}
