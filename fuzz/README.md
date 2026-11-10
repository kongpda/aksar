# Fuzz targets

libFuzzer targets for aksar. This directory is its own workspace (excluded from the root one).

| Target | Input | Checks |
| --- | --- | --- |
| `syllable` | any UTF-8 text | `clusters` concatenate back to the input; `normalize` and `repair` are idempotent and never turn valid text (`validate` ok) into broken text |
| `dict` | any UTF-8 text | bundled `Dictionary::check` and `segment` do not panic or time out; segments concatenate back to the token |
| `shape` | any UTF-8 text | `shape` with the test font (Noto Sans Khmer) does not fail or panic; every glyph cluster is a char boundary |
| `image` | 2 bytes (width, height up to 255) + pixels | `aksar_image::prepare_page` does not panic; line boxes stay inside the page |

`aksar_ocr::decode_png` has no target: aksar-ocr depends on rten, which also builds as a dylib
and then fails to link with coverage instrumentation. Its size limit (`MAX_PIXELS`) is covered by unit tests in `crates/aksar-ocr`.

## Run with cargo-fuzz (nightly)

```sh
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run syllable -- -max_total_time=60 -timeout=10
```

Seed the text targets with the shaping corpus for a faster start:

```sh
mkdir -p corpus/syllable && grep -v '^#' ../testdata/corpus/km.txt | split -l 1 - corpus/syllable/l
```

## Run on stable

Coverage instrumentation works on stable; only AddressSanitizer needs nightly. `--target` keeps
build scripts uninstrumented.

```sh
cd fuzz
RUSTFLAGS="--cfg fuzzing -Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 \
  -Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table \
  -Cllvm-args=-sanitizer-coverage-trace-compares -Cdebug-assertions -Coverflow-checks" \
  cargo build --release --bins --target aarch64-apple-darwin   # or x86_64-unknown-linux-gnu
mkdir -p corpus/syllable
target/aarch64-apple-darwin/release/syllable corpus/syllable -max_total_time=60 -timeout=10
```

A crash input is written to the current directory (`crash-*`); replay it by passing the file
instead of a corpus directory. Add it as a regression test before fixing.
