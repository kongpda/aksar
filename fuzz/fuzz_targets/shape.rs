#![no_main]

use std::sync::LazyLock;

use aksar::shape::{Font, ShapeOptions, shape};
use libfuzzer_sys::fuzz_target;

static FONT: LazyLock<Font> = LazyLock::new(|| {
    Font::from_bytes(include_bytes!("../../testdata/fonts/NotoSansKhmer-Regular.ttf").to_vec()).unwrap()
});

fuzz_target!(|s: &str| {
    let run = shape(s, &FONT, &ShapeOptions::default()).unwrap();
    // Every cluster is a byte offset into the text, at a char boundary.
    for g in &run.glyphs {
        assert!(s.is_char_boundary(g.cluster as usize), "cluster {} in {s:?}", g.cluster);
    }
});
