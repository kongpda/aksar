# Changelog

## 0.1.1 — 2026-11-11

First version published to crates.io (`aksar`), PyPI (`aksar`) and npm (`aksar-wasm`).
Changes in behavior: `contains` normalizes, `read` drops `\r` from CRLF lines, `aksar shape`
rounds to 3 decimals and `OcrPage::to_json` adds `"schema": 1`. Khmer normalization, validation and shaping output
are unchanged.

- **Changed:** `Dictionary::contains`, `contains_exact`, `UserWords::contains`,
  `WithUserWords::contains` and `Words::contains` normalize their input, like every other
  dictionary method. A word typed in another mark order (e.g. a vowel before its subscript)
  now finds its listed spelling. A word listed exactly as given still costs one lookup and
  no allocation; only a miss is normalized and looked up again. New `contains_normalized`
  on all four keeps the old single-lookup behavior for hot loops over normalized text.
- OCR training pipeline, complete up to the GPU run (see `training/README.md`):
  - `train_crnn.py`: `--device auto|cpu|cuda|mps`, `--amp` (CUDA mixed precision),
    `--workers`, several training sets (comma-separated), `.png` + `.gt.txt` sets, and
    `--holdout` test sets: training refuses to start if a test or validation image is in
    the training data. `manifest.json` records checksums of every set, the settings, git
    commit, torch version, device, best CER and the model's SHA-256. Exports write
    `reference.txt`, PyTorch's output on a fixed line.
  - `split_set.py`: reproducible train/test split of a real line set by source page.
  - `evaluate.sh`: parity check plus `aksar-bench` (with Tesseract), JSON and markdown.
  - `package_model.py`: verifies the model checksum and parity, applies the OCR_PLAN §8
    gate (aksar CER below Tesseract on every set, no failed or invalid lines), writes the
    release folder with a generated `MODEL_CARD.md` and `SHA256SUMS`. Uploads nothing.
  - `smoke_test.sh`: the whole pipeline on a few lines on the CPU.
- `aksar-recog`: `Recognizer::from_dir` loads `model.onnx` + `alphabet.json` from a folder
  (size-capped reads, errors name the file); `Recognizer::check_reference` compares rten's
  output with the PyTorch reference (max difference 1e-4); `check_model` example.
- `aksar-ocr`: `Ocr::from_dir`.
- `aksar ocr --model-dir <dir>`, or the `AKSAR_MODEL_DIR` environment variable, instead of
  `--model` + `--alphabet` (still accepted). The image is read before the model is loaded.
- `aksar-bench`: `--model-dir`, `--json <file>` (schema 1, read by `package_model.py`) and
  an "Invalid" column: lines whose reading fails `validate`.
- `aksar check` and Python `issues()` map byte offsets to character offsets in one pass.
  They were quadratic: an 800 KB file with 200,000 broken clusters took 6.5 s in `check`,
  now 0.08 s. Output is unchanged. `check --json` no longer validates the text twice.
- `read`: a `\r\n` line break no longer leaves `\r` at the end of each line's `text`, and
  `Document::text` joins lines with `\n`. Output changes only for CRLF input.
- `search::fts_query` keeps at most `MAX_QUERY_TERMS` (64) terms, so a pasted page cannot
  become thousands of FTS5 lookups. A longer query can only match more rows. Indexes need no
  rebuild.
- `aksar-ocr`: `OcrPage::to_json` is now `"schema": 1`, described by
  `crates/aksar-ocr/schema/page.schema.json` (same content as before).
- New `aksar::read::write_json_str`, used by the CLI and `aksar-ocr` instead of their copies.
- `document.schema.json`: `$id` points at the `kongpda/aksar` repository.
- harfrust 0.14 → 0.15 and skrifa 0.48 → 0.49 (one read-fonts). Shaping still matches
  HarfBuzz glyph for glyph on the golden corpus. WASM sizes: `wasm-small` shaping 845 KB →
  988 KB, `wasm-release` 1.06 MB → 1.13 MB; budgets raised to 1 MiB and 1.25 MiB.
- `scripts/check-wasm-size.sh` honors `CARGO_TARGET_DIR`.
- `deny.toml` lists only the licenses in use (MIT, Apache-2.0, Zlib, Unicode-3.0,
  Apache-2.0 WITH LLVM-exception).
- **Fixed:** `GlyphRun::dotted_circles` missed circles the shaper inserted right after a
  typed `◌` (e.g. `◌ៅិ` reported 0, now 1), so `aksar-render` could accept broken text.
  Glyphs are unchanged.
- `aksar-ocr` tests, doctest and examples build without the `png` feature; the
  `make_lines` example builds on Rust 1.85 again.
- **Fixed:** `check` no longer "corrects" a token to the empty string (a lone vowel sign
  such as `ា` got `Corrected("")`); it is `Suspect`.
- Python: wheels now include the license files (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`
  and the word list's Unicode license), and a wheel built from the sdist keeps its type
  stubs (`__init__.pyi`, `py.typed`). The package moved to maturin's mixed layout
  (`crates/aksar-py/python/aksar/`); imports are unchanged.
- npm: the package includes its README.
- `scripts/check-copies.sh` (run by `check.sh` and CI) fails when a package's copy of a
  license file differs from the original.
- README and docs reworded for the public repository, with install commands.

### `aksar` CLI

- `aksar shape` prints pixel values with at most 3 decimals (`9.216`, not `9.216001`).
- Bare `aksar` prints the command list; unknown commands and options suggest the closest name
  (`did you mean "check"?`); usage errors point at the command's own `--help`.
- `aksar ocr --model/--alphabet` refuses files over the same size caps as `--model-dir`
  (256 MiB model, 1 MiB alphabet) instead of reading them whole.

These CLI changes do not change Khmer output.

## 0.1.0 — 2026-11-10

First version (not published to package registries). The API may change
before 1.0; output types are `#[non_exhaustive]` so fields can be added in a minor version.

### `aksar` (core, `no_std` + `alloc`, MSRV 1.85)

- `chars`: Khmer character classes (deprecated independent vowels ឣ ឤ count as bases), digits
  (`digit_value`, `to_khmer_digits`), Khmer Symbols block in `is_khmer`.
- `syllable`:
  - `syllables` / `clusters`: segmentation with HarfBuzz's Khmer syllable grammar, including
    its placeholder set (`-`, digits, NBSP, ×, dashes, •, ◌, ◻–◾).
  - `validate` / `issues`: an error exactly where HarfBuzz draws a dotted circle (checked on
    `testdata/golden/validity.tsv` and 320,000 random sequences), plus warnings (split vowel,
    deprecated character, dangling coeng, subscript after vowel, deep stack). `Issue` and
    `IssueKind` implement `Display`; `IssueKind::name()` is the stable kind name used by the
    CLI, WASM and Python.
  - `normalize`: NiDA storage order, idempotent, never breaks renderable text. Register
    shifters and robat move with coeng-RO; ឤ is expanded to អា only when that stays valid.
  - `repair`: `normalize` plus OCR cleanup (invisible U+17B4/U+17B5, ៘ → ។ល។, stuttered
    vowels and signs).
- `shape` (default feature): `Font`, `shape`, a reusable `Shaper` (~3.5× faster for many
  strings) on harfrust, glyph-identical to HarfBuzz on the 1,044-line corpus.
  `ShapeOptions` has builder setters (`px`, `with_size`, `with_language`, `with_features`).
  Errors: `InvalidSize` (NaN, infinite, zero or negative), `InvalidLanguage` (not BCP-47
  syntax). `Shaper::set_size` returns `Result` and keeps the old size on error.
  `Shaper::shape_into` leaves the run empty on error. Builds with `libm` without `std`.
- `dict` (feature `dict`; `bundled-dict` adds the cleaned 80,630-word ICU list):
  - `Dictionary` (`new`, `from_prepared`, `bundled`, `words`), `contains`, `contains_exact`,
    `spellings`, `segment`, `segment_cost` (returns a `Cost { unknown_clusters, words }`),
    `unknown_clusters`, `check` → `Check::{Known, Corrected, Suspect}`. Word lists with CRLF
    line ends or a BOM load correctly.
  - ជើងត (្ត) and ជើងដ (្ដ) are accepted for each other and the spelling as read is kept.
  - Conservative OCR corrector in linear time: single edits, left-hand vowels written before
    their consonant (several per word, within 28 clusters), the វ/រ confusion. It fixes only
    when exactly one fix makes the whole word known, and flags otherwise.
  - `best_alternative` / `SymbolChoice`: re-rank recognizer alternatives with the dictionary.
  - `UserWords` and `Dictionary::with_user_words`: reviewer-confirmed words count as known.
    `WithUserWords` has inherent `contains`, `segment`, `check` and friends.
- `search` (feature `search`): `index_form`, `search_tokens`, `fts_query` (Khmer cluster
  unigrams and bigrams, quoted FTS5 terms) and `*_with` variants for other scripts. ្ដ folds
  to ្ត and joiners are dropped, so either spelling finds the other.
- `read` (feature `read`): `Aksar` reads text into a `Document` (pages, blocks, lines, words)
  with repaired, checked and surely-corrected words, suggestions, review flags, `text()`,
  `search_tokens()` and `to_json()`. The JSON has `"schema": 1` and is described by
  `schema/document.schema.json`; a test checks real output against it. NBSP does not split
  words.

### Tools and bindings

- `aksar-cli`: `shape`, `check`, `normalize`, `spell`, `words`, `read`, and `ocr` behind the
  `ocr` feature (Rust 1.94). `--json` on `check`, `spell`, `words`, `normalize` and `read`;
  `check` reports char offsets and the text of each span. `-` reads stdin; a BOM is stripped.
  Options work in any position, `--` ends options, each command has `--help`, `-V` works
  anywhere. A closed pipe exits 0 quietly. Exit 1 means problems found (`check`,
  `spell`, `read`), exit 2 an error.
- `aksar-wasm`: `KhmerFont.shape` (cluster offsets are JS string indices), `isValid`,
  `normalize`, `issues` (JSON with UTF-16 offsets), and `read` with the `read` feature.
  Sizes: shaping 353 KB gzip, validate-only 27 KB gzip, size-checked in CI.
- `aksar-py` (PyO3 + maturin, type stubs): `normalize`, `repair`, `is_valid`, `clusters`,
  `words`, `spell`, `issues`, `read`, `search_tokens`, `fts_query`. `check` is a deprecated
  alias of `spell`.

### OCR crates (not published, no released model)

- `aksar-render`: draws Khmer lines for training data, refuses text it can't draw exactly,
  seeded augmentations, `make_lines` example (`real`, `noisy`, `alphabet` modes).
- `aksar-image`: grayscale, Otsu and sliding-window Sauvola binarization, polarity, skew
  detection (sampled) and correction, Khmer-aware line boxes, line crops. `prepare_page`
  refuses pages over `MAX_PIXELS`; an A4 page at 600 dpi peaks at 108 MB.
- `aksar-decode`: CTC prefix beam search that only keeps valid Khmer, re-ranked by unknown
  dictionary clusters; `Options::prune`; `Candidate::confidence`; deterministic output; a
  dangling coeng or joiner never ends a candidate. Memory stays flat on long lines (800
  steps: 2 MB peak).
- `aksar-recog`: line recognizer on rten (ONNX). `Recognizer::new` checks the model against
  the alphabet and rejects raw logits. A line that fails to read is empty with confidence 0.
- `aksar-ocr`: image in, checked text out, with line boxes, confidence and JSON. Images over
  64 MP are refused from the PNG header.
- `aksar-bench`: CER, cluster CER, folded CER, WER, speed, model size and peak memory per
  line set and engine (aksar, Tesseract).
- Training scripts (`training/`), Tesseract fine-tune recipe and error report
  (`research/khmer/`).

### Project

- License: code MIT OR Apache-2.0; the `aksar` package is `(MIT OR Apache-2.0) AND
  Unicode-3.0` because it ships the ICU word list.
- CI: fmt, clippy `-D warnings`, tests on Linux, macOS and Windows, MSRV, `no_std` on
  `thumbv7em-none-eabihf`, WASM size budgets, Python wheel tests on three OSes, `cargo deny`,
  a short fuzz run. Actions are pinned by SHA and checkouts keep no credentials.
- Release workflow builds wheels, sdist and the npm package; publishing needs a manual run
  and green CI, and uses PyPI and npm trusted publishing (no long-lived tokens).
- Fuzz targets for syllables, the dictionary, shaping and the page pipeline (`fuzz/`).
- `SECURITY.md`, `CONTRIBUTING.md`.
