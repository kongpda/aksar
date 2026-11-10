//! How often is a random substring of real text found by FTS-style AND matching?
use pdftape_ocr::khmer as cur;
use std::collections::HashSet;
use unicode_segmentation::UnicodeSegmentation;

fn unspaced(c: char) -> bool {
    matches!(c as u32, 0x0E00..=0x0EFF | 0x1000..=0x109F | 0xAA60..=0xAA7F | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F)
}

/// Proposed splitter: grapheme unigrams (doc only) + bigrams for unspaced scripts, ICU words otherwise.
fn proto(doc: bool) -> impl FnMut(&str, &mut Vec<String>) {
    move |run: &str, out: &mut Vec<String>| {
        for chunk in run.split_whitespace() {
            if chunk.chars().any(unspaced) {
                let g: Vec<&str> = chunk.graphemes(true).filter(|g| g.chars().any(char::is_alphanumeric) || g.chars().any(unspaced)).collect();
                if doc || g.len() == 1 { out.extend(g.iter().map(|s| s.to_string())); }
                out.extend(g.windows(2).map(|w| format!("{}{}", w[0], w[1])));
            } else {
                for w in cur::segment_words(chunk) { out.push(w.to_lowercase()); }
            }
        }
    }
}

struct Rng(u64);
impl Rng { fn next(&mut self) -> usize { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 as usize } }

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    println!("{:<9} {:>7} {:>11} {:>11} {:>14}", "lang", "probes", "current", "proposed", "index size x");
    for lang in ["khm", "tha", "lao", "mya", "cmn_hans", "cmn_hant", "jpn", "kor", "eng", "vie"] {
        let text = std::fs::read_to_string(format!("{dir}/udhr_{lang}.txt")).unwrap();
        let paras: Vec<&str> = text.lines().filter(|l| l.chars().count() > 30).collect();
        let mut rng = Rng(0xC0FFEE);
        let (mut n, mut hit_cur, mut hit_new, mut sz_cur, mut sz_new) = (0, 0, 0, 0usize, 0usize);
        for p in &paras {
            sz_cur += cur::search_tokens(p).split(' ').count();
            sz_new += aksar::search::search_tokens_with(p, proto(true)).split(' ').count();
        }
        while n < 400 {
            let p = paras[rng.next() % paras.len()];
            let g: Vec<&str> = p.graphemes(true).collect();
            let len = 2 + rng.next() % 5;
            if g.len() <= len { continue; }
            let st = rng.next() % (g.len() - len);
            let q: String = g[st..st + len].concat();
            if q.chars().any(|c| c.is_whitespace() || !c.is_alphanumeric() && !unspaced(c) && !pdftape_ocr::khmer::is_khmer(c)) { continue; }
            n += 1;
            let found = |doc: String, query: Option<String>| -> bool {
                let d: HashSet<&str> = doc.split(' ').collect();
                query.is_some_and(|q| q.split(" AND ").all(|t| d.contains(t.trim_matches('"'))))
            };
            hit_cur += found(cur::search_tokens(p), cur::fts_query(&q)) as usize;
            hit_new += found(aksar::search::search_tokens_with(p, proto(true)), aksar::search::fts_query_with(&q, proto(false))) as usize;
        }
        // Whole-word queries: a multi-character ICU word of the paragraph, searched alone.
        let (mut wn, mut wc, mut wp) = (0, 0, 0);
        let mut tries = 0;
        while wn < 300 && tries < 20000 {
            tries += 1;
            let p = paras[rng.next() % paras.len()];
            let words: Vec<String> = cur::segment_words(p).into_iter().filter(|w| w.graphemes(true).count() >= 2 && w.chars().all(|c| c.is_alphanumeric() || unspaced(c) || pdftape_ocr::khmer::is_khmer(c))).collect();
            if words.is_empty() { continue; }
            let q = &words[rng.next() % words.len()];
            wn += 1;
            let found = |doc: String, query: Option<String>| -> bool {
                let d: HashSet<&str> = doc.split(' ').collect();
                query.is_some_and(|q| q.split(" AND ").all(|t| d.contains(t.trim_matches('"'))))
            };
            wc += found(cur::search_tokens(p), cur::fts_query(q)) as usize;
            wp += found(aksar::search::search_tokens_with(p, proto(true)), aksar::search::fts_query_with(q, proto(false))) as usize;
        }
        println!("{:<9} {:>7} {:>10.0}% {:>10.0}% {:>14.1}   words: {:>4.0}% -> {:>4.0}%", lang, n, 100.0 * hit_cur as f64 / n as f64, 100.0 * hit_new as f64 / n as f64, sz_new as f64 / sz_cur as f64, 100.0 * wc as f64 / wn.max(1) as f64, 100.0 * wp as f64 / wn.max(1) as f64);
    }
}
