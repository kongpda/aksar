#![no_main]

// The page pipeline that runs after PNG decoding. aksar-ocr's decoder has no target:
// its rten dependency does not link with coverage instrumentation (see README).
use aksar_image::{Gray, prepare_page};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let [w, h, rest @ ..] = data else { return };
    // Pages up to 255 x 255 keep runs fast and still cover the edge cases (0, 1, thin).
    let (w, h) = (u32::from(*w), u32::from(*h));
    let pixels =
        (0..(w * h) as usize).map(|i| if rest.is_empty() { 255 } else { rest[i % rest.len()] }).collect();
    // 255 x 255 is far below the page cap, so this never fails.
    let (page, boxes) = prepare_page(&Gray::new(w, h, pixels)).expect("small page");
    assert_eq!(page.pixels.len(), (page.width * page.height) as usize);
    for b in boxes {
        assert!(
            b.x + b.w <= page.width && b.y + b.h <= page.height,
            "{b:?} outside {}x{}",
            page.width,
            page.height
        );
    }
});
