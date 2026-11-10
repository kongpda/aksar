//! Python bindings: `pip install ./crates/aksar-py` (maturin), then
//!
//! ```python
//! import aksar, json
//! aksar.normalize("កា្រ")            # 'ក្រា'
//! aksar.is_valid("ាក")               # False
//! doc = json.loads(aksar.read("ទីក្រុងភ្នំពេញ កម្ពជា"))
//! aksar.spell("កម្ពជា")              # ('corrected', 'កម្ពុជា')
//! aksar.issues("\u17b6")             # [('broken-cluster', 0, 1, 'broken cluster: ...')]
//! aksar.words("ព្រះរាជាណាចក្រកម្ពុជា")  # ['ព្រះរាជាណាចក្រ', 'កម្ពុជា']
//! ```

use aksar_core::dict::{Check, Dictionary};
use pyo3::prelude::*;

/// Canonical Khmer storage order (safe for typed text and OCR output).
#[pyfunction]
fn normalize(text: &str) -> String {
    aksar_core::syllable::normalize(text)
}

/// `normalize` plus OCR cleanup (invisible vowels, stutters).
#[pyfunction]
fn repair(text: &str) -> String {
    aksar_core::syllable::repair(text)
}

/// True when the text has no broken clusters (renders without ◌).
#[pyfunction]
fn is_valid(text: &str) -> bool {
    aksar_core::syllable::validate(text).is_ok()
}

/// Khmer clusters (base + subscripts + vowel + signs) and other characters.
#[pyfunction]
fn clusters(text: &str) -> Vec<String> {
    aksar_core::syllable::clusters(text).into_iter().map(String::from).collect()
}

/// Splits Khmer text into dictionary words.
#[pyfunction]
fn words(text: &str) -> Vec<String> {
    Dictionary::bundled().segment(text).into_iter().map(String::from).collect()
}

/// Spell-checks one token against the dictionary (as `aksar spell` does):
/// ('known', normalized), ('corrected', fix) or ('suspect', normalized).
#[pyfunction]
fn spell(token: &str) -> (&'static str, String) {
    match Dictionary::bundled().check(token) {
        Check::Known => ("known", aksar_core::syllable::normalize(token)),
        Check::Corrected(fix) => ("corrected", fix),
        _ => ("suspect", aksar_core::syllable::normalize(token)),
    }
}

/// Deprecated alias of `spell` (it is a dictionary lookup, not `aksar check`).
#[pyfunction]
fn check(py: Python<'_>, token: &str) -> PyResult<(&'static str, String)> {
    PyErr::warn(
        py,
        &py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
        c"aksar.check() is deprecated; use aksar.spell() (or aksar.issues() for validation)",
        1,
    )?;
    Ok(spell(token))
}

/// Validation issues, as `aksar check` reports them: a list of
/// (kind, start, end, message). `start`/`end` are character offsets, so
/// `text[start:end]` is the offending text. `kind` is `broken-cluster` (the only
/// error), `deprecated`, `split-vowel`, `dangling-coeng`, `deep-stack` or
/// `subscript-after-vowel`.
#[pyfunction]
fn issues(text: &str) -> Vec<(&'static str, usize, usize, String)> {
    aksar_core::syllable::issues(text)
        .into_iter()
        .map(|i| {
            let start = text[..i.range.start].chars().count();
            let end = start + text[i.range.clone()].chars().count();
            (i.kind.name(), start, end, i.kind.to_string())
        })
        .collect()
}

/// Reads Khmer text and returns the document as a JSON string
/// (pages → blocks → lines → words with parts, suggestions and flags).
#[pyfunction]
#[pyo3(signature = (text, autocorrect = true, user_words = Vec::new()))]
fn read(text: &str, autocorrect: bool, user_words: Vec<String>) -> String {
    aksar_core::read::Aksar::new().autocorrect(autocorrect).user_words(user_words).read(text).to_json()
}

/// Space-separated full-text index tokens (any Khmer substring is findable).
#[pyfunction]
fn search_tokens(text: &str) -> String {
    aksar_core::search::search_tokens(text)
}

/// FTS5 MATCH expression for a user query, or None for an empty query.
#[pyfunction]
fn fts_query(query: &str) -> Option<String> {
    aksar_core::search::fts_query(query)
}

#[pymodule]
fn aksar(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    for f in [
        wrap_pyfunction!(normalize, m)?,
        wrap_pyfunction!(repair, m)?,
        wrap_pyfunction!(is_valid, m)?,
        wrap_pyfunction!(clusters, m)?,
        wrap_pyfunction!(words, m)?,
        wrap_pyfunction!(spell, m)?,
        wrap_pyfunction!(check, m)?,
        wrap_pyfunction!(issues, m)?,
        wrap_pyfunction!(read, m)?,
        wrap_pyfunction!(search_tokens, m)?,
        wrap_pyfunction!(fts_query, m)?,
    ] {
        m.add_function(f)?;
    }
    Ok(())
}
