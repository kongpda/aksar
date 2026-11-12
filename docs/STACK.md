# Recommended dependency stack

> **v0.1.0 uses only `harfrust` (optional `shape` feature); the core has no dependencies.**
> The rows below are measured choices for later crates (`aksar-render`, the OCR engine,
> bindings). The OCR engine (rten) is planned in [`OCR_PLAN.md`](OCR_PLAN.md); it is preferred over
> Tesseract only after beating it on that plan's gates.
>
> **What v0.1.0 actually uses** (where it differs from the rows below): `aksar-recog` runs rten
> 0.27 on **ONNX** files (feature `onnx_format`, no conversion to `.rten`); the recognition
> model is aksar's own CRNN from `training/train_crnn.py`, not the ocrs-models pipeline;
> preprocessing is `aksar-image` with no dependencies (rten-imageproc is not used); PNG
> decoding uses `png` 0.18 in `aksar-ocr`. No detection model is used yet (lines are found by
> projection).

Goal: every platform (Windows, macOS, Linux, iOS, Android, embedded) **and** WebAssembly,
small, fast and accurate. Every choice below is pure Rust (no C/C++ toolchain needed on any
target) and permissively licensed. Numbers were measured on 2026-10-18 (4-core Linux, Node 22
for WASM, `opt-level=3`, fat LTO, `panic=abort`) unless marked otherwise.

**Rule that drove most choices: one font stack.** `harfrust` 0.15 uses `read-fonts` 0.46 (0.14 used 0.45; upgraded 2026-11-11).
Anything that pins an older `read-fonts`/`skrifa` (swash, parley, cosmic-text) compiles a
second copy of the font parser, costing size and doubling font memory.

## Summary

| Need | Use | Instead of (plan / common choice) | Why (measured) |
|------|-----|-------------------------------|----------------|
| Shaping | **harfrust 0.15** | rustybuzz, allsorts | HarfBuzz port, glyph-exact vs HarfBuzz on our corpus. rustybuzz's last release was 2024-11. |
| Font parsing | **read-fonts 0.45** (via harfrust) | ttf-parser | Shared with shaper; ttf-parser's last release was 2024-11. |
| Glyph outlines + hinting | **skrifa 0.49** | swash | Same `read-fonts` as harfrust. swash pins skrifa ≤ 0.44 → **two font parsers**. Outlines add +67 KB gzip. |
| Rasterizing (native, synth data) | **zeno 0.3** | tiny-skia | +51 KB gzip on top of outlines vs +86 KB for tiny-skia. Same rasterizer swash uses. |
| Rasterizing (web) | **Browser Canvas `Path2D`** from skrifa outlines | any WASM rasterizer | 0 KB extra; GPU-accelerated, matches browser text AA. |
| Word segmentation / line breaking | **icu_segmenter 2.3, dictionary mode, Khmer-only data blob** | unicode-linebreak (plan), LSTM mode | Precision **1.00**, recall 0.83 on hand-segmented sentences, **42 MB/s** native. LSTM: 0.97 / 0.89 but **0.4 MB/s** (100× slower). unicode-linebreak can't split Khmer at all. |
| Script runs | **unicode-script 0.5** | icu_properties | Tiny; only needed for mixed-script text. |
| Bidi (mixed Arabic/Hebrew only) | **unicode-bidi 0.3**, optional feature | — | Khmer is LTR; keep off by default. |
| Serialization / correction store | **serde + postcard 1.1** | sled, redb, SQLite (plan) | no_std, tiny, runs in WASM; the app persists the bytes (IndexedDB / file). sled is unmaintained; redb/SQLite need a filesystem. |
| Fuzzy dictionary (spelling suggestions) | **fst 0.4** (+ Levenshtein automaton) | hash maps, SymSpell | Compact, memory-mappable, used by tantivy; fuzzy lookup without a big index. Stable since 2021. |
| OCR inference | **rten 0.27** | tract, ort (plan) | See table below: 4.5× smaller WASM than tract, 1.55× faster natively. ort needs C++ and has no WASM. |
| OCR models | **ocrs** detection model + own Khmer recognition model, trained with the ocrs-models pipeline, run on rten | Tesseract | rten-native format; detection model 2.5 MB; recognition must be trained for Khmer (the ocrs one is Latin). |
| Image decode (native) | **png 0.18 + zune-jpeg 0.5** | image | Only the two codecs needed; pure Rust. |
| Image decode (web) | **Browser `createImageBitmap`** → RGBA into WASM | any decoder in WASM | 0 KB extra. |
| Image preprocessing | Own small code (gray, Sauvola, deskew) + **rten-imageproc** | imageproc | rten-imageproc is built for OCR post-processing; imageproc is large. |
| Web bindings | **wasm-bindgen 0.2** (+ wasm-bindgen-cli / wasm-pack 0.15) | — | Standard; both maintained. |
| iOS / Android | **UniFFI 0.32** (Swift + Kotlin) | hand-written JNI/ObjC | MPL-2.0: weak copyleft, fine in proprietary apps (only changes to UniFFI's own files must be shared). Add to `deny.toml` when adopted. |
| Flutter | **flutter_rust_bridge 2.13** (optional) | — | MIT. |
| C, C#, Go, Python, … | **C ABI + cbindgen 0.29** (build tool only) | — | Universal fallback. |
| Glyph-run cache | **quick_cache 0.7** + **foldhash** | lru | Fast, small, no_std-friendly hashing. |
| Testing | HarfBuzz golden data (in use), **proptest 1.12**, **cargo-fuzz** | — | — |
| Benchmarks | **criterion 0.8** (in use) | divan | divan's last release was 2025-04. |
| Memory allocator (WASM) | **Default (dlmalloc)** | talc, lol_alloc | talc measured identical speed (191 vs 192 µs per corpus pass); no reason to add it. |

## Measurements

### Word segmentation (icu_segmenter 2.3)

12 hand-segmented Khmer sentences (compounds kept whole), boundary-level scores:

| Mode | Precision | Recall | Native speed | WASM speed | WASM size (all bundled data) |
|------|-----------|--------|--------------|------------|------------------------------|
| Dictionary | **1.00** | 0.83 | **42.6 MB/s** | 15.3 MB/s | 3.9 MB (includes Chinese/Japanese/Thai/Lao/Burmese) |
| LSTM | 0.97 | 0.89 | 0.4 MB/s | 0.2 MB/s | 460 KB |

- Precision matters most for line breaking: a false boundary breaks a word across lines. The
  dictionary never did; its misses were compounds it kept whole (e.g. ភាសាខ្មែរ), which is safe.
- The Khmer dictionary alone is **780 KB (394 KB gzip)**. Ship it as a separate data file:
  bundle it in native apps, download on demand on the web. Load it through ICU4X's buffer
  provider (`try_new_dictionary_with_buffer_provider` + `icu_provider_blob`), so the core WASM
  stays small and layout without the dictionary still works (breaks at spaces/ZWSP).
  The buffer provider adds deserialization code; measure it in Phase 3.
- The test set is small and hand-made. Phase 3 should score against a published Khmer
  word-segmentation corpus before final sign-off.

### OCR runtime (real 9.7 MB CRNN text-recognition model, 64×256 line)

| Runtime | Native ms/line | WASM ms/line (no SIMD → SIMD) | Model load (WASM) | WASM size (raw / gzip) |
|---------|----------------|-------------------------------|-------------------|------------------------|
| **rten 0.27** | **20.5** | 243 → **77** | **25 ms** | **3.1–3.3 MB / 0.90–0.99 MB** |
| tract 0.23 | 31.8 | 232 → 65 | 202 ms | 17.2 MB / 4.1 MB |

- Build the OCR WASM module with `-C target-feature=+simd128` (3× faster). Supported by all
  current browsers; keep a non-SIMD fallback only if very old Safari matters.
- OCR is a **separate WASM module** loaded on demand; it never touches the 335 KB core.

### Rendering stack (WASM, on top of harfrust shaping)

| Build | Raw | gzip | Δ gzip |
|-------|-----|------|--------|
| harfrust shaping only | 955 KB | 317 KB | — |
| + skrifa outlines | 1,141 KB | 385 KB | +67 KB |
| + skrifa + zeno raster | 1,336 KB | 436 KB | +118 KB |
| + skrifa + tiny-skia raster | 1,438 KB | 470 KB | +153 KB |

## What each platform ships

| Platform | Core | Optional, loaded when used |
|----------|------|----------------------------|
| Web | `aksar-wasm` 335 KB gzip (shape + validate + normalize) or 24 KB (validate only); +67 KB for outlines | Khmer dictionary 394 KB gzip; OCR module ~1 MB gzip + model |
| iOS / Android | Static/shared lib via UniFFI; dictionary bundled | OCR models downloaded |
| Desktop | Rust crate or C ABI | same |
| Embedded | `aksar` with `--no-default-features` (`no_std`), optional `shape` + `libm` | — |

## Rejected, and why

- **swash, parley, cosmic-text**: all pull an older `read-fonts`/`skrifa` (second font parser);
  cosmic-text also uses unicode-linebreak (no Khmer word breaks) and harfrust 0.5.
- **rustybuzz, ttf-parser**: superseded by harfrust / read-fonts, no release since 2024-11.
- **tract**: works, but 4.5× bigger WASM and slower to load than rten.
- **ort (ONNX Runtime)**: C++ library, no `wasm32-unknown-unknown`, tens of MB natively.
- **Tesseract**: C++ (plus Leptonica), native only, weak on Khmer; keep as an optional
  comparison baseline at most.
- **sled / redb / SQLite** in the core: unmaintained / need a filesystem / C code.
- **image, imageproc**: much bigger than the two codecs and few filters OCR needs.
- **klippa** (Rust font subsetter): too young (83 downloads in 90 days). Subset fonts at
  build time with `hb-subset` or `fonttools` instead; no runtime dependency.
- **wasm-opt, simd128 for shaping, talc**: measured, no speedup.
