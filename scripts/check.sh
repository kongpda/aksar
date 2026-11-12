#!/usr/bin/env bash
# Runs the checks CI runs on every platform, in CI's order, so a PR is green the first time.
#   scripts/check.sh          fmt, clippy, tests, feature variants (what CONTRIBUTING asks for)
#   scripts/check.sh --all    also the WASM/no_std builds and size budget, and the Python tests
# Stops at the first failure and names the step that failed.
set -euo pipefail
cd "$(dirname "$0")/.."

all=false
case "${1:-}" in
  --all) all=true ;;
  "") ;;
  -h | --help) sed -n '2,5p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) echo "unknown argument: $1 (try --help)" >&2; exit 2 ;;
esac

step() {
  printf '\n==> %s\n' "$*"
  "$@" || { printf '\nFAILED: %s\n' "$*" >&2; exit 1; }
}

step ./scripts/check-copies.sh
step cargo fmt --all --check
step cargo clippy --locked --workspace --all-targets -- -D warnings
step cargo test --locked --workspace
step cargo test --locked -p aksar --no-default-features
step cargo test --locked -p aksar --all-features
step cargo clippy --locked -p aksar-cli --all-targets --features ocr -- -D warnings
step cargo test --locked -p aksar-cli --features ocr

if $all; then
  step rustup target add wasm32-unknown-unknown thumbv7em-none-eabihf
  step cargo build --locked -p aksar --no-default-features --target wasm32-unknown-unknown
  step cargo build --locked -p aksar --no-default-features --features shape,libm,dict,search --target thumbv7em-none-eabihf
  step ./scripts/check-wasm-size.sh
  # Python: build a wheel and test it in a throwaway venv under target/, never your own env.
  if command -v python3 > /dev/null; then
    venv=target/check-venv
    step python3 -m venv --clear "$venv"
    py=$venv/bin/python; [ -x "$py" ] || py=$venv/Scripts/python.exe
    step "$py" -m pip install -q "maturin==1.16.0" "pytest==9.2.0"
    step "$py" -m maturin build --locked -m crates/aksar-py/Cargo.toml --out target/check-wheels
    step "$py" -m pip install -q --force-reinstall --no-index --find-links target/check-wheels aksar
    step "$py" -m pytest -q crates/aksar-py/tests
  else
    printf '\n(skipped the Python tests: needs python3)\n'
  fi
fi

printf '\nAll checks passed.\n'
