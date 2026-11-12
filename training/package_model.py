#!/usr/bin/env python3
"""Package a trained model for release: check it, apply the OCR_PLAN §8 gate, write a
model card and checksums. Nothing is uploaded.

Usage:
  package_model.py <run-dir> --results <bench.json>... --name print-v1 [--out release]
                   [--no-check] [--allow-failed-gate]

<run-dir>: a train_crnn.py output (model.onnx, alphabet.json, reference.txt,
manifest.json, log.tsv). <bench.json>: `aksar-bench ... --json <file>` results for this
model, with `--tesseract` for the comparison (training/evaluate.sh writes them).

Checks, in order (any failure stops, nothing is written):
  1. the run's files exist and model.onnx is the one manifest.json describes (sha256);
  2. rten loads the model and matches PyTorch's reference output (check_model example;
     skip with --no-check, e.g. without a Rust toolchain);
  3. the results were measured on a model of this size;
  4. the gate: on every set, aksar-ocr read every line (no failures), every reading
     passes `validate`, and its CER is below Tesseract's on the same set. A set without a
     Tesseract row fails the gate: there is nothing to compare with. --allow-failed-gate
     packages anyway and says so in the model card (for a pre-release, never a release).

Writes <out>/<name>/: model.onnx, alphabet.json, reference.txt, manifest.json,
results.json, MODEL_CARD.md and SHA256SUMS (check with `sha256sum -c SHA256SUMS`).
Fill in the model card's "To write before release" section by hand.
"""
import argparse, datetime, json, os, shutil, subprocess, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from train_crnn import sha256_file  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RUN_FILES = ('model.onnx', 'alphabet.json', 'reference.txt', 'manifest.json')


def fail(msg):
    raise SystemExit(f'package_model: {msg}')


def load_results(paths):
    rows, sizes = [], set()
    for p in paths:
        with open(p, encoding='utf-8') as f:
            doc = json.load(f)
        if doc.get('schema') != 1 or not isinstance(doc.get('rows'), list):
            fail(f'{p}: not aksar-bench --json output (schema 1)')
        if doc.get('model_bytes') is not None:
            sizes.add(doc['model_bytes'])
        rows += doc['rows']
    return rows, sizes


def gate(rows):
    """[(set, aksar row, tesseract row or None, problems)] for every set aksar-ocr ran on."""
    out = []
    sets = sorted({r['set'] for r in rows if r['engine'] == 'aksar-ocr'})
    for s in sets:
        ours = [r for r in rows if r['set'] == s and r['engine'] == 'aksar-ocr']
        theirs = [r for r in rows if r['set'] == s and r['engine'].startswith('tesseract')]
        if len(ours) > 1:
            fail(f'set {s!r} has {len(ours)} aksar-ocr rows: give each results file once')
        a = ours[0]
        t = min(theirs, key=lambda r: r['cer']) if theirs else None  # the best Tesseract setting
        problems = []
        if a['failed']:
            problems.append(f"{a['failed']} lines failed to read")
        if a['invalid']:
            problems.append(f"{a['invalid']} readings fail validate")
        if t is None:
            problems.append('no Tesseract row to compare with (run aksar-bench with --tesseract)')
        elif not a['cer'] < t['cer']:
            problems.append(f"CER {a['cer']:.2f}% is not below {t['engine']}'s {t['cer']:.2f}%")
        out.append((s, a, t, problems))
    if not out:
        fail('the results have no aksar-ocr rows')
    return out


def card(name, manifest, gates, passed, log_rows, model_bytes, sha):
    m = manifest
    fonts = sorted({f for t in m.get('train', []) for f in t.get('fonts', [])})
    lines = sum(t.get('lines', 0) for t in m.get('train', []))
    env = m.get('environment', {})
    out = [f'# aksar line model `{name}`', '']
    if not passed:
        out += ['> **Gate not passed (OCR_PLAN §8).** Packaged with `--allow-failed-gate`: a test',
                '> build, not a release.', '']
    out += [
        'A Khmer (plus Latin, digits, punctuation) text-line recognizer for '
        '[aksar](https://github.com/kongpda/aksar): CNN + BiLSTM, CTC over Unicode code points, '
        'decoded by `aksar-decode` (grammar-constrained beam search, dictionary re-ranking).',
        '',
        '## Files', '',
        '| File | Size | SHA-256 |', '|---|---|---|',
        f'| model.onnx | {model_bytes / 1e6:.1f} MB | `{sha}` |',
        '', 'All files: `SHA256SUMS` (`sha256sum -c SHA256SUMS`).', '',
        '## Use', '',
        '```sh', f'aksar ocr --model-dir {name} page.png', '```', '',
        '```rust', f'let ocr = aksar_ocr::Ocr::from_dir("{name}")?;', '```', '',
        f"Input: {m.get('input', 'a 48 px high gray line')}. Alphabet: {m.get('alphabet_size')} symbols "
        '(index 0 is the CTC blank).', '',
        '## Measured accuracy', '',
        '`aksar-bench` (CER and cluster CER ignore whitespace; text normalized first).', '',
        '| Set | aksar CER | Cluster CER | WER | Invalid | Tesseract CER | Gate |', '|---|---|---|---|---|---|---|',
    ]
    for s, a, t, problems in gates:
        tcer = f"{t['cer']:.2f}% ({t['engine']})" if t else '—'
        out.append(f"| {s} | {a['cer']:.2f}% | {a['cluster_cer']:.2f}% | {a['wer']:.2f}% | {a['invalid']} | "
                   f"{tcer} | {'pass' if not problems else 'FAIL: ' + '; '.join(problems)} |")
    out += ['', f"Best validation CER during training (greedy decode): {100 * m.get('best_val_cer', float('nan')):.2f}%."]
    if log_rows:
        out += ['', 'Training log (`log.tsv`): ' + ', '.join(f'epoch {e}: {100 * float(c):.2f}%' for e, _, c, *_ in log_rows) + '.']
    out += [
        '', '## Training', '',
        f'- Lines: {lines} from {len(m.get("train", []))} set(s); validation {m.get("val", {}).get("lines")} lines.',
        f'- Fonts: {", ".join(fonts) if fonts else "see the training sets"}.',
        f'- Held-out sets checked for overlap: {len(m.get("holdout", []))}.',
        f'- Text: {m.get("text_source", "?")}.',
        f'- Settings: `{json.dumps(m.get("settings", {}), ensure_ascii=False)}`.',
        f'- Environment: torch {env.get("torch")}, {env.get("device")}, commit `{env.get("git_commit")}`, '
        f'{m.get("hours", "?")} h.',
        '', 'Data checksums are in `manifest.json`.', '',
        '## To write before release', '',
        '- Font licenses (each font above, and that its license allows training).',
        '- Real data used, with its license (e.g. EKYC: MIT).',
        '- Limits: what was not measured (photos, handwriting, columns, very small text).',
        '- License of the model: Apache-2.0 (OCR_PLAN §10).',
        '- Release location (URL) for these files.',
        '', f'Packaged {datetime.date.today().isoformat()} by `training/package_model.py`.', '',
    ]
    return '\n'.join(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description='Check, gate and package a trained aksar model (no upload).')
    ap.add_argument('run', help='train_crnn.py output directory')
    ap.add_argument('--results', nargs='+', required=True, help='aksar-bench --json files for this model')
    ap.add_argument('--name', required=True, help='release name, e.g. print-v1')
    ap.add_argument('--out', default='release', help='parent directory for <name>/ (default: release)')
    ap.add_argument('--no-check', action='store_true', help='skip the rten load and parity check')
    ap.add_argument('--allow-failed-gate', action='store_true', help='package even if the gate fails (test builds)')
    a = ap.parse_args(argv)
    if not a.name or os.sep in a.name or a.name in ('.', '..') or (os.altsep and os.altsep in a.name):
        fail('--name must be a plain folder name')

    for f in RUN_FILES:
        if not os.path.isfile(os.path.join(a.run, f)):
            fail(f'{os.path.join(a.run, f)} is missing (finish training, or re-export with export_onnx.py)')
    with open(os.path.join(a.run, 'manifest.json'), encoding='utf-8') as f:
        manifest = json.load(f)
    model_path = os.path.join(a.run, 'model.onnx')
    sha, size = sha256_file(model_path), os.path.getsize(model_path)
    recorded = manifest.get('model', {}).get('sha256')
    if recorded is None:
        fail('manifest.json has no model checksum: the run did not finish (train_crnn.py writes it at the end)')
    if recorded != sha:
        fail('model.onnx is not the model manifest.json describes (re-exported or edited after training?)')

    if not a.no_check:
        cmd = ['cargo', 'run', '--quiet', '--release', '-p', 'aksar-recog', '--example', 'check_model', '--',
               os.path.abspath(a.run)]
        try:
            r = subprocess.run(cmd, cwd=ROOT)
        except FileNotFoundError:
            fail('cargo not found: install Rust, or pass --no-check after running check_model elsewhere')
        if r.returncode != 0:
            fail('check_model failed (see above)')

    rows, sizes = load_results(a.results)
    if sizes and sizes != {size}:
        fail(f'the results were measured on a {sorted(sizes)} byte model, this one is {size} bytes')
    gates = gate(rows)
    passed = all(not p for *_, p in gates)
    for s, ga, t, problems in gates:
        print(f"{'pass' if not problems else 'FAIL'}  {s}: aksar {ga['cer']:.2f}%"
              + (f", {t['engine']} {t['cer']:.2f}%" if t else '') + ''.join(f'; {p}' for p in problems))
    if not passed and not a.allow_failed_gate:
        fail('the gate failed: not packaged (use --allow-failed-gate only for a test build)')

    log_rows = []
    log_path = os.path.join(a.run, 'log.tsv')
    if os.path.exists(log_path):
        with open(log_path, encoding='utf-8') as f:
            log_rows = [l.rstrip('\n').split('\t') for l in f if l.strip() and not l.startswith('epoch')]

    dest = os.path.join(a.out, a.name)
    if os.path.exists(dest):
        fail(f'{dest} exists: pick a new --name or remove it')
    tmp = dest + '.tmp'
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    for f in RUN_FILES:
        shutil.copyfile(os.path.join(a.run, f), os.path.join(tmp, f))
    with open(os.path.join(tmp, 'results.json'), 'w', encoding='utf-8') as f:
        json.dump({'schema': 1, 'gate_passed': passed, 'rows': rows}, f, ensure_ascii=False, indent=1)
        f.write('\n')
    with open(os.path.join(tmp, 'MODEL_CARD.md'), 'w', encoding='utf-8') as f:
        f.write(card(a.name, manifest, gates, passed, log_rows, size, sha))
    files = sorted(os.listdir(tmp))
    with open(os.path.join(tmp, 'SHA256SUMS'), 'w', encoding='ascii') as f:
        for name in files:
            f.write(f'{sha256_file(os.path.join(tmp, name))}  {name}\n')
    os.replace(tmp, dest)
    print(f"{'packaged' if passed else 'packaged (GATE NOT PASSED, test build only)'}: {dest}\n"
          f'next: fill in "To write before release" in {os.path.join(dest, "MODEL_CARD.md")}')


if __name__ == '__main__':
    main()
