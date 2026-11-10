use pdfk::khmer as old;
use pdftape_ocr::khmer as new;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    println!("{:<10} {:>6} {:>9} {:>10} {:>13} {:>9} {:>9}", "lang", "paras", "normalize", "index_form", "search_tokens", "clusters", "words");
    let mut langs: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "txt")).collect();
    langs.sort();
    for p in langs {
        let text = std::fs::read_to_string(&p).unwrap();
        let paras: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let mut d = [0usize; 5];
        let mut example: Option<String> = None;
        for l in &paras {
            let checks = [
                old::normalize(l) != new::normalize(l),
                old::index_form(l) != new::index_form(l),
                old::search_tokens(l) != new::search_tokens(l),
                old::clusters(l) != new::clusters(l),
                old::segment_words(l) != new::segment_words(l),
            ];
            for (i, c) in checks.iter().enumerate() { if *c { d[i] += 1; } }
            if checks[0] && example.is_none() { example = Some(format!("normalize: {:?} vs {:?}", &old::normalize(l).chars().take(40).collect::<String>(), &new::normalize(l).chars().take(40).collect::<String>())); }
        }
        let name = p.file_stem().unwrap().to_string_lossy().replace("udhr_", "");
        println!("{:<10} {:>6} {:>9} {:>10} {:>13} {:>9} {:>9}", name, paras.len(), d[0], d[1], d[2], d[3], d[4]);
        if let Some(e) = example { println!("    e.g. {e}"); }
    }
}
