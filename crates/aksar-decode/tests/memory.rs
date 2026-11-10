//! Memory use of the beam search on a long line. Own test binary: the counting
//! allocator sees only this one test.
use aksar_decode::{Decoder, Options};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let now = LIVE.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(now, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// A flat distribution keeps every beam and every symbol alive at each step
/// (the worst case). Prefixes the search drops must be freed: keeping every
/// prefix ever tried took 536 MB at 800 steps (a 2400-step line would need
/// more than a gigabyte).
#[test]
fn a_long_flat_line_does_not_keep_every_prefix_ever_tried() {
    let mut alphabet = vec![String::new()];
    alphabet.extend((0x1780u32..0x17B3).filter_map(char::from_u32).map(String::from));
    let d = Decoder::new(&alphabet).unwrap();
    let flat = vec![-(alphabet.len() as f32).ln(); alphabet.len()];
    let frames = vec![flat; 800];
    let before = LIVE.load(Relaxed);
    PEAK.store(before, Relaxed);
    let out = d.decode(&frames, &Options { unknown_cluster_penalty: 0.0, ..Options::default() });
    assert!(!out.is_empty());
    let used = PEAK.load(Relaxed) - before;
    assert!(used < 32 << 20, "peak {} MB for one 800-step line", used >> 20);
}
