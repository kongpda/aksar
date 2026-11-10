# Other languages: what aksar should (and shouldn't) add

> **Decision (2026-10-18): aksar stays Khmer-first.** Nothing below is scheduled in aksar
> yet. PDFTape keeps handling other languages itself (NFC + ICU); its optional n-gram search
> for unspaced scripts (A) is planned **in PDFTape** (`docs/AKSAR_INTEGRATION_PLAN.md` W6).
> Revisit when a PDFTape need is measured.

PDFTape OCRs 19 languages; aksar handles Khmer. This page answers two questions with
measurements (2026-10-18; scripts in `research/multilang/`):

1. Did moving PDFTape's Khmer code to aksar hurt its other languages?
2. Which other languages would gain from aksar's techniques, and how?

## 1. PDFTape's other languages are unaffected

PDFTape now routes **only Khmer runs** to aksar; all other text keeps NFC + ICU, as before.
Checked by running PDFTape's old and new `normalize`, `index_form`, `search_tokens`,
`clusters` and `segment_words` on every paragraph of the UDHR in all 19 languages:

| Languages | Paragraphs | Output differences |
|-----------|------------|--------------------|
| eng, vie, fra, deu, spa, por, ind, msa, rus, hin, arb, cmn_hans, cmn_hant, jpn, kor, tha, lao, mya | 1,656 | **0** in every function |
| khm | 92 | only the intended changes: `search_tokens` (single clusters added), `segment_words` (aksar dictionary instead of ICU LSTM); `normalize` identical on real text |

## 2. What would help other languages (measured)

### A. Search for scripts written without spaces: **big win, do it**

PDFTape's in-app search tokenizes Thai, Lao, Burmese, Chinese, Japanese and Korean into ICU
words. A query that isn't exactly one of those words is missed. aksar's Khmer method
(index every cluster and every pair of neighbouring clusters) fixes that for any script.

400 random 2–6 character pieces and 300 whole words of real UDHR text per language, searched
against their paragraph (FTS-style: every query token must be in the page):

| Language | Pieces found now | with cluster n-grams | Whole words now | with n-grams | Index tokens |
|----------|-----------------|----------------------|-----------------|--------------|--------------|
| Khmer (already aksar) | 100% | 100% | 100% | 100% | 1.0× |
| Thai | **13%** | 100% | **93%** | 100% | 4.7× |
| Lao | **14%** | 100% | **91%** | 100% | 3.9× |
| Burmese | **24%** | 100% | **95%** | 100% | 3.4× |
| Chinese (Simpl.) | **40%** | 100% | 100% | 100% | 3.3× |
| Chinese (Trad.) | **40%** | 100% | 100% | 100% | 3.4× |
| Japanese | **36%** | 100% | 100% | 100% | 3.5× |
| Korean | **27%** | 100% | 100% | 100% | 4.2× |
| English, Vietnamese | word search (unchanged by design) | | 100% | 100% | 1.0× |

- Thai/Lao/Burmese miss 5–9% of **real words** today: a word segmented on its own can split
  differently than inside the sentence. Korean misses stems without their particle
  (`한국` in `한국은`); Chinese/Japanese miss anything that isn't a dictionary word.
- Cost: 3–5× more index tokens for those scripts (FTS5 handles it; measure DB size on real
  libraries). Latin/Cyrillic/Vietnamese stay word-based.
- How: generalize `aksar::search` from "Khmer runs" to "unspaced-script runs", with clusters
  from aksar's syllable grammar for Khmer/Burmese and Unicode grapheme clusters for the rest.
  PDFTape then calls one function for every language. Bump `PIPELINE_VERSION` (index format).

### B. One HarfBuzz-exact validator for many scripts: **cheap, do it, low OCR impact**

aksar's Khmer validator mirrors HarfBuzz's grammar by hand. Instead, shaping with harfrust and
a **5 KB "probe" font** (every character of the complex scripts mapped to one blank glyph, plus a
dotted circle) gives HarfBuzz's exact broken-cluster decision without per-script code:

| Script | Random sequences | Probe font agrees with the real Noto font |
|--------|------------------|-------------------------------------------|
| Khmer | 6,000 (aksar's labeled set) | 6,000 / 6,000 |
| Burmese | 5,000 | 5,000 / 5,000 |
| Devanagari | 5,000 | 5,000 / 5,000 |
| Thai | 5,000 | 5,000 / 5,000 |
| Lao | 5,000 | 5,000 / 5,000 |

- Full coverage matters: with only a dotted-circle glyph it disagreed on 139/6,000 Khmer
  cases, because HarfBuzz decomposes split vowels only for characters the font maps.
- Khmer, Burmese and Indic scripts (and the Universal Shaping Engine scripts) get full
  checks. **Thai and Lao** are only checked at the start of the text (HarfBuzz has no
  syllable grammar for them); they need explicit rules (e.g. Thai WTT 2.0 sequence checks).
- Impact on OCR is small: Tesseract output had at most 3 broken clusters per 2 pages in every
  language tested. The value is for typed/pasted text and as a safety check on the text layer.
- Needs the `shape` feature (harfrust); fine for PDFTape native, 335 KB gzip on the web.

### C. Dictionary single-mark fixer for Thai/Lao/Burmese: **don't port it (yet)**

It is aksar's main OCR win for Khmer, and mark-only errors are a large share of errors in
these scripts, so it looked promising. Measured on Tesseract output of 2 UDHR pages each
(Noto fonts, 300 dpi, raw Tesseract PSM 6, ICU word lists):

| Language | Errors that are mark-only | Errors that are only mark **order** | CER before → after fixer |
|----------|---------------------------|-------------------------------------|--------------------------|
| Khmer (control) | 24% | 0% | 1.69% → **1.57%** |
| Thai | 36% | 0% | 2.33% → 2.31% |
| Lao | 41% | 0% | 16.04% → 16.07% |
| Burmese | 29% | 0% | 39.34% → 39.46% |

- No gain for Thai, slightly worse for Lao and Burmese: their errors are rarely fixable by one
  unique edit with ICU's lists. It would need per-language confusion sets and real scans.
- These raw Lao/Burmese CERs are far above PDFTape's pipeline (8.95% / 6.62%), which removes
  stacked-mark junk lines first; only the error **mix** is comparable.
- **0% of errors are mark order**, in every language: Tesseract already emits canonical order,
  so reordering normalizers don't improve OCR for any of them. (Keep them for typed text.)

### D. Not aksar's job

- **Accuracy of the experimental languages** (tha 4.49%, lao 8.95%, mya 6.62%, chi_tra 4.99%,
  ara 11.91% clean CER in PDFTape) is the recognition model: try `tessdata_best` for those
  packs and fine-tune with `tools/ocr-train` (generalize it beyond `khm`). aksar's shaping can
  render training lines in any script harfrust supports.
- Latin, Cyrillic, Vietnamese: NFC + lowercase via ICU is correct; nothing to add.
- Arabic RTL order and the PDF text layer stay in PDFTape.
- Not measured yet, worth a look: Burmese **Zawgyi** (legacy non-Unicode encoding common in
  born-digital Burmese PDFs) detection/conversion; Arabic search folding (hamza/alef forms,
  tashkeel, tatweel); Traditional/Simplified Chinese folding in search.

## Recommendation

1. **Generalize `aksar::search` to all unspaced scripts** (A). Largest measured user-facing
   gain for 7 of PDFTape's languages, low risk, small code.
2. **Add `aksar::validate_any`** via harfrust + the probe font (B), with Thai/Lao rules as a
   follow-up.
3. Keep the dictionary fixer and normalizer **Khmer-only** until a per-language version beats
   the baseline on PDFTape's benchmark (C).
4. Put OCR accuracy effort for the experimental languages into models (D).

The name still fits: *akṣara* is the shared word for "letter / script" across these languages
(Khmer អក្សរ, Thai อักษร, Lao ອັກສອນ, Burmese အက္ခရာ, Hindi अक्षर).
