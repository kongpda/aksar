#!/usr/bin/env python3
"""Turn aksar-render lines (PNG + labels.tsv) into Tesseract LSTM training files.

Usage: prep.py <lines-dir> <unicharset> <out-dir> [--eval 0.05] [--jobs 4]

- Skips labels the model can't encode (characters missing from its unicharset:
  a fine-tune can't add characters).
- Writes <stem>.gt.txt and a WordStr <stem>.box per line, runs
  `tesseract <png> <stem> --psm 13 lstm.train` to make <stem>.lstmf, and writes
  list.train / list.eval (split is deterministic).
"""
import argparse, os, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

def load_unichars(path):
    units = set()
    for line in open(path, encoding='utf-8').read().splitlines()[1:]:
        u = line.split(' ')[0]
        if u not in ('NULL', 'Joined', '|Broken|0|1'):
            units.add(u)
    return units, max(len(u) for u in units)

def encodable(text, units, longest):
    i = 0
    while i < len(text):
        if text[i] == ' ':
            i += 1
            continue
        for n in range(min(longest, len(text) - i), 0, -1):
            if text[i:i + n] in units:
                i += n
                break
        else:
            return False
    return True

def png_size(path):
    with open(path, 'rb') as f:
        head = f.read(24)
    return int.from_bytes(head[16:20], 'big'), int.from_bytes(head[20:24], 'big')

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('lines'); ap.add_argument('unicharset'); ap.add_argument('out')
    ap.add_argument('--eval', type=float, default=0.05); ap.add_argument('--jobs', type=int, default=4)
    a = ap.parse_args()
    units, longest = load_unichars(a.unicharset)
    os.makedirs(a.out, exist_ok=True)
    rows = [l.rstrip('\n').split('\t') for l in open(os.path.join(a.lines, 'labels.tsv'), encoding='utf-8') if l.strip()]
    keep, skipped = [], 0
    for file, text, *_ in rows:
        if not encodable(text, units, longest):
            skipped += 1
            continue
        stem = os.path.join(a.out, file.rsplit('.', 1)[0])
        src = os.path.join(a.lines, file)
        w, h = png_size(src)
        png = stem + '.png'
        # Always replace: a reused out dir may hold a different line under the same name.
        for old in (png, stem + '.lstmf'):
            if os.path.exists(old):
                os.remove(old)
        os.link(src, png) if os.stat(src).st_dev == os.stat(a.out).st_dev else __import__('shutil').copy(src, png)
        open(stem + '.gt.txt', 'w', encoding='utf-8').write(text + '\n')
        # WordStr box: the whole line is one "word" spanning the image.
        with open(stem + '.box', 'w', encoding='utf-8') as b:
            b.write(f'WordStr 0 0 {w} {h} 0 #{text}\n')
            b.write(f'\t {w} {h} {w + 1} {h + 1} 0\n')
        keep.append(stem)

    def run(stem):
        r = subprocess.run(['tesseract', stem + '.png', stem, '--psm', '13', 'lstm.train'],
                           capture_output=True, env={**os.environ, 'OMP_THREAD_LIMIT': '1'})
        return r.returncode == 0 and os.path.exists(stem + '.lstmf')

    with ThreadPoolExecutor(a.jobs) as ex:
        ok = list(ex.map(run, keep))
    made = [s for s, o in zip(keep, ok) if o]
    n_eval = max(1, int(len(made) * a.eval))
    with open(os.path.join(a.out, 'list.eval'), 'w') as f:
        f.writelines(s + '.lstmf\n' for s in made[:n_eval])
    with open(os.path.join(a.out, 'list.train'), 'w') as f:
        f.writelines(s + '.lstmf\n' for s in made[n_eval:])
    print(f'{len(made)} lines ({n_eval} eval), {skipped} skipped (characters not in the model), '
          f'{len(keep) - len(made)} failed', file=sys.stderr)

if __name__ == '__main__':
    main()
