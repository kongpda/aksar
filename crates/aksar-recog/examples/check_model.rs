//! Checks an exported model directory before it is used or released: the model
//! loads in rten, matches its alphabet, outputs log-probabilities, and computes
//! what PyTorch computed (`reference.txt`, within 1e-4).
//!
//! ```text
//! cargo run --release -p aksar-recog --example check_model -- <model-dir>
//! ```
//!
//! `<model-dir>` holds `model.onnx`, `alphabet.json` and `reference.txt`, as
//! `training/train_crnn.py` and `training/export_onnx.py` write them. Exit
//! status 0 when every check passes, 1 otherwise.

use aksar_recog::{REFERENCE_FILE, Recognizer};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir] = args.as_slice() else {
        eprintln!("usage: check_model <model-dir>  (with model.onnx, alphabet.json, reference.txt)");
        return ExitCode::from(2);
    };
    let dir = std::path::Path::new(dir);
    let rec = match Recognizer::from_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("FAIL load: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("ok   load: model and alphabet match, output is log-probabilities");
    let reference = dir.join(REFERENCE_FILE);
    match std::fs::read_to_string(&reference) {
        Ok(text) => match rec.check_reference(&text) {
            Ok(diff) => println!("ok   parity: rten matches PyTorch (largest difference {diff:.1e})"),
            Err(e) => {
                eprintln!("FAIL parity: {e}");
                return ExitCode::FAILURE;
            }
        },
        Err(e) => {
            eprintln!("FAIL parity: {}: {e} (re-export with training/export_onnx.py)", reference.display());
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
