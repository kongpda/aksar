//! Builds `crates/aksar/data/khmer-words.txt` from the ICU Khmer word list:
//! normalize every entry, drop entries that still render broken, sort, dedupe.
//!
//! cargo run -p aksar --example clean_wordlist -- khmerdict.txt > crates/aksar/data/khmer-words.txt
fn main() {
    let path = std::env::args().nth(1).expect("usage: clean_wordlist <icu khmerdict.txt>");
    let text = std::fs::read_to_string(path).unwrap();
    let mut words: Vec<String> = text
        .lines()
        // The ICU file ends with a BOM-prefixed `# Copyright` line: not a word.
        .map(|l| l.trim().trim_start_matches('\u{feff}').trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && l.chars().any(aksar::chars::is_khmer))
        .map(aksar::syllable::normalize)
        .filter(|w| aksar::syllable::validate(w).is_ok())
        .collect();
    let before = text.lines().filter(|l| !l.trim().is_empty()).count();
    words.sort();
    words.dedup();
    eprintln!("{before} entries -> {} words", words.len());
    for w in words {
        println!("{w}");
    }
}
