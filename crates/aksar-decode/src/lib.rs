//! Turns a line recognizer's output (CTC log-probabilities per time step) into
//! Khmer text, using what aksar knows about Khmer (`docs/OCR_PLAN.md` §4.2):
//!
//! - **Grammar:** a prefix that contains a broken syllable (marks with no base,
//!   which renders with a dotted circle ◌) is dropped the moment it appears.
//!   Appending characters can never repair it, so this loses nothing and the
//!   output is always valid Khmer.
//! - **Dictionary:** finished candidates are re-ranked by how many Khmer
//!   clusters the dictionary (plus user words) leaves unknown, weighted against
//!   the recognizer's own score. Numbers and Latin are never re-ranked.
//!
//! The recognizer is not part of this crate: anything that produces a `T × V`
//! matrix of log-probabilities over an alphabet with a blank works.
//!
//! ```
//! use aksar_decode::{Decoder, Options};
//! // Alphabet: blank, then symbols.
//! let alphabet = ["", "ក", "ា", "ម"];
//! let dec = Decoder::new(&alphabet).unwrap();
//! // Two steps: "ក" then "ា".
//! let ln = |p: f32| p.ln();
//! let frames = vec![
//!     vec![ln(0.05), ln(0.9), ln(0.03), ln(0.02)],
//!     vec![ln(0.05), ln(0.02), ln(0.9), ln(0.03)],
//! ];
//! let best = &dec.decode(&frames, &Options::default())[0];
//! assert_eq!(best.text, "កា");
//! ```

use aksar::dict::{Dictionary, UserWords};
use aksar::syllable::{SyllableKind, normalize, syllables};
use std::collections::HashMap;

/// Errors building a [`Decoder`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The alphabet must start with the blank (an empty string) and have symbols.
    BadAlphabet,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "alphabet must be [\"\", symbol, ...]")
    }
}

impl std::error::Error for Error {}

/// Decoding settings.
#[derive(Clone, Debug)]
pub struct Options {
    /// Candidates kept per time step (at least one).
    pub beam: usize,
    /// Natural-log penalty per unknown Khmer cluster when re-ranking finished
    /// candidates (0 = dictionary off). Tune on a validation set.
    pub unknown_cluster_penalty: f32,
    /// Drop prefixes with a broken syllable (the grammar constraint).
    pub grammar: bool,
    /// Candidates returned (best first; at least one).
    pub n_best: usize,
    /// Natural-log margin: at each step, symbols less likely than the step's
    /// best by more than this are not tried. 10 keeps anything above about
    /// 1/22000 of the best; larger is slower and almost never changes the text.
    pub prune: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options { beam: 16, unknown_cluster_penalty: 2.0, grammar: true, n_best: 5, prune: 10.0 }
    }
}

/// One decoded reading.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Candidate {
    /// Normalized text.
    pub text: String,
    /// Recognizer log-probability of the text (all CTC paths).
    pub ctc: f32,
    /// Khmer clusters the dictionary doesn't cover.
    pub unknown_clusters: usize,
    /// Final score used for ranking: `ctc - penalty * unknown_clusters`.
    pub score: f32,
}

impl Candidate {
    /// A candidate with the given scores.
    pub fn new(text: impl Into<String>, ctc: f32, unknown_clusters: usize, score: f32) -> Self {
        Self { text: text.into(), ctc, unknown_clusters, score }
    }

    /// How sure the recognizer is of this reading, 0 to 1: the geometric mean
    /// probability per character (`exp(ctc / characters)`, spaces not
    /// counted), so long and short lines compare. Use it to send low-confidence
    /// lines to review; the threshold is for the caller to tune. Text with
    /// nothing but whitespace (a blank line) has confidence 0: the recognizer
    /// read no characters, so there is nothing to be sure of.
    pub fn confidence(&self) -> f32 {
        let chars = self.text.chars().filter(|c| !c.is_whitespace()).count();
        if chars == 0 || self.ctc.is_nan() {
            return 0.0;
        }
        (self.ctc / chars as f32).exp().clamp(0.0, 1.0)
    }
}

/// A CTC decoder for one alphabet.
pub struct Decoder {
    symbols: Vec<String>,
    user: UserWords,
}

/// A log-probability as the search uses it: NaN is impossible, and +inf (not a
/// probability; a broken model) counts as certain.
fn clean(v: f32) -> f32 {
    if v.is_nan() {
        f32::NEG_INFINITY
    } else if v == f32::INFINITY {
        0.0
    } else {
        v
    }
}

fn log_add(a: f32, b: f32) -> f32 {
    if a == f32::NEG_INFINITY {
        return b;
    }
    if b == f32::NEG_INFINITY {
        return a;
    }
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    hi + (lo - hi).exp().ln_1p()
}

/// True when `text` has no broken syllable (checked on its tail: earlier
/// syllables were checked when they were added).
fn tail_is_valid(text: &str) -> bool {
    syllables(&text[cluster_start_before(text, text.len().saturating_sub(64))..])
        .iter()
        .all(|s| s.kind != SyllableKind::Broken)
}

/// A byte offset at or before `at` where a syllable surely starts, so parsing
/// from there gives the same syllables as parsing the whole text: a Khmer base
/// (consonant or independent vowel) not attached by a coeng or joiner before
/// it, or any character that is neither Khmer nor a joiner. 0 if none is near.
fn cluster_start_before(text: &str, at: usize) -> usize {
    let is_base = |c: char| ('\u{1780}'..='\u{17B3}').contains(&c);
    let is_khmer_or_joiner =
        |c: char| ('\u{1780}'..='\u{17FF}').contains(&c) || c == '\u{200C}' || c == '\u{200D}';
    // Candidates: every character starting at or before `at`, nearest first.
    let mut chars = text.char_indices().rev().skip_while(|&(i, _)| i > at).peekable();
    let mut seen = 0;
    while let Some((i, c)) = chars.next() {
        seen += c.len_utf8();
        if seen > 256 {
            return 0;
        }
        let attached =
            chars.peek().is_some_and(|&(_, p)| p == '\u{17D2}' || p == '\u{200C}' || p == '\u{200D}');
        if !attached && (is_base(c) || !is_khmer_or_joiner(c)) {
            return i;
        }
    }
    0
}

impl Decoder {
    /// `alphabet[0]` must be the blank (`""`); the rest are the recognizer's
    /// output symbols (code points, or units such as `"្ត"`).
    pub fn new<S: AsRef<str>>(alphabet: &[S]) -> Result<Self, Error> {
        if alphabet.len() < 2
            || !alphabet[0].as_ref().is_empty()
            || alphabet[1..].iter().any(|s| s.as_ref().is_empty())
        {
            return Err(Error::BadAlphabet);
        }
        Ok(Decoder {
            symbols: alphabet.iter().map(|s| s.as_ref().to_string()).collect(),
            user: UserWords::new(),
        })
    }

    /// Adds words (names, company terms) the re-ranking treats as known.
    pub fn with_user_words<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, words: I) -> Self {
        for w in words {
            self.user.insert(Dictionary::bundled(), w.as_ref());
        }
        self
    }

    fn check_frames(&self, frames: &[Vec<f32>]) {
        assert!(
            frames.iter().all(|f| f.len() == self.symbols.len()),
            "every frame must have one log-probability per alphabet symbol ({})",
            self.symbols.len()
        );
    }

    /// Most likely symbol per step, repeats collapsed, blanks removed (no
    /// grammar, no dictionary): the baseline the beam search must beat.
    ///
    /// # Panics
    ///
    /// If a frame's length differs from the alphabet's.
    pub fn greedy(&self, frames: &[Vec<f32>]) -> String {
        self.check_frames(frames);
        let mut out = String::new();
        let mut prev = 0;
        for f in frames {
            let k = f
                .iter()
                .map(|&v| clean(v))
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(0, |(k, _)| k);
            if k != 0 && k != prev {
                out.push_str(&self.symbols[k]);
            }
            prev = k;
        }
        out
    }

    /// CTC prefix beam search with the grammar constraint, then dictionary
    /// re-ranking. `frames[t][k]` is the natural-log probability of symbol `k`
    /// (0 = blank) at step `t`; NaN counts as impossible. Returns between one
    /// and `n_best` candidates, best first; the result is deterministic. If the
    /// grammar rules out every symbol path, the best candidate is the empty text.
    /// +inf counts as log-probability 0. A reading that ends in a coeng or
    /// joiner is never returned (it would not be valid Khmer).
    ///
    /// # Panics
    ///
    /// If a frame's length differs from the alphabet's.
    pub fn decode(&self, frames: &[Vec<f32>], opt: &Options) -> Vec<Candidate> {
        self.check_frames(frames);
        // Prefixes live once in a tree: a node is its parent's text plus one
        // symbol. The grammar check runs once per node, and beams carry node ids
        // instead of copying the text each step. After each step only the
        // surviving beams' nodes are kept (ids renumbered in rank order), so
        // memory stays bounded by the beam, not by every prefix ever tried.
        struct Node {
            text: String,
            valid: bool,
        }
        let mut nodes = vec![Node { text: String::new(), valid: true }];
        // (prefix node, last symbol) -> (log p ending in blank, log p ending in
        // the last symbol). Keyed by the last symbol too: with units such as
        // "្ត" next to "្" and "ត", different symbol paths can spell the same
        // text, and each needs its own repeat rule. A ranked list, not a map:
        // summing in a fixed order keeps the float result identical run to run.
        type Beam = HashMap<(usize, usize), (f32, f32)>;
        const NONE: (f32, f32) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut beams: Vec<((usize, usize), (f32, f32))> = vec![((0, 0), (0.0, f32::NEG_INFINITY))];
        // (parent, symbol) -> child, so a prefix reached by different beams (or
        // on different steps) is one node and their probabilities add up.
        let mut children: HashMap<(usize, usize), usize> = HashMap::new();
        // Survivors plus their children fit well under this; memory stays
        // bounded by the beam, not by every prefix ever tried.
        let compact_at = (opt.beam.max(1) * self.symbols.len() * 4).max(1024);
        for f in frames {
            let mut child = |nodes: &mut Vec<Node>, parent: usize, k: usize| -> Option<usize> {
                let id = *children.entry((parent, k)).or_insert_with(|| {
                    let text = format!("{}{}", nodes[parent].text, self.symbols[k]);
                    let valid = !opt.grammar || tail_is_valid(&text);
                    nodes.push(Node { text, valid });
                    nodes.len() - 1
                });
                nodes[id].valid.then_some(id)
            };
            // Symbols far below this step's best can't change the beam.
            let best = f.iter().map(|&v| clean(v)).fold(f32::NEG_INFINITY, f32::max);
            let floor = best - opt.prune;
            let mut next: Beam = HashMap::with_capacity(beams.len() * 4);
            for &((node, last), (pb, pnb)) in &beams {
                let total = log_add(pb, pnb);
                // Blank: the prefix stays.
                let e = next.entry((node, last)).or_insert(NONE);
                e.0 = log_add(e.0, total + clean(f[0]));
                for (k, lp) in f.iter().map(|&v| clean(v)).enumerate().skip(1) {
                    if lp < floor || lp == f32::NEG_INFINITY {
                        continue;
                    }
                    if k == last {
                        // Repeat without a blank between: same prefix.
                        let e = next.entry((node, last)).or_insert(NONE);
                        e.1 = log_add(e.1, pnb + lp);
                        // Repeat after a blank: the symbol is written again.
                        if let Some(ext) = child(&mut nodes, node, k) {
                            let e = next.entry((ext, k)).or_insert(NONE);
                            e.1 = log_add(e.1, pb + lp);
                        }
                    } else if let Some(ext) = child(&mut nodes, node, k) {
                        let e = next.entry((ext, k)).or_insert(NONE);
                        e.1 = log_add(e.1, total + lp);
                    }
                }
            }
            let mut ranked: Vec<((usize, usize), (f32, f32))> = next.into_iter().collect();
            ranked.sort_by(|a, b| {
                log_add(b.1.0, b.1.1)
                    .total_cmp(&log_add(a.1.0, a.1.1))
                    .then_with(|| nodes[a.0.0].text.cmp(&nodes[b.0.0].text))
                    .then_with(|| a.0.1.cmp(&b.0.1))
                    // Same text, same last symbol, different symbol paths:
                    // node ids are assigned in a fixed order.
                    .then_with(|| a.0.0.cmp(&b.0.0))
            });
            ranked.truncate(opt.beam.max(1));
            // Once the tree outgrows the beam, keep only the survivors' nodes,
            // renumbered in rank order. Not every step: rebuilding the maps
            // each step cost about 25% of decoding time.
            if nodes.len() > compact_at {
                let mut kept: Vec<Node> = Vec::with_capacity(ranked.len());
                let mut new_id: HashMap<usize, usize> = HashMap::new();
                for (key, _) in &mut ranked {
                    key.0 = *new_id.entry(key.0).or_insert_with(|| {
                        kept.push(std::mem::replace(
                            &mut nodes[key.0],
                            Node { text: String::new(), valid: false },
                        ));
                        kept.len() - 1
                    });
                }
                // Also keep the survivors' children: the next step tries them again,
                // and rebuilding each one (text plus grammar check) every step made
                // decoding about 2x slower. Bounded by beam x alphabet nodes.
                // Sorted so node ids (a ranking tie-break) are the same every run.
                let mut extra: Vec<(usize, usize, usize)> = children
                    .iter()
                    .filter_map(|(&(p, k), &c)| {
                        let p = *new_id.get(&p)?;
                        (!new_id.contains_key(&c)).then_some((p, k, c))
                    })
                    .collect();
                extra.sort_unstable();
                for (_, _, c) in extra {
                    kept.push(std::mem::replace(&mut nodes[c], Node { text: String::new(), valid: false }));
                    new_id.insert(c, kept.len() - 1);
                }
                nodes = kept;
                children = children
                    .into_iter()
                    .filter_map(|((p, k), c)| Some(((*new_id.get(&p)?, k), *new_id.get(&c)?)))
                    .collect();
            }
            beams = ranked;
        }
        let beams = beams.into_iter().map(|((node, _), p)| (nodes[node].text.clone(), p));

        let dict = Dictionary::bundled().with_user_words(&self.user);
        let mut merged: HashMap<String, f32> = HashMap::new();
        for (prefix, (pb, pnb)) in beams {
            // Different symbol orders can normalize to the same text: merge them.
            let text = normalize(&prefix);
            let e = merged.entry(text).or_insert(f32::NEG_INFINITY);
            *e = log_add(*e, log_add(pb, pnb));
        }
        let mut out: Vec<Candidate> = merged
            .into_iter()
            // A trailing coeng or joiner is half a cluster: not valid Khmer as
            // final text (a longer reading can complete it, a final one can't).
            .filter(|(text, _)| !text.ends_with(['\u{17D2}', '\u{200C}', '\u{200D}']))
            .filter(|(text, ctc)| ctc.is_finite() || text.is_empty())
            .map(|(text, ctc)| {
                let unknown = if opt.unknown_cluster_penalty > 0.0 {
                    text.split_whitespace()
                        .filter(|t| !t.chars().any(|c| c.is_ascii_alphanumeric() || ('០'..='៩').contains(&c)))
                        .map(|t| dict.unknown_clusters(t))
                        .sum()
                } else {
                    0
                };
                let score = ctc - opt.unknown_cluster_penalty * unknown as f32;
                Candidate { text, ctc, unknown_clusters: unknown, score }
            })
            .collect();
        // Never none: the empty text is the fallback when nothing else survives.
        if out.is_empty() {
            out.push(Candidate {
                text: String::new(),
                ctc: f32::NEG_INFINITY,
                unknown_clusters: 0,
                score: f32::NEG_INFINITY,
            });
        }
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.text.cmp(&b.text)));
        out.truncate(opt.n_best.max(1));
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn confidence_is_per_character_and_bounded() {
        let c = |text: &str, ctc: f32| Candidate { text: text.into(), ctc, unknown_clusters: 0, score: ctc };
        // Same probability per character: same confidence, whatever the length.
        let short = c("ក", 0.5f32.ln());
        let long = c("កក ក", 3.0 * 0.5f32.ln());
        assert!((short.confidence() - 0.5).abs() < 1e-5);
        assert!((long.confidence() - 0.5).abs() < 1e-5);
        assert_eq!(c("", f32::NEG_INFINITY).confidence(), 0.0);
        assert_eq!(c("ក", f32::NAN).confidence(), 0.0);
        assert_eq!(c("ក", 0.0).confidence(), 1.0);
    }

    use super::*;

    const BLANK: usize = 0;

    /// Frames from per-step distributions over `alphabet` (missing mass to blank).
    fn frames(alphabet: &[&str], steps: &[&[(&str, f32)]]) -> Vec<Vec<f32>> {
        steps
            .iter()
            .map(|s| {
                let mut p = vec![1e-6f32; alphabet.len()];
                let mut used = 0.0;
                for (sym, prob) in s.iter() {
                    let k = alphabet.iter().position(|a| a == sym).unwrap();
                    p[k] = *prob;
                    used += prob;
                }
                p[BLANK] = (1.0 - used).max(1e-6);
                p.iter().map(|x| x.ln()).collect()
            })
            .collect()
    }

    #[test]
    fn a_unit_symbol_next_to_its_parts_decodes_the_same_every_time() {
        // "្ត" is one symbol and also "្" + "ត": both paths spell the same text.
        let a = ["", "ក", "្", "ត", "្ត"];
        let d = Decoder::new(&a).unwrap();
        let f =
            frames(&a, &[&[("ក", 0.95)], &[("្ត", 0.5), ("្", 0.45)], &[("្ត", 0.5), ("ត", 0.45)], &[("្ត", 0.9)]]);
        let first = d.decode(&f, &Options::default());
        for _ in 0..50 {
            assert_eq!(d.decode(&f, &Options::default()), first);
        }
        assert_eq!(first[0].text, "ក្ត");
    }

    #[test]
    fn nan_is_impossible_not_best() {
        let a = ["", "ក", "ា"];
        let d = Decoder::new(&a).unwrap();
        let mut f = frames(&a, &[&[("ក", 0.9)], &[("ា", 0.9)]]);
        f[1][2] = f32::NAN;
        let best = &d.decode(&f, &Options::default())[0];
        assert!(best.ctc.is_finite(), "{best:?}");
        assert_eq!(best.text, "ក");
        assert_eq!(d.greedy(&f), "ក");
    }

    #[test]
    #[should_panic(expected = "one log-probability per alphabet symbol")]
    fn a_frame_of_the_wrong_length_is_a_clear_panic() {
        let d = Decoder::new(&["", "ក"]).unwrap();
        d.decode(&[vec![0.0]], &Options::default());
    }

    #[test]
    fn the_tail_check_starts_at_a_real_syllable() {
        // A joiner 64 bytes from the end must not make the mark after it look broken.
        let base = "ក".repeat(20); // 60 bytes
        let text = format!("{base}ឳ\u{200C}ឹ");
        assert!(syllables(&text).iter().all(|s| s.kind != SyllableKind::Broken));
        assert!(tail_is_valid(&text));
        // A broken syllable long after the window start is still caught.
        let broken = format!("កា{}ុ", "ំ".repeat(22));
        assert_eq!(tail_is_valid(&broken), syllables(&broken).iter().all(|s| s.kind != SyllableKind::Broken));
    }

    #[test]
    fn collapses_repeats_and_keeps_doubles_split_by_blank() {
        let a = ["", "ក", "ា"];
        let d = Decoder::new(&a).unwrap();
        let f = frames(&a, &[&[("ក", 0.9)], &[("ក", 0.9)], &[], &[("ក", 0.9)], &[("ា", 0.9)]]);
        assert_eq!(d.greedy(&f), "កកា");
        let o = Options { unknown_cluster_penalty: 0.0, ..Options::default() };
        assert_eq!(d.decode(&f, &o)[0].text, "កកា");
    }

    #[test]
    fn grammar_removes_dotted_circle_readings() {
        // The most likely path starts with a vowel: broken text.
        let a = ["", "ក", "ា", "ិ"];
        let d = Decoder::new(&a).unwrap();
        let f = frames(&a, &[&[("ា", 0.6), ("ក", 0.35)], &[("ិ", 0.6), ("ក", 0.3)]]);
        let greedy = d.greedy(&f);
        assert!(aksar::syllable::validate(&greedy).is_err(), "{greedy}");
        let o = Options { unknown_cluster_penalty: 0.0, ..Options::default() };
        let best = &d.decode(&f, &o)[0];
        assert!(aksar::syllable::validate(&best.text).is_ok(), "{}", best.text);
        assert_eq!(best.text, "កិ");
        // Without the grammar the broken reading wins.
        let off = Options { grammar: false, unknown_cluster_penalty: 0.0, ..Options::default() };
        assert!(aksar::syllable::validate(&d.decode(&f, &off)[0].text).is_err());
    }

    #[test]
    fn dictionary_breaks_close_calls() {
        // "រដូវ" (season) vs the misreading "រដួវ": the vowel is a near tie.
        let a = ["", "រ", "ដ", "ូ", "ួ", "វ"];
        let d = Decoder::new(&a).unwrap();
        let f = frames(&a, &[&[("រ", 0.95)], &[("ដ", 0.95)], &[("ួ", 0.5), ("ូ", 0.45)], &[("វ", 0.95)]]);
        let plain = Options { unknown_cluster_penalty: 0.0, ..Options::default() };
        assert_eq!(d.decode(&f, &plain)[0].text, "រដួវ");
        let best = &d.decode(&f, &Options::default())[0];
        assert_eq!((best.text.as_str(), best.unknown_clusters), ("រដូវ", 0));
        // A clear reading is not overridden by the dictionary.
        let f = frames(&a, &[&[("រ", 0.95)], &[("ដ", 0.95)], &[("ួ", 0.97), ("ូ", 0.01)], &[("វ", 0.95)]]);
        assert_eq!(
            d.decode(&f, &Options { unknown_cluster_penalty: 0.5, ..Options::default() })[0].text,
            "រដួវ"
        );
    }

    #[test]
    fn user_words_count_as_known() {
        let a = ["", "ស", "ុ", "ខ", "ា"];
        let d = Decoder::new(&a).unwrap().with_user_words(["សុខា"]);
        let f = frames(&a, &[&[("ស", 0.9)], &[("ុ", 0.9)], &[("ខ", 0.9)], &[("ា", 0.9)]]);
        assert_eq!(d.decode(&f, &Options::default())[0].unknown_clusters, 0);
    }

    #[test]
    fn random_recognizer_output_always_decodes_to_valid_khmer() {
        // Alphabet: blank, every Khmer letter, vowel, sign and the coeng.
        let mut alphabet = vec![String::new()];
        alphabet.extend((0x1780u32..=0x17D2).filter_map(char::from_u32).map(String::from));
        let d = Decoder::new(&alphabet).unwrap();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };
        let o = Options { beam: 8, unknown_cluster_penalty: 0.0, ..Options::default() };
        for _ in 0..200 {
            let steps = 4 + (rnd() * 8.0) as usize;
            let f: Vec<Vec<f32>> = (0..steps)
                .map(|_| {
                    let raw: Vec<f32> = (0..alphabet.len()).map(|_| rnd().powi(6) + 1e-6).collect();
                    let sum: f32 = raw.iter().sum();
                    raw.iter().map(|x| (x / sum).ln()).collect()
                })
                .collect();
            for c in d.decode(&f, &o) {
                assert!(aksar::syllable::issues(&c.text).iter().all(|i| !i.kind.is_error()), "{:?}", c.text);
            }
        }
    }

    /// Real recognizers can be unsure of everything: a best symbol at log-prob
    /// -35 is a weak reading, not a reason to read nothing.
    #[test]
    fn a_line_the_model_is_unsure_of_still_decodes() {
        let a = ["", "ក", "ា"];
        let d = Decoder::new(&a).unwrap();
        let f = vec![vec![-50.0, -35.0, -45.0], vec![-50.0, -45.0, -35.0]];
        let best = &d.decode(&f, &Options::default())[0];
        assert_eq!(best.text, "កា", "a fixed -30 floor skipped the whole alphabet");
    }

    /// A trailing coeng (or joiner) is half a cluster: it must never be the
    /// end of the text, or the output is not valid Khmer.
    #[test]
    fn text_never_ends_in_a_dangling_coeng_or_joiner() {
        let a = ["", "ក", "្", "\u{200D}"];
        let d = Decoder::new(&a).unwrap();
        for tail in ["្", "\u{200D}"] {
            let f = frames(&a, &[&[("ក", 0.9)], &[(tail, 0.9)]]);
            let o = Options { unknown_cluster_penalty: 0.0, ..Options::default() };
            let got = d.decode(&f, &o);
            assert!(!got.is_empty());
            for c in &got {
                assert!(!c.text.ends_with(['\u{17D2}', '\u{200C}', '\u{200D}']), "{:?}", c.text);
            }
            assert_eq!(got[0].text, "ក");
        }
        // Nothing but a coeng: the empty text is the fallback, not an empty list.
        let f = frames(&a, &[&[("្", 0.99)]]);
        let got = d.decode(&f, &Options { beam: 1, ..Options::default() });
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "");
    }

    /// The contract is 1..=n_best candidates; +inf (a broken model) used to
    /// leave none.
    #[test]
    fn plus_infinity_does_not_empty_the_result() {
        let a = ["", "ក"];
        let d = Decoder::new(&a).unwrap();
        let f = vec![vec![-1.0, f32::INFINITY]];
        let got = d.decode(&f, &Options { beam: 1, ..Options::default() });
        assert!(!got.is_empty(), "contract is at least one candidate");
        assert!(got.iter().all(|c| !c.ctc.is_nan()));
    }

    /// Blank text is not "sure": confidence of nothing is 0, not 1.
    #[test]
    fn empty_text_has_no_confidence() {
        let c = |text: &str| Candidate { text: text.into(), ctc: 0.0, unknown_clusters: 0, score: 0.0 };
        assert_eq!(c("").confidence(), 0.0);
        assert_eq!(c(" \n").confidence(), 0.0);
        assert_eq!(c("ក").confidence(), 1.0);
    }

    /// Summing log-probabilities in hash order can change the last float bits
    /// from run to run, and equal scores must not be ordered by chance: the
    /// same input must give the same bits every time. Unit symbols make many
    /// symbol paths spell the same text, which is where the sums and ties are.
    #[test]
    fn decoding_is_bit_for_bit_repeatable() {
        let a = ["", "ក", "្", "ត", "្ត", "ក្", "ក្ត", "ា"];
        let d = Decoder::new(&a).unwrap();
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };
        let o = Options { beam: 64, unknown_cluster_penalty: 0.0, grammar: false, ..Options::default() };
        let bits = |v: Vec<Candidate>| v.into_iter().map(|c| (c.text, c.ctc.to_bits())).collect::<Vec<_>>();
        for _ in 0..10 {
            let f: Vec<Vec<f32>> = (0..10)
                .map(|_| {
                    let raw: Vec<f32> = (0..a.len()).map(|_| rnd() + 0.05).collect();
                    let sum: f32 = raw.iter().sum();
                    raw.iter().map(|x| (x / sum).ln()).collect()
                })
                .collect();
            let first = bits(d.decode(&f, &o));
            for _ in 0..5 {
                assert_eq!(bits(d.decode(&f, &o)), first);
            }
        }
    }

    /// Paths that reach the same prefix on different steps must add up: the
    /// text "ក" over 3 steps of {ក: 0.5, blank: 0.5} has 6 of the 8 paths
    /// (ក__, _ក_, __ក, កក_, _កក, កកក; ក_ក spells "កក").
    #[test]
    fn paths_to_the_same_prefix_on_different_steps_are_summed() {
        let a = ["", "ក"];
        let d = Decoder::new(&a).unwrap();
        let f = vec![vec![0.5f32.ln(); 2]; 3];
        let o = Options { unknown_cluster_penalty: 0.0, ..Options::default() };
        let got = d.decode(&f, &o);
        let ក = got.iter().find(|c| c.text == "ក").unwrap();
        assert!((ក.ctc.exp() - 0.75).abs() < 1e-5, "{}", ក.ctc.exp());
    }

    #[test]
    fn rejects_bad_alphabets() {
        assert_eq!(Decoder::new(&["ក", ""]).err(), Some(Error::BadAlphabet));
        assert_eq!(Decoder::new(&[""]).err(), Some(Error::BadAlphabet));
    }
}
