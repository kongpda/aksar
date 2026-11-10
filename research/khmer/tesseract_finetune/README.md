# Fine-tuning Tesseract `khm` with aksar-render lines (accuracy plan M1)

Free, CPU-only recipe. Takes about 40 minutes on 4 cores.

```sh
# 1. Fonts: weak fonts twice (weighting), strong fonts once (so nothing is forgotten).
#    OFL fonts only; check each license.
mkdir ftfonts && cp NotoSansKhmer*.ttf KantumruyPro*.ttf Moul-Regular.ttf ftfonts/   # twice, renamed a-/b-
cp Battambang-Regular.ttf NotoSerifKhmer*.ttf Hanuman*.ttf Siemreap.ttf ftfonts/

# 2. Lines: random dictionary text, random font/size, scan-like damage (seeded).
cargo run --release -p aksar-render --example make_lines -- ftfonts lines 6000 42

# 3. Tesseract training files (skips labels with characters the model lacks).
combine_tessdata -u khm.traineddata khm.          # tessdata_best khm (not fast)
python3 research/khmer/tesseract_finetune/prep.py lines khm.lstm-unicharset ftdata

# 4. Fine-tune and package.
lstmtraining --model_output out/khm_ft --continue_from khm.lstm --traineddata khm.traineddata \
  --train_listfile ftdata/list.train --eval_listfile ftdata/list.eval --max_iterations 3000
lstmtraining --stop_training --continue_from out/khm_ft_checkpoint \
  --traineddata khm.traineddata --model_output khm.traineddata
```

## Results (2026-10-19, run 1: 5,343 lines, 3,000 iterations)

| Test | Original `khm` (best) | Fine-tuned |
|------|-----------------------|------------|
| Held-out rendered lines (267) | 19.36% CER | 10.67% CER |
| PDFTape full pipeline, 27 Khmer benchmark pages | 2.74% CER, 22.3% WER | **1.62% CER, 9.2% WER** |
| Same, English pages | 0.04% | 0.04% |
| Plain `tesseract` CLI, Khmer pages | 24.36% | 22.16% |

PDFTape pipeline per category: blur, clean, jpeg, lighting, photo, rotated and skew all
better (jpeg 4.08% → 0.87%, skew 5.37% → 1.61%, rotated 4.79% → 1.21%). 17 pages better,
6 worse. The gate still **fails**: `khm_noise_01` went 9.7% → 22.4%. Five of its eight lines
are now almost perfect, but two lines came out in swapped order and the order-sensitive CER
counts that as many errors. Exact line breaks fell 93% → 89%. Plain-CLI per font: Moul
40.8% → 30.2%, while the strong fonts got slightly worse (Noto Serif Khmer 3.4% → 4.1%,
Battambang 0.7% → 0.8%).

**Caveat.** The benchmark pages use the same five fonts as the training lines (different
text). Real scans must confirm the gain before PDFTape ships the model.

**Real documents (EKYC, 2,513 Khmer line crops, 2026-10-19): no gain.** Truth and output
normalized, whitespace ignored, `--psm 7`:

| EKYC task | Original `khm` | Run 1 |
|-----------|----------------|-------|
| All Khmer lines | 32.64% | 32.24% |
| Clean PDF export | 7.83% | 6.84% |
| Messenger-compressed | 24.52% | 22.19% |
| Printed and scanned | 58.00% | 61.84% |
| Scanner app | 23.23% | 24.71% |
| Street photos, good light | 31.08% | 29.42% |
| Low-light photos | 45.48% | 43.54% |

The 41% gain on PDFTape's synthetic pages shrinks to about 1% on real documents, and
scans get worse. **Decision: PDFTape keeps the original `khm`.** Synthetic-only training
needs real data before it ships (accuracy plan M0, S2).

## Run 2 (2026-10-19): continued with 2,688 extra noisy lines — rejected

Continued run 1 for 2,000 more iterations on 7,630 lines (run 1's plus 2,688 lines made with
`make_lines ... noisy`).

| Test | Original | Run 1 | Run 2 |
|------|----------|-------|-------|
| Held-out rendered lines (run 1 set, 267) | 19.36% | 10.67% | **9.49%** |
| Held-out rendered lines (run 2 set, 401) | — | 14.12% | **12.48%** |
| PDFTape full pipeline, 27 Khmer pages | 2.74% | **1.62%** | 3.97% |

Better on single lines, worse on pages: `khm_rotated_00` went 0.24% (run 1) → 61.6%, with
garbage lines (`ជា xs ៩ wap ជជាៗ`) where run 1 read the text; blur, jpeg, lighting and skew
also got worse, and only the noise pages improved (8.3% → 6.9%). The extra noise data taught
the model to read texture as text. **Run 1 stays the candidate.** Line-level error alone is
not a good enough gate: always run the page benchmark.
