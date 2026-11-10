//! Prints how many entries of a word list change under `normalize` or fail `validate`.
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let text = std::fs::read_to_string(path).unwrap();
    let (mut n, mut changed, mut invalid, mut warn) = (0, 0, 0, 0);
    for w in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        n += 1;
        let norm = aksar::syllable::normalize(w);
        if norm != w {
            changed += 1;
            if changed <= 5 {
                println!("changed: {w} -> {norm}");
            }
        }
        if aksar::syllable::validate(w).is_err() {
            invalid += 1;
            if invalid <= 5 {
                println!("broken: {w}");
            }
        }
        if !aksar::syllable::issues(w).is_empty() {
            warn += 1;
        }
    }
    println!("{n} words, {changed} change under normalize, {invalid} broken, {warn} with any issue");
}
