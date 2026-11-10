//! Saves the page pipeline's intermediate images and prints the line boxes,
//! to see why lines are missed.
//!
//! cargo run --release -p aksar-ocr --example page_debug -- <image.png> <out-prefix>
use aksar_image::Gray;

fn save(img: &Gray, path: &str) {
    let mut enc = png::Encoder::new(std::fs::File::create(path).unwrap(), img.width, img.height);
    enc.set_color(png::ColorType::Grayscale);
    enc.write_header().unwrap().write_image_data(&img.pixels).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&args[0]).unwrap()));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    let ch = info.color_type.samples();
    let px = buf[..info.buffer_size()].chunks(ch).map(|p| p[0]).collect();
    let img = Gray::new(info.width, info.height, px);
    let window = (img.height / 30).clamp(15, 61) | 1;
    let bw = aksar_image::sauvola(&img, window, 0.34);
    save(&bw, &format!("{}-sauvola.png", args[1]));
    let angle = aksar_image::skew_angle(&bw, 5.0);
    println!("window {window}, skew {angle:.1} degrees");
    let straight = aksar_image::otsu(&aksar_image::rotate(&bw, angle));
    save(&straight, &format!("{}-straight.png", args[1]));
    let w = straight.width as usize;
    let rows: Vec<usize> =
        straight.pixels.chunks(w).map(|r| r.iter().filter(|&&p| p < 128).count()).collect();
    println!("row ink 300..460: {:?}", &rows[300.min(rows.len())..460.min(rows.len())]);
    let (_, boxes) = aksar_image::prepare_page(&img).unwrap();
    println!("{} lines: {boxes:?}", boxes.len());
}
