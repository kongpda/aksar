//! Reads line images listed in a `labels.tsv` (from aksar-render's
//! `make_lines`) and reports character error rate, greedy vs aksar-decode.
//!
//! cargo run --release -p aksar-recog --example read_lines -- <model.onnx> <alphabet.json> <lines-dir> [limit]
use aksar_image::Gray;

fn load_png(path: &std::path::Path) -> Gray {
    let mut dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    let (w, h) = (info.width, info.height);
    let ch = info.color_type.samples();
    let px = buf[..(w * h) as usize * ch]
        .chunks(ch)
        .map(|p| {
            if ch >= 3 {
                ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8
            } else {
                p[0]
            }
        })
        .collect();
    Gray::new(w, h, px)
}

/// Edit distance over characters, whitespace ignored (as the Tesseract benchmark scores).
fn edit(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (
        a.chars().filter(|c| !c.is_whitespace()).collect(),
        b.chars().filter(|c| !c.is_whitespace()).collect(),
    );
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + usize::from(ca != cb)));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rec = aksar_recog::Recognizer::new(
        std::fs::read(&args[1]).unwrap(),
        &std::fs::read_to_string(&args[2]).unwrap(),
    )
    .unwrap();
    let dir = std::path::Path::new(&args[3]);
    let limit: usize = args.get(4).map_or(usize::MAX, |s| s.parse().unwrap());
    // category -> (chars, greedy edits, decoder edits, lines)
    let mut agg: std::collections::BTreeMap<String, (usize, usize, usize, usize)> = Default::default();
    let mut out = std::env::var("OUT").ok().map(|p| std::fs::File::create(p).unwrap());
    let t = std::time::Instant::now();
    for line in std::fs::read_to_string(dir.join("labels.tsv")).unwrap().lines().take(limit) {
        let mut f = line.split('\t');
        let (Some(file), Some(truth)) = (f.next(), f.next()) else { continue };
        // Score against the same canonical form the model is trained on.
        let truth = &aksar::syllable::normalize(&truth.replace(['\u{200B}', '\u{200C}', '\u{200D}'], ""));
        let category = f.next().unwrap_or("all").to_string();
        let img = load_png(&dir.join(file));
        let frames = rec.frames(&img).unwrap();
        let g = rec.decoder().greedy(&frames);
        let b =
            rec.decoder().decode(&frames, &rec.options).first().map(|c| c.text.clone()).unwrap_or_default();
        let n = truth.chars().filter(|c| !c.is_whitespace()).count();
        let (eg, eb) = (edit(&g, truth), edit(&b, truth));
        for key in [category.clone(), "ALL".to_string()] {
            let e = agg.entry(key).or_default();
            e.0 += n;
            e.1 += eg;
            e.2 += eb;
            e.3 += 1;
        }
        if let Some(o) = out.as_mut() {
            use std::io::Write;
            writeln!(o, "{file}\t{category}\t{n}\t{eg}\t{eb}\t{b}").unwrap();
        }
    }
    let lines = agg.get("ALL").map_or(0, |e| e.3);
    println!("| set | lines | greedy CER | aksar-decode CER |\n|---|---|---|---|");
    for (k, (n, eg, eb, l)) in &agg {
        let n = &(*n).max(1);
        println!(
            "| {k} | {l} | {:.2}% | {:.2}% |",
            100.0 * *eg as f64 / *n as f64,
            100.0 * *eb as f64 / *n as f64
        );
    }
    println!("({:.1} ms/line)", t.elapsed().as_secs_f64() * 1000.0 / lines.max(1) as f64);
}
