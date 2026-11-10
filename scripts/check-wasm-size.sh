#!/usr/bin/env bash
# Builds the WASM module and fails if it exceeds the size budget (bytes, before wasm-opt).
set -euo pipefail
cd "$(dirname "$0")/.."

check() {
  local name=$1 profile=$2 budget=$3; shift 3
  cargo build -q -p aksar-wasm --target wasm32-unknown-unknown --profile "$profile" "$@"
  local WASM=target/wasm32-unknown-unknown/$profile/aksar_wasm.wasm size gz
  size=$(wc -c < "$WASM"); gz=$(gzip -9c "$WASM" | wc -c)
  printf '%-16s %8d bytes (%d gzip), budget %d\n' "$name" "$size" "$gz" "$budget"
  if [ "$size" -gt "$budget" ]; then echo "::error::$name WASM is over budget"; exit 1; fi
}

# Budgets leave ~15% headroom over the sizes measured on 2026-10-18.
check "shaping (fast)"  wasm-release 1179648
check "shaping (small)" wasm-small    921600
check "validate-only"   wasm-small     81920 --no-default-features
check "read (small)"    wasm-small   3145728 --no-default-features --features read
