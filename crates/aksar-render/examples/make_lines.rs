//! Make a training set of Khmer text-line images.
//!
//! ```text
//! cargo run --release -p aksar-render --example make_lines -- \
//!     <fonts-dir> <out-dir> [count] [seed] [noisy] [alphabet] [real]
//! ```
//!
//! Lines are random runs of 2-8 words from the bundled dictionary (with Khmer
//! digits now and then), joined without spaces or with a space between
//! phrases, drawn in a random font from `<fonts-dir>` (every .ttf/.otf that
//! covers Khmer) at 20-48 px, then degraded with `augment::Augment::random`.
//! Writes `<out-dir>/000001.png ...` and `<out-dir>/labels.tsv`
//! (`file<TAB>text<TAB>font`). Every label is normalized and valid, and is
//! exactly the text that was drawn. Same seed, same dataset. `noisy` adds strong
//! pixel noise and salt-and-pepper to every line (for noisy scans). `alphabet`
//! replaces about 1 word in 8 with a syllable drawn evenly from the whole
//! alphabet chart (any consonant, often with any subscript, any vowel, or an
//! independent vowel), so forms that are rare in the word list (្ផ, ្ឡ, ្ឍ ...)
//! are seen often enough. `real` imitates line crops from real documents
//! (measured on the EKYC benchmark): longer lines (2-20 words) with Khmer
//! punctuation (។ ៗ ៖), brackets, Western digits, phone/date patterns and
//! English words; smaller text (14-48 px), tight crops, bold weights, stronger
//! low resolution, and slivers of the neighbouring lines above and below.
//! Rotated lines get paper above and below so their ends are not cut off
//! (datasets made before this fix differ only there). Otherwise, without
//! these flags the output is the same as before.

use aksar_render::augment::{Augment, Rng, apply_all};
use aksar_render::{LineRenderer, RenderOptions};
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: make_lines <fonts-dir> <out-dir> [count] [seed] [noisy] [alphabet] [real]");
        std::process::exit(2);
    }
    let (fonts_dir, out_dir) = (&args[0], &args[1]);
    let bad = |what: &str, value: &str| -> ! {
        eprintln!("make_lines: {what} must be a whole number, got {value:?}");
        std::process::exit(2);
    };
    let count: usize = args.get(2).map_or(100, |c| c.parse().unwrap_or_else(|_| bad("count", c)));
    let mut rng = Rng::new(args.get(3).map_or(1, |s| s.parse().unwrap_or_else(|_| bad("seed", s))));
    let flags = args.get(4..).unwrap_or_default();
    if let Some(f) = flags.iter().find(|f| !matches!(f.as_str(), "noisy" | "alphabet" | "real")) {
        eprintln!("make_lines: unknown flag {f:?} (expected noisy, alphabet or real)");
        std::process::exit(2);
    }
    let noisy = flags.iter().any(|a| a == "noisy");
    let alphabet = flags.iter().any(|a| a == "alphabet");
    let real = flags.iter().any(|a| a == "real");

    let mut fonts: Vec<(String, LineRenderer)> = std::fs::read_dir(fonts_dir)
        .expect("fonts dir")
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.ends_with(".ttf") || n.ends_with(".otf")
        })
        .filter_map(|e| {
            let r = LineRenderer::new(std::fs::read(e.path()).ok()?).ok()?;
            r.covers_khmer().then(|| (e.file_name().to_string_lossy().into_owned(), r))
        })
        .collect();
    fonts.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!fonts.is_empty(), "no Khmer font in {fonts_dir}");
    eprintln!("{} Khmer fonts", fonts.len());

    let words: Vec<&str> = aksar::dict::Dictionary::bundled().words().collect();
    std::fs::create_dir_all(out_dir).expect("out dir");
    let mut labels = std::fs::File::create(format!("{out_dir}/labels.tsv")).expect("labels");
    let (mut made, mut skipped) = (0, 0);
    while made < count {
        let n = if real { 2 + (rng.f32() * 19.0) as usize } else { 2 + (rng.f32() * 7.0) as usize };
        let mut text = String::new();
        for i in 0..n {
            if i > 0 && rng.chance(0.25) {
                text.push(' ');
            }
            if real && rng.chance(0.2) {
                text.push_str(&real_token(&mut rng));
            } else if alphabet && rng.chance(0.125) {
                text.push_str(&chart_syllable(&mut rng));
            } else if rng.chance(0.08) {
                let digits: String = (0..1 + (rng.f32() * 4.0) as usize)
                    .map(|_| char::from_u32(0x17E0 + (rng.f32() * 10.0) as u32).unwrap())
                    .collect();
                text.push_str(&digits);
            } else {
                text.push_str(words[(rng.f32() * words.len() as f32) as usize % words.len()]);
            }
        }
        let text = aksar::syllable::normalize(&text);
        let text = if real { text.split_whitespace().collect::<Vec<_>>().join(" ") } else { text };
        if aksar::syllable::validate(&text).is_err() {
            skipped += 1;
            continue;
        }
        let (font_name, font) = &fonts[(rng.f32() * fonts.len() as f32) as usize % fonts.len()];
        let mut opt = RenderOptions {
            size_px: rng.range(20.0, 48.0),
            padding: 4 + (rng.f32() * 10.0) as u32,
            ink: (rng.f32() * 60.0) as u8,
            paper: 255 - (rng.f32() * 40.0) as u8,
            ..RenderOptions::default()
        };
        if real {
            opt.size_px = rng.range(14.0, 48.0);
            opt.padding = (rng.f32() * 6.0) as u32;
            if rng.chance(0.3) {
                opt.variations = vec![("wght".into(), rng.range(550.0, 800.0))];
            }
        }
        let Ok(mut img) = font.render(&text, &opt) else {
            skipped += 1;
            continue;
        };
        if real && rng.chance(0.4) {
            // Slivers of the lines above and below, as loose line boxes catch them.
            let other = |rng: &mut Rng| {
                let t: String =
                    (0..6).map(|_| words[(rng.f32() * words.len() as f32) as usize % words.len()]).collect();
                font.render(&aksar::syllable::normalize(&t), &RenderOptions { padding: 0, ..opt.clone() })
                    .ok()
            };
            if let Some(above) = other(&mut rng) {
                let k = 1 + (rng.f32() * (above.height as f32 * 0.25)) as u32;
                img = stack(&crop_rows(&above, above.height - k, k, opt.paper), &img, opt.paper);
            }
            // Nested, not a let chain: this crate builds with Rust 1.85.
            #[allow(clippy::collapsible_if)]
            if rng.chance(0.6) {
                if let Some(below) = other(&mut rng) {
                    let k = 1 + (rng.f32() * (below.height as f32 * 0.25)) as u32;
                    img = stack(&img, &crop_rows(&below, 0, k, opt.paper), opt.paper);
                }
            }
        }
        let mut steps = Augment::random(&mut rng);
        if real {
            // Stronger low resolution, combined with any from `random`, but
            // never below ~14 px text: smaller is unreadable and its label noise.
            let at = steps.iter().position(|s| matches!(s, Augment::LowRes(_)));
            let mut factor = at.map_or(1.0, |i| match steps[i] {
                Augment::LowRes(f) => f,
                _ => 1.0,
            });
            if rng.chance(0.4) {
                factor *= rng.range(0.3, 0.6);
            }
            let factor = factor.max((14.0 / opt.size_px).min(1.0));
            match at {
                Some(i) => steps[i] = Augment::LowRes(factor),
                // Before blur and noise, where `random` puts it.
                None if factor < 1.0 => {
                    let i = steps
                        .iter()
                        .position(|s| !matches!(s, Augment::Rotate(_) | Augment::Ink(_)))
                        .unwrap_or(steps.len());
                    steps.insert(i, Augment::LowRes(factor));
                }
                None => {}
            }
        }
        // Rotation keeps the image size: give the tilted line room so its ends
        // are not cut off (the label still has those letters).
        if let Some(deg) = steps.iter().find_map(|s| match s {
            Augment::Rotate(d) => Some(*d),
            _ => None,
        }) {
            let pad = (img.width as f32 / 2.0 * deg.to_radians().sin().abs()).ceil() as u32 + 1;
            let band = aksar_render::GrayImage {
                width: img.width,
                height: pad,
                pixels: vec![opt.paper; (img.width * pad) as usize],
            };
            img = stack(&stack(&band, &img, opt.paper), &band, opt.paper);
        }
        if noisy {
            steps.push(Augment::Noise(rng.range(12.0, 30.0)));
            steps.push(Augment::SaltPepper(rng.range(0.004, 0.02)));
        }
        let img = apply_all(&img, &steps, &mut rng);
        made += 1;
        let file = format!("{made:06}.png");
        img.save_png(format!("{out_dir}/{file}")).expect("png");
        writeln!(labels, "{file}\t{text}\t{font_name}").expect("labels");
    }
    eprintln!("wrote {made} lines to {out_dir} ({skipped} skipped: font gaps or invalid text)");
}

/// A random syllable from the alphabet chart, every letter equally likely.
fn chart_syllable(rng: &mut Rng) -> String {
    const CONSONANTS: [char; 33] = [
        'ក', 'ខ', 'គ', 'ឃ', 'ង', 'ច', 'ឆ', 'ជ', 'ឈ', 'ញ', 'ដ', 'ឋ', 'ឌ', 'ឍ', 'ណ', 'ត', 'ថ', 'ទ', 'ធ', 'ន',
        'ប', 'ផ', 'ព', 'ភ', 'ម', 'យ', 'រ', 'ល', 'វ', 'ស', 'ហ', 'ឡ', 'អ',
    ];
    const VOWELS: [&str; 24] = [
        "ា", "ិ", "ី", "ឹ", "ឺ", "ុ", "ូ", "ួ", "ើ", "ឿ", "ៀ", "េ", "ែ", "ៃ", "ោ", "ៅ", "ុំ", "ំ", "ាំ", "ះ", "ិះ", "ុះ",
        "េះ", "ោះ",
    ];
    const INDEPENDENT: [&str; 14] = ["អា", "ឥ", "ឦ", "ឧ", "ឩ", "ឪ", "ឫ", "ឬ", "ឭ", "ឮ", "ឯ", "ឰ", "ឱ", "ឳ"];
    let pick = |rng: &mut Rng, n: usize| (rng.f32() * n as f32) as usize % n;
    if rng.chance(0.1) {
        return INDEPENDENT[pick(rng, INDEPENDENT.len())].to_string();
    }
    let mut s = String::from(CONSONANTS[pick(rng, 33)]);
    if rng.chance(0.6) {
        s.push('\u{17D2}');
        s.push(CONSONANTS[pick(rng, 33)]);
    }
    if rng.chance(0.8) {
        s.push_str(VOWELS[pick(rng, VOWELS.len())]);
    }
    s
}

/// A token that real documents have and the word list lacks: punctuation,
/// brackets, Western digits, phone numbers and dates, English words.
fn real_token(rng: &mut Rng) -> String {
    const PUNCT: [&str; 14] = ["។", "ៗ", "៖", "-", "/", "(", ")", ":", ",", ".", "«", "»", "\"", "%"];
    const MORE_PUNCT: &str = "៕៚៙!?;'&@$+<>*#=_[]{}|~^`";
    const LATIN: [&str; 24] = [
        "Phnom", "Penh", "Cambodia", "Kingdom", "Ministry", "of", "the", "and", "No.", "Tel", "Fax", "Email",
        "www", "gov.kh", "Street", "Road", "Sangkat", "Khan", "PDF", "ID", "USD", "KHR", "Page", "Date",
    ];
    let pick = |rng: &mut Rng, n: usize| (rng.f32() * n as f32) as usize % n;
    let digits = |rng: &mut Rng, n: usize, khmer: bool| -> String {
        (0..n)
            .map(|_| char::from_u32(if khmer { 0x17E0 } else { 0x30 } + pick(rng, 10) as u32).unwrap())
            .collect()
    };
    let khmer = rng.chance(0.6);
    match pick(rng, 6) {
        0 | 1 => PUNCT[pick(rng, PUNCT.len())].to_string(),
        2 => format!(
            "({}){} {} {}",
            digits(rng, 3, khmer),
            digits(rng, 2, khmer),
            digits(rng, 2, khmer),
            digits(rng, 2, khmer)
        ),
        3 => format!("{}/{}/{}", digits(rng, 2, khmer), digits(rng, 2, khmer), digits(rng, 4, khmer)),
        4 => {
            let n = 1 + pick(rng, 5);
            digits(rng, n, false)
        }
        _ if rng.chance(0.3) => format!(" {} ", LATIN[pick(rng, LATIN.len())]),
        _ if rng.chance(0.25) => {
            let p: Vec<char> = MORE_PUNCT.chars().collect();
            p[pick(rng, p.len())].to_string()
        }
        _ => {
            // A random Latin word: lower, Title or UPPER case, 2-9 letters.
            let n = 2 + pick(rng, 8);
            let case = pick(rng, 3);
            let w: String = (0..n)
                .map(|i| {
                    let c = (b'a' + pick(rng, 26) as u8) as char;
                    if case == 2 || (case == 1 && i == 0) { c.to_ascii_uppercase() } else { c }
                })
                .collect();
            format!(" {w} ")
        }
    }
}

/// `k` rows of `img` starting at row `y0`.
fn crop_rows(img: &aksar_render::GrayImage, y0: u32, k: u32, paper: u8) -> aksar_render::GrayImage {
    let mut out = aksar_render::GrayImage {
        width: img.width,
        height: k,
        pixels: vec![paper; (img.width * k) as usize],
    };
    for y in 0..k.min(img.height.saturating_sub(y0)) {
        let src = ((y0 + y) * img.width) as usize;
        out.pixels[(y * img.width) as usize..][..img.width as usize]
            .copy_from_slice(&img.pixels[src..][..img.width as usize]);
    }
    out
}

/// `top` above `bottom`, left-aligned, the narrower one padded with paper.
fn stack(
    top: &aksar_render::GrayImage,
    bottom: &aksar_render::GrayImage,
    paper: u8,
) -> aksar_render::GrayImage {
    let width = bottom.width.max(top.width);
    let mut out = aksar_render::GrayImage {
        width,
        height: top.height + bottom.height,
        pixels: vec![paper; (width * (top.height + bottom.height)) as usize],
    };
    for (img, y0) in [(top, 0), (bottom, top.height)] {
        for y in 0..img.height {
            let dst = ((y0 + y) * width) as usize;
            out.pixels[dst..][..img.width as usize]
                .copy_from_slice(&img.pixels[(y * img.width) as usize..][..img.width as usize]);
        }
    }
    out
}
