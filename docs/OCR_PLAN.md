# aksar OCR plan — a free, best-in-class Khmer OCR engine

**Goal:** the most accurate free Khmer OCR. It reads printed documents, scans, phone photos
and handwriting. It's pure Rust and runs on desktop, mobile and WebAssembly.
Code is `MIT OR Apache-2.0`, models are Apache-2.0, and the data we create is CC BY 4.0.
**Status: main goal (decided 2026-10-19).** Users must be able to read printed and handwritten
Khmer without paid AI, without internet and without per-page cost. Everything here runs on
the user's device; nothing needs a cloud AI service. Training needs compute once (§6), then
running is free. Tesseract stays the comparison baseline, and PDFTape's default engine until
aksar's engine beats it on the gates in §8.

**Rules:**
- Every claim is a measurement (see §8).
- Khmer-first: other scripts only go into the recognizer as far as they appear inside
  Khmer documents (Latin, digits, punctuation).

## 1. Why aksar can be the best

Existing free options and their limits (as of 2026-10):

| Option | Limit |
|--------|-------|
| Tesseract `khm` (Apache-2.0) | Fine on clean print in good fonts. Weak fonts reach 8–9% CER on PDFTape's benchmark (Noto Sans Khmer 8.42%, Kantumruy Pro 9.41% vs Battambang 0.21%). It doesn't read handwriting, and it's C++ with no WASM build |
| Kiri-OCR (Apache-2.0, 12M synthetic lines) | Measured on PDFTape's benchmark: **19.18% CER vs 4.19%** for Tesseract at the time, worse in every category |
| Netra-OCR, community CRNN/TrOCR models | Self-reported numbers on their own data, unclear licenses, Python only |
| UKTR (Kong et al., arXiv 2603.00702, 2026) | Research model covering print, scene and handwriting. Data and models are in a gated repository, and the paper is CC BY-NC-ND |

What aksar has that none of them have:
1. **A HarfBuzz-exact Khmer grammar.** The validator was checked on 320,000 sequences.
   Used in decoding (§4), the output is **always valid Khmer**: no orphan vowels or
   impossible stacks.
2. **A dictionary corrector and word segmenter** (80,630 words, linear time) and a
   normalizer, already in production in PDFTape.
3. **A Khmer-exact renderer** (`aksar::shape` + skrifa + zeno, planned) to produce unlimited
   correctly shaped training images.
4. **A real application** (PDFTape) with a review UI, so user corrections can become
   training data with consent.
5. **Pure Rust + rten:** one engine for all platforms, including the browser. Measured: a
   9.7 MB CRNN runs in 20.5 ms/line native and 77 ms/line in WASM with SIMD
   ([`STACK.md`](STACK.md)).

## 2. Benchmark suite first (B0) — 2 weeks

Nothing gets built before it can be measured. The suite is `aksar-bench`, a Rust binary
(runner exists: `labels.tsv` and `.gt.txt` sets, aksar-ocr and Tesseract; CER, cluster CER,
្ត/្ដ-folded CER, WER, ms/line, model size, peak memory; line-detection F1 still to add), plus `research/khmer/error_report.py` from M0 in
[`KHMER_ACCURACY_PLAN.md`](KHMER_ACCURACY_PLAN.md).

| Set | Content | Source / license | Stored |
|-----|---------|------------------|--------|
| **S1** synthetic print | PDFTape's 54 pages, 5 fonts × 6 categories | PDFTape, OFL fonts | repo |
| **S2** real print scans | 100 → 500 pages: government letters, books, schoolbooks, invoices, forms | collected with permission (M0) | private |
| **S3** phone photos | 300 photos of the S2 pages (angle, shadow, low light) | own | private |
| **S4** EKYC benchmark | clean / messenger-compressed / scanned / app-scanned, scene, handwriting | `EKYCSolutions/khmer-ocr-benchmark-dataset`, **MIT** | fetched |
| **S5** handwriting | 2,000 lines from 100 writers, held out by writer | own collection (§5.4) | public test set, CC BY 4.0 |
| **S6** palm-leaf (optional) | SleukRith Set lines | **no license published**: ask the authors first | — |

**Metrics.** All text is `normalize`d before scoring.
- Accuracy: CER on code points, CER on grapheme clusters, WER on aksar words, and CER with
  ្ត/្ដ folded.
- Layout: line-detection F1.
- Speed and size: ms per line and per page (native and WASM), peak memory, model size.

**Engines compared on every run:**
- Tesseract `khm` best
- Tesseract + aksar post-processing (PDFTape today)
- Kiri-OCR
- aksar engine versions

A cloud API can be added for reference, as numbers only and never as training data.

**Done when:** one command prints the table for all sets and engines, and a baseline is
committed in `docs/OCR_RESULTS.md`.

## 3. Architecture

```
image ─▶ aksar-image ─▶ aksar-detect ─▶ line crops ─▶ aksar-recog ─▶ aksar-decode ─▶ aksar (text) ─▶ Page
          preprocess     text lines      + order        CNN+CTC       grammar beam     normalize,
          deskew, bin.   (model)                        (model)       + word LM        corrector, ្ត/្ដ
```

| Crate | What | Depends on |
|-------|------|------------|
| `aksar` (exists) | text: validate, normalize, repair, dict, search, shape | — |
| `aksar-image` (exists) | grayscale, resize, deskew (projection profile), Sauvola binarization, denoise, perspective correction (photos), line-crop normalization (height 48 px, keeps Khmer above/below marks) | — (own code + `rten-imageproc`) |
| `aksar-detect` | text-line detection: DB-style segmentation model → polygons → lines → reading order (columns by XY-cut) | `rten`, `aksar-image` |
| `aksar-recog` (exists) | line recognizer (§4.1) | `rten` |
| `aksar-decode` (exists) | CTC beam search constrained by the Khmer grammar + word LM + user words (§4.2) | `aksar` |
| `aksar-render` (exists) | text → line/page images for training (§5.1) | `aksar/shape`, skrifa, zeno |
| `aksar-ocr` (exists) | the one-call facade. Today: `Ocr::new(model, alphabet)?.read(&image) -> OcrPage`. Planned: `Aksar::new()?.read(input) -> Document` for text, PDFs, images and photos ([`API.md`](API.md)) | all above |
| `aksar-legacy` | Limon/ABC → Unicode (M7), for PDFs with legacy fonts: exact, no OCR | `aksar` |
| bindings | `aksar-wasm` OCR module (separate, simd128), UniFFI (Swift/Kotlin), C ABI, **Python (PyO3)** for researchers and data work | — |

**Public result type** (stable at v1.0; the full API, inputs and bindings are in [`API.md`](API.md)):

```rust
pub struct Page { pub width: u32, pub height: u32, pub blocks: Vec<Block> }
pub struct Block { pub bbox: Rect, pub lines: Vec<Line> }
pub struct Line { pub polygon: Vec<Point>, pub text: String, pub confidence: f32, pub words: Vec<Word> }
pub struct Word {
    pub bbox: Rect, pub text: String, pub confidence: f32,
    pub alternatives: Vec<(String, f32)>,   // for review UIs
    pub flags: WordFlags,                   // UNKNOWN_WORD, CORRECTED, LOW_CONFIDENCE
}
```

Exports: plain text, JSON, hOCR and ALTO XML. With these, aksar plugs into existing tools
(OCRmyPDF-style text layers, archives) and PDFTape keeps its own PDF text-layer writer.

**Models** ship as ONNX files exported from PyTorch; rten loads ONNX directly, so there is no
conversion step (see `training/README.md`). They are downloaded on demand, versioned, and checked by SHA-256 in a `models.json` catalog, the
same pattern as PDFTape's tessdata catalog.

| Model | Params (target) | Size (fp32 / stored f16) | Use |
|-------|-----------------|--------------------------|-----|
| `detect-v1` | ~2.5 M | ~10 / 5 MB | all inputs |
| `print-fast-v1` | ~4 M | ~16 / 8 MB | desktop/mobile/web default |
| `print-accurate-v1` | ~15 M | ~60 / 30 MB | desktop, hard scans |
| `hand-v1` | ~20 M | ~80 / 40 MB | handwriting |

Measure int8 quantization in rten before relying on it. Until then, store weights as f16.

### 3.1 Vision-first (OCR-free) models — a later feature, not the architecture

*Recorded 2026-10-24.* "Vision-first" (also "OCR-free") means a large vision-language model
reads the page image and writes the text directly, with no line finding or character
recognizer. Examples: LightOnOCR (about 1B parameters), DeepSeek-OCR (a ~380M vision encoder
plus a ~3B decoder). Most 2026 practice is hybrid: classic OCR for ordinary pages, a vision
model for hard ones (tables, complex layouts, handwriting).

**Decision: aksar stays OCR-first** (the pipeline above). Vision-first may come later as an
optional feature, once measured. Why:

| | aksar (OCR-first) | Vision-first models |
|---|---|---|
| Size | 2.1 M parameters, about 8 MB | about 1-3 B parameters, gigabytes |
| Runs on | any CPU, phones, WASM (§8 size gates) | usually a strong GPU |
| Speed | about 70 ms per line on CPU (v2 pass 1) | much slower without a GPU |
| Khmer | trained for it; every letter of the chart | no Khmer results published (searched 2026-10) |
| Output | valid Khmer only (grammar decoding), dictionary-checked | generative: can write plausible text that is not on the page |

Where it could help, in order of value:

1. **Labelling data, offline, once:** a vision model drafts the text of real scans and
   handwriting, people correct it (§5.3, §5.4). Nothing changes in the shipped engine.
2. **Desktop fallback, opt-in:** lines below a confidence threshold (`OcrLine::confidence`)
   go to a vision model as a second engine next to aksar's in `aksar-ocr` (like §9's
   `Engine` trait). Phones and WASM keep the fast path.
3. **Hard layouts:** tables, forms, receipts.

Gate before any of these: run an open vision model on S4 (EKYC, the 2,503 in-alphabet lines)
with `aksar-bench` and compare CER with aksar and Tesseract. Needs a GPU (a Mac or a free
Colab GPU). Sources: [LightOnOCR](https://arxiv.org/pdf/2601.14251),
[GutenOCR](https://arxiv.org/pdf/2601.14490) (OCR-based vs OCR-free),
[Vision RAG vs OCR 2026](https://webscraft.org/blog/vision-rag-vs-ocr-2026-yakiy-pidhid-krasche-dlya-roboti-z-dokumentami?lang=en).

## 4. Recognition and decoding

### 4.1 Line recognizer

- **Input:** a grayscale line crop at height 48 (64 for handwriting), any width.
- **Network:**
  - CNN backbone: MobileNetV3-small style for "fast", ResNet-ish for "accurate".
  - Sequence model: 2 BiLSTM layers (fast) or a 4-layer Transformer encoder (accurate, hand).
  - Output: a **CTC** head, which is non-autoregressive: fast, simple, works well in WASM.
- **Alphabet = Unicode code points, not clusters:**
  - All assigned Khmer and Khmer Symbols code points (~140), ASCII printable (95),
    common punctuation (« » – — … ៖ ។ ៕ etc.) and the CTC blank, ~250 symbols in total.
  - Why not clusters: the cluster vocabulary runs into the thousands and is long-tailed;
    code points keep the head small and rare clusters learnable.
  - **ជើងត and ជើងដ.** They are different letters (the spelling decides the word), but most
    fonts draw them alike. Try two designs and measure: (a) two classes, letting the model use
    context; (b) one class, with the spelling chosen by a word model trained on text with
    standard spellings. The ICU word list is not enough for (b): it lacks many standard ជើងដ
    spellings ([`KHMER_ACCURACY_PLAN.md`](KHMER_ACCURACY_PLAN.md) M3).
  - Labels are always `aksar::syllable::normalize`d. The model learns storage order, so its
    output is directly comparable with the truth.
- **Handwriting:**
  - Same CTC model family, larger, trained with a modality token (print/photo/hand),
    following UKTR's finding that modalities transfer.
  - Optional v2: an attention decoder rescoring the CTC n-best, only if measured better.

### 4.2 Grammar-constrained decoding — aksar's main advantage

CTC beam search where each beam carries
`(text, ctc_score, syllable_state, lm_state, word_state)`:

1. **Syllable automaton:**
   - A DFA compiled from the existing validator categories (HarfBuzz `khmer_machine`).
   - Extending a beam with a code point that makes an invalid syllable is **pruned**.
     Result: output never contains ◌-producing sequences.
   - Storage-order variants are merged, so "split vowel" beams don't compete with their
     composed form.
2. **Word LM:**
   - Cluster-level n-gram (order 4), trained on licensed Khmer text (§5.2) and stored
     compactly (`fst` or a pruned hash table, a few MB).
   - Plus an in-vocabulary bonus from `dict::Words` (with user words). Score:
     `log P_ctc + α·log P_lm + β·[word in dictionary]`, with α and β tuned on the
     validation set.
3. **Outputs:**
   - best text, per-character and per-word confidence (from beam posteriors),
     top-k word alternatives;
   - then the existing `Words::check` corrector, the M3 ្ត/្ដ resolver, and `normalize`.
4. **No-harm guard:** the decoder never applies the LM to Latin runs, digits or numbers
   (amounts and dates must be read, not guessed).

**Measure separately:** greedy CTC → + grammar → + LM → + corrector. Each step must lower
CER, otherwise it stays off.

### 4.3 Detection and layout

- **Model:**
  - Start from the ocrs detection model architecture (2.5 MB, rten-native).
  - Retrain it on Khmer pages: Khmer lines are taller, with marks above and below that
    Latin-trained detectors cut off.
  - Labels come for free from `aksar-render` page synthesis and from PDFTape's
    born-digital PDFs (text positions).
- **Lines:** polygon → line crop with a margin of 0.35 × x-height above and below.
- **Reading order:** XY-cut for columns; tables are a later milestone, read cell by cell
  after table detection.

## 5. Data — the hard part

### 5.1 Synthetic print (`aksar-render`)

- **Fonts (OFL, Google Fonts families; check each license file and record it in
  `data/fonts.toml`):**
  - Angkor, Battambang, Bayon, Bokor, Chenla, Content, Dangrek, Fasthand, Freehand,
    Hanuman, Kantumruy Pro, Kdam Thmor Pro, Koulen, Metal, Moul, Moulpali, Nokora,
    Odor Mean Chey, Preahvihear, Siemreap, Suwannaphum, Taprom, Noto Sans Khmer,
    Noto Serif Khmer.
  - Khmer OS fonts (LGPL) are the most common in real documents: use them for
    **evaluation**. Use them for training only after KhableCom decides on the license
    question.
- **Rendering:** text → `aksar::shape` → skrifa outlines → zeno raster.
  - Truth is exact by construction: the image is drawn from the label.
  - Bold and italic synthesized; sizes 8–28 pt at 150–400 dpi.
  - Mixed Khmer + Latin + digits lines (invoices, IDs, dates).
- **Augmentation** (measured per kind, keep what helps S2/S3):
  - blur, noise, JPEG, ink bleed/erosion, background paper textures;
  - rotation ±3°, perspective, uneven lighting, scanner streaks, photocopy degradation,
    binarization artifacts.
- **Volume:** 5 M lines for pretraining (~1 day to render on 8 cores; measure first).
- **Also used for the Tesseract fine-tune (M1).** This keeps PDFTape improving before the
  aksar engine is ready.

### 5.2 Text to render (and for the LM)

| Source | License | Use |
|--------|---------|-----|
| aksar word list (80,630 words) | Unicode v3 | random word sequences, rare-word coverage |
| Project-written sentences (letters, forms, invoices, school texts) | own, CC BY 4.0 | training + no-harm corpus |
| Khmer Wikipedia | CC BY-SA 4.0 | LM and rendering **only after a KhableCom decision** (attribution in the model card; the text itself is not redistributed) |
| Public-domain / government texts | check each | only with a recorded license |
| User corrections from PDFTape | opt-in only, per export (PLAN §5 format) | fine-tuning, never public without consent |

Every model gets a **data manifest**: sources, licenses, counts, hashes. A model without a
complete manifest is not released.

### 5.3 Real print

- S2/S3 collection (M0) continues as the private evaluation set, grown to 500 pages.
- For training: PDFTape users' confirmed corrections (opt-in), and born-digital Khmer PDFs
  whose text layer passes `aksar::syllable::validate`. Render the PDF page and pair it with
  its text: free, real-font, real-layout data.

### 5.4 Handwriting — collect the first open Khmer handwriting dataset

There is no clearly licensed public dataset of modern Khmer handwriting:
- **SleukRith Set** is palm-leaf manuscripts with no license published.
- **`SoyVitou/khmer-handwritten-dataset-4.2k`** has unclear terms.
- **The EKYC benchmark** (MIT) has only a small handwritten test level.
- **UKTR's data** is gated.

So aksar creates one. That alone makes the project useful to everyone working on Khmer.

1. **Collection kit:**
   - A web app built with `aksar-wasm`. It shows a prompt sentence; the writer writes it on
     paper and photographs it, or writes on a tablet. The app crops, rotates and uploads.
   - Printable forms with QR codes for schools and offices (scan → auto-align by QR → line
     crops).
2. **Consent:**
   - Each writer agrees to release their samples under **CC BY 4.0**. They record only age
     band, writing hand and region (no names), and can withdraw before a release.
   - Minors only with guardian consent through the school.
   - Have the consent text reviewed by a lawyer before launch.
3. **Prompts:**
   - Sentences from §5.2 chosen to cover every consonant, subscript, vowel and sign, and
     the 2,000 most frequent clusters.
   - Plus numbers, dates, names and addresses (forms are a key real use).
4. **Targets:** phase 1: 200 writers × 50 lines = 10,000 lines; phase 2: 1,000 writers,
   50,000 lines. Write the test set from separate writers.
5. **Synthetic handwriting:**
   - Fasthand/Freehand-style fonts plus elastic distortion, stroke-width variation and
     baseline wobble, for pretraining.
   - Tablet strokes re-rendered with different pens: one stroke sample → many images.
6. **Labels:** prompt text is the starting label. Reviewers fix what the writer actually
   wrote; it must pass `validate`, with two reviewers on the test set.
7. **Release:** `aksar-data-hand-v1` with a datasheet (who, how, consent, known biases),
   CC BY 4.0, on GitHub releases and Hugging Face.

Palm-leaf manuscripts (S6) are a separate, later track: they need different glyph forms
and permission from the dataset authors.

## 6. Training (`training/` directory, Python)

- **Stack:** PyTorch, a reproducible config per model (YAML), seeds fixed, logs and metrics
  saved, export with `torch.onnx` (rten reads the ONNX file as is), plus a parity test that
  rten and PyTorch outputs are within 1e-4.
- **Curriculum:**
  1. pretrain on synthetic print;
  2. fine-tune on synthetic + augmented + born-digital PDF pairs;
  3. fine-tune on real data (opt-in corrections), with S2 kept strictly for evaluation;
  4. handwriting: start from (2), train on synthetic hand + collected hand with the
     modality token.
- **Compute (estimate, measure on the first run):**
  - `print-fast` pretraining: ~1–2 GPU-days on one 24 GB GPU;
  - `hand-v1`: ~3–5 GPU-days.
  - Free options first: Kaggle notebooks (about 30 free GPU hours a week), Google Colab's
    free tier, or a Mac with Apple Silicon (PyTorch `mps`) for the small models. Renting a
    cloud GPU for a full cycle stays in the low hundreds of US dollars.
  - Training is a one-time cost per model version. Running the models is free.
- **Evaluation:** every checkpoint runs the B0 suite. Results go into `docs/OCR_RESULTS.md`
  automatically, and the model card is generated from them.

## 7. Roadmap

Effort assumes one engineer plus a GPU; the handwriting collection needs partners
(schools, NGOs, offices) in parallel.

| Version | Content | Effort |
|---------|---------|--------|
| **v0.1.0** ✅ | B0 benchmark suite (§2); M0 measurement; M2 confusions, M3 ្ត/្ដ, M6 extra-េ (Tesseract path); `aksar-render` + augmentation (`real` mode measured on EKYC crops); Tesseract `khm` fine-tune (M1: not shipped, no gain on S4); `aksar-image`; manifests. Pulled forward: `aksar-decode`, `aksar-recog`, `aksar-ocr` (projection line finding), Python bindings, first S4 head-to-head | done |
| **v0.2.0** | print model trained to beat Tesseract on S4 print; model card + release; LM in the decoder; S2 real scans | 5 wk |
| **v0.3.0** | `aksar-detect` (learned, for photos and columns); WASM OCR module; PDFTape engine option behind a flag (Khmer pages); `aksar-legacy` (M7) | 5 wk |
| **v0.4.0** | photos (S3): perspective, lighting, photo augmentation; print-accurate model; **handwriting collection launched** (§5.4) | 4 wk + collection |
| **v0.5.0** | `hand-v1` beta; handwriting test set and dataset v1 released | 5 wk |
| **v1.0.0** | stable API and `Page` type; published crates.io `aksar*`, npm `aksar-wasm`, PyPI `aksar`; models + model cards; CLI `aksar ocr` with text/JSON/hOCR/ALTO | 3 wk |
| later | tables and forms; palm-leaf; on-device fine-tuning from user corrections; table/key-value extraction for IDs and invoices | — |

M5 and M8 (stray Latin, spacing) stay in PDFTape's Tesseract path; the aksar engine covers
them by design (Latin in the alphabet, words from the decoder).

## 8. Gates and targets

**Gates: a version ships only if all of these hold.**

| Gate | Rule |
|------|------|
| Print, engine switch | aksar engine CER **below Tesseract + aksar** on S1 **and** S2 **and** S4-print, with no S1 category worse. Only then does PDFTape default to it for Khmer |
| No harm | correct-text corpus: 0 changes by decoder + corrector |
| Validity | 100% of output lines pass `validate` (by construction; tested) |
| Numbers | digits/amounts/dates exact rate ≥ Tesseract's on S2 |
| Speed | desktop page ≤ Tesseract's time; WASM ≤ 150 ms per line; mobile `print-fast` ≤ 40 ms per line on a mid-range phone |
| Size | WASM OCR module ≤ 1.2 MB gzip, excluding models |
| Handwriting | published CER on S5 (writer-disjoint), with confidence intervals; no claims beyond it |

**Targets (goals, not claims):**
- S1 CER < 1.0%, and every font < 2% (today: 2.93% overall, 9.41% worst font).
- S2 real scans: best measured free engine, and < 3% CER.
- Phone photos (S3) < 5%.
- Neat handwriting (S5) < 10% CER at v0.5, < 6% at v1.0.

## 9. PDFTape integration

1. `pdftape-ocr` gets an `Engine` trait with `TesseractEngine` (today) and `AksarEngine`
   (from v0.3, behind a cargo feature and a setting).
2. Routing: pages whose languages include `khm` → `AksarEngine` when the user enables it,
   otherwise Tesseract. Other languages → Tesseract, unchanged. Mixed Khmer + English lines
   are fine: the aksar alphabet includes Latin.
3. The review UI uses `Word.alternatives` and `flags`. Confirmed corrections → opt-in export
   (PLAN §5) → next fine-tune.
4. PDFTape's benchmark gate (task T5) compares both engines. The default only switches
   after the §8 gate.
5. Models are downloaded via PDFTape's pack catalog (SHA-256, size), like tessdata.

## 10. Licensing — what "free" means here

| Part | License |
|------|---------|
| Code (all crates, training code) | `MIT OR Apache-2.0` |
| Model weights | Apache-2.0, with a data manifest and model card |
| Data we create (handwriting, sentences, synthetic sets) | CC BY 4.0 |
| Third-party data | only with a license that allows training and redistribution of the weights; NC/ND data (e.g. CC BY-NC-ND) is never used for training |
| Fonts | OFL for training; anything else only after a written decision |

Open decisions for KhableCom:
- Wikipedia (CC BY-SA) for LM and rendering text;
- LGPL Khmer OS fonts for training;
- the open-source timing of the repo.

## 11. Risks

| Risk | Mitigation |
|------|------------|
| New engine doesn't beat Tesseract on real scans | Tesseract path keeps improving (M1–M6); the switch is gated; real data from opt-in corrections |
| Synthetic-to-real gap | real-scan eval in every gate; born-digital PDF pairs; augmentation measured per kind |
| Handwriting data too small | partners early; synthetic and stroke re-rendering; writer-disjoint test set so claims stay honest |
| Consent and privacy | no names, withdrawal, legal review, minors only via guardians |
| Model size on web/mobile | f16 weights, int8 after measuring, fast/accurate split, on-demand download |
| GPU cost/time | small models first; measure the first run before scaling |
| rten gaps (ops, quantization) | stick to ops rten supports; parity test on export; tract as fallback runtime |
| Scope creep (tables, other scripts) | §7 order; Khmer-first rule |

## 12. First two weeks (concrete)

1. `aksar-bench` skeleton: read S1 (PDFTape bench) and S4 (EKYC, fetched by a script with
   pinned commit + SHA-256); run Tesseract, Tesseract + aksar and Kiri-OCR; write
   `docs/OCR_RESULTS.md`.
2. `research/khmer/error_report.py` (M0) on S1 and S4.
3. Start S2: collect 100 pages with permission; ground truth typed by two people,
   normalized, validated.
4. Syllable DFA: compile the validator categories into a transition table; property test
   that DFA acceptance equals `validate` on the 320,000-sequence fuzz set. This is the first
   piece of `aksar-decode` and is useful immediately (a fast validity check for PDFTape's
   text-layer check).
5. Draft the consent text and the collection-app spec for §5.4; find 2–3 partner schools or
   offices.
