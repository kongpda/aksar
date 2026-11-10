# aksar · អក្សរ

A small, pure-Rust **Khmer text engine** for native apps and WebAssembly:

- **Shaping** — coeng stacks, split vowels, coeng-RO reordering, marks, digits — on top of
  [harfrust](https://crates.io/crates/harfrust). Output matches HarfBuzz glyph-for-glyph on
  the test corpus.
- **Validation** — finds text that renders with a dotted circle ◌, using the same syllable
  grammar as HarfBuzz/Uniscribe, plus warnings for likely typing/OCR mistakes.
- **Normalization** — puts each syllable into NiDA keyboard order (e.g. `កា្រ` → `ក្រា`,
  `េ + ា` → `ោ`) without ever breaking text that renders.
- **Dictionary + OCR corrector** (feature `bundled-dict`) — 80k-word ICU list, word
  segmentation, and a conservative fixer for common OCR slips (`កម្ពជា` → `កម្ពុជា`,
  `ពិភពេលាក` → `ពិភពលោក`) that flags instead of guessing when unsure.
- **Search tokens** (feature `search`) — find Khmer substrings in an FTS index whatever
  the spacing or word segmentation.
- `no_std` + `alloc`, `wasm32`, MIT OR Apache-2.0.

Status: **v0.1.0**, Khmer-first, used by PDFTape's OCR ([CHANGELOG.md](CHANGELOG.md)).
Nothing is published yet (crates.io, PyPI, npm): build from this repository. The API may
still change before 1.0.

Docs:

- [docs/PLAN.md](docs/PLAN.md): versioned roadmap and release process.
- [docs/API.md](docs/API.md): the one-call API design (text, PDFs, images, photos → best result).
  This is the target design; the parity table below shows what ships today.
- [docs/OCR_PLAN.md](docs/OCR_PLAN.md): the main goal, a free on-device Khmer reader for print,
  photos and handwriting. Results so far: [docs/OCR_RESULTS.md](docs/OCR_RESULTS.md).
- [docs/KHMER_ACCURACY_PLAN.md](docs/KHMER_ACCURACY_PLAN.md): Khmer accuracy milestones.
- [docs/STACK.md](docs/STACK.md): measured dependency choices.
- [docs/LANGUAGES.md](docs/LANGUAGES.md): which other languages aksar should support.
- [docs/PDFTAPE.md](docs/PDFTAPE.md): what was reused from PDFTape's OCR.
- [docs/REVIEW.md](docs/REVIEW.md): review of the original "khmer-shape" plan (history).

## Use

```rust
use aksar::shape::{Font, ShapeOptions, Shaper, shape};
use aksar::syllable::{normalize, validate};

let font = Font::from_bytes(std::fs::read("NotoSansKhmer-Regular.ttf")?)?;
let run = shape("សួស្តី", &font, &ShapeOptions::px(32.0))?;
println!("{} glyphs, width {}", run.glyphs.len(), run.width);

// Many strings: reuse a Shaper (~3x faster).
let mut shaper = Shaper::new(&font, &ShapeOptions::px(16.0))?;
for line in ["ខ្មែរ", "កម្ពុជា"] {
    println!("{line}: {}", shaper.shape(line)?.width);
}

assert!(validate("ាក").is_err());          // vowel with no base → ◌
assert_eq!(normalize("កា្រ"), "ក្រា");      // subscript before vowel
```

CLI:

```sh
cargo run -p aksar-cli -- shape --font testdata/fonts/NotoSansKhmer-Regular.ttf --size 32 "ខ្មែរ"
cargo run -p aksar-cli -- check "កេា"       # warning at 3..9: split vowel: use ោ ("េា")
                                            # normalized: កោ
cargo run -p aksar-cli -- normalize "បា៉"   # ប៉ា
cargo run -p aksar-cli -- spell "ពិភពេលាក" # fix  ពិភពេលាក -> ពិភពលោក
cargo run -p aksar-cli -- words "ព្រះរាជាណាចក្រកម្ពុជា"  # ព្រះរាជាណាចក្រ | កម្ពុជា
echo "ពិភពេលាក" | cargo run -p aksar-cli -- read -   # ពិភពលោក (--json for the full document)
# OCR a PNG page (needs a line model; Rust 1.94):
cargo run -p aksar-cli --features ocr -- ocr --model m.onnx --alphabet a.json page.png
```

Python (see `crates/aksar-py`, not on PyPI yet):

```sh
pip install maturin && maturin develop -m crates/aksar-py/Cargo.toml
python -c 'import aksar; print(aksar.normalize("កា្រ"))'   # ក្រា
```

WebAssembly (see `crates/aksar-wasm`):

```js
import init, { KhmerFont, isValid, normalize } from "./pkg/aksar_wasm.js"; // wasm-bindgen --target web
await init();
const font = new KhmerFont(fontBytes);   // Uint8Array of a .ttf/.otf
const run = font.shape("សួស្តី", 32);   // run.glyphIds, run.positions, run.clusters (JS string indices)
run.free(); font.free();                 // wasm memory is not garbage collected
isValid("ា");                            // false
normalize("កា្រ");                        // "ក្រា"
```

## API parity

| Feature | Rust (`aksar`) | CLI | WASM | Python |
|---------|----------------|-----|------|--------|
| Validate | `syllable::validate`, `issues` | `check` | `isValid`, `issues` (JSON, UTF-16 offsets) | `is_valid`, `issues` |
| Normalize / repair | `syllable::normalize`, `repair` | `normalize` | `normalize` | `normalize`, `repair` |
| Clusters | `syllable::clusters` | — | — | `clusters` |
| Shape | `shape::shape`, `Shaper` | `shape` | `KhmerFont.shape` | — |
| Word split | `dict::Dictionary::segment` | `words` | — | `words` |
| Spell check one word | `Dictionary::check` | `spell` | — | `spell` (`check` is a deprecated alias) |
| Read text | `read::Aksar` (feature `read`) | `read` | `read` (feature `read`) | `read` |
| Search tokens | `search::search_tokens`, `fts_query` | — | — | `search_tokens`, `fts_query` |
| OCR a page | `aksar-ocr` crate | `ocr` (feature `ocr`) | — | — |

`read --json`, `Document::to_json` and the WASM `read` share one JSON format, described by
[`crates/aksar/schema/document.schema.json`](crates/aksar/schema/document.schema.json)
(`"schema": 1`).

## Crates

| Crate | What | Status |
|-------|------|--------|
| `aksar` | chars, syllable validate/normalize/repair/clusters, shaping, dict (+ user words), search, one-call `read` | not published |
| `aksar-cli` | `aksar shape / check / normalize / spell / words / read` (+ `ocr` with feature `ocr`) | not published |
| `aksar-wasm` | wasm-bindgen API (feature `read` adds `read`) | not published |
| `aksar-py` | Python bindings (PyO3 + maturin) | not published |
| `aksar-render` | draws Khmer lines as images for OCR training data | not published |
| `aksar-image` | grayscale, binarization, deskew, text-line crops | not published |
| `aksar-decode` | CTC beam search constrained to valid Khmer, dictionary re-ranking | not published |
| `aksar-recog` | Khmer line recognizer (ONNX via rten) | not published, no released model |
| `aksar-ocr` | image in, checked text out | not published, no released model |
| `aksar-bench` | OCR benchmark: CER / cluster CER / WER per line set and engine | not published |
| `aksar::legacy`, `aksar-layout`, `aksar-detect` | see [OCR plan](docs/OCR_PLAN.md) | planned |

## Performance and size

Defaults favor speed. Use `Shaper` (or the WASM `KhmerFont`, which caches one) to shape many
strings: it builds the shaping plan once and reuses its buffer.

Shaping the 45 hand-written lines of the test corpus (259 glyphs) with a reused `Shaper`,
lower is better. Native times and all sizes measured 2026-11-10 on an Apple M1 Pro; the WASM
times are from 2026-10-18 (a different machine) and have not been re-measured.

| Build | Time per pass | WASM size (raw / gzip) |
|-------|---------------|------------------------|
| Native, `release` (opt-level 3) | **63 µs** | — |
| WASM `wasm-release` (default, opt-level 3) | ~195 µs (~315 µs from JS) | 1.07 MB / 353 KB |
| WASM `wasm-small` (opt-level z) | ~450 µs (~650 µs from JS) | 848 KB / 296 KB |
| WASM validate + normalize only | — | 71 KB / 27 KB |
| WASM `wasm-small` with `read` (dictionary) | — | 2.15 MB / 417 KB |

- `Shaper` reuse vs. calling `shape()` per line: ~3.5× faster (63 µs vs. 223 µs).
- `wasm-opt` and `simd128` gave no speedup, so they are not used.
- Validation runs at ~74 MB/s and normalization at ~48 MB/s natively.

Run `cargo bench -p aksar` for native numbers. CI fails if WASM sizes grow past the
budgets in `scripts/check-wasm-size.sh`.

## Tests

```sh
cargo test --workspace
```

Reference data comes from HarfBuzz. To regenerate after changing the corpus or font:

```sh
python -m venv .venv && .venv/bin/pip install uharfbuzz
.venv/bin/python scripts/gen_golden.py
```

- `testdata/golden/noto-sans-khmer.tsv`: shaping output for every line of `testdata/corpus/km.txt`.
- `testdata/golden/validity.tsv`: 6,000 random sequences and whether HarfBuzz draws ◌.
  The validator must agree on every one, and `normalize` must never break a renderable one.
- `scripts/gen_corpus.py` rebuilds the generated part of the 1,044-line corpus.
- Fuzzing: see [fuzz/README.md](fuzz/README.md).

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md). Report security problems privately as described in
[SECURITY.md](SECURITY.md).

## License

Code: MIT OR Apache-2.0, at your option. The bundled ICU word list
(`crates/aksar/data/khmer-words.txt`): Unicode License v3, so the `aksar` package is
`(MIT OR Apache-2.0) AND Unicode-3.0`. Test font: SIL OFL 1.1 (see `NOTICE`).
