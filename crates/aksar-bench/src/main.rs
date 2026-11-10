//! Prints the B0 results table for line sets and engines.
//!
//! ```text
//! cargo run -p aksar-bench --release -- --set <dir>... \
//!     [--model m.onnx --alphabet a.json] [--tesseract [--lang khm] [--tessdata <dir>]]
//! ```
//!
//! A set is a directory with a `labels.tsv`, or `<stem>.png` + `<stem>.gt.txt` pairs.

use aksar_bench::{Engine, Row, load_set, run, table, tesseract_installed};
use std::path::PathBuf;
use std::process::exit;

const USAGE: &str = "usage: aksar-bench --set <dir>... [--model m.onnx --alphabet a.json] \
                     [--tesseract [--lang khm] [--tessdata <dir>]] [-V]";

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e}");
        exit(1);
    }
}

fn real_main() -> Result<(), String> {
    let mut sets: Vec<PathBuf> = Vec::new();
    let (mut model, mut alphabet, mut tessdata) = (None, None, None);
    let mut tesseract = false;
    let mut lang = None;
    let mut args = std::env::args().skip(1).peekable();
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--set" => {
                sets.push(value()?.into());
                while let Some(more) = args.next_if(|n| !n.starts_with('-')) {
                    sets.push(more.into());
                }
            }
            "--model" => model = Some(PathBuf::from(value()?)),
            "--alphabet" => alphabet = Some(PathBuf::from(value()?)),
            "--tessdata" => tessdata = Some(PathBuf::from(value()?)),
            "--lang" => lang = Some(value()?),
            "--tesseract" => tesseract = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("aksar-bench {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }
    if sets.is_empty() {
        return Err(format!("no --set given\n{USAGE}"));
    }
    if !tesseract && (lang.is_some() || tessdata.is_some()) {
        // Otherwise the run would quietly skip the engine these options are for.
        return Err(format!("--lang and --tessdata need --tesseract\n{USAGE}"));
    }
    let lang = lang.unwrap_or_else(|| "khm".into());

    let mut engines = Vec::new();
    let mut skipped = Vec::new();
    let mut model_size = None;
    match (model, alphabet) {
        (Some(m), Some(a)) => {
            let bytes = std::fs::read(&m).map_err(|e| format!("{}: {e}", m.display()))?;
            model_size = Some(bytes.len());
            let json = std::fs::read_to_string(&a).map_err(|e| format!("{}: {e}", a.display()))?;
            let ocr = aksar_ocr::Ocr::new(bytes, &json).map_err(|e| format!("model: {e}"))?;
            engines.push(Engine::Aksar(Box::new(ocr)));
        }
        (None, None) => skipped.push("aksar-ocr: no --model and --alphabet given"),
        _ => return Err("--model and --alphabet go together".into()),
    }
    if !tesseract {
        skipped.push("tesseract: --tesseract not given");
    } else if tesseract_installed() {
        engines.push(Engine::Tesseract { lang, tessdata });
    } else {
        // Asked for by name: a table without it would look like a full result.
        return Err("--tesseract given but no `tesseract` on PATH".into());
    }

    for s in &skipped {
        eprintln!("skipped {s}");
    }
    if engines.is_empty() {
        return Err("no engine to run (see the skipped lines above)".into());
    }

    let mut rows = Vec::new();
    for dir in &sets {
        let lines = load_set(dir)?;
        let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into());
        for engine in &engines {
            eprintln!("{name}: {} ({} lines)", engine.name(), lines.len());
            rows.push(run(&name, &lines, engine));
        }
    }
    print!("{}", table(&rows));
    println!(
        "\nText is normalized with aksar::syllable::normalize before scoring. CER and cluster CER \
         ignore whitespace; WER counts words split with the bundled dictionary."
    );
    if let Some(n) = model_size {
        println!("aksar-ocr model: {:.1} MB.", n as f64 / 1e6);
    }
    // aksar-ocr runs in this process; Tesseract runs as child processes.
    let ran = |tess: bool| engines.iter().any(|e| matches!(e, Engine::Tesseract { .. }) == tess);
    if let Some(n) = peak_rss(Who::This).filter(|_| ran(false)) {
        println!("Peak memory: {:.0} MB in this process (aksar-ocr).", n as f64 / 1e6);
    }
    if let Some(n) = peak_rss(Who::Children).filter(|_| ran(true)) {
        println!("Peak memory: {:.0} MB in the largest child process (Tesseract).", n as f64 / 1e6);
    }
    for s in &skipped {
        println!("Skipped {s}.");
    }
    let failed: Vec<&Row> = rows.iter().filter(|r| r.failed > 0).collect();
    if !failed.is_empty() {
        let lines: Vec<String> = failed
            .iter()
            .map(|r| {
                format!(
                    "{} / {}: {} failed, first: {}",
                    r.set,
                    r.engine,
                    r.failed,
                    r.first_error.as_deref().unwrap_or("?")
                )
            })
            .collect();
        return Err(format!("some lines failed to read (left out of the scores):\n{}", lines.join("\n")));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Who {
    This,
    Children,
}

/// Peak resident memory in bytes: of this process, or of the largest finished child.
#[cfg(unix)]
fn peak_rss(who: Who) -> Option<u64> {
    let who = match who {
        Who::This => libc::RUSAGE_SELF,
        Who::Children => libc::RUSAGE_CHILDREN,
    };
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage fills the struct it is given; it is only read on success.
    let usage = unsafe {
        if libc::getrusage(who, usage.as_mut_ptr()) != 0 {
            return None;
        }
        usage.assume_init()
    };
    let max = u64::try_from(usage.ru_maxrss).ok()?;
    // macOS reports bytes, Linux and the BSDs kilobytes.
    Some(if cfg!(target_os = "macos") { max } else { max * 1024 })
}

#[cfg(not(unix))]
fn peak_rss(_: Who) -> Option<u64> {
    None
}
