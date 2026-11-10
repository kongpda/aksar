# What aksar takes from PDFTape, and what PDFTape should fix

PDFTape (`KhableCom/pdftape`, `crates/pdftape-ocr`) already ships offline Khmer OCR on
desktop and mobile: Tesseract 5 in-process, a preprocessing pipeline, a Khmer
normalizer, a dictionary corrector, search tokens, a searchable-PDF text layer, a
benchmark corpus and a Tesseract fine-tuning pipeline. Reviewed 2026-10-18 at commit
`4ff4d28`.

## Status (2026-10-18)

In use: PDFTape PR #8 (`feat/ocr-use-aksar`) vendors aksar v0.1.0 and turns
`khmer.rs`/`dictionary.rs` into wrappers. Bugs 1–7 below are fixed there; PDFTape's newer
user-word feature runs on `aksar::dict::UserWords`. Measured vs PDFTape `main`: Khmer CER
2.95% → 2.93%, 54 pages none worse, 18 other languages byte-identical.

## Ported into aksar (done)

| PDFTape | aksar | Changes |
|---------|-------|---------|
| `dictionary.rs` (ICU word list, single-edit corrector) | `aksar::dict` (features `dict`, `bundled-dict`) | Same rules and all of PDFTape's tests. **Linear time** instead of quadratic, no 109 ms startup build, cleaned word list. A differential test checks it gives the same answer as the original algorithm on random OCR-like tokens. |
| `khmer.rs` `index_form`, `search_tokens`, `fts_query` | `aksar::search` (feature `search`) | Same design (Khmer cluster bigrams + lowercase words, quoted FTS5 terms). Fixed the cluster bug below; idempotent normalization. |
| `khmer.rs` `normalize` cleanup steps | `aksar::syllable::repair` | Invisible U+17B4/U+17B5 removed, ៘ → ។ល។, stuttered vowels/signs dropped, on top of aksar's HarfBuzz-checked normalizer. |
| `assets/khmer-words.txt` (ICU, Unicode-3.0) | `crates/aksar/data/khmer-words.txt` | Normalized, 46 broken entries removed, sorted, deduplicated: 81,029 → 80,630 words. |

Measured (release, 4-core Linux):

| Token length | PDFTape `check` | aksar `check` |
|--------------|-----------------|---------------|
| 39 chars | 6.1 ms | 2.0 ms |
| 72 chars | 25.3 ms | 1.9 ms |
| 138 chars | 91.5 ms | 2.0 ms |
| 270 chars | 356.8 ms | 2.4 ms |
| short known word | 14 µs | 10 µs |

First use: PDFTape builds a `HashSet` with 81k normalized words (109 ms); aksar indexes the
pre-normalized list into a compact hash table in 5–7 ms.

## Bugs found in PDFTape (fix there, or switch to aksar)

Found by running PDFTape's code through aksar's HarfBuzz-checked test data (60,000 random
Khmer sequences, each labeled by real HarfBuzz as rendering cleanly or with a dotted circle).

1. **`khmer::normalize` breaks text that rendered fine** (3 of 22,862 valid cases). It
   moves register shifters (៉ ៊) after all subscripts. When one was on the base and one on a
   subscript, both end up after the subscript, which HarfBuzz rejects. Example:
   `ស គ ៉ ្គ ៊` (U+179F 1782 17C9 17D2 1782 17CA) → `ស គ ្គ ៉ ៊` renders with ◌.
2. **`khmer::normalize` is not idempotent** (53 cases): running it twice changes the text
   again, around a coeng with nothing to stack (e.g. U+1781 17D1 17DD 17BD 17D2). Search
   indexes the document with one pass and the query with another, so rare strings stop
   matching.
3. **`khmer::clusters` swallows the character after a dangling coeng.** `ក្ ខ` gives the
   cluster `"ក្ "` (with the space) and `ក្។` gives one cluster `"ក្។"`. The code takes the
   next character after ្ without checking that it is a consonant.
4. **Final signs are not ordered after other signs.** `mark_rank` gives ះ ៈ ៓ ៝ the same
   rank as ំ ់ etc., so `កះំ` stays as typed, which HarfBuzz renders with ◌. aksar puts finals last.
5. **`dictionary::check` is quadratic in token length** (table above). On a page with a few
   long unspaced Khmer lines this adds up to seconds.
6. **The bundled word list contains 46 entries that render broken** (e.g. `ខន្តីី`,
   `ខា្លំង`, `កំណួចខ្្សែ`) and 663 in non-canonical order. Normalizing at load fixes the
   order, but the broken entries stay as valid "words", so the corrector can "correct"
   toward a misspelling.
7. **`segment_words` uses `WordSegmenter::new_auto`** (LSTM for Khmer). Measured in aksar's
   STACK.md: LSTM is ~100× slower than dictionary mode (0.4 vs 42 MB/s) with slightly lower
   precision (0.97 vs 1.00), and `new_auto` links both LSTM models and dictionaries.
   `new_dictionary` is faster and safer for line breaking and search.

Suggested fix order for PDFTape: depend on `aksar` (features `dict`, `search`) and delete
`khmer.rs`/`dictionary.rs`, or patch 1–5 in place. The ported tests are the same, so
behavior on PDFTape's fixtures should not change except for the fixed cases.

## Reuse next (not ported yet)

- **Benchmark corpus and scorer** (`bench/make_corpus.py`, `score.py`, CI fixtures with
  per-page CER ceilings): becomes aksar-ocr's accuracy gate. Swap Pillow+raqm rendering for
  aksar's own shaping (glyph-exact with HarfBuzz) so training and test images come from the
  production shaper.
- **Preprocessing** (`preprocess.rs`: DPI normalization, Sauvola, despeckle, deskew,
  orientation): the measured biggest win (degraded Khmer 12.8% → 0.9% CER). Port to
  aksar-ocr, replacing the `image` crate with plain gray buffers so it builds for WASM.
- **Tesseract fine-tune pipeline** (`tools/ocr-train`): already targets the measured
  weakness. The remaining Khmer error is mostly font coverage (Battambang 0.21% CER vs Noto
  Sans Khmer 8.42%, Kantumruy Pro 9.41%), not preprocessing. Fine-tuning `khm` on those fonts
  is the cheapest accuracy gain; aksar's renderer can generate the line images.
- **Kiri-OCR result:** PDFTape measured it worse than its Tesseract pipeline in every
  category (4.19% → 19.18% CER), so no neural engine should replace Tesseract without beating
  this benchmark. aksar's rten plan (STACK.md) keeps that gate.
- **Searchable PDF text layer** (`text_layer.rs`, GlyphLess font, ActualText) and the
  **signed language-pack catalog** (`catalog.rs`): product-level, stay in PDFTape.

## Licensing note

PDFTape is a private KhableCom repository. The ported code is now under aksar's
`MIT OR Apache-2.0`. Confirm that KhableCom agrees to that license for these parts before
publishing aksar.
