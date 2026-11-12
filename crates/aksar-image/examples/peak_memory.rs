//! Peak memory of the page pipeline on a synthetic 600 dpi A4 page (4960x7016).
//! Usage: /usr/bin/time -l peak_memory [setup|sauvola|skew|prepare]
//! (`setup` only builds the page: subtract its peak to see what a step adds.)
use aksar_image::{Gray, rotate};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "prepare".into());
    let (w, h) = (4960usize, 7016usize);
    // 60 lines of 100 px with word-like blocks (about 10% ink), tilted 1 degree.
    let mut px = vec![255u8; w * h];
    for line in 0..60 {
        for y in line * 110 + 200..line * 110 + 300 {
            for x in 400..w - 400 {
                if (x / 40) % 5 != 4 && (y - line * 110) % 7 < 5 {
                    px[y * w + x] = 0;
                }
            }
        }
    }
    let page = rotate(&Gray::new(w as u32, h as u32, px), 1.0);
    match mode.as_str() {
        "setup" => {}
        "sauvola" => {
            let bw = aksar_image::sauvola(&page, 61, 0.34);
            println!("ink {}", bw.pixels.iter().filter(|&&p| p == 0).count());
        }
        "skew" => println!("skew {}", aksar_image::skew_angle(&aksar_image::otsu(&page), 5.0)),
        _ => {
            let (gray, boxes) = aksar_image::prepare_page(&page).unwrap();
            println!("{} lines, page {}x{}", boxes.len(), gray.width, gray.height);
        }
    }
}
