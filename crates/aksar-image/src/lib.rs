//! Page clean-up for Khmer OCR (`docs/OCR_PLAN.md` §3): grayscale,
//! binarization (Otsu, Sauvola), skew detection and correction, and text-line
//! crops sized for a line recognizer.
//!
//! Pure Rust, no dependencies; images are 8-bit grayscale, 0 = black.
//!
//! ```
//! use aksar_image::Gray;
//! let page = Gray::new(200, 100, vec![255; 200 * 100]);
//! assert!(aksar_image::lines(&aksar_image::sauvola(&page, 15, 0.34)).is_empty());
//! ```

/// An 8-bit grayscale image, row-major, 0 = black, 255 = white.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// `Gray`'s fields are public, so a hand-built image can disagree with its
/// size; the functions that index by `width * height` check that in debug builds.
fn debug_check_len(img: &Gray) {
    debug_assert_eq!(
        Some(img.pixels.len()),
        (img.width as usize).checked_mul(img.height as usize),
        "pixel count"
    );
}

/// One RGBA pixel as gray: Rec. 601 luma, composited over white paper.
pub fn luma(r: u8, g: u8, b: u8, a: u8) -> u8 {
    let y = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    let a = f32::from(a) / 255.0;
    (y * a + 255.0 * (1.0 - a)).round() as u8
}

impl Gray {
    /// Wraps pixels (`width * height` bytes, row by row).
    ///
    /// # Panics
    ///
    /// If `pixels.len()` is not `width * height`.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert_eq!(Some(pixels.len()), (width as usize).checked_mul(height as usize), "pixel count");
        Gray { width, height, pixels }
    }

    /// From RGBA bytes (Rec. 601 luma; transparent pixels read as white).
    ///
    /// # Panics
    ///
    /// If `rgba.len()` is not `4 * width * height`.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let pixels = rgba.chunks_exact(4).map(|p| luma(p[0], p[1], p[2], p[3])).collect();
        Gray::new(width, height, pixels)
    }

    /// Pixel at (x, y); outside reads as white.
    pub fn get(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            255
        } else {
            self.pixels[y as usize * self.width as usize + x as usize]
        }
    }

    /// The pixels inside `r` (clipped to the image).
    pub fn crop(&self, r: Rect) -> Gray {
        let x1 = r.x.saturating_add(r.w).min(self.width);
        let y1 = r.y.saturating_add(r.h).min(self.height);
        let (x0, y0) = (r.x.min(x1), r.y.min(y1));
        let (w, h) = (x1 - x0, y1 - y0);
        let mut px = Vec::with_capacity(w as usize * h as usize);
        for y in y0..y1 {
            let row = y as usize * self.width as usize;
            px.extend_from_slice(&self.pixels[row + x0 as usize..row + x1 as usize]);
        }
        Gray::new(w, h, px)
    }
}

/// An axis-aligned rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Global threshold by Otsu's method (maximizes between-class variance).
pub fn otsu_threshold(img: &Gray) -> u8 {
    let mut hist = [0u64; 256];
    for &p in &img.pixels {
        hist[p as usize] += 1;
    }
    let total = img.pixels.len() as f64;
    let sum_all: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    // One gray level only: no ink, so every pixel must land above the threshold.
    // No let-chain: aksar-image keeps the workspace MSRV (1.85).
    if let Some(v) = hist.iter().position(|&c| c > 0).filter(|&v| hist[v] as usize == img.pixels.len()) {
        return (v as u8).saturating_sub(1);
    }
    let (mut w0, mut sum0, mut best, mut best_t) = (0.0, 0.0, -1.0, 128u8);
    for (t, &c) in hist.iter().enumerate() {
        w0 += c as f64;
        if w0 == 0.0 {
            continue;
        }
        let w1 = total - w0;
        if w1 == 0.0 {
            break;
        }
        sum0 += t as f64 * c as f64;
        let (m0, m1) = (sum0 / w0, (sum_all - sum0) / w1);
        let between = w0 * w1 * (m0 - m1) * (m0 - m1);
        if between > best {
            best = between;
            best_t = t as u8;
        }
    }
    best_t
}

/// Black/white with one threshold for the page (fine for clean scans).
pub fn otsu(img: &Gray) -> Gray {
    let t = otsu_threshold(img);
    Gray::new(img.width, img.height, img.pixels.iter().map(|&p| if p <= t { 0 } else { 255 }).collect())
}

/// Black/white with a local threshold (Sauvola): handles shadows and uneven
/// light in photos. `window` is the neighbourhood size in pixels (about two text
/// line heights), `k` the sensitivity (0.2-0.5; 0.34 is a common default).
///
/// Memory: one output page plus a few rows (the window sums slide over rows and
/// columns; no full-page integral images).
///
/// # Panics
///
/// If `img.pixels` does not hold `width * height` bytes (the fields are public;
/// checked in debug builds, otherwise an out-of-range index panics or the extra
/// pixels are ignored).
pub fn sauvola(img: &Gray, window: u32, k: f32) -> Gray {
    debug_check_len(img);
    let (w, h) = (img.width as usize, img.height as usize);
    let r = (window / 2).max(1) as usize;
    let mut out = vec![255u8; w * h];
    // Per-column sums of values and squares over the rows [lo, hi) the current
    // row's window covers. Integers: exact, so the result equals what full
    // integral images (f64) give.
    let (mut col_sum, mut col_sq) = (vec![0u64; w], vec![0u64; w]);
    let (mut lo, mut hi) = (0usize, 0usize);
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        while hi < y1 {
            for (x, &p) in img.pixels[hi * w..(hi + 1) * w].iter().enumerate() {
                col_sum[x] += u64::from(p);
                col_sq[x] += u64::from(p) * u64::from(p);
            }
            hi += 1;
        }
        while lo < y0 {
            for (x, &p) in img.pixels[lo * w..(lo + 1) * w].iter().enumerate() {
                col_sum[x] -= u64::from(p);
                col_sq[x] -= u64::from(p) * u64::from(p);
            }
            lo += 1;
        }
        // Slide the column window [x0, x1) along the row.
        let (mut s, mut q) = (0u64, 0u64);
        let (mut c0, mut c1) = (0usize, 0usize);
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            while c1 < x1 {
                s += col_sum[c1];
                q += col_sq[c1];
                c1 += 1;
            }
            while c0 < x0 {
                s -= col_sum[c0];
                q -= col_sq[c0];
                c0 += 1;
            }
            let n = ((x1 - x0) * (y1 - y0)) as f64;
            let mean = s as f64 / n;
            let var = (q as f64 / n - mean * mean).max(0.0);
            let t = mean * (1.0 + k as f64 * (var.sqrt() / 128.0 - 1.0));
            if (img.pixels[y * w + x] as f64) <= t {
                out[y * w + x] = 0;
            }
        }
    }
    Gray::new(img.width, img.height, out)
}

/// Rotates by `degrees` (positive = counter-clockwise) around the centre,
/// keeping the size; uncovered corners become white. A non-finite angle
/// returns the image unchanged.
///
/// # Panics
///
/// If `img.pixels` does not hold `width * height` bytes (the fields are public;
/// checked in debug builds).
pub fn rotate(img: &Gray, degrees: f32) -> Gray {
    debug_check_len(img);
    if !degrees.is_finite() {
        return img.clone();
    }
    let (s, c) = degrees.to_radians().sin_cos();
    let (cx, cy) = ((img.width as f32 - 1.0) / 2.0, (img.height as f32 - 1.0) / 2.0);
    let mut out = vec![255u8; img.pixels.len()];
    for y in 0..img.height {
        for x in 0..img.width {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let (sx, sy) = (c * dx - s * dy + cx, s * dx + c * dy + cy);
            let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            let p = |ox, oy| img.get(x0 + ox, y0 + oy) as f32;
            let v = p(0, 0) * (1.0 - fx) * (1.0 - fy)
                + p(1, 0) * fx * (1.0 - fy)
                + p(0, 1) * (1.0 - fx) * fy
                + p(1, 1) * fx * fy;
            out[y as usize * img.width as usize + x as usize] = v.round() as u8;
        }
    }
    Gray::new(img.width, img.height, out)
}

/// Most ink points the skew search looks at. A page has millions; a few
/// hundred thousand evenly spaced ones give the same projection profile.
const MAX_SKEW_POINTS: usize = 200_000;

/// Every n-th ink pixel (row-major order) as coordinates, at most `cap` of them.
fn ink_sample(bw: &Gray, cap: usize) -> Vec<(f32, f32)> {
    let w = bw.width as usize;
    let total = bw.pixels.iter().filter(|&&p| p < 128).count();
    let stride = total.div_ceil(cap.max(1)).max(1);
    bw.pixels
        .iter()
        .enumerate()
        .filter(|&(_, &p)| p < 128)
        .step_by(stride)
        .map(|(i, _)| ((i % w) as f32, (i / w) as f32))
        .collect()
}

/// Skew angle in degrees (rotate by the result to straighten), from the
/// projection profile of a black/white image: text lines are straight when the
/// row sums vary the most. Searches ±`max_degrees` (at most 45) in 0.1° steps.
/// Looks at no more than 200000 evenly spaced ink pixels, so the cost does not
/// grow with the page's ink.
///
/// # Panics
///
/// If `bw.pixels` does not hold `width * height` bytes (the fields are public;
/// checked in debug builds).
pub fn skew_angle(bw: &Gray, max_degrees: f32) -> f32 {
    debug_check_len(bw);
    let max_degrees = if max_degrees.is_finite() { max_degrees.clamp(0.0, 45.0) } else { 0.0 };
    // Work on ink pixel coordinates (cheap) instead of rotating the image.
    let ink = ink_sample(bw, MAX_SKEW_POINTS);
    if ink.len() < 20 {
        return 0.0;
    }
    let mut rows = vec![0u32; bw.height as usize * 2 + bw.width as usize];
    let mut score = |deg: f32| {
        let (s, c) = deg.to_radians().sin_cos();
        rows.fill(0);
        let off = bw.width as f32;
        for &(x, y) in &ink {
            let r = (y * c + x * s + off).round();
            if r >= 0.0 && (r as usize) < rows.len() {
                rows[r as usize] += 1;
            }
        }
        rows.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>()
    };
    let steps = (max_degrees * 10.0).round() as i32;
    let mut best = (f64::MIN, 0.0f32);
    for i in -steps..=steps {
        let deg = i as f32 / 10.0;
        let s = score(deg);
        if s > best.0 {
            best = (s, deg);
        }
    }
    // The best angle is the page's tilt in this coordinate frame; undoing it
    // means rotating the other way.
    -best.1
}

/// Text-line boxes of a straight black/white page, top to bottom. Rows with ink
/// form bands; bands thinner than a third of the typical band (Khmer vowels and
/// signs above or below a line, or noise) join the nearest line; each line gets
/// a margin of 15% of its height so marks are not cut (never into the gap's
/// midpoint to the next line, so boxes do not overlap).
///
/// A line shorter than a third of the typical line height that is not within
/// half a typical height of another line (a lone short caption, a rule, a
/// speck) is dropped, not reported: it is indistinguishable from noise.
///
/// # Panics
///
/// If `bw.pixels` does not hold `width * height` bytes (the fields are public).
pub fn lines(bw: &Gray) -> Vec<Rect> {
    debug_check_len(bw);
    let (w, h) = (bw.width as usize, bw.height as usize);
    let row_ink: Vec<usize> =
        (0..h).map(|y| bw.pixels[y * w..(y + 1) * w].iter().filter(|&&p| p < 128).count()).collect();
    // A row is part of a line when it has a few percent of a busy text row's
    // ink: at scan resolution the gaps between lines still catch a few pixels
    // of marks and noise, and a fixed count would join every line on the page.
    let mut busy: Vec<usize> = row_ink.iter().copied().filter(|&n| n > 0).collect();
    busy.sort_unstable();
    let p90 = busy.get(busy.len() * 9 / 10).copied().unwrap_or(0);
    let min_ink = (w / 400).max(1).max(p90 / 30);
    let mut bands: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (y, &n) in row_ink.iter().enumerate() {
        match (n >= min_ink, start) {
            (true, None) => start = Some(y),
            (false, Some(s)) => {
                bands.push((s, y));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        bands.push((s, h));
    }
    if bands.is_empty() {
        return Vec::new();
    }
    // Typical line height: the 75th percentile of band heights. Specks and
    // separated marks are the short bands; a logo or a title block is one
    // tall band and must not set the scale either.
    let mut heights: Vec<usize> = bands.iter().map(|b| b.1 - b.0).collect();
    heights.sort_unstable();
    let typical = heights[heights.len() * 3 / 4].max(1);
    // Merge thin bands (marks above or below a line) into the closer line, if
    // one is near; a thin band far from every line is a speck and is dropped.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    let thin = |b: &(usize, usize)| (b.1 - b.0) * 3 < typical;
    let lone = bands.len() == 1;
    let mut i = 0;
    while i < bands.len() {
        let b = bands[i];
        if thin(&b) && !lone {
            let gap_prev = merged.last().map(|p| b.0 - p.1).filter(|&g| g * 2 <= typical);
            let gap_next =
                bands.get(i + 1).filter(|n| !thin(n)).map(|n| n.0 - b.1).filter(|&g| g * 2 <= typical);
            match (gap_prev, gap_next) {
                (Some(gp), Some(gn)) if gn < gp => bands[i + 1].0 = b.0,
                (Some(_), _) => merged.last_mut().unwrap().1 = b.1,
                (None, Some(_)) => bands[i + 1].0 = b.0,
                (None, None) => {}
            }
        } else {
            merged.push(b);
        }
        i += 1;
    }
    (0..merged.len())
        .map(|i| {
            let (y0, y1) = merged[i];
            let pad = ((y1 - y0) as f32 * 0.15).ceil() as usize;
            // The margin stops halfway into the gap to a neighbouring line.
            let top = if i > 0 {
                y0.saturating_sub(pad).max((merged[i - 1].1 + y0) / 2)
            } else {
                y0.saturating_sub(pad)
            };
            let bottom = match merged.get(i + 1) {
                Some(next) => (y1 + pad).min((y1 + next.0) / 2),
                None => (y1 + pad).min(h),
            };
            let (y0, y1) = (top, bottom);
            // Leftmost and rightmost ink over the rows.
            let (mut first, mut last) = (usize::MAX, 0);
            for row in bw.pixels[y0 * w..y1 * w].chunks_exact(w.max(1)) {
                if let Some(a) = row.iter().position(|&p| p < 128) {
                    first = first.min(a);
                    last = last.max(row.iter().rposition(|&p| p < 128).unwrap_or(a) + 1);
                }
            }
            let (x0, x1) = if first == usize::MAX { (0, w) } else { (first, last) };
            let (x0, x1) = (x0.saturating_sub(pad), (x1 + pad).min(w));
            Rect { x: x0 as u32, y: y0 as u32, w: (x1 - x0) as u32, h: (y1 - y0) as u32 }
        })
        .collect()
}

/// Which way round a line crop's ink is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polarity {
    /// Dark text on a light background (the usual case).
    DarkOnLight,
    /// Light text on a dark background (signs, labels, inverted headers).
    LightOnDark,
    /// The border is dark but the bright pixels are the majority: a dim photo
    /// of dark text, or light text in a tight crop. Try both.
    Unsure,
}

/// Guesses a line crop's polarity: the border of a line crop is mostly
/// background, and text covers less of the line than its background.
pub fn polarity(line: &Gray) -> Polarity {
    if line.width < 3 || line.height < 3 {
        return Polarity::DarkOnLight;
    }
    let t = otsu_threshold(line);
    let (w, h) = (line.width as i64, line.height as i64);
    let border =
        (0..w).flat_map(|x| [(x, 0), (x, h - 1)]).chain((1..h - 1).flat_map(|y| [(0, y), (w - 1, y)]));
    let (mut dark, mut total) = (0u64, 0u64);
    for (x, y) in border {
        total += 1;
        // Otsu's dark class is `<= t` (t is 0 for a clean two-level crop).
        dark += u64::from(line.get(x, y) <= t);
    }
    if dark * 10 <= total * 6 {
        return Polarity::DarkOnLight;
    }
    let bright = line.pixels.iter().filter(|&&p| p > t).count();
    if bright * 2 < line.pixels.len() { Polarity::LightOnDark } else { Polarity::Unsure }
}

/// True when [`polarity`] is sure the crop is light text on a dark background.
pub fn is_light_on_dark(line: &Gray) -> bool {
    polarity(line) == Polarity::LightOnDark
}

/// The negative image (255 - v).
pub fn invert(img: &Gray) -> Gray {
    Gray::new(img.width, img.height, img.pixels.iter().map(|&p| 255 - p).collect())
}

/// Scales a line crop to `height` pixels (aspect kept), the input size a line
/// recognizer expects. Same filter as Pillow's `resize(..., BILINEAR)` (a
/// triangle filter widened when shrinking), which the training script uses, so
/// thin strokes such as subscripts survive shrinking as they did in training.
pub fn fit_height(line: &Gray, height: u32) -> Gray {
    if line.height == 0 || line.width == 0 || height == 0 {
        return Gray::new(1, height, vec![255; height as usize]);
    }
    let width = ((line.width as f64 * height as f64 / line.height as f64).round() as u32).max(1);
    resize(line, width, height)
}

/// Pillow-style separable resampling with a triangle (bilinear) filter.
fn resize(img: &Gray, width: u32, height: u32) -> Gray {
    // For each output index: first source index and normalized weights.
    fn coeffs(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
        let scale = src as f64 / dst as f64;
        let filter_scale = scale.max(1.0);
        let support = filter_scale; // triangle support 1, widened when shrinking
        (0..dst)
            .map(|i| {
                let center = (i as f64 + 0.5) * scale;
                let lo = ((center - support + 0.5).floor().max(0.0)) as usize;
                let hi = (((center + support + 0.5).floor()) as usize).min(src).max(lo + 1);
                let mut w: Vec<f32> = (lo..hi)
                    .map(|j| (1.0 - ((j as f64 - center + 0.5) / filter_scale).abs()).max(0.0) as f32)
                    .collect();
                let total: f32 = w.iter().sum();
                if total > 0.0 {
                    w.iter_mut().for_each(|v| *v /= total);
                } else {
                    w.iter_mut().for_each(|v| *v = 1.0 / (hi - lo) as f32);
                }
                (lo, w)
            })
            .collect()
    }
    let (sw, sh, dw, dh) = (img.width as usize, img.height as usize, width as usize, height as usize);
    let (cx, cy) = (coeffs(sw, dw), coeffs(sh, dh));
    // Horizontal pass into f32 rows, then vertical.
    let mut tmp = vec![0f32; dw * sh];
    for y in 0..sh {
        let row = &img.pixels[y * sw..(y + 1) * sw];
        for (x, (lo, w)) in cx.iter().enumerate() {
            tmp[y * dw + x] = w.iter().enumerate().map(|(k, &wk)| wk * row[lo + k] as f32).sum();
        }
    }
    let mut out = vec![0u8; dw * dh];
    for (y, (lo, w)) in cy.iter().enumerate() {
        for x in 0..dw {
            let v: f32 = w.iter().enumerate().map(|(k, &wk)| wk * tmp[(lo + k) * dw + x]).sum();
            out[y * dw + x] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    Gray::new(width, height, out)
}

/// Largest page [`prepare_page`] accepts, in pixels (64 MP: an A4 page at
/// 600 dpi is 35 MP). It works on a few full-page buffers, so a small file with
/// a huge header must not reach it.
pub const MAX_PIXELS: u64 = 64_000_000;

/// The page is larger than [`MAX_PIXELS`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageTooLarge {
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Display for PageTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{} is larger than {MAX_PIXELS} pixels", self.width, self.height)
    }
}

impl std::error::Error for PageTooLarge {}

/// The whole chain for a page photo or scan: Sauvola, deskew, line boxes.
/// Returns the straightened gray page and its line boxes, or [`PageTooLarge`]
/// (checked first, before anything is allocated).
///
/// Peak memory is about three pages (the input, the straightened gray page and
/// one black/white page) on top of the caller's own copy.
///
/// # Panics
///
/// If `img.pixels` does not hold `width * height` bytes (the fields are public;
/// checked in debug builds).
pub fn prepare_page(img: &Gray) -> Result<(Gray, Vec<Rect>), PageTooLarge> {
    if u64::from(img.width) * u64::from(img.height) > MAX_PIXELS {
        return Err(PageTooLarge { width: img.width, height: img.height });
    }
    let window = (img.height / 30).clamp(15, 61) | 1;
    let mut bw = sauvola(img, window, 0.34);
    let angle = skew_angle(&bw, 5.0);
    let gray = if angle.abs() >= 0.1 {
        // Rotating blends the 0/255 pixels; the old page is dropped before the
        // gray one is rotated so only three pages are alive at once.
        bw = rotate(&bw, angle);
        rotate(img, angle)
    } else {
        img.clone()
    };
    // Back to two levels in place (Otsu on the possibly blended pixels).
    let t = otsu_threshold(&bw);
    for p in &mut bw.pixels {
        *p = if *p <= t { 0 } else { 255 };
    }
    let boxes = lines(&bw);
    Ok((gray, boxes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aksar_render::{LineRenderer, RenderOptions};

    fn renderer() -> Option<LineRenderer> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fonts");
        let e = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().ends_with(".ttf"))?;
        LineRenderer::new(std::fs::read(e.path()).ok()?).ok()
    }

    /// A page with the given lines stacked with gaps.
    fn page(lines: &[&str]) -> Option<Gray> {
        let r = renderer()?;
        let imgs: Vec<_> = lines
            .iter()
            .map(|t| r.render(t, &RenderOptions { size_px: 28.0, padding: 2, ..Default::default() }).unwrap())
            .collect();
        let width = imgs.iter().map(|i| i.width).max()? + 80;
        let height: u32 = imgs.iter().map(|i| i.height + 24).sum::<u32>() + 60;
        let mut px = vec![255u8; (width * height) as usize];
        let mut y = 30;
        for i in &imgs {
            for yy in 0..i.height {
                for xx in 0..i.width {
                    px[((y + yy) * width + 40 + xx) as usize] = i.pixels[(yy * i.width + xx) as usize];
                }
            }
            y += i.height + 24;
        }
        Some(Gray::new(width, height, px))
    }

    const TEXT: [&str; 4] = ["ព្រះរាជាណាចក្រកម្ពុជា", "ជាតិ សាសនា ព្រះមហាក្សត្រ", "ក្រសួងអប់រំ យុវជន និងកីឡា", "សេចក្តីជូនដំណឹង"];

    #[test]
    fn a_tall_logo_does_not_swallow_the_text_lines() {
        let Some(mut p) = page(&TEXT) else { return };
        let clean = lines(&otsu(&p));
        // A solid 120-row block (an emblem) above the text, far taller than a line.
        let w = p.width as usize;
        let mut px = vec![255u8; w * 160];
        for y in 10..130 {
            for x in 40..160 {
                px[y * w + x] = 0;
            }
        }
        px.extend_from_slice(&p.pixels);
        p = Gray::new(p.width, p.height + 160, px);
        let boxes = lines(&otsu(&p));
        assert_eq!(boxes.len(), clean.len() + 1, "{boxes:?}");
    }

    #[test]
    fn a_speck_far_from_the_text_is_not_part_of_a_line() {
        let Some(mut p) = page(&TEXT[..2]) else { return };
        let clean = lines(&otsu(&p));
        // A 2x2 speck near the top and one near the bottom of the page.
        for (x, y) in [(5, 2), (6, 2), (5, 3), (6, 3)] {
            p.pixels[y * p.width as usize + x] = 0;
            let yb = p.height as usize - 1 - y;
            p.pixels[yb * p.width as usize + x] = 0;
        }
        assert_eq!(
            lines(&otsu(&p)).iter().map(|r| (r.y, r.h)).collect::<Vec<_>>(),
            clean.iter().map(|r| (r.y, r.h)).collect::<Vec<_>>()
        );
    }

    /// Margins must not run into the next line: two crops that overlap would
    /// read the same ink twice.
    #[test]
    fn padded_boxes_stop_at_the_gap_midpoint() {
        // Two solid 30-row bars 4 rows apart: a 15% margin (5 rows) would overlap.
        let (w, h) = (100usize, 90usize);
        let mut px = vec![255u8; w * h];
        for y in (20..50).chain(54..84) {
            for x in 10..90 {
                px[y * w + x] = 0;
            }
        }
        let boxes = lines(&Gray::new(w as u32, h as u32, px));
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        assert!(boxes[0].y + boxes[0].h <= boxes[1].y, "overlap: {boxes:?}");
        // Ink is still inside, and the sides keep their margin.
        assert!(boxes[0].y <= 20 && boxes[0].y + boxes[0].h >= 50, "{boxes:?}");
        assert!(boxes[1].y <= 54 && boxes[1].y + boxes[1].h >= 84, "{boxes:?}");
        assert_eq!((boxes[0].x, boxes[0].w), (10 - 5, 80 + 10));
    }

    /// The column extent comes from the ink in the box's rows only.
    #[test]
    fn box_width_follows_the_ink_of_its_own_rows() {
        let (w, h) = (200usize, 60usize);
        let mut px = vec![255u8; w * h];
        for y in 10..30 {
            for x in 40..120 {
                px[y * w + x] = 0;
            }
        }
        // Ink in other rows (not part of the band) must not widen the box.
        px[55 * w + 5] = 0;
        let boxes = lines(&Gray::new(w as u32, h as u32, px));
        assert_eq!((boxes[0].x, boxes[0].w), (40 - 3, 80 + 6), "{boxes:?}");
    }

    /// A page header can claim any size; refuse before allocating (and before
    /// the integral-image-sized buffers could overflow a 32-bit usize).
    #[test]
    fn a_huge_page_is_refused_before_any_work() {
        // Fields are public: a header-only image costs nothing to build.
        let huge = Gray { width: 100_000, height: 100_000, pixels: Vec::new() };
        assert_eq!(prepare_page(&huge).err(), Some(PageTooLarge { width: 100_000, height: 100_000 }));
        let at_limit = Gray::new(1000, 64, vec![255; 64_000]);
        assert!(prepare_page(&at_limit).is_ok());
    }

    /// Skew detection must not scale with the page's ink: millions of points
    /// were collected (8 bytes each) and scanned 101 times.
    #[test]
    fn skew_looks_at_a_capped_even_sample_of_ink() {
        let (w, h) = (1500u32, 1000u32);
        let px: Vec<u8> = (0..w * h).map(|i| if (i / w + i % w) % 3 == 0 { 0 } else { 255 }).collect();
        let g = Gray::new(w, h, px);
        let s = ink_sample(&g, 1000);
        assert!(s.len() <= 1000 && s.len() > 500, "{}", s.len());
        // Evenly spread: both halves of the page are sampled.
        assert!(s.iter().any(|p| p.1 < 100.0) && s.iter().any(|p| p.1 > 900.0));
        // Few ink pixels: all of them.
        let few = Gray::new(10, 10, (0..100).map(|i| if i % 10 == 0 { 0 } else { 255 }).collect());
        assert_eq!(ink_sample(&few, 1000).len(), 10);
    }

    /// Sampling must not move the answer on a page with far more ink than the cap.
    #[test]
    fn skew_on_a_dense_page_matches_the_truth() {
        // Horizontal bars (dense text-like lines), 1200x900, ~480k ink points.
        let (w, h) = (1200usize, 900usize);
        let mut px = vec![255u8; w * h];
        for line in 0..15 {
            for y in line * 60 + 10..line * 60 + 40 {
                for x in 50..1150 {
                    px[y * w + x] = 0;
                }
            }
        }
        let flat = Gray::new(w as u32, h as u32, px);
        assert!(flat.pixels.iter().filter(|&&p| p < 128).count() > 2 * MAX_SKEW_POINTS);
        let tilted = otsu(&rotate(&flat, 2.0));
        let found = skew_angle(&tilted, 5.0);
        assert!((found + 2.0).abs() <= 0.3, "found {found}");
    }

    /// The sliding sums must give what plain per-pixel window sums give.
    #[test]
    fn sauvola_matches_a_direct_window_computation() {
        let (w, h) = (23usize, 17usize);
        let px: Vec<u8> = (0..w * h).map(|i| ((i * 37 + i / w * 11) % 251) as u8).collect();
        let img = Gray::new(w as u32, h as u32, px.clone());
        for (window, k) in [(1u32, 0.34f32), (5, 0.34), (15, 0.2), (61, 0.5)] {
            let got = sauvola(&img, window, k);
            let r = (window / 2).max(1) as usize;
            for y in 0..h {
                for x in 0..w {
                    let (x0, y0) = (x.saturating_sub(r), y.saturating_sub(r));
                    let (x1, y1) = ((x + r + 1).min(w), (y + r + 1).min(h));
                    let n = ((x1 - x0) * (y1 - y0)) as f64;
                    let vals: Vec<f64> = (y0..y1)
                        .flat_map(|yy| (x0..x1).map(move |xx| (yy, xx)))
                        .map(|(yy, xx)| px[yy * w + xx] as f64)
                        .collect();
                    let mean = vals.iter().sum::<f64>() / n;
                    let var = (vals.iter().map(|v| v * v).sum::<f64>() / n - mean * mean).max(0.0);
                    let t = mean * (1.0 + k as f64 * (var.sqrt() / 128.0 - 1.0));
                    let want = if (px[y * w + x] as f64) <= t { 0 } else { 255 };
                    assert_eq!(got.pixels[y * w + x], want, "window {window} at {x},{y}");
                }
            }
        }
    }

    /// The fields are public, so a hand-built image can lie about its size; in
    /// debug builds that is a clear panic, not an index error deep inside.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "pixel count")]
    fn an_image_that_lies_about_its_size_is_a_clear_panic() {
        let bad = Gray { width: 10, height: 10, pixels: vec![255; 5] };
        sauvola(&bad, 15, 0.34);
    }

    #[test]
    fn shrinking_keeps_thin_strokes() {
        // 1 px vertical strokes every 8 px, 192 px high, shrunk 4x.
        let (w, h) = (192u32, 192u32);
        let px = (0..h).flat_map(|_| (0..w).map(|x| if x % 8 == 0 { 0 } else { 255 })).collect();
        let small = fit_height(&Gray::new(w, h, px), 48);
        assert_eq!((small.width, small.height), (48, 48));
        let darkest = *small.pixels[..48].iter().min().unwrap();
        assert!(darkest < 230, "strokes vanished: {darkest}");
        // Upscaling a uniform image stays uniform.
        let up = fit_height(&Gray::new(3, 2, vec![77; 6]), 48);
        assert!(up.pixels.iter().all(|&v| v == 77));
    }

    #[test]
    fn edge_cases_do_not_panic() {
        let one = Gray::new(1, 1, vec![0]);
        assert_eq!(rotate(&one, 180.0).pixels, vec![0]);
        assert_eq!(rotate(&one, f32::NAN), one);
        assert_eq!(one.crop(Rect { x: u32::MAX, y: 0, w: 2, h: 1 }).width, 0);
        // A uniform image has no ink.
        assert!(otsu(&Gray::new(4, 4, vec![100; 16])).pixels.iter().all(|&v| v == 255));
        assert!(lines(&otsu(&Gray::new(4, 4, vec![100; 16]))).is_empty());
        assert_eq!(skew_angle(&Gray::new(4, 4, vec![255; 16]), 1e9), 0.0);
        for (w, h) in [(0, 0), (0, 5), (5, 0), (1, 5), (5, 1)] {
            let g = Gray::new(w, h, vec![0; (w * h) as usize]);
            let _ = prepare_page(&g).unwrap();
            let _ = fit_height(&g, 48);
        }
    }

    #[test]
    fn light_on_dark_is_detected_and_dark_on_light_is_not() {
        let Some(p) = page(&TEXT[..1]) else { return };
        let (_, boxes) = prepare_page(&p).unwrap();
        let line = p.crop(boxes[0]);
        assert!(!is_light_on_dark(&line));
        assert!(is_light_on_dark(&invert(&line)));
        assert_eq!(polarity(&line), Polarity::DarkOnLight);
        assert_eq!(polarity(&invert(&line)), Polarity::LightOnDark);
        assert_eq!(invert(&invert(&line)), line);
        // Degenerate sizes are never "inverted".
        assert!(!is_light_on_dark(&Gray::new(2, 2, vec![0; 4])));
        assert!(!is_light_on_dark(&Gray::new(0, 0, vec![])));
    }

    #[test]
    fn finds_every_line_and_keeps_marks_inside() {
        let Some(p) = page(&TEXT) else { return };
        let boxes = lines(&otsu(&p));
        assert_eq!(boxes.len(), 4, "{boxes:?}");
        // No ink left outside the boxes (marks above/below were not cut off).
        let bw = otsu(&p);
        for y in 0..p.height {
            for x in 0..p.width {
                if bw.pixels[(y * p.width + x) as usize] == 0 {
                    assert!(
                        boxes.iter().any(|b| x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h),
                        "ink at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn measures_and_corrects_skew() {
        let Some(p) = page(&TEXT) else { return };
        for deg in [-3.0f32, -1.2, 2.0, 4.0] {
            let tilted = rotate(&p, deg);
            let found = skew_angle(&otsu(&tilted), 5.0);
            assert!((found + deg).abs() <= 0.3, "rotated {deg}, found {found}");
        }
        let (_, boxes) = prepare_page(&rotate(&p, 2.5)).unwrap();
        assert_eq!(boxes.len(), 4);
    }

    #[test]
    fn sauvola_survives_a_shadow_that_breaks_otsu() {
        let Some(p) = page(&TEXT) else { return };
        // Darken the right half strongly (a shadow across the page).
        let mut shaded = p.clone();
        for y in 0..p.height {
            for x in p.width / 2..p.width {
                let i = (y * p.width + x) as usize;
                shaded.pixels[i] = (shaded.pixels[i] as f32 * 0.45) as u8;
            }
        }
        let ink = |g: &Gray| g.pixels.iter().filter(|&&v| v == 0).count() as f32 / g.pixels.len() as f32;
        let truth = ink(&otsu(&p));
        let local = ink(&sauvola(&shaded, 41, 0.34));
        let global = ink(&otsu(&shaded));
        assert!(
            (local - truth).abs() < (global - truth).abs(),
            "sauvola {local}, otsu {global}, clean {truth}"
        );
        assert!(local < truth * 2.0);
    }

    /// A clean two-level crop (screenshot, synthetic line) has Otsu threshold
    /// 0: the black border must still count as dark.
    #[test]
    fn two_level_white_on_black_is_light_on_dark() {
        let (w, h) = (40u32, 12u32);
        let mut px = vec![0u8; (w * h) as usize];
        for y in 4..8 {
            for x in 5..35 {
                px[(y * w + x) as usize] = 255;
            }
        }
        let line = Gray::new(w, h, px);
        assert_eq!(otsu_threshold(&line), 0);
        assert_eq!(polarity(&line), Polarity::LightOnDark);
        assert_eq!(polarity(&invert(&line)), Polarity::DarkOnLight);
    }

    #[test]
    fn crops_scale_to_the_recognizer_height() {
        let Some(p) = page(&TEXT[..1]) else { return };
        let b = lines(&otsu(&p))[0];
        let line = fit_height(&p.crop(b), 48);
        assert_eq!(line.height, 48);
        assert!(line.width > 48 * 3);
        assert_eq!(otsu_threshold(&Gray::new(2, 1, vec![0, 255])), 0);
    }
}
