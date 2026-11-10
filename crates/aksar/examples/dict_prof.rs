use std::time::Instant;
fn main() {
    let d = aksar::dict::Dictionary::bundled();
    let tok = "ទីក្រុងភ្នំពេញគឺជារាជធានីនៃប្រទេសកម្ពជា";
    let t = Instant::now();
    for _ in 0..200 {
        std::hint::black_box(d.check(tok));
    }
    println!("{:.2} ms per check", t.elapsed().as_secs_f64() * 1e3 / 200.0);
    // Pieces
    let t = Instant::now();
    for _ in 0..200_000 {
        std::hint::black_box(aksar::syllable::normalize(std::hint::black_box("ម្ពុ")));
    }
    println!("normalize(cluster): {:.2} µs", t.elapsed().as_secs_f64() * 1e6 / 200_000.0);
    let t = Instant::now();
    for _ in 0..200_000 {
        std::hint::black_box(aksar::dict::clusters(std::hint::black_box("ម្ពុ")));
    }
    println!("clusters(cluster): {:.2} µs", t.elapsed().as_secs_f64() * 1e6 / 200_000.0);
    let t = Instant::now();
    for _ in 0..200_000 {
        std::hint::black_box(d.contains(std::hint::black_box("កម្ពុជា")));
    }
    println!("contains: {:.3} µs", t.elapsed().as_secs_f64() * 1e6 / 200_000.0);
}
