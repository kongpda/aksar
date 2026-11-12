# Khmer accuracy plan

aksar is **Khmer-first**: everything here aims to make Khmer OCR output, search and text
handling as accurate as possible. Other languages come later, only when measured worth it
(`docs/LANGUAGES.md`).

Last updated: 2026-10-18. These milestones improve a **Tesseract pipeline** with aksar post-processing;
aksar's own free, on-device engine (the main goal) is planned in [`OCR_PLAN.md`](OCR_PLAN.md). Release mapping (OCR_PLAN §7):
**v0.1.0** = M0, M2, M3, M6, M1 (measured, not shipped); **v0.3.0** = M7; M4 folds into `aksar-decode`
(v0.2.0); M5, M8 belong in the app's OCR engine, not in aksar.

## Status (2026-10-19)

Measured on the S1 benchmark (54 synthetic pages, not in this repo; Tesseract 5.3.4,
deterministic budgets), each step against the one before it:

| Step | Where | Khmer CER | Khmer WER | Space-exact lines | Pages worse |
|------|-------|-----------|-----------|-------------------|-------------|
| Baseline (Tesseract pipeline + aksar 0.1.0) | app | 2.93% | 24.2% | 88% | — |
| M5 stray Latin + M8 spacing | app | 2.89% | 24.0% | 92% | 0 |
| M3 folded ្ត/្ដ + M6 visual-order vowels | aksar | 2.79% | 23.1% | 92% | 0 |
| M2 first confusion pair វ/រ | aksar | **2.74%** | **22.3%** | 92% | 0 |

| Milestone | Status |
|-----------|--------|
| M0 measurement | `research/khmer/error_report.py` done; the no-harm check runs on 940 correct tokens; **real scans still missing** (needs collected pages) |
| M1 Tesseract fine-tune | run: better on the S1 synthetic pages, **no gain on real documents** (EKYC), not shipped ([OCR_RESULTS.md](OCR_RESULTS.md)) |
| M2 confusion sets | first pair វ/រ added (the top consonant error). Caveat: chosen and measured on the same synthetic pages; confirm on real scans before adding more pairs |
| M3 ្ត/្ដ | done: search folds (v0.1.0); the dictionary accepts either but **never rewrites** one into the other (measured: correcting to the ICU list rewrote 13-45 correct UDHR tokens); a trustworthy spelling list is needed before any automatic choice |
| M4 symbol alternatives | `dict::best_alternative` / `SymbolChoice` done (0.1.0) for Tesseract-style engines. Not used in `aksar-ocr`: its decoder gives whole-line candidates, not per-symbol confidences, and its dictionary-aware beam search already tries every symbol |
| M5, M8 | done in the app's engine; M8 turned out to be about *missing* spaces, not spaces inside words |
| M6 extra េ | done: several visual-order vowels per word fixed; fewer-changes tie-break |
| M7 legacy | detection improved in the app; converter not started (needs font tables) |

## 1. Where Khmer stands (baseline)

A full Tesseract pipeline (orientation, preprocessing, second pass, aksar) on the 54-page S1 corpus, Tesseract 5.3.4, `khm` best,
after the aksar integration: **Khmer CER 2.93%, WER 24.2%**, line breaks exact 93%,
spaces exact 88%. By font (a separate run, Tesseract 5.5): Battambang 0.21%,
Noto Serif Khmer 2.59%, Moul 6.05%, Noto Sans Khmer 8.42%, Kantumruy Pro 9.41%.

What the remaining errors are (299 edits over 10,130 characters, aligned against the truth):

| Error type | Share | Top examples |
|------------|-------|--------------|
| Vowel / sign missing, extra or wrong | 35% | extra េ ×19, ុ dropped ×8, ូ dropped ×6, ែ→ៃ ×6, ើ→ី ×6 |
| Consonant read as another | 27% | វ→រ ×15, ភ→ក ×4 |
| Consonant / coeng missing or extra | 23% | ្ dropped ×9, extra រ ×10, extra ្ ×7 |
| Latin / ASCII involved | 9% | stray Latin fragments in Khmer lines |
| Coeng TA / DA (ជើងត ្ត vs ជើងដ ្ដ: different letters drawn alike, so a spelling question the picture can't answer) | 4% | ដ→ត ×12 (~20% on macOS fonts: CER 4.77% raw vs 3.80% folded) |
| Punctuation, digits | 1% | |

All of this is **synthetic**. No real-scan number exists yet; M0 fixes that first.

## 2. Principles

1. Measure before and after every change, on the same machine, same corpus, same pack.
2. Correct text must stay correct: every corrector change runs a "no harm" test on correct
   Khmer text (target: zero changes).
3. Fix only when sure; otherwise flag for review (an app can show flagged words to a reviewer).
4. Every rule is data-backed: a confusion pair enters the corrector only if it lowers CER
   on the benchmark with no page worse.

## 3. Milestones

Order by expected gain per effort. Each has: what, how (files/APIs), test, done-when.

### M0. Measurement foundation — 1 week (blocks honest claims, not the other work)

- **Error report tool** `research/khmer/error_report.py`: aligns OCR output with truth,
  prints the table above (types, top pairs, per font/category). Input: benchmark output
  `*.jsonl` + corpus. Used by every milestone.
- **No-harm corpus** `testdata/corpus/km-correct.txt`: 5,000+ correct Khmer sentences
  (news, government letters, school texts) written or licensed for the project; the
  Wikipedia dump is CC BY-SA, fine for local measurement, not for committing. Test:
  `Dictionary::check` returns `Known` or leaves the token unchanged on every token.
- **Real scans set** (stored privately, not in the repo): 100 pages to start — government
  letters (Khmer OS Muol Light / Battambang, the most common real-world fonts, LGPL so not
  used for training), books, school material, invoices, phone photos. Ground truth typed by
  two people; disagreements resolved; every line passes `aksar::syllable::validate` and is
  stored `normalize`d, as `page.png + page.gt.txt`.
- Report both raw CER and CER with ្ត/្ដ folded, per font and category.

Done when: baseline table for synthetic + real scans is in this file.

### M1. Fine-tune Tesseract `khm` on the weak fonts — 2 weeks · biggest expected gain

The gap is the model: 0.21% on Battambang vs 8.42% Noto Sans Khmer and 9.41% Kantumruy Pro.
Fine-tune `khm` (fonts pinned and license-checked, eval gate, leakage controls), with
exact rendering from aksar:

- New crate **`aksar-render`** (workspace member, not in the core): renders a text line to a
  grayscale PNG with aksar shaping (harfrust) + skrifa outlines + zeno rasterizer
  (`docs/STACK.md`: same `read-fonts` as harfrust, +118 KB). API:
  `render_line(text, &Font, px, &RenderOptions) -> GrayImage` with tracking, weight
  (variable fonts via `Font::with_variations`), padding, baseline jitter.
- Why not Pillow+raqm/Chrome: aksar output is glyph-identical to HarfBuzz (tested), runs in
  Rust offline, deterministic, any OFL font and weight, and the same shaper an app would use
  to draw its text.
- CLI `aksar-render lines --fonts fonts.json --text train.txt --out lines/` writes
  `NNNNNN.png` + `NNNNNN.gt.txt` (tesstrain layout). Augmentations: blur, noise, JPEG, skew, uneven light, low DPI, 40% binarized.
- Ground truth: `aksar::syllable::normalize`, lines that fail `validate` dropped.
- Train with `lstmtraining` (`lstmf` from these images), fine-tune from
  `tessdata_best` `khm`, lr 1e-4, 20k iterations, weak fonts weighted ×3, anchors kept.

Test / gate: weak-font mean CER −5% relative or better,
no weak font worse than +0.3 pt, Battambang/Noto Serif not worse than +0.3 pt, real scans
not worse than +0.3 pt, median time ≤ 1.25× base. Then run the S1 benchmark.

Done when: a fine-tuned `khm.traineddata` passes the gate. (Run and measured; it did not
pass on real documents, see [OCR_RESULTS.md](OCR_RESULTS.md).)

### M2. Data-driven confusion sets in the corrector — 1 week

Today the corrector's substitution groups are vowels plus ដ/ត. The top consonant
confusion **វ→រ (15×)** and the extra-េ insertions are not covered.

- API: `aksar::dict::Confusions` (groups for substitution, insertable/deletable marks,
  consonant pairs) and `Dictionary::check_with(token, &Confusions)`; `check` keeps using
  `Confusions::default()`, which becomes the measured set.
- Tool (to write, e.g. `research/khmer/confusions.py`): from error reports over the training eval set (never
  the test set), list pairs with count ≥ 5.
- Add candidates **one at a time**: វ/រ, ែ/ៃ, ើ/ី, deletion of a duplicated/extra េ, ភ/ក,
  extra រ deletion. For each: S1 benchmark CER/WER, no-harm test (0 changes), corrector
  time. Keep only winners. Record each decision in this file.
- Keep the linear-time guarantee: new edits go through the same windowed scoring; the
  differential test (`tests/dict_reference.rs`) extends to the new edit kinds.

Done when: default set beats the current one on the benchmark, no-harm = 0.

### M3. ជើងត (្ត) and ជើងដ (្ដ) — 1 week

**Facts:** ្ត is **ជើងត** (cheung ta, the subscript form of ត) and ្ដ is **ជើងដ** (cheung da,
the subscript of ដ). They are **different letters, and the spelling decides the word**: ត្ត
and ដ្ដ have different meanings. But most fonts draw ជើងត and ជើងដ the same, so no OCR engine
can tell them apart in an image; only knowledge of the word can. Both spellings are in use.

**Decision (2026-10-19, measured):** aksar never changes one into the other automatically.
Correcting OCR text to the spelling in the bundled ICU list rewrote 45 of 940 correct UDHR
tokens (13 even when only unreadable spellings were corrected): standard ជើងដ spellings such
as អន្ដរជាតិ, សន្ដិសុខ, ពេញចិត្ដ are missing from the list, which mostly has ជើងត forms. So:
search finds both, the dictionary accepts both, the spelling read is kept, and the listed
spelling is offered as a suggestion (`Dictionary::spellings`, `read::Word::suggestions`).
An automatic choice needs a trustworthy spelling source (e.g. a licensed standard
dictionary) first.

Measured on the bundled word list (2026-10-18): 4,692 entries contain ្ត and 2,576 contain ្ដ.
877 words are listed in **both** spellings (ស្តី/ស្ដី, ក្តី/ក្ដី, ប្តី/ប្ដី …), and 186 entries
mix them (ចិត្ដ next to ចិត្ត, ឧត្ដម next to ឧត្តម).

Bugs this causes in v0.1.0:
1. **Search misses:** a query typed with ្ដ does not find a page that has ្ត (`fts_query("ស្ដាប់")`
   does not match `search_tokens("ស្តាប់")`), and the reverse.
2. **The corrector treats a TA/DA swap as an edit.** It picks one of two look-alike
   spellings without a reason: ចិដ្ដ → ចិត្ដ, a mixed form, where ចិត្ត is the common one.

Plan:
- **Search (v0.1.0):** `index_form` folds ្ដ → ្ត for tokens and queries. Either
  spelling then finds both. Apps must rebuild stored search tokens once (no re-OCR).
- **Dictionary (v0.2):**
  - Look words up by their folded form, so a TA/DA difference never makes a word unknown
    and never counts as an edit.
  - Keep a `folded → spellings` table built from the word list.
- **Which spelling to output (v0.2):**
  - `Spelling::AsRead` is the default and keeps what OCR/typing produced, unless only one
    spelling is a known word; then that one is used.
  - `Spelling::Preferred` uses a preference table, one entry per pair, for apps that want one
    consistent spelling. The table comes from a licensed or project-made source; record it.
  - Mixed forms like ចិត្ដ map to the pair's preferred form when the table has one.
- **Word list cleanup:** flag the 186 mixed entries. Keep them, so they're found, but never
  output them under `Preferred`.
- **OCR engine (OCR_PLAN §4.1):** the recognizer has **one class for both subscripts**; the
  decoder picks the spelling from the dictionary or LM. Training labels are folded for that
  class, so the model never has to learn an impossible visual difference.
- **Metric:** the primary visual-accuracy number is CER with ្ត/្ដ folded. Raw CER is reported
  as "spelling agreement".
- **No-harm:** the no-harm corpus stays at 0 changes under `AsRead`.

### M4. Symbol alternatives scoring in aksar — 1 week

An app can try Tesseract's alternative symbols (within 25 confidence points) and keep one
that makes the word known. Move that scoring into aksar:

- API: `Dictionary::best_alternative(symbols: &[(String, f32, Vec<(String, f32)>)]) -> Option<String>`.
- Use windowed scoring (only the clusters around the changed symbol), allow up to 2 swaps
  in a word, prefer the highest combined confidence among candidates that make every cluster
  known; ties → no change.
- Test: single- and multi-swap cases; benchmark gate.

### M5. Stray Latin and junk in Khmer lines — 3 days (app's OCR engine, Khmer-aware)

9% of edits involve Latin/ASCII inside Khmer lines. Use aksar's dictionary: in a Khmer line, drop a Latin fragment of ≤ 2 letters with low
confidence when the Khmer text around it segments into known words without it. Measure on
the benchmark (Khmer pages include real English lines, so the no-harm check is "English
lines unchanged").

### M6. The extra-េ insertion (top single error) — 2 days investigation

19 extra េ in 27 pages. First find the pattern with the error report (which words, which
fonts, next to which clusters: likely a split vowel ើ/ោ/ៅ read as េ + vowel, or the
pre-base vowel duplicated). Then fix in the narrowest place: normalization (if it is
`េ` + split vowel), corrector (M2), or Tesseract config. Test with the found cases.

### M7. Legacy Limon / ABC → Unicode converter — 3 weeks

Many older Khmer PDFs store text in legacy font encodings (glyphs in Latin code points), so
extracted text is garbage even though the page looks fine; OCR is the only fix today.
Existing mapping tables are GPL/LGPL/unlicensed, so they can't be reused.

- `aksar::legacy` (feature `legacy`): `detect(font_name, text) -> Option<LegacyFont>`,
  `to_unicode(text, LegacyFont) -> String`.
- Tables **authored by the project**: for each legacy font, render every code point with
  skrifa, match the glyph to Khmer Unicode glyphs (outline comparison against a reference
  Unicode font, then manual review of the unmatched ones), and record the mapping. Then
  convert visual order to logical order (pre-base vowels and coeng-RO stored before the
  consonant) and run `normalize`.
- Test: round trip on sample PDFs per font; every output line passes `validate`.
- Apps: in born-digital text extraction, when the font is a known legacy font, convert
  instead of OCR.

### M8. Khmer spacing — 1 week (later)

Spaces are exact on 88% of lines. Khmer spaces separate phrases, not words, so the
dictionary can't place them; but it can veto a space **inside** a dictionary word. In
the engine that places spaces, for Khmer pairs, if the gap is near the space threshold and
the two sides join into a known word, drop the space. Measure space-exact lines.

### Ongoing

- Validator/normalizer: keep the HarfBuzz fuzz test in CI; run a large-seed fuzz (≥ 300k
  cases) nightly or before each release.
- Word list: track ICU updates of `khmerdict.txt`; re-run `clean_wordlist`; add a project
  supplement list (names of provinces, ministries, common terms missing from ICU) with its
  own license file.

## 4. Schedule and targets

| Milestone | Effort | Target (synthetic unless noted) |
|-----------|--------|---------------------------------|
| M0 | 1 wk | real-scan baseline exists |
| M1 | 2 wk | weak-font CER −5% relative or better (gate), aim Noto Sans/Kantumruy < 4% |
| M2 | 1 wk | Khmer CER below 2.93% with 0 no-harm changes |
| M3 | 1 wk | search finds both spellings; no TA/DA swap counted as an edit; raw vs folded gap halved under `Preferred` |
| M4 | 1 wk | CER down, no page worse |
| M5, M6 | 1 wk | Latin-related and extra-េ edits halved |
| M7 | 3 wk | legacy PDFs extract as valid Unicode |
| M8 | 1 wk | space-exact lines > 92% |

Overall aim after M1–M4: Khmer CER ≤ 2.0% on the synthetic corpus, and a measured real-scan
number with no regression. Targets are goals, not claims: publish only measured results.

## 5. Risks

| Risk | Mitigation |
|------|------------|
| Corrector "fixes" correct text | no-harm corpus test at 0 changes; fix only unique edits |
| Overfitting to the synthetic corpus | confusion pairs learned on the training eval set; real scans in every gate |
| Fine-tune forgets good fonts | anchor fonts in the mix; eval gate on Battambang/Noto Serif |
| Legacy tables legally unclear | author tables ourselves from fonts; record method per font |
| Frequency data license (M3) | ship only derived preference pairs, not the source text |
