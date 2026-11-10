//! Khmer text shaping on top of [`harfrust`].
//!
//! ```no_run
//! use aksar::shape::{Font, ShapeOptions, shape};
//!
//! let bytes = std::fs::read("NotoSansKhmer-Regular.ttf").unwrap();
//! let font = Font::from_bytes(bytes).unwrap();
//! let run = shape("សួស្តី", &font, &ShapeOptions::px(32.0)).unwrap();
//! for g in &run.glyphs {
//!     println!("glyph {} cluster {} advance {}", g.glyph_id, g.cluster, g.x_advance);
//! }
//! ```

use alloc::vec::Vec;
use core::fmt;
use core::str::FromStr;

use harfrust::{Buffer, BufferFlags, Direction, Feature, Language, Script, ShaperFont};

/// Errors from loading fonts or shaping.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a font harfrust can read.
    InvalidFont,
    /// A feature string such as `"liga=0"` could not be parsed.
    InvalidFeature,
    /// harfrust refused to shape the buffer.
    Shape,
    /// `ShapeOptions::size` is not a finite number above zero.
    InvalidSize,
    /// `ShapeOptions::language` is not a BCP-47 tag harfrust can parse.
    InvalidLanguage,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::InvalidFont => "invalid or unsupported font data",
            Error::InvalidFeature => "invalid OpenType feature string",
            Error::Shape => "shaping failed",
            Error::InvalidSize => "size must be a finite number above zero",
            Error::InvalidLanguage => "invalid BCP-47 language tag",
        })
    }
}

impl core::error::Error for Error {}

/// A parsed font, ready to shape. Cheap to clone.
#[derive(Clone)]
pub struct Font {
    inner: harfrust::Font,
}

impl Font {
    /// Parses a TTF/OTF font (index 0 of a collection).
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        Self::from_bytes_index(bytes, 0)
    }

    /// Parses font number `index` of a TTC/OTC collection.
    pub fn from_bytes_index(bytes: Vec<u8>, index: u32) -> Result<Self, Error> {
        harfrust::Font::new(bytes, index).map(|inner| Self { inner }).ok_or(Error::InvalidFont)
    }

    /// Returns this font at a variable-font location, e.g. `[("wght", 700.0)]`.
    /// Unknown axes are ignored; static fonts are returned unchanged.
    pub fn with_variations(&self, axes: &[(&str, f32)]) -> Self {
        let inner = self.inner.instance_builder().variations(axes.iter().copied()).build();
        Self { inner }
    }

    /// Design units per em.
    pub fn units_per_em(&self) -> u16 {
        self.inner.units_per_em()
    }

    /// Glyph id for a character, if the font maps it.
    pub fn glyph_for(&self, c: char) -> Option<u32> {
        ShaperFont::new(&self.inner).nominal_glyph(c as u32).map(|g| g.to_u32())
    }

    /// True when the font maps every Khmer consonant, coeng and dependent vowel.
    pub fn covers_khmer(&self) -> bool {
        let sf = ShaperFont::new(&self.inner);
        (0x1780..=0x17A2).chain(0x17B6..=0x17D2).all(|u| sf.nominal_glyph(u).is_some())
    }
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font").field("units_per_em", &self.units_per_em()).finish()
    }
}

/// Options for [`shape`].
///
/// Build with [`ShapeOptions::default`] or [`ShapeOptions::px`] and the
/// `with_*` setters; the struct is `#[non_exhaustive]`, so new options can be
/// added without breaking callers.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ShapeOptions {
    /// Font size in pixels (or any unit; output is `units / upem * size`).
    /// `None` returns positions in font design units.
    pub size: Option<f32>,
    /// BCP-47 language. Defaults to `"km"` (OpenType language system `KHM`).
    /// `None` shapes with no language. An unparsable tag (including `""`)
    /// makes [`shape`] and [`Shaper::new`] fail with [`Error::InvalidLanguage`];
    /// only the syntax is checked (hyphen-separated subtags, e.g. `km`, `km-KH`),
    /// not the IANA registry.
    pub language: Option<alloc::string::String>,
    /// Extra OpenType features, HarfBuzz syntax: `"kern=0"`, `"+liga"`, `"ss01"`.
    pub features: Vec<alloc::string::String>,
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self { size: None, language: Some("km".into()), features: Vec::new() }
    }
}

impl ShapeOptions {
    /// Options for a pixel size.
    pub fn px(size: f32) -> Self {
        Self { size: Some(size), ..Self::default() }
    }

    /// Sets the size in pixels (`None` = font design units).
    #[must_use]
    pub fn with_size(mut self, size: Option<f32>) -> Self {
        self.size = size;
        self
    }

    /// Sets the BCP-47 language (`None` = no language).
    #[must_use]
    pub fn with_language(mut self, language: Option<&str>) -> Self {
        self.language = language.map(Into::into);
        self
    }

    /// Sets the extra OpenType features, HarfBuzz syntax (`"kern=0"`, `"+liga"`).
    #[must_use]
    pub fn with_features<I, S>(mut self, features: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<alloc::string::String>,
    {
        self.features = features.into_iter().map(Into::into).collect();
        self
    }
}

/// One positioned glyph.
#[derive(Copy, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ShapedGlyph {
    /// Glyph id in the font. `0` is `.notdef` (missing glyph).
    pub glyph_id: u32,
    /// Byte offset of the first character of this glyph's cluster. `u32`, so
    /// text over 4 GiB is out of scope.
    pub cluster: u32,
    /// Horizontal advance.
    pub x_advance: f32,
    /// Vertical advance.
    pub y_advance: f32,
    /// Horizontal offset from the pen position.
    pub x_offset: f32,
    /// Vertical offset from the pen position.
    pub y_offset: f32,
}

/// Shaped output for one string. `GlyphRun::default()` is an empty run to
/// reuse with [`Shaper::shape_into`].
///
/// Cluster offsets are `u32` byte offsets: text longer than 4 GiB is out of scope.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct GlyphRun {
    /// Glyphs in visual (left-to-right) order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Sum of advances.
    pub width: f32,
    /// Number of `.notdef` glyphs (characters the font cannot draw).
    pub missing: usize,
    /// Number of dotted-circle glyphs the shaper inserted for broken clusters.
    pub dotted_circles: usize,
}

/// Shapes `text` as Khmer with `font`.
///
/// The whole string is shaped as one left-to-right Khmer run. Latin, digits
/// and punctuation shape fine in it; splitting mixed-script paragraphs into
/// runs (and bidi) is a layout concern handled a level up.
pub fn shape(text: &str, font: &Font, options: &ShapeOptions) -> Result<GlyphRun, Error> {
    Shaper::new(font, options)?.shape(text)
}

/// BCP-47 shape: hyphen-separated subtags of 1-8 ASCII letters or digits, the
/// first one 2-8 letters (or `i`/`x`). It does not look tags up in the registry.
fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let first = parts.next().unwrap_or("");
    let first_ok = (matches!(first.len(), 2..=8) && first.bytes().all(|b| b.is_ascii_alphabetic()))
        || matches!(first, "i" | "x" | "I" | "X");
    first_ok && parts.all(|p| matches!(p.len(), 1..=8) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn check_size(size: Option<f32>) -> Result<(), Error> {
    if size.is_some_and(|s| !(s.is_finite() && s > 0.0)) {
        return Err(Error::InvalidSize);
    }
    Ok(())
}

/// A reusable shaper for one font and one set of options.
///
/// Builds the shaping plan (feature and lookup selection) once and reuses its
/// buffer, so shaping many strings is much faster than calling [`shape`] each
/// time. Use one per thread.
///
/// ```no_run
/// # use aksar::shape::{Font, ShapeOptions, Shaper};
/// # let font = Font::from_bytes(std::fs::read("NotoSansKhmer-Regular.ttf").unwrap()).unwrap();
/// let mut shaper = Shaper::new(&font, &ShapeOptions::px(16.0)).unwrap();
/// for line in ["ខ្មែរ", "កម្ពុជា"] {
///     let run = shaper.shape(line).unwrap();
///     # let _ = run;
/// }
/// ```
pub struct Shaper {
    font: Font,
    plan: harfrust::ShapePlan,
    features: Vec<Feature>,
    language: Option<Language>,
    scale: f32,
    dotted: Option<u32>,
    buffer: Buffer,
}

impl Shaper {
    /// Prepares a shaper. Fails if a feature string or the size is invalid.
    pub fn new(font: &Font, options: &ShapeOptions) -> Result<Self, Error> {
        check_size(options.size)?;
        let features = options
            .features
            .iter()
            .map(|f| Feature::from_str(f).map_err(|_| Error::InvalidFeature))
            .collect::<Result<Vec<_>, _>>()?;
        let language = options
            .language
            .as_deref()
            .map(|l| {
                // harfrust only rejects "" and lowercases anything else, so check the syntax here.
                if !is_language_tag(l) {
                    return Err(Error::InvalidLanguage);
                }
                Language::from_str(l).map_err(|_| Error::InvalidLanguage)
            })
            .transpose()?;
        let plan = harfrust::ShapePlan::new(
            &font.inner,
            Direction::LeftToRight,
            Some(Script::KHMER),
            language.as_ref(),
            &features,
        );
        Ok(Self {
            font: font.clone(),
            plan,
            features,
            language,
            scale: options.size.map_or(1.0, |s| s / f32::from(font.units_per_em())),
            dotted: font.glyph_for('\u{25CC}'),
            buffer: Buffer::new(),
        })
    }

    /// Changes the output size in pixels (`None` = font units). Keeps the plan.
    /// Fails with [`Error::InvalidSize`], like [`Shaper::new`], unless the size
    /// is finite and above zero; the old size stays in that case.
    pub fn set_size(&mut self, size: Option<f32>) -> Result<(), Error> {
        check_size(size)?;
        self.scale = size.map_or(1.0, |s| s / f32::from(self.font.units_per_em()));
        Ok(())
    }

    /// Shapes `text` as one left-to-right Khmer run.
    pub fn shape(&mut self, text: &str) -> Result<GlyphRun, Error> {
        let mut run = GlyphRun::default();
        self.shape_into(text, &mut run)?;
        Ok(run)
    }

    /// Like [`Shaper::shape`] but writes into `run`, reusing its allocation.
    /// On error `run` is left empty, never holding the previous text's glyphs.
    pub fn shape_into(&mut self, text: &str, run: &mut GlyphRun) -> Result<(), Error> {
        run.glyphs.clear();
        run.width = 0.0;
        run.missing = 0;
        run.dotted_circles = 0;
        let buffer = &mut self.buffer;
        buffer.clear();
        buffer.push_str(text);
        buffer.set_direction(Direction::LeftToRight);
        buffer.set_script(Some(Script::KHMER));
        buffer.set_language(self.language.clone());
        buffer.set_flags(BufferFlags::BEGINNING_OF_TEXT | BufferFlags::END_OF_TEXT);

        let sf = ShaperFont::new(&self.font.inner);
        let opts = harfrust::ShapeOptions::new().plan(Some(&self.plan)).features(&self.features);
        harfrust::shape(&sf, buffer, opts).map_err(|_| Error::Shape)?;

        let scale = self.scale;
        run.glyphs.reserve(buffer.len());
        for (info, pos) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
            let g = ShapedGlyph {
                glyph_id: info.glyph_id,
                cluster: info.cluster,
                x_advance: pos.x_advance as f32 * scale,
                y_advance: pos.y_advance as f32 * scale,
                x_offset: pos.x_offset as f32 * scale,
                y_offset: pos.y_offset as f32 * scale,
            };
            run.width += g.x_advance;
            run.missing += usize::from(g.glyph_id == 0);
            // Only count dotted circles the shaper inserted, not ones in the input.
            if Some(g.glyph_id) == self.dotted && !text[g.cluster as usize..].starts_with('\u{25CC}') {
                run.dotted_circles += 1;
            }
            run.glyphs.push(g);
        }
        Ok(())
    }
}
