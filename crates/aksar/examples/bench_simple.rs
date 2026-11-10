//! Tiny timing loop that also runs under WASI: `cargo run --release --example bench_simple`.
use aksar::shape::{Font, GlyphRun, ShapeOptions, Shaper};
use std::time::Instant;

const FONT: &[u8] = include_bytes!("../../../testdata/fonts/NotoSansKhmer-Regular.ttf");
const CORPUS: &str = include_str!("../../../testdata/corpus/km.txt");

fn main() {
    // The 45 hand-written lines above the generated part, as in the README numbers.
    let hand = CORPUS.split("# ---- generated").next().unwrap();
    let lines: Vec<&str> = hand.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let font = Font::from_bytes(FONT.to_vec()).unwrap();
    let mut shaper = Shaper::new(&font, &ShapeOptions::default()).unwrap();
    let mut run = GlyphRun::default();
    let rounds = 2000;
    let t = Instant::now();
    let mut glyphs = 0;
    for _ in 0..rounds {
        for l in &lines {
            shaper.shape_into(l, &mut run).unwrap();
            glyphs += run.glyphs.len();
        }
    }
    let per_round = t.elapsed().as_secs_f64() * 1e6 / rounds as f64;
    println!("{per_round:.1} µs per corpus pass ({} glyphs/pass)", glyphs / rounds);
}
