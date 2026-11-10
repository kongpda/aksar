//! Every letter of the standard Khmer alphabet chart: the 33 consonants, each
//! with its own subscript (ក្ក ខ្ខ … ដ្ដ … ត្ត … អ្អ), the dependent vowels
//! (including ុំ ាំ and the ះ forms) on every consonant, and the independent
//! vowels. Each must be valid, unchanged by `normalize` and `repair`, and one
//! cluster.
use aksar::syllable::{clusters, normalize, repair, validate};

/// The 33 consonants in alphabet order (U+179D ឝ and U+179E ឞ are obsolete).
const CONSONANTS: &str = "កខគឃងចឆជឈញដឋឌឍណតថទធនបផពភមយរលវសហឡអ";
const VOWELS: [&str; 24] = [
    "ា", "ិ", "ី", "ឹ", "ឺ", "ុ", "ូ", "ួ", "ើ", "ឿ", "ៀ", "េ", "ែ", "ៃ", "ោ", "ៅ", "ុំ", "ំ", "ាំ", "ះ", "ិះ", "ុះ",
    "េះ", "ោះ",
];
const INDEPENDENT: [&str; 15] = ["អ", "អា", "ឥ", "ឦ", "ឧ", "ឩ", "ឪ", "ឫ", "ឬ", "ឭ", "ឮ", "ឯ", "ឰ", "ឱ", "ឳ"];

fn check(s: &str) {
    assert!(validate(s).is_ok(), "{s:?} not valid: {:?}", validate(s));
    assert_eq!(normalize(s), s, "normalize changed {s:?}");
    assert_eq!(repair(s), s, "repair changed {s:?}");
    assert_eq!(clusters(s), vec![s], "{s:?} is not one cluster");
}

#[test]
fn consonants_with_their_subscripts() {
    assert_eq!(CONSONANTS.chars().count(), 33);
    for c in CONSONANTS.chars() {
        check(&c.to_string());
        check(&format!("{c}\u{17D2}{c}"));
    }
}

#[test]
fn every_vowel_on_every_consonant() {
    for c in CONSONANTS.chars() {
        for v in VOWELS {
            check(&format!("{c}{v}"));
            // With a subscript between the consonant and the vowel too.
            check(&format!("{c}\u{17D2}{c}{v}"));
        }
    }
}

#[test]
fn independent_vowels() {
    for v in INDEPENDENT {
        check(v);
    }
}

#[test]
fn subscript_ta_and_da_stay_as_written() {
    // ជើងត and ជើងដ are different letters: nothing may swap them.
    for s in ["ដ្ដ", "ត្ត", "ក្ដ", "ក្ត"] {
        check(s);
    }
}
