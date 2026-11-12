#!/usr/bin/env bash
# Builds the WASM module and fails if it exceeds the size budget (bytes, before wasm-opt).
set -euo pipefail
cd "$(dirname "$0")/.."

check() {
  local name=$1 profile=$2 budget=$3; shift 3
  cargo build -q -p aksar-wasm --target wasm32-unknown-unknown --profile "$profile" "$@"
  local WASM=${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/$profile/aksar_wasm.wasm size gz
  size=$(wc -c < "$WASM"); gz=$(gzip -9c "$WASM" | wc -c)
  printf '%-16s %8d bytes (%d gzip), budget %d\n' "$name" "$size" "$gz" "$budget"
  if [ "$size" -gt "$budget" ]; then echo "::error::$name WASM is over budget"; exit 1; fi
}

# Budgets leave headroom over the sizes measured on 2026-10-18. The shaping
# budgets were raised on 2026-11-11 for harfrust 0.15 (small: 845 KB -> 988 KB,
# fast: 1.06 MB -> 1.13 MB): small is 1 MiB, fast 1.25 MiB. Raise them on purpose
# when a dependency upgrade needs it, and note the new sizes here.
check "shaping (fast)"  wasm-release 1310720
check "shaping (small)" wasm-small   1048576
check "validate-only"   wasm-small     81920 --no-default-features
check "read (small)"    wasm-small   3145728 --no-default-features --features read
