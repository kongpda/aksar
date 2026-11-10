#!/usr/bin/env python3
"""Where do Khmer OCR errors come from? Aligns OCR output with the ground truth
and prints the error table used by docs/KHMER_ACCURACY_PLAN.md.

Usage:
  error_report.py <corpus-dir> <bench.jsonl> [--mode full] [--top 15]

<corpus-dir>: page images + <stem>.gt.txt (+ manifest.tsv with a font column);
<bench.jsonl>: PDFTape's crates/pdftape-ocr/bench output (one JSON per page and
mode, with "file", "mode", "text"). Only Khmer pages are counted.

Edits are character-level on text with whitespace removed (like the CER in
PDFTape's score.py). Categories, in priority order:
  ta/da        subscript TA <-> DA (drawn the same: spelling, not recognition)
  latin        a Latin letter or ASCII symbol involved
  digit/punct  digits or punctuation (Khmer or ASCII)
  vowel/sign   dependent vowels and signs missing, extra or swapped
  coeng        the subscript mark ្ missing or extra
  consonant    consonants and independent vowels read as another, missing or extra
"""
import argparse, csv, difflib, json, os, unicodedata
from collections import Counter, defaultdict

COENG = '្'
def is_khmer(c): return 'ក' <= c <= '៿' or '᧠' <= c <= '᧿'
def is_cons(c): return 'ក' <= c <= 'ឳ'
def is_vowel_sign(c): return 'ា' <= c <= '៑' or c == '៝'
def is_digit_punct(c):
    return ('។' <= c <= 'ៜ') or ('០' <= c <= '៹') or c.isdigit() or unicodedata.category(c).startswith('P')

def category(a, b, prev_a, prev_b):
    chars = a + b
    if a in ('ត', 'ដ') and b in ('ត', 'ដ') and prev_a == COENG and prev_b == COENG:
        return 'ta/da'
    if any(c.isascii() and (c.isalpha() or not c.isalnum()) and not c.isdigit() and not unicodedata.category(c).startswith('P') for c in chars) \
       or any(c.isascii() and c.isalpha() for c in chars):
        return 'latin'
    if chars and all(is_digit_punct(c) for c in chars):
        return 'digit/punct'
    if chars and all(is_vowel_sign(c) for c in chars):
        return 'vowel/sign'
    if COENG in chars:
        return 'coeng'
    return 'consonant'

def edits(gt, ocr):
    """(category, gt_piece, ocr_piece, cost) per edit, cost = chars changed."""
    sm = difflib.SequenceMatcher(None, gt, ocr, autojunk=False)
    out = []
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op == 'equal':
            continue
        a, b = gt[i1:i2], ocr[j1:j2]
        prev_a = gt[i1 - 1] if i1 else ''
        prev_b = ocr[j1 - 1] if j1 else ''
        # Split long replacements into aligned single-character pairs when the lengths match.
        if op == 'replace' and len(a) == len(b) and len(a) > 1:
            for k, (x, y) in enumerate(zip(a, b)):
                pa = a[k - 1] if k else prev_a
                pb = b[k - 1] if k else prev_b
                out.append((category(x, y, pa, pb), x, y, 1))
            continue
        out.append((category(a, b, prev_a, prev_b), a, b, max(len(a), len(b))))
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('corpus'); ap.add_argument('results')
    ap.add_argument('--mode', default='full'); ap.add_argument('--top', type=int, default=15)
    a = ap.parse_args()
    meta = {}
    mf = os.path.join(a.corpus, 'manifest.tsv')
    if os.path.exists(mf):
        for row in csv.DictReader(open(mf, encoding='utf-8'), delimiter='\t'):
            meta[row['file']] = row
    by_cat, pairs = Counter(), Counter()
    by_font, by_page_cat = defaultdict(lambda: [0, 0]), defaultdict(lambda: [0, 0])
    total_chars = total_cost = pages = 0
    for line in open(a.results, encoding='utf-8'):
        r = json.loads(line)
        if r.get('mode') != a.mode or not r['lang'].startswith('khm'):
            continue
        stem = r['file'].rsplit('.', 1)[0]
        gt = unicodedata.normalize('NFC', open(os.path.join(a.corpus, stem + '.gt.txt'), encoding='utf-8').read())
        gt = ''.join(c for c in gt if not c.isspace())
        ocr = ''.join(c for c in unicodedata.normalize('NFC', r['text']) if not c.isspace())
        pages += 1
        total_chars += len(gt)
        m = meta.get(r['file'], {})
        page_cost = 0
        for cat, x, y, cost in edits(gt, ocr):
            by_cat[cat] += cost
            pairs[(cat, x or '∅', y or '∅')] += 1
            page_cost += cost
        total_cost += page_cost
        for key, table in ((m.get('font', '?'), by_font), (m.get('category', r.get('category', '?')), by_page_cat)):
            table[key][0] += page_cost
            table[key][1] += len(gt)

    print(f'### Khmer OCR errors ({pages} pages, mode `{a.mode}`)\n')
    print(f'{total_cost} edits over {total_chars} characters (CER {100 * total_cost / max(1, total_chars):.2f}%).\n')
    print('| Error type | Edits | Share | Top examples (truth → OCR ×count) |')
    print('|---|---|---|---|')
    for cat, n in by_cat.most_common():
        top = [(k, c) for k, c in pairs.most_common() if k[0] == cat][:4]
        ex = ', '.join(f'{x}→{y} ×{c}' for (_, x, y), c in top)
        print(f'| {cat} | {n} | {100 * n / max(1, total_cost):.0f}% | {ex} |')
    folded = total_cost - by_cat.get('ta/da', 0)
    print(f'\nCER with ្ត/្ដ folded: {100 * folded / max(1, total_chars):.2f}%\n')
    for title, table in (('Font', by_font), ('Category', by_page_cat)):
        print(f'| {title} | CER |\n|---|---|')
        for k, (cost, chars) in sorted(table.items(), key=lambda kv: -kv[1][0] / max(1, kv[1][1])):
            print(f'| {k} | {100 * cost / max(1, chars):.2f}% |')
        print()
    print(f'Top {a.top} single edits:\n')
    for (cat, x, y), c in pairs.most_common(a.top):
        print(f'- `{x}` → `{y}` ×{c} ({cat})')

if __name__ == '__main__':
    main()
