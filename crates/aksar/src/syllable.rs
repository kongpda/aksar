//! Khmer syllable segmentation, validation and normalization.
//!
//! # What "valid" means here
//!
//! [`validate`] reports an [`IssueKind::is_error`] issue exactly when an
//! OpenType Khmer shaper (HarfBuzz / harfrust, matching Uniscribe) would treat
//! part of the text as a *broken cluster* and draw a dotted circle (◌). The
//! grammar mirrors HarfBuzz's `khmer_machine.rl`, and the test suite checks
//! the two agree on thousands of random sequences.
//!
//! On top of that, it reports *warnings*: text that renders but is very likely
//! a typing or OCR mistake (split vowels, deprecated characters, a dangling
//! coeng, unusually deep stacks).
//!
//! # Normalization
//!
//! [`normalize`] reorders the marks of each syllable into the NiDA / Unicode
//! storage order and fixes a small set of unambiguous mistakes. It never
//! changes which letters are present (except composing split vowels and
//! replacing deprecated independent vowels with their recommended spelling),
//! so it is safe to run on user input and OCR output.
//!
//! Storage order after normalization:
//!
//! 1. base consonant or independent vowel
//! 2. subscripts (coeng + consonant) and register shifters / robat, in typed
//!    order, with coeng-RO moved after the other subscripts
//! 3. dependent vowel
//! 4. above/other signs (nikahit, bantoc, ...)
//! 5. final signs (reahmuk, yuukaleapintu, atthacan)
//!
//! Shifters and robat are not moved across subscripts: shapers accept them
//! on the base or on a subscript, and moving them can attach them to a
//! different letter. One typed right after coeng-RO moves with it.
//!
//! It deliberately does **not** touch the coeng-TA / coeng-DA ambiguity
//! (្ត vs ្ដ look the same in most fonts); which one is correct depends on the
//! word, so that belongs in a dictionary-driven correction layer.

use crate::chars::{self, COENG};
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

/// Category of a character as seen by the OpenType Khmer syllable grammar.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Cat {
    /// Consonant or independent vowel (C | Ra | V in HarfBuzz).
    Base,
    /// NBSP, dotted circle, ASCII digits, hyphens, etc.: may carry marks without a dotted circle.
    Placeholder,
    /// Coeng.
    H,
    /// Register shifters and robat.
    Robatic,
    VPre,
    VBlw,
    VAbv,
    VPst,
    /// Above / other signs.
    X,
    /// Final visarga-like signs.
    Y,
    Joiner,
    Other,
}

/// Category of a single character. Split vowels are handled by [`cats_of`].
fn cat(c: char) -> Cat {
    match c as u32 {
        0x1780..=0x17B3 => Cat::Base,
        // HarfBuzz's Indic table (shared by Khmer): hyphen-minus, ASCII digits,
        // NBSP, multiplication sign, Khmer digits and phnaek muan, dashes, bullet, dotted circle, boxes.
        0x002D
        | 0x0030..=0x0039
        | 0x00A0
        | 0x00D7
        | 0x17D9
        | 0x17E0..=0x17E9
        | 0x2010..=0x2015
        | 0x2022
        | 0x25CC
        | 0x25FB..=0x25FE => Cat::Placeholder,
        0x17D2 => Cat::H,
        0x17C9 | 0x17CA | 0x17CC => Cat::Robatic,
        0x17C1..=0x17C3 => Cat::VPre,
        0x17BB..=0x17BD => Cat::VBlw,
        0x17B7..=0x17BA | 0x17BE => Cat::VAbv,
        0x17B6 | 0x17BF | 0x17C0 | 0x17C4 | 0x17C5 => Cat::VPst,
        0x17C6 | 0x17CB | 0x17CD..=0x17D1 => Cat::X,
        0x17C7 | 0x17C8 | 0x17D3 | 0x17DD => Cat::Y,
        0x200C | 0x200D => Cat::Joiner,
        _ => Cat::Other,
    }
}

/// Grammar categories for one character. Split vowels (ើ ឿ ៀ ោ ៅ) are
/// decomposed by the shaper into a pre-base part (U+17C1) plus the rest:
/// ើ is pre + above; ឿ ៀ ោ ៅ are pre + post.
fn cats_of(c: char, out: &mut Vec<Cat>) {
    match c as u32 {
        0x17BE => out.extend([Cat::VPre, Cat::VAbv]),
        0x17BF | 0x17C0 | 0x17C4 | 0x17C5 => out.extend([Cat::VPre, Cat::VPst]),
        _ => out.push(cat(c)),
    }
}

fn is_c(c: Option<&Cat>) -> bool {
    matches!(c, Some(Cat::Base))
}

/// Matches `cn = c ((ZWJ|ZWNJ)? Robatic)?` after the `c`; returns the new index.
fn opt_robatic(cats: &[Cat], mut j: usize) -> usize {
    if cats.get(j) == Some(&Cat::Joiner) && cats.get(j + 1) == Some(&Cat::Robatic) {
        j += 2;
    } else if cats.get(j) == Some(&Cat::Robatic) {
        j += 1;
    }
    j
}

/// Matches `xgroup = (joiner* Xgroup)*`.
fn xgroup(cats: &[Cat], mut j: usize) -> usize {
    loop {
        let mut k = j;
        while cats.get(k) == Some(&Cat::Joiner) {
            k += 1;
        }
        if cats.get(k) == Some(&Cat::X) {
            j = k + 1;
        } else {
            return j;
        }
    }
}

/// Matches the part of `broken_cluster` that follows the base, starting at `j`.
///
/// `broken_cluster = Robatic? (H cn)* (H | syllable_tail)`
fn after_base(cats: &[Cat], mut j: usize) -> usize {
    if cats.get(j) == Some(&Cat::Robatic) {
        j += 1;
    }
    while cats.get(j) == Some(&Cat::H) && is_c(cats.get(j + 1)) {
        j = opt_robatic(cats, j + 2);
    }
    if cats.get(j) == Some(&Cat::H) {
        return j + 1;
    }
    // syllable_tail = xgroup matra_group xgroup (H c)? ygroup
    // matra_group  = VPre? xgroup VBlw? xgroup (joiner? VAbv)? xgroup VPst?
    j = xgroup(cats, j);
    if cats.get(j) == Some(&Cat::VPre) {
        j += 1;
    }
    j = xgroup(cats, j);
    if cats.get(j) == Some(&Cat::VBlw) {
        j += 1;
    }
    j = xgroup(cats, j);
    if cats.get(j) == Some(&Cat::VAbv) {
        j += 1;
    } else if cats.get(j) == Some(&Cat::Joiner) && cats.get(j + 1) == Some(&Cat::VAbv) {
        j += 2;
    }
    j = xgroup(cats, j);
    if cats.get(j) == Some(&Cat::VPst) {
        j += 1;
    }
    j = xgroup(cats, j);
    if cats.get(j) == Some(&Cat::H) && is_c(cats.get(j + 1)) {
        j += 2;
    }
    while cats.get(j) == Some(&Cat::Y) {
        j += 1;
    }
    j
}

/// Length (in categories) of the longest Khmer cluster starting at `i`.
/// Returns `(len, broken)`; `len == 0` means "not a Khmer cluster".
fn match_at(cats: &[Cat], i: usize) -> (usize, bool) {
    match cats[i] {
        Cat::Base => {
            let j = opt_robatic(cats, i + 1);
            (after_base(cats, j) - i, false)
        }
        Cat::Placeholder => (after_base(cats, i + 1) - i, false),
        Cat::Other | Cat::Joiner => (0, false),
        // A mark with no base: a broken cluster.
        _ => (after_base(cats, i).max(i + 1) - i, true),
    }
}

/// One segment of text: a Khmer syllable, a broken cluster, or a single
/// non-Khmer character.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Syllable {
    /// Byte range in the original string.
    pub range: Range<usize>,
    /// What kind of segment this is.
    pub kind: SyllableKind,
}

/// Kind of a [`Syllable`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SyllableKind {
    /// A well-formed Khmer syllable (base plus marks).
    Valid,
    /// Marks without a base. Shapers draw a dotted circle here.
    Broken,
    /// A character that is not part of a Khmer cluster.
    Other,
}

/// Splits text into syllables using the OpenType Khmer grammar.
pub fn syllables(text: &str) -> Vec<Syllable> {
    // Map each grammar category back to the byte offset of its character.
    let mut cats = Vec::with_capacity(text.len() / 2);
    let mut offs = Vec::with_capacity(text.len() / 2);
    for (o, c) in text.char_indices() {
        let before = cats.len();
        cats_of(c, &mut cats);
        offs.extend(core::iter::repeat_n(o, cats.len() - before));
    }
    offs.push(text.len());

    let mut out = Vec::new();
    let mut i = 0;
    while i < cats.len() {
        let (len, broken) = match_at(&cats, i);
        let (len, kind) = match len {
            0 => (1, SyllableKind::Other),
            n if broken => (n, SyllableKind::Broken),
            n => (n, SyllableKind::Valid),
        };
        let mut end = i + len;
        // Never split a decomposed split vowel across two segments.
        while end < cats.len() && offs[end] == offs[end - 1] {
            end += 1;
        }
        out.push(Syllable { range: offs[i]..offs[end], kind });
        i = end;
    }
    out
}

/// Splits text into the units words, search tokens and spacing work with: each
/// Khmer syllable (base plus its subscripts, vowel and signs), each broken
/// cluster, and each other character on its own. Concatenating the pieces gives
/// back `text`.
///
/// ```
/// assert_eq!(aksar::syllable::clusters("កម្ពុជា"), ["ក", "ម្ពុ", "ជា"]);
/// ```
pub fn clusters(text: &str) -> Vec<&str> {
    syllables(text).into_iter().map(|s| &text[s.range]).collect()
}

/// A problem found by [`validate`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Issue {
    /// Byte range in the input the issue refers to (`usize`, so no 4 GiB limit).
    pub range: Range<usize>,
    /// What is wrong.
    pub kind: IssueKind,
}

/// Kinds of [`Issue`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum IssueKind {
    /// Error: marks with no base, or in an order the shaper cannot attach.
    /// Renders with a dotted circle (◌).
    BrokenCluster,
    /// Warning: a deprecated or discouraged character (e.g. U+17A3, U+17B4).
    Deprecated(char),
    /// Warning: a vowel typed as two parts, e.g. េ + ា instead of ោ.
    SplitVowel {
        /// The single code point that should be used instead.
        expected: char,
    },
    /// Warning: a coeng at the end of a syllable (no subscript follows).
    DanglingCoeng,
    /// Warning: more than two subscripts in one syllable (very rare in real text).
    DeepStack(usize),
    /// Warning: a subscript typed after the vowel (e.g. ក ា ្ រ). Shapers
    /// accept it, but it is not NiDA order and breaks search and sorting.
    SubscriptAfterVowel,
}

impl IssueKind {
    /// True for issues that change rendering (dotted circles).
    pub fn is_error(&self) -> bool {
        matches!(self, IssueKind::BrokenCluster)
    }

    /// Stable kebab-case name of this kind, the one `aksar check --json` and
    /// the Python and WASM `issues` use: `broken-cluster`, `deprecated`,
    /// `split-vowel`, `dangling-coeng`, `deep-stack`, `subscript-after-vowel`.
    /// Names never change once published; new kinds get new names.
    pub fn name(&self) -> &'static str {
        match self {
            IssueKind::BrokenCluster => "broken-cluster",
            IssueKind::Deprecated(_) => "deprecated",
            IssueKind::SplitVowel { .. } => "split-vowel",
            IssueKind::DanglingCoeng => "dangling-coeng",
            IssueKind::DeepStack(_) => "deep-stack",
            IssueKind::SubscriptAfterVowel => "subscript-after-vowel",
        }
    }
}

/// A short English description, e.g. `split vowel: use ោ`.
impl core::fmt::Display for IssueKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IssueKind::BrokenCluster => f.write_str("broken cluster: renders with a dotted circle"),
            IssueKind::Deprecated(c) => write!(f, "deprecated character U+{:04X}", u32::from(*c)),
            IssueKind::SplitVowel { expected } => write!(f, "split vowel: use {expected}"),
            IssueKind::DanglingCoeng => f.write_str("coeng with no subscript after it"),
            IssueKind::DeepStack(n) => write!(f, "{n} subscripts in one syllable"),
            IssueKind::SubscriptAfterVowel => f.write_str("subscript typed after the vowel"),
        }
    }
}

/// `error` or `warning`, the byte range, then the kind, e.g.
/// `warning at 0..9: split vowel: use ោ`.
impl core::fmt::Display for Issue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let level = if self.kind.is_error() { "error" } else { "warning" };
        write!(f, "{level} at {}..{}: {}", self.range.start, self.range.end, self.kind)
    }
}

const SPLIT_VOWELS: [(char, char, char); 2] = [
    ('\u{17C1}', '\u{17B6}', '\u{17C4}'), // េ + ា → ោ
    ('\u{17C1}', '\u{17B8}', '\u{17BE}'), // េ + ី → ើ
];

/// Every issue in `text`, errors and warnings, in text order.
pub fn issues(text: &str) -> Vec<Issue> {
    let mut out = Vec::new();
    for s in syllables(text) {
        let seg = &text[s.range.clone()];
        let at = |r: Range<usize>| (s.range.start + r.start)..(s.range.start + r.end);
        if s.kind == SyllableKind::Broken {
            out.push(Issue { range: s.range.clone(), kind: IssueKind::BrokenCluster });
        }
        let chars: Vec<(usize, char)> = seg.char_indices().collect();
        let mut subs = 0;
        let mut seen_vowel = false;
        for (n, &(o, c)) in chars.iter().enumerate() {
            if chars::class(c) == chars::Class::DependentVowel {
                seen_vowel = true;
            }
            let r = o..o + c.len_utf8();
            if chars::class(c) == chars::Class::Deprecated {
                out.push(Issue { range: at(r.clone()), kind: IssueKind::Deprecated(c) });
            }
            if c == COENG {
                match chars.get(n + 1) {
                    Some(&(_, next)) if cat(next) == Cat::Base => {
                        subs += 1;
                        if seen_vowel {
                            out.push(Issue {
                                range: at(r.start..r.end + next.len_utf8()),
                                kind: IssueKind::SubscriptAfterVowel,
                            });
                        }
                    }
                    _ if s.kind == SyllableKind::Valid => {
                        out.push(Issue { range: at(r.clone()), kind: IssueKind::DanglingCoeng })
                    }
                    _ => {}
                }
            }
            if let Some(&(o2, c2)) = chars.get(n + 1) {
                if let Some(&(_, _, expected)) = SPLIT_VOWELS.iter().find(|(a, b, _)| *a == c && *b == c2) {
                    out.push(Issue {
                        range: at(o..o2 + c2.len_utf8()),
                        kind: IssueKind::SplitVowel { expected },
                    });
                }
            }
        }
        if subs > 2 {
            out.push(Issue { range: s.range.clone(), kind: IssueKind::DeepStack(subs) });
        }
    }
    out
}

/// Checks `text`. `Ok(())` means no errors (warnings are allowed);
/// otherwise returns all issues, errors and warnings.
pub fn validate(text: &str) -> Result<(), Vec<Issue>> {
    let all = issues(text);
    if all.iter().any(|i| i.kind.is_error()) { Err(all) } else { Ok(()) }
}

/// Sort key for a mark inside a syllable (see module docs for the order).
fn mark_rank(c: char, prev_was_coeng: bool) -> u8 {
    if prev_was_coeng {
        return if c == '\u{179A}' { 3 } else { 2 };
    }
    match cat(c) {
        Cat::Robatic => 2,
        Cat::VPre | Cat::VBlw | Cat::VAbv | Cat::VPst => 5,
        Cat::X => 6,
        Cat::Y => 7,
        _ => 6,
    }
}

/// Reorders and repairs Khmer text into canonical storage order.
///
/// - Moves subscripts (coeng + consonant) before vowels and signs, with
///   coeng-RO last among subscripts.
/// - Moves register shifters and robat before vowels.
/// - Puts vowels before signs, and reahmuk / yuukaleapintu last.
/// - Composes split vowels (េ + ា → ោ, េ + ី → ើ).
/// - Replaces deprecated U+17A3 with អ and U+17A4 with អា.
///
/// The result is idempotent: `normalize(&normalize(s)) == normalize(s)`.
pub fn normalize(text: &str) -> String {
    /// A unit that moves as a whole: (rank, typed position, text).
    type Unit = (u8, usize, String);

    fn flush(out: &mut String, marks: &mut Vec<Unit>) {
        marks.sort_by_key(|m| (m.0, m.1));
        let mut buf: String = marks.drain(..).map(|m| m.2).collect();
        // Compose split vowels that ended up adjacent after sorting.
        for (a, b, to) in SPLIT_VOWELS {
            let pair: String = [a, b].iter().collect();
            if buf.contains(pair.as_str()) {
                buf = buf.replace(pair.as_str(), to.encode_utf8(&mut [0; 4]));
            }
        }
        out.push_str(&buf);
    }

    let mut out = String::with_capacity(text.len());
    let mut marks: Vec<Unit> = Vec::new();
    let mut in_syllable = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        let k = cat(c);
        if matches!(k, Cat::Base | Cat::Placeholder) || !(in_syllable && is_mark_cat(k)) {
            flush(&mut out, &mut marks);
            in_syllable = matches!(k, Cat::Base | Cat::Placeholder);
            match c {
                '\u{17A3}' => out.push('\u{17A2}'),
                // ឤ → អា, unless a vowel follows in this syllable: អា plus a second
                // vowel would be broken (ឤឿ renders, អាឿ doesn't).
                '\u{17A4}' if !vowel_follows(chars.clone()) => {
                    // The ា joins this syllable's marks, so shifters and other
                    // marks typed after ឤ still sort into place (ឤ៊ → អ៊ា).
                    out.push('\u{17A2}');
                    marks.push((mark_rank('\u{17B6}', false), 0, String::from('\u{17B6}')));
                }
                _ => out.push(c),
            }
            continue;
        }
        let seq = marks.len();
        // Gather a run of joiners; they travel with the mark that follows.
        let mut unit = String::new();
        unit.push(c);
        let mut last = c;
        while cat(last) == Cat::Joiner {
            match chars.peek().copied() {
                Some(n) if is_mark_cat(cat(n)) => {
                    chars.next();
                    unit.push(n);
                    last = n;
                }
                _ => break,
            }
        }
        match cat(last) {
            // coeng + consonant is one subscript unit.
            Cat::H => match chars.peek().copied() {
                Some(n) if cat(n) == Cat::Base && unit.len() == last.len_utf8() => {
                    chars.next();
                    unit.push(n);
                    marks.push((mark_rank(n, true), seq, unit));
                }
                // A coeng with nothing to stack (or joiners before it) stays
                // exactly where it is: moving it could stack a different letter.
                _ => {
                    flush(&mut out, &mut marks);
                    out.push_str(&unit);
                }
            },
            // Joiners not followed by a mark also stay in place.
            Cat::Joiner => {
                flush(&mut out, &mut marks);
                out.push_str(&unit);
            }
            // A shifter or robat typed right after coeng-RO belongs to that
            // subscript and moves with it (`ក្រ៉` stays, not `ក៉្រ`).
            Cat::Robatic if marks.last().is_some_and(|m| m.0 == 3) => marks.push((3, seq, unit)),
            _ => marks.push((mark_rank(last, false), seq, unit)),
        }
    }
    flush(&mut out, &mut marks);
    out
}

/// [`normalize`] plus cleanup for OCR output and search indexes (from PDFTape):
///
/// - removes the invisible deprecated vowels U+17B4 / U+17B5;
/// - expands deprecated U+17D8 (៘) to ។ល។;
/// - drops a dependent vowel or sign repeated right after itself in one
///   syllable (a common OCR stutter: កាា → កា).
///
/// Idempotent, and never turns text that renders into broken text.
pub fn repair(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{17B4}' | '\u{17B5}' => {}
            '\u{17D8}' => cleaned.push_str("\u{17D4}\u{179B}\u{17D4}"),
            _ => cleaned.push(c),
        }
    }
    let n = normalize(&cleaned);
    let mut out = String::with_capacity(n.len());
    let mut prev: Option<char> = None;
    for c in n.chars() {
        let stutter =
            prev == Some(c) && matches!(chars::class(c), chars::Class::DependentVowel | chars::Class::Sign);
        if !stutter {
            out.push(c);
        }
        prev = Some(c);
    }
    out
}

/// Whether a dependent vowel follows in the current syllable (subscripts,
/// shifters and signs are stepped over; the next base ends the syllable).
/// A coeng with no consonant right after it also counts: it is valid after ឤ
/// but not after the ា of អា.
fn vowel_follows(it: impl Iterator<Item = char>) -> bool {
    let mut it = it.peekable();
    while let Some(c) = it.next() {
        match cat(c) {
            Cat::VPre | Cat::VBlw | Cat::VAbv | Cat::VPst => return true,
            Cat::H => {
                if it.next_if(|&n| matches!(cat(n), Cat::Base)).is_none() {
                    return true;
                }
            }
            k if is_mark_cat(k) => {}
            _ => return false,
        }
    }
    false
}

fn is_mark_cat(k: Cat) -> bool {
    !matches!(k, Cat::Base | Cat::Placeholder | Cat::Other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    // The CLI (`check --json`), Python and WASM publish these names; changing
    // one breaks their users.
    #[test]
    fn issue_kind_names_are_stable() {
        let names = [
            (IssueKind::BrokenCluster, "broken-cluster"),
            (IssueKind::Deprecated('\u{17A3}'), "deprecated"),
            (IssueKind::SplitVowel { expected: '\u{17C4}' }, "split-vowel"),
            (IssueKind::DanglingCoeng, "dangling-coeng"),
            (IssueKind::DeepStack(3), "deep-stack"),
            (IssueKind::SubscriptAfterVowel, "subscript-after-vowel"),
        ];
        for (kind, name) in names {
            assert_eq!(kind.name(), name);
        }
    }

    fn kinds(s: &str) -> Vec<SyllableKind> {
        syllables(s).into_iter().map(|s| s.kind).collect()
    }

    // HarfBuzz draws no dotted circle after ASCII digits, hyphens or NBSP, so a
    // mark after them is valid and `normalize` must not reorder it into a
    // sequence that does render one.
    #[test]
    fn digits_and_hyphens_are_placeholder_bases() {
        for base in ['1', '9', '-', '\u{2010}', '\u{2011}', '\u{A0}', '\u{D7}'] {
            let s = alloc::format!("{base}\u{17B6}");
            assert!(validate(&s).is_ok(), "{base:?}");
        }
        let s = "\u{17E1}\u{17C1}\u{17C4}\u{17D2}\u{17A4}";
        assert_eq!(validate(&normalize(s)).is_ok(), validate(s).is_ok());
        for base in ['\u{17E1}', '\u{17D9}'] {
            assert!(validate(&alloc::format!("{base}\u{17B6}")).is_ok(), "{base:?}");
        }
    }

    #[test]
    fn nida_examples_are_valid() {
        for s in ["ក្ក", "គ្រា", "ខ្មែរ", "សួស្តី", "ស្ត្រី", "កម្ពុជា", "ព្រះរាជាណាចក្រ", "២០២៦"]
        {
            assert_eq!(validate(s), Ok(()), "{s}");
        }
    }

    #[test]
    fn segmentation() {
        assert_eq!(syllables("ខ្មែរ").len(), 2); // ខ្មែ + រ
        assert_eq!(kinds("ក a"), [SyllableKind::Valid, SyllableKind::Other, SyllableKind::Other]);
        assert_eq!(syllables("កើ")[0].range, 0..6); // split vowel stays whole
    }

    #[test]
    fn broken_clusters() {
        assert!(validate("ា").is_err()); // vowel without base
        assert!(validate("កាា").is_err()); // two post-base vowels
        assert!(validate("\u{17D2}ក").is_err()); // coeng without base
        assert!(validate("កា្រ្ក").is_err()); // only one subscript may follow a vowel
        assert!(validate("ាក").is_err());
    }

    #[test]
    fn subscript_after_vowel_renders_but_warns() {
        // Shapers accept one trailing coeng + consonant after the vowel.
        assert!(validate("កា្រ").is_ok());
        assert!(issues("កា្រ").iter().any(|i| i.kind == IssueKind::SubscriptAfterVowel));
    }

    #[test]
    fn warnings_do_not_fail_validation() {
        let w = issues("កេា");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, IssueKind::SplitVowel { expected: 'ោ' });
        assert!(validate("កេា").is_ok());
        assert!(issues("ក្").iter().any(|i| i.kind == IssueKind::DanglingCoeng));
        assert!(issues("ឣ").iter().any(|i| i.kind == IssueKind::Deprecated('ឣ')));
    }

    #[test]
    fn normalize_fixes_order() {
        assert_eq!(normalize("កា្រ"), "ក្រា"); // vowel before subscript
        assert_eq!(normalize("ក្រ្ស"), "ក្ស្រ"); // coeng-ro last
        assert_eq!(normalize("ប៉ា"), "ប៉ា"); // already fine
        assert_eq!(normalize("បា៉"), "ប៉ា"); // shifter before vowel
        assert_eq!(normalize("កំុ"), "កុំ"); // vowel before nikahit
        assert_eq!(normalize("កះា"), "កាះ"); // reahmuk last
        assert_eq!(normalize("កេា"), "កោ"); // compose split vowel
        assert_eq!(normalize("កេី"), "កើ");
        assert_eq!(normalize("ឣ"), "អ");
        assert_eq!(normalize("ឤ"), "អា");
        // Found by aksar-decode's random test: expanding ឤ before another vowel broke it.
        assert_eq!(normalize("ឤ\u{17CA}"), "អ\u{17CA}ា");
        // A coeng with nothing to stack is valid after ឤ, broken after អា (found by fuzzing).
        for t in [
            "ឤឿ",
            "ឤិ",
            "ឤ\u{17C1}",
            "ឤ\u{17CA}",
            "ឤ\u{17C6}",
            "ឤ្ផឿ",
            "ឤ្ក",
            "រឤ\u{17D2}",
            "ឣឤ\u{17D2}\u{200D}ក",
            "ខឤ\u{17D2}aគរឿ",
        ] {
            assert!(validate(t).is_ok() && validate(&normalize(t)).is_ok(), "{t}");
            assert_eq!(normalize(&normalize(t)), normalize(t), "{t}");
        }
        assert_eq!(normalize("hello ខ្មែរ!"), "hello ខ្មែរ!");
    }

    /// The CLI and apps print issues; Debug output (`SplitVowel { expected: 'ោ' }`)
    /// is not for people.
    #[test]
    fn issues_read_as_sentences() {
        let all = issues("កេា");
        assert_eq!(all[0].to_string(), "warning at 3..9: split vowel: use ោ");
        assert_eq!(
            issues("ាក")[0].to_string(),
            "error at 0..3: broken cluster: renders with a dotted circle"
        );
    }

    /// The module doc promises shifters are never moved across subscripts: a
    /// shifter typed on coeng-RO used to jump to the base (`ក្រ៉` → `ក៉្រ`),
    /// which changes the letter it attaches to and the search key.
    #[test]
    fn shifter_on_coeng_ro_stays_on_it() {
        for t in ["ក្រ៉", "ស្រ៊ី", "ក្រ៉ា", "ក្ស្រ៉"] {
            assert_eq!(normalize(t), t, "{t}");
        }
        // Coeng-RO still goes last, taking its shifter along.
        assert_eq!(normalize("ក្រ៉្ស"), "ក្ស្រ៉");
    }

    #[test]
    fn repair_cleans_ocr_output() {
        assert_eq!(repair("កា\u{17B6}"), "កា"); // stutter
        assert_eq!(repair("ក\u{17B4}"), "ក"); // invisible vowel
        assert_eq!(repair("\u{17A3}"), "\u{17A2}");
        assert_eq!(repair("\u{17D8}"), "។ល។");
        assert_eq!(repair("កា្រ"), "ក្រា");
        // PDFTape's "already canonical" cases stay unchanged.
        for s in ["ព្រះរាជាណាចក្រកម្ពុជា", "ក្រសួងអប់រំ", "ប៊ី", "ឲ្យ", "Hello, world"]
        {
            assert_eq!(repair(s), s, "{s}");
        }
    }

    #[test]
    fn normalize_is_idempotent_and_valid() {
        for s in ["កា្រ", "បា៉", "កំុ", "ក្រ្ស", "សួស្តី", "កេា", "ក្", "ាក"]
        {
            let n = normalize(s);
            assert_eq!(normalize(&n), n, "{s}");
        }
        for s in ["កា្រ", "បា៉", "កំុ", "កះា"] {
            assert_eq!(validate(&normalize(s)), Ok(()), "{s}");
        }
    }
}
