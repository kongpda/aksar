# OCR results

Every number here is a measurement, with how it was made. Benchmark sets are described in
[`OCR_PLAN.md`](OCR_PLAN.md) §2. CER ignores whitespace and is computed after NFC.

## S1: PDFTape's synthetic corpus (54 pages: 27 Khmer, 27 English)

Five OFL fonts (Battambang, Kantumruy Pro, Moul, Noto Sans Khmer, Noto Serif Khmer), nine
damage categories. Tesseract 5.3.4, `tessdata_best` khm, pinned packs. "PDFTape pipeline" =
PDFTape's full mode (orientation, preprocessing variants, second pass, aksar), with
deterministic budgets (`BENCH_BUDGET_SECS=3600`).

| Engine | Khmer CER | Khmer WER | English CER | Date |
|--------|-----------|-----------|-------------|------|
| Plain `tesseract` CLI (psm 3), original khm | 24.36% | — | 12.99% | 2026-10-19 |
| Plain `tesseract` CLI, khm fine-tuned (run 1) | 22.16% | — | 12.99% | 2026-10-19 |
| PDFTape pipeline, aksar 0.1.0 | 2.93% | 24.2% | 0.04% | 2026-10-18 |
| PDFTape pipeline + T8/T9 | 2.89% | 24.0% | 0.04% | 2026-10-19 |
| PDFTape pipeline, aksar pre-0.1 | 2.74% | 22.3% | 0.04% | 2026-10-19 |
| PDFTape pipeline, aksar pre-0.1, khm fine-tuned (run 1) | **1.62%** | **9.2%** | 0.04% | 2026-10-19 |
| PDFTape pipeline, aksar pre-0.1, khm fine-tuned (run 2, more noise; rejected) | 3.97% | 13.0% | 0.04% | 2026-10-19 |

The plain CLI fails on rotated pages (>100% CER: it reads them sideways), which is why
PDFTape's pipeline matters. The fine-tuned model was trained on the same five fonts (different
text), so its S1 gain overstates what real scans will show. Recipe and per-category numbers:
`research/khmer/tesseract_finetune/README.md`.

## S2/S3: real scans and photos

Not measured yet: needs the collected pages (accuracy plan M0).

## S4: EKYC benchmark (MIT)

Real images: 15 government documents in four versions (clean PDF export, Messenger
compression, printed and scanned, scanner app) and 800 street photos (good and low light).
`research/khmer/ekyc/convert.py` cuts the labelled boxes into 4,077 line crops. The main
table uses the **2,503 lines whose characters are all in aksar's alphabet** (Khmer,
Western digits, common punctuation) and that every system read. Every column is scored
the same way: truth and output normalized with `aksar::syllable::normalize`, zero-width
characters removed, whitespace ignored, character edit distance. (Earlier versions of
this table used a slightly different scorer: Tesseract read 32.64% there.) Tesseract:
5.3.4, `tessdata_best`, `--psm 7`, `khm+eng`. aksar: `aksar-recog` with
grammar-constrained decoding (`aksar-decode`), one thread, about 240 ms per line.

No EKYC image is used for training: the aksar models learn from `aksar-render` lines only,
so this is a held-out test.

| EKYC task | lines | Tesseract `khm` | Tesseract fine-tuned (M1 run 1) | aksar v1 | **aksar v2, pass 1** |
|-----------|-------|-----------------|---------------------------------|----------|----------------------|
| **All** | 2,503 | 31.20% | 31.02% | 32.96% | **23.37%** |
| Clean PDF export | 250 | **7.53%** | 6.75% | 15.85% | 9.71% |
| Messenger-compressed | 250 | 20.92% | **19.99%** | 26.72% | 22.09% |
| Printed and scanned | 249 | 55.02% | 59.42% | 53.33% | **45.49%** |
| Scanner app | 253 | 22.26% | 22.98% | 37.05% | **20.78%** |
| Street photos, good light | 766 | 30.46% | 28.91% | 38.15% | **21.07%** |
| Low-light photos | 735 | 44.86% | 42.84% | 28.38% | **21.98%** |

- **aksar v1:** trained on clean `make_lines` output (2026-10-18).
- **aksar v2, pass 1:** v1 fine-tuned for one pass over `make_lines ... real` lines (long
  lines, punctuation, digits, English, small text, tight crops, bold, slivers of
  neighbouring lines, light-on-dark). Training continues; later passes will be added here.
- aksar is already ahead on photos, scans and scanner-app images. It is still behind on
  clean PDF exports and Messenger compression, where Tesseract's dictionaries help most.

Over all 3,603 Khmer-tagged lines (characters outside aksar's alphabet count as errors
for aksar): Tesseract 30.61%, fine-tuned Tesseract 31.58%, aksar v2 pass 1 25.98%.

The fine-tune's large S1 gain does not carry over to real documents (see the fine-tune
README); PDFTape keeps the original `khm`.

## S5: handwriting

Not measured yet: needs the handwriting collection (`OCR_PLAN.md` §5.4).
