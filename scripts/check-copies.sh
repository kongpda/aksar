#!/usr/bin/env bash
# Packages carry their own copies of the license files (a crate or wheel can only
# ship files inside its own folder). Fails if a copy differs from the original.
set -euo pipefail
cd "$(dirname "$0")/.."

bad=0
same() {
  cmp -s "$1" "$2" || { echo "$2 differs from $1 (copy it again)" >&2; bad=1; }
}
for f in LICENSE-MIT LICENSE-APACHE NOTICE; do
  same "$f" "crates/aksar/$f"
  same "$f" "crates/aksar-py/$f"
done
same crates/aksar/data/khmer-words.LICENSE.txt crates/aksar-py/khmer-words.LICENSE.txt
exit $bad
