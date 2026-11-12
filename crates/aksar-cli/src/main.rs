//! `aksar` command-line tool.
//!
//! ```text
//! aksar shape --font NotoSansKhmer-Regular.ttf [--size 32] "សួស្តី"
//! aksar check "កា្រ"
//! aksar normalize "កា្រ"
//! aksar spell "ទីក្រុងភ្នំពេញ កម្ពជា"
//! aksar words "ព្រះរាជាណាចក្រកម្ពុជា"
//! aksar read letter.txt            # or: echo "..." | aksar read -   (--json, --no-fix)
//! aksar ocr --model-dir models/print-v1 page.png       # feature `ocr` (--json, --no-fix)
//! ```

use std::io::{BufWriter, Write};
use std::process::ExitCode;

use aksar::dict::{Check, Dictionary};
use aksar::read::write_json_str;
use aksar::shape::{Font, ShapeOptions, shape};
use aksar::syllable::{Issue, issues, normalize};

const USAGE: &str = "usage: aksar <command> [options] <input>

commands:
  shape      shape text with a font and print the glyphs
  check      report broken clusters and likely mistakes
  normalize  print text in canonical NiDA order
  spell      check each space-separated token against the Khmer dictionary
  words      split Khmer text into dictionary words
  read       read a text file: repaired, checked, corrected text
  ocr        read a PNG page or photo with a line model
             (needs a build with the `ocr` feature:
             cargo install --path crates/aksar-cli --features ocr)

`aksar <command> --help` shows that command's options and an example.

input:
  check, normalize, spell and words take the text as one quoted argument, or `-`
  to read it from standard input. `read` takes a file name or `-`. On file and
  stdin input a leading UTF-8 byte-order mark and one trailing newline are ignored.

options (before or after the input):
  --json         machine-readable output (check, normalize, spell, words, read, ocr)
  --             ends the options: everything after it is text or a file name
  -h, --help     show this help (or a command's help)
  -V, --version  print the version

exit status:
  shape      0 ok, 2 error
  check      0 ok (warnings only), 1 broken clusters found, 2 error
  normalize  0 ok, 2 error
  spell      0 every token known or fixable, 1 suspect tokens, 2 error
  words      0 ok, 2 error
  read, ocr  0 ok, 1 words to review (listed on stderr), 2 error
  A closed output pipe (`aksar read big.txt | head -1`) is a quiet exit 0.";

const HELP_SHAPE: &str = "usage: aksar shape --font <file> [--size <px>] [--feature <f>]... <text>

Shape text with a font and print one line per glyph (glyph id, cluster byte
offset, advance, offsets; at most 3 decimals), then the total width, missing
glyphs and dotted circles.

  --font <file>    TrueType/OpenType font file (required)
  --size <px>      font size in pixels (default: font units)
  --feature <f>    OpenType feature such as `kern` or `-liga`; repeatable

example: aksar shape --font NotoSansKhmer-Regular.ttf --size 32 \"សួស្តី\"
exit status: 0 ok, 2 error (empty text, unreadable font, bad option)";

const HELP_CHECK: &str = "usage: aksar check [--json] <text|->

Report broken clusters (errors, shown with a dotted circle) and likely mistakes
(warnings), then offer the normalized text when it fixes everything. Offsets in
the text output are characters (Unicode code points), counted from 0.

  --json   one JSON object: {\"ok\", \"issues\": [{\"start\", \"end\" (UTF-8 byte
           offsets), \"kind\", \"error\", \"text\", \"message\", \"char_start\",
           \"char_end\" (code point offsets)}], \"normalized\"}

example: aksar check \"កា្រ\"
         echo \"កា្រ\" | aksar check - --json
exit status: 0 ok (warnings only), 1 broken clusters found, 2 error";

const HELP_NORMALIZE: &str = "usage: aksar normalize [--json] <text|->

Print the text in canonical NiDA storage order.

  --json   {\"text\": \"...\"}

example: aksar normalize \"កា្រ\"
exit status: 0 ok, 2 error";

const HELP_SPELL: &str = "usage: aksar spell [--json] <text|->

Check each space-separated token against the bundled Khmer dictionary. Text
output has one tab-separated line per token: `ok`, `fix` (with the correction) or
`check` (suspect).

  --json   {\"ok\", \"tokens\": [{\"token\", \"status\": known|corrected|suspect,
           \"fix\": string|null}]}

example: aksar spell \"ទីក្រុងភ្នំពេញ កម្ពជា\"
exit status: 0 every token known or fixable, 1 suspect tokens, 2 error";

const HELP_WORDS: &str = "usage: aksar words [--json] <text|->

Split Khmer text into dictionary words, shown separated by ` | `.

  --json   {\"words\": [\"...\"]}

example: aksar words \"ព្រះរាជាណាចក្រកម្ពុជា\"
exit status: 0 ok, 2 error";

const HELP_READ: &str = "usage: aksar read [--json] [--no-fix] <file|->

Read a text file (or stdin): repaired, checked, corrected text on stdout; words
to review go to stderr as `check<TAB>word<TAB>-> suggestions`.

  --json     print the whole document as JSON instead of text
  --no-fix   only suggest corrections; do not apply them

example: aksar read letter.txt
         echo \"ទីក្រុងភ្នំពេញ កម្ពជា\" | aksar read - --json
exit status: 0 ok, 1 words to review, 2 error";

const HELP_OCR: &str =
    "usage: aksar ocr [--model-dir <dir> | --model <onnx> --alphabet <json>] [--json] [--no-fix] <png>

Read a PNG page or photo with a line model, then check it like `read`. Needs a
build with the `ocr` feature: cargo install --path crates/aksar-cli --features ocr
No model is released yet; training/README.md shows how to train one.

  --model-dir <dir>   folder with model.onnx and alphabet.json (default: the
                      AKSAR_MODEL_DIR environment variable)
  --model <onnx>      line recognition model file (with --alphabet)
  --alphabet <json>   its alphabet file
  --json              print the page as JSON
  --no-fix            only suggest corrections; do not apply them

example: aksar ocr --model-dir models/print-v1 page.png
exit status: 0 ok, 1 words to review, 2 error";

/// Every command, for "did you mean" hints.
const COMMANDS: [&str; 7] = ["shape", "check", "normalize", "spell", "words", "read", "ocr"];

fn help_for(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "shape" => HELP_SHAPE,
        "check" => HELP_CHECK,
        "normalize" => HELP_NORMALIZE,
        "spell" => HELP_SPELL,
        "words" => HELP_WORDS,
        "read" => HELP_READ,
        "ocr" => HELP_OCR,
        _ => return None,
    })
}

/// Why a command failed: bad arguments (usage is shown), a runtime error, or
/// a closed output pipe (not an error: the reader has what it wanted).
enum Fail {
    Usage(String),
    Run(String),
    Closed,
}

impl From<&str> for Fail {
    fn from(msg: &str) -> Self {
        Fail::Usage(msg.to_string())
    }
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            Fail::Closed
        } else {
            Fail::Run(format!("cannot write output: {e}"))
        }
    }
}

/// Levenshtein distance in characters; inputs here are short command and option names.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// `"; did you mean X?"` for the closest of `known` within two edits, else `""`.
fn did_you_mean<'a>(given: &str, known: impl IntoIterator<Item = &'a str>) -> String {
    known
        .into_iter()
        .map(|k| (edit_distance(given, k), k))
        .filter(|&(d, _)| d <= 2)
        .min_by_key(|&(d, _)| d)
        .map(|(_, k)| format!("; did you mean {k:?}?"))
        .unwrap_or_default()
}

fn unknown_command(cmd: &str) -> Fail {
    Fail::Usage(format!("unknown command {cmd:?}{}", did_you_mean(cmd, COMMANDS)))
}

/// An option-like argument (`-x`, `--x`); `-` alone means stdin.
fn is_option(a: &str) -> bool {
    a.starts_with('-') && a.len() > 1
}

/// Parsed command line: the flags a command allows, in any position, and the
/// remaining positional arguments.
struct Parsed<'a> {
    on: Vec<&'static str>,
    values: Vec<(&'static str, &'a str)>,
    pos: Vec<&'a str>,
}

impl<'a> Parsed<'a> {
    fn has(&self, flag: &str) -> bool {
        self.on.contains(&flag)
    }
    /// The last value given for a value flag.
    fn value(&self, flag: &str) -> Option<&'a str> {
        self.values.iter().rev().find(|(f, _)| *f == flag).map(|(_, v)| *v)
    }
    fn all(&self, flag: &'static str) -> impl Iterator<Item = &'a str> + '_ {
        self.values.iter().filter(move |(f, _)| *f == flag).map(|(_, v)| *v)
    }
}

/// Splits `args` into flags and positionals. `flags` lists the options the
/// command allows as (name, takes a value); `--` ends the options.
fn parse<'a>(args: &'a [String], flags: &[(&'static str, bool)]) -> Result<Parsed<'a>, Fail> {
    let mut p = Parsed { on: Vec::new(), values: Vec::new(), pos: Vec::new() };
    let mut it = args.iter();
    let mut opts = true;
    while let Some(a) = it.next() {
        if !opts || !is_option(a) {
            p.pos.push(a);
        } else if a == "--" {
            opts = false;
        } else if let Some(&(name, takes_value)) = flags.iter().find(|(n, _)| *n == a) {
            if takes_value {
                let v = it.next().ok_or_else(|| Fail::Usage(format!("{name} needs a value")))?;
                p.values.push((name, v));
            } else {
                p.on.push(name);
            }
        } else {
            // Suggest among this command's options and the global ones.
            let known = flags.iter().map(|(n, _)| *n).chain(["--help", "--version"]);
            return Err(Fail::Usage(format!("unknown option {a:?}{}", did_you_mean(a, known))));
        }
    }
    Ok(p)
}

/// Drops a leading UTF-8 byte-order mark (editors on Windows add one) and one
/// trailing newline, which belongs to the file, not to the text: without this
/// `read` reports an extra empty line.
fn clean_input(mut s: String) -> String {
    if s.starts_with('\u{FEFF}') {
        s.remove(0);
    }
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    s
}

fn read_stdin() -> Result<String, Fail> {
    let mut s = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
        .map_err(|e| Fail::Run(format!("stdin: {e}")))?;
    Ok(clean_input(s))
}

/// Contents of a text file, or of stdin for `-`.
fn read_input(path: &str) -> Result<String, Fail> {
    match path {
        "-" => read_stdin(),
        "" => Err(Fail::Run("empty file name".to_string())),
        _ => std::fs::read_to_string(path).map(clean_input).map_err(|e| Fail::Run(format!("{path}: {e}"))),
    }
}

/// The single text argument of a text command; `-` reads stdin.
fn text_arg(p: &Parsed) -> Result<String, Fail> {
    match p.pos.as_slice() {
        ["-"] => read_stdin(),
        [text] => Ok(text.to_string()),
        [] => Err("missing text (use - for stdin)".into()),
        _ => Err("too many arguments (quote the text: \"...\")".into()),
    }
}

/// Writes a line to stderr; a closed stderr is not worth failing for.
fn warn(msg: &str) {
    let _ = writeln!(std::io::stderr(), "{msg}");
}

/// Each issue's UTF-8 byte range as a range of characters (code points).
///
/// One pass over `text` for all issues: counting from the start for every
/// issue took minutes on large inputs full of issues.
fn char_ranges(text: &str, found: &[Issue]) -> Vec<(usize, usize)> {
    let mut at: Vec<usize> = found.iter().flat_map(|i| [i.range.start, i.range.end]).collect();
    at.sort_unstable();
    at.dedup();
    let mut chars = Vec::with_capacity(at.len());
    let mut want = at.iter().peekable();
    for (n, (b, _)) in text.char_indices().chain([(text.len(), ' ')]).enumerate() {
        while want.next_if(|&&w| w <= b).is_some() {
            chars.push(n);
        }
    }
    let char_at = |b: usize| chars[at.binary_search(&b).expect("offset collected above")];
    found.iter().map(|i| (char_at(i.range.start), char_at(i.range.end))).collect()
}

/// `check --json`: issues with UTF-8 byte ranges (`start`, `end`) and character
/// ranges (`char_start`, `char_end`), a stable `kind` name, the message, and
/// the normalized text when normalizing fixes everything.
fn check_json(text: &str, found: &[Issue]) -> String {
    let mut j = format!("{{\"ok\":{},\"issues\":[", !found.iter().any(|i| i.kind.is_error()));
    for (n, (i, (cs, ce))) in found.iter().zip(char_ranges(text, found)).enumerate() {
        let kind = i.kind.name();
        if n > 0 {
            j.push(',');
        }
        j.push_str(&format!(
            "{{\"start\":{},\"end\":{},\"kind\":\"{kind}\",\"error\":{},\"text\":",
            i.range.start,
            i.range.end,
            i.kind.is_error()
        ));
        write_json_str(&mut j, &text[i.range.clone()]);
        j.push_str(",\"message\":");
        write_json_str(&mut j, &i.to_string());
        j.push_str(&format!(",\"char_start\":{cs},\"char_end\":{ce}}}"));
    }
    j.push_str("],\"normalized\":");
    let fixed = normalize(text);
    if fixed != text && issues(&fixed).is_empty() {
        write_json_str(&mut j, &fixed);
    } else {
        j.push_str("null");
    }
    j.push('}');
    j
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // No command at all: show what there is rather than a one-line error.
    if args.is_empty() {
        warn(USAGE);
        return ExitCode::from(2);
    }
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let result = run(&args, &mut out).and_then(|code| out.flush().map(|()| code).map_err(Fail::from));
    match result {
        Ok(code) => ExitCode::from(code),
        // The reader went away (`| head -1`): it has what it wanted.
        Err(Fail::Closed) => ExitCode::SUCCESS,
        Err(Fail::Usage(msg)) => {
            // Point at the help of the command that was used, when there is one.
            let help = match args.first().map(String::as_str) {
                Some(c) if help_for(c).is_some() => format!("aksar {c} --help"),
                _ => "aksar --help".to_string(),
            };
            warn(&format!("error: {msg}\n(run `{help}` for usage)"));
            ExitCode::from(2)
        }
        Err(Fail::Run(msg)) => {
            warn(&format!("error: {msg}"));
            ExitCode::from(2)
        }
    }
}

/// Runs one command line, writing its output to `out`; returns the exit status.
fn run(args: &[String], out: &mut dyn Write) -> Result<u8, Fail> {
    let (cmd, rest) = args.split_first().ok_or("missing command")?;
    let version = || format!("aksar {}", env!("CARGO_PKG_VERSION"));
    match cmd.as_str() {
        "-h" | "--help" => {
            writeln!(out, "{USAGE}")?;
            return Ok(0);
        }
        "help" => {
            let text = match rest.first() {
                None => USAGE,
                Some(c) => help_for(c).ok_or_else(|| unknown_command(c))?,
            };
            writeln!(out, "{text}")?;
            return Ok(0);
        }
        "-V" | "--version" | "version" => {
            writeln!(out, "{}", version())?;
            return Ok(0);
        }
        _ => {}
    }
    let help = help_for(cmd).ok_or_else(|| unknown_command(cmd))?;
    // --help and --version work in any position, except after `--`.
    let before_dashdash: Vec<&String> = rest.iter().take_while(|a| *a != "--").collect();
    if before_dashdash.iter().any(|a| *a == "-h" || *a == "--help") {
        writeln!(out, "{help}")?;
        return Ok(0);
    }
    if before_dashdash.iter().any(|a| *a == "-V" || *a == "--version") {
        writeln!(out, "{}", version())?;
        return Ok(0);
    }
    match cmd.as_str() {
        "shape" => cmd_shape(rest, out),
        "check" => cmd_check(rest, out),
        "normalize" => {
            let p = parse(rest, &[("--json", false)])?;
            let fixed = normalize(&text_arg(&p)?);
            if p.has("--json") {
                let mut j = String::from("{\"text\":");
                write_json_str(&mut j, &fixed);
                writeln!(out, "{j}}}")?;
            } else {
                writeln!(out, "{fixed}")?;
            }
            Ok(0)
        }
        "spell" => cmd_spell(rest, out),
        "words" => {
            let p = parse(rest, &[("--json", false)])?;
            let text = text_arg(&p)?;
            let words = Dictionary::bundled().segment(&text);
            if p.has("--json") {
                let mut j = String::from("{\"words\":[");
                for (n, w) in words.iter().enumerate() {
                    if n > 0 {
                        j.push(',');
                    }
                    write_json_str(&mut j, w);
                }
                writeln!(out, "{j}]}}")?;
            } else {
                writeln!(out, "{}", words.join(" | "))?;
            }
            Ok(0)
        }
        "read" => cmd_read(rest, out),
        "ocr" => cmd_ocr(rest, out),
        _ => unreachable!("help_for knows exactly the commands"),
    }
}

fn cmd_check(args: &[String], out: &mut dyn Write) -> Result<u8, Fail> {
    let p = parse(args, &[("--json", false)])?;
    let text = text_arg(&p)?;
    let found = issues(&text);
    let code = u8::from(found.iter().any(|i| i.kind.is_error()));
    if p.has("--json") {
        writeln!(out, "{}", check_json(&text, &found))?;
        return Ok(code);
    }
    for (i, (a, b)) in found.iter().zip(char_ranges(&text, &found)) {
        let level = if i.kind.is_error() { "error" } else { "warning" };
        writeln!(out, "{level} at chars {a}..{b}: {} (\"{}\")", i.kind, &text[i.range.clone()])?;
    }
    if found.is_empty() {
        writeln!(out, "ok")?;
    }
    // Offer the fix when `normalize` gives clean text.
    let fixed = normalize(&text);
    if fixed != text && issues(&fixed).is_empty() {
        writeln!(out, "normalized: {fixed}")?;
    }
    Ok(code)
}

fn cmd_spell(args: &[String], out: &mut dyn Write) -> Result<u8, Fail> {
    let p = parse(args, &[("--json", false)])?;
    let text = text_arg(&p)?;
    let dict = Dictionary::bundled();
    let json = p.has("--json");
    let mut suspect = false;
    let mut tokens = String::new();
    for token in text.split_whitespace() {
        // (status, fix); suspect also covers any outcome added later.
        let (status, fix) = match dict.check(token) {
            Check::Known => ("known", None),
            Check::Corrected(fixed) => ("corrected", Some(fixed)),
            _ => {
                suspect = true;
                ("suspect", None)
            }
        };
        if json {
            if !tokens.is_empty() {
                tokens.push(',');
            }
            tokens.push_str("{\"token\":");
            write_json_str(&mut tokens, token);
            tokens.push_str(&format!(",\"status\":\"{status}\",\"fix\":"));
            match &fix {
                Some(f) => write_json_str(&mut tokens, f),
                None => tokens.push_str("null"),
            }
            tokens.push('}');
        } else {
            match (status, &fix) {
                ("known", _) => writeln!(out, "ok\t{token}")?,
                (_, Some(f)) => writeln!(out, "fix\t{token}\t-> {f}")?,
                _ => writeln!(out, "check\t{token}")?,
            }
        }
    }
    if json {
        writeln!(out, "{{\"ok\":{},\"tokens\":[{tokens}]}}", !suspect)?;
    }
    Ok(u8::from(suspect))
}

fn cmd_read(args: &[String], out: &mut dyn Write) -> Result<u8, Fail> {
    let p = parse(args, &[("--json", false), ("--no-fix", false)])?;
    let path = match p.pos.as_slice() {
        [path] => *path,
        [] => return Err("missing file (use - for stdin)".into()),
        _ => return Err("too many arguments: give one file".into()),
    };
    let text = read_input(path)?;
    let doc = aksar::read::Aksar::new().autocorrect(!p.has("--no-fix")).read(&text);
    report(&doc, p.has("--json").then(|| doc.to_json()), out)
}

/// Prints a checked document (as `json` when given, else as text) and its
/// words to review on stderr. Exit 1 when any word needs review.
fn report(doc: &aksar::read::Document, json: Option<String>, out: &mut dyn Write) -> Result<u8, Fail> {
    if let Some(json) = json {
        writeln!(out, "{json}")?;
    } else {
        let text = doc.text();
        write!(out, "{text}")?;
        if !text.ends_with('\n') {
            writeln!(out)?;
        }
    }
    // Hand the output over before the review list: a closed pipe ends here.
    out.flush()?;
    let review: Vec<&aksar::read::Word> = doc.words().filter(|w| w.needs_review()).collect();
    for w in &review {
        let hint = if w.suggestions.is_empty() {
            String::new()
        } else {
            format!("\t-> {}", w.suggestions.join(", "))
        };
        warn(&format!("check\t{}{hint}", w.text));
    }
    Ok(u8::from(!review.is_empty()))
}

#[cfg(not(feature = "ocr"))]
fn cmd_ocr(_: &[String], _: &mut dyn Write) -> Result<u8, Fail> {
    Err(Fail::Run(
        "this aksar was built without OCR; rebuild with: cargo install --path crates/aksar-cli --features ocr"
            .to_string(),
    ))
}

/// Where `ocr` loads its model from.
#[cfg(feature = "ocr")]
enum ModelSource<'a> {
    /// A folder with model.onnx and alphabet.json.
    Dir(std::borrow::Cow<'a, str>),
    /// The two files named one by one.
    Files { model: &'a str, alphabet: &'a str },
}

/// `ocr` arguments.
#[cfg(feature = "ocr")]
struct OcrArgs<'a> {
    source: ModelSource<'a>,
    image: &'a str,
    json: bool,
    fix: bool,
}

/// Environment variable naming the default model folder for `ocr`.
#[cfg(feature = "ocr")]
const MODEL_DIR_ENV: &str = "AKSAR_MODEL_DIR";

#[cfg(feature = "ocr")]
fn ocr_args<'a>(args: &'a [String], env_dir: Option<String>) -> Result<OcrArgs<'a>, Fail> {
    let p = parse(
        args,
        &[
            ("--model-dir", true),
            ("--model", true),
            ("--alphabet", true),
            ("--json", false),
            ("--no-fix", false),
        ],
    )?;
    let image = match p.pos.as_slice() {
        [image] => *image,
        [] => return Err("missing image (a PNG file)".into()),
        _ => return Err("too many arguments: give one image".into()),
    };
    let source = match (p.value("--model-dir"), p.value("--model"), p.value("--alphabet")) {
        (Some(dir), None, None) => ModelSource::Dir(dir.into()),
        (None, Some(model), Some(alphabet)) => ModelSource::Files { model, alphabet },
        (Some(_), _, _) => return Err("give --model-dir, or --model with --alphabet, not both".into()),
        (None, Some(_), None) => return Err("--model needs --alphabet (or use --model-dir)".into()),
        (None, None, Some(_)) => return Err("--alphabet needs --model (or use --model-dir)".into()),
        (None, None, None) => match env_dir.filter(|d| !d.is_empty()) {
            Some(dir) => ModelSource::Dir(dir.into()),
            None => {
                return Err(Fail::Usage(format!("no model: give --model-dir <dir> or set {MODEL_DIR_ENV}")));
            }
        },
    };
    Ok(OcrArgs { source, image, json: p.has("--json"), fix: !p.has("--no-fix") })
}

#[cfg(feature = "ocr")]
fn cmd_ocr(args: &[String], out: &mut dyn Write) -> Result<u8, Fail> {
    let a = ocr_args(args, std::env::var(MODEL_DIR_ENV).ok())?;
    // Model files get the same size caps as --model-dir.
    let read = |p: &str, cap: u64| {
        use std::io::Read;
        if p.is_empty() {
            return Err(Fail::Run("empty file name".to_string()));
        }
        let fail = |e: std::io::Error| Fail::Run(format!("{p}: {e}"));
        let mut bytes = Vec::new();
        std::fs::File::open(p)
            .map_err(fail)?
            .take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(fail)?;
        if bytes.len() as u64 > cap {
            return Err(Fail::Run(format!("{p}: more than the {cap} bytes allowed")));
        }
        Ok(bytes)
    };
    // Read the image first: a typo in its name should not wait for a model load.
    let image = read(a.image, u64::MAX)?;
    let ocr = match &a.source {
        // from_dir's errors already name the file.
        ModelSource::Dir(dir) => {
            aksar_ocr::Ocr::from_dir(dir.as_ref()).map_err(|e| Fail::Run(e.to_string()))?
        }
        ModelSource::Files { model, alphabet } => {
            let model_bytes = read(model, aksar_recog::MAX_MODEL_BYTES)?;
            let alphabet_text = String::from_utf8(read(alphabet, aksar_recog::MAX_ALPHABET_BYTES)?)
                .map_err(|_| Fail::Run(format!("{alphabet}: not valid UTF-8")))?;
            aksar_ocr::Ocr::new(model_bytes, &alphabet_text).map_err(|e| {
                // Name the file that is wrong.
                let file = match e {
                    aksar_ocr::Error::Recognizer(aksar_recog::Error::Alphabet) => alphabet,
                    _ => model,
                };
                Fail::Run(format!("{file}: {e}"))
            })?
        }
    }
    .autocorrect(a.fix);
    let page = ocr.read_png(&image).map_err(|e| Fail::Run(format!("{}: {e}", a.image)))?;
    report(&page.document, a.json.then(|| page.to_json()), out)
}

/// A pixel value for people: at most 3 decimals, no float noise (`9.216001` -> `9.216`),
/// no trailing zeros (`20.320002` -> `20.32`, `0.0` -> `0`).
fn num(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

fn cmd_shape(args: &[String], out: &mut dyn Write) -> Result<u8, Fail> {
    let p = parse(args, &[("--font", true), ("--size", true), ("--feature", true)])?;
    let mut opts = ShapeOptions::default();
    if let Some(v) = p.value("--size") {
        let size: f32 = v.parse().map_err(|_| Fail::Usage(format!("bad size {v:?}")))?;
        if !(size.is_finite() && size > 0.0) {
            return Err(Fail::Usage(format!("bad size {v:?}: must be a positive number")));
        }
        opts.size = Some(size);
    }
    opts.features = p.all("--feature").map(String::from).collect();
    let text = match p.pos.as_slice() {
        [text] => *text,
        [] => return Err("missing text".into()),
        _ => return Err("too many arguments (quote the text: \"...\")".into()),
    };
    let path = p.value("--font").ok_or("--font is required")?;
    if text.is_empty() {
        return Err("empty text: nothing to shape".into());
    }
    let bytes = std::fs::read(path).map_err(|e| {
        let what = if path.is_empty() { "empty file name".to_string() } else { format!("{path}: {e}") };
        Fail::Run(format!("cannot read the font file given to --font ({what})"))
    })?;
    let font = Font::from_bytes(bytes).map_err(|e| Fail::Run(format!("{path}: {e}")))?;
    let run = shape(text, &font, &opts).map_err(|e| match e {
        aksar::shape::Error::InvalidFeature => {
            Fail::Usage(format!("{e} in --feature {}", opts.features.join(", --feature ")))
        }
        e => Fail::Run(e.to_string()),
    })?;
    writeln!(out, "glyph\tcluster\tx_adv\tx_off\ty_off")?;
    for g in &run.glyphs {
        let (adv, xo, yo) = (num(g.x_advance), num(g.x_offset), num(g.y_offset));
        writeln!(out, "{}\t{}\t{adv}\t{xo}\t{yo}", g.glyph_id, g.cluster)?;
    }
    writeln!(
        out,
        "width {}  missing {}  dotted_circles {}",
        num(run.width),
        run.missing,
        run.dotted_circles
    )?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn check_json_reports_issues_and_the_fix() {
        // ក then a split vowel េ+ា (should be ោ): a warning, fixed by normalize.
        let j = check_json("កេា", &issues("កេា"));
        assert!(j.starts_with("{\"ok\":true,\"issues\":[{\"start\":"), "{j}");
        assert!(j.contains("\"kind\":\"split-vowel\",\"error\":false"), "{j}");
        assert!(j.contains("\"normalized\":\"កោ\""), "{j}");
        // A lone mark is an error and normalize can't fix it.
        let j = check_json("\u{17B6}", &issues("\u{17B6}"));
        assert!(
            j.contains("\"ok\":false") && j.contains("broken-cluster") && j.ends_with("\"normalized\":null}"),
            "{j}"
        );
        assert_eq!(check_json("ក", &[]), "{\"ok\":true,\"issues\":[],\"normalized\":null}");
        assert!(go(&["check", "--json", "ក"]).0.is_ok());
    }

    /// Both offset kinds are in the JSON; byte offsets stay for existing
    /// consumers, char offsets are what Python `str` and the text output use.
    #[test]
    fn check_json_has_byte_and_char_offsets() {
        // "ក x" then a lone vowel sign: bytes 5..8, but characters 3..4.
        let j = check_json("ក x\u{17B6}", &issues("ក x\u{17B6}"));
        assert!(j.contains("\"start\":5,\"end\":8"), "{j}");
        assert!(j.contains("\"char_start\":3,\"char_end\":4"), "{j}");
    }

    use super::*;

    /// One pass gives the same character offsets as counting from the start
    /// for each issue, including issues that overlap (a broken cluster and a
    /// deprecated character inside it) and non-Khmer text before them.
    #[test]
    fn char_ranges_match_counting_from_the_start() {
        let text = "🙂 \u{17B6}\u{17B4} កេា x\u{17D2} ក្ក្ក្ក\u{17B6}";
        let found = issues(text);
        assert!(found.len() >= 4, "{found:?}");
        let naive: Vec<(usize, usize)> = found
            .iter()
            .map(|i| (text[..i.range.start].chars().count(), text[..i.range.end].chars().count()))
            .collect();
        assert_eq!(char_ranges(text, &found), naive);
        assert!(char_ranges("", &[]).is_empty());
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// Runs a command line; returns its result and what it wrote to stdout.
    fn go(a: &[&str]) -> (Result<u8, Fail>, String) {
        let mut out = Vec::new();
        let r = run(&args(a), &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    /// The text of a message-carrying failure, for assertions.
    fn why(r: Result<u8, Fail>) -> String {
        match r {
            Err(Fail::Usage(m)) | Err(Fail::Run(m)) => m,
            _ => panic!("expected an error"),
        }
    }

    /// Flags work before or after the text, so `check "x" --json` is not a trap.
    #[test]
    fn flags_work_in_any_position() {
        let (a, ja) = go(&["check", "--json", "ក"]);
        let (b, jb) = go(&["check", "ក", "--json"]);
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(ja, jb);
        assert!(ja.starts_with("{\"ok\":true"));
        assert!(go(&["spell", "ក", "--json"]).1.starts_with("{\"ok\":"));
        assert!(go(&["words", "--json", "ក"]).1.starts_with("{\"words\":["));
        assert!(go(&["normalize", "ក", "--json"]).1.starts_with("{\"text\":"));
    }

    /// Readable output: no Debug escapes like \u{17d2}, and character offsets.
    #[test]
    fn check_text_is_readable_with_char_offsets() {
        // A lone coeng after a base: the issue text is a mark Debug would escape.
        let (_, o) = go(&["check", "ក\u{17D2}"]);
        assert!(!o.contains("\\u{"), "no Debug escapes: {o}");
        assert!(o.contains("(\"\u{17D2}\")") || o.contains("\u{17D2}"), "{o}");
        // Offsets count characters (2..3), not UTF-8 bytes (4..7).
        let (_, o) = go(&["check", "ក x\u{17B6}"]);
        assert!(o.contains("chars 3..4"), "{o}");
        assert!(!o.contains("5..8"), "byte offsets leaked: {o}");
    }

    #[test]
    fn spell_json_shape_and_exit_code() {
        let (r, o) = go(&["spell", "--json", "កម្ពជា ឆ្ឃឞឞ"]);
        assert!(
            o.starts_with(
                "{\"ok\":false,\"tokens\":[{\"token\":\"កម្ពជា\",\"status\":\"corrected\",\"fix\":\"កម្ពុជា\"}"
            ),
            "{o}"
        );
        assert!(o.contains("\"status\":\"suspect\",\"fix\":null"), "{o}");
        assert_eq!(r.ok(), Some(1), "suspect tokens exit 1");
    }

    #[test]
    fn words_json_lists_the_words() {
        let (_, o) = go(&["words", "--json", "ព្រះរាជាណាចក្រកម្ពុជា"]);
        assert_eq!(o.trim_end(), "{\"words\":[\"ព្រះរាជាណាចក្រ\",\"កម្ពុជា\"]}");
    }

    /// The BOM and the file's final newline are not part of the text.
    #[test]
    fn clean_input_drops_bom_and_one_final_newline() {
        assert_eq!(clean_input("\u{FEFF}ក\n".into()), "ក");
        assert_eq!(clean_input("ក\r\n".into()), "ក");
        assert_eq!(clean_input("ក\n\n".into()), "ក\n", "only one newline: a blank last line is text");
        assert_eq!(clean_input("ក".into()), "ក");
    }

    #[test]
    fn empty_file_name_is_named() {
        assert_eq!(why(go(&["read", ""]).0), "empty file name");
    }

    /// A wrong --font path must say it came from --font.
    #[test]
    fn missing_font_mentions_the_flag() {
        let m = why(go(&["shape", "--font", "/nonexistent/f.ttf", "ក"]).0);
        assert!(m.contains("--font") && m.contains("/nonexistent/f.ttf"), "{m}");
    }

    /// Shaping nothing is a usage mistake, not a successful "width 0".
    #[test]
    fn shape_empty_text_is_a_usage_error() {
        let font = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/fonts/NotoSansKhmer-Regular.ttf");
        assert!(matches!(go(&["shape", "--font", font, ""]).0, Err(Fail::Usage(_))));
    }

    /// Each command has its own help with an example; the top-level help names
    /// every command and the exit codes.
    #[test]
    fn help_is_per_command() {
        for cmd in ["shape", "check", "normalize", "spell", "words", "read", "ocr"] {
            let (r, o) = go(&[cmd, "--help"]);
            assert_eq!(r.ok(), Some(0));
            assert!(o.starts_with(&format!("usage: aksar {cmd}")), "{cmd}: {o}");
            assert!(o.contains("example:") && o.contains("exit status:"), "{cmd}: {o}");
            assert_eq!(go(&["help", cmd]).1, o);
        }
        let (_, top) = go(&["--help"]);
        for cmd in ["shape", "check", "normalize", "spell", "words", "read", "ocr"] {
            assert!(top.contains(&format!("  {cmd} ")), "{cmd} missing from top-level help");
        }
        assert!(top.contains("--json") && top.contains("exit status"));
    }

    #[test]
    fn version_works_anywhere() {
        let want = format!("aksar {}\n", env!("CARGO_PKG_VERSION"));
        for a in [&["-V"][..], &["--version"], &["check", "-V"], &["read", "x", "--version"]] {
            assert_eq!(go(a).1, want, "{a:?}");
        }
        // After `--` it is text.
        assert_eq!(go(&["normalize", "--", "-V"]).1, "-V\n");
    }

    #[test]
    fn shape_numbers_have_no_float_noise() {
        assert_eq!(num(9.216001), "9.216");
        assert_eq!(num(20.320002), "20.32");
        assert_eq!(num(0.0), "0");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(-0.832), "-0.832");
        assert_eq!(num(512.0), "512");
        let font = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/fonts/NotoSansKhmer-Regular.ttf");
        let o = go(&["shape", "--font", font, "--size", "32", "ខ្មែរ"]).1;
        assert!(o.contains("width 38.752  missing 0"), "{o}");
    }

    /// A typo gets a hint instead of a dead end; unrelated words get none.
    #[test]
    fn typos_suggest_the_closest_name() {
        assert!(why(go(&["chek", "x"]).0).ends_with("did you mean \"check\"?"));
        assert!(why(go(&["help", "nomralize"]).0).ends_with("did you mean \"normalize\"?"));
        assert!(why(go(&["check", "--jsn", "x"]).0).ends_with("did you mean \"--json\"?"));
        assert!(why(go(&["shape", "--fnt", "f", "x"]).0).ends_with("did you mean \"--font\"?"));
        assert!(!why(go(&["banana"]).0).contains("did you mean"));
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    /// `--` lets text or a file name start with `-` without being read as an option.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(go(&["normalize", "--", "-x"]).1, "-x\n");
        assert!(matches!(go(&["normalize", "-x"]).0, Err(Fail::Usage(_))));
        assert_eq!(go(&["normalize", "--", "--help"]).1, "--help\n", "--help after -- is text");
        let missing = go(&["read", "--", "--json"]).0;
        assert!(matches!(missing, Err(Fail::Run(m)) if m.starts_with("--json:")), "a file named --json");
    }

    /// A bad --feature names the value, so the user knows which one to fix.
    #[test]
    fn bad_feature_is_named() {
        let font = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/fonts/NotoSansKhmer-Regular.ttf");
        match go(&["shape", "--font", font, "--feature", "+++", "ក"]).0 {
            Err(Fail::Usage(m)) => assert!(m.contains("+++"), "{m}"),
            _ => panic!("expected a usage error"),
        }
    }

    // Without the feature, `ocr` must say how to get it, not "unknown command".
    #[cfg(not(feature = "ocr"))]
    #[test]
    fn ocr_without_feature_explains_the_rebuild() {
        match go(&["ocr", "--model", "m", "--alphabet", "a", "p.png"]).0 {
            Err(Fail::Run(msg)) => assert!(msg.contains("--features ocr"), "{msg}"),
            _ => panic!("expected a run error naming the `ocr` feature"),
        }
    }

    // Bad arguments are usage errors (exit 2 with usage), found before any file is read.
    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_rejects_bad_arguments_before_loading_files() {
        for bad in [
            &["ocr"][..],
            &["ocr", "--alphabet", "a.json", "p.png"],
            &["ocr", "--model", "m.onnx", "p.png"],
            &["ocr", "--model", "m.onnx", "--alphabet", "a.json"],
            &["ocr", "--model", "m.onnx", "--alphabet", "a.json", "p.png", "q.png"],
            &["ocr", "--model", "m.onnx", "--alphabet", "a.json", "--fast", "p.png"],
            &["ocr", "--model"],
        ] {
            assert!(matches!(go(bad).0, Err(Fail::Usage(_))), "{bad:?}");
        }
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_parses_flags_in_any_order() {
        let a = args(&["p.png", "--no-fix", "--alphabet", "a.json", "--json", "--model", "m.onnx"]);
        let Ok(o) = ocr_args(&a, None) else { panic!("valid arguments rejected") };
        assert!(matches!(o.source, ModelSource::Files { model: "m.onnx", alphabet: "a.json" }));
        assert_eq!((o.image, o.json, o.fix), ("p.png", true, false));
    }

    /// `--model-dir`, else AKSAR_MODEL_DIR; never both kinds of source at once.
    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_finds_its_model_folder() {
        let dir = |a: &[&str], env: Option<&str>| match ocr_args(&args(a), env.map(String::from)) {
            Ok(OcrArgs { source: ModelSource::Dir(d), .. }) => Ok(d.into_owned()),
            Ok(_) => Err("files".to_string()),
            Err(Fail::Usage(m) | Fail::Run(m)) => Err(m),
            Err(_) => Err(String::new()),
        };
        assert_eq!(dir(&["--model-dir", "m", "p.png"], None).unwrap(), "m");
        assert_eq!(dir(&["--model-dir", "m", "p.png"], Some("env")).unwrap(), "m");
        assert_eq!(dir(&["p.png"], Some("env")).unwrap(), "env");
        assert!(dir(&["p.png"], None).unwrap_err().contains("AKSAR_MODEL_DIR"));
        assert!(dir(&["p.png"], Some("")).unwrap_err().contains("--model-dir"));
        assert_eq!(
            dir(&["--model", "m.onnx", "--alphabet", "a.json", "p.png"], Some("env")).unwrap_err(),
            "files"
        );
        assert!(
            dir(&["--model-dir", "m", "--model", "x.onnx", "p.png"], None).unwrap_err().contains("not both")
        );
    }

    /// A bad alphabet must name the alphabet file, not the model.
    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_bad_alphabet_names_the_alphabet_file() {
        let dir = std::env::temp_dir().join(format!("aksar-cli-alpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (m, al, png) = (dir.join("m.onnx"), dir.join("a.json"), dir.join("p.png"));
        std::fs::write(&m, b"not a model").unwrap();
        std::fs::write(&al, b"{}").unwrap();
        std::fs::write(&png, b"").unwrap();
        let r = go(&[
            "ocr",
            "--model",
            m.to_str().unwrap(),
            "--alphabet",
            al.to_str().unwrap(),
            png.to_str().unwrap(),
        ])
        .0;
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(r, Err(Fail::Run(msg)) if msg.starts_with(al.to_str().unwrap())));
    }

    // A missing model file is a run error (exit 2, no usage dump) naming the file.
    #[cfg(feature = "ocr")]
    #[test]
    fn ocr_missing_model_is_a_run_error() {
        let dir = std::env::temp_dir().join(format!("aksar-cli-nomodel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("p.png");
        std::fs::write(&png, b"").unwrap();
        let png = png.to_str().unwrap();
        let r = go(&["ocr", "--model", "/nonexistent/m.onnx", "--alphabet", "a.json", png]).0;
        assert!(matches!(r, Err(Fail::Run(m)) if m.contains("/nonexistent/m.onnx")));
        let r = go(&["ocr", "--model-dir", "/nonexistent", png]).0;
        // The model path is joined to --model-dir, so it uses the platform's separator.
        let joined = std::path::Path::new("/nonexistent").join("model.onnx");
        let joined = joined.display().to_string();
        assert!(matches!(r, Err(Fail::Run(m)) if m.contains(&joined)));
        // The image is read first, so a typo there is reported before any model work.
        let r = go(&["ocr", "--model-dir", "/nonexistent", "/nonexistent/p.png"]).0;
        assert!(matches!(r, Err(Fail::Run(m)) if m.starts_with("/nonexistent/p.png")));
        // --alphabet has the same size cap as --model-dir.
        let big = dir.join("big.json");
        std::fs::write(&big, vec![b' '; aksar_recog::MAX_ALPHABET_BYTES as usize + 1]).unwrap();
        let r = go(&["ocr", "--model", png, "--alphabet", big.to_str().unwrap(), png]).0;
        assert!(matches!(r, Err(Fail::Run(m)) if m.contains("big.json") && m.contains("bytes allowed")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
