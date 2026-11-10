#![no_main]

use aksar::dict::Dictionary;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|s: &str| {
    let d = Dictionary::bundled();
    // Must not panic. Time is bounded by libFuzzer's -timeout (see README).
    let _ = d.check(s);
    assert_eq!(d.segment(s).concat(), s, "segments must cover the token");
});
