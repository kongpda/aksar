# Security

## Reporting a problem

Please report security problems privately: on GitHub, open the repository's **Security** tab
and choose **Report a vulnerability**. Do not open a public issue. Include the input that
triggers the problem (text, font, image or model file) and the crate and version.

## Supported versions

aksar is pre-1.0: the latest `0.x` release gets fixes.

## What aksar treats as untrusted

Every input can come from a user or a scanned document:

| Input | Where | Expected behavior on bad input |
|-------|-------|-------------------------------|
| Text | every crate | Must not panic (fuzzed). Work grows about linearly with the text; the dictionary corrector is bounded per token. Callers should still bound input size, as with any parser. |
| Font bytes | `aksar::shape`, `aksar-render`, `aksar-wasm` | Returns an error. Parsing is done by `harfrust` and `skrifa`. |
| PNG images | `aksar-ocr::decode_png` | Returns an error for corrupt files and for images over the size limit. |
| Page images | `aksar-image::prepare_page`, `aksar-recog`, `aksar-ocr` | Returns an error for pages over `aksar_image::MAX_PIXELS`, checked before any allocation. A line the model fails on reads as empty text with confidence 0. |
| RGBA buffers | `aksar-ocr`, `aksar-image` | `aksar-ocr` returns an error when the length does not match width × height; `aksar-image`'s `Gray::new` / `Gray::from_rgba` panic (documented under `# Panics`), so check lengths before calling them with untrusted sizes. |
| Render sizes | `aksar-render` | Returns an error for sizes that are not finite and positive, and for images over the pixel limit. |
| ONNX models | `aksar-recog`, `aksar-ocr` | Parsed by `rten`. `Recognizer::new` rejects a model whose output does not match the alphabet or is not log-probabilities. Only load models you trust: a model decides what the recognizer reads. |
| Model output | `aksar-decode` | NaN and infinite scores are cleaned; beam memory stays bounded by the beam width, not the line length. |
| Training checkpoints | `training/*.py` | Loaded with `torch.load(..., weights_only=True)`. |

The `aksar` core crate has `#![forbid(unsafe_code)]`. The fuzz targets in [`fuzz/`](fuzz/README.md)
cover text, dictionary, shaping and page images. The CLI exits cleanly (status 0) when its
output pipe closes, for example `aksar read big.txt | head -1`.

Dependency licenses and advisories are checked with `cargo deny` ([`deny.toml`](deny.toml))
and the `audit` workflow.
