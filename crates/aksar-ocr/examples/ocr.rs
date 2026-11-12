//! Read a PNG page or photo.
//!
//! cargo run --release -p aksar-ocr --example ocr -- <model.onnx> <alphabet.json> <image.png> [--json]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: ocr <model.onnx> <alphabet.json> <image.png> [--json]");
        std::process::exit(2);
    }
    let read = |p: &str| std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let ocr = aksar_ocr::Ocr::new(read(&args[0]), &String::from_utf8(read(&args[1])).expect("UTF-8"))
        .unwrap_or_else(|e| panic!("{e}"));
    let page = ocr.read_png(&read(&args[2])).unwrap_or_else(|e| panic!("{e}"));
    if args.iter().any(|a| a == "--json") {
        println!("{}", page.to_json());
    } else {
        println!("{}", page.text());
        for w in page.document.words().filter(|w| w.needs_review()) {
            eprintln!("review: {} {:?}", w.text, w.suggestions);
        }
    }
}
