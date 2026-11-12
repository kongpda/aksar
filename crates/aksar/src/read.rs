//! The one-call entry point: give aksar Khmer text, get the best clean version
//! back with every word checked (see `docs/API.md`).
//!
//! This version reads **text**. Images, PDFs and photos come with the OCR
//! engine (`docs/OCR_PLAN.md`); they will return the same [`Document`].
//!
//! ```
//! use aksar::read::Aksar;
//!
//! let ak = Aksar::new();
//! let doc = ak.read("ទីក្រុងភ្នំពេញគឺជារាជធានីនៃប្រទេសកម្ពជា");
//! assert_eq!(doc.text(), "ទីក្រុងភ្នំពេញគឺជារាជធានីនៃប្រទេសកម្ពុជា"); // ុ restored
//! let w = doc.words().next().unwrap();
//! assert!(w.flags.corrected && !w.needs_review());
//! ```

use crate::dict::{Check, Dictionary, UserWords};
use crate::syllable::repair;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Reads Khmer text with the bundled dictionary and optional user words.
#[derive(Clone, Debug)]
pub struct Aksar {
    user: UserWords,
    autocorrect: bool,
}

/// Same as [`Aksar::new`] (autocorrect on).
impl Default for Aksar {
    fn default() -> Self {
        Aksar::new()
    }
}

/// Whitespace that separates tokens. NBSP is not one: the Khmer grammar uses it
/// as a base for a lone vowel or sign, so splitting there would orphan the mark.
fn is_separator(c: char) -> bool {
    c.is_whitespace() && c != '\u{A0}'
}

impl Aksar {
    /// Defaults: bundled dictionary, no user words, sure corrections applied.
    pub fn new() -> Self {
        Aksar { user: UserWords::new(), autocorrect: true }
    }

    /// Adds words (names, company terms) that are never flagged.
    ///
    /// Each entry must be one word: entries that [`UserWords::accept`] rejects
    /// (spaces or punctuation, over [`crate::dict::MAX_USER_WORD_CHARS`]
    /// characters, already in the dictionary) are skipped. Check them with
    /// `accept` first to tell the user which ones were not added.
    pub fn user_words<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, words: I) -> Self {
        for w in words {
            self.user.insert(Dictionary::bundled(), w.as_ref());
        }
        self
    }

    /// `false`: never change a word, only flag it and offer the fix as a
    /// suggestion (for text people typed, where a "fix" may be unwanted).
    pub fn autocorrect(mut self, on: bool) -> Self {
        self.autocorrect = on;
        self
    }

    /// Reads `text`: repairs and normalizes Khmer, checks every word against the
    /// dictionary, applies fixes that are certain (one unique edit) and flags the
    /// rest. Spacing is kept as it is. Lines end at `\n` or `\r\n`; the `\r`
    /// belongs to the line break, so it is in no [`Line::text`] and
    /// [`Document::text`] joins lines with `\n`.
    pub fn read(&self, text: &str) -> Document {
        let lex = Dictionary::bundled().with_user_words(&self.user);
        let mut blocks = Vec::new();
        let mut block = Block { lines: Vec::new() };
        let mut after_blank = false;
        for raw_line in text.split('\n') {
            let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if raw_line.trim_matches(is_separator).is_empty() {
                // Blank lines end the paragraph they follow (kept for `text()`).
                block.lines.push(Line { text: raw_line.to_string(), words: Vec::new() });
                after_blank = true;
                continue;
            }
            if after_blank && block.lines.iter().any(|l| !l.words.is_empty()) {
                blocks.push(core::mem::replace(&mut block, Block { lines: Vec::new() }));
            }
            after_blank = false;
            block.lines.push(self.read_line(&lex, raw_line));
        }
        if !block.lines.is_empty() {
            blocks.push(block);
        }
        Document { pages: alloc::vec![Page { index: 0, source: Source::Text, blocks }] }
    }

    fn read_line(&self, lex: &crate::dict::WithUserWords<'_>, raw_line: &str) -> Line {
        let mut out = String::with_capacity(raw_line.len());
        let mut words = Vec::new();
        let mut rest = raw_line;
        while !rest.is_empty() {
            let ws = rest.len() - rest.trim_start_matches(is_separator).len();
            out.push_str(&rest[..ws]);
            rest = &rest[ws..];
            let end = rest.find(is_separator).unwrap_or(rest.len());
            let (token, tail) = rest.split_at(end);
            rest = tail;
            if token.is_empty() {
                continue;
            }
            let word = self.read_word(lex, token);
            out.push_str(&word.text);
            words.push(word);
        }
        Line { text: out, words }
    }

    fn read_word(&self, lex: &crate::dict::WithUserWords<'_>, token: &str) -> Word {
        let fixed = repair(token);
        let khmer = fixed.chars().any(crate::chars::is_khmer);
        let mut flags = Flags {
            number: fixed.chars().any(|c| c.is_ascii_digit() || ('០'..='៩').contains(&c)),
            latin: fixed.chars().any(|c| c.is_ascii_alphabetic()),
            ..Flags::default()
        };
        let mut text = fixed.clone();
        let mut suggestions = Vec::new();
        if khmer {
            match lex.check(&fixed) {
                Check::Known => {}
                Check::Corrected(c) => {
                    if self.autocorrect {
                        text = c;
                        flags.corrected = true;
                    } else {
                        suggestions.push(c);
                        flags.unknown_word = true;
                    }
                }
                _ => flags.unknown_word = true,
            }
            // The other spelling of ្ត/្ដ, when the dictionary has both.
            for s in Dictionary::bundled().spellings(&text) {
                if s != text && !suggestions.iter().any(|x| x == s) {
                    suggestions.push(s.to_string());
                }
            }
        }
        let parts = if khmer {
            lex.segment(&text).into_iter().map(str::to_string).collect()
        } else {
            alloc::vec![text.clone()]
        };
        Word { raw: token.to_string(), text, parts, suggestions, flags }
    }
}

/// Where a page's text came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Source {
    /// Text given to [`Aksar::read`].
    Text,
    /// Text recognized from an image (the `aksar-ocr` crate).
    Image,
}

/// The result of [`Aksar::read`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Document {
    /// Pages in order (text input is one page).
    pub pages: Vec<Page>,
}

/// One page.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Page {
    /// 0-based page number.
    pub index: usize,
    /// Where the text came from.
    pub source: Source,
    /// Paragraphs: runs of non-empty lines, each followed by its blank lines.
    pub blocks: Vec<Block>,
}

/// A paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Block {
    /// Lines in order. Empty lines are kept so `text()` gives the line breaks back.
    pub lines: Vec<Line>,
}

/// One line of text.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Line {
    /// The line after reading (original whitespace kept).
    pub text: String,
    /// Space-separated tokens (a Khmer token may hold several words; see [`Word::parts`]).
    pub words: Vec<Word>,
}

/// One space-separated token.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Word {
    /// Final text: repaired, normalized, corrected when sure.
    pub text: String,
    /// Exactly as given.
    pub raw: String,
    /// Dictionary words inside `text` (Khmer has no spaces between words).
    pub parts: Vec<String>,
    /// Alternatives to offer in a review screen, best first.
    pub suggestions: Vec<String>,
    /// What aksar found.
    pub flags: Flags,
}

impl Word {
    /// Worth showing to a person: Khmer the dictionary does not know. Digits
    /// never count as unknown, so a token mixing a number and a misspelled word
    /// still needs review.
    pub fn needs_review(&self) -> bool {
        self.flags.unknown_word
    }
}

/// Facts about a [`Word`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Flags {
    /// Khmer clusters remain that the dictionary (and user words) don't cover.
    pub unknown_word: bool,
    /// `text` differs from `raw` by a dictionary fix.
    pub corrected: bool,
    /// Contains digits (ASCII or Khmer). The digits themselves are never
    /// changed; Khmer words in the same token are still checked and corrected.
    pub number: bool,
    /// Contains Latin letters.
    pub latin: bool,
}

impl Document {
    /// Full text in reading order: lines joined with `\n` (a `\r\n` input comes
    /// back as `\n`), spacing as given.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (pi, page) in self.pages.iter().enumerate() {
            if pi > 0 {
                out.push('\u{c}');
            }
            let mut first = true;
            for line in page.blocks.iter().flat_map(|b| &b.lines) {
                if !first {
                    out.push('\n');
                }
                first = false;
                out.push_str(&line.text);
            }
        }
        out
    }

    /// Every word, in order.
    pub fn words(&self) -> impl Iterator<Item = &Word> {
        self.pages.iter().flat_map(|p| &p.blocks).flat_map(|b| &b.lines).flat_map(|l| &l.words)
    }

    /// Tokens for a full-text index ([`crate::search::search_tokens`]).
    pub fn search_tokens(&self) -> String {
        crate::search::search_tokens(&self.text())
    }

    /// The document as JSON (`"schema": 1`; the JSON Schema is `schema/document.schema.json` in this crate).
    pub fn to_json(&self) -> String {
        let mut j = String::from("{\"schema\":1,\"pages\":[");
        for (pi, page) in self.pages.iter().enumerate() {
            if pi > 0 {
                j.push(',');
            }
            let source = match page.source {
                Source::Text => "text",
                Source::Image => "image",
            };
            j.push_str(&alloc::format!("{{\"index\":{},\"source\":\"{source}\",\"blocks\":[", page.index));
            for (bi, block) in page.blocks.iter().enumerate() {
                if bi > 0 {
                    j.push(',');
                }
                j.push_str("{\"lines\":[");
                for (li, line) in block.lines.iter().enumerate() {
                    if li > 0 {
                        j.push(',');
                    }
                    j.push_str("{\"text\":");
                    json_str(&mut j, &line.text);
                    j.push_str(",\"words\":[");
                    for (wi, w) in line.words.iter().enumerate() {
                        if wi > 0 {
                            j.push(',');
                        }
                        j.push_str("{\"text\":");
                        json_str(&mut j, &w.text);
                        j.push_str(",\"raw\":");
                        json_str(&mut j, &w.raw);
                        j.push_str(",\"parts\":");
                        json_list(&mut j, &w.parts);
                        j.push_str(",\"suggestions\":");
                        json_list(&mut j, &w.suggestions);
                        j.push_str(&alloc::format!(
                            ",\"unknown_word\":{},\"corrected\":{},\"number\":{},\"latin\":{},\"needs_review\":{}}}",
                            w.flags.unknown_word,
                            w.flags.corrected,
                            w.flags.number,
                            w.flags.latin,
                            w.needs_review()
                        ));
                    }
                    j.push_str("]}");
                }
                j.push_str("]}");
            }
            j.push_str("]}");
        }
        j.push_str("]}");
        j
    }
}

/// Appends `s` to `out` as a JSON string literal (quoted and escaped), the way
/// [`Document::to_json`] writes strings. For tools that wrap the document in
/// their own JSON.
pub fn write_json_str(out: &mut String, s: &str) {
    json_str(out, s);
}

fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&alloc::format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_list(out: &mut String, items: &[String]) {
    out.push('[');
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(out, s);
    }
    out.push(']');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_corrects_and_keeps_layout() {
        let text = "  ក្រសួងអប់រំ  PDF 2026\n\nកម្ពជា ឆ្ឃឞឞ\n";
        let doc = Aksar::new().read(text);
        assert_eq!(doc.text(), "  ក្រសួងអប់រំ  PDF 2026\n\nកម្ពុជា ឆ្ឃឞឞ\n");
        let words: Vec<&Word> = doc.words().collect();
        assert_eq!(words.len(), 5);
        assert_eq!(words[0].parts, ["ក្រសួង", "អប់រំ"]);
        assert!(words[1].flags.latin && !words[1].needs_review());
        assert!(words[2].flags.number && !words[2].flags.corrected);
        assert!(words[3].flags.corrected && words[3].raw == "កម្ពជា");
        assert!(words[4].needs_review());
        assert_eq!(doc.pages[0].blocks.len(), 2);
    }

    // NBSP carries a lone vowel without a dotted circle; splitting the token
    // there left a bare ា that was flagged as an unknown word.
    #[test]
    fn nbsp_base_stays_in_its_token() {
        let doc = Aksar::new().read("\u{A0}\u{17B6}");
        let words: Vec<&Word> = doc.words().collect();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].raw, "\u{A0}\u{17B6}");
        assert!(!words[0].needs_review());
        assert_eq!(doc.text(), "\u{A0}\u{17B6}");
    }

    // A trailing newline stays in the document as a final empty line so that
    // `text()` round-trips LF input exactly (CRLF comes back as LF); callers that print lines (the CLI)
    // trim one trailing newline themselves.
    #[test]
    fn trailing_newline_round_trips_as_empty_last_line() {
        let doc = Aksar::new().read("កម្ពុជា\n");
        let lines: Vec<&Line> = doc.pages[0].blocks.iter().flat_map(|b| &b.lines).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].text.is_empty() && lines[1].words.is_empty());
        assert_eq!(doc.text(), "កម្ពុជា\n");
    }

    /// Windows files end lines with `\r\n`. The `\r` is part of the break,
    /// not of the line: JSON consumers saw `"text":"...\r"` on every line but
    /// the last.
    #[test]
    fn crlf_line_breaks_are_not_line_text() {
        let doc = Aksar::new().read("កម្ពជា\r\nខ្មែរ\r\n\r\nភ្នំពេញ");
        let lines: Vec<&Line> = doc.pages[0].blocks.iter().flat_map(|b| &b.lines).collect();
        assert!(lines.iter().all(|l| !l.text.contains('\r')), "{lines:?}");
        assert_eq!(doc.pages[0].blocks.len(), 2, "the blank CRLF line still ends a paragraph");
        assert_eq!(doc.text(), "កម្ពុជា\nខ្មែរ\n\nភ្នំពេញ");
        // A lone \r inside a line is spacing, kept as given.
        assert_eq!(Aksar::new().read("ក\rខ").text(), "ក\rខ");
    }

    #[test]
    fn default_is_new() {
        // A derived Default once turned autocorrect off.
        assert_eq!(Aksar::default().read("កម្ពជា").text(), "កម្ពុជា");
    }

    /// Khmer has no spaces, so a year often shares a token with words. The
    /// digit must not hide a misspelling in the same token.
    #[test]
    fn a_number_does_not_hide_an_unknown_word() {
        let doc = Aksar::new().read("ឆ្ឃឞឞ២០២៦ ២០២៦");
        let w: Vec<_> = doc.words().collect();
        assert!(w[0].flags.number && w[0].flags.unknown_word && w[0].needs_review());
        assert!(w[1].flags.number && !w[1].needs_review(), "a bare number is fine");
    }

    #[test]
    fn without_autocorrect_the_fix_is_only_suggested() {
        let doc = Aksar::new().autocorrect(false).read("កម្ពជា");
        let w = doc.words().next().unwrap();
        assert_eq!(w.text, "កម្ពជា");
        assert_eq!(w.suggestions, ["កម្ពុជា"]);
        assert!(w.needs_review());
    }

    #[test]
    fn user_words_are_not_flagged_and_ta_da_offers_the_other_spelling() {
        let doc = Aksar::new().user_words(["សុខស្ដារ៉ា"]).read("សុខស្តារ៉ា ស្តី");
        let w: Vec<&Word> = doc.words().collect();
        assert!(!w[0].needs_review());
        assert_eq!(w[1].suggestions, ["ស្ដី"]);
    }

    /// Checks `v` against the subset of JSON Schema the shipped schema uses:
    /// `$ref`, `type`, `const`, `enum`, `required`, `properties`, `items`, `minimum`.
    fn check_schema(root: &serde_json::Value, schema: &serde_json::Value, v: &serde_json::Value, at: &str) {
        use serde_json::Value;
        if let Some(r) = schema.get("$ref") {
            let name = r.as_str().unwrap().strip_prefix("#/$defs/").unwrap();
            return check_schema(root, &root["$defs"][name], v, at);
        }
        if let Some(c) = schema.get("const") {
            assert_eq!(c, v, "{at}: const");
        }
        if let Some(e) = schema.get("enum") {
            assert!(e.as_array().unwrap().contains(v), "{at}: {v} not in enum");
        }
        if let Some(t) = schema.get("type") {
            let ok = match t.as_str().unwrap() {
                "object" => v.is_object(),
                "array" => v.is_array(),
                "string" => v.is_string(),
                "boolean" => v.is_boolean(),
                "integer" => v.is_u64() || v.is_i64(),
                t => panic!("{at}: schema type {t} not supported by this test"),
            };
            assert!(ok, "{at}: expected {t}, got {v}");
        }
        if let (Some(min), Some(n)) = (schema.get("minimum"), v.as_i64()) {
            assert!(n >= min.as_i64().unwrap(), "{at}: below minimum");
        }
        if let Value::Object(map) = v {
            let props = schema.get("properties").and_then(Value::as_object);
            for key in schema.get("required").map_or(&[][..], |r| r.as_array().unwrap().as_slice()) {
                assert!(map.contains_key(key.as_str().unwrap()), "{at}: missing key {key}");
            }
            // The schema lists every key to_json emits: a new key must be added there.
            for (k, child) in map {
                let sub =
                    props.and_then(|p| p.get(k)).unwrap_or_else(|| panic!("{at}: key {k:?} not in schema"));
                check_schema(root, sub, child, &alloc::format!("{at}.{k}"));
            }
        }
        if let (Value::Array(items), Some(sub)) = (v, schema.get("items")) {
            for (i, item) in items.iter().enumerate() {
                check_schema(root, sub, item, &alloc::format!("{at}[{i}]"));
            }
        }
    }

    // Consumers rely on the published schema; to_json must keep matching it.
    #[test]
    fn to_json_matches_the_published_schema() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../schema/document.schema.json")).unwrap();
        let texts = ["", "\n\n", "កម្ពជា ឆ្ឃឞឞ២០២៦ PDF \"q\" \t\\\n\nក្រសួងអប់រំ\n", "\u{A0}\u{17B6} x"];
        for text in texts {
            let json = Aksar::new().read(text).to_json();
            let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}: {json}"));
            check_schema(&schema, &schema, &v, "$");
        }
    }

    #[test]
    fn repairs_and_json() {
        let doc = Aksar::new().read("ស\u{17D2}\u{179A}\u{17D2}\u{178F}\u{17B8} \"q\"");
        assert_eq!(doc.words().next().unwrap().text, "ស្ត្រី");
        let json = doc.to_json();
        assert!(json.starts_with("{\"schema\":1,\"pages\":[{\"index\":0,\"source\":\"text\""));
        assert!(json.contains("\"raw\":\"\\\"q\\\"\""));
        assert!(!doc.search_tokens().is_empty());
    }
}
