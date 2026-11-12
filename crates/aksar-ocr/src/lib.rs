//! Khmer OCR in one call: an image in, checked text out, on the device.
//!
//! 1. [`aksar_image`] binarizes and straightens the page and finds its lines.
//! 2. [`aksar_recog`] reads each line with a small model (no internet, no
//!    per-page cost), keeping only valid Khmer (grammar-constrained decoding).
//! 3. [`aksar::read`] checks the text: dictionary words, safe fixes, spelling
//!    suggestions and words to review.
//!
//! ```no_run
//! # #[cfg(feature = "png")] {
//! // A directory with model.onnx and alphabet.json (or use `Ocr::new` with the bytes).
//! let ocr = aksar_ocr::Ocr::from_dir("models/print-v1").unwrap().user_words(["ខេមរភាសា"]);
//! let page = ocr.read_png(&std::fs::read("page.png").unwrap()).unwrap();
//! println!("{}", page.text());
//! for word in page.document.words().filter(|w| w.needs_review()) {
//!     println!("check: {} (suggestions: {:?})", word.text, word.suggestions);
//! }
//! # }
//! ```

use aksar::read::{Aksar, Document, Source, write_json_str};
use aksar_image::{Gray, Rect};
use aksar_recog::Recognizer;
/// File names in a model directory (see [`Ocr::from_dir`]).
pub use aksar_recog::{ALPHABET_FILE, MODEL_FILE};

/// What went wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The model or its alphabet could not be loaded, or the model failed.
    Recognizer(aksar_recog::Error),
    /// The image could not be decoded.
    Image(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Recognizer(e) => write!(f, "{e}"),
            Error::Image(e) => write!(f, "image: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<aksar_recog::Error> for Error {
    fn from(e: aksar_recog::Error) -> Self {
        Error::Recognizer(e)
    }
}

/// One line of a page.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OcrLine {
    /// Where the line is on the straightened page.
    pub rect: Rect,
    /// What the recognizer read (best decoder candidate).
    pub recognized: String,
    /// Other readings the decoder kept, best first.
    pub alternatives: Vec<String>,
    /// How sure the recognizer is of `recognized`, 0 to 1 (see
    /// [`aksar_recog::Candidate::confidence`]). Low values are worth a review.
    pub confidence: f32,
}

/// A read page.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OcrPage {
    /// Lines top to bottom, with their boxes.
    pub lines: Vec<OcrLine>,
    /// The checked text (words, fixes, suggestions, review flags). Its
    /// non-blank lines are `lines` in order; a blank line marks a paragraph gap.
    pub document: Document,
}

impl OcrPage {
    /// The checked text, one line per text line.
    pub fn text(&self) -> String {
        self.document.text()
    }

    /// JSON: line boxes and readings plus the checked document (`aksar::read`
    /// schema 1). The format is `"schema": 1`, described by
    /// `schema/page.schema.json` in this crate.
    pub fn to_json(&self) -> String {
        let mut j = String::from("{\"schema\":1,\"lines\":[");
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                j.push(',');
            }
            j.push_str(&format!(
                "{{\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"recognized\":",
                l.rect.x, l.rect.y, l.rect.w, l.rect.h
            ));
            write_json_str(&mut j, &l.recognized);
            let confidence = if l.confidence.is_finite() { l.confidence } else { 0.0 };
            j.push_str(&format!(",\"confidence\":{confidence:.3},\"alternatives\":["));
            for (k, a) in l.alternatives.iter().enumerate() {
                if k > 0 {
                    j.push(',');
                }
                write_json_str(&mut j, a);
            }
            j.push_str("]}");
        }
        j.push_str("],\"document\":");
        j.push_str(&self.document.to_json());
        j.push('}');
        j
    }
}

fn line_from(rect: Rect, candidates: Vec<aksar_recog::Candidate>) -> OcrLine {
    let confidence = candidates.first().map_or(0.0, aksar_recog::Candidate::confidence);
    let mut texts = candidates.into_iter().map(|c| c.text);
    let recognized = texts.next().unwrap_or_default();
    OcrLine { rect, recognized, alternatives: texts.collect(), confidence }
}

/// Largest image accepted, in pixels (64 MP: an A4 page at 600 dpi is 35 MP).
/// Larger images are an [`Error::Image`], so a small file with a huge header
/// cannot exhaust memory.
pub const MAX_PIXELS: u64 = aksar_image::MAX_PIXELS;

fn check_size(width: u32, height: u32) -> Result<(), Error> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::Image(format!("{width}x{height} is larger than {MAX_PIXELS} pixels")));
    }
    Ok(())
}

/// A loaded OCR engine.
pub struct Ocr {
    recognizer: Recognizer,
    text: Aksar,
}

impl Ocr {
    /// `model`: the recognizer (ONNX) bytes; `alphabet_json`: its `alphabet.json`.
    pub fn new(model: Vec<u8>, alphabet_json: &str) -> Result<Self, Error> {
        Ok(Ocr { recognizer: Recognizer::new(model, alphabet_json)?, text: Aksar::new() })
    }

    /// Loads `model.onnx` and `alphabet.json` from a model directory (a
    /// training output or a released model); see
    /// [`aksar_recog::Recognizer::from_dir`].
    pub fn from_dir(dir: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        Ok(Ocr { recognizer: Recognizer::from_dir(dir)?, text: Aksar::new() })
    }

    /// Names and terms to treat as known words, both when choosing between
    /// readings and when checking the text.
    pub fn user_words<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, words: I) -> Self {
        let words: Vec<String> = words.into_iter().map(|w| w.as_ref().to_string()).collect();
        self.recognizer = self.recognizer.with_user_words(&words);
        self.text = self.text.user_words(&words);
        self
    }

    /// Apply the safe automatic fixes when checking (default on). Off keeps
    /// the text as recognized and only flags and suggests.
    pub fn autocorrect(mut self, on: bool) -> Self {
        self.text = self.text.autocorrect(on);
        self
    }

    /// Reads a page or photo. A line the recognizer fails on does not fail the
    /// page: it comes back with empty text and confidence 0 (and adds nothing
    /// to the checked text), like a blank line.
    pub fn read(&self, page: &Gray) -> Result<OcrPage, Error> {
        check_size(page.width, page.height)?;
        let (gray, boxes) = aksar_image::prepare_page(page).map_err(|e| Error::Image(e.to_string()))?;
        let mut lines = Vec::with_capacity(boxes.len());
        for rect in boxes.into_iter().filter(aksar_recog::is_text_box) {
            let candidates = self.recognizer.read_line(&gray.crop(rect)).unwrap_or_default();
            lines.push(line_from(rect, candidates));
        }
        Ok(self.check(lines))
    }

    /// Reads a single line crop (no line finding).
    pub fn read_line(&self, line: &Gray) -> Result<OcrPage, Error> {
        check_size(line.width, line.height)?;
        let rect = Rect { x: 0, y: 0, w: line.width, h: line.height };
        let candidates = self.recognizer.read_line(line)?;
        Ok(self.check(vec![line_from(rect, candidates)]))
    }

    /// Reads RGBA pixels (e.g. from a canvas or a decoded photo).
    /// A buffer that is not `4 * width * height` bytes is an [`Error::Image`].
    pub fn read_rgba(&self, width: u32, height: u32, rgba: &[u8]) -> Result<OcrPage, Error> {
        self.read(&gray_from_rgba(width, height, rgba)?)
    }

    /// Reads a PNG file's bytes.
    #[cfg(feature = "png")]
    pub fn read_png(&self, bytes: &[u8]) -> Result<OcrPage, Error> {
        self.read(&decode_png(bytes)?)
    }

    fn check(&self, lines: Vec<OcrLine>) -> OcrPage {
        let mut document = self.text.read(&page_text(&lines));
        for page in &mut document.pages {
            page.source = Source::Image;
        }
        OcrPage { lines, document }
    }
}

/// [`Gray::from_rgba`] with the size checked instead of a panic: the buffer
/// often comes from outside (a canvas, FFI).
fn gray_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Gray, Error> {
    check_size(width, height)?;
    let want = u64::from(width) * u64::from(height) * 4;
    if rgba.len() as u64 != want {
        return Err(Error::Image(format!("{width}x{height} RGBA needs {want} bytes, got {}", rgba.len())));
    }
    Ok(Gray::from_rgba(width, height, rgba))
}

/// The lines joined into text, with a blank line at each paragraph gap.
fn page_text(lines: &[OcrLine]) -> String {
    let mut text = String::new();
    let gap = paragraph_gap(lines);
    // Lines read as blank add nothing: an empty line would be a paragraph break.
    let mut prev: Option<&OcrLine> = None;
    for l in lines.iter().filter(|l| !l.recognized.trim().is_empty()) {
        if let Some(p) = prev {
            text.push('\n');
            if gap.is_some_and(|g| l.rect.y.saturating_sub(p.rect.y + p.rect.h) > g) {
                text.push('\n');
            }
        }
        prev = Some(l);
        // A line break inside a reading would split it in the document.
        text.push_str(&l.recognized.replace(['\n', '\r'], " "));
    }
    text
}

/// Vertical gap above which a new paragraph starts: a line height, measured
/// as the median line box. None with fewer than 2 lines.
fn paragraph_gap(lines: &[OcrLine]) -> Option<u32> {
    if lines.len() < 2 {
        return None;
    }
    let mut h: Vec<u32> = lines.iter().map(|l| l.rect.h).collect();
    h.sort_unstable();
    Some(h[h.len() / 2])
}

/// Decodes a PNG into grayscale. Refuses images over [`MAX_PIXELS`].
#[cfg(feature = "png")]
pub fn decode_png(bytes: &[u8]) -> Result<Gray, Error> {
    let err = |e: &dyn std::fmt::Display| Error::Image(e.to_string());
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| err(&e))?;
    check_size(reader.info().width, reader.info().height)?;
    let size = reader.output_buffer_size().ok_or_else(|| Error::Image("too large".into()))?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| err(&e))?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    Ok(match info.color_type {
        png::ColorType::Grayscale => Gray::new(w, h, px.to_vec()),
        // Straight to gray: no full-size RGBA copy of a large page.
        png::ColorType::GrayscaleAlpha => Gray::new(
            w,
            h,
            px.as_chunks::<2>().0.iter().map(|p| aksar_image::luma(p[0], p[0], p[0], p[1])).collect(),
        ),
        png::ColorType::Rgb => Gray::new(
            w,
            h,
            px.as_chunks::<3>().0.iter().map(|p| aksar_image::luma(p[0], p[1], p[2], 255)).collect(),
        ),
        png::ColorType::Rgba => Gray::from_rgba(w, h, px),
        png::ColorType::Indexed => return Err(Error::Image("indexed PNG was not expanded".into())),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_a_page_end_to_end_with_a_real_model() {
        // testdata/models/tiny.onnx reads ink as "ក" and paper as blank.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/models/");
        let ocr = Ocr::new(
            std::fs::read(format!("{dir}tiny.onnx")).unwrap(),
            &std::fs::read_to_string(format!("{dir}tiny.alphabet.json")).unwrap(),
        )
        .unwrap();
        // A white page with one dark line of "text".
        let (w, h) = (400u32, 200u32);
        let mut px = vec![255u8; (w * h) as usize];
        for y in 90..110 {
            for x in 40..360 {
                px[(y * w + x) as usize] = 0;
            }
        }
        let page = ocr.read(&Gray::new(w, h, px)).unwrap();
        assert_eq!(page.lines.len(), 1, "{:?}", page.lines);
        // The toy model only knows "ក"; a long bar may read as several.
        let read = &page.lines[0].recognized;
        assert!(!read.is_empty() && read.chars().all(|c| c == 'ក'), "{read:?}");
        assert!(page.lines[0].confidence > 0.5, "{:?}", page.lines);
        assert_eq!(page.text().trim(), read.as_str());
        check_against_schema(&page.to_json());
    }

    /// `to_json` matches `schema/page.schema.json`: every required key with
    /// its type, and the embedded document at its own schema version.
    fn check_against_schema(json: &str) {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../schema/page.schema.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["schema"], schema["properties"]["schema"]["const"]);
        assert_eq!(v["document"]["schema"], 1);
        assert!(v["document"]["pages"].as_array().unwrap().iter().all(|p| p["source"] == "image"));
        let line = &schema["$defs"]["line"];
        let lines = v["lines"].as_array().unwrap();
        assert!(!lines.is_empty());
        for l in lines {
            for key in line["required"].as_array().unwrap() {
                let key = key.as_str().unwrap();
                let ok = match line["properties"][key]["type"].as_str().unwrap() {
                    "integer" => l[key].is_u64(),
                    "number" => l[key].as_f64().is_some_and(|c| (0.0..=1.0).contains(&c)),
                    "string" => l[key].is_string(),
                    "array" => l[key].as_array().is_some_and(|a| a.iter().all(|s| s.is_string())),
                    t => panic!("type {t} not handled by this test"),
                };
                assert!(ok, "{key}: {}", l[key]);
            }
        }
    }

    use super::*;

    fn line(y: u32, h: u32) -> OcrLine {
        OcrLine {
            rect: Rect { x: 0, y, w: 100, h },
            recognized: "ក".into(),
            alternatives: vec![],
            confidence: 1.0,
        }
    }

    #[test]
    fn a_big_gap_starts_a_paragraph() {
        let lines = [line(0, 30), line(40, 30), line(150, 30)];
        let gap = paragraph_gap(&lines).unwrap();
        assert_eq!(gap, 30);
        assert!(40u32.saturating_sub(30) <= gap && 150u32.saturating_sub(70) > gap);
        assert_eq!(paragraph_gap(&lines[..1]), None);
        assert_eq!(page_text(&lines), "ក\nក\n\nក");
    }

    /// A line the recognizer failed on stays on the page as an empty reading
    /// with confidence 0, and adds nothing to the text.
    #[test]
    fn a_failed_line_is_blank_with_zero_confidence() {
        let rect = Rect { x: 0, y: 0, w: 100, h: 30 };
        let blank = line_from(rect, Vec::new());
        assert_eq!((blank.recognized.as_str(), blank.confidence), ("", 0.0));
        assert_eq!(page_text(&[line(0, 30), blank, line(40, 30)]), "ក\nក");
    }

    // A box read as blank (a thin rule, noise) must not add a paragraph break.
    #[test]
    fn blank_lines_are_left_out_of_the_text() {
        let mut rule = line(32, 2);
        rule.recognized = " ".into();
        assert_eq!(page_text(&[line(0, 30), rule, line(40, 30)]), "ក\nក");
    }

    #[test]
    fn json_strings_are_escaped() {
        let mut s = String::new();
        write_json_str(&mut s, "a\"b\\c\nក\u{1}");
        assert_eq!(s, r#""a\"b\\c\nក\u0001""#);
    }

    #[test]
    fn bad_model_and_image_are_errors() {
        assert!(matches!(Ocr::new(vec![0; 4], r#"["", "ក"]"#), Err(Error::Recognizer(_))));
        #[cfg(feature = "png")]
        assert!(matches!(decode_png(b"not a png"), Err(Error::Image(_))));
    }

    #[test]
    fn wrong_rgba_length_is_an_error_not_a_panic() {
        assert!(matches!(gray_from_rgba(2, 2, &[0; 15]), Err(Error::Image(_))));
        assert!(matches!(gray_from_rgba(100_000, 100_000, &[]), Err(Error::Image(_))));
        assert_eq!(gray_from_rgba(2, 1, &[0, 0, 0, 255, 255, 255, 255, 255]).unwrap().pixels, vec![0, 255]);
    }

    /// RGB and gray+alpha PNGs go straight to gray; the pixels must equal the
    /// RGBA path (Rec. 601, alpha over white) that canvases use.
    #[cfg(feature = "png")]
    #[test]
    fn rgb_and_gray_alpha_png_match_the_rgba_path() {
        let rgb = [0u8, 0, 0, 255, 255, 255, 200, 30, 90, 12, 250, 7];
        let ga = [0u8, 0, 0, 255, 128, 77, 255, 10];
        let encode = |color, data: &[u8]| {
            let mut bytes = Vec::new();
            let mut enc = png::Encoder::new(&mut bytes, 2, 2);
            enc.set_color(color);
            enc.write_header().unwrap().write_image_data(data).unwrap();
            bytes
        };
        let rgba: Vec<u8> = rgb.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        let got = decode_png(&encode(png::ColorType::Rgb, &rgb)).unwrap();
        assert_eq!(got.pixels, Gray::from_rgba(2, 2, &rgba).pixels);
        let rgba: Vec<u8> = ga.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect();
        let got = decode_png(&encode(png::ColorType::GrayscaleAlpha, &ga)).unwrap();
        assert_eq!(got.pixels, Gray::from_rgba(2, 2, &rgba).pixels);
    }

    // A tiny file claiming 60000x60000 pixels must be refused before the
    // 10 GB buffer is allocated.
    #[cfg(feature = "png")]
    #[test]
    fn huge_png_header_is_refused() {
        let mut bytes = Vec::new();
        let mut enc = png::Encoder::new(&mut bytes, 60_000, 60_000);
        enc.set_color(png::ColorType::Rgb);
        let mut w = enc.write_header().unwrap();
        w.write_chunk(png::chunk::IDAT, &[]).unwrap();
        drop(w);
        let e = decode_png(&bytes).err().unwrap();
        assert!(e.to_string().contains("larger than"), "{e}");
    }
}
