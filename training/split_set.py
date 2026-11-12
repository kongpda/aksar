#!/usr/bin/env python3
"""Split a line set into train and test parts, by source page, reproducibly.

Usage: split_set.py <line-set> <out-dir> [--test 0.2] [--seed aksar-v1]

<line-set>: a directory with labels.tsv, or <stem>.png + <stem>.gt.txt pairs (what
research/khmer/ekyc/convert.py writes). Writes <out-dir>/train/labels.tsv and
<out-dir>/test/labels.tsv, pointing at the original images (no copies), and
<out-dir>/split.json with the counts, the settings and a checksum of the test part.

Lines are grouped by source page: a crop named <task>_<image>_<n>.png belongs to
page <task>_<image> (the trailing _<n> is dropped), so crops of one page never
land on both sides. A page goes to test when a hash of (seed, page) falls under
--test: the same page always lands on the same side, even when pages are added,
so the test split stays untouched as the data grows.

Train on <out-dir>/train; give <out-dir>/test to train_crnn.py as --holdout and to
aksar-bench as a --set.
"""
import argparse, hashlib, json, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from train_crnn import load, set_digest  # noqa: E402

CROP_SUFFIX = re.compile(r'_\d+$')


def page_of(path):
    stem = os.path.splitext(os.path.basename(path))[0]
    return CROP_SUFFIX.sub('', stem)


def goes_to_test(page, seed, fraction):
    h = hashlib.sha256(f'{seed}\0{page}'.encode('utf-8')).digest()
    return int.from_bytes(h[:8], 'big') / 2 ** 64 < fraction


def write_labels(dir_, rows):
    os.makedirs(dir_, exist_ok=True)
    tmp = os.path.join(dir_, 'labels.tsv.tmp')
    with open(tmp, 'w', encoding='utf-8') as f:
        for path, text, font in rows:
            if '\t' in text or '\n' in text:
                raise SystemExit(f'{path}: label has a tab or newline, which labels.tsv cannot hold')
            rel = os.path.relpath(path, dir_)
            f.write(f'{rel}\t{text}' + (f'\t{font}' if font else '') + '\n')
    os.replace(tmp, os.path.join(dir_, 'labels.tsv'))


def main(argv=None):
    ap = argparse.ArgumentParser(description='Split a line set into train and test parts by source page.')
    ap.add_argument('set', help='line set directory')
    ap.add_argument('out', help='output directory (gets train/, test/, split.json)')
    ap.add_argument('--test', type=float, default=0.2, help='fraction of pages for test (default 0.2)')
    ap.add_argument('--seed', default='aksar-v1', help='changing it reshuffles the split: keep it fixed')
    a = ap.parse_args(argv)
    if not 0 < a.test < 1:
        ap.error('--test must be between 0 and 1')
    rows = load(a.set)
    if not rows:
        raise SystemExit(f'no lines in {a.set}')
    out = os.path.abspath(a.out)
    if os.path.commonpath([out, os.path.abspath(a.set)]) == os.path.abspath(a.set):
        raise SystemExit('put <out-dir> outside the line set, so its labels.tsv files are not read as lines')
    train, test = [], []
    for row in rows:
        (test if goes_to_test(page_of(row[0]), a.seed, a.test) else train).append(row)
    if not train or not test:
        raise SystemExit(f'one side is empty ({len(train)} train, {len(test)} test lines): '
                         'too few pages for this --test fraction')
    write_labels(os.path.join(out, 'train'), train)
    write_labels(os.path.join(out, 'test'), test)
    info = {
        'source': os.path.abspath(a.set), 'seed': a.seed, 'test_fraction': a.test,
        'train': {'lines': len(train), 'pages': len({page_of(p) for p, _, _ in train})},
        'test': {'lines': len(test), 'pages': len({page_of(p) for p, _, _ in test}), 'sha256': set_digest(test)},
    }
    with open(os.path.join(out, 'split.json'), 'w', encoding='utf-8') as f:
        json.dump(info, f, indent=1)
        f.write('\n')
    print(f"train: {info['train']['lines']} lines from {info['train']['pages']} pages; "
          f"test: {info['test']['lines']} lines from {info['test']['pages']} pages -> {out}")


if __name__ == '__main__':
    main()
