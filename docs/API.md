# aksar API — one call, best result

**Promise:** give aksar Khmer **text**, a **file** (PDF, image, plain text), or a **photo**
and get the best result it can produce: clean Unicode text, word boxes, confidence,
suggested fixes. There are no settings to learn first. Defaults are the most accurate
mode; options only trade accuracy for speed or size.

Status: design, 2026-10-18. **The code below is the target API, not what ships today**, so it
does not compile against the current crate (for example `Aksar::new()` returns `Aksar`, and
`read` takes text only). The shipped API is in the rustdoc of `aksar::read`, the README's API parity table, and
the JSON schema [`crates/aksar/schema/document.schema.json`](../crates/aksar/schema/document.schema.json) (`"schema": 1`). `read` for text ships in v0.1; images and photos in v0.2–v0.4,
handwriting in v0.5 ([`PLAN.md`](PLAN.md) §4). Everything runs on the device; no paid AI. Engine internals are in [`OCR_PLAN.md`](OCR_PLAN.md). The
v0.1.0 text functions (`syllable`, `dict`, `search`) stay as the low-level layer under
this API.

## 1. The whole API in one screen

```rust
use aksar::Aksar;

let ak = Aksar::new()?;                  // models: bundled, cached or downloaded (§6)

let doc = ak.read("letter.pdf")?;        // a path, &[u8], an image, or &str — auto-detected
println!("{}", doc.text());              // best plain text, reading order
for w in doc.words() {                   // every word: text, box, confidence, fixes
    if w.needs_review() { println!("{} → {:?}", w.text, w.suggestions); }
}
std::fs::write("letter.json", doc.to_json())?;   // also: to_hocr, to_alto, to_markdown

let t = ak.read("ខ្មែរ​ភាសា")?;            // plain text gets the same treatment:
println!("{}", t.text());                // normalized, repaired, spell-checked, segmented
```

The same three steps in every language:

| Platform | Code |
|----------|------|
| CLI | `aksar read letter.pdf` · `aksar read photo.jpg --json` · `echo "…" \| aksar read -` |
| JavaScript / browser (`aksar-wasm`) | `const ak = await Aksar.load(); const doc = await ak.read(file); doc.text()` (`file`: `File`, `Blob`, `ArrayBuffer`, `ImageBitmap`, canvas or string) |
| Python (`pip install aksar`) | `ak = aksar.Aksar(); doc = ak.read("scan.png"); doc.text` |
| Swift | `let doc = try await Aksar().read(url)` (also `UIImage`, `Data`, `String`) |
| Kotlin | `val doc = Aksar(context).read(uri)` (also `Bitmap`, `ByteArray`, `String`) |
| C | `aksar_read(ak, bytes, len, &opts, &doc)`; `aksar_doc_text(doc)` |
| HTTP (`aksar serve`) | `curl -F file=@scan.pdf localhost:8080/v1/read` → JSON (§5) |

## 2. Input: anything with Khmer in it

`read` accepts `impl Into<Input>`:

```rust
pub enum Input<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),            // file contents; type sniffed from magic bytes
    Path(&'a Path),             // native only
    Image(ImageRef<'a>),        // already-decoded pixels: width, height, stride, Gray8/Rgb8/Rgba8
    Pages(Vec<Input<'a>>),      // several photos = one document (multi-page scan)
}
```

What happens per kind (decided automatically, recorded in `doc.pages[i].source`):

| Input | Path through aksar |
|-------|--------------------|
| Plain text / `.txt` / UTF-8 bytes | `repair` → `normalize` → dictionary check → words, issues. No OCR |
| Legacy-encoded text (Limon/ABC in a `.txt` or PDF font) | detected → `aksar-legacy` converts it exactly |
| PDF with a good Khmer text layer | text taken from the PDF, checked with `validate` and the dictionary; **no OCR** (exact and fast) |
| PDF with a broken text layer (bad ToUnicode, invalid syllables) or scanned pages | page rendered → OCR. A mixed PDF is decided **per page** |
| PNG, JPEG, WebP, TIFF (multi-page), BMP, GIF | decode → OCR |
| HEIC/AVIF (iPhone photos) | decoded by the platform (Swift/Kotlin/browser bindings pass pixels); native Rust build returns `Error::UnsupportedFormat` with a hint |
| Photo of a page | auto: EXIF rotation, page-edge detection, perspective flattening, lighting, then OCR |
| Handwriting | auto: a small modality classifier (print / photo / handwriting per line) picks the right recognizer |
| Mixed Khmer + English / numbers | always on: Latin, digits and punctuation are in the recognizer's alphabet; no language setting needed |
| DOCX, ODT (later) | text extracted from the XML, then like plain text |

Decoders are pure Rust so every target, WASM included, behaves the same:
- `png` and `zune-jpeg` for PNG/JPEG;
- `image-webp` and `tiff` behind features;
- PDF parsing with `lopdf`;
- PDF rendering with **`hayro`** (pure Rust, WASM-capable, still marked experimental).
  Measure it on a real PDF corpus first. Optional `pdfium` feature on desktop for PDFs
  hayro can't draw. Encrypted PDFs → `Error::Encrypted` (caller supplies the password via
  options).

## 3. Output: `Document`

```rust
pub struct Document {
    pub pages: Vec<Page>,
    pub issues: Vec<Issue>,              // document-level: encrypted page skipped, low-res image …
}
impl Document {
    pub fn text(&self) -> String;                    // reading order, paragraphs separated by \n\n
    pub fn words(&self) -> impl Iterator<Item = &Word>;
    pub fn confidence(&self) -> f32;                 // mean over words, weighted by length
    pub fn to_json(&self) -> String;                 // stable schema, versioned ("schema": 1)
    pub fn to_hocr(&self) -> String;
    pub fn to_alto(&self) -> String;
    pub fn to_markdown(&self) -> String;             // headings/lists when layout says so (later: tables)
    pub fn search_tokens(&self) -> Vec<String>;      // aksar::search, ready for a full-text index
}

pub struct Page {
    pub index: usize, pub width: u32, pub height: u32,
    pub source: Source,                  // TextLayer | Legacy | Ocr { model: &'static str } | Text
    pub rotation: u16,                   // detected and corrected (0/90/180/270)
    pub blocks: Vec<Block>,              // Block → lines → words, each with a box
}

pub struct Word {
    pub text: String,                    // final: normalized, corrected if sure
    pub raw: String,                     // what the recognizer read, before correction
    pub bbox: Rect, pub confidence: f32,
    pub suggestions: Vec<Suggestion>,    // ranked alternatives, each with a score
    pub flags: Flags,                    // UNKNOWN_WORD | CORRECTED | LOW_CONFIDENCE | NUMBER | LATIN | HANDWRITTEN
}
impl Word { pub fn needs_review(&self) -> bool; } // LOW_CONFIDENCE or UNKNOWN_WORD
```

**Guarantees** (tested):
- `text()` is always valid, normalized Unicode Khmer: never a dotted-circle sequence,
  never legacy code.
- `raw` is kept, so nothing is lost.
- Numbers, dates and Latin are never "corrected" by the Khmer dictionary.
- The same input and model versions give the same output (deterministic, single- or
  multi-threaded).

## 4. Options (all optional)

```rust
let ak = Aksar::builder()
    .quality(Quality::Best)          // Best (default) | Balanced | Fast
    .user_words(my_words)            // names, company terms: never flagged, preferred in decoding
    .threads(4)                      // default: all cores natively, 1 in WASM (or a worker pool)
    .max_pages(500).max_pixels(80_000_000)   // safety limits for untrusted files
    .password("…")                   // encrypted PDF
    .force_ocr(false)                // true: ignore PDF text layers
    .build()?;

let doc = ak.read_with(input, &ReadOptions { pages: Some(0..10), progress: Some(&cb), cancel: Some(&token) })?;
```

| Quality | Models | Typical use |
|---------|--------|-------------|
| `Best` | print-accurate + hand + full decoding | desktop, server |
| `Balanced` | print-fast + hand + full decoding | mobile |
| `Fast` | print-fast, greedy + grammar only | live camera preview, huge batches |

Progress and cancel:
- Progress is reported per page and per stage.
- `cancel` stops at the next line.
- Async wrappers (`read_async`) exist for JS, Swift and Kotlin.
- A streaming iterator (`ak.pages(input)`) yields pages as they finish, for long PDFs.

## 5. HTTP service (`aksar serve`)

For apps in any language, or a self-hosted server. A Docker image is published with the
models included.

| Endpoint | Request | Response |
|----------|---------|----------|
| `POST /v1/read` | multipart `file` or JSON `{"text": "…"}`; query `quality`, `format=json\|text\|hocr\|alto\|markdown` | the `Document` JSON schema |
| `POST /v1/check` | `{"text": "…"}` | normalized text, issues, suggestions (fast path, no OCR) |
| `GET /v1/health` | — | version, model versions |

- **Limits** (configurable): 50 MB per file, 500 pages, 60 s per page. Requests over the
  limits are refused.
- **Data:** no request data is stored. That's the default and documented.
- **Access:** optional API key header. CORS is off by default.

## 6. Models: no setup by default

`Aksar::new()` finds models in this order:
1. models bundled in the binary (`bundled-models` feature, print-fast);
2. the cache directory (`~/.cache/aksar`, the app container on mobile, IndexedDB on web);
3. a download from the release catalog. Every file is checked against the SHA-256 and size
   in `models.json`. The download is resumable and happens once.

Offline apps call `Aksar::from_dir(path)` or `Aksar::from_bytes(...)`. `ak.models()` lists
names, versions and licenses for "about" screens.

## 7. Errors

`aksar::Error` is a non-exhaustive enum:
- `UnsupportedFormat { detected }`
- `Encrypted`
- `CorruptFile { detail }`
- `TooLarge { limit }`
- `ModelMissing { name }` / `ModelDownload { detail }`
- `Cancelled`

The rules:
- No panics on any input. Fuzzed with `cargo-fuzz` on PDFs, images and text.
- An unreadable page is reported in `doc.issues`; the other pages are still returned.

## 8. Quality bar for "the best API"

| Check | How |
|-------|-----|
| First result in 3 lines of code in every binding | doc tests and example apps per platform; CI runs them |
| Same `Document` JSON from every binding | one shared golden-file test run through Rust, WASM, Python, Swift, Kotlin |
| Defaults are the most accurate mode | B0 benchmark runs through `ak.read` itself, not internals |
| Stable at v1.0 | `#[non_exhaustive]` structs/enums, JSON `schema` version, semver checks (`cargo-semver-checks`) in CI |
| Small where it matters | web: core 24–335 KB gzip, OCR module ≤ 1.2 MB gzip + models on demand |
| Secure for untrusted files | size/page/pixel limits, fuzzing, no network except the model download (which can be disabled) |

## 9. When each part lands

| Version | API |
|---------|-----|
| **v0.1.0** ✅ | `Aksar::new()`, `read(&str)` for text: normalize, repair, check, words, `Document` JSON v0; `aksar read` CLI for text |
| **v0.2.0** | `read(Image)` on line and page images |
| **v0.3.0** | PDF (text layer, legacy fonts, per-page OCR), all image formats, photos, `aksar-wasm` `read`, Swift/Kotlin, `aksar serve` |
| **v0.5.0** | handwriting through the same `read` (auto modality) |
| **v1.0.0** | API and JSON schema frozen; published to crates.io, npm, PyPI, Swift Package, Maven, Docker |
