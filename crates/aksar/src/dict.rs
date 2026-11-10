//! Khmer word dictionary: lookup, segmentation cost and a conservative
//! single-edit corrector for OCR output.
//!
//! Ported from PDFTape's `pdftape-ocr::dictionary` (same rules, same tests) with
//! two changes:
//!
//! - **Linear time.** PDFTape re-segments the whole token for every candidate
//!   edit, so checking grows with the square of the token length (measured 6 ms
//!   for 39 characters, 357 ms for 270). Here each surviving candidate only re-scores a
//!   window of clusters around the edit, using forward and backward costs
//!   computed once, after an exact lower-bound filter skips edits that cannot
//!   help (270 characters: 2.4 ms).
//! - **Fast startup.** The word list is stored pre-normalized, so loading it is
//!   one pass that indexes lines into a compact hash table (no per-word
//!   normalization or allocation; PDFTape's build took 109 ms).
//!
//! The rules, unchanged: a token is split into dictionary words with the fewest
//! unknown Khmer clusters. If clusters stay unknown, the token is **corrected**
//! only when exactly one edit from a small confusion set (vowels and signs
//! Tesseract drops or swaps, subscript ដ/ត, pre-base vowels emitted in visual
//! order) makes it fully known; otherwise it is **flagged**. Proper names and
//! rare words are often missing from the list, so they are never rewritten blindly.

use crate::syllable::normalize;
use alloc::string::String;
use alloc::vec::Vec;

/// Longest dictionary entry tried, in clusters. Longer words can be stored but
/// never count as known, so [`UserWords::accept`] refuses them.
pub const MAX_WORD_CLUSTERS: usize = 14;

/// Clusters longer than this (in chars) get no single-cluster edits: no word
/// has one, and building edits for them costs the square of their length.
const MAX_EDIT_CLUSTER_CHARS: usize = 32;

/// Dependent vowels and signs that may be missing, extra or swapped.
const INSERTABLE: &[char] =
    &['ា', 'ិ', 'ី', 'ឹ', 'ឺ', 'ុ', 'ូ', 'ួ', 'ើ', 'ឿ', 'ៀ', 'េ', 'ែ', 'ៃ', 'ោ', 'ៅ', 'ំ', 'ះ', '់', '៉', '៊'];
/// Visually confusable groups (substitution only within a group).
const GROUPS: &[&[char]] = &[
    &['ុ', 'ូ', 'ួ'],
    &['ិ', 'ី', 'ឹ', 'ឺ'],
    &['េ', 'ែ', 'ៃ'],
    &['ើ', 'ឿ', 'ៀ'],
    &['ោ', 'ៅ'],
    &['៉', '៊'],
    &['ដ', 'ត'],
    // Measured on PDFTape's benchmark: វ read as រ is the top consonant error (accuracy plan M2).
    &['វ', 'រ'],
];

/// Segmentation cost. Costs compare field by field in declaration order, so
/// fewer unknown clusters always wins, then fewer words.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct Cost {
    /// Khmer clusters that are in no dictionary word.
    pub unknown_clusters: usize,
    /// Segments used (words plus unknown clusters).
    pub words: usize,
}

const INF: Cost = Cost { unknown_clusters: usize::MAX / 2, words: usize::MAX / 2 };

fn cost(unknown_clusters: usize, words: usize) -> Cost {
    Cost { unknown_clusters, words }
}

fn add(a: Cost, b: Cost) -> Cost {
    cost(a.unknown_clusters + b.unknown_clusters, a.words + b.words)
}

fn is_consonant(c: char) -> bool {
    ('\u{1780}'..='\u{17A2}').contains(&c)
}

/// A Khmer letter, vowel or sign: what words are made of. Digits,
/// punctuation and lunar-date symbols are Khmer too, but never words, so they
/// cost nothing (otherwise every date and sentence end would be flagged).
fn is_word_char(c: char) -> bool {
    crate::chars::is_khmer(c)
        && !matches!(c, '\u{17D4}'..='\u{17DC}' | '\u{17E0}'..='\u{17E9}' | '\u{17F0}'..='\u{17F9}' | '\u{19E0}'..='\u{19FF}')
}

pub use crate::syllable::clusters;

/// Outcome of [`Words::check`]. More outcomes may be added; match with a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Check {
    /// Every Khmer cluster is covered by dictionary words (or there is no Khmer).
    Known,
    /// Exactly one confusion-set edit makes the token fully known.
    Corrected(String),
    /// Unknown clusters remain and no unique fix exists. Show it for review.
    Suspect,
}

/// Error from [`Dictionary::from_prepared`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotPrepared {
    /// 1-based line number of the first entry out of order or repeated.
    pub line: usize,
}

impl core::fmt::Display for NotPrepared {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "word list line {} is out of order or repeated", self.line)
    }
}

impl core::error::Error for NotPrepared {}

/// A sorted Khmer word list.
pub struct Dictionary<'a> {
    text: alloc::borrow::Cow<'a, str>,
    /// Byte ranges of each word in `text`, sorted by word.
    words: Vec<(u32, u32)>,
    /// Open-addressing hash table of indices into `words` (u32::MAX = empty).
    table: Vec<u32>,
    /// Longest word in bytes: multi-cluster lookups stop past it.
    max_len: usize,
    /// Words spelled with ជើងដ (្ដ), folded to ជើងត (្ត), for lookups up to that
    /// choice (which OCR can't make from the picture).
    folded_da: alloc::collections::BTreeSet<String>,
}

const COENG_TA: &str = "\u{17D2}\u{178F}";
const COENG_DA: &str = "\u{17D2}\u{178A}";

/// `word` with every ជើងដ (្ដ, subscript DA) written as ជើងត (្ត, subscript TA).
/// They are different letters and the spelling decides the word, but most fonts
/// draw them the same, so OCR can't tell them apart. A matching key (search,
/// lookups up to that choice), never display text.
pub fn fold_subscript_da(word: &str) -> alloc::borrow::Cow<'_, str> {
    if word.contains(COENG_DA) {
        alloc::borrow::Cow::Owned(word.replace(COENG_DA, COENG_TA))
    } else {
        alloc::borrow::Cow::Borrowed(word)
    }
}

const EMPTY: u32 = u32::MAX;

/// Position of the first subscript TA or DA in UTF-8 `word` (one byte scan; most
/// words have neither, so lookups stay as fast as an exact match).
#[inline]
fn subscript_ta_da(word: &[u8]) -> Option<usize> {
    // ្ = E1 9F 92, ត = E1 9E 8F, ដ = E1 9E 8A.
    let mut i = 2;
    while i + 3 < word.len() {
        if word[i] == 0x92
            && word[i - 1] == 0x9F
            && word[i - 2] == 0xE1
            && word[i + 1] == 0xE1
            && word[i + 2] == 0x9E
            && matches!(word[i + 3], 0x8F | 0x8A)
        {
            return Some(i - 2);
        }
        i += 1;
    }
    None
}

/// Small fast hash for word bytes (FxHash-style, 8 bytes at a time).
fn hash(bytes: &[u8]) -> u64 {
    const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
    let mut h: u64 = bytes.len() as u64;
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        let v = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        h = (h.rotate_left(5) ^ v).wrapping_mul(K);
    }
    for &b in chunks.remainder() {
        h = (h.rotate_left(5) ^ u64::from(b)).wrapping_mul(K);
    }
    h ^ (h >> 29)
}

impl Dictionary<'static> {
    /// Builds a dictionary from any word list (one word per line): entries are
    /// normalized, broken entries dropped, then sorted and deduplicated.
    pub fn new(list: &str) -> Self {
        // A UTF-8 BOM (files saved by Windows tools) is not whitespace, so
        // `trim` would leave it glued to the first word.
        let list = list.strip_prefix('\u{FEFF}').unwrap_or(list);
        let mut words: Vec<String> = list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(normalize)
            .filter(|w| crate::syllable::validate(w).is_ok())
            .collect();
        words.sort();
        words.dedup();
        let mut text = String::with_capacity(words.iter().map(|w| w.len() + 1).sum());
        for w in &words {
            text.push_str(w);
            text.push('\n');
        }
        Dictionary::index(alloc::borrow::Cow::Owned(text))
    }

    /// The bundled ICU Khmer word list (80,630 words, Unicode License v3; see
    /// `data/khmer-words.LICENSE.txt`). Adds about 2 MB (370 KB gzip).
    #[cfg(feature = "bundled-dict")]
    pub fn bundled() -> &'static Dictionary<'static> {
        static DICT: std::sync::OnceLock<Dictionary<'static>> = std::sync::OnceLock::new();
        DICT.get_or_init(|| Dictionary::index(include_str!("../data/khmer-words.txt").into()))
    }
}

impl<'a> Dictionary<'a> {
    /// Uses a list that is already normalized, sorted and deduplicated (one word
    /// per line), without copying it. Fails on the first line that is out of
    /// order or repeated. Normalization is not checked: entries that are not in
    /// [`normalize`] form are kept but never match a lookup. A leading UTF-8 BOM
    /// and a `\r` before each `\n` (CRLF lists) are ignored.
    ///
    /// # Panics
    ///
    /// If `list` is 4 GiB or larger (word offsets are 32-bit).
    pub fn from_prepared(list: &'a str) -> Result<Self, NotPrepared> {
        let d = Dictionary::index(alloc::borrow::Cow::Borrowed(list));
        for (i, pair) in d.words.windows(2).enumerate() {
            if d.word(pair[0]) >= d.word(pair[1]) {
                return Err(NotPrepared { line: i + 2 });
            }
        }
        Ok(d)
    }

    fn index(text: alloc::borrow::Cow<'a, str>) -> Self {
        assert!(
            u32::try_from(text.len()).is_ok(),
            "word list is {} bytes; the dictionary supports lists under 4 GiB",
            text.len()
        );
        let bytes = text.as_bytes();
        // Line ends are LF or CRLF; the `\r` is never part of a word.
        let push = |words: &mut Vec<(u32, u32)>, start: usize, mut end: usize| {
            if end > start && bytes[end - 1] == b'\r' {
                end -= 1;
            }
            if end > start {
                words.push((start as u32, end as u32));
            }
        };
        let mut words = Vec::with_capacity(text.len() / 24);
        let mut start = if text.starts_with('\u{FEFF}') { '\u{FEFF}'.len_utf8() } else { 0 };
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                push(&mut words, start, i);
                start = i + 1;
            }
        }
        push(&mut words, start, bytes.len());
        let size = (words.len() * 3).next_power_of_two().max(16);
        let mask = size - 1;
        let mut table = alloc::vec![EMPTY; size];
        let mut max_len = 0;
        let all = text.as_bytes();
        let at = |(s, e): (u32, u32)| &all[s as usize..e as usize];
        for (i, &r) in words.iter().enumerate() {
            let b = at(r);
            max_len = max_len.max(b.len());
            let mut h = hash(b) as usize & mask;
            loop {
                match table[h] {
                    EMPTY => {
                        table[h] = i as u32;
                        break;
                    }
                    k if at(words[k as usize]) == b => break,
                    _ => h = (h + 1) & mask,
                }
            }
        }
        let folded_da = words
            .iter()
            .map(|&r| at(r))
            .filter(|b| b.windows(COENG_DA.len()).any(|w| w == COENG_DA.as_bytes()))
            .filter_map(|b| core::str::from_utf8(b).ok())
            .map(|w| w.replace(COENG_DA, COENG_TA))
            .collect();
        Dictionary { text, words, table, max_len, folded_da }
    }

    /// Lookup accepting either ជើងត (្ត) or ជើងដ (្ដ) where the list has the other.
    fn lookup(&self, word: &[u8]) -> bool {
        match subscript_ta_da(word) {
            None => self.lookup_exact(word),
            Some(_) => {
                let Ok(text) = core::str::from_utf8(word) else { return false };
                let folded = fold_subscript_da(text);
                self.lookup_exact(folded.as_bytes()) || self.folded_da.contains(folded.as_ref())
            }
        }
    }

    fn lookup_exact(&self, word: &[u8]) -> bool {
        if word.len() > self.max_len || self.words.is_empty() {
            return false;
        }
        let mask = self.table.len() - 1;
        let mut h = hash(word) as usize & mask;
        loop {
            match self.table[h] {
                EMPTY => return false,
                k if self.bytes(self.words[k as usize]) == word => return true,
                _ => h = (h + 1) & mask,
            }
        }
    }

    fn word(&self, (s, e): (u32, u32)) -> &str {
        &self.text[s as usize..e as usize]
    }

    /// Word bytes: same order as `&str`, without UTF-8 boundary checks.
    fn bytes(&self, (s, e): (u32, u32)) -> &[u8] {
        &self.text.as_bytes()[s as usize..e as usize]
    }

    /// Number of words.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// True when the dictionary has no words.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Every word, sorted.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.words.iter().map(|&r| self.word(r))
    }

    /// Lookup of a word that is already in normalized form (run
    /// [`normalize`] first if unsure). This does
    /// not normalize, to stay cheap in hot loops: an unnormalized word can miss
    /// even when the normalized spelling is listed. Accepts ជើងត (្ត) where the
    /// dictionary has ជើងដ (្ដ) and the other way round. They are different
    /// letters with different meanings in words, but they look alike, so OCR text
    /// may have either. aksar never changes one into the other: the ICU list
    /// mostly has ជើងត forms and lacks many standard ជើងដ spellings (measured:
    /// correcting to it rewrote 13-45 correct UDHR tokens such as អន្ដរជាតិ,
    /// សន្ដិសុខ). Use [`Dictionary::contains_exact`] and [`Dictionary::spellings`]
    /// to show the listed spelling for review.
    pub fn contains(&self, word: &str) -> bool {
        self.lookup(word.as_bytes())
    }

    /// Lookup of an already-normalized word, spelled exactly as given.
    pub fn contains_exact(&self, word: &str) -> bool {
        self.lookup_exact(word.as_bytes())
    }

    /// The dictionary's spellings of `word` (normalized first) when ្ត and ្ដ are
    /// accepted for each other, sorted. Empty when the word is unknown. Use it to
    /// offer the other spelling in a review screen.
    pub fn spellings(&self, word: &str) -> Vec<&str> {
        let w = normalize(word);
        let target = fold_subscript_da(&w).into_owned();
        // ្ត and ្ដ have the same length, so no spelling of a too-long word fits.
        if target.len() > self.max_len {
            return Vec::new();
        }
        let n = target.chars().filter(|&c| c == '\u{17D2}').count();
        // Every spelling is target with some of its subscript TAs written as DA.
        let positions: Vec<usize> = target.match_indices(COENG_TA).map(|(i, _)| i).collect();
        let mut out: Vec<&str> = Vec::new();
        if positions.len() > 12 || n > 16 {
            return out;
        }
        for mask in 0u32..(1 << positions.len()) {
            let mut cand = target.clone();
            for (bit, &at) in positions.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    cand.replace_range(at..at + COENG_TA.len(), COENG_DA);
                }
            }
            if let Some(found) = self.find_exact(&cand) {
                out.push(found);
            }
        }
        out.sort_unstable();
        out
    }

    fn find_exact(&self, word: &str) -> Option<&str> {
        let b = word.as_bytes();
        if b.len() > self.max_len || self.words.is_empty() {
            return None;
        }
        let mask = self.table.len() - 1;
        let mut h = hash(b) as usize & mask;
        loop {
            match self.table[h] {
                EMPTY => return None,
                k if self.bytes(self.words[k as usize]) == b => {
                    return Some(self.word(self.words[k as usize]));
                }
                _ => h = (h + 1) & mask,
            }
        }
    }

    /// Best segmentation cost of `token` (normalized first).
    pub fn segment_cost(&self, token: &str) -> Cost {
        Words::segment_cost(self, token)
    }

    /// Unknown Khmer clusters in the best segmentation of `token`.
    pub fn unknown_clusters(&self, token: &str) -> usize {
        Words::unknown_clusters(self, token)
    }

    /// Splits `token` into dictionary words (unknown clusters stand alone).
    pub fn segment<'t>(&self, token: &'t str) -> Vec<&'t str> {
        Words::segment(self, token)
    }

    /// Checks one recognized token (a run of text between spaces).
    pub fn check(&self, token: &str) -> Check {
        Words::check(self, token)
    }

    /// This dictionary plus a user word list (names and rare words a reviewer
    /// confirmed). The result has the same lookups, segmentation and checks.
    pub fn with_user_words<'s>(&'s self, user: &'s UserWords) -> WithUserWords<'s> {
        WithUserWords { base: self, user }
    }
}

mod sealed {
    /// What the algorithms need from a word set.
    pub trait Lexicon {
        /// Exact lookup of a normalized word.
        fn has(&self, word: &str) -> bool;
        /// Longest word in bytes (multi-cluster lookups stop past it).
        fn max_word_len(&self) -> usize;
    }
}
use sealed::Lexicon;

impl Lexicon for Dictionary<'_> {
    fn has(&self, word: &str) -> bool {
        self.lookup(word.as_bytes())
    }
    fn max_word_len(&self) -> usize {
        self.max_len
    }
}

/// Lookups, segmentation and the OCR corrector, for a [`Dictionary`] or a
/// dictionary with user words ([`Dictionary::with_user_words`]).
pub trait Words: Lexicon {
    /// Lookup of an already-normalized word, accepting either ជើងត or ជើងដ.
    fn contains(&self, word: &str) -> bool {
        self.has(word)
    }

    /// Best segmentation cost of `token` (normalized first).
    fn segment_cost(&self, token: &str) -> Cost {
        let token = normalize(token);
        let cl = clusters(&token);
        forward(self, &cl)[cl.len()]
    }

    /// Unknown Khmer clusters in the best segmentation of `token`.
    fn unknown_clusters(&self, token: &str) -> usize {
        self.segment_cost(token).unknown_clusters
    }

    /// Splits `token` into dictionary words (unknown clusters stand alone).
    fn segment<'t>(&self, token: &'t str) -> Vec<&'t str> {
        segment(self, token)
    }

    /// Checks one recognized token (a run of text between spaces).
    fn check(&self, token: &str) -> Check {
        check(self, token)
    }
}

impl Words for Dictionary<'_> {}

/// Calls `f(end)` for each run of clusters `cl[i..end]` (end > i+1) that is a
/// word. Returns whether `cl[i]` alone is a word.
fn walk<L: Lexicon + ?Sized>(
    lex: &L,
    cl: &[&str],
    i: usize,
    buf: &mut String,
    mut f: impl FnMut(usize),
) -> bool {
    let single = lex.has(cl[i]);
    buf.clear();
    buf.push_str(cl[i]);
    let end = cl.len().min(i + MAX_WORD_CLUSTERS);
    let max = lex.max_word_len();
    for (j, c) in cl.iter().enumerate().take(end).skip(i + 1) {
        buf.push_str(c);
        if buf.len() > max {
            break;
        }
        if lex.has(buf) {
            f(j + 1);
        }
    }
    single
}

/// Relaxes DP transitions out of position `i` of `cl`.
fn relax<L: Lexicon + ?Sized>(lex: &L, cl: &[&str], i: usize, best: &mut [Cost], buf: &mut String) {
    let base = best[i];
    if base == INF {
        return;
    }
    let khmer = cl[i].chars().next().is_some_and(is_word_char);
    if !khmer {
        let single = add(base, cost(0, 1));
        if single < best[i + 1] {
            best[i + 1] = single;
        }
        return;
    }
    let word = add(base, cost(0, 1));
    let single_known = walk(lex, cl, i, buf, |end| {
        if word < best[end] {
            best[end] = word;
        }
    });
    let single = add(base, cost(usize::from(!single_known), 1));
    if single < best[i + 1] {
        best[i + 1] = single;
    }
}

/// Forward costs: `fwd[i]` = best cost of `cl[..i]`.
fn forward<L: Lexicon + ?Sized>(lex: &L, cl: &[&str]) -> Vec<Cost> {
    let mut best = alloc::vec![INF; cl.len() + 1];
    best[0] = Cost::default();
    let mut buf = String::new();
    for i in 0..cl.len() {
        relax(lex, cl, i, &mut best, &mut buf);
    }
    best
}

/// Backward costs: `back[i]` = best cost of `cl[i..]`.
fn backward<L: Lexicon + ?Sized>(lex: &L, cl: &[&str]) -> Vec<Cost> {
    let n = cl.len();
    let mut back = alloc::vec![INF; n + 1];
    back[n] = Cost::default();
    let mut buf = String::new();
    for i in (0..n).rev() {
        let khmer = cl[i].chars().next().is_some_and(is_word_char);
        let mut b = INF;
        let single_known = khmer
            && walk(lex, cl, i, &mut buf, |end| {
                b = b.min(add(back[end], cost(0, 1)));
            });
        b = b.min(add(back[i + 1], cost(usize::from(khmer && !single_known), 1)));
        back[i] = b;
    }
    back
}

fn segment<'t, L: Lexicon + ?Sized>(lex: &L, token: &'t str) -> Vec<&'t str> {
    let cl = clusters(token);
    let n = cl.len();
    let mut best = alloc::vec![INF; n + 1];
    let mut from = alloc::vec![0usize; n + 1];
    best[0] = Cost::default();
    let mut buf = String::new();
    for i in 0..n {
        let hi = (i + MAX_WORD_CLUSTERS + 1).min(n + 1);
        let mut before = [INF; MAX_WORD_CLUSTERS];
        before[..hi - (i + 1)].copy_from_slice(&best[i + 1..hi]);
        relax(lex, &cl, i, &mut best, &mut buf);
        for (k, old) in before[..hi - (i + 1)].iter().enumerate() {
            if best[i + 1 + k] < *old {
                from[i + 1 + k] = i;
            }
        }
    }
    let mut cuts = alloc::vec![n];
    let mut j = n;
    while j > 0 {
        j = from[j];
        cuts.push(j);
    }
    cuts.reverse();
    let off = |k: usize| if k == n { token.len() } else { cl[k].as_ptr() as usize - token.as_ptr() as usize };
    cuts.windows(2).map(|w| &token[off(w[0])..off(w[1])]).collect()
}

/// Cost of `cl` with clusters `a..b` replaced by `new`, reusing `fwd`/`back`.
fn cost_with<L: Lexicon + ?Sized>(
    lex: &L,
    cl: &[&str],
    fwd: &[Cost],
    back: &[Cost],
    a: usize,
    b: usize,
    new: &[&str],
) -> Cost {
    let s = a.saturating_sub(MAX_WORD_CLUSTERS - 1);
    let tail_end = (b + MAX_WORD_CLUSTERS).min(cl.len());
    let mut seq: Vec<&str> = Vec::with_capacity(tail_end - s + new.len());
    seq.extend_from_slice(&cl[s..a]);
    seq.extend_from_slice(new);
    seq.extend_from_slice(&cl[b..tail_end]);
    let mut best = alloc::vec![INF; seq.len() + 1];
    best[..=a - s].copy_from_slice(&fwd[s..=a]);
    let mut buf = String::new();
    for i in 0..seq.len() {
        relax(lex, &seq, i, &mut best, &mut buf);
    }
    // Every segmentation has a boundary in any window of MAX_WORD_CLUSTERS
    // positions, so the best total passes through one of these cuts.
    let tail_start = a - s + new.len();
    (tail_start..=seq.len()).map(|t| add(best[t], back[b + (t - tail_start)])).min().unwrap_or(INF)
}

fn check<L: Words + ?Sized>(lex: &L, token: &str) -> Check {
    let token = normalize(token);
    if !token.chars().any(is_word_char) || lex.has(&token) {
        return Check::Known;
    }
    let cl = clusters(&token);
    let fwd = forward(lex, &cl);
    if fwd[cl.len()].unknown_clusters == 0 {
        return Check::Known;
    }
    let back = backward(lex, &cl);
    // Exact filter: every segmentation has a word boundary within any
    // MAX_WORD_CLUSTERS positions, so an edit of clusters a..b can only
    // reach zero unknowns if the untouched text before some cut in
    // [a-13, a] and after some cut in [b, b+13] is already fully known.
    let n = cl.len();
    let prefix_ok: Vec<bool> = (0..=n)
        .map(|a| fwd[a.saturating_sub(MAX_WORD_CLUSTERS - 1)..=a].iter().any(|c| c.unknown_clusters == 0))
        .collect();
    let suffix_ok: Vec<bool> = (0..=n)
        .map(|b| back[b..=(b + MAX_WORD_CLUSTERS - 1).min(n)].iter().any(|c| c.unknown_clusters == 0))
        .collect();

    // Candidate edits, each as (first cluster, end cluster, replacement text).
    // (first cluster, end cluster, replacement, changes made): single edits count 1.
    let mut edits: Vec<(usize, usize, String, usize)> = Vec::new();
    for (k, c) in cl.iter().enumerate() {
        let chars: Vec<char> = c.chars().collect();
        let single = chars.len() <= MAX_EDIT_CLUSTER_CHARS && prefix_ok[k] && suffix_ok[k + 1];
        for i in (0..chars.len()).filter(|_| single) {
            if let Some(g) = GROUPS.iter().find(|g| g.contains(&chars[i])) {
                for &r in g.iter().filter(|&&r| r != chars[i]) {
                    let mut v = chars.clone();
                    v[i] = r;
                    edits.push((k, k + 1, v.into_iter().collect(), 1));
                }
            }
            if INSERTABLE.contains(&chars[i]) {
                let mut v = chars.clone();
                v.remove(i);
                edits.push((k, k + 1, v.into_iter().collect(), 1));
            }
            if is_consonant(chars[i]) {
                for &ins in INSERTABLE {
                    let mut v = chars.clone();
                    v.insert(i + 1, ins);
                    edits.push((k, k + 1, v.into_iter().collect(), 1));
                }
            }
        }
        // Pre-base vowel emitted in visual order: េ/ែ/ៃ ends this cluster (or
        // stands alone) and belongs after the next cluster's consonant stack.
        if let (Some(&last), Some(next)) = (chars.last(), cl.get(k + 1)) {
            let nchars: Vec<char> = next.chars().collect();
            if matches!(last, 'េ' | 'ែ' | 'ៃ') && nchars.first().is_some_and(|&c| is_consonant(c)) {
                let mut stack = 1;
                while stack + 1 < nchars.len()
                    && nchars[stack] == '\u{17D2}'
                    && is_consonant(nchars[stack + 1])
                {
                    stack += 2;
                }
                let head: String = chars[..chars.len() - 1].iter().collect();
                let cons: String = nchars[..stack].iter().collect();
                let rest: String = nchars[stack..].iter().collect();
                if last == 'េ' && rest.starts_with('ា') {
                    for merged in ['ោ', 'ៅ'] {
                        edits.push((
                            k,
                            k + 2,
                            alloc::format!("{head}{cons}{merged}{}", &rest['ា'.len_utf8()..]),
                            1,
                        ));
                    }
                }
                edits.push((k, k + 2, alloc::format!("{head}{cons}{last}{rest}"), 1));
            }
        }
    }
    edits.extend(visual_order_edits(&cl));

    // (fixed token, words in its segmentation, changes made)
    let mut found: Vec<(String, usize, usize)> = Vec::new();
    for (a, b, text, changes) in edits {
        if !prefix_ok[a] || !suffix_ok[b] {
            continue;
        }
        let text = normalize(&text);
        let new = clusters(&text);
        let cost = cost_with(lex, &cl, &fwd, &back, a, b, &new);
        if cost.unknown_clusters != 0 {
            continue;
        }
        let start = if a == 0 { 0 } else { cl[a].as_ptr() as usize - token.as_ptr() as usize };
        let end = if b == cl.len() { token.len() } else { cl[b].as_ptr() as usize - token.as_ptr() as usize };
        let full = normalize(&alloc::format!("{}{}{}", &token[..start], text, &token[end..]));
        // Confirm on the whole string (normalization can cross cluster edges).
        let exact = lex.segment_cost(&full);
        if full == token || exact.unknown_clusters != 0 {
            continue;
        }
        match found.iter_mut().find(|f| f.0 == full) {
            Some(f) => f.2 = f.2.min(changes),
            None => found.push((full, exact.words, changes)),
        }
    }
    // Keep the fixes that segment into the fewest words, then those that change
    // the least; correct only when exactly one remains.
    let best = found.iter().map(|f| (f.1, f.2)).min();
    let mut winners: Vec<String> =
        found.into_iter().filter(|f| Some((f.1, f.2)) == best).map(|f| f.0).collect();
    if winners.len() == 1 { Check::Corrected(winners.pop().unwrap_or_default()) } else { Check::Suspect }
}

/// Vowels whose drawing already starts with the left-hand េ part.
fn starts_with_left_e(c: char) -> bool {
    matches!(c, 'េ' | 'ោ' | 'ៅ' | 'ើ' | 'ឿ' | 'ៀ')
}

/// Tesseract writes the left-hand vowels េ ែ ៃ where they are drawn, before the
/// consonant, so one OCR word can hold several of them (`ទិញេសៀវេភៅ` for
/// `ទិញសៀវភៅ`). The single-edit rules fix only one; these edits fix any
/// combination at once (accuracy plan M6). For every cluster ending in such a
/// vowel and followed by a consonant cluster, the vowel either moves after the
/// next consonant stack (where `normalize` composes េ+ា → ោ and េ+ី → ើ), or is
/// dropped when the next vowel already contains it (`េ` + `ៅ`). Each candidate
/// returned spans the first to the last moved vowel and counts one change per
/// vowel; `check` prefers fewer changes and still accepts a fix only when
/// exactly one best candidate makes the whole token known. Sites further apart
/// than `2 * MAX_WORD_CLUSTERS` clusters are not combined: in a long unspaced
/// paragraph they belong to different words, and every candidate spanning them
/// would cost the whole span to build and score.
fn visual_order_edits(cl: &[&str]) -> Vec<(usize, usize, String, usize)> {
    // Per position: (k, replacement options for clusters k and k + 1).
    let mut sites: Vec<(usize, Vec<String>)> = Vec::new();
    for k in 0..cl.len().saturating_sub(1) {
        let chars: Vec<char> = cl[k].chars().collect();
        let nchars: Vec<char> = cl[k + 1].chars().collect();
        let Some(&last) = chars.last() else { continue };
        if !matches!(last, 'េ' | 'ែ' | 'ៃ') || !nchars.first().is_some_and(|&c| is_consonant(c)) {
            continue;
        }
        if sites.last().is_some_and(|(p, _)| *p + 1 == k) {
            continue; // overlapping with the previous site
        }
        let mut stack = 1;
        while stack + 1 < nchars.len() && nchars[stack] == '\u{17D2}' && is_consonant(nchars[stack + 1]) {
            stack += 2;
        }
        let head: String = chars[..chars.len() - 1].iter().collect();
        let cons: String = nchars[..stack].iter().collect();
        let rest: String = nchars[stack..].iter().collect();
        let mut options = alloc::vec![alloc::format!("{head}{cons}{last}{rest}")];
        if last == 'េ' && rest.starts_with('ា') {
            options.push(alloc::format!("{head}{cons}ោ{}", &rest['ា'.len_utf8()..]));
            options.push(alloc::format!("{head}{cons}ៅ{}", &rest['ា'.len_utf8()..]));
        }
        let next_vowel = rest.chars().next();
        if next_vowel.is_some_and(|v| (last == 'េ' && starts_with_left_e(v)) || v == last) {
            options.push(alloc::format!("{head}{cons}{rest}"));
        }
        sites.push((k, options));
    }
    if sites.is_empty() || sites.len() > 6 {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Every non-empty subset of sites, every option per chosen site (capped).
    for mask in 1u32..(1 << sites.len()) {
        let chosen: Vec<&(usize, Vec<String>)> =
            sites.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, s)| s).collect();
        if chosen[chosen.len() - 1].0 - chosen[0].0 > 2 * MAX_WORD_CLUSTERS {
            continue;
        }
        // Single moves with only the plain option are already in the edit list.
        let combos: usize = chosen.iter().map(|(_, o)| o.len()).product();
        if combos > 64 {
            continue;
        }
        for mut n in 0..combos {
            let a = chosen[0].0;
            let b = chosen[chosen.len() - 1].0 + 2;
            let mut text = String::new();
            let mut at = a;
            for (k, options) in &chosen {
                for c in &cl[at..*k] {
                    text.push_str(c);
                }
                text.push_str(&options[n % options.len()]);
                n /= options.len();
                at = k + 2;
            }
            out.push((a, b, text, chosen.len()));
        }
    }
    out
}

/// One recognized symbol with the recognizer's alternatives (Tesseract's
/// "choice iterator", or the top-k of any recognizer).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SymbolChoice {
    /// The chosen symbol (one or more characters).
    pub text: String,
    /// Its confidence, any scale (Tesseract: 0-100).
    pub confidence: f32,
    /// Other readings with their confidence, same scale.
    pub alternatives: Vec<(String, f32)>,
}

impl SymbolChoice {
    /// A symbol with its confidence and alternatives.
    pub fn new(text: impl Into<String>, confidence: f32, alternatives: Vec<(String, f32)>) -> Self {
        Self { text: text.into(), confidence, alternatives }
    }
}

/// Re-ranks a word's symbol alternatives with the dictionary (accuracy plan M4):
/// when the chosen symbols leave Khmer clusters unknown, try swapping in one
/// alternative whose confidence is within `max_gap` of the chosen symbol's; the
/// fix is the swap that makes every cluster known with the highest alternative
/// confidence. `None` when the word is already known, nothing qualifies, or the
/// best swaps tie with different results (the recognizer can't tell; leave it).
/// Results are normalized. PDFTape used `max_gap` 25 on Tesseract confidences.
pub fn best_alternative<L: Words + ?Sized>(
    lex: &L,
    symbols: &[SymbolChoice],
    max_gap: f32,
) -> Option<String> {
    let text: String = symbols.iter().map(|s| s.text.as_str()).collect();
    if !text.chars().any(is_word_char) || lex.unknown_clusters(&text) == 0 {
        return None;
    }
    let mut best: Option<(f32, String)> = None;
    let mut tie = false;
    for (i, sym) in symbols.iter().enumerate() {
        for (alt, conf) in &sym.alternatives {
            // False for a NaN confidence or gap, so NaN never qualifies.
            let close_enough = *conf >= sym.confidence - max_gap;
            if !close_enough || *alt == sym.text {
                continue;
            }
            let cand: String = symbols
                .iter()
                .enumerate()
                .map(|(j, s)| if j == i { alt.as_str() } else { s.text.as_str() })
                .collect();
            let cand = normalize(&cand);
            if lex.unknown_clusters(&cand) != 0 {
                continue;
            }
            match &best {
                Some((c, b)) if conf < c || (conf == c && *b == cand) => {}
                Some((c, _)) if conf == c => tie = true,
                _ => {
                    best = Some((*conf, cand));
                    tie = false;
                }
            }
        }
    }
    if tie { None } else { best.map(|b| b.1) }
}

/// Longest user word kept, in characters (a word, not a sentence).
pub const MAX_USER_WORD_CHARS: usize = 60;

/// Words a user confirmed or typed in review (names, rare words), kept on top
/// of a [`Dictionary`]. Stored normalized; cheap to clone and persist.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserWords {
    words: alloc::collections::BTreeSet<String>,
    /// `words` with subscript DA folded to TA, for lookups.
    folded: alloc::collections::BTreeSet<String>,
    max_len: usize,
}

impl UserWords {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// The normalized form of `word` if it can be a user word for `base`:
    /// Khmer word characters only (no digits, punctuation or Latin), at most
    /// [`MAX_USER_WORD_CHARS`] characters and as many clusters as segmentation
    /// can see, and not already in `base`.
    pub fn accept(base: &Dictionary<'_>, word: &str) -> Option<String> {
        let w = normalize(word.trim());
        let ok = !w.is_empty()
            && w.chars().count() <= MAX_USER_WORD_CHARS
            && clusters(&w).len() <= MAX_WORD_CLUSTERS
            && w.chars().all(is_word_char)
            && !base.contains(&w);
        ok.then_some(w)
    }

    /// Adds `word` (see [`UserWords::accept`]). Returns its normalized form when it
    /// was new, so the caller can persist exactly that.
    pub fn insert(&mut self, base: &Dictionary<'_>, word: &str) -> Option<String> {
        let w = Self::accept(base, word)?;
        if self.words.contains(&w) {
            return None;
        }
        self.max_len = self.max_len.max(w.len());
        self.folded.insert(fold_subscript_da(&w).into_owned());
        self.words.insert(w.clone());
        Some(w)
    }

    /// Removes `word` (normalized first). Returns whether it was there.
    ///
    /// This rebuilds the folded lookup set, so it is O(n) in the number of user
    /// words: fine for hundreds of words, slow in a loop over tens of thousands.
    pub fn remove(&mut self, word: &str) -> bool {
        let removed = self.words.remove(&normalize(word.trim()));
        if removed {
            self.max_len = self.words.iter().map(String::len).max().unwrap_or(0);
            self.folded = self.words.iter().map(|w| fold_subscript_da(w).into_owned()).collect();
        }
        removed
    }

    /// Lookup of a normalized word, accepting either ជើងត or ជើងដ.
    pub fn contains(&self, word: &str) -> bool {
        self.words.contains(word) || self.folded.contains(fold_subscript_da(word).as_ref())
    }

    /// Number of words.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// True when there are no user words.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The words, sorted.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.words.iter().map(String::as_str)
    }
}

/// A [`Dictionary`] plus [`UserWords`]; see [`Dictionary::with_user_words`].
#[derive(Clone, Copy)]
pub struct WithUserWords<'s> {
    base: &'s Dictionary<'s>,
    user: &'s UserWords,
}

impl Lexicon for WithUserWords<'_> {
    fn has(&self, word: &str) -> bool {
        self.base.lookup(word.as_bytes()) || self.user.contains(word)
    }
    fn max_word_len(&self) -> usize {
        self.base.max_len.max(self.user.max_len)
    }
}

impl Words for WithUserWords<'_> {}

/// These mirror [`Words`] so callers need not import the trait.
impl WithUserWords<'_> {
    /// Lookup of an already-normalized word, accepting either ជើងត or ជើងដ.
    pub fn contains(&self, word: &str) -> bool {
        Words::contains(self, word)
    }

    /// Best segmentation cost of `token` (normalized first).
    pub fn segment_cost(&self, token: &str) -> Cost {
        Words::segment_cost(self, token)
    }

    /// Unknown Khmer clusters in the best segmentation of `token`.
    pub fn unknown_clusters(&self, token: &str) -> usize {
        Words::unknown_clusters(self, token)
    }

    /// Splits `token` into words (unknown clusters stand alone).
    pub fn segment<'t>(&self, token: &'t str) -> Vec<&'t str> {
        Words::segment(self, token)
    }

    /// Checks one recognized token (a run of text between spaces).
    pub fn check(&self, token: &str) -> Check {
        Words::check(self, token)
    }
}

#[cfg(all(test, feature = "bundled-dict"))]
mod tests {
    use super::*;

    fn d() -> &'static Dictionary<'static> {
        Dictionary::bundled()
    }

    // The tests below are PDFTape's, kept as regression tests.
    #[test]
    fn known_phrases_have_no_unknown_clusters() {
        assert_eq!(d().unknown_clusters("ព្រះរាជាណាចក្រកម្ពុជា"), 0);
        assert_eq!(d().unknown_clusters("ក្រសួងអប់រំ"), 0);
        assert_eq!(d().check("PDF"), Check::Known);
    }

    #[test]
    fn corrects_a_dropped_vowel_uniquely() {
        assert_eq!(d().check("កម្ពជា"), Check::Corrected("កម្ពុជា".into()));
        assert_eq!(
            d().check("ទីក្រុងភ្នំពេញគឺជារាជធានីនៃប្រទេសកម្ពជា"),
            Check::Corrected("ទីក្រុងភ្នំពេញគឺជារាជធានីនៃប្រទេសកម្ពុជា".into())
        );
    }

    #[test]
    fn fixes_visual_order_pre_base_vowels() {
        assert_eq!(d().check("ពិភពេលាក"), Check::Corrected("ពិភពលោក".into()));
        assert_eq!(d().check("វិក្កយបត្រេលខ"), Check::Corrected("វិក្កយបត្រលេខ".into()));
    }

    #[test]
    fn digits_and_punctuation_are_not_unknown_words() {
        assert_eq!(d().check("ឆ្នាំ២០២៦"), Check::Known);
        assert_eq!(d().check("ថ្ងៃទី១០"), Check::Known);
        assert_eq!(d().check("ផងដែរ។"), Check::Known);
        assert_eq!(d().check("២០២៦"), Check::Known);
        assert_eq!(d().check("ឆ្ឃឞឞ២០២៦"), Check::Suspect);
    }

    #[test]
    fn leaves_unfixable_tokens_as_suspect() {
        assert_eq!(d().check("ឆ្ឃឞឞ"), Check::Suspect);
    }

    #[test]
    fn known_word_and_its_misspelling() {
        assert!(d().contains("រដូវ"));
        assert!(d().unknown_clusters("រដួវ") > 0);
    }

    // ជើងត and ជើងដ (accuracy plan M3).
    /// Errors implement `core::error::Error` without `std`, so no_std callers can
    /// box them or use `?` into their own error types.
    #[test]
    fn errors_are_core_errors() {
        fn is_error<E: core::error::Error>() {}
        is_error::<NotPrepared>();
        #[cfg(feature = "shape")]
        is_error::<crate::shape::Error>();
    }

    /// Every bundled entry is a Khmer word that renders: no license line,
    /// comment or BOM slipped in from the ICU source file.
    #[test]
    fn bundled_list_has_only_valid_khmer_words() {
        for line in include_str!("../data/khmer-words.txt").lines() {
            // ZWNJ/ZWJ are part of some spellings (`ហ៊\u{200c}ិន`).
            let khmer = |c| crate::chars::is_khmer(c) || matches!(c, '\u{200c}' | '\u{200d}');
            assert!(line.chars().all(khmer), "{line:?}");
            assert!(crate::syllable::validate(line).is_ok(), "{line:?}");
            // Lookups normalize the query, so an entry not in normal form never matches.
            assert_eq!(normalize(line), line, "rebuild with clean_wordlist");
        }
        assert_eq!(d().len(), 80_630);
    }

    #[test]
    fn lookups_accept_either_subscript_ta_or_da() {
        for (ta, da) in [("ស្តាប់", "ស្ដាប់"), ("ក្តី", "ក្ដី"), ("ប្តី", "ប្ដី"), ("ចិត្ត", "ចិត្ដ")]
        {
            assert!(d().contains(ta) && d().contains(da), "{ta} / {da}");
            assert_eq!(d().check(ta), Check::Known);
            assert_eq!(d().check(da), Check::Known);
        }
        // Every listed word is known in both spellings of its subscripts.
        for line in include_str!("../data/khmer-words.txt").lines() {
            let swapped =
                line.replace(COENG_DA, "\u{0}").replace(COENG_TA, COENG_DA).replace('\u{0}', COENG_TA);
            assert!(d().contains(line) && d().contains(&swapped), "{line}");
        }
    }

    #[test]
    fn the_subscript_spelling_is_never_changed_but_the_listed_one_is_offered() {
        // A word listed only with ជើងត, read with ជើងដ: kept as read (the list is
        // not an authority on this choice), and the listed spelling is available.
        let only_ta = include_str!("../data/khmer-words.txt")
            .lines()
            .find(|w| w.contains(COENG_TA) && !d().contains_exact(&w.replace(COENG_TA, COENG_DA)))
            .unwrap();
        let read = only_ta.replace(COENG_TA, COENG_DA);
        assert_eq!(d().check(&read), Check::Known);
        assert_eq!(d().spellings(&read), [only_ta]);
        // Standard ជើងដ spellings missing from the list stay untouched.
        for t in ["អន្ដរជាតិ", "សន្ដិសុខ", "ពេញចិត្ដ"] {
            assert!(!matches!(d().check(t), Check::Corrected(_)), "{t}");
        }
    }

    #[test]
    fn a_ta_da_difference_is_never_an_edit_and_keeps_the_spelling_read() {
        // Base ដ misread for ត: fixed, and the subscript stays as read.
        assert_eq!(d().check("ចិដ្ដ"), Check::Corrected("ចិត្ដ".into()));
        assert_eq!(d().check("ចិដ្ត"), Check::Corrected("ចិត្ត".into()));
        assert_eq!(d().check("ឧដ្ដម"), Check::Corrected("ឧត្ដម".into()));
    }

    #[test]
    fn spellings_lists_the_dictionary_forms() {
        // Sorted: ដ (U+178A) before ត (U+178F).
        assert_eq!(d().spellings("ស្តី"), ["ស្ដី", "ស្តី"]);
        assert_eq!(d().spellings("ស្ដី"), ["ស្ដី", "ស្តី"]);
        assert!(d().spellings("ឆ្ឃឞឞ").is_empty());
        assert!(d().contains_exact("ស្តី") && d().contains_exact("ស្ដី"));
        // A word listed only with ្ត: found by `contains`, not by `contains_exact`.
        let only_ta = include_str!("../data/khmer-words.txt")
            .lines()
            .find(|w| w.contains(COENG_TA) && !d().contains_exact(&w.replace(COENG_TA, COENG_DA)))
            .unwrap();
        let as_da = only_ta.replace(COENG_TA, COENG_DA);
        assert!(d().contains(&as_da) && !d().contains_exact(&as_da));
        assert_eq!(d().spellings(&as_da), [only_ta]);
        assert_eq!(fold_subscript_da("ក្ដី"), "ក្តី");
    }

    #[test]
    fn user_words_ignore_the_ta_da_difference() {
        let mut user = UserWords::new();
        assert!(user.insert(d(), "សុខស្ដារ៉ា").is_some());
        let lex = d().with_user_words(&user);
        assert!(lex.contains("សុខស្តារ៉ា") && lex.contains("សុខស្ដារ៉ា"));
        assert!(user.remove("សុខស្ដារ៉ា"));
        assert!(!d().with_user_words(&user).contains("សុខស្តារ៉ា"));
    }

    // Left-hand vowels written before the consonant (accuracy plan M6).
    #[test]
    fn fixes_several_visual_order_vowels_in_one_word() {
        // Two fixes make this known; the one with fewer changes wins (as in v0.1.1).
        assert_eq!(d().check("និងេរសៀវេតៅគ្រួសារ"), Check::Corrected("និងេរសៀវតៅគ្រួសារ".into()));
        assert_eq!(d().check("ទិញេសៀវេភៅ"), Check::Corrected("ទិញសៀវភៅ".into()));
        assert_eq!(d().check("េបីកេធ្វីការ"), Check::Corrected("បើកធ្វើការ".into()));
        // A misplaced vowel that leaves the word unknown either way stays flagged.
        assert_eq!(d().check("េឆ្ឃឞឞ"), Check::Suspect);
        assert_eq!(d().check("ការិយាល័យេបីក"), Check::Corrected("ការិយាល័យបើក".into()));
        assert_eq!(d().check("ទំនិញេកីនេឡើង"), Check::Corrected("ទំនិញកើនឡើង".into()));
    }

    /// Khmer has no spaces, so `read` can hand `check` a whole paragraph. Moved
    /// vowels far apart are in different words: combining them built candidates
    /// as long as the paragraph (60 KB took 1 s). Only nearby sites combine.
    #[test]
    fn far_apart_visual_order_sites_are_not_combined() {
        let filler = "ព្រះរាជាណាចក្រកម្ពុជា".repeat(10);
        let token = alloc::format!("ទិញេសៀវភៅ{filler}ទិញេសៀវភៅ");
        let cl = clusters(&token);
        let edits = visual_order_edits(&cl);
        assert!(!edits.is_empty() && edits.iter().all(|e| e.3 == 1), "only single-site edits");
        let near = clusters("ទិញេសៀវេភៅ");
        assert!(visual_order_edits(&near).iter().any(|e| e.3 == 2), "nearby sites still combine");
    }

    // Symbol alternatives (accuracy plan M4).
    #[test]
    fn picks_the_alternative_that_makes_the_word_known() {
        let sym = |t: &str, c: f32, alts: &[(&str, f32)]| SymbolChoice {
            text: t.into(),
            confidence: c,
            alternatives: alts.iter().map(|(a, c)| (a.to_string(), *c)).collect(),
        };
        // រដួវ → រដូវ: the vowel's runner-up makes the word known.
        let word = [
            sym("រ", 95.0, &[]),
            sym("ដ", 93.0, &[]),
            sym("ួ", 70.0, &[("ូ", 60.0), ("ុ", 30.0)]),
            sym("វ", 90.0, &[]),
        ];
        assert_eq!(best_alternative(d(), &word, 25.0), Some("រដូវ".into()));
        // Too far below the chosen symbol: not used.
        assert_eq!(best_alternative(d(), &word, 5.0), None);
        // Already known: nothing to do.
        let known =
            [sym("រ", 95.0, &[]), sym("ដ", 93.0, &[]), sym("ូ", 90.0, &[("ួ", 80.0)]), sym("វ", 90.0, &[])];
        assert_eq!(best_alternative(d(), &known, 25.0), None);
    }

    /// A recognizer can emit NaN scores; one must never count as close enough
    /// to rewrite a word.
    #[test]
    fn nan_confidence_never_qualifies() {
        let sym = |t: &str, c: f32, alts: &[(&str, f32)]| SymbolChoice {
            text: t.into(),
            confidence: c,
            alternatives: alts.iter().map(|(a, c)| (a.to_string(), *c)).collect(),
        };
        let word = |vowel: f32, alt: f32| {
            [sym("រ", 95.0, &[]), sym("ដ", 93.0, &[]), sym("ួ", vowel, &[("ូ", alt)]), sym("វ", 90.0, &[])]
        };
        assert_eq!(best_alternative(d(), &word(70.0, f32::NAN), 25.0), None);
        assert_eq!(best_alternative(d(), &word(f32::NAN, 60.0), 25.0), None);
        assert_eq!(best_alternative(d(), &word(70.0, 60.0), f32::NAN), None);
    }

    // aksar additions.
    #[test]
    fn segments_into_words() {
        assert_eq!(d().segment("ព្រះរាជាណាចក្រកម្ពុជា").concat(), "ព្រះរាជាណាចក្រកម្ពុជា");
        assert!(d().segment("ព្រះរាជាណាចក្រកម្ពុជា").len() >= 2);
    }

    #[test]
    fn cleaned_list_drops_broken_entries() {
        // In the raw ICU list but renders as a broken cluster.
        assert!(!d().contains("ខន្តីី"));
        assert!(d().len() > 80_000);
    }

    // PDFTape's user-word tests (its learned-words feature), on aksar.
    #[test]
    fn user_words_stop_flagging_and_can_be_removed() {
        // A made-up name, so it never collides with the ICU list.
        let name = "ឞ្ឃោង";
        let mut user = UserWords::new();
        assert_eq!(d().with_user_words(&user).check(name), Check::Suspect);
        assert_eq!(user.insert(d(), name), Some(normalize(name)));
        assert_eq!(user.insert(d(), "PDF"), None);
        assert_eq!(user.insert(d(), ""), None);
        // Adding again reports nothing new.
        assert_eq!(user.insert(d(), name), None);
        let w = d().with_user_words(&user);
        assert_eq!(w.check(name), Check::Known);
        // Also inside a longer token, next to dictionary words.
        assert_eq!(w.unknown_clusters(&alloc::format!("លោក{name}")), 0);
        assert!(user.remove(name));
        assert_eq!(d().with_user_words(&user).check(name), Check::Suspect);
    }

    #[test]
    fn user_words_longer_than_segmentation_can_see_are_refused() {
        let long = "ឞា".repeat(MAX_WORD_CLUSTERS + 1);
        assert_eq!(UserWords::accept(d(), &long), None);
        let ok = "ឞា".repeat(MAX_WORD_CLUSTERS);
        assert!(UserWords::accept(d(), &ok).is_some());
    }

    #[test]
    fn user_word_filter_keeps_only_new_khmer_words() {
        assert!(UserWords::accept(d(), "ឞ្ឃោង").is_some());
        assert_eq!(UserWords::accept(d(), "កម្ពុជា"), None); // already in the list
        assert_eq!(UserWords::accept(d(), "hello"), None);
        assert_eq!(UserWords::accept(d(), "២០២៦"), None);
        assert_eq!(UserWords::accept(d(), ""), None);
    }

    #[test]
    fn user_words_enable_corrections_next_to_them() {
        let mut user = UserWords::new();
        user.insert(d(), "ឞ្ឃោង");
        assert_eq!(d().with_user_words(&user).check("ឞ្ឃោងកម្ពជា"), Check::Corrected("ឞ្ឃោងកម្ពុជា".into()));
    }

    #[test]
    fn prepared_list_must_be_sorted() {
        assert!(Dictionary::from_prepared("ក\nខ\n").is_ok());
        assert_eq!(Dictionary::from_prepared("ខ\nក\n").err(), Some(NotPrepared { line: 2 }));
        let own = Dictionary::new("ខ\nក\nកា្រ\n");
        assert!(own.contains("ក្រា") && own.contains("ក"));
    }

    // Edits are skipped for clusters longer than any word's, so the cap must
    // stay above the longest cluster in the bundled list or fixes would be lost.
    #[test]
    fn edit_cap_is_above_every_word_cluster() {
        let longest = d().words().flat_map(clusters).map(|c| c.chars().count()).max().unwrap();
        assert!(longest + 1 < MAX_EDIT_CLUSTER_CHARS, "longest word cluster: {longest}");
    }

    // One huge cluster of garbage OCR text must not take quadratic time
    // (20,000 signs took 20 s before the cap).
    #[test]
    fn long_cluster_is_checked_quickly() {
        let junk: String = core::iter::once('ក').chain(core::iter::repeat_n('ំ', 20_000)).collect();
        let t = std::time::Instant::now();
        assert_eq!(d().check(&junk), Check::Suspect);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }

    // Lists saved on Windows have CRLF endings and often a BOM. The `\r` stuck
    // to every word of a borrowed list (no lookup matched) and `new` lost the
    // first word to the BOM.
    #[test]
    fn crlf_and_bom_lists_work() {
        let list = "\u{FEFF}\u{1780}\r\n\u{1781}\r\n";
        let prepared = Dictionary::from_prepared(list).unwrap();
        assert!(prepared.contains("\u{1780}") && prepared.contains("\u{1781}"));
        let built = Dictionary::new(list);
        assert!(built.contains("\u{1780}") && built.contains("\u{1781}"));
        // Out-of-order check still sees clean words, and reports the right line.
        assert_eq!(
            Dictionary::from_prepared("\u{1781}\r\n\u{1780}\r\n").err(),
            Some(NotPrepared { line: 2 })
        );
    }
}
