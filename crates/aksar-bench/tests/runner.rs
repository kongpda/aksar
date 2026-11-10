//! End-to-end checks of the bench binary on a tiny set drawn with aksar-render.

use aksar_bench::{load_set, tesseract_installed};
use aksar_ocr::decode_png;
use aksar_render::{LineRenderer, RenderOptions};
use std::path::PathBuf;
use std::process::{Command, Output};

const LINES: [&str; 2] = ["ព្រះរាជាណាចក្រកម្ពុជា", "ជាតិ សាសនា ព្រះមហាក្សត្រ"];

/// Draws `LINES` into a fresh temp dir in the make_lines format.
fn make_set(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aksar-bench-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let font =
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/fonts/NotoSansKhmer-Regular.ttf"))
            .unwrap();
    let r = LineRenderer::new(font).unwrap();
    let mut tsv = String::new();
    for (i, text) in LINES.iter().enumerate() {
        let file = format!("{i:06}.png");
        r.render(text, &RenderOptions::default()).unwrap().save_png(dir.join(&file)).unwrap();
        tsv.push_str(&format!("{file}\t{text}\tNotoSansKhmer-Regular.ttf\n"));
    }
    std::fs::write(dir.join("labels.tsv"), tsv).unwrap();
    dir
}

fn bench(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aksar-bench")).args(args).output().unwrap()
}

#[test]
fn loads_make_lines_format() {
    let dir = make_set("load");
    let lines = load_set(&dir).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].truth, LINES[1]);
    let gray = decode_png(&std::fs::read(&lines[0].image).unwrap()).unwrap();
    assert!(gray.width > gray.height && gray.height > 0);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The EKYC converter's format: `<stem>.png` + `<stem>.gt.txt`, no labels.tsv.
/// The truth loses only its trailing newline; an image with no `.gt.txt` is not a line.
#[test]
fn loads_gt_txt_pairs_when_there_is_no_labels_tsv() {
    let dir = make_set("gt");
    std::fs::remove_file(dir.join("labels.tsv")).unwrap();
    for (i, text) in LINES.iter().enumerate() {
        std::fs::write(dir.join(format!("{i:06}.gt.txt")), format!("{text}\n")).unwrap();
    }
    std::fs::copy(dir.join("000000.png"), dir.join("unlabelled.png")).unwrap();
    let lines = load_set(&dir).unwrap();
    assert_eq!(lines.iter().map(|l| l.truth.as_str()).collect::<Vec<_>>(), LINES);
    assert!(lines[0].image.ends_with("000000.png"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// With no engine to run, the bench must fail and say why, not print an
/// empty table that looks like a result.
#[test]
fn no_engine_fails_and_names_what_was_skipped() {
    let dir = make_set("none");
    let out = bench(&["--set", dir.to_str().unwrap()]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "printed a table: {}", String::from_utf8_lossy(&out.stdout));
    assert!(err.contains("skipped aksar-ocr: no --model and --alphabet given"), "{err}");
    assert!(err.contains("skipped tesseract: --tesseract not given"), "{err}");
    assert!(err.contains("no engine to run"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// An engine the user asked for must not be dropped quietly.
#[test]
fn tesseract_asked_but_missing_is_an_error() {
    let dir = make_set("no-tess");
    let empty_path = dir.join("empty-path");
    std::fs::create_dir_all(&empty_path).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_aksar-bench"))
        .args(["--set", dir.to_str().unwrap(), "--tesseract"])
        .env("PATH", &empty_path)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(err.contains("--tesseract given but no `tesseract` on PATH"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn model_needs_alphabet() {
    let dir = make_set("model");
    let out = bench(&["--set", dir.to_str().unwrap(), "--model", "m.onnx"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--model and --alphabet go together"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_labels_is_an_error() {
    let dir = std::env::temp_dir().join(format!("aksar-bench-{}-empty", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let err = load_set(&dir).unwrap_err();
    assert!(err.contains("labels.tsv") && err.contains(".gt.txt"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A Tesseract failure (here: no `khm` data in an empty tessdata dir) must
/// fail the run, not be scored as an empty reading. The table still prints,
/// with the failed lines counted and left out of the score.
#[test]
fn tesseract_failure_is_loud() {
    if !tesseract_installed() {
        eprintln!("tesseract not installed: test not run");
        return;
    }
    let dir = make_set("tess");
    let empty = dir.join("no-tessdata");
    std::fs::create_dir_all(&empty).unwrap();
    let out = bench(&["--set", dir.to_str().unwrap(), "--tesseract", "--tessdata", empty.to_str().unwrap()]);
    let err = String::from_utf8_lossy(&out.stderr);
    let table = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success());
    assert!(table.contains("| Failed |"), "{table}");
    assert!(err.contains("failed to read") && err.contains("tesseract failed on"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Tesseract options without `--tesseract` would be ignored, and the run
/// would look like it measured Tesseract with them.
#[test]
fn tesseract_options_need_tesseract() {
    let dir = make_set("orphan");
    for opt in [["--lang", "khm+eng"], ["--tessdata", "/x"]] {
        let out = bench(&["--set", dir.to_str().unwrap(), opt[0], opt[1]]);
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("--lang and --tessdata need --tesseract"), "{err}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn version_prints_the_crate_version() {
    let out = bench(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        concat!("aksar-bench ", env!("CARGO_PKG_VERSION"))
    );
}
