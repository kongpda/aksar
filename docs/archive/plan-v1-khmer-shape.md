> Archived v1 plan (latest of the 10 stacked copies in the original file), kept for history. See ../PLAN.md for the current plan and ../REVIEW.md for what changed.

# khmer-shape — Implementation Plan

**Project Name:** khmer-shape  
**Goal:** A small, pure-Rust text shaping and rendering foundation that fully supports all Khmer fonts, words, signs (vowel signs, diacritics, coeng stacks), and numbers. Cross-platform (desktop, mobile, embedded) + first-class WebAssembly. Commercial-ready (can be used and sold in proprietary apps) with an optional open-source path.

**Last updated:** 2026-10-18 (renamed to khmer-shape)

---

## Executive Summary

**khmer-shape** is a **tiny, pure-Rust, WASM-first** text shaping library built on `harfrust` (MIT). The core ship binary is shaping-only (optionally + learning) and stays small. OCR is an optional, hybrid, downloadable layer:

- Own lightweight post-processor + Learning Engine (NiDA-aware)
- Tesseract baseline (printed)
- Optional quantized ONNX models (printed and handwritten)

Handwritten Khmer uses the **same orthographic structure** as the NiDA keyboard. Visual recognition improves via synthetic data and an active-learning loop with user corrections. All platforms (native, WASM, mobile) are supported; large models and traineddata are never in the default binary.

---

## 1. Recommendation & Rationale

### Primary Engine: `harfrust` (MIT License)

| Criterion              | harfrust                          | allsorts                          | Notes |
|------------------------|-----------------------------------|-----------------------------------|-------|
| License                | MIT (fully commercial-friendly)  | MIT                               | Both allow proprietary use, modification, and selling. |
| Maintenance            | Official HarfBuzz port; frequent updates matching latest HarfBuzz; maintained under harfbuzz org + Google Fonts team | Actively maintained by YesLogic (Prince) | harfrust is the most “upstream” and frequently updated. |
| Popularity / Adoption  | Used by cosmic-text, high download volume, growing ecosystem | Solid but smaller footprint       | Prefer the one with strongest long-term trajectory. |
| Khmer Support          | Full complex shaper (pref, blwf, abvf, pstf, pres, etc.) | Full Khmer support since ~0.6   | Both correct; harfrust tracks HarfBuzz tests closely. |
| Size & WASM            | Pure Rust, `default-features = false`, no_std friendly | Pure Rust, good for subsetting   | Both excellent; we will optimize further. |
| Extra Features         | Shaping only (clean separation)  | Parsing + shaping + subsetting   | Subsetting is useful later for shipping tiny fonts. |

**Decision:** Build on **harfrust** as the core shaper. It is the most popular, frequently updated, and officially aligned option with a commercial-friendly MIT license. We can optionally pull in allsorts later for subsetting if needed.

**License strategy for our product:**
- Our library/wrapper: MIT or MIT-OR-Apache-2.0 (recommended for dual commercial + open-source).
- This allows us to sell a proprietary app that embeds it, open-source the engine later, or dual-license if we ever want.
- Fonts: Use open fonts (Noto Sans Khmer, Battambang, etc.) or licensed commercial fonts. Subset aggressively.

---

## 1.5 Khmer Unicode Input Model (NiDA Standard Keyboard)

The official **NiDA Standard Khmer Unicode Keyboard** (V1.0) defines how users actually type Khmer. This is critical for shaping correctness, learning-engine normalization, and OCR post-processing (both printed and handwritten).

**Important insight:** Handwritten Khmer follows the **same orthographic structure** as the NiDA keyboard. The Unicode creators based the encoding and keyboard on traditional Khmer spelling rules. Therefore:

- Handwritten text still consists of base consonants + coeng (subscripts) + vowel signs + final marks.
- The logical Unicode sequence after recognition should match what a user would type on the NiDA keyboard.
- Our NiDA-aware validator, normalizer, and learning engine apply equally to both printed and handwritten recognition output.
- The extra difficulty of handwriting is primarily **visual** (stroke variation, connection, style), not a different linguistic model.

### Core composition rules (from the keyboard diagram)
- **Coeng (subscript)**: Type base consonant + `U+17D2` (KHMER SIGN COENG, invisible stacker) + following consonant.
  - Example from diagram: `ក + ្ + ក → ក្ក`
  - Example: `គ + ្ + រ + ា → ក្រា` (and similar multi-part sequences)
- **Order of input** (logical / storage order):
  1. Base consonant (or independent vowel)
  2. Coeng + consonant (repeat for multiple subscripts, in spelling order)
  3. Register shifters (if any)
  4. Dependent vowel signs
  5. Final signs (nikahit, reahmuk, bantoc, etc.)
- **Special notes from the diagram**:
  - Divination signs and Lunar dates: use Ctrl + AltGr.
  - ZWJ / ZWNJ available for ligation control.
  - Khmer digits (០–៩) on the number row.
  - Full coverage of consonants, independent vowels, dependent vowels, and punctuation.

### Implications for our project
- **Shaping**: `harfrust` already implements the correct Khmer OpenType shaper for these sequences. We must test against real NiDA-typed strings.
- **Learning / Correction Engine**: Normalize user input and OCR output (printed or handwritten) toward valid coeng sequences. Prefer modern Khmer conventions. Users correcting handwriting recognition are effectively teaching the system the same NiDA-style sequences.
- **OCR post-processor**: Map common mis-recognitions back to correct `base + U+17D2 + consonant + vowel` sequences. Especially important for handwriting where recognition is noisier, but the target structure is identical.
- **Reading / validation**: Provide a lightweight validator that checks whether a string follows NiDA-compatible syllable structure (works for both typed and recognized text).

**Reference**: NiDA Standard Khmer Unicode Keyboard diagram (user-supplied) + Unicode Technical Note #61 and OpenType Khmer shaping documents.

---

## 1.6 Best Ways to Improve Visual Recognition (Size-Aware)

Visual recognition (OCR) improves most effectively through better data and feedback, not by embedding large models in the ship binary. Recommended priority order:

1. **Active learning loop (highest ROI)**  
   Use the existing Learning / Correction Engine. Every time a user corrects an OCR result, store the image snippet + corrected NiDA-style text. Periodically export anonymized pairs to fine-tune the downloadable model. This continuously improves both printed and handwritten accuracy without increasing the core binary size.

2. **Synthetic data generation**  
   Generate large volumes of labeled Khmer text-line images using real fonts (Noto Sans Khmer, Battambang, etc.) plus handwriting-style augmentations (stroke jitter, thickness variation, connection noise, elastic distortion). Modern pipelines (SynthTIGER-style or simple Pillow/skia rendering) can produce millions of examples cheaply. This is how the best current Khmer models (e.g., Netra-OCR) were trained.

3. **Downloadable quantized ONNX models**  
   Ship a tiny core. Offer optional quantized CRNN or lightweight Transformer models (via `ort` crate or pure-Rust inference) that users download on demand. Quantization (int8) keeps models small enough for mobile and WASM. Separate models for printed vs handwritten if needed.

4. **Lightweight pure-Rust preprocessing**  
   Improve input quality before recognition: deskew, adaptive thresholding, line segmentation, contrast normalization. These steps are small, pure Rust, and help every backend (Tesseract, AI model, handwritten).

5. **Error-focused post-processing**  
   Expand the own post-processor with rules and dictionary fixes derived from real user corrections and common Khmer confusion pairs (similar-looking consonants, coeng misplacements, vowel sign errors). This is tiny and works everywhere.

**What to avoid in the ship binary:** Large vision models, full Tesseract traineddata, or unquantized networks. Keep them optional and downloadable.

---

## 1.7 Recommended Synthetic Data Generator

**Best practical approach for Khmer:**

A lightweight pure-Rust (or Python for initial prototyping) text-line renderer that:

1. **Source text**: Khmer corpus (Wikipedia dumps, news, dictionary entries, common phrases). Prefer real text over random characters so coeng stacks and vowel combinations appear naturally.
2. **Fonts**: Multiple OFL Khmer fonts (Noto Sans Khmer, Battambang, Siemreap, etc.). Vary size, weight, and slight tracking.
3. **Rendering**: Use a simple canvas (skia-safe, tiny-skia, or Pillow in Python prototype) to render each line at fixed height (e.g., 48–64 px) with random padding.
4. **Augmentations** (applied after rendering):
   - Gaussian noise, salt-and-pepper
   - Slight blur, motion blur
   - Elastic distortion / stroke jitter (for handwriting style)
   - Perspective / slight rotation
   - Contrast and brightness variation
   - Background texture or solid color variation
5. **Output format**: One image per line + a simple labels file (see section 1.8).

**Recommended tools / starting points:**
- Python prototype: `khmerocr-tools` or a minimal Pillow script (fast to iterate).
- Production: Port the generator to Rust (tiny-skia or skia-safe) so it can run offline and stay in the same toolchain.
- For more complex layouts later: SynthTIGER-style or SynthDoG-inspired pipeline, but start with clean text lines.

**Target**: Generate 100k–1M labeled lines covering common and rare coeng sequences. This is the foundation for training or fine-tuning the downloadable ONNX models.

---

## 1.8 Recommended Active-Learning Export Format

**Simple, portable format** that works with most OCR training pipelines (PaddleOCR, CRNN, custom ONNX training):

### Directory structure
```
active-learning-export/
  images/
    000001.png
    000002.png
    ...
  labels.txt
```

### labels.txt format (tab-separated)
```
images/000001.png	ខ្មែរ
images/000002.png	សួស្តី
images/000003.png	ក + ្ + ក → ក្ក
```

- One line per sample: `relative_image_path<TAB>corrected_text`
- Text is the final NiDA-normalized Unicode string (after user correction).
- Images are cropped text-line snippets (not full pages).
- UTF-8 encoding.

### Optional metadata (JSON sidecar or extra columns)
```json
{
  "id": "000001",
  "original_ocr": "ខ្មែរ",
  "corrected": "ខ្មែរ",
  "source": "user",
  "font_hint": "NotoSansKhmer",
  "is_handwritten": false,
  "timestamp": "2026-10-18T10:00:00Z"
}
```

**Why this format:**
- Compatible with PaddleOCR SimpleDataSet and most open-source CRNN trainers.
- Easy to generate from the Learning Engine database.
- Easy to filter, merge, or convert to LMDB later if needed.
- Privacy-friendly: only store what the user explicitly corrected and opted to export.

**Export flow:**
1. User corrects text in the app.
2. Learning Engine stores the correction locally.
3. On opt-in export, generate the `images/` + `labels.txt` package.
4. Use the package to fine-tune the next version of the downloadable ONNX model.

---

## 2. High-Level Architecture

```
┌─────────────────────────────────────────────┐
│             Application Layer               │
│  (Native UI, Web, Mobile, Desktop, CLI)     │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│         khmer-shape Public API              │
│  - shape(text, font, options) → GlyphRun    │
│  - layout / measure                         │
│  - optional rasterize                       │
│  - optional: correct(text) via Learning     │
│  - optional: ocr(image) → text (then shape) │
│  - optional: validate_khmer_sequence(text)  │
│  - WASM bindings + C/FFI optional           │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│     Optional Layers (feature-gated)         │
│  - Learning / Correction Engine             │
│  - OCR Pipeline (hybrid, size-aware)        │
│    • Tiny own post-processor / bug fixes    │
│    • Tesseract (baseline / fallback)        │
│    • Optional AI model (ONNX, downloadable) │
│    • Optional handwritten model             │
│  - NiDA-compatible sequence validator       │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│              Core Engine                    │
│  - harfrust (shaping)                       │
│  - read-fonts / skrifa (font parsing)       │
│  - Optional: swash or tiny rasterizer       │
│  - Unicode helpers (bidi, linebreak)        │
└─────────────────────────────────────────────┘
```

**Key principles:**
- Pure Rust core, no C/C++ dependencies in the minimal / ship build.
- Feature flags for size control (`shaping-only`, `raster`, `wasm`, `learning`, `ocr-lite`, `ocr-tesseract`, `ocr-ai`, `ocr-handwritten`, `no_std`).
- Ship version is as small as possible: core shaping (+ optional tiny learning) is the default; OCR is opt-in.
- Small public API focused on Khmer correctness first.
- Deterministic output for shaping.
- Learning and OCR are optional layers that never bloat the core.
- All text processing respects NiDA coeng-based composition rules (same for typed and handwritten).
- Handwritten support is optional and harder visually; the linguistic target remains the same NiDA structure. Models stay downloadable.
- Visual recognition improves primarily via data (synthetic + user corrections) and downloadable models.

---

## 3. Phased Implementation Plan

### Phase 0: Foundation & Research (1–2 weeks)
**Deliverables:**
- Repository setup (Cargo workspace).
- License files (MIT + optional Apache-2.0 dual).
- Dependency audit and pin strategy.
- Khmer test corpus collection (Wikipedia extracts, news, names, edge cases, digits).
- Baseline binary/WASM size measurement of raw harfrust.
- Initial evaluation of Tesseract Khmer (`khm`) accuracy on sample images.
- Collect common Khmer OCR error patterns for later own fixes.
- Extract and document NiDA keyboard composition examples as golden tests.
- Survey existing handwritten Khmer datasets (e.g., KHT) and research results.

**Tasks:**
- Create `khmer-shape` crate (library) + `khmer-shape-cli` + `khmer-shape-wasm`.
- Add harfrust with `default-features = false`.
- Collect and version a set of test fonts (Noto Sans Khmer, one system font, one variable if desired).
- Document Khmer Unicode ranges and OpenType feature order.
- Add NiDA-derived test cases (coeng sequences, multi-subscript, vowel + coeng combinations).

**Exit criteria:** Clean workspace builds for native + wasm32-unknown-unknown; first “hello Khmer” shapes correctly; NiDA examples pass.

---

### Phase 1: Core Shaping Engine (2–3 weeks)
**Goal:** Correct, complete Khmer shaping for words, signs, coeng stacks, and numbers, matching real NiDA input.

**Features:**
- Full Khmer block (U+1780–U+17FF) + Khmer digits (U+17E0–U+17E9) + symbols.
- Proper syllable analysis, reordering, and feature application (locl, ccmp, pref, blwf, abvf, pstf, cfar, pres, abvs, blws, psts).
- Script = KHMER, language = khr / km.
- Support for multiple fonts (TTF/OTF, variable fonts via instancing).
- Glyph clustering, advances, offsets, and mark attachment.
- Error handling for malformed fonts / missing glyphs.
- Correct handling of `base + U+17D2 + consonant` sequences (and longer stacks).

**Implementation notes:**
- Thin wrapper around harfrust `shape`.
- Public types: `ShapedGlyph`, `GlyphRun`, `ShapeOptions`.
- Scale handling (units-per-em → pixels/points).
- Cache shaped results for repeated strings (optional, feature-gated).

**Testing:**
- Unit tests against known good outputs (compare with HarfBuzz CLI or browser).
- Corpus tests for real-world Khmer text.
- Property tests for random valid Khmer sequences.
- Golden tests derived from the NiDA keyboard diagram examples.

**Exit criteria:** All standard Khmer test sequences (including NiDA examples) render identically to HarfBuzz / browser reference.

---

### Phase 1.5: Learning / Correction Engine (optional, 1–2 weeks)
**Goal:** Allow users to flag and improve incorrect words, preferred spellings, or rendering issues. Keep this completely separate from the core shaper so the minimal binary stays tiny. Also serves as a post-OCR correction dictionary. Normalizes toward valid NiDA-style sequences. Especially valuable for noisy handwritten OCR output, where users correct toward the same keyboard-style sequences. This engine is the primary source of real-world training data for visual recognition improvements.

**Features:**
- User corrections: original text + context → preferred form / normalized text.
- Learned words / custom dictionary (frequency, preferred spelling, coeng variants).
- Font preferences and user-added fonts.
- Local storage (embedded DB).
- Optional anonymized export for improving bundled data.
- Pre-shaping normalization pass that applies learned rules before calling harfrust.
- Post-OCR correction pass (printed and handwritten).
- Optional canonicalization of coeng sequences (modern Khmer conventions).
- Store image snippets with corrections for later model fine-tuning (opt-in).
- Export in the recommended active-learning format (section 1.8).

**Recommended storage (feature-gated):**
- Pure-Rust options preferred for WASM: `redb`, `sled`, or a simple file-based JSON/SQLite.
- Native can use `rusqlite` if desired.
- Schema sketch:
  - `corrections`: id, original_text, font_hash, preferred_output, votes, created_at
  - `learned_words`: word, normalized_form, frequency, source
  - `fonts`: id, name, hash, path_or_bytes, is_user_added

**Integration:**
- `CorrectionLayer::apply(text) -> String` runs before shaping and after OCR.
- UI hook: “This word looks wrong → suggest correction”.
- Privacy: all data local by default; sync is opt-in.

**Exit criteria:** Users can add a correction and see it applied on subsequent shapes of the same text. NiDA-invalid sequences can be normalized. Export produces a valid `images/` + `labels.txt` package.

---

### Phase 2: Font Management & Subsetting (1–2 weeks)
**Goal:** Ship small, self-contained fonts and handle font loading robustly.

**Features:**
- Load fonts from bytes, file, or system (native).
- Font subsetting (optional dependency on allsorts or external tool) to reduce size.
- Preferred font fallback list for Khmer.
- Metadata extraction (name, supported scripts, metrics).
- Variable font axis support (wght, etc.).
- Integration with learning layer for user-preferred fonts.

**Size strategy:**
- Bundle one or two subsetted Noto Sans Khmer variants.
- Target: subset font < 100–200 KB for common text.

---

### Phase 3: Layout, Measurement & Optional Rasterization (2 weeks)
**Goal:** From shaped glyphs to usable layout and pixels.

**Features:**
- Simple single-line and multi-line layout (using unicode-linebreak).
- Measurement (width, height, baseline).
- Bidirectional text support (Khmer is LTR but mixed content happens).
- Optional rasterization path:
  - Lightweight: output glyph outlines or use a minimal coverage rasterizer.
  - Full: optional `swash` feature.
- For WASM: primarily emit glyph IDs + positions so the host (Canvas/WebGL) can draw.

**Exit criteria:** Can measure and layout paragraphs of Khmer text; optional pixel output matches reference.

---

### Phase 4: WebAssembly & Cross-Platform Bindings (2–3 weeks)
**Goal:** First-class WASM + easy integration on all platforms. Ship version remains as small as possible.

**WASM features:**
- `wasm-pack` target (`web` and `bundler`).
- Clean JS/TS API: `shape(text: string, fontBytes: Uint8Array, options) → GlyphRun`.
- Minimal glue, no heavy dependencies.
- Size optimization: `opt-level = "z"`, LTO, strip, panic=abort.
- Optional WebGPU/Canvas helper examples.
- Learning engine WASM path uses pure-Rust DB only.
- OCR backends that are not WASM-friendly are excluded from the default WASM build.

**Other platforms:**
- Native Rust API (primary).
- Optional C FFI (for non-Rust languages).
- Mobile notes: UniFFI or similar for Swift/Kotlin if needed later.
- no_std / embedded path (feature flag) — shaping only.

**Size targets (release, optimized):**
- Shaping-only WASM (ship default): aim for < 200–400 KB (gzipped much smaller).
- With learning layer: still keep under ~600 KB if possible.
- OCR (including handwritten) is never in the default ship binary.

---

### Phase 4.5: Hybrid OCR Pipeline — Own Improvements + Tesseract + Optional AI + Handwritten (optional, 3–4 weeks)
**Goal:** Provide accurate Khmer OCR for both printed and handwritten text while keeping the ship version tiny. Take advantage of Tesseract where it helps, add our own bug fixes and improvements, optionally layer a small AI model, and support handwritten as a separate optional path. All output is normalized to valid NiDA-style Unicode sequences (same structure as keyboard typing).

**Strategy (size-first hybrid):**

1. **Own lightweight post-processor / bug-fix layer (always preferred in ship if OCR is enabled)**  
   - Rule-based and dictionary-driven fixes for common Khmer OCR errors (coeng misreads, vowel sign confusion, digit mix-ups, known font-specific issues).  
   - Uses the Learning / Correction Engine as the source of truth.  
   - Pure Rust, tiny, works everywhere including WASM.  
   - Continuous improvement via user corrections.  
   - Explicitly maps OCR output toward `base + U+17D2 + consonant + vowel` sequences.  
   - Works for both printed and handwritten output because the target orthography is the same.

2. **Tesseract as baseline / fallback (printed text)**  
   - Official `khm` support exists (Apache-2.0).  
   - Use via existing Rust bindings on native platforms.  
   - WASM possible via `tesseract-wasm` but traineddata is large — load on demand.  
   - Accuracy moderate on printed text; weak on handwriting.

3. **Optional AI improvement for printed text (downloadable)**  
   - Small ONNX model (quantized CRNN or lightweight Transformer).  
   - Feature flag: `ocr-ai`.  
   - Never in default ship binary.

4. **Optional handwritten path (harder visually, same linguistic model)**  
   - Handwritten Khmer is harder because of stroke variation, connection, and individual style — but the underlying orthography matches the NiDA keyboard.  
   - Existing research (e.g., UKTR) shows specialized training can reach ~6% CER on handwritten datasets such as KHT.  
   - Recommended approach: separate downloadable ONNX model fine-tuned or trained for handwriting.  
   - Feature flag: `ocr-handwritten`.  
   - Strongly rely on the Learning / Correction Engine so users can correct remaining errors toward proper NiDA sequences.  
   - Output still forced through NiDA-style normalization.  
   - Not included in any default ship binary; load on demand only.  
   - WASM support possible if the model is quantized and small enough, otherwise native-first.

**Recommended ship configuration:**
- Default: shaping only (or shaping + learning).
- Optional OCR lite: own post-processor + learning dictionary (tiny, works for both printed and as post-fix for handwriting).
- Optional full printed OCR: Tesseract or downloadable AI model.
- Optional handwritten: separate downloadable model + learning corrections.
- Never include large models or traineddata in the default binary.

**Tasks:**
- Build the pure-Rust Khmer OCR error-correction rules + dictionary integration (NiDA-aware).
- Prototype Tesseract (`khm`) integration behind a feature flag.
- Design ONNX model interface for both printed and handwritten.
- Evaluate size/accuracy trade-offs for a handwritten model.
- Ensure all OCR paths feed into the same shaping + learning pipeline.
- Document platform support matrix.
- Implement the synthetic data generator (section 1.7) and active-learning export (section 1.8).

**Exit criteria:** Clear size numbers for each OCR variant; own post-processor reduces common errors; optional handwritten path works on supported platforms; all output is NiDA-valid; synthetic generator and export format are functional.

---

### Phase 5: Optimization, Robustness & Production Hardening (2 weeks)
**Goal:** Ready to ship in a commercial app. Ship version as small as possible.

**Tasks:**
- Binary size audit and feature-flag pruning (especially OCR and handwritten).
- Performance benchmarks (shaping throughput, cold start, OCR latency).
- Memory usage profiling.
- Fuzzing (font + text + OCR images).
- Edge-case fixes (malformed fonts, extremely long strings, mixed scripts, common OCR bugs, handwriting noise).
- Deterministic builds.
- Documentation (rustdoc + user guide).
- Examples: CLI, web demo, simple GUI.
- Learning engine privacy review and export controls.
- Platform matrix verification (WASM, desktop, mobile).
- NiDA sequence validator tests.

**Commercial readiness:**
- Clear license headers and NOTICE file.
- Dependency license audit (cargo-deny or similar).
- No GPL/LGPL contamination.
- Security review for untrusted font data, user-supplied corrections, and optional models.

---

### Phase 6: Packaging, Docs, Open-Source Option & Ship (1–2 weeks)
**Deliverables:**
- crates.io-ready library (if open-sourcing).
- npm package for WASM (if web).
- Comprehensive README, API docs, migration guide.
- Test suite and CI (GitHub Actions: native + WASM + size checks).
- Example applications demonstrating full Khmer support.
- Decision point: open-source the core under MIT/Apache-2.0 or keep proprietary with commercial license.
- Optional: publish learning-engine schema, starter dictionary, and OCR correction rules.
- Separate downloadable OCR models / traineddata packages (printed and handwritten).
- Include NiDA keyboard reference and composition guide in docs.
- Note on handwritten limitations and recommended use of the learning engine.
- Document the synthetic data generator and active-learning export format.

**Open-source path (recommended if we want community + future-proofing):**
- Dual license or pure MIT.
- Contribution guidelines, code of conduct.
- This keeps the commercial app free to use the same code.

---

## 4. Full Feature List (Ship Checklist)

### Must-have (v1.0 ship)
- [ ] Complete Khmer shaping (words, coeng stacks, vowel signs, diacritics)
- [ ] Khmer digits and mixed numbers
- [ ] Multiple fonts + subsetting path
- [ ] Cross-platform native
- [ ] WebAssembly with clean JS API
- [ ] Small binary size (optimized profiles, shaping-first)
- [ ] Deterministic, well-tested output
- [ ] Commercial-friendly licensing
- [ ] Basic layout/measurement
- [ ] Correct handling of NiDA coeng sequences

### Optional (feature-gated, not in default ship binary)
- [ ] Learning / Correction Engine (NiDA-aware normalization)
- [ ] Own lightweight OCR post-processor / bug fixes
- [ ] Tesseract baseline OCR (native; on-demand WASM)
- [ ] Optional AI OCR model for printed text (downloadable ONNX)
- [ ] Optional handwritten Khmer OCR model (downloadable)
- [ ] NiDA sequence validator
- [ ] Full multi-line layout + line breaking
- [ ] Optional high-quality rasterization
- [ ] Variable fonts
- [ ] Font fallback chains
- [ ] Caching layer
- [ ] C FFI / UniFFI
- [ ] Color fonts / emoji
- [ ] Performance counters / tracing
- [ ] Synthetic data generator
- [ ] Active-learning export

---

## 5. Size & Performance Optimization Guidelines

```toml
# Cargo.toml release profile (library + WASM)
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
overflow-checks = false
```

- Prefer feature flags over always-on dependencies.
- Subset fonts aggressively.
- Measure with `twiggy`, `cargo bloat`, and `wasm-opt`.
- For WASM: use `wasm-opt -Oz`.
- Learning, Tesseract, AI OCR, and handwritten models must be feature-gated so the core ship binary stays small.
- Large assets (traineddata, ONNX models) are downloaded on demand, never embedded by default.

---

## 6. Testing Strategy

1. **Unit tests** — known sequences (simple syllables, coeng, stacked vowels).
2. **Corpus tests** — real Khmer Wikipedia / news excerpts.
3. **Reference comparison** — HarfBuzz CLI and browser rendering.
4. **Font matrix** — at least 3–5 different Khmer-capable fonts.
5. **Fuzzing** — random text + malformed fonts.
6. **WASM integration tests** — shape in browser and compare pixel or glyph output.
7. **Size regression CI** — fail if default binary grows beyond budget.
8. **Learning engine tests** — corrections apply correctly and do not break shaping.
9. **OCR evaluation** — CER/WER on held-out Khmer images for each path (own post-processor, Tesseract, optional AI, handwritten).
10. **Platform matrix** — native, WASM, and (later) mobile smoke tests.
11. **NiDA golden tests** — all composition examples from the official keyboard diagram.
12. **Handwritten evaluation** — separate CER on handwriting samples; measure improvement from learning engine corrections toward NiDA sequences.
13. **Synthetic data tests** — generated images render correctly and labels match.
14. **Export format tests** — active-learning package is valid and loadable by common trainers.

---

## 7. Risk & Mitigation

| Risk                          | Mitigation |
|-------------------------------|------------|
| harfrust API changes          | Pin version, track changelog, thin wrapper |
| Missing rare Khmer sequences  | Continuous corpus expansion + community reports + NiDA tests |
| WASM size creep               | Strict feature flags + CI size checks; OCR never default |
| Font licensing                | Prefer OFL fonts; subset; document |
| Performance on low-end devices| Benchmarks + optional caching |
| Learning data privacy         | Local-only by default; opt-in sync; clear docs |
| Tesseract Khmer accuracy      | Use as baseline only; own post-processor + learning; optional AI model |
| AI model size / WASM          | Downloadable models; feature-gated; pure-Rust lite path preferred |
| Cross-platform OCR gaps       | Clear matrix; lite path works everywhere |
| Invalid coeng order           | NiDA-aware validator + normalization in learning/OCR layers |
| Handwritten accuracy          | Optional downloadable model; heavy reliance on learning engine corrections toward same NiDA structure; set user expectations |

---

## 8. Timeline Summary (Aggressive but Realistic)

| Phase     | Duration   | Cumulative |
|-----------|------------|------------|
| 0         | 1–2 weeks  | 2 weeks    |
| 1         | 2–3 weeks  | 5 weeks    |
| 1.5 (opt) | 1–2 weeks  | 7 weeks    |
| 2         | 1–2 weeks  | 9 weeks    |
| 3         | 2 weeks    | 11 weeks   |
| 4         | 2–3 weeks  | 14 weeks   |
| 4.5 (opt) | 3–4 weeks  | 18 weeks   |
| 5         | 2 weeks    | 20 weeks   |
| 6         | 1–2 weeks  | 22 weeks   |

**MVP (core shaping + WASM + basic layout) target: ~8–10 weeks.**  
**With learning engine: ~10–12 weeks.**  
**Production-ready ship (tiny core + optional OCR paths including handwritten): ~18–22 weeks** with testing and hardening.

---

## 9. Next Immediate Actions

1. Create the Cargo workspace and add harfrust.
2. Write the first integration test with a real Khmer string and Noto font.
3. Measure baseline WASM size.
4. Decide open-source vs proprietary packaging early (affects CI and docs).
5. Collect a small set of Khmer images and run stock Tesseract (`khm`) to establish a baseline CER and collect common error patterns.
6. Design the CorrectionLayer interface so it can be added later without changing the core API.
7. Sketch the pure-Rust OCR post-processor rules based on observed Tesseract errors.
8. Add the NiDA keyboard composition examples as the first golden test suite.
9. Survey available handwritten Khmer datasets and estimate model size for a downloadable handwritten path.
10. Prototype a simple synthetic data generator for Khmer text lines (fonts + basic augmentations).
11. Implement the active-learning export format (images/ + labels.txt).

---

**This plan prioritizes a tiny, pure-Rust, WASM-friendly core shaper on `harfrust`. OCR is an optional, hybrid, size-aware layer that covers printed text (own fixes + Tesseract + optional AI) and handwritten text (optional downloadable model). Handwritten Khmer follows the same NiDA orthographic structure as keyboard typing, so the learning engine and normalizer apply equally. Visual recognition improves primarily through synthetic data and an active learning loop with user corrections. The ship version stays as small as possible while supporting all major platforms.**

Ready for implementation.  
*Official project name: **khmer-shape** — 2026-10-18.*
