//! Degrades clean renders the way scans and photos do. Every operation is
//! deterministic for a given seed, so a dataset can be rebuilt exactly.
//!
//! Which ones help is decided by measurement on real scans (`docs/OCR_PLAN.md`
//! §5.1); [`Augment::random`] draws a plausible mix.

use crate::GrayImage;

/// Small, fast, seedable generator (xorshift64*). Not for cryptography.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator for `seed` (any value, 0 included).
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
    /// Approximately normal (mean 0, sd 1).
    pub fn normal(&mut self) -> f32 {
        (0..4).map(|_| self.f32()).sum::<f32>() * 1.732 - 3.464
    }
    /// True with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
}

/// Largest blur and ink radius applied. The filters cost `(2r + 1)^2` per pixel,
/// so a huge radius from a config typo would hang instead of erroring.
pub const MAX_RADIUS: u32 = 16;

/// One degradation step.
#[derive(Clone, Debug, PartialEq)]
pub enum Augment {
    /// Box blur with this radius in pixels (1-2 looks like a soft scan).
    /// Radii above [`MAX_RADIUS`] are treated as `MAX_RADIUS`.
    Blur(u32),
    /// Gaussian-like pixel noise with this standard deviation (gray levels).
    Noise(f32),
    /// Salt and pepper: this share of pixels set to black or white.
    SaltPepper(f32),
    /// Thicker (positive) or thinner (negative) strokes by this many pixels,
    /// at most [`MAX_RADIUS`] either way.
    Ink(i32),
    /// Contrast multiplier around mid-gray, then brightness offset.
    Contrast(f32, f32),
    /// Rotation in degrees (small: ±3 for scans).
    Rotate(f32),
    /// Downscale by this factor and back up (low resolution, e.g. 0.5).
    LowRes(f32),
    /// Uneven lighting: brightness changing linearly by this much across the width.
    Gradient(f32),
}

impl Augment {
    /// A random, plausible mix for scanned or photographed print.
    pub fn random(rng: &mut Rng) -> Vec<Augment> {
        let mut v = Vec::new();
        if rng.chance(0.4) {
            v.push(Augment::Rotate(rng.range(-2.5, 2.5)));
        }
        if rng.chance(0.3) {
            v.push(Augment::Ink(if rng.chance(0.5) { 1 } else { -1 }));
        }
        if rng.chance(0.3) {
            v.push(Augment::LowRes(rng.range(0.45, 0.8)));
        }
        if rng.chance(0.4) {
            v.push(Augment::Blur(1 + (rng.f32() * 1.5) as u32));
        }
        if rng.chance(0.3) {
            v.push(Augment::Gradient(rng.range(-60.0, 60.0)));
        }
        if rng.chance(0.4) {
            v.push(Augment::Contrast(rng.range(0.6, 1.1), rng.range(-30.0, 20.0)));
        }
        if rng.chance(0.5) {
            v.push(Augment::Noise(rng.range(3.0, 18.0)));
        }
        if rng.chance(0.15) {
            v.push(Augment::SaltPepper(rng.range(0.001, 0.01)));
        }
        v
    }

    /// Applies this step. The image keeps its size.
    pub fn apply(&self, img: &GrayImage, rng: &mut Rng) -> GrayImage {
        match *self {
            Augment::Blur(r) => blur(img, r),
            Augment::Noise(sd) => map(img, |p| p as f32 + sd * rng.normal()),
            Augment::SaltPepper(share) => {
                map(
                    img,
                    |p| {
                        if rng.chance(share) { if rng.chance(0.5) { 0.0 } else { 255.0 } } else { p as f32 }
                    },
                )
            }
            Augment::Ink(n) => morph(img, n),
            Augment::Contrast(c, b) => map(img, |p| (p as f32 - 128.0) * c + 128.0 + b),
            Augment::Rotate(deg) => rotate(img, deg),
            Augment::LowRes(f) => low_res(img, f),
            Augment::Gradient(amount) => {
                let w = img.width.max(1) as f32;
                let mut out = img.clone();
                for y in 0..img.height {
                    for x in 0..img.width {
                        let i = (y * img.width + x) as usize;
                        let v = img.pixels[i] as f32 + amount * (x as f32 / w - 0.5);
                        out.pixels[i] = v.round().clamp(0.0, 255.0) as u8;
                    }
                }
                out
            }
        }
    }
}

/// Applies `steps` in order.
pub fn apply_all(img: &GrayImage, steps: &[Augment], rng: &mut Rng) -> GrayImage {
    steps.iter().fold(img.clone(), |acc, s| s.apply(&acc, rng))
}

fn map(img: &GrayImage, mut f: impl FnMut(u8) -> f32) -> GrayImage {
    GrayImage {
        width: img.width,
        height: img.height,
        pixels: img.pixels.iter().map(|&p| f(p).round().clamp(0.0, 255.0) as u8).collect(),
    }
}

fn blur(img: &GrayImage, r: u32) -> GrayImage {
    let r = i64::from(r.min(MAX_RADIUS));
    let mut out = img.clone();
    for y in 0..img.height as i64 {
        for x in 0..img.width as i64 {
            let (mut sum, mut n) = (0u32, 0u32);
            for dy in -r..=r {
                for dx in -r..=r {
                    sum += img.get(x + dx, y + dy) as u32;
                    n += 1;
                }
            }
            out.pixels[y as usize * img.width as usize + x as usize] = (sum / n) as u8;
        }
    }
    out
}

/// Dilate ink (n > 0, min filter) or erode it (n < 0, max filter).
fn morph(img: &GrayImage, n: i32) -> GrayImage {
    let r = i64::from(n.unsigned_abs().min(MAX_RADIUS));
    let mut out = img.clone();
    for y in 0..img.height as i64 {
        for x in 0..img.width as i64 {
            let mut v = if n > 0 { 255u8 } else { 0u8 };
            for dy in -r..=r {
                for dx in -r..=r {
                    let p = img.get(x + dx, y + dy);
                    v = if n > 0 { v.min(p) } else { v.max(p) };
                }
            }
            out.pixels[y as usize * img.width as usize + x as usize] = v;
        }
    }
    out
}

fn bilinear(img: &GrayImage, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |dx, dy| img.get(x0 + dx, y0 + dy) as f32;
    p(0, 0) * (1.0 - fx) * (1.0 - fy)
        + p(1, 0) * fx * (1.0 - fy)
        + p(0, 1) * (1.0 - fx) * fy
        + p(1, 1) * fx * fy
}

fn rotate(img: &GrayImage, deg: f32) -> GrayImage {
    let (s, c) = deg.to_radians().sin_cos();
    let (cx, cy) = (img.width as f32 / 2.0, img.height as f32 / 2.0);
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let (sx, sy) = (c * dx + s * dy + cx, -s * dx + c * dy + cy);
            out.pixels[(y * img.width + x) as usize] = bilinear(img, sx, sy).round() as u8;
        }
    }
    out
}

fn low_res(img: &GrayImage, f: f32) -> GrayImage {
    let f = f.clamp(0.1, 1.0);
    let (w, h) =
        (((img.width as f32 * f).round() as u32).max(1), ((img.height as f32 * f).round() as u32).max(1));
    let mut small = GrayImage::white(w, h);
    for y in 0..h {
        for x in 0..w {
            small.pixels[(y * w + x) as usize] = bilinear(img, x as f32 / f, y as f32 / f).round() as u8;
        }
    }
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            out.pixels[(y * img.width + x) as usize] =
                bilinear(&small, x as f32 * f, y as f32 * f).round() as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GrayImage {
        let mut img = GrayImage::white(40, 20);
        for y in 5..15 {
            for x in 10..30 {
                img.pixels[y * 40 + x] = 0;
            }
        }
        img
    }

    /// A radius past MAX_RADIUS is clamped: the result equals MAX_RADIUS instead
    /// of running a (2r + 1)^2 filter that would never finish.
    #[test]
    fn huge_radii_are_clamped() {
        let img = sample();
        let mut rng = Rng::new(1);
        assert_eq!(
            Augment::Blur(u32::MAX).apply(&img, &mut rng),
            Augment::Blur(MAX_RADIUS).apply(&img, &mut rng)
        );
        assert_eq!(
            Augment::Ink(i32::MIN).apply(&img, &mut rng),
            Augment::Ink(-(MAX_RADIUS as i32)).apply(&img, &mut rng)
        );
        assert_eq!(
            Augment::Ink(i32::MAX).apply(&img, &mut rng),
            Augment::Ink(MAX_RADIUS as i32).apply(&img, &mut rng)
        );
    }

    #[test]
    fn every_step_keeps_the_size_and_is_repeatable() {
        let img = sample();
        let steps = [
            Augment::Blur(1),
            Augment::Noise(10.0),
            Augment::SaltPepper(0.01),
            Augment::Ink(1),
            Augment::Ink(-1),
            Augment::Contrast(0.7, -10.0),
            Augment::Rotate(2.0),
            Augment::LowRes(0.5),
            Augment::Gradient(40.0),
        ];
        for s in &steps {
            let a = s.apply(&img, &mut Rng::new(7));
            assert_eq!((a.width, a.height), (40, 20), "{s:?}");
            assert_eq!(a, s.apply(&img, &mut Rng::new(7)), "{s:?} with the same seed");
        }
    }

    #[test]
    fn ink_steps_change_stroke_weight() {
        let img = sample();
        let thick = Augment::Ink(1).apply(&img, &mut Rng::new(0));
        let thin = Augment::Ink(-1).apply(&img, &mut Rng::new(0));
        assert!(thick.ink() > img.ink() && thin.ink() < img.ink());
    }

    #[test]
    fn random_mix_is_seeded() {
        assert_eq!(Augment::random(&mut Rng::new(3)), Augment::random(&mut Rng::new(3)));
    }
}
