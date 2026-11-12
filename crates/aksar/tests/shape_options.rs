//! Option validation in `aksar::shape`: bad input is an error, never a silent fallback.
#![cfg(feature = "shape")]

use aksar::shape::{Error, Font, ShapeOptions, Shaper, shape};

fn font() -> Font {
    Font::from_bytes(include_bytes!("../../../testdata/fonts/NotoSansKhmer-Regular.ttf").to_vec()).unwrap()
}

// An empty or malformed tag used to fall back to "no language" without a word,
// so a typo like "km_KH" silently changed which OpenType rules applied.
#[test]
fn an_unparsable_language_is_an_error() {
    let f = font();
    for bad in ["", "km_KH!", "not a tag"] {
        let opts = ShapeOptions::px(16.0).with_language(Some(bad));
        assert_eq!(shape("ក", &f, &opts).unwrap_err(), Error::InvalidLanguage, "{bad:?}");
        assert!(Shaper::new(&f, &opts).is_err());
    }
    // No language at all is still allowed.
    assert!(shape("ក", &f, &ShapeOptions::px(16.0).with_language(None)).is_ok());
    assert!(shape("ក", &f, &ShapeOptions::px(16.0).with_language(Some("km"))).is_ok());
}

// A NaN size used to give NaN positions after `set_size` (only `new` checked).
#[test]
fn set_size_validates_like_new_and_keeps_the_old_size() {
    let f = font();
    let mut s = Shaper::new(&f, &ShapeOptions::px(10.0)).unwrap();
    let w10 = s.shape("កម្ពុជា").unwrap().width;
    for bad in [f32::NAN, 0.0, -1.0, f32::INFINITY] {
        assert_eq!(s.set_size(Some(bad)), Err(Error::InvalidSize), "{bad}");
    }
    assert_eq!(s.shape("កម្ពុជា").unwrap().width, w10, "failed set_size must not change the size");
    s.set_size(Some(20.0)).unwrap();
    assert!((s.shape("កម្ពុជា").unwrap().width - 2.0 * w10).abs() < 1e-3);
    s.set_size(None).unwrap();
}

#[test]
fn with_features_applies_and_bad_features_error() {
    let f = font();
    let ok = ShapeOptions::px(16.0).with_features(["kern=0", "+liga"]);
    assert_eq!(ok.features, ["kern=0", "+liga"]);
    assert!(shape("ក", &f, &ok).is_ok());
    let bad = ShapeOptions::px(16.0).with_features(["???"]);
    assert_eq!(shape("ក", &f, &bad).unwrap_err(), Error::InvalidFeature);
}
