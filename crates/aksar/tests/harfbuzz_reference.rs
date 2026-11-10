//! Checks aksar against reference output from HarfBuzz (see scripts/gen_golden.py).

use aksar::syllable::{normalize, repair, validate};

const VALIDITY: &str = include_str!("../../../testdata/golden/validity.tsv");

fn decode(hex: &str) -> String {
    hex.split(' ').map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap()).collect()
}

fn rows(data: &str) -> impl Iterator<Item = (String, &str)> {
    data.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.starts_with('#') && !l.is_empty()).map(|l| {
        let (text, rest) = l.split_once('\t').unwrap();
        (decode(text), rest)
    })
}

/// The validator's errors must match HarfBuzz's dotted circles exactly.
#[test]
fn validator_matches_harfbuzz_broken_clusters() {
    let mut mismatches = Vec::new();
    let mut n = 0;
    for (text, broken) in rows(VALIDITY) {
        n += 1;
        let hb_broken = broken == "1";
        if validate(&text).is_err() != hb_broken {
            mismatches.push(format!(
                "{:04X?} harfbuzz_broken={hb_broken}",
                text.chars().map(|c| c as u32).collect::<Vec<_>>()
            ));
        }
    }
    assert!(n > 5000, "{n}");
    assert!(
        mismatches.is_empty(),
        "{} of {n} disagree:\n{}",
        mismatches.len(),
        mismatches[..mismatches.len().min(30)].join("\n")
    );
}

/// `repair` (OCR cleanup) has the same guarantees as `normalize`.
#[test]
fn repair_is_safe_on_fuzz_corpus() {
    for (text, broken) in rows(VALIDITY) {
        let r = repair(&text);
        assert_eq!(repair(&r), r, "not idempotent: {text:?}");
        if broken == "0" {
            assert!(validate(&r).is_ok(), "repair broke {text:?} -> {r:?}");
        }
    }
}

/// Search index form must be idempotent, so a query and a stored page that were
/// normalized a different number of times still match.
#[cfg(feature = "search")]
#[test]
fn index_form_is_idempotent_on_fuzz_corpus() {
    for (text, _) in rows(VALIDITY) {
        let i = aksar::search::index_form(&text);
        assert_eq!(aksar::search::index_form(&i), i, "{text:?}");
    }
}

/// Normalizing must never turn renderable text into broken text, and must be idempotent.
#[test]
fn normalize_is_safe_on_fuzz_corpus() {
    for (text, broken) in rows(VALIDITY) {
        let n = normalize(&text);
        assert_eq!(normalize(&n), n, "not idempotent: {text:?}");
        if broken == "0" {
            assert!(validate(&n).is_ok(), "normalize broke {text:?} -> {n:?}");
        }
    }
}

#[cfg(feature = "shape")]
mod shaping {
    use super::*;
    use aksar::shape::{Font, GlyphRun, ShapeOptions, Shaper, shape};

    const GOLDEN: &str = include_str!("../../../testdata/golden/noto-sans-khmer.tsv");
    const FONT: &[u8] = include_bytes!("../../../testdata/fonts/NotoSansKhmer-Regular.ttf");

    fn font() -> Font {
        Font::from_bytes(FONT.to_vec()).unwrap()
    }

    /// Every corpus line must produce exactly HarfBuzz's glyphs and positions.
    #[test]
    fn corpus_matches_harfbuzz() {
        let font = font();
        let mut n = 0;
        for (text, expected) in rows(GOLDEN) {
            n += 1;
            let run = shape(&text, &font, &ShapeOptions::default()).unwrap();
            let got = run
                .glyphs
                .iter()
                .map(|g| {
                    format!("{}:{}:{}:{}:{}", g.glyph_id, g.cluster, g.x_advance, g.x_offset, g.y_offset)
                })
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(got, expected, "text {text:?}");
        }
        assert!(n >= 1000, "golden has {n} lines; run scripts/gen_corpus.py and gen_golden.py");
    }

    /// One reused `Shaper` (cached plan + buffer) must give the same results.
    #[test]
    fn reused_shaper_matches_harfbuzz() {
        let font = font();
        let mut shaper = Shaper::new(&font, &ShapeOptions::default()).unwrap();
        let mut run = GlyphRun::default();
        for _ in 0..2 {
            for (text, expected) in rows(GOLDEN) {
                shaper.shape_into(&text, &mut run).unwrap();
                let got = run
                    .glyphs
                    .iter()
                    .map(|g| {
                        format!("{}:{}:{}:{}:{}", g.glyph_id, g.cluster, g.x_advance, g.x_offset, g.y_offset)
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                assert_eq!(got, expected, "text {text:?}");
            }
        }
    }

    #[test]
    fn font_covers_khmer() {
        assert!(font().covers_khmer());
        assert_eq!(font().units_per_em(), 1000);
    }

    #[test]
    fn coeng_ro_is_reordered_before_base() {
        let font = font();
        let run = shape("ក្រ", &font, &ShapeOptions::default()).unwrap();
        // Coeng-RO draws to the left of the base, so the base is not first.
        assert_ne!(run.glyphs[0].glyph_id, font.glyph_for('ក').unwrap());
        assert_eq!(run.glyphs[1].glyph_id, font.glyph_for('ក').unwrap());
    }

    #[test]
    fn broken_text_reports_dotted_circles() {
        let font = font();
        assert_eq!(shape("ា", &font, &ShapeOptions::default()).unwrap().dotted_circles, 1);
        assert_eq!(shape("កា", &font, &ShapeOptions::default()).unwrap().dotted_circles, 0);
        assert_eq!(shape("\u{25CC}ា", &font, &ShapeOptions::default()).unwrap().dotted_circles, 0);
    }

    #[test]
    fn scaling_and_determinism() {
        let font = font();
        let a = shape("ខ្មែរ", &font, &ShapeOptions::px(20.0)).unwrap();
        let b = shape("ខ្មែរ", &font, &ShapeOptions::px(20.0)).unwrap();
        assert_eq!(a, b);
        let units = shape("ខ្មែរ", &font, &ShapeOptions::default()).unwrap();
        assert!((a.width - units.width * 20.0 / 1000.0).abs() < 1e-3);
        assert_eq!(a.missing, 0);
    }

    #[test]
    fn features_and_errors() {
        let font = font();
        let mut o = ShapeOptions::default();
        o.features.push("kern=0".into());
        assert!(shape("ខ្មែរ", &font, &o).is_ok());
        o.features.push("not a feature!!".into());
        assert!(shape("ខ្មែរ", &font, &o).is_err());
        assert!(Font::from_bytes(b"not a font".to_vec()).is_err());
    }

    /// A NaN or negative size would give NaN or mirrored positions that look
    /// like a result; it must be an error instead.
    #[test]
    fn bad_sizes_are_errors() {
        let font = font();
        for size in [f32::NAN, f32::INFINITY, 0.0, -12.0] {
            assert_eq!(
                shape("ខ្មែរ", &font, &ShapeOptions::px(size)).unwrap_err(),
                aksar::shape::Error::InvalidSize
            );
        }
        assert!(shape("ខ្មែរ", &font, &ShapeOptions::px(0.5)).is_ok());
    }
}
