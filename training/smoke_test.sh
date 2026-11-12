#!/usr/bin/env bash
# End-to-end check of the training pipeline on a few synthetic lines, on the CPU, in
# a couple of minutes: render lines, split a "real" set by page, train one epoch,
# export, check rten against PyTorch, benchmark, package. It proves the scripts and
# the Rust side fit together; the model it trains is useless.
#
# usage: training/smoke_test.sh [work-dir]   (PYTHON=venv/bin/python to pick Python)
# needs: Rust, and Python with torch, onnx, pillow, numpy (training/requirements.txt)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
py="${PYTHON:-python3}"
work="${1:-$(mktemp -d)}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
cd "$root"
fonts="$work/fonts"
mkdir -p "$fonts"
cp testdata/fonts/NotoSansKhmer-Regular.ttf "$fonts/"

step() { printf '\n== %s\n' "$*"; }
expect_fail() {  # expect_fail <what> <command...>: the command must fail
  local what="$1"; shift
  if "$@" >"$work/expect_fail.log" 2>&1; then
    echo "FAIL: $what was accepted" >&2
    exit 1
  fi
  echo "ok: $what refused: $(tail -n 1 "$work/expect_fail.log")"
}

step "render lines"
cargo build --quiet --release -p aksar-render --example make_lines
make_lines=target/release/examples/make_lines
"$make_lines" "$fonts" "$work/train" 64 1001 noisy >/dev/null
"$make_lines" "$fonts" "$work/val" 16 2002 >/dev/null
# A stand-in for a real set in the EKYC layout: <page>_<n>.png + .gt.txt, 4 crops per page.
"$make_lines" "$fonts" "$work/real-src" 40 3003 real >/dev/null
mkdir -p "$work/real"
n=0
while IFS=$'\t' read -r file text _; do
  page=$((n / 4)); crop=$((n % 4)); n=$((n + 1))
  cp "$work/real-src/$file" "$work/real/doc_p${page}_${crop}.png"
  printf '%s\n' "$text" >"$work/real/doc_p${page}_${crop}.gt.txt"
done <"$work/real-src/labels.tsv"

step "split the real set by page"
"$py" training/split_set.py "$work/real" "$work/real-split" --test 0.3

step "train refuses a leaked test split"
expect_fail "a validation set inside --holdout" \
  "$py" training/train_crnn.py "$work/train" "$work/real-split/test" "$work/leak" \
  --epochs 1 --device cpu --workers 0 --holdout "$work/real-split/test"

step "train (1 epoch, CPU)"
"$py" training/train_crnn.py "$work/train,$work/real-split/train" "$work/val" "$work/run" \
  --epochs 1 --batch 8 --device cpu --workers 0 --threads 2 --holdout "$work/real-split/test"

step "re-export the best weights and check parity"
"$py" training/export_onnx.py "$work/run/model.pt" "$work/run/alphabet.json" "$work/export"
cargo run --quiet --release -p aksar-recog --example check_model -- "$work/export"

step "evaluate"
training/evaluate.sh "$work/run" "$work/real-split/test"

step "package"
expect_fail "packaging without a passed gate" \
  "$py" training/package_model.py "$work/run" --results "$work/run/results.json" --name smoke --out "$work/release"
"$py" training/package_model.py "$work/run" --results "$work/run/results.json" --name smoke \
  --out "$work/release" --allow-failed-gate --no-check
(cd "$work/release/smoke" && sha256sum -c --quiet SHA256SUMS 2>/dev/null || shasum -a 256 -c SHA256SUMS)
grep -q "Gate not passed" "$work/release/smoke/MODEL_CARD.md"

step "the CLI reads a page with the packaged model"
cargo run --quiet --release -p aksar-cli --features ocr -- ocr --model-dir "$work/release/smoke" --json \
  "$work/real/doc_p0_0.png" >"$work/page.json" || [ $? -eq 1 ]  # 1: words to review
grep -q '"schema":1' "$work/page.json"

printf '\nsmoke test passed (work dir: %s)\n' "$work"
