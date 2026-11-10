# Training aksar's own recognizer

Free, CPU-only. The model is small (2.1 M parameters: 5 conv layers, 2-layer BiLSTM, CTC over
Unicode code points) so it trains on a laptop and runs on phones and in the browser.

```sh
# 1. Lines (seeded): train and validation sets from different seeds.
cargo run --release -p aksar-render --example make_lines -- fonts/ data/train 30000 1001 noisy
cargo run --release -p aksar-render --example make_lines -- fonts/ data/val 1000 2002 noisy

# 2. Train (PyTorch, CPU is fine). Writes model.onnx, alphabet.json, manifest.json, log.tsv.
python3 -m venv venv && venv/bin/pip install torch onnx pillow numpy
venv/bin/python training/train_crnn.py data/train data/val out --epochs 8 --threads 4

# 3. Read with Rust (rten loads the ONNX file directly; no conversion step).
cargo run --release -p aksar-recog --example read_lines -- out/model.onnx out/alphabet.json data/val
```

`manifest.json` records the data (fonts, line counts, text source) and the training seed;
the `make_lines` seeds are the ones you passed above. Every released model needs one, plus a
model card (OCR_PLAN §6).

Options:

- `make_lines ... alphabet`: also mixes in syllables drawn evenly from the whole alphabet chart,
  so rare subscripts are trained. `real`: line crops like real documents (long lines,
  punctuation, digits, small text, tight crops). Flags combine with `noisy`.
- `train_crnn.py --resume`: continue from `<out>/state.pt`.
- `train_crnn.py --init <weights.pt>`: start from earlier weights with a new schedule; add
  `--init-alphabet <alphabet.json>` when that model's alphabet differs.
- `training/export_onnx.py <weights.pt> <alphabet.json> <out.onnx>`: export a checkpoint while
  training is still running. Results: [`docs/OCR_RESULTS.md`](../docs/OCR_RESULTS.md).
