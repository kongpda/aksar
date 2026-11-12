# Remaining work checklist

Open work after v0.1.1, as tasks a contributor can pick up. Each item says where to start
and when it is done. Plans: [`PLAN.md`](PLAN.md) §4, [`OCR_PLAN.md`](OCR_PLAN.md)
§7–§8, [`KHMER_ACCURACY_PLAN.md`](KHMER_ACCURACY_PLAN.md).
**Updated:** 2026-11-12.

Ground rules for every item:

- Rust: stable (aksar-ocr, aksar-recog and aksar-bench need 1.94; the rest keep MSRV 1.85).
  Before a commit: `cargo fmt --check`, clippy `-D warnings`, the tests of the crates you
  touched.
- One fix or feature per commit, with a test that fails without it.
- Every accuracy claim is a measurement written into [`OCR_RESULTS.md`](OCR_RESULTS.md).

## 0. Before anything else

- [ ] CI green on `main` on every platform. Checked locally on 2026-11-10 (macOS): fmt,
  clippy `-D warnings`, workspace tests, feature variants, Python tests, `no_std` on
  `thumbv7em-none-eabihf` and `wasm32`, WASM size budgets, fuzz targets compile,
  `cargo package -p aksar`, actionlint. Only CI covers Linux and Windows, MSRV 1.85,
  `cargo deny`, the fuzz run, wasm-bindgen/npm and the release workflow.

## 1. v0.2.0: first aksar print model (needs a GPU)

The code path is complete and tested end to end on the CPU (`training/smoke_test.sh`):
training with `--device cuda|mps`, held-out checks, export with a PyTorch reference, rten
parity check, `aksar-bench --json`, the gate and packaging with a generated model card.
What is left needs a GPU, fonts, Tesseract and data: follow
[`training/README.md`](../training/README.md) "The v0.2 print model, step by step".

- [ ] Train the print line model (`training/train_crnn.py`) on `aksar-render` data (`real`
  and `noisy` modes) plus EKYC train crops. Keep the EKYC test split untouched.
  Done when: an ONNX model and alphabet JSON load in `aksar-recog`.
- [ ] Run `aksar-bench` on EKYC (S4) with engines aksar and `tesseract khm+eng`, all six
  tasks. Done when: the table is in `OCR_RESULTS.md` with CER, cluster CER, folded CER, WER,
  speed, model size and peak memory.
- [ ] Gate (OCR_PLAN §8): aksar CER below Tesseract on S4 print tasks, 100% of lines pass
  `validate`, no-harm corpus at 0 changes.
- [ ] Model card (data, fonts, licenses, measured CER, limits) and a release location with
  SHA-256 and size. Do not upload until asked.
- [ ] LM in the decoder: word or syllable n-gram from the ICU list, scored in
  `aksar-decode`'s beam. Done when: CER drops on S4 with no line worse in the no-harm set.
- [ ] Re-run `decode_time` on EKYC lines with the real model. The prefix-tree compaction
  (memory flat on long lines) was checked only on synthetic frames, in release on an M1 Pro:
  a 249-step line takes 39 ms (37 ms before compaction), a 400-step flat line 56 ms (59 ms).
- [ ] Per-word confidence (line confidence exists: `Candidate::confidence`,
  `OcrLine::confidence`).
- [ ] Test `aksar-ocr` end to end on real page images with the real model.

## 2. Data (blocks the gates)

- [ ] S2 real-scan set (accuracy plan M0): 50+ scanned pages, lines labelled, license
  recorded in the set's `manifest.json` (data stays out of git: `/data/` is ignored). Done
  when: `aksar-bench` loads it and both engines have numbers.
- [ ] S3 phone-photo set, same format.
- [ ] Spelling preference table for ជើងត/ជើងដ (M3): a reviewed list of which spelling each
  word uses. Only then may the corrector choose one automatically.
- [ ] More M2 confusion pairs, each confirmed on S2, not only on synthetic pages.
- [ ] No-harm corpus: grow it from 940 tokens with real-scan text once S2 exists.

## 3. v0.3.0

- [ ] `aksar-detect`: learned text detection for photos and columns. Bench: line-detection
  F1 in `aksar-bench`.
- [ ] `aksar-ocr` full pages (detect, then recognize lines, then read order).
- [ ] WASM OCR module. Gate: ≤ 1.2 MB gzip without the model, ≤ 150 ms per line.
- [ ] Engine option for apps behind a flag (OCR_PLAN §9). Default stays Tesseract until the §8
  gate passes.
- [ ] `aksar-legacy` (M7): Limon/ABC to Unicode. Needs our own font tables. Done when:
  legacy PDFs extract as text that passes `validate`.
- [ ] Start the handwriting collection (OCR_PLAN §5.4): consent form, partners, writer IDs.

## 4. v0.4.0 to v1.0.0

- [ ] v0.4.0: photo augmentation (perspective, lighting), print-accurate model, S3 numbers.
- [ ] v0.5.0: `hand-v1` beta, writer-disjoint S5 test set, open dataset v1.
- [ ] v1.0.0: stable API and `Page` type; publish the OCR crates and the CLI; CLI `aksar ocr`
  with text, JSON, hOCR and ALTO.
- [ ] API decisions still open before 1.0:
  - `UserWords::remove` is O(n) (documented); fine for hundreds of words.
  - `normalize` allocates per syllable; measure before optimizing.
- Done in 0.1.0: `#[non_exhaustive]` on output types, `dict::Cost` struct, `WithUserWords`
  inherent methods, `Shaper::set_size` returns `Result`, `Error::InvalidLanguage`,
  documented size limits, the JSON schema, `IssueKind::name`, Python `spell` (the old
  `check` is a deprecated alias), the README API parity table.
- Decided after 0.1.0 (2026-11-11 audit): `aksar-ocr` JSON is `"schema": 1` with its own
  `schema/page.schema.json`. `Dictionary::contains` (and the other `contains`) normalize
  their input like every other dictionary method; a listed word still costs one lookup, and
  `contains_normalized` is the single-lookup form for hot loops over normalized text.

## 5. Tests, fuzzing, benchmarks

- [ ] Fuzz: run each target for 1 hour on nightly with ASan
  ([`fuzz/README.md`](../fuzz/README.md)); fix every crash with a regression test. CI runs
  60 s per target (non-blocking).
- [ ] PNG decoder fuzz target (`aksar_ocr::decode_png`): blocked because rten builds as a
  dylib and does not link with coverage instrumentation. Option: move PNG decoding into a
  crate without rten.
- [ ] 5-font golden shaping matrix (Battambang, Kantumruy Pro, Moul, Noto Sans Khmer, Noto
  Serif Khmer) against HarfBuzz; fuzz with font bytes.
- [ ] `aksar-bench`: per-category breakdown (by folder or a `category` column), line-detection
  F1, Kiri-OCR as a third engine.
- [ ] Syllable DFA speed work (OCR_PLAN §12) if the decoder profile shows `validate` in the
  beam as a hot spot.
- [ ] WASM shaping times in the README date from 2026-10-18; add a small node timing script
  and re-measure.
- [ ] `fuzz/fuzz_targets/shape.rs` unwraps `shape`: kept on purpose (shaping should never fail
  on text with a valid font). If harfrust returns `Error::Shape` on some input, decide whether
  that is a harfrust bug or an allowed error before relaxing the target.

## 6. Dependencies

- Done 2026-11-11: harfrust 0.15 and skrifa 0.49, upgraded together (they share read-fonts;
  fuzz/Cargo.lock too). The small shaping WASM grew from 845 KB to 988 KB, so its budget is
  now 1 MiB (fast: 1.25 MiB). Budgets may grow again on purpose for a dependency upgrade.

## 7. Later

- [ ] Vision-first (OCR-free) models as an optional feature ([`OCR_PLAN.md`](OCR_PLAN.md) §3.1):
  first measure an open vision model on S4 with `aksar-bench` (needs a GPU). If it reads
  Khmer well, use it to draft labels for real scans and handwriting, then consider an opt-in
  desktop fallback for low-confidence lines. aksar's own OCR-first engine stays the default.
- [ ] `aksar-layout` (line breaking with aksar segmentation, measurement, carets).
- [ ] Active-learning export (PLAN §5) for apps that store review edits.
- [ ] Multi-language search for unspaced scripts ([`LANGUAGES.md`](LANGUAGES.md)), only when
  a measured need appears.
