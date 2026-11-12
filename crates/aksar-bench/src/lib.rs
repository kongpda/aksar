//! Khmer OCR benchmark (B0 in `docs/OCR_PLAN.md`): runs engines over line sets
//! and scores them.
//!
//! A line set is a directory of line images with their text, either
//! - a `labels.tsv` (`file<TAB>text[<TAB>font]`), the format `aksar-render`'s
//!   `make_lines` writes, or
//! - `<stem>.png` + `<stem>.gt.txt` pairs, the format `research/khmer/ekyc/convert.py`
//!   writes (used when there is no `labels.tsv`).
//!
//! Scoring, after [`aksar::syllable::normalize`] on both sides:
//! - CER: edit distance on code points with whitespace removed, divided by the
//!   truth's length. Same text preparation as `research/khmer/tesseract_bench.py`
//!   and `error_report.py`; those count edits with Python's `difflib`, which
//!   can count more edits than the true (Levenshtein) distance used here.
//! - Cluster CER: the same on Khmer clusters ([`aksar::syllable::clusters`]).
//! - Folded CER: CER after subscript DA (្ដ) is folded to subscript TA (្ត). The two
//!   look the same in most fonts, so this shows how much of the CER is that confusion.
//! - WER: edit distance on words, where words are whitespace-separated tokens
//!   split with the bundled dictionary.
//!
//! Rates are pooled over a set: total edits / total truth length. Each row also
//! counts the lines whose reading fails [`aksar::syllable::validate`] (the
//! validity gate of OCR_PLAN §8 wants 0 for aksar), and [`json`] gives the rows
//! in a machine-readable form for `training/package_model.py`.

use aksar::dict::Dictionary;
use aksar::syllable::{clusters, normalize, validate};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Edit (Levenshtein) distance between two sequences.
pub fn edit_distance<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let next = (diag + usize::from(x != y)).min(row[j] + 1).min(row[j + 1] + 1);
            diag = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// Normalized text with whitespace removed: what CER is computed on.
pub fn char_text(text: &str) -> String {
    normalize(text).chars().filter(|c| !c.is_whitespace()).collect()
}

/// [`char_text`] with subscript DA (្ដ) folded to subscript TA (្ត).
pub fn folded_text(text: &str) -> String {
    char_text(text).replace("\u{17D2}\u{178A}", "\u{17D2}\u{178F}")
}

/// Normalized text split into words: whitespace tokens, each segmented with
/// the bundled dictionary.
pub fn words(text: &str) -> Vec<String> {
    let dict = Dictionary::bundled();
    let text = normalize(text);
    text.split_whitespace().flat_map(|t| dict.segment(t)).map(str::to_string).collect()
}

/// Edits and truth length, summed over lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    pub edits: usize,
    pub len: usize,
}

impl Count {
    /// Error rate in percent (0 when the truth is empty and nothing was read).
    pub fn rate(&self) -> f64 {
        if self.len == 0 {
            if self.edits == 0 { 0.0 } else { 100.0 }
        } else {
            100.0 * self.edits as f64 / self.len as f64
        }
    }

    fn add(&mut self, edits: usize, len: usize) {
        self.edits += edits;
        self.len += len;
    }
}

/// Character, cluster and word counts for a set of lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    pub chars: Count,
    pub clusters: Count,
    pub folded: Count,
    pub words: Count,
}

impl Score {
    /// Scores one line: `truth` against what the engine read.
    pub fn add(&mut self, truth: &str, read: &str) {
        let (t, r) = (char_text(truth), char_text(read));
        let (tc, rc): (Vec<char>, Vec<char>) = (t.chars().collect(), r.chars().collect());
        self.chars.add(edit_distance(&tc, &rc), tc.len());
        let (tk, rk) = (clusters(&t), clusters(&r));
        self.clusters.add(edit_distance(&tk, &rk), tk.len());
        let (tf, rf): (Vec<char>, Vec<char>) =
            (folded_text(truth).chars().collect(), folded_text(read).chars().collect());
        self.folded.add(edit_distance(&tf, &rf), tf.len());
        let (tw, rw) = (words(truth), words(read));
        self.words.add(edit_distance(&tw, &rw), tw.len());
    }
}

/// One labelled line image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub image: PathBuf,
    pub truth: String,
}

/// Loads a line set: `<dir>/labels.tsv` (`file<TAB>text[<TAB>...]`), or else every
/// `<stem>.png` in `<dir>` that has a `<stem>.gt.txt`, sorted by file name.
pub fn load_set(dir: &Path) -> Result<Vec<Line>, String> {
    let labels = dir.join("labels.tsv");
    if !labels.exists() {
        return load_gt_set(dir);
    }
    let tsv = std::fs::read_to_string(&labels).map_err(|e| format!("{}: {e}", labels.display()))?;
    let mut lines = Vec::new();
    for (n, row) in tsv.lines().enumerate() {
        if row.trim().is_empty() {
            continue;
        }
        let mut cols = row.split('\t');
        let (Some(file), Some(text)) = (cols.next(), cols.next()) else {
            return Err(format!("{}:{}: expected file<TAB>text", labels.display(), n + 1));
        };
        lines.push(Line { image: dir.join(file), truth: text.to_string() });
    }
    if lines.is_empty() {
        return Err(format!("{}: no lines", labels.display()));
    }
    Ok(lines)
}

fn load_gt_set(dir: &Path) -> Result<Vec<Line>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut images: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.extension().is_some_and(|x| x == "png") {
            images.push(path);
        }
    }
    images.sort();
    let mut lines = Vec::new();
    for image in images {
        let gt = image.with_extension("gt.txt");
        if !gt.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&gt).map_err(|e| format!("{}: {e}", gt.display()))?;
        lines.push(Line { image, truth: text.trim_end_matches(['\n', '\r']).to_string() });
    }
    if lines.is_empty() {
        return Err(format!("{}: no labels.tsv and no <stem>.png + <stem>.gt.txt pairs", dir.display()));
    }
    Ok(lines)
}

/// An OCR engine under test.
pub enum Engine {
    /// aksar-ocr reading each line crop (`Ocr::read_line`).
    Aksar(Box<aksar_ocr::Ocr>),
    /// `tesseract <img> - -l <lang> --psm 7`, optionally with `--tessdata-dir`.
    Tesseract { lang: String, tessdata: Option<PathBuf> },
}

impl Engine {
    pub fn name(&self) -> String {
        match self {
            Engine::Aksar(_) => "aksar-ocr".into(),
            Engine::Tesseract { lang, .. } => format!("tesseract {lang}"),
        }
    }

    /// Reads one line image. Any engine failure is an error: a failed read
    /// must not be scored as empty text.
    pub fn read(&self, image: &Path) -> Result<String, String> {
        match self {
            Engine::Aksar(ocr) => {
                let bytes = std::fs::read(image).map_err(|e| format!("{}: {e}", image.display()))?;
                let gray = aksar_ocr::decode_png(&bytes).map_err(|e| format!("{}: {e}", image.display()))?;
                let page = ocr.read_line(&gray).map_err(|e| format!("{}: {e}", image.display()))?;
                Ok(page.lines.into_iter().map(|l| l.recognized).collect::<Vec<_>>().join("\n"))
            }
            Engine::Tesseract { lang, tessdata } => {
                let mut cmd = Command::new("tesseract");
                cmd.arg(image).arg("-");
                if let Some(dir) = tessdata {
                    cmd.arg("--tessdata-dir").arg(dir);
                }
                cmd.args(["-l", lang, "--psm", "7"]).env("OMP_THREAD_LIMIT", "1");
                let out = cmd.output().map_err(|e| format!("tesseract: {e}"))?;
                if !out.status.success() {
                    return Err(format!(
                        "tesseract failed on {} ({}): {}",
                        image.display(),
                        out.status,
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
                String::from_utf8(out.stdout).map_err(|e| format!("tesseract output: {e}"))
            }
        }
    }
}

/// True when a `tesseract` binary runs.
pub fn tesseract_installed() -> bool {
    Command::new("tesseract").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// One table row: an engine over a set.
#[derive(Clone, Debug)]
pub struct Row {
    pub set: String,
    pub engine: String,
    pub lines: usize,
    pub score: Score,
    pub time: Duration,
    /// Lines the engine failed to read. They are left out of the score (a
    /// failed read is not an empty reading) and make the run exit non-zero.
    pub failed: usize,
    /// The first failure's message, for the report.
    pub first_error: Option<String>,
    /// Lines read whose text fails [`aksar::syllable::validate`] (broken clusters).
    pub invalid: usize,
}

/// Runs `engine` over every line of a set. A failed read is counted and
/// skipped, so one bad line doesn't lose the rows already measured; the caller
/// reports [`Row::failed`] and fails the run.
pub fn run(set: &str, lines: &[Line], engine: &Engine) -> Row {
    let mut score = Score::default();
    let mut time = Duration::ZERO;
    let (mut failed, mut first_error, mut invalid) = (0, None, 0);
    for line in lines {
        let t = Instant::now();
        match engine.read(&line.image) {
            Ok(read) => {
                time += t.elapsed();
                score.add(&line.truth, &read);
                invalid += usize::from(validate(&read).is_err());
            }
            Err(e) => {
                failed += 1;
                first_error.get_or_insert(e);
            }
        }
    }
    let read = lines.len() - failed;
    Row {
        set: set.to_string(),
        engine: engine.name(),
        lines: read,
        score,
        time,
        failed,
        first_error,
        invalid,
    }
}

/// The markdown results table.
pub fn table(rows: &[Row]) -> String {
    let mut s = String::from(
        "| Set | Engine | Lines | Failed | Invalid | CER | Cluster CER | CER ្ដ=្ត | WER | ms/line |\n|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        let ms = r.time.as_secs_f64() * 1000.0 / r.lines.max(1) as f64;
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.2}% | {:.2}% | {:.2}% | {:.2}% | {ms:.1} |\n",
            r.set,
            r.engine,
            r.lines,
            r.failed,
            r.invalid,
            r.score.chars.rate(),
            r.score.clusters.rate(),
            r.score.folded.rate(),
            r.score.words.rate(),
        ));
    }
    s
}

/// Facts about a run that are not per row, for [`json`].
#[derive(Clone, Debug, Default)]
pub struct RunInfo {
    /// Size of the aksar-ocr model file in bytes, when one ran.
    pub model_bytes: Option<usize>,
    /// Peak resident memory of this process (aksar-ocr), in bytes.
    pub peak_bytes: Option<u64>,
    /// Peak resident memory of the largest child process (Tesseract), in bytes.
    pub peak_child_bytes: Option<u64>,
}

/// The rows as JSON (`"schema": 1`): per row the set, engine, line counts and
/// rates in percent, as in [`table`]. Read by `training/package_model.py`.
pub fn json(rows: &[Row], info: &RunInfo) -> String {
    fn esc(out: &mut String, s: &str) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    let opt = |v: Option<u64>| v.map_or_else(|| "null".to_string(), |n| n.to_string());
    let mut j = format!(
        "{{\"schema\":1,\"version\":\"{}\",\"model_bytes\":{},\"peak_bytes\":{},\"peak_child_bytes\":{},\"rows\":[",
        env!("CARGO_PKG_VERSION"),
        opt(info.model_bytes.map(|n| n as u64)),
        opt(info.peak_bytes),
        opt(info.peak_child_bytes),
    );
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push_str("{\"set\":");
        esc(&mut j, &r.set);
        j.push_str(",\"engine\":");
        esc(&mut j, &r.engine);
        let ms = r.time.as_secs_f64() * 1000.0 / r.lines.max(1) as f64;
        j.push_str(&format!(
            ",\"lines\":{},\"failed\":{},\"invalid\":{},\"cer\":{:.4},\"cluster_cer\":{:.4},\
             \"folded_cer\":{:.4},\"wer\":{:.4},\"ms_per_line\":{ms:.3}}}",
            r.lines,
            r.failed,
            r.invalid,
            r.score.chars.rate(),
            r.score.clusters.rate(),
            r.score.folded.rate(),
            r.score.words.rate(),
        ));
    }
    j.push_str("]}");
    j
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_hand_checked() {
        let d =
            |a: &str, b: &str| edit_distance(&a.chars().collect::<Vec<_>>(), &b.chars().collect::<Vec<_>>());
        assert_eq!(d("", ""), 0);
        assert_eq!(d("abc", ""), 3); // 3 deletions
        assert_eq!(d("", "ab"), 2); // 2 insertions
        assert_eq!(d("kitten", "sitting"), 3); // k->s, e->i, +g
        assert_eq!(d("ab", "ba"), 2); // no transpositions in Levenshtein
    }

    #[test]
    fn cer_ignores_whitespace_and_counts_code_points() {
        let mut s = Score::default();
        // កម្ពុជា is 7 code points; dropping ុ is one edit.
        s.add("កម្ពុ ជា", "កម្ពជា");
        assert_eq!(s.chars, Count { edits: 1, len: 7 });
        // Clusters ក | ម្ពុ | ជា vs ក | ម្ព | ជា: one cluster wrong of 3.
        assert_eq!(s.clusters, Count { edits: 1, len: 3 });
    }

    #[test]
    fn folded_cer_forgives_only_the_ta_da_confusion() {
        // ស្ដី read as ស្តី: one code point wrong, but the same after folding.
        let mut s = Score::default();
        s.add("ស្ដី", "ស្តី");
        assert_eq!(s.chars.edits, 1);
        assert_eq!(s.folded, Count { edits: 0, len: 4 });
        // Any other error still counts after folding.
        let mut s = Score::default();
        s.add("ស្ដី", "ស្ដ");
        assert_eq!(s.folded.edits, 1);
    }

    #[test]
    fn scoring_normalizes_first() {
        // កា្រ and ក្រា are the same text in a different typing order:
        // without normalize this would count edits.
        let mut s = Score::default();
        s.add("ក្រា", "កា្រ");
        assert_eq!(s.chars.edits, 0);
        assert_eq!(s.clusters.edits, 0);
        assert_eq!(s.words.edits, 0);
    }

    #[test]
    fn wer_uses_dictionary_words() {
        // No space between the words: the dictionary still splits them,
        // so a dropped word is 1 edit of 2, not a whole-line error.
        assert_eq!(words("ព្រះរាជាណាចក្រកម្ពុជា"), ["ព្រះរាជាណាចក្រ", "កម្ពុជា"]);
        let mut s = Score::default();
        s.add("ព្រះរាជាណាចក្រកម្ពុជា", "ព្រះរាជាណាចក្រ");
        assert_eq!(s.words, Count { edits: 1, len: 2 });
        assert_eq!(s.words.rate(), 50.0);
        // A misread word is not in the dictionary, so it falls apart into
        // pieces: each piece counts, as in any WER over segmented text.
        let mut s = Score::default();
        s.add("កម្ពុជា", "កម្ពជា");
        assert_eq!(s.words.len, 1);
        assert!(s.words.edits >= 1);
    }

    #[test]
    fn rates_pool_over_lines() {
        // 1 edit in 2 chars + 0 in 8 = 1/10, not the mean of 50% and 0%.
        let mut s = Score::default();
        s.add("ab", "a");
        s.add("abcdefgh", "abcdefgh");
        assert_eq!(s.chars, Count { edits: 1, len: 10 });
        assert_eq!(s.chars.rate(), 10.0);
    }

    #[test]
    fn json_has_every_row_and_escapes_names() {
        let mut score = Score::default();
        score.add("ab", "a");
        let row = Row {
            set: "s\"1".into(),
            engine: "aksar-ocr".into(),
            lines: 1,
            score,
            time: Duration::from_millis(3),
            failed: 0,
            first_error: None,
            invalid: 1,
        };
        let info = RunInfo { model_bytes: Some(10), ..RunInfo::default() };
        let j = json(&[row.clone(), row], &info);
        assert!(j.starts_with("{\"schema\":1,"), "{j}");
        assert!(j.contains("\"model_bytes\":10,\"peak_bytes\":null"), "{j}");
        assert!(j.contains("\"set\":\"s\\\"1\""), "{j}");
        assert!(j.contains("\"invalid\":1,\"cer\":50.0000"), "{j}");
        assert_eq!(j.matches("\"engine\"").count(), 2);
        assert!(j.ends_with("]}"));
    }

    #[test]
    fn empty_truth_with_output_is_full_error() {
        assert_eq!(Count { edits: 0, len: 0 }.rate(), 0.0);
        assert_eq!(Count { edits: 3, len: 0 }.rate(), 100.0);
    }
}
