//! WebAssembly bindings. Build with:
//!
//! ```text
//! cargo build -p aksar-wasm --target wasm32-unknown-unknown --profile wasm-release   # fast (default)
//! cargo build -p aksar-wasm --target wasm32-unknown-unknown --profile wasm-small     # smallest
//! wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/wasm-release/aksar_wasm.wasm
//!
//! # with `read` (bundles the dictionary, about +370 KB gzip). The default build, and
//! # the published npm package, are built WITHOUT it, so they export no `read`:
//! cargo build -p aksar-wasm --target wasm32-unknown-unknown --profile wasm-release --features read
//! ```
//!
//! ```js
//! import init, { KhmerFont, normalize, isValid, issues } from "./pkg/aksar_wasm.js";
//! await init();
//! const font = new KhmerFont(new Uint8Array(await (await fetch("NotoSansKhmer-Regular.ttf")).arrayBuffer()));
//! const run = font.shape("សួស្តី", 32);
//! run.glyphIds;   // Uint32Array
//! run.positions;  // Float32Array: x_advance, y_advance, x_offset, y_offset per glyph
//! run.clusters;   // Uint32Array: JS string index (UTF-16) where each glyph's cluster starts
//! run.free();     // wasm memory is not garbage collected: free runs and fonts when done
//! font.free();
//!
//! // JSON array of {kind, start, end, message}; start/end are UTF-16 indices into `text`.
//! JSON.parse(issues("ក \u17b6"));   // [{ kind: "broken-cluster", start: 2, end: 3, message: "..." }]
//!
//! // `read` exists only in a build with `--features read` (the dictionary feature); the default
//! // build and the npm package do not export it, so `import { read }` fails there.
//! // With it: repaired, checked, corrected text and every word as JSON.
//! const doc = JSON.parse(read("ទីក្រុងភ្នំពេញ កម្ពជា", true));
//! doc.pages[0].blocks[0].lines[0].text;   // "ទីក្រុងភ្នំពេញ កម្ពុជា"
//! ```
//!
//! Each `GlyphRun` getter builds a new typed array: read it once into a variable
//! instead of in a loop.

use wasm_bindgen::prelude::*;

/// True when the text has no broken clusters (would render without ◌).
#[wasm_bindgen(js_name = isValid)]
pub fn is_valid(text: &str) -> bool {
    aksar::syllable::validate(text).is_ok()
}

/// Canonical NiDA storage order (safe for user input and OCR output).
#[wasm_bindgen]
pub fn normalize(text: &str) -> String {
    aksar::syllable::normalize(text)
}

/// Validation issues as a JSON array of `{"kind","start","end","message"}`, in text order.
/// `start`/`end` are UTF-16 code unit offsets (JS string indices), so
/// `text.slice(start, end)` is the offending text. `kind` is one of `broken-cluster` (the
/// only error), `deprecated`, `split-vowel`, `dangling-coeng`, `deep-stack` or
/// `subscript-after-vowel`: the same names as `aksar check --json` and Python's `issues`.
#[wasm_bindgen]
pub fn issues(text: &str) -> String {
    issues_json(text)
}

fn issues_json(text: &str) -> String {
    let found = aksar::syllable::issues(text);
    let starts = utf16_offsets(text, found.iter().map(|i| i.range.start as u32));
    let ends = utf16_offsets(text, found.iter().map(|i| i.range.end as u32));
    let mut j = String::from("[");
    for (n, i) in found.iter().enumerate() {
        if n > 0 {
            j.push(',');
        }
        let message = i.kind.to_string().replace('\\', "\\\\").replace('"', "\\\"");
        j.push_str(&format!(
            "{{\"kind\":\"{}\",\"start\":{},\"end\":{},\"message\":\"{message}\"}}",
            i.kind.name(),
            starts[n],
            ends[n]
        ));
    }
    j.push(']');
    j
}

/// Reads Khmer text and returns the document as JSON (see aksar `read::Document::to_json`):
/// repaired and normalized text, every word with parts, suggestions and review flags.
/// `autocorrect = false` only suggests fixes instead of applying them.
///
/// Only exists when built with the `read` feature (it bundles the dictionary); the default
/// build and the npm package leave it out.
#[cfg(feature = "read")]
#[wasm_bindgen]
pub fn read(text: &str, autocorrect: bool) -> String {
    aksar::read::Aksar::new().autocorrect(autocorrect).read(text).to_json()
}

/// A parsed font. Create once, shape many times.
#[cfg(feature = "shape")]
#[wasm_bindgen]
pub struct KhmerFont {
    font: aksar::shape::Font,
    /// Last shaper and the features it was built for; reused while they match.
    cache: core::cell::RefCell<Option<(Vec<String>, aksar::shape::Shaper)>>,
}

#[cfg(feature = "shape")]
#[wasm_bindgen]
impl KhmerFont {
    /// Parses TTF/OTF bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: Vec<u8>) -> Result<KhmerFont, JsError> {
        let font = aksar::shape::Font::from_bytes(bytes).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(KhmerFont { font, cache: Default::default() })
    }

    /// Shapes `text` at `size` px (a positive number). Pass `features` like
    /// `["kern=0"]` or nothing.
    ///
    /// The shaping plan is cached, so repeated calls with the same features are fast.
    pub fn shape(&self, text: &str, size: f32, features: Option<Vec<String>>) -> Result<GlyphRun, JsError> {
        if !(size.is_finite() && size > 0.0) {
            return Err(JsError::new(&format!("size must be a positive number, got {size}")));
        }
        let features = features.unwrap_or_default();
        let err = |e: aksar::shape::Error| JsError::new(&e.to_string());
        let mut cache = self.cache.borrow_mut();
        if cache.as_ref().is_none_or(|(f, _)| *f != features) {
            let opts = aksar::shape::ShapeOptions::default().with_features(features.iter().cloned());
            *cache = Some((features, aksar::shape::Shaper::new(&self.font, &opts).map_err(err)?));
        }
        let (_, shaper) = cache.as_mut().expect("cache filled above");
        shaper.set_size(Some(size)).map_err(err)?;
        let run = shaper.shape(text).map_err(err)?;
        let clusters = utf16_offsets(text, run.glyphs.iter().map(|g| g.cluster));
        Ok(GlyphRun { run, clusters })
    }

    /// Design units per em.
    #[wasm_bindgen(getter, js_name = unitsPerEm)]
    pub fn units_per_em(&self) -> u16 {
        self.font.units_per_em()
    }
}

/// Result of [`KhmerFont::shape`].
#[cfg(feature = "shape")]
#[wasm_bindgen]
pub struct GlyphRun {
    run: aksar::shape::GlyphRun,
    /// Cluster starts as UTF-16 offsets (JS string indices).
    clusters: Vec<u32>,
}

/// UTF-8 byte offsets in `text` → UTF-16 offsets, the indices JS strings use.
fn utf16_offsets(text: &str, bytes: impl Iterator<Item = u32>) -> Vec<u32> {
    // Cumulative UTF-16 length at each char boundary (glyph clusters are on boundaries).
    let mut at = Vec::with_capacity(text.len() + 1);
    let mut units = 0u32;
    for (i, c) in text.char_indices() {
        at.resize(i + 1, units);
        units += c.len_utf16() as u32;
    }
    at.resize(text.len() + 1, units);
    bytes.map(|b| at.get(b as usize).copied().unwrap_or(units)).collect()
}

#[cfg(feature = "shape")]
#[wasm_bindgen]
impl GlyphRun {
    /// Glyph ids, in visual order.
    #[wasm_bindgen(getter, js_name = glyphIds)]
    pub fn glyph_ids(&self) -> Vec<u32> {
        self.run.glyphs.iter().map(|g| g.glyph_id).collect()
    }
    /// Where each glyph's cluster starts in the input, as a JS string index
    /// (UTF-16 code units): `text.slice(clusters[i])` begins at that cluster.
    #[wasm_bindgen(getter)]
    pub fn clusters(&self) -> Vec<u32> {
        self.clusters.clone()
    }
    /// Four floats per glyph: x_advance, y_advance, x_offset, y_offset.
    #[wasm_bindgen(getter)]
    pub fn positions(&self) -> Vec<f32> {
        self.run.glyphs.iter().flat_map(|g| [g.x_advance, g.y_advance, g.x_offset, g.y_offset]).collect()
    }
    /// Total advance width.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> f32 {
        self.run.width
    }
    /// Dotted circles inserted for broken clusters.
    #[wasm_bindgen(getter, js_name = dottedCircles)]
    pub fn dotted_circles(&self) -> u32 {
        self.run.dotted_circles as u32
    }
    /// Glyphs the font could not draw (`.notdef`).
    #[wasm_bindgen(getter)]
    pub fn missing(&self) -> u32 {
        self.run.missing as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{issues_json, utf16_offsets};

    /// Offsets are UTF-16 units like `clusters`, so `text.slice(start, end)` works in JS.
    #[test]
    fn issues_use_utf16_offsets_and_stable_names() {
        // 🙂 is 2 UTF-16 units, so the lone vowel ា sits at unit 3.
        let j = issues_json("🙂 \u{17B6}");
        assert_eq!(
            j,
            "[{\"kind\":\"broken-cluster\",\"start\":3,\"end\":4,\"message\":\"broken cluster: renders with a dotted circle\"}]"
        );
        assert_eq!(issues_json("ក្រុង"), "[]");
    }

    /// JS indexes strings in UTF-16 units; Khmer is 3 UTF-8 bytes but 1 UTF-16
    /// unit per char, so byte offsets pointed far past the cluster in JS.
    #[test]
    fn clusters_are_js_string_indices() {
        let text = "ក🙂ខ្មែរ"; // ក: 3 bytes/1 unit, 🙂: 4 bytes/2 units
        let got = utf16_offsets(text, [0u32, 3, 7].into_iter());
        assert_eq!(got, [0, 1, 3]);
        let js: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&js[3..]).unwrap(), "ខ្មែរ");
        assert_eq!(utf16_offsets(text, [text.len() as u32].into_iter()), [js.len() as u32]);
    }
}
