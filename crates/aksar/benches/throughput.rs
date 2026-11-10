//! `cargo bench -p aksar` — shaping and validation throughput on the test corpus.

use aksar::shape::{Font, GlyphRun, ShapeOptions, Shaper, shape};
use aksar::syllable::{normalize, validate};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

const FONT: &[u8] = include_bytes!("../../../testdata/fonts/NotoSansKhmer-Regular.ttf");
const CORPUS: &str = include_str!("../../../testdata/corpus/km.txt");

/// The 45 hand-written lines above the generated part, so timings stay comparable with the
/// numbers in the README.
fn lines() -> Vec<&'static str> {
    let hand = CORPUS.split("# ---- generated").next().unwrap();
    hand.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect()
}

fn bench(c: &mut Criterion) {
    let font = Font::from_bytes(FONT.to_vec()).unwrap();
    let lines = lines();
    let bytes: usize = lines.iter().map(|l| l.len()).sum();
    let paragraph = lines.join(" ");

    let mut g = c.benchmark_group("shape");
    g.throughput(Throughput::Bytes(bytes as u64));
    g.bench_function("shape() per line", |b| {
        b.iter(|| {
            for l in &lines {
                black_box(shape(l, &font, &ShapeOptions::default()).unwrap());
            }
        })
    });
    g.bench_function("Shaper reused per line", |b| {
        let mut shaper = Shaper::new(&font, &ShapeOptions::default()).unwrap();
        let mut run = GlyphRun::default();
        b.iter(|| {
            for l in &lines {
                shaper.shape_into(l, &mut run).unwrap();
                black_box(&run);
            }
        })
    });
    g.bench_function("Shaper one paragraph", |b| {
        let mut shaper = Shaper::new(&font, &ShapeOptions::default()).unwrap();
        let mut run = GlyphRun::default();
        b.iter(|| {
            shaper.shape_into(&paragraph, &mut run).unwrap();
            black_box(&run);
        })
    });
    g.finish();

    let mut g = c.benchmark_group("text");
    g.throughput(Throughput::Bytes(paragraph.len() as u64));
    g.bench_function("validate", |b| b.iter(|| black_box(validate(black_box(&paragraph)).is_ok())));
    g.bench_function("normalize", |b| b.iter(|| black_box(normalize(black_box(&paragraph)))));
    g.finish();

    c.bench_function("font load", |b| b.iter(|| black_box(Font::from_bytes(FONT.to_vec()).unwrap())));
}

criterion_group!(benches, bench);
criterion_main!(benches);
