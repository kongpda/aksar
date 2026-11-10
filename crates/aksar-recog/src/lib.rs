//! Khmer line recognizer.
//!
//! Runs the small CTC model trained by `training/train_crnn.py` (CNN + BiLSTM,
//! input: a gray line 48 px high, output: log-probabilities over Unicode code
//! points) with [rten](https://crates.io/crates/rten), then decodes it with
//! [`aksar_decode`]: beam search that never keeps a broken syllable, re-ranked
//! by the dictionary. Pages are cut into lines with [`aksar_image`].
//!
//! ```no_run
//! let model = std::fs::read("model.onnx").unwrap();
//! let alphabet = std::fs::read_to_string("alphabet.json").unwrap();
//! let rec = aksar_recog::Recognizer::new(model, &alphabet).unwrap();
//! # let page = aksar_image::Gray::new(1, 1, vec![255]);
//! for line in rec.read_page(&page).unwrap() {
//!     println!("{}", line.text);
//! }
//! ```

pub use aksar_decode::{Candidate, Decoder, Options};
use aksar_image::{Gray, Rect};
use rten::Model;
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, Tensor};

/// Line height the model was trained on.
pub const HEIGHT: u32 = 48;

/// Boxes wider than this many times their height are not text lines (rules,
/// borders, noise rows): [`Recognizer::read_page`] skips them.
pub const MAX_LINE_ASPECT: u32 = 200;

/// Widest model input, in pixels after scaling to [`HEIGHT`]. A wider line
/// is an [`Error::Line`]: it would need a huge tensor (a 64000x1 crop scales
/// to over 3 billion columns).
pub const MAX_INPUT_WIDTH: u32 = HEIGHT * MAX_LINE_ASPECT;

/// True for a box that can be a text line: not empty and at most
/// [`MAX_LINE_ASPECT`] times wider than high.
pub fn is_text_box(rect: &Rect) -> bool {
    rect.w > 0 && rect.h > 0 && rect.w <= rect.h.saturating_mul(MAX_LINE_ASPECT)
}

/// What went wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The model file could not be loaded.
    Model(String),
    /// `alphabet.json` is not a JSON array of strings starting with `""`.
    Alphabet,
    /// The model output does not match the alphabet, or is not a number.
    Output(String),
    /// The line crop is too wide for the model (see [`MAX_INPUT_WIDTH`]).
    Line(String),
    /// The page is larger than [`aksar_image::MAX_PIXELS`].
    Image(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Model(e) => write!(f, "model: {e}"),
            Error::Alphabet => f.write_str("alphabet must be a JSON array of strings, the first one empty"),
            Error::Output(e) => write!(f, "model output: {e}"),
            Error::Line(e) => write!(f, "line: {e}"),
            Error::Image(e) => write!(f, "image: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// One recognized line of a page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Line {
    /// Where the line is on the (straightened) page.
    pub rect: Rect,
    /// Best reading.
    pub text: String,
    /// All candidates from the decoder, best first.
    pub candidates: Vec<Candidate>,
}

/// A loaded model and its decoder.
pub struct Recognizer {
    model: Model,
    decoder: Decoder,
    symbols: usize,
    /// Decoder settings used by [`Recognizer::read_line`] and [`Recognizer::read_page`].
    pub options: Options,
}

impl Recognizer {
    /// `model`: ONNX (or `.rten`) bytes. `alphabet_json`: the `alphabet.json`
    /// written next to the model by the training script.
    ///
    /// The model is checked here, not at the first line: one blank line is run
    /// through it, and its output must have one step per 4 pixels of width and
    /// one log-probability per alphabet symbol (a model paired with the wrong
    /// alphabet, or not shaped like aksar's, is an [`Error::Model`]). Its rows
    /// must also be log-probabilities (`logsumexp` within 0.01 of 0, a
    /// log-softmax output): a model that ends in raw logits would decode to
    /// plausible-looking garbage, so it is refused here instead of normalized.
    pub fn new(model: Vec<u8>, alphabet_json: &str) -> Result<Self, Error> {
        let alphabet = parse_string_array(alphabet_json).ok_or(Error::Alphabet)?;
        let decoder = Decoder::new(&alphabet).map_err(|_| Error::Alphabet)?;
        let model = Model::load(model).map_err(|e| Error::Model(e.to_string()))?;
        let rec = Recognizer { model, decoder, symbols: alphabet.len(), options: Options::default() };
        const PROBE_WIDTH: u32 = 64;
        let blank = Gray::new(PROBE_WIDTH, HEIGHT, vec![255; (PROBE_WIDTH * HEIGHT) as usize]);
        let frames =
            rec.run(&blank).map_err(|e| Error::Model(format!("does not run on a test line: {e}")))?;
        if frames.len() != (PROBE_WIDTH / 4) as usize {
            return Err(Error::Model(format!(
                "a {PROBE_WIDTH} px line gave {} steps, expected {} (one per 4 px)",
                frames.len(),
                PROBE_WIDTH / 4
            )));
        }
        check_normalized(&frames)?;
        Ok(rec)
    }

    /// Adds names and terms the decoder should treat as known words.
    pub fn with_user_words<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, words: I) -> Self {
        self.decoder = self.decoder.with_user_words(words);
        self
    }

    /// The decoder (for decoding [`Recognizer::frames`] yourself).
    pub fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    /// Raw model output for a line crop (any height; scaled to 48 px; light
    /// text on a dark background is inverted first):
    /// `frames[t][k]` = natural-log probability of symbol `k` at step `t`.
    pub fn frames(&self, line: &Gray) -> Result<Vec<Vec<f32>>, Error> {
        use aksar_image::Polarity;
        // The model reads dark text on light paper: flip signs and labels.
        match aksar_image::polarity(line) {
            Polarity::DarkOnLight => self.run(line),
            Polarity::LightOnDark => self.run(&aksar_image::invert(line)),
            Polarity::Unsure => {
                let plain = self.run(line)?;
                let inverted = self.run(&aksar_image::invert(line))?;
                Ok(if letter_confidence(&inverted) > letter_confidence(&plain) { inverted } else { plain })
            }
        }
    }

    fn run(&self, line: &Gray) -> Result<Vec<Vec<f32>>, Error> {
        check_line(line.width, line.height)?;
        let img = aksar_image::fit_height(line, HEIGHT);
        // The network downsamples width by 4; pad with paper to a multiple of 4.
        let width = (img.width.max(8)).div_ceil(4) * 4;
        let mut input = NdTensor::<f32, 4>::full([1, 1, HEIGHT as usize, width as usize], 1.0);
        for y in 0..img.height as usize {
            for x in 0..img.width as usize {
                input[[0, 0, y, x]] = f32::from(img.pixels[y * img.width as usize + x]) / 255.0;
            }
        }
        let out = self.model.run_one(input.into(), None).map_err(|e| Error::Model(e.to_string()))?;
        let out: Tensor<f32> = out.try_into().map_err(|_| Error::Output("not f32".into()))?;
        let shape = out.shape().to_vec();
        if shape.len() != 3 || shape[1] != 1 || shape[2] != self.symbols {
            return Err(Error::Output(format!("shape {shape:?}, expected [T, 1, {}]", self.symbols)));
        }
        let out = out.to_vec();
        check_output(&out)?;
        Ok(out.chunks(self.symbols).map(<[f32]>::to_vec).collect())
    }

    /// Reads one line crop. Returns the decoder's candidates, best first.
    pub fn read_line(&self, line: &Gray) -> Result<Vec<Candidate>, Error> {
        Ok(self.decoder.decode(&self.frames(line)?, &self.options))
    }

    /// Reads one line with the plain best-path decode (no grammar, no
    /// dictionary): the baseline for measuring what the decoder adds.
    pub fn read_line_greedy(&self, line: &Gray) -> Result<String, Error> {
        Ok(self.decoder.greedy(&self.frames(line)?))
    }

    /// Reads a page: binarize, deskew, find lines ([`aksar_image::prepare_page`]),
    /// then read each line top to bottom. A page over [`aksar_image::MAX_PIXELS`]
    /// is an [`Error::Image`]. A line that fails (see [`Recognizer::read_line`])
    /// does not fail the page: it comes back with empty text, no candidates
    /// and nothing else to tell it from a blank line, so one bad crop cannot
    /// cost the whole page.
    pub fn read_page(&self, page: &Gray) -> Result<Vec<Line>, Error> {
        let (gray, boxes) = aksar_image::prepare_page(page).map_err(|e| Error::Image(e.to_string()))?;
        Ok(boxes
            .into_iter()
            .filter(is_text_box)
            .map(|rect| line_or_blank(rect, self.read_line(&gray.crop(rect))))
            .collect())
    }
}

/// A line from a read, or an empty one when the read failed.
fn line_or_blank(rect: Rect, read: Result<Vec<Candidate>, Error>) -> Line {
    let candidates = read.unwrap_or_default();
    let text = candidates.first().map(|c| c.text.clone()).unwrap_or_default();
    Line { rect, text, candidates }
}

/// Refuses a crop whose width after scaling to [`HEIGHT`] exceeds
/// [`MAX_INPUT_WIDTH`]. Computed the way [`aksar_image::fit_height`] scales.
fn check_line(width: u32, height: u32) -> Result<(), Error> {
    if width == 0 || height == 0 {
        return Ok(()); // fit_height gives a blank 1-pixel-wide line
    }
    let scaled = (f64::from(width) * f64::from(HEIGHT) / f64::from(height)).round();
    if scaled > f64::from(MAX_INPUT_WIDTH) {
        return Err(Error::Line(format!(
            "{width}x{height} scales to {scaled} px wide at {HEIGHT} px high, more than {MAX_INPUT_WIDTH}"
        )));
    }
    Ok(())
}

/// Log-probabilities may be -inf (probability 0) but never NaN or +inf: a
/// broken model would otherwise decode to an empty line without an error.
fn check_output(out: &[f32]) -> Result<(), Error> {
    if out.iter().any(|v| v.is_nan() || *v == f32::INFINITY) {
        return Err(Error::Output("NaN or +inf log-probability (broken model?)".into()));
    }
    Ok(())
}

/// Every row must be a log-softmax output: its probabilities sum to 1.
fn check_normalized(frames: &[Vec<f32>]) -> Result<(), Error> {
    for (t, row) in frames.iter().enumerate() {
        let hi = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let lse = hi + row.iter().map(|&v| (v - hi).exp()).sum::<f32>().ln();
        if lse.is_nan() || lse.abs() > 0.01 {
            return Err(Error::Model(format!(
                "output is not log-probabilities (step {t}: logsumexp is {lse}, expected 0); \
                 end the model with log_softmax"
            )));
        }
    }
    Ok(())
}

/// How sure the model is of the letters it reads: mean log-probability of
/// the best symbol over the steps where that symbol is not the blank. A
/// reading with no letters scores lowest (an inverted page reads as blank,
/// confidently).
fn letter_confidence(frames: &[Vec<f32>]) -> f32 {
    let (mut sum, mut n) = (0.0, 0);
    for f in frames {
        let (k, best) =
            f.iter().enumerate().fold((0, f32::NEG_INFINITY), |a, (k, &v)| if v > a.1 { (k, v) } else { a });
        if k != 0 {
            sum += best;
            n += 1;
        }
    }
    if n == 0 { f32::NEG_INFINITY } else { sum / n as f32 }
}

/// Parses a JSON array of strings (what `json.dump(list_of_str)` writes).
fn parse_string_array(s: &str) -> Option<Vec<String>> {
    let mut it = s.trim_start_matches('\u{FEFF}').trim().chars().peekable();
    let skip_ws = |it: &mut std::iter::Peekable<std::str::Chars>| {
        while it.peek().is_some_and(|c| c.is_whitespace()) {
            it.next();
        }
    };
    if it.next()? != '[' {
        return None;
    }
    let mut out = Vec::new();
    skip_ws(&mut it);
    if it.peek() == Some(&']') {
        it.next();
        return it.next().is_none().then_some(out);
    }
    loop {
        skip_ws(&mut it);
        if it.next()? != '"' {
            return None;
        }
        let mut cur = String::new();
        loop {
            match it.next()? {
                '"' => break,
                '\\' => match it.next()? {
                    '"' => cur.push('"'),
                    '\\' => cur.push('\\'),
                    '/' => cur.push('/'),
                    'n' => cur.push('\n'),
                    't' => cur.push('\t'),
                    'r' => cur.push('\r'),
                    'b' => cur.push('\u{8}'),
                    'f' => cur.push('\u{c}'),
                    'u' => {
                        let hex = |it: &mut std::iter::Peekable<std::str::Chars>| {
                            let h: String = (0..4).filter_map(|_| it.next()).collect();
                            (h.len() == 4 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                                .then(|| u32::from_str_radix(&h, 16).ok())
                                .flatten()
                        };
                        let hi = hex(&mut it)?;
                        let c = if (0xD800..0xDC00).contains(&hi) {
                            if it.next()? != '\\' || it.next()? != 'u' {
                                return None;
                            }
                            let lo = hex(&mut it).filter(|v| (0xDC00..=0xDFFF).contains(v))?;
                            char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))?
                        } else {
                            char::from_u32(hi)?
                        };
                        cur.push(c);
                    }
                    _ => return None,
                },
                c if (c as u32) < 0x20 => return None,
                c => cur.push(c),
            }
        }
        out.push(cur);
        skip_ws(&mut it);
        match it.next()? {
            ',' => continue,
            ']' => break,
            _ => return None,
        }
    }
    skip_ws(&mut it);
    it.next().is_none().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> (Vec<u8>, String) {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/models/");
        (
            std::fs::read(format!("{dir}tiny.onnx")).unwrap(),
            std::fs::read_to_string(format!("{dir}tiny.alphabet.json")).unwrap(),
        )
    }

    #[test]
    fn a_model_is_checked_against_its_alphabet_at_load() {
        let (model, alphabet) = tiny();
        let rec = Recognizer::new(model.clone(), &alphabet).expect("matching model and alphabet");
        // Ink reads as the model's one letter, paper as blank.
        let mut px = vec![255u8; 64 * 48];
        for y in 0..48 {
            for x in 8..16 {
                px[y * 64 + x] = 0;
            }
        }
        assert_eq!(rec.read_line(&Gray::new(64, 48, px)).unwrap()[0].text, "ក");
        // Wrong alphabet size: refused at load, not at the first line.
        let err = Recognizer::new(model.clone(), r#"["", "ក", "ា"]"#).err().expect("size mismatch");
        assert!(matches!(err, Error::Model(_)), "{err}");
        assert!(Recognizer::new(b"not a model".to_vec(), &alphabet).is_err());
    }

    #[test]
    fn alphabet_json_parses() {
        assert_eq!(
            parse_string_array(r#"["", "ក", "្", "\"", "\\"]"#).unwrap(),
            vec!["", "ក", "\u{17D2}", "\"", "\\"]
        );
        assert_eq!(parse_string_array("[]").unwrap(), Vec::<String>::new());
        assert_eq!(parse_string_array(r#"["😀"]"#).unwrap(), vec!["😀"]);
        assert_eq!(parse_string_array("\u{FEFF}[\"a\"]").unwrap(), vec!["a"]);
        for bad in [
            "",
            "[",
            "[\"a\"",
            "[\"a\",]",
            "[1]",
            "[\"a\"] x",
            r#"["\x"]"#,
            r#"["\u+041"]"#,
            r#"["\ud800\u0041"]"#,
            "[\"a\u{1}\"]",
        ] {
            assert!(parse_string_array(bad).is_none(), "{bad}");
        }
    }

    /// A thin, very wide crop must be refused before any allocation, not
    /// scaled to a multi-gigabyte tensor.
    #[test]
    fn too_wide_lines_are_refused() {
        assert!(matches!(check_line(64_000, 1), Err(Error::Line(_))));
        assert!(check_line(MAX_INPUT_WIDTH, HEIGHT).is_ok());
        assert!(matches!(check_line(MAX_INPUT_WIDTH + 1, HEIGHT), Err(Error::Line(_))));
        assert!(check_line(0, 0).is_ok());
        let rect = |w, h| Rect { x: 0, y: 0, w, h };
        assert!(is_text_box(&rect(400, 20)));
        assert!(!is_text_box(&rect(20_000, 3)));
        assert!(!is_text_box(&rect(10, 0)));
    }

    #[test]
    fn broken_model_output_is_an_error() {
        assert!(check_output(&[-0.1, -2.3, f32::NEG_INFINITY]).is_ok());
        assert!(matches!(check_output(&[-0.1, f32::NAN]), Err(Error::Output(_))));
        assert!(matches!(check_output(&[f32::INFINITY]), Err(Error::Output(_))));
    }

    /// A model that ends in raw logits passes the shape check but its numbers
    /// are not log-probabilities; the decoder's scores would be meaningless.
    #[test]
    fn raw_logits_are_not_log_probabilities() {
        let ok = vec![vec![0.5f32.ln(), 0.25f32.ln(), 0.25f32.ln()]; 3];
        assert!(check_normalized(&ok).is_ok());
        // Log-probabilities may contain -inf (probability 0).
        assert!(check_normalized(&[vec![0.0, f32::NEG_INFINITY]]).is_ok());
        let logits = vec![vec![2.0, -1.0, 0.5]; 3];
        assert!(matches!(check_normalized(&logits), Err(Error::Model(_))));
        // The wrong direction too: a row that sums to less than one.
        assert!(check_normalized(&[vec![-3.0, -3.0]]).is_err());
    }

    /// One failing crop must not lose the rest of the page.
    #[test]
    fn a_failed_line_reads_as_blank() {
        let rect = Rect { x: 1, y: 2, w: 3, h: 4 };
        let l = line_or_blank(rect, Err(Error::Line("too wide".into())));
        assert_eq!((l.rect, l.text.as_str(), l.candidates.len()), (rect, "", 0));
        let c = Candidate::new("ក", 0.0, 0, 0.0);
        assert_eq!(line_or_blank(rect, Ok(vec![c])).text, "ក");
    }

    #[test]
    fn a_page_over_the_pixel_cap_is_an_error() {
        let (model, alphabet) = tiny();
        let rec = Recognizer::new(model, &alphabet).unwrap();
        let huge = Gray { width: 100_000, height: 100_000, pixels: Vec::new() };
        assert!(matches!(rec.read_page(&huge), Err(Error::Image(_))));
    }

    #[test]
    fn bad_inputs_are_errors_not_panics() {
        assert!(matches!(Recognizer::new(vec![1, 2, 3], r#"["", "ក"]"#), Err(Error::Model(_))));
        assert!(matches!(Recognizer::new(vec![], r#"["ក"]"#), Err(Error::Alphabet)));
    }
}
