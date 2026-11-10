# Contributing

Thanks for helping with aksar. The plan is in [`docs/PLAN.md`](docs/PLAN.md) and open work is
in [`docs/CHECKLIST.md`](docs/CHECKLIST.md).

## Toolchains

- `aksar` and most crates: Rust 1.85 or newer (the MSRV).
- `aksar-ocr`, `aksar-recog` and `aksar-bench`: Rust 1.94 or newer (needed by `rten`).
- WASM: the `wasm32-unknown-unknown` target. Python: `maturin` 1.x (CI pins the exact version).

## Before you open a pull request

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p aksar --no-default-features
cargo test -p aksar --all-features
cargo test -p aksar-cli --features ocr
```

CI also checks MSRV 1.85, `no_std` and WASM builds, WASM size budgets
(`scripts/check-wasm-size.sh`), the Python bindings and `cargo deny`.

## How changes are made

- One fix or feature per commit, with a test that fails without it. The test should say why
  the behavior matters, not only what it is.
- Khmer output changes (normalize, repair, search tokens, corrections) go in
  [`CHANGELOG.md`](CHANGELOG.md) and say whether a search index must be rebuilt.
- Shaping and validity are checked against HarfBuzz. To regenerate the golden data, see
  "Tests" in the [README](README.md#tests).
- Every accuracy claim is a measurement written into
  [`docs/OCR_RESULTS.md`](docs/OCR_RESULTS.md).
- New dependencies must pass `cargo deny check` and stay commercial-friendly.
- Committed test data and fonts need a license that allows redistribution (OFL for fonts).
  Record it in `NOTICE`. Training data stays out of git (`/data/` is ignored); its
  `manifest.json` records sources and licenses.

Security problems: see [`SECURITY.md`](SECURITY.md), not public issues.

By contributing you agree that your work is licensed under MIT OR Apache-2.0, as the code is.
