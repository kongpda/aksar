# aksar — Plan (v3, Khmer-first)

**Project:** aksar (អក្សរ, "letters / script"), formerly "khmer-shape".
**Goal:** the best free Khmer reader we can build: printed and handwritten Khmer recognized **on the user's device**, with no paid AI, no internet and no per-page cost, plus the Khmer text engine (analysis, correction, search, shaping) that checks every result, in pure Rust, for desktop,
mobile, embedded and WebAssembly, licensed for commercial use. First user: [PDFTape](https://pdftape.com)'s OCR.
**Scope rule:** Khmer first. Other languages only when measured worth it
([`LANGUAGES.md`](LANGUAGES.md)); until then apps handle them themselves.
**Updated:** 2026-10-18. History: v1 plan reviewed in [`REVIEW.md`](REVIEW.md); dependency
choices measured in [`STACK.md`](STACK.md);
accuracy milestones in [`KHMER_ACCURACY_PLAN.md`](KHMER_ACCURACY_PLAN.md).

## 1. Decisions

| Topic | Decision | Why |
|-------|----------|-----|
| Scope | Khmer only (Unicode Khmer + Khmer Symbols blocks) | Where accuracy matters most; other scripts measured, deferred |
| OCR engine | **aksar's own free engine is the main goal** ([`OCR_PLAN.md`](OCR_PLAN.md)): small models for print and handwriting that run offline on desktop, mobile and web, checked by aksar's grammar and dictionary. Paid or cloud AI is never required. While it is built, aksar keeps improving Tesseract pipelines through post-processing, and apps should switch their default only when aksar's engine beats Tesseract on the benchmark gates (OCR_PLAN §8) | Users should not have to pay per page or send files away to read Khmer. Training costs compute once (free GPU tiers or a Mac are enough for the small models); running is free. Kiri-OCR measured worse than Tesseract in every category (4.19% → 19.18% CER), so every claim stays a measurement |
| Shaper | `harfrust` 0.15, optional feature `shape` | HarfBuzz port; glyph-exact vs HarfBuzz on our corpus |
| Word segmentation | aksar's own dictionary segmenter (`dict::Words::segment`) | Same precision as ICU dictionary mode (1.00) with no extra data; ICU's LSTM is 100× slower |
| Dependencies | Core: none. `shape`: harfrust. Later crates per [`STACK.md`](STACK.md) | Small, auditable, `no_std` |
| License | `MIT OR Apache-2.0`; word list Unicode License v3 | Usable in open-source and proprietary apps |
| Build | Speed first (opt-level 3, fat LTO); `wasm-small` for size | Measured 2.2× faster WASM shaping for +53 KB gzip |
| Truth | HarfBuzz (uharfbuzz, pinned) for shaping and validity; the S1/S4 benchmarks for OCR | Every claim is a measurement |

## 2. Khmer text model

The NiDA keyboard, printed text and handwriting share one orthography, so one validator and
normalizer serve typed text and OCR.

**Storage order** (what `normalize` produces): base consonant / independent vowel →
subscripts `្ + consonant` (coeng-RO last; register shifters ៉ ៊ and robat ៌ keep their typed
position) → one dependent vowel → signs ំ ់ ៍ ៎ ៏ ័ ៑ → finals ះ ៈ ៓ ៝.

**Valid** means HarfBuzz/Uniscribe would draw no dotted circle ◌ (checked on 320,000 random
sequences). Shapers also accept one subscript after the vowel (`ក ា ្ រ`): renders, but warned
and normalized. Split vowels (េ + ា vs ោ) are warned and composed. Deprecated characters are
warned; U+17A3 → អ, U+17A4 → អា; `repair` also drops U+17B4/U+17B5 and expands ៘ → ។ល។.
**ជើងត (្ត) and ជើងដ (្ដ) are different letters** (the spelling decides the word, ត្ត ≠ ដ្ដ) that
most fonts draw alike, so OCR can't choose between them from the picture. Search finds both;
the dictionary accepts both; aksar keeps the spelling as read and only suggests the listed one
(accuracy plan M3).
**Tags:** script `khmr`, OpenType language `KHM `, BCP-47 `km`.

## 3. Crates

| Crate | What | Status |
|-------|------|--------|
| `aksar` | `chars`, `syllable` (validate, normalize, repair, clusters), `dict` (word list, segmentation, OCR corrector, user words), `search` (FTS tokens), `shape`, `read` (one-call API) | v0.1.1, crates.io |
| `aksar-cli` | `shape`, `check`, `normalize`, `spell`, `words`, `read` | v0.1.1 (not published) |
| `aksar-wasm` | wasm-bindgen API: `KhmerFont.shape`, `isValid`, `normalize`, `read` (feature `read`) | v0.1.1, npm `aksar-wasm` |
| `aksar-render` | text line → image with aksar shaping + skrifa + zeno, seeded augmentation, `make_lines` dataset example (plain, `alphabet`, `real`, `noisy`) | v0.1.1 (not published) |
| `aksar-image` | grayscale, Otsu/Sauvola, deskew, line boxes, polarity, Pillow-compatible scaling | v0.1.1 (not published) |
| `aksar-decode` | CTC beam search that only keeps valid Khmer, dictionary re-ranking | v0.1.1 (not published) |
| `aksar-recog` | line recognizer: ONNX model run with rten (Rust 1.94) | v0.1.1 (not published); model not released yet |
| `aksar-ocr` | image in, checked text out | v0.1.1 (not published); model not released yet |
| `aksar-py` | Python bindings (PyO3) | v0.1.1, PyPI `aksar` |
| `aksar-bench` | OCR benchmark: CER, cluster CER, WER, speed and memory per line set and engine | v0.1.1 (not published) |
| `aksar-detect` | learned text detection for photos and complex layouts | planned (v0.3) |
| `aksar-legacy` or `aksar::legacy` | Limon/ABC → Unicode | planned (v0.3) |
| `aksar-layout` | line breaking (aksar segmentation), measurement | later |

## 4. Roadmap by version

### v0.1.0 — the text engine (2026-10-18)

Everything in the `aksar` row above. Tests: HarfBuzz golden shaping (45 lines), validator vs
HarfBuzz (6,000 in CI), normalize/repair/index_form idempotent and safe on the same data,
corrector equals the exhaustive algorithm on random OCR-like tokens, the earlier dictionary,
search and user-word tests. In a production Tesseract pipeline: Khmer CER 2.95% → 2.93% on
the 54-page S1 benchmark, no page worse, 18 other languages byte-identical. See [`CHANGELOG.md`](../CHANGELOG.md).

One track, in order. Each version is useful on its own; the Tesseract post-processing
improvements ship along the way.

| Version | Main content |
|---------|--------------|
| v0.1.0 ✅ | First release. Text engine: shaping, validation, normalization, dictionary + OCR corrector, search tokens, `read` for text ([`API.md`](API.md)); accuracy plan M0, M2 (first pair វ/រ), M3 (ជើងត/ជើងដ folding), M6 (visual-order vowels): Khmer CER 2.89% → 2.74% on S1. OCR groundwork: `aksar-render`, `aksar-image`, `aksar-decode`, `aksar-recog`, `aksar-ocr`, Python bindings, first real-document benchmark (EKYC). Tesseract fine-tune (M1) measured and not shipped: no gain on real documents |
| v0.1.1 ✅ | Maintenance: `Dictionary::contains` normalizes, OCR training pipeline up to the GPU run, `--model-dir`, linear-time `check` offsets, harfrust 0.15, CLI suggestions and help ([`CHANGELOG.md`](../CHANGELOG.md)) |
| v0.2.0 | `aksar-recog` print model + `aksar-decode` (grammar + dictionary) on lines; first head-to-head vs Tesseract |
| v0.3.0 | `aksar-detect` + `aksar-ocr` full pages; WASM OCR; engine option for apps; `aksar-legacy` (M7); handwriting sample collection starts |
| v0.4.0 | phone photos; print-accurate model |
| v0.5.0 | `hand-v1` handwriting model (beta); open handwriting dataset v1 |
| v1.0.0 | stable API; the OCR crates on crates.io; models and model cards |

Details, data plan and gates: [`OCR_PLAN.md`](OCR_PLAN.md) §5–§8.
Remaining work as a task list: [`CHECKLIST.md`](CHECKLIST.md).

### Later

- **M5, M8** (in the app's OCR engine, using aksar): stray Latin in Khmer lines, Khmer spacing.
- 5-font golden matrix; fuzzing fonts. (Done: 1,044-line corpus, fuzz targets for text, dictionary,
  shaping and page images.)
- `aksar-layout` (line breaking with aksar segmentation, measurement, caret positions).
- Multi-language: start with search for unspaced scripts and the probe-font validator
  ([`LANGUAGES.md`](LANGUAGES.md)) only when a measured need appears.
- Active-learning export (below), for apps that store review edits and user words.

## 5. Active-learning export format (later)

```
export/
  images/000001.png
  labels.txt          # image<TAB>text, UTF-8, text already normalized
  meta.jsonl          # optional, one JSON object per line
```

Labels pass `validate` and equal `normalize(label)`; images are line crops; nothing is
exported without the user's opt-in for that export.

## 6. Release process

1. All checks green: `cargo test --workspace --all-features`, `cargo test -p aksar
   --no-default-features`, clippy `-D warnings`, rustfmt, `cargo +1.85 check`, WASM size
   budgets, `cargo package -p aksar`.
2. Update `CHANGELOG.md`, bump `version` in the workspace `Cargo.toml` (both
   `[workspace.package] version` and the `aksar` entry in `[workspace.dependencies]`) and the
   `version = "…"` of the `aksar` path dependency in `crates/aksar-py/Cargo.toml`.
3. Tag `vX.Y.Z` on `main`.
4. Apps that store aksar output (search tokens, corrected text): rebuild indexes if the
   changelog says Khmer output changed.
5. Pushing the tag runs `.github/workflows/release.yml`. It builds abi3 Python wheels
   (Linux x86_64/aarch64, macOS arm64/x86_64, Windows x64), the sdist and the `aksar-wasm`
   npm package (default features, no `read`). It uploads them as workflow artifacts and
   publishes nothing.
6. Check the artifacts. To publish, run the Release workflow on the tag with `publish`
   checked. Publishing waits for the full CI suite on the tag.
   PyPI and npm both use trusted publishing (OIDC, environments `pypi` and `npm`), so no
   long-lived token is stored; npm adds `--provenance`.
7. crates.io stays manual: `cargo publish -p aksar`.

One-time setup before the first publish:

- Create the `pypi` and `npm` environments in the GitHub repo settings.
- Register a PyPI trusted publisher: `kongpda/aksar`, workflow `release.yml`, environment
  `pypi`.
- Register an npm trusted publisher: package `aksar-wasm`, repo `kongpda/aksar`, workflow
  `release.yml`, environment `npm`. The repo must be public for `--provenance`.

Versioning: 0.x minor versions may change APIs; patch versions never change output except to
fix a bug, and say so in the changelog.

## 7. Risks

| Risk | Mitigation |
|------|------------|
| harfrust API changes (pre-1.0) | Pin minor; thin wrapper; golden tests |
| Corrector changes correct text | No-harm corpus test at 0 changes; fix only unique edits |
| Validator drifts from shaper | Fuzz agreement test against HarfBuzz in CI |
| Overfitting to synthetic pages | Real-scan set in every accuracy gate |
| Font and data licensing | OFL fonts only; word lists with their licenses; own legacy tables |
