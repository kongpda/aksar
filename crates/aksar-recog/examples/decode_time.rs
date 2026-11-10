//! Decoder speed and accuracy at several beam widths, on frames computed once.
//! Usage: decode_time <model.onnx> <alphabet.json> <dir with labels.tsv> [limit]
use aksar_decode::Options;
use aksar_image::Gray;
use std::time::Instant;

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
    let mut lines = Vec::new();
    let t = Instant::now();
    for line in std::fs::read_to_string(dir.join("labels.tsv")).unwrap().lines().take(limit) {
        let mut f = line.split('\t');
        let (Some(file), Some(truth)) = (f.next(), f.next()) else { continue };
        let truth = aksar::syllable::normalize(&truth.replace(['\u{200B}', '\u{200C}', '\u{200D}'], ""));
        let frames = rec.frames(&load_png(&dir.join(file))).unwrap();
        lines.push((truth, frames));
    }
    let model_ms = t.elapsed().as_secs_f64() * 1000.0 / lines.len() as f64;
    println!(
        "{} lines, model {model_ms:.1} ms/line, mean {:.0} steps",
        lines.len(),
        lines.iter().map(|l| l.1.len()).sum::<usize>() as f64 / lines.len() as f64
    );
    println!("| decode | ms/line | CER |\n|---|---|---|");
    let chars: usize = lines.iter().map(|(t, _)| t.chars().filter(|c| !c.is_whitespace()).count()).sum();
    let t = Instant::now();
    let errs: usize = lines.iter().map(|(truth, fr)| edit(&rec.decoder().greedy(fr), truth)).sum();
    println!(
        "| greedy | {:.2} | {:.2}% |",
        t.elapsed().as_secs_f64() * 1000.0 / lines.len() as f64,
        100.0 * errs as f64 / chars as f64
    );
    let runs = [(4, 10.0), (8, 10.0), (16, 10.0), (32, 10.0), (16, f32::INFINITY)];
    for (beam, prune) in runs {
        let opt = Options { beam, prune, ..Options::default() };
        let t = Instant::now();
        let errs: usize =
            lines.iter().map(|(truth, fr)| edit(&rec.decoder().decode(fr, &opt)[0].text, truth)).sum();
        println!(
            "| beam {beam}, prune {prune} | {:.2} | {:.2}% |",
            t.elapsed().as_secs_f64() * 1000.0 / lines.len() as f64,
            100.0 * errs as f64 / chars as f64
        );
    }
}
