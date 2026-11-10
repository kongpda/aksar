//! End-to-end tests of the `aksar` binary: behavior that needs real pipes and
//! processes (stdin, closed stdout, exit status).

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn aksar() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aksar"))
}

/// Runs `aksar args` with `stdin` as standard input.
fn with_stdin(args: &[&str], stdin: &str) -> Output {
    let mut child = aksar()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

/// `aksar read big.txt | head -1` used to panic with "failed printing to
/// stdout: Broken pipe" (exit 101). A reader that went away is not an error.
#[test]
fn closed_stdout_is_a_quiet_success() {
    let dir = std::env::temp_dir().join(format!("aksar-cli-pipe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("big.txt");
    std::fs::write(&big, "ទីក្រុងភ្នំពេញ កម្ពជា\n".repeat(3_000)).unwrap();
    let mut child = aksar()
        .args(["read", "--json", big.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take()); // the reader closes the pipe before reading anything
    let o = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!err.contains("panicked") && !err.contains("Broken pipe"), "{err}");
    assert_eq!(o.status.code(), Some(0), "{err}");
}

#[test]
fn dash_reads_stdin_for_text_commands() {
    assert_eq!(stdout(&with_stdin(&["normalize", "-"], "កា្រ\n")), "ក្រា\n");
    assert_eq!(stdout(&with_stdin(&["words", "-"], "ព្រះរាជាណាចក្រកម្ពុជា\n")), "ព្រះរាជាណាចក្រ | កម្ពុជា\n");
    let o = with_stdin(&["spell", "-", "--json"], "កម្ពជា\n");
    assert!(stdout(&o).contains("\"status\":\"corrected\""), "{}", stdout(&o));
    assert_eq!(o.status.code(), Some(0));
    let o = with_stdin(&["check", "-", "--json"], "កេា\n");
    assert!(stdout(&o).contains("\"kind\":\"split-vowel\""), "{}", stdout(&o));
}

/// A BOM is invisible but is not Khmer: it must not reach the engine.
#[test]
fn bom_is_stripped_from_stdin_and_files() {
    assert_eq!(stdout(&with_stdin(&["normalize", "-"], "\u{FEFF}ក\n")), "ក\n");
    let o = with_stdin(&["read", "-", "--json"], "\u{FEFF}ក\n");
    assert!(!stdout(&o).contains('\u{FEFF}'), "{}", stdout(&o));
    let path = std::env::temp_dir().join(format!("aksar-cli-bom-{}.txt", std::process::id()));
    std::fs::write(&path, "\u{FEFF}ក\n").unwrap();
    let o = aksar().args(["read", "--json", path.to_str().unwrap()]).output().unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!stdout(&o).contains('\u{FEFF}'), "{}", stdout(&o));
}

/// The final newline of a file ends its last line; it does not start another.
#[test]
fn final_newline_adds_no_empty_line() {
    let with = stdout(&with_stdin(&["read", "-", "--json"], "កម្ពុជា\n"));
    let without = stdout(&with_stdin(&["read", "-", "--json"], "កម្ពុជា"));
    assert_eq!(with, without);
    assert!(!with.contains("{\"text\":\"\",\"words\":[]}"), "{with}");
}

#[test]
fn flags_after_the_text_and_exit_codes() {
    let o = aksar().args(["check", "កេា", "--json"]).output().unwrap();
    assert!(stdout(&o).starts_with("{\"ok\":true"), "{}", stdout(&o));
    assert_eq!(o.status.code(), Some(0));
    let o = aksar().args(["check", "\u{17B6}", "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "broken clusters exit 1");
}

#[test]
fn usage_errors_exit_2() {
    for args in [&["shape", "--font", "x.ttf", ""][..], &["read", ""], &["check"], &["bogus"]] {
        let o = aksar().args(args).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(!String::from_utf8_lossy(&o.stderr).contains("panicked"), "{args:?}");
    }
}
