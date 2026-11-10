//! Search index tokens for Khmer text (works with SQLite FTS5 or any inverted index).
//!
//! Khmer has no spaces between words, OCR spacing is unreliable, and word
//! segmenters disagree on boundaries. So every Khmer run is indexed as single
//! clusters plus **overlapping pairs of clusters**: a query is found wherever it
//! appears, whatever the segmenter or the spacing did. A one-cluster query
//! matches the cluster; a longer one must contain all of its pairs.
//! Other scripts are indexed as lowercase words: split on non-alphanumerics by
//! default, or by your own splitter (e.g. ICU for Thai/CJK) with
//! [`search_tokens_with`] / [`fts_query_with`].
//!
//! Matching is at cluster boundaries: `កម្ពុជា` (clusters ក ម្ពុ ជា) is found
//! by `ម្ពុជា` but not by `ពុជា`, which starts inside the stack ម្ពុ.
//! Callers should bound query length: every cluster pair becomes one `AND` term.
//!
//! **SQLite FTS5 needs a tokenizer that keeps Khmer marks.** The default
//! `unicode61` tokenizer treats vowel signs and coeng as separators, so every
//! term collapses to its base consonant and a query matches far too much. Store
//! [`search_tokens`] in a table like this (tokens are already space-separated
//! and lowercase):
//!
//! ```sql
//! CREATE VIRTUAL TABLE pages_fts USING fts5(
//!     tokens, tokenize = "unicode61 categories 'L* M* N* P* S* Co'"
//! );
//! ```
//!
//! Ported from PDFTape's `pdftape-ocr::khmer` with these fixes:
//! - clusters come from the HarfBuzz-exact syllable grammar, so a coeng with
//!   nothing to stack no longer swallows the next character (PDFTape turned
//!   `ក្ ខ` into the cluster `"ក្ "` and `ក្។` into one cluster);
//! - normalization is [`crate::syllable::repair`], which is idempotent, so a
//!   query and the indexed text always normalize the same way;
//! - subscript DA (្ដ) is folded to subscript TA (្ត): the two are drawn the
//!   same, so users and OCR write either one, and a query must find both.

use crate::chars::{ZWJ, ZWNJ, ZWSP, is_khmer};
use crate::syllable::{clusters, repair};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Text for indexing and comparison: repaired and normalized, with ZWSP removed
/// (and ZWNJ/ZWJ after a Khmer character, which only change the drawing),
/// whitespace between Khmer characters removed (Khmer spacing is phrase-level and
/// unreliable in OCR), other whitespace runs collapsed to one space, and
/// subscript DA (្ដ) folded to subscript TA (្ត), which looks the same.
///
/// This is a matching key, not display text: ស្ដី and ស្តី give the same result.
///
/// ```
/// use aksar::search::index_form;
/// assert_eq!(index_form("ស្ដាប់"), index_form("ស្តាប់"));
/// assert_eq!(index_form("ដើម"), "ដើម"); // base ដ is a different letter: kept
/// ```
pub fn index_form(input: &str) -> String {
    let n = repair(input);
    let mut chars: Vec<char> = Vec::with_capacity(n.len());
    for c in n.chars() {
        // A joiner inside a Khmer word would split its run, so the cluster pair
        // across it would never be indexed. Other scripts keep theirs.
        let khmer_joiner = matches!(c, ZWNJ | ZWJ) && chars.last().is_some_and(|&p| is_khmer(p));
        if c != ZWSP && !khmer_joiner {
            chars.push(c);
        }
    }
    let mut out = String::with_capacity(n.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            // A space between two Khmer letters is dropped (Khmer has no word
            // spaces); any other run of whitespace becomes one space.
            let between_khmer =
                start > 0 && is_khmer(chars[start - 1]) && i < chars.len() && is_khmer(chars[i]);
            let keep = !between_khmer && !out.is_empty() && i < chars.len();
            if keep {
                out.push(' ');
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    // Dropping a space can join a stray mark onto the previous syllable, so
    // repair once more; this keeps `index_form` idempotent. Fold first: repair
    // never produces ្ដ, so the result stays folded.
    repair(&fold_subscript_da(&out))
}

/// Subscript DA → subscript TA. Base letters are left alone.
fn fold_subscript_da(text: &str) -> String {
    text.replace("\u{17D2}\u{178A}", "\u{17D2}\u{178F}")
}

/// Splits index-form text into Khmer runs and other runs.
fn runs(text: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut cur: Option<bool> = None;
    for (i, c) in text.char_indices() {
        let k = is_khmer(c);
        if cur.is_some_and(|p| p != k) {
            out.push((cur.unwrap_or(false), &text[start..i]));
            start = i;
        }
        cur = Some(k);
    }
    if let Some(k) = cur {
        out.push((k, &text[start..]));
    }
    out
}

/// Default splitter for non-Khmer text: lowercase alphanumeric words.
pub fn split_words(run: &str, out: &mut Vec<String>) {
    for w in run.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        out.push(w.to_lowercase());
    }
}

fn push_terms<F: FnMut(&str, &mut Vec<String>)>(
    text: &str,
    unigrams: bool,
    terms: &mut Vec<String>,
    other: &mut F,
) {
    for (khmer, run) in runs(text) {
        if khmer {
            let cl = clusters(run);
            if unigrams || cl.len() == 1 {
                terms.extend(cl.iter().map(|c| c.to_string()));
            }
            for pair in cl.windows(2) {
                let mut t = String::with_capacity(pair[0].len() + pair[1].len());
                t.push_str(pair[0]);
                t.push_str(pair[1]);
                terms.push(t);
            }
        } else {
            other(run, terms);
        }
    }
}

/// Space-separated, sorted, deduplicated tokens to store in a full-text index.
pub fn search_tokens(input: &str) -> String {
    search_tokens_with(input, split_words)
}

/// [`search_tokens`] with a custom splitter for non-Khmer runs. The splitter
/// gets the run in [`index_form`] and should push lowercase words.
pub fn search_tokens_with<F: FnMut(&str, &mut Vec<String>)>(input: &str, mut other: F) -> String {
    let mut terms = Vec::new();
    push_terms(&index_form(input), true, &mut terms, &mut other);
    terms.sort();
    terms.dedup();
    terms.join(" ")
}

/// FTS5 `MATCH` expression for a user query: every term must appear. Terms are
/// quoted, so user input can never inject FTS syntax. `None` for an empty query.
pub fn fts_query(query: &str) -> Option<String> {
    fts_query_with(query, split_words)
}

/// [`fts_query`] with the same custom splitter used for [`search_tokens_with`].
pub fn fts_query_with<F: FnMut(&str, &mut Vec<String>)>(query: &str, mut other: F) -> Option<String> {
    let mut terms = Vec::new();
    push_terms(&index_form(query), false, &mut terms, &mut other);
    // Drop repeats anywhere, keeping the query's order.
    let mut seen = alloc::collections::BTreeSet::new();
    terms.retain(|t| seen.insert(t.clone()));
    if terms.is_empty() {
        return None;
    }
    let quoted: Vec<String> =
        terms.iter().map(|t| alloc::format!("\"{}\"", t.replace('"', "\"\""))).collect();
    Some(quoted.join(" AND "))
}

#[cfg(test)]
mod tests {
    use super::*;

    // PDFTape's tests, kept as regression tests.
    #[test]
    fn index_form_removes_khmer_spaces_and_zwsp() {
        assert_eq!(index_form("ជាតិ  សាសនា\u{200B}ព្រះមហាក្សត្រ PDF  file"), "ជាតិសាសនាព្រះមហាក្សត្រ PDF file");
    }

    #[test]
    fn clusters_keep_subscripts_with_base() {
        assert_eq!(clusters("ស្ត្រី"), ["ស្ត្រី"]);
        // aksar keeps a split vowel with its base: "ជា" is one cluster.
        assert_eq!(clusters("កម្ពុជា"), ["ក", "ម្ពុ", "ជា"]);
    }

    #[test]
    fn query_tokens_are_subset_of_document_tokens() {
        let doc = search_tokens("ក្រសួងអប់រំ យុវជន និងកីឡា invoice 2026");
        let doc: alloc::collections::BTreeSet<&str> = doc.split(' ').collect();
        let q = fts_query("សួងអប់").unwrap(); // substring crossing a word boundary
        for t in q.split(" AND ") {
            assert!(doc.contains(t.trim_matches('"')), "{t} missing");
        }
        assert!(fts_query("Invoice").unwrap().contains("\"invoice\""));
    }

    #[test]
    fn fts_query_escapes_quotes_and_rejects_empty() {
        assert_eq!(fts_query("   "), None);
        assert_eq!(fts_query("a\"b").unwrap().matches('"').count() % 2, 0);
    }

    /// A repeated word must appear once: each term is another FTS5 lookup.
    #[test]
    fn fts_query_drops_repeated_terms_anywhere() {
        assert_eq!(fts_query("invoice total invoice").unwrap(), "\"invoice\" AND \"total\"");
    }

    // Fixed bugs.
    #[test]
    fn dangling_coeng_does_not_swallow_the_next_character() {
        assert_eq!(clusters("ក្ ខ"), ["ក្", " ", "ខ"]);
        assert_eq!(clusters("ក្។"), ["ក្", "។"]);
    }

    #[test]
    fn single_cluster_query_matches_inside_a_phrase() {
        let doc = search_tokens("ខ្ញុំនៅផ្ទះ");
        let doc: alloc::collections::BTreeSet<&str> = doc.split(' ').collect();
        assert_eq!(fts_query("នៅ").unwrap(), "\"នៅ\"");
        assert!(doc.contains("នៅ"));
    }

    #[test]
    fn custom_splitter_for_other_scripts() {
        let t = search_tokens_with("ខ្មែរ ABC-def", |run, out| {
            out.push(alloc::format!("[{}]", run.trim()))
        });
        assert!(t.contains("[ABC-def]"), "{t}");
    }

    #[test]
    fn removing_spaces_renormalizes() {
        // The space hid a mark that belongs to the previous syllable.
        let once = index_form("អោ \u{17CD}\u{17B8}");
        assert_eq!(index_form(&once), once);
    }

    #[test]
    fn subscript_ta_and_da_find_each_other() {
        let words = [("ស្តាប់", "ស្ដាប់"), ("ស្ត្រី", "ស្ដ្រី"), ("ក្តី", "ក្ដី"), ("ចិត្ត", "ចិត្ដ")];
        for (ta, da) in words {
            for (doc, query) in [(ta, da), (da, ta)] {
                let doc = search_tokens(doc);
                let doc: alloc::collections::BTreeSet<&str> = doc.split(' ').collect();
                for t in fts_query(query).unwrap().split(" AND ") {
                    assert!(doc.contains(t.trim_matches('"')), "{query} vs {doc:?}");
                }
            }
        }
        // Inside a phrase, and a one-cluster query.
        let doc = search_tokens("ខ្ញុំស្តាប់វិទ្យុ");
        assert!(doc.split(' ').any(|t| t == fts_query("ស្ដា").unwrap().trim_matches('"')));
    }

    /// Some words are spelled with a joiner (`ហ៊\u{200C}ិន`). It split the
    /// Khmer run, so a query across it (`សួងអប់`) could not match.
    #[test]
    fn joiners_inside_khmer_do_not_split_words() {
        let doc = search_tokens("ក្រសួង\u{200C}អប់រំ");
        let q = fts_query("សួងអប់").unwrap();
        for term in q.split(" AND ") {
            assert!(doc.split(' ').any(|t| alloc::format!("\"{t}\"") == term), "{term} not in {doc}");
        }
        assert_eq!(index_form("ហ៊\u{200C}ិន"), index_form("ហ៊ិន"));
        // Outside Khmer a joiner is part of the word (Persian, Indic): kept.
        assert!(index_form("می\u{200C}خواهم").contains('\u{200C}'));
    }

    #[test]
    fn only_the_subscript_is_folded() {
        assert_eq!(index_form("ដ្ដ"), "ដ្ត"); // base ដ kept, subscript folded
        assert_eq!(index_form("ដល់ ការ ដឹង"), "ដល់ការដឹង");
        assert_ne!(index_form("ដើម"), index_form("តើម"));
        assert!(!index_form("ស្ដី ក្ដី ប្ដី").contains("\u{17D2}\u{178A}"));
    }

    #[test]
    fn query_and_document_normalize_the_same_way() {
        // Typed in a different order than the document: still found.
        let doc = search_tokens("ស្ត្រី");
        let q = fts_query("ស្រ្តី").unwrap(); // coeng-RO typed first
        assert!(doc.contains(q.trim_matches('"')));
    }
}
