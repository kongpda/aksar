#!/usr/bin/env python3
"""Run plain Tesseract (CLI) over a corpus and score it: compare language packs.

Usage: tesseract_bench.py <tessdata-dir> <corpus-dir> [--jobs 4] [--psm 3] [--out results.jsonl]

--psm 7 for single-line images (e.g. the EKYC line crops).

<corpus-dir>: page images + <stem>.gt.txt (+ optional manifest.tsv with file, lang,
dpi, category, font). Language: manifest's, else khm+eng when the truth has Khmer,
else eng. Prints CER (whitespace ignored, NFC) per language, category and font,
and writes one JSON line per page (same fields error_report.py reads).
"""
import argparse, csv, difflib, json, os, subprocess, time, unicodedata
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor

def cer(gt, ocr):
    g = [c for c in unicodedata.normalize('NFC', gt) if not c.isspace()]
    o = [c for c in unicodedata.normalize('NFC', ocr) if not c.isspace()]
    sm = difflib.SequenceMatcher(None, g, o, autojunk=False)
    edits = sum(max(i2 - i1, j2 - j1) for op, i1, i2, j1, j2 in sm.get_opcodes() if op != 'equal')
    return edits, len(g)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('tessdata'); ap.add_argument('corpus')
    ap.add_argument('--jobs', type=int, default=4); ap.add_argument('--out')
    ap.add_argument('--psm', default='3')
    a = ap.parse_args()
    meta = {}
    mf = os.path.join(a.corpus, 'manifest.tsv')
    if os.path.exists(mf):
        meta = {r['file']: r for r in csv.DictReader(open(mf, encoding='utf-8'), delimiter='\t')}
    files = sorted(f for f in os.listdir(a.corpus) if f.lower().endswith(('.png', '.jpg', '.jpeg', '.tif', '.tiff')))

    def run(f):
        stem = f.rsplit('.', 1)[0]
        gt = open(os.path.join(a.corpus, stem + '.gt.txt'), encoding='utf-8').read()
        m = meta.get(f, {})
        lang = m.get('lang') or ('khm+eng' if any('ក' <= c <= '៿' for c in gt) else 'eng')
        args = ['tesseract', os.path.join(a.corpus, f), '-', '--tessdata-dir', a.tessdata, '-l', lang, '--psm', a.psm]
        if m.get('dpi'):
            args += ['--dpi', m['dpi']]
        t = time.time()
        r = subprocess.run(args, capture_output=True, text=True, env={**os.environ, 'OMP_THREAD_LIMIT': '1'})
        if r.returncode != 0:
            raise SystemExit(f'tesseract failed on {f}: {r.stderr.strip()}')
        e, n = cer(gt, r.stdout)
        return {'file': f, 'lang': lang, 'category': m.get('category', 'real'), 'font': m.get('font', '?'),
                'mode': 'tesseract', 'ms': int((time.time() - t) * 1000), 'text': r.stdout, 'edits': e, 'chars': n}

    with ThreadPoolExecutor(a.jobs) as ex:
        rows = list(ex.map(run, files))
    if a.out:
        with open(a.out, 'w', encoding='utf-8') as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + '\n')
    for key in ('lang', 'category', 'font'):
        agg = defaultdict(lambda: [0, 0])
        for r in rows:
            k = (r['lang'].split('+')[0], r[key])
            agg[k][0] += r['edits']; agg[k][1] += r['chars']
        print(f'\n| {key} | CER |\n|---|---|')
        for (lang, k), (e, n) in sorted(agg.items()):
            print(f'| {lang} {k} | {100 * e / max(1, n):.2f}% |')

if __name__ == '__main__':
    main()
