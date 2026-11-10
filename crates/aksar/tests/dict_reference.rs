//! The fast corrector must agree with the exhaustive algorithm (PDFTape's:
//! every edit re-scored on the whole token) on random OCR-like tokens.
#![cfg(feature = "bundled-dict")]

use aksar::dict::{Check, Dictionary};
use aksar::syllable::normalize;
use std::time::Instant;

const INSERTABLE: &[char] =
    &['ា', 'ិ', 'ី', 'ឹ', 'ឺ', 'ុ', 'ូ', 'ួ', 'ើ', 'ឿ', 'ៀ', 'េ', 'ែ', 'ៃ', 'ោ', 'ៅ', 'ំ', 'ះ', '់', '៉', '៊'];
const GROUPS: &[&[char]] = &[
    &['ុ', 'ូ', 'ួ'],
    &['ិ', 'ី', 'ឹ', 'ឺ'],
    &['េ', 'ែ', 'ៃ'],
    &['ើ', 'ឿ', 'ៀ'],
    &['ោ', 'ៅ'],
    &['៉', '៊'],
    &['ដ', 'ត'],
    &['វ', 'រ'],
];
fn cons(c: char) -> bool {
    ('\u{1780}'..='\u{17A2}').contains(&c)
}

/// PDFTape's `check`, unchanged except it uses aksar's normalize and dictionary.
fn reference(d: &Dictionary, token: &str) -> Check {
    let token = normalize(token);
    let any_word = token.chars().any(|c| aksar::chars::is_khmer(c) && !matches!(c, '\u{17D4}'..='\u{17DC}' | '\u{17E0}'..='\u{17E9}' | '\u{17F0}'..='\u{17F9}' | '\u{19E0}'..='\u{19FF}'));
    if !any_word || d.unknown_clusters(&token) == 0 {
        return Check::Known;
    }
    let chars: Vec<char> = token.chars().collect();
    let mut found = std::collections::HashSet::new();
    let mut try_c = |v: Vec<char>| {
        let s = normalize(&v.into_iter().collect::<String>());
        if s != token && d.unknown_clusters(&s) == 0 {
            found.insert(s);
        }
    };
    for i in 0..chars.len() {
        if let Some(g) = GROUPS.iter().find(|g| g.contains(&chars[i])) {
            for &r in g.iter().filter(|&&r| r != chars[i]) {
                let mut c = chars.clone();
                c[i] = r;
                try_c(c);
            }
        }
        if INSERTABLE.contains(&chars[i]) {
            let mut c = chars.clone();
            c.remove(i);
            try_c(c);
        }
    }
    for i in 0..chars.len() {
        if !matches!(chars[i], 'េ' | 'ែ' | 'ៃ') {
            continue;
        }
        let mut end = i + 1;
        if end >= chars.len() || !cons(chars[end]) {
            continue;
        }
        end += 1;
        while end + 1 < chars.len() && chars[end] == '\u{17D2}' && cons(chars[end + 1]) {
            end += 2;
        }
        let mut moved: Vec<char> = chars[..i].to_vec();
        moved.extend_from_slice(&chars[i + 1..end]);
        if chars[i] == 'េ' && chars.get(end) == Some(&'ា') {
            for m in ['ោ', 'ៅ'] {
                let mut c = moved.clone();
                c.push(m);
                c.extend_from_slice(&chars[end + 1..]);
                try_c(c);
            }
        }
        moved.push(chars[i]);
        moved.extend_from_slice(&chars[end..]);
        try_c(moved);
    }
    for i in 1..=chars.len() {
        if !cons(chars[i - 1]) {
            continue;
        }
        for &v in INSERTABLE {
            let mut c = chars.clone();
            c.insert(i, v);
            try_c(c);
        }
    }
    let fewest = found.iter().map(|c| d.segment_cost(c).words).min();
    let mut w: Vec<String> = found.into_iter().filter(|c| Some(d.segment_cost(c).words) == fewest).collect();
    if w.len() == 1 { Check::Corrected(w.pop().unwrap()) } else { Check::Suspect }
}

/// Small deterministic PRNG (no dev-dependency needed).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize
    }
}

#[test]
fn fast_corrector_matches_exhaustive_reference() {
    let d = Dictionary::bundled();
    let words: Vec<&str> = include_str!("../data/khmer-words.txt").lines().collect();
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let (mut n, mut corrected, mut suspect) = (0, 0, 0);
    let (mut t_fast, mut t_ref) = (0.0, 0.0);
    let mut mismatches = vec![];
    // Fewer in debug builds (the exhaustive reference is slow unoptimized).
    let rounds = if cfg!(debug_assertions) { 200 } else { 1500 };
    for _ in 0..rounds {
        // 1-4 real words, then one OCR-like edit: drop, swap or insert a sign.
        let k = 1 + rng.next() % 4;
        let mut s: String = (0..k).map(|_| words[rng.next() % words.len()]).collect();
        let mut cs: Vec<char> = s.chars().collect();
        let p = rng.next() % cs.len();
        match rng.next() % 3 {
            0 if INSERTABLE.contains(&cs[p]) => {
                cs.remove(p);
            }
            1 => {
                if let Some(g) = GROUPS.iter().find(|g| g.contains(&cs[p])) {
                    cs[p] = g[rng.next() % g.len()];
                }
            }
            _ => cs.insert(p, INSERTABLE[rng.next() % INSERTABLE.len()]),
        }
        s = cs.into_iter().collect();
        let t = Instant::now();
        let fast = d.check(&s);
        t_fast += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let slow = reference(d, &s);
        t_ref += t.elapsed().as_secs_f64();
        n += 1;
        match &fast {
            Check::Corrected(_) => corrected += 1,
            Check::Suspect => suspect += 1,
            _ => {}
        }
        if fast != slow {
            mismatches.push(format!("{s:?}: fast {fast:?} ref {slow:?}"));
        }
    }
    println!(
        "{n} tokens: {corrected} corrected, {suspect} suspect; fast {:.1} ms total, reference {:.1} ms ({:.1}x)",
        t_fast * 1e3,
        t_ref * 1e3,
        t_ref / t_fast
    );
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches[..mismatches.len().min(10)].join("\n")
    );
}

#[test]
fn long_tokens_stay_fast() {
    if cfg!(debug_assertions) {
        return; // timing only means something in release
    }
    let d = Dictionary::bundled();
    let base = "ទីក្រុងភ្នំពេញគឺជារាជធានីនៃប្រទេស";
    for reps in [1, 2, 4, 8] {
        let tok = format!("{}កម្ពជា", base.repeat(reps));
        let t = Instant::now();
        let r = d.check(&tok);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let r2 = reference(d, &tok);
        let ms_ref = t.elapsed().as_secs_f64() * 1e3;
        println!("{:>4} chars: fast {ms:>6.2} ms, exhaustive {ms_ref:>8.1} ms", tok.chars().count());
        assert_eq!(r, r2);
        assert!(matches!(r, Check::Corrected(_)));
    }
}

// `WithUserWords` is usable with only `Dictionary` imported (no `Words` trait).
#[test]
fn with_user_words_has_inherent_methods() {
    use aksar::dict::UserWords;
    let d = Dictionary::bundled();
    let mut user = UserWords::new();
    let name = user.insert(d, "សុខស្ដារ៉ា").unwrap();
    let lex = d.with_user_words(&user);
    assert!(lex.contains(&name));
    assert_eq!(lex.check(&name), Check::Known);
    assert_eq!(lex.segment(&name), [name.as_str()]);
    assert_eq!(lex.unknown_clusters(&name), 0);
    assert_eq!(lex.segment_cost(&name).unknown_clusters, 0);
}
