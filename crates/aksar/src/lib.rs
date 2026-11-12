//! # aksar — a small Khmer text engine
//!
//! `aksar` (អក្សរ, "letters / script") gives Rust and WebAssembly apps:
//!
//! - [`chars`]: Khmer character classes (consonants, coeng, vowels, signs, digits).
//! - [`syllable`]: a NiDA/Unicode-order syllable validator and a safe normalizer
//!   for common typing and OCR ordering mistakes.
//! - [`shape`] (feature `shape`, on by default): Khmer shaping on top of
//!   [`harfrust`](https://crates.io/crates/harfrust), returning positioned glyphs.
//! - [`dict`] (feature `dict`, or `bundled-dict` with the ICU word list): word
//!   lookup, segmentation and a conservative OCR corrector.
//! - [`search`] (feature `search`): index/search tokens that find Khmer
//!   substrings regardless of word segmentation or spacing.
//!
//! - [`read`] (feature `read`): one call that reads Khmer text and returns it
//!   repaired, checked and corrected word by word, with JSON output.
//!
//! The crate is `no_std` + `alloc` when the `std` feature is off.
//!
//! ```
//! use aksar::syllable::{validate, normalize};
//!
//! // "ក + ្ + ក" is a valid subscript stack.
//! assert!(validate("ក្ក").is_ok());
//! // A vowel with no base renders as a dotted circle: an error.
//! assert!(validate("ាក").is_err());
//! // Vowel typed before the subscript: renders, but not NiDA order.
//! assert_eq!(normalize("កា្រ"), "ក្រា");
//! ```
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod chars;
pub mod syllable;

#[cfg(feature = "dict")]
pub mod dict;
#[cfg(feature = "read")]
pub mod read;
#[cfg(feature = "search")]
pub mod search;
#[cfg(feature = "shape")]
pub mod shape;
