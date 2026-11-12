#!/usr/bin/env bash
# Evaluate a model directory on line sets: rten parity check, then aksar-bench with
# Tesseract (when installed) for comparison. Writes <model-dir>/results.json (for
# package_model.py) and <model-dir>/results.md (the table, for docs/OCR_RESULTS.md).
#
# usage: training/evaluate.sh <model-dir> <set-dir>... [-- extra aksar-bench options]
#   e.g. training/evaluate.sh out/print-v1 data/ekyc-split/test data/s2 -- --lang khm+eng
set -euo pipefail

if [ $# -lt 2 ]; then
  echo "usage: training/evaluate.sh <model-dir> <set-dir>... [-- extra aksar-bench options]" >&2
  exit 2
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
model_dir="$(cd "$1" && pwd)"
shift
sets=()
while [ $# -gt 0 ] && [ "$1" != "--" ]; do
  sets+=("$(cd "$1" && pwd)")
  shift
done
[ "${1:-}" = "--" ] && shift
extra=("$@")
if [ ${#sets[@]} -eq 0 ]; then
  echo "evaluate.sh: give at least one line set" >&2
  exit 2
fi

cd "$root"
echo "== check_model"
cargo run --quiet --release -p aksar-recog --example check_model -- "$model_dir"

tess=()
if command -v tesseract >/dev/null 2>&1; then
  tess=(--tesseract)
else
  echo "note: tesseract is not installed; results will have no comparison and the release gate will fail" >&2
fi

echo "== aksar-bench"
cargo run --quiet --release -p aksar-bench -- --set "${sets[@]}" --model-dir "$model_dir" \
  ${tess[@]+"${tess[@]}"} --json "$model_dir/results.json" ${extra[@]+"${extra[@]}"} \
  | tee "$model_dir/results.md"
echo "wrote $model_dir/results.json and results.md"
