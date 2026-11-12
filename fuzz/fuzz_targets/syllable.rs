#![no_main]

use aksar::syllable::{clusters, normalize, repair, validate};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|s: &str| {
    // Clusters cover the input exactly, in order.
    assert_eq!(clusters(s).concat(), s);

    // normalize and repair are idempotent and never break renderable text.
    let valid = validate(s).is_ok();
    let n = normalize(s);
    assert_eq!(normalize(&n), n, "normalize not idempotent: {s:?}");
    if valid {
        assert!(validate(&n).is_ok(), "normalize broke {s:?} -> {n:?}");
    }
    let r = repair(s);
    assert_eq!(repair(&r), r, "repair not idempotent: {s:?}");
    if valid {
        assert!(validate(&r).is_ok(), "repair broke {s:?} -> {r:?}");
    }
});
