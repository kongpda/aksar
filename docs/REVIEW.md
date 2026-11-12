# Review of the "khmer-shape" implementation plan

Reviewed: `khmer-text-engine-implementation-plan.md` (2026-10-18).
Outcome: the direction is sound (harfrust core, size-first, optional OCR), but the
document has factual bugs, a few unrealistic targets, and gaps that would have
caused real defects. The revised plan is [`PLAN.md`](PLAN.md). Items marked
**verified** were checked by running code in this repo.

## A. Bugs (wrong as written)

| # | Where | Problem | Fix |
|---|-------|---------|-----|
| A1 | Whole file | The plan is pasted ~10 times (4,762 lines). Only lines 1–610 are the latest; 9 older drafts follow with conflicting timelines (14–16 vs 16–21 vs 18–22 weeks), feature-flag lists and exit criteria. | One copy, in `docs/PLAN.md`. |
| A2 | §1.5 | `គ + ្ + រ + ា → ក្រា`: the output base (ក) does not match the input base (គ). | `គ + ្ + រ + ា → គ្រា`. |
| A3 | §1.8 | Third `labels.txt` row is `ក + ្ + ក → ក្ក`. That is a description, not a label: a model trained on it learns to output `+` and `→`. | Label is the final string: `ក្ក`. |
| A4 | Phase 1 | "language = khr / km". `KHR` is the ISO 4217 code for the riel, not a language. | BCP-47 `km`; OpenType language system `KHM `; script tag `khmr`. |
| A5 | §1.5 | Input order omits robat (U+17CC), says shifters always follow subscripts, and says nothing about the one subscript HarfBuzz accepts *after* a vowel. | Order and rules taken from HarfBuzz's Khmer grammar (see PLAN §2). **Verified** against HarfBuzz on 320,000 random sequences. |
| A6 | Phase 1 | "Full Khmer block + symbols" misses the separate **Khmer Symbols** block U+19E0–U+19FF (the lunar-date signs the plan itself mentions) and lek attak U+17F0–U+17F9. | Explicit coverage list; corpus includes both. |
| A7 | §3 Phase 3 | `unicode-linebreak` alone cannot break Khmer lines. UAX #14 puts Khmer in class SA ("needs a dictionary"), so a long Khmer sentence with no spaces gets no break opportunities. ICU4X `icu_segmenter` dictionary mode with Khmer-only data (measured precision 1.00, 42 MB/s), ZWSP fallback. |
| A8 | §4 / Phase 4 | Size target "< 200–400 KB" for shaping WASM, and size chosen over speed. | **Verified:** smallest shaping WASM is 809 KB / 282 KB gzip (opt-level z), but opt-level 3 is 2.2× faster for 1.02 MB / 335 KB gzip. Decision: speed by default, `wasm-small` profile when size matters. Validate/normalize-only: 59 KB / 24 KB. |
| A9 | Phase 1.5 | `sled` is effectively unmaintained (still 0.34.x beta, last release 2024). `redb` and SQLite need a filesystem, so none of them run on `wasm32-unknown-unknown`. `rusqlite` is C, contradicting "pure Rust". | `Store` trait; in-memory store serialized with postcard, the host persists the bytes (IndexedDB / file). |
| A10 | §1.6 / 4.5 | `ort` wraps the ONNX Runtime C++ library: no `wasm32-unknown-unknown`, adds tens of MB natively. `rten` (pure Rust): measured 4.5× smaller WASM and 1.55× faster natively than `tract`. See STACK.md. |
| A11 | Phase 1 exit | "Render identically to HarfBuzz / browser" can't be tested: browsers differ, and rendering ≠ shaping. | Exit = same glyph ids, clusters and positions as pinned HarfBuzz for a pinned font. **Implemented:** `testdata/golden`, all 45 corpus lines match. |
| A12 | Older drafts | "Prefer coeng ta over coeng da when visually identical." Both are correct in different words (្ត vs ្ដ); a blind rewrite corrupts text. | Validator never rewrites it; only a dictionary-driven correction layer may. |
| A13 | §5 profile | `opt-level = "z"` everywhere (measured: 20–50% slower native, 2.2× slower WASM), `panic = "abort"` in the shared release profile, and `overflow-checks = false` (already the default). | opt-level 3 + fat LTO by default; abort only in WASM profiles; `wasm-small` for size. |
| A14 | Phase 1 | "Thin wrapper around harfrust `shape`" rebuilds the shaping plan on every call. | Reusable `Shaper` caches plan + buffer: **measured ~3× faster**. The WASM binding uses it. |

## B. Gaps (missing, would cause defects)

1. **Split vowels.** ើ ឿ ៀ ោ ៅ are drawn in two parts. Users and OCR often produce `េ + ា` instead of `ោ`: it renders the same, so it slips through, but breaks search, sorting and dictionary lookups. → validator warns, normalizer composes. *(implemented)*
2. **Deprecated characters** (U+17A3, U+17A4, U+17B4, U+17B5, U+17D3, U+17D8) need a policy. → warn; normalize U+17A3 → អ, U+17A4 → អា. *(implemented)*
3. **Validator must agree with the shaper.** If "valid" and "renders without ◌" disagree, users get contradictory feedback. → the validator mirrors HarfBuzz's grammar and a test checks agreement. *(implemented)*
4. **Script itemization / font fallback.** Mixed Khmer + Latin + emoji text must be split into runs (script, font) before shaping; the plan only mentions bidi.
5. **Positioning features.** Feature list stops at substitution features; Khmer also uses `dist`, `abvm`, `blwm`, `kern`, `mark`, `mkmk` (harfrust applies them; tests should cover mark placement).
6. **Font licensing detail.** OFL fonts with a Reserved Font Name must be renamed when subset or modified (Noto has none). Several popular Khmer font families are not OFL; check each before bundling.
7. **Personal data.** Handwriting snippets can identify people. Needs explicit opt-in per export, deletion, and a retention limit, not just "local by default".
8. **Collation and search.** Khmer sorting/search needs normalized text; normalizer is the base for this.

## C. Unverified claims to check in Phase 0

The plan states these as fact without sources: "khmerocr-tools" (package name/existence),
UKTR ≈ 6% CER on KHT, Netra-OCR trained on synthetic data, Tesseract `khm` CER 3–11%,
allsorts "full Khmer since ~0.6", harfrust "used by cosmic-text". Keep them as hypotheses
until a source or our own measurement backs them.

## D. Structure and naming

- **Scope outgrew the name.** "khmer-shape" now covers shaping, validation, correction,
  OCR and synthetic data. One crate carrying OCR would break the size goal. → a workspace of
  small crates (PLAN §3).
- **Optional work sat on the critical path.** Phase 1.5 (learning) came before fonts, layout
  and WASM. → MVP phases first; optional phases after v1.0.
- **Recommended name: `aksar`** (អក្សរ, "letters / script"). Short, Khmer-meaningful,
  free on crates.io and GitHub. npm `aksar` is taken by an unrelated package, so the web
  package ships as `aksar-wasm` or `@kongpda/aksar`.
