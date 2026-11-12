# Training aksar's own recognizer

The model is small (2.1 M parameters: 5 conv layers, 2-layer BiLSTM, CTC over Unicode code
points), so it runs on phones and in the browser. It trains on a CPU for experiments; a full
print model wants a GPU (CUDA, or Apple Silicon with `--device mps`).

Everything below is tested end to end, on the CPU and on a few lines, by
`training/smoke_test.sh` (about two minutes). Run it first on a new machine:

```sh
python3 -m venv venv && venv/bin/pip install -r training/requirements.txt
PYTHON=venv/bin/python training/smoke_test.sh
```

## The v0.2 print model, step by step

What you supply: a GPU, the fonts, the EKYC benchmark download, and (for S2) real scans.
Data stays out of git (`/data/` is ignored).

```sh
# 1. Synthetic lines (seeded: same seed, same set). Fonts: a folder of Khmer .ttf/.otf
#    files whose licenses allow training (record them for the model card).
cargo run --release -p aksar-render --example make_lines -- fonts/ data/synth-real  200000 1001 real alphabet
cargo run --release -p aksar-render --example make_lines -- fonts/ data/synth-noisy 100000 1002 noisy
cargo run --release -p aksar-render --example make_lines -- fonts/ data/val          2000 2002 real noisy

# 2. EKYC real crops, split once by page. Keep the seed: the test split never changes,
#    even if more pages are added later.
python3 research/khmer/ekyc/convert.py <ekyc-download> data/ekyc
venv/bin/python training/split_set.py data/ekyc data/ekyc-split   # train/ test/ split.json

# 3. Train. --holdout refuses to start if any test image is in the training or validation data.
venv/bin/python training/train_crnn.py data/synth-real,data/synth-noisy,data/ekyc-split/train \
    data/val runs/print-v1 --epochs 20 --batch 64 --device cuda --amp \
    --holdout data/ekyc-split/test
#    Interrupted? Run the same command with --resume.

# 4. Check and measure: rten parity, then aksar vs Tesseract (install tesseract with the
#    khm and eng data). Add data/s2 here once the real-scan set exists.
training/evaluate.sh runs/print-v1 data/ekyc-split/test -- --lang khm+eng

# 5. Gate and package (OCR_PLAN §8). Refuses if aksar is not below Tesseract on every set,
#    or any line fails to read or validate. Nothing is uploaded.
venv/bin/python training/package_model.py runs/print-v1 --results runs/print-v1/results.json \
    --name print-v1 --out release
```

Then: fill in "To write before release" in `release/print-v1/MODEL_CARD.md`, copy the table
from `runs/print-v1/results.md` into [`docs/OCR_RESULTS.md`](../docs/OCR_RESULTS.md), and
read pages with it:

```sh
cargo run --release -p aksar-cli --features ocr -- ocr --model-dir release/print-v1 page.png
export AKSAR_MODEL_DIR=$PWD/release/print-v1   # or once, for every `aksar ocr`
```

The line counts and epochs above are starting points, not measured optima: the 2.1 M model
reached the results in `docs/OCR_RESULTS.md` on 30,000 lines on a CPU. Watch `log.tsv`
(validation CER per epoch) and stop adding epochs when it flattens.

No GPU at hand: Kaggle notebooks (about 30 free GPU hours a week) and Google Colab run the
same commands; on a Mac, `--device mps`. `--device auto` (the default) picks CUDA, then mps,
then the CPU.

## The scripts

| Script | Does |
|---|---|
| `train_crnn.py` | Trains; writes `model.onnx`, `alphabet.json`, `reference.txt`, `manifest.json`, `log.tsv`, `model.pt`, `state.pt` |
| `export_onnx.py <weights.pt> <alphabet.json> <out-dir>` | Exports a checkpoint (also `state.pt`) while training runs |
| `split_set.py <set> <out-dir>` | Splits a real line set into train and test by source page, reproducibly |
| `evaluate.sh <model-dir> <set>...` | `check_model`, then `aksar-bench` (with Tesseract if installed); writes `results.json` and `results.md` |
| `package_model.py <run-dir> --results ... --name ...` | Verifies the checksum and parity, applies the gate, writes the release folder with `MODEL_CARD.md` and `SHA256SUMS` |
| `smoke_test.sh` | All of the above on a few lines, on the CPU |
| `make_tiny_test_model.py` | The 4-symbol model in `testdata/models/` that the Rust tests use |

A line set is a folder with a `labels.tsv` (`file<TAB>text[<TAB>font]`; paths may be relative
to the folder) or `<stem>.png` + `<stem>.gt.txt` pairs. `train_crnn.py` and `aksar-bench`
read both.

### `train_crnn.py` options

- `--device auto|cpu|cuda|mps`, `--amp` (CUDA mixed precision: bf16 where the GPU has it,
  else fp16 with loss scaling). The CTC loss runs in float32 either way, on the CPU for mps.
- `--workers N`: image loading processes (default 4; 0 loads in the training process).
- `--holdout <set>` (repeatable): test sets that must share no image (by SHA-256) with
  training or validation. Validation images that are also training images are refused too.
- `--resume`: continue from `<out>/state.pt` (saved every epoch and every `--checkpoint-mins`).
- `--init <weights.pt>`: start from earlier weights with a new schedule; add
  `--init-alphabet <alphabet.json>` when that model's alphabet differs.

### Files of a model folder

- `model.onnx`: input `line` 1x1x48xW (gray 0..1, white paper = 1), output `logprobs`
  [W/4, 1, symbols], log-softmax. rten loads it as is; no conversion step.
- `alphabet.json`: the symbols, index 0 the CTC blank.
- `reference.txt`: PyTorch's output on a fixed line. `cargo run --release -p aksar-recog
  --example check_model -- <dir>` runs the same line in rten and fails beyond 1e-4.
- `manifest.json`: every training set with its line count, fonts and a checksum of its images
  and labels; settings; git commit, torch version and device; best validation CER; the
  model's size and SHA-256. Needed for every released model (OCR_PLAN §6).
