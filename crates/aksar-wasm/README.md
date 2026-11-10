# aksar-wasm

WebAssembly bindings for [aksar](https://github.com/kongpda/aksar), a Khmer text engine
written in Rust: shaping with a font you supply, cluster validation and normalization.
Everything runs locally in the browser or Node; there are no network calls.

```js
import init, { KhmerFont, normalize, isValid } from "aksar-wasm";
await init();
normalize("កា្រ");   // "ក្រា"
isValid("ាក");       // false
const font = new KhmerFont(new Uint8Array(await (await fetch("NotoSansKhmer-Regular.ttf")).arrayBuffer()));
const run = font.shape("សួស្តី", 32);
run.glyphIds; run.positions; run.clusters;
run.free(); font.free();   // wasm memory is not garbage collected
```

`issues(text)` returns a JSON string: an array of `{kind, start, end, message}`, with
`start`/`end` as JS string indices (UTF-16 code units), so `text.slice(start, end)` is the
problem span. `JSON.parse` it; there is nothing to `free()`.

## `read` needs the dictionary build

`read(text, autocorrect)` (repaired, checked, corrected text and every word as JSON) exists
**only when the module is built with the `read` Cargo feature**, because it bundles the
80,000-word dictionary (about +370 KB gzip). The default build **and the published npm
package are built without it**, so they do not export `read`; `import { read }` fails there.
To get it, build your own module:

```text
cargo build -p aksar-wasm --target wasm32-unknown-unknown --profile wasm-release --features read
wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/wasm-release/aksar_wasm.wasm
```

Without the `shape` feature (`--no-default-features`) the module is a small
validate/normalize-only build with no `KhmerFont`.
