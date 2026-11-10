//! Draws Khmer text lines as grayscale images, for OCR training data.
//!
//! Text is shaped with aksar's HarfBuzz-exact shaper, glyph outlines come from
//! `skrifa` (the same font parser), and `zeno` fills them. The label of every
//! image is exactly the text that was drawn, so training data is correct by
//! construction (`docs/OCR_PLAN.md` §5.1). [`augment`] then degrades images the
//! way scans and photos do.
//!
//! ```no_run
//! use aksar_render::{LineRenderer, RenderOptions};
//! let font = std::fs::read("Battambang-Regular.ttf").unwrap();
//! let r = LineRenderer::new(font).unwrap();
//! let img = r.render("ព្រះរាជាណាចក្រកម្ពុជា", &RenderOptions::default()).unwrap();
//! img.save_png("line.png").unwrap();
//! ```

use aksar::shape::{Font, ShapeOptions, Shaper};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use zeno::{Command, Format, Mask, Origin, PathBuilder, Point};

pub mod augment;

/// Errors from rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The font file could not be parsed.
    InvalidFont,
    /// The font has no glyph for some character (the label would be wrong).
    MissingGlyphs(usize),
    /// The text would draw dotted circles (broken Khmer): not a valid label.
    BrokenText,
    /// Nothing to draw (empty or whitespace-only text).
    Empty,
    /// A size option is not a finite number in range, or the image would be too large.
    Size(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidFont => write!(f, "invalid font"),
            Error::MissingGlyphs(n) => write!(f, "font is missing {n} glyphs"),
            Error::BrokenText => write!(f, "text has broken Khmer clusters"),
            Error::Empty => write!(f, "nothing to draw"),
            Error::Size(why) => write!(f, "bad size: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// An 8-bit grayscale image, row-major, 0 = black, 255 = white.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl GrayImage {
    /// A white image.
    pub fn white(width: u32, height: u32) -> Self {
        GrayImage { width, height, pixels: vec![255; (width * height) as usize] }
    }

    /// Pixel at (x, y); out of range reads as white.
    pub fn get(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            255
        } else {
            self.pixels[(y as u32 * self.width + x as u32) as usize]
        }
    }

    /// Share of pixels darker than mid-gray (rough "ink" measure).
    pub fn ink(&self) -> f32 {
        self.pixels.iter().filter(|&&p| p < 128).count() as f32 / self.pixels.len().max(1) as f32
    }

    /// Writes an 8-bit grayscale PNG.
    #[cfg(feature = "png")]
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(file, self.width, self.height);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(std::io::Error::other)?;
        w.write_image_data(&self.pixels).map_err(std::io::Error::other)
    }
}

/// How a line is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOptions {
    /// Font size in pixels (em height).
    pub size_px: f32,
    /// White margin around the ink, in pixels.
    pub padding: u32,
    /// Ink darkness, 0 = black, 255 = invisible.
    pub ink: u8,
    /// Paper brightness.
    pub paper: u8,
    /// Extra letter spacing in pixels added after every glyph (can be negative).
    pub tracking_px: f32,
    /// Variable-font axes, e.g. `[("wght", 700.0)]` (ignored by static fonts).
    pub variations: Vec<(String, f32)>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            size_px: 32.0,
            padding: 8,
            ink: 0,
            paper: 255,
            tracking_px: 0.0,
            variations: Vec::new(),
        }
    }
}

/// Largest `size_px` [`LineRenderer::render`] accepts.
pub const MAX_SIZE_PX: f32 = 1024.0;
/// Largest image [`LineRenderer::render`] draws, in pixels (width * height).
pub const MAX_PIXELS: u64 = 1 << 28;

/// A font ready to draw lines.
pub struct LineRenderer {
    bytes: Vec<u8>,
    font: Font,
}

impl LineRenderer {
    /// Parses a TTF/OTF font.
    pub fn new(bytes: Vec<u8>) -> Result<Self, Error> {
        FontRef::new(&bytes).map_err(|_| Error::InvalidFont)?;
        let font = Font::from_bytes(bytes.clone()).map_err(|_| Error::InvalidFont)?;
        Ok(LineRenderer { bytes, font })
    }

    /// True when the font maps every Khmer consonant, coeng and dependent vowel.
    pub fn covers_khmer(&self) -> bool {
        self.font.covers_khmer()
    }

    /// Draws `text` on one line. Fails instead of drawing a wrong picture: missing
    /// glyphs and broken Khmer (dotted circles) are errors, so a label always
    /// matches its image.
    pub fn render(&self, text: &str, opt: &RenderOptions) -> Result<GrayImage, Error> {
        if text.trim().is_empty() {
            return Err(Error::Empty);
        }
        if !(opt.size_px > 0.0 && opt.size_px <= MAX_SIZE_PX) {
            return Err(Error::Size(format!("size_px {} is not in (0, {MAX_SIZE_PX}]", opt.size_px)));
        }
        if !opt.tracking_px.is_finite() {
            return Err(Error::Size(format!("tracking_px {} is not finite", opt.tracking_px)));
        }
        let axes: Vec<(&str, f32)> = opt.variations.iter().map(|(t, v)| (t.as_str(), *v)).collect();
        let font = if axes.is_empty() { self.font.clone() } else { self.font.with_variations(&axes) };
        let mut shaper =
            Shaper::new(&font, &ShapeOptions::px(opt.size_px)).map_err(|_| Error::InvalidFont)?;
        let run = shaper.shape(text).map_err(|_| Error::InvalidFont)?;
        if run.missing > 0 {
            return Err(Error::MissingGlyphs(run.missing));
        }
        if run.dotted_circles > 0 {
            return Err(Error::BrokenText);
        }

        let fref = FontRef::new(&self.bytes).map_err(|_| Error::InvalidFont)?;
        let coords: Vec<skrifa::instance::NormalizedCoord> = if axes.is_empty() {
            Vec::new()
        } else {
            fref.axes().location(axes.iter().copied()).coords().to_vec()
        };
        let location = LocationRef::new(&coords);
        let outlines = fref.outline_glyphs();
        let size = Size::new(opt.size_px);

        // Outlines in pixel space, y down, baseline at y = 0.
        let mut path: Vec<Command> = Vec::new();
        let mut pen_x = 0.0f32;
        for g in &run.glyphs {
            if let Some(glyph) = outlines.get(GlyphId::new(g.glyph_id)) {
                let mut pen = ZenoPen { path: &mut path, dx: pen_x + g.x_offset, dy: -g.y_offset };
                let _ = glyph.draw(DrawSettings::unhinted(size, location), &mut pen);
            }
            pen_x += g.x_advance + opt.tracking_px;
        }
        if path.is_empty() {
            return Err(Error::Empty);
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in path.iter().flat_map(points) {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let pad = opt.padding as f32;
        let (w, h) = (((x1 - x0) + 2.0 * pad).ceil(), ((y1 - y0) + 2.0 * pad).ceil());
        // Check before `max(1.0)`: it turns a NaN extent into a valid 1.
        if !extent_fits(w, h) {
            return Err(Error::Size(format!("a {w}x{h} image is larger than {MAX_PIXELS} pixels")));
        }
        let (w, h) = (w.max(1.0), h.max(1.0));
        let (width, height) = (w as u32, h as u32);
        let (dx, dy) = (pad - x0, pad - y0);
        let shifted: Vec<Command> = path.iter().map(|c| translate(*c, dx, dy)).collect();

        let mut coverage = vec![0u8; (width * height) as usize];
        Mask::new(&shifted[..])
            .format(Format::Alpha)
            .origin(Origin::TopLeft)
            .size(width, height)
            .render_into(&mut coverage, None);
        let (ink, paper) = (opt.ink as f32, opt.paper as f32);
        let pixels = coverage
            .iter()
            .map(|&a| {
                let a = a as f32 / 255.0;
                (paper + (ink - paper) * a).round().clamp(0.0, 255.0) as u8
            })
            .collect();
        Ok(GrayImage { width, height, pixels })
    }
}

struct ZenoPen<'a> {
    path: &'a mut Vec<Command>,
    dx: f32,
    dy: f32,
}

impl ZenoPen<'_> {
    // Font units are y-up; images are y-down.
    fn p(&self, x: f32, y: f32) -> Point {
        Point::new(x + self.dx, -y + self.dy)
    }
}

impl OutlinePen for ZenoPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.path.move_to(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.path.line_to(p);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (c, p) = (self.p(cx, cy), self.p(x, y));
        self.path.quad_to(c, p);
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        let (c0, c1, p) = (self.p(c0x, c0y), self.p(c1x, c1y), self.p(x, y));
        self.path.curve_to(c0, c1, p);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

fn points(c: &Command) -> Vec<Point> {
    match *c {
        Command::MoveTo(p) | Command::LineTo(p) => vec![p],
        Command::QuadTo(a, b) => vec![a, b],
        Command::CurveTo(a, b, p) => vec![a, b, p],
        Command::Close => vec![],
    }
}

fn translate(c: Command, dx: f32, dy: f32) -> Command {
    let t = |p: Point| Point::new(p.x + dx, p.y + dy);
    match c {
        Command::MoveTo(p) => Command::MoveTo(t(p)),
        Command::LineTo(p) => Command::LineTo(t(p)),
        Command::QuadTo(a, b) => Command::QuadTo(t(a), t(b)),
        Command::CurveTo(a, b, p) => Command::CurveTo(t(a), t(b), t(p)),
        Command::Close => Command::Close,
    }
}

/// True when a raw `w` x `h` extent (before the 1 pixel minimum) is finite and within [`MAX_PIXELS`]. Huge finite
/// tracking can push the ink extent to inf or NaN, and NaN compares false against
/// any limit, so finiteness is checked before the product. The f64 product
/// cannot overflow.
fn extent_fits(w: f32, h: f32) -> bool {
    w.is_finite() && h.is_finite() && f64::from(w) * f64::from(h) <= MAX_PIXELS as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Noto Sans Khmer from the shaping test data, if the workspace has it.
    fn font() -> Option<LineRenderer> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fonts");
        let entry = std::fs::read_dir(&dir).ok()?.flatten().find(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.contains("khmer") && (n.ends_with(".ttf") || n.ends_with(".otf"))
        })?;
        LineRenderer::new(std::fs::read(entry.path()).ok()?).ok()
    }

    /// NaN, zero, negative or huge sizes used to reach the rasterizer (and a huge
    /// one could overflow `width * height`); they must be clean errors.
    #[test]
    fn bad_sizes_are_errors() {
        let Some(r) = font() else { return };
        for size_px in [f32::NAN, 0.0, -4.0, f32::INFINITY, MAX_SIZE_PX * 2.0] {
            let o = RenderOptions { size_px, ..RenderOptions::default() };
            assert!(matches!(r.render("ក", &o), Err(Error::Size(_))), "size_px {size_px}");
        }
        let o = RenderOptions { tracking_px: f32::NAN, ..RenderOptions::default() };
        assert!(matches!(r.render("ក", &o), Err(Error::Size(_))));
        // A long line at the largest size is over MAX_PIXELS.
        let o = RenderOptions { size_px: MAX_SIZE_PX, ..RenderOptions::default() };
        let long = "ក".repeat(2000);
        assert!(matches!(r.render(&long, &o), Err(Error::Size(_))));
        assert!(r.render("ក", &o).is_ok(), "the largest size itself is allowed");
    }

    /// A NaN extent passed `product > MAX_PIXELS` (false), and `max(1.0)` turned
    /// NaN into 1: it was rendered as a 1x1 image. Non-finite extents must be
    /// rejected before either.
    #[test]
    fn non_finite_extents_do_not_fit() {
        assert!(extent_fits(100.0, 40.0));
        assert!(!extent_fits(f32::NAN, 40.0));
        assert!(!extent_fits(100.0, f32::NAN));
        assert!(!extent_fits(f32::INFINITY, 1.0));
        assert!(!extent_fits(1.0, f32::NEG_INFINITY));
        assert!(!extent_fits(MAX_PIXELS as f32, 2.0), "over the limit");
    }

    /// Huge finite tracking is a clean size error end to end.
    #[test]
    fn overflowing_tracking_is_a_size_error() {
        let Some(r) = font() else { return };
        for tracking_px in [f32::MAX, -f32::MAX] {
            let o = RenderOptions { tracking_px, ..RenderOptions::default() };
            assert!(matches!(r.render("  ក", &o), Err(Error::Size(_))), "tracking_px {tracking_px}");
        }
    }

    #[test]
    fn draws_ink_and_sizes_follow_the_text() {
        let Some(r) = font() else { return };
        assert!(r.covers_khmer());
        let o = RenderOptions::default();
        let short = r.render("ក", &o).unwrap();
        let long = r.render("ព្រះរាជាណាចក្រកម្ពុជា", &o).unwrap();
        assert!(short.ink() > 0.01 && long.ink() > 0.01);
        assert!(long.width > 5 * short.width);
        // A subscript stack hangs below the base: taller than a lone consonant.
        let stack = r.render("ស្ត្រី", &o).unwrap();
        assert!(stack.height > short.height);
        // Same input, same pixels.
        assert_eq!(r.render("ខ្មែរ", &o).unwrap(), r.render("ខ្មែរ", &o).unwrap());
        // Bigger size, bigger image.
        let big = r.render("ក", &RenderOptions { size_px: 64.0, ..o.clone() }).unwrap();
        assert!(big.height > short.height);
    }

    #[test]
    fn refuses_text_it_cannot_draw_correctly() {
        let Some(r) = font() else { return };
        let o = RenderOptions::default();
        assert_eq!(r.render("   ", &o), Err(Error::Empty));
        assert_eq!(r.render("ាក", &o), Err(Error::BrokenText));
        assert!(matches!(r.render("中文", &o), Err(Error::MissingGlyphs(_))));
    }
}
