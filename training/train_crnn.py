#!/usr/bin/env python3
"""Train aksar's Khmer line recognizer (CNN + BiLSTM + CTC) on line images.

Usage:
  train_crnn.py <train-dir>[,<train-dir>...] <val-dir> <out-dir>
                [--epochs 10] [--batch 32] [--device auto] [--amp]
                [--holdout <test-dir>]... [--init weights.pt [--init-alphabet a.json]] [--resume]

A line set is a directory with a `labels.tsv` (`file<TAB>text[<TAB>font]`, what
`cargo run -p aksar-render --example make_lines` and `training/split_set.py` write)
or `<stem>.png` + `<stem>.gt.txt` pairs (what research/khmer/ekyc/convert.py writes).
Give several training sets separated by commas, e.g. synthetic `real` and `noisy`
lines plus the EKYC train split.

`--holdout` names a test set (repeatable). Training refuses to start if any training
or validation image is byte-identical to one of its images, so a test split cannot
leak into training by mistake. Training also refuses validation images that are in
the training data.

Writes to <out-dir>:
  alphabet.json   the output symbols (index 0 = CTC blank): Unicode code points
  model.pt        best weights (lowest validation CER)
  model.onnx      exported with a dynamic line width (input: 1x1x48xW, 0..1, white=1)
  reference.txt   PyTorch's output on a fixed test line, for the rten parity check
                  (`cargo run -p aksar-recog --example check_model -- <out-dir>`)
  log.tsv         epoch, train loss, validation CER, seconds
  state.pt        model + optimizer + schedule, after each epoch and every
                  --checkpoint-mins inside one (for --resume)
  manifest.json   data (with checksums), settings, environment and results; needed
                  before any model release (training/package_model.py reads it)

The model emits log-probabilities [T, 1, V]; decode them with aksar-decode
(grammar-constrained beam search). See docs/OCR_PLAN.md §4.1 and §6 and
training/README.md.
"""
import argparse, contextlib, datetime, functools, hashlib, json, math, os, platform, random, subprocess, time, unicodedata, warnings

import numpy as np
import torch
import torch.nn as nn
from PIL import Image

HEIGHT = 48
REFERENCE_WIDTH = 128  # a multiple of 4, so rten sees the pixels unpadded
OPSET = 17

# Symbols the model always has, seen in training or not, so new data rarely
# needs a new alphabet: printable ASCII, Khmer, Khmer symbols, common quotes.
BASE_SYMBOLS = ([chr(c) for c in range(0x20, 0x7F)] + [chr(c) for c in range(0x1780, 0x17DE)]
                + [chr(c) for c in range(0x17E0, 0x17EA)] + [chr(c) for c in range(0x17F0, 0x17FA)]
                + [chr(c) for c in range(0x19E0, 0x1A00)] + list('«»“”‘’…–—•'))


# --- data ----------------------------------------------------------------------------------

def load(dir_):
    """Rows (image path, NFC text, font) of a line set: labels.tsv, else .png + .gt.txt pairs."""
    tsv = os.path.join(dir_, 'labels.tsv')
    rows = []
    if os.path.exists(tsv):
        with open(tsv, encoding='utf-8') as f:
            for n, line in enumerate(f, 1):
                line = line.rstrip('\r\n')
                if not line.strip():
                    continue
                if '\t' not in line:
                    raise SystemExit(f'{tsv}:{n}: expected file<TAB>text')
                name, text, *rest = line.split('\t')
                path = os.path.normpath(os.path.join(dir_, name))
                rows.append((path, unicodedata.normalize('NFC', text), rest[0] if rest else ''))
        return rows
    if not os.path.isdir(dir_):
        raise SystemExit(f'{dir_}: not a directory')
    for name in sorted(os.listdir(dir_)):
        if not name.endswith('.png'):
            continue
        gt = os.path.join(dir_, name[:-4] + '.gt.txt')
        if os.path.exists(gt):
            with open(gt, encoding='utf-8') as f:
                text = f.read().rstrip('\r\n')
            rows.append((os.path.join(dir_, name), unicodedata.normalize('NFC', text), ''))
    return rows


def sha256_file(path, chunk=1 << 20):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(chunk), b''):
            h.update(block)
    return h.hexdigest()


# Training images do not change during a run: the overlap check and the manifest hash each once.
image_hash = functools.lru_cache(maxsize=None)(sha256_file)


def set_digest(rows):
    """One checksum for a line set: every image's bytes and its label, in order."""
    h = hashlib.sha256()
    for path, text, _ in rows:
        h.update(image_hash(path).encode())
        h.update(b'\t' + text.encode('utf-8') + b'\n')
    return h.hexdigest()


def check_disjoint(train_rows, val_rows, holdouts):
    """Refuse identical images across train, validation and held-out test sets."""
    def hashes(rows):
        return {image_hash(p): p for p, _, _ in rows}
    train = hashes(train_rows)
    val = hashes(val_rows)
    both = train.keys() & val.keys()
    if both:
        h = next(iter(both))
        raise SystemExit(f'{len(both)} validation images are also training images, e.g. {val[h]} = {train[h]}')
    for dir_ in holdouts:
        test = hashes(load(dir_))
        for name, seen in (('training', train), ('validation', val)):
            leak = seen.keys() & test.keys()
            if leak:
                h = next(iter(leak))
                raise SystemExit(f'{len(leak)} {name} images are in the held-out set {dir_}, '
                                 f'e.g. {seen[h]} = {test[h]}')


def scaled_width(path):
    """Width after scaling to HEIGHT (reads only the PNG header)."""
    with Image.open(path) as img:
        return max(1, round(img.width * HEIGHT / img.height))


def image(path):
    # Same as aksar-recog: scale to HEIGHT (Pillow bilinear, which aksar_image::fit_height
    # matches); narrow lines are padded with paper by collate, not stretched.
    with Image.open(path) as img:
        img = img.convert('L')
        img = img.resize((max(1, round(img.width * HEIGHT / img.height)), HEIGHT), Image.BILINEAR)
    return np.asarray(img, dtype=np.float32) / 255.0


def batches(widths, batch, seed):
    """Index batches of similar width (little padding), in random order."""
    rng = random.Random(seed)
    idx = list(range(len(widths)))
    rng.shuffle(idx)
    out = []
    for g in range(0, len(idx), batch * 50):
        group = sorted(idx[g:g + batch * 50], key=lambda i: widths[i])
        out += [group[i:i + batch] for i in range(0, len(group), batch)]
    rng.shuffle(out)
    return out


class Lines(torch.utils.data.Dataset):
    def __init__(self, rows, index):
        self.rows, self.index = rows, index

    def __len__(self):
        return len(self.rows)

    def __getitem__(self, i):
        path, text, _ = self.rows[i]
        return image(path), [self.index[c] for c in text if c in self.index], text


def collate(batch):
    width = max(8, max(b[0].shape[1] for b in batch))
    width = int(math.ceil(width / 4) * 4)
    x = np.ones((len(batch), 1, HEIGHT, width), dtype=np.float32)  # pad with paper
    for i, (img, _, _) in enumerate(batch):
        x[i, 0, :, :img.shape[1]] = img
    targets = torch.tensor([t for b in batch for t in b[1]], dtype=torch.long)
    lengths = torch.tensor([len(b[1]) for b in batch], dtype=torch.long)
    # Output steps that cover each line (the CNN halves the width twice).
    steps = torch.tensor([max(1, math.ceil(max(8, b[0].shape[1]) / 4)) for b in batch], dtype=torch.long)
    return torch.from_numpy(x), targets, lengths, steps, [b[2] for b in batch]


# --- model ---------------------------------------------------------------------------------

class CRNN(nn.Module):
    """About 2.1 M parameters. Width is downsampled by 4: one output per 4 px."""

    def __init__(self, n_classes):
        super().__init__()

        def block(cin, cout, pool):
            return [nn.Conv2d(cin, cout, 3, padding=1), nn.BatchNorm2d(cout), nn.ReLU(inplace=True), nn.MaxPool2d(pool)]

        self.cnn = nn.Sequential(
            *block(1, 32, (2, 2)),      # 24 x W/2
            *block(32, 64, (2, 2)),     # 12 x W/4
            *block(64, 128, (2, 1)),    # 6
            *block(128, 192, (2, 1)),   # 3
            nn.Conv2d(192, 256, (3, 1)), nn.BatchNorm2d(256), nn.ReLU(inplace=True),  # 1
        )
        self.rnn = nn.LSTM(256, 192, num_layers=2, bidirectional=True, dropout=0.1)
        self.out = nn.Linear(384, n_classes)

    def forward(self, x):                     # x: B x 1 x 48 x W
        f = self.cnn(x).squeeze(2)            # B x 256 x T
        f = f.permute(2, 0, 1)                # T x B x 256
        f, _ = self.rnn(f)
        # float32 even under autocast: CTC and the decoder need real log-probabilities.
        return self.out(f).float().log_softmax(-1)    # T x B x V


def greedy(logp, alphabet, steps):
    out = []
    for seq, n in zip(logp.argmax(-1).permute(1, 0).tolist(), steps.tolist()):
        s, prev = [], 0
        for k in seq[:n]:
            if k != 0 and k != prev:
                s.append(alphabet[k])
            prev = k
        out.append(''.join(s))
    return out


def edit(a, b):
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a):
        cur = [i + 1]
        for j, cb in enumerate(b):
            cur.append(min(prev[j + 1] + 1, cur[j] + 1, prev[j] + (ca != cb)))
        prev = cur
    return prev[-1]


# --- export --------------------------------------------------------------------------------

def reference_line(width=REFERENCE_WIDTH, seed=0):
    """A fixed, text-like test line (8-bit gray): paper, dark strokes, a little noise."""
    rng = np.random.RandomState(seed)
    px = np.full((HEIGHT, width), 245, dtype=np.int32)
    for x0 in range(4, width - 8, 11):
        w, top, bottom = rng.randint(2, 7), rng.randint(8, 20), rng.randint(28, 42)
        px[top:bottom, x0:x0 + w] = rng.randint(0, 60)
    px += rng.randint(-10, 11, size=px.shape)
    return np.clip(px, 0, 255).astype(np.uint8)


def write_reference(model, path):
    """PyTorch's log-probabilities on reference_line(), as aksar-recog's check_reference reads them."""
    px = reference_line()
    x = torch.from_numpy(px.astype(np.float32) / 255.0)[None, None]
    with torch.no_grad():
        out = model(x)[:, 0, :].numpy()
    steps, symbols = out.shape
    tmp = path + '.tmp'
    with open(tmp, 'w', encoding='ascii') as f:
        f.write('# PyTorch output on a fixed line; checked against rten by aksar-recog (check_model)\n')
        f.write(f'aksar-reference 1 {px.shape[1]} {steps} {symbols}\n')
        f.write(' '.join(str(v) for v in px.reshape(-1).tolist()) + '\n')
        for row in out:
            f.write(' '.join(repr(float(v)) for v in row) + '\n')
    os.replace(tmp, path)


def export(model, out_dir, onnx_name='model.onnx'):
    """Writes <out_dir>/model.onnx (dynamic width) and reference.txt from a CPU copy of `model`."""
    model = model.to('cpu').float().eval()
    onnx_path = os.path.join(out_dir, onnx_name)
    tmp = onnx_path + '.tmp'
    with warnings.catch_warnings():
        # The TorchScript exporter is deprecated but is the one that writes this LSTM in a
        # form rten runs (checked by the parity test); its batch-size note does not apply
        # (aksar always runs batch 1).
        warnings.simplefilter('ignore', DeprecationWarning)
        warnings.filterwarnings('ignore', message='Exporting a model to ONNX with a batch_size other than 1')
        torch.onnx.export(model, torch.ones(1, 1, HEIGHT, 256), tmp,
                          input_names=['line'], output_names=['logprobs'],
                          dynamic_axes={'line': {3: 'width'}, 'logprobs': {0: 'steps'}}, opset_version=OPSET,
                          dynamo=False)
    os.replace(tmp, onnx_path)
    write_reference(model, os.path.join(out_dir, 'reference.txt'))
    return onnx_path


# --- training ------------------------------------------------------------------------------

def pick_device(name):
    if name == 'auto':
        if torch.cuda.is_available():
            return torch.device('cuda')
        if getattr(torch.backends, 'mps', None) and torch.backends.mps.is_available():
            return torch.device('mps')
        return torch.device('cpu')
    if name == 'cuda' and not torch.cuda.is_available():
        raise SystemExit('--device cuda: no CUDA GPU visible to PyTorch')
    if name == 'mps' and not (getattr(torch.backends, 'mps', None) and torch.backends.mps.is_available()):
        raise SystemExit('--device mps: Apple GPU not available')
    return torch.device(name)


def git_commit():
    try:
        here = os.path.dirname(os.path.abspath(__file__))
        out = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=here, capture_output=True, text=True, timeout=10)
        dirty = subprocess.run(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=here,
                               capture_output=True, text=True, timeout=10).stdout.strip()
        return out.stdout.strip() + ('-dirty' if dirty else '') if out.returncode == 0 else None
    except (OSError, subprocess.SubprocessError):
        return None


def write_json(path, obj):
    tmp = path + '.tmp'
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(obj, f, ensure_ascii=False, indent=1)
        f.write('\n')
    os.replace(tmp, path)


def parse_args(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('train', help='training line set(s), comma-separated')
    ap.add_argument('val', help='validation line set (picks the best epoch)')
    ap.add_argument('out', help='output directory')
    ap.add_argument('--epochs', type=int, default=10)
    ap.add_argument('--batch', type=int, default=32)
    ap.add_argument('--lr', type=float, default=1e-3)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--device', default='auto', choices=['auto', 'cpu', 'cuda', 'mps'],
                    help='auto: CUDA, then Apple mps, then CPU')
    ap.add_argument('--amp', action='store_true', help='mixed precision on CUDA (bf16 where supported, else fp16)')
    ap.add_argument('--threads', type=int, default=4, help='CPU threads for PyTorch')
    ap.add_argument('--workers', type=int, default=min(4, os.cpu_count() or 1),
                    help='image loading processes (0 loads in the training process)')
    ap.add_argument('--holdout', action='append', default=[],
                    help='a test set that must share no image with training or validation (repeatable)')
    ap.add_argument('--init', help='start from these weights, new schedule')
    ap.add_argument('--init-alphabet', help="the --init model's alphabet.json when it differs: shared symbols keep their output rows")
    ap.add_argument('--resume', action='store_true', help='continue from <out>/state.pt')
    ap.add_argument('--checkpoint-mins', type=float, default=10,
                    help='also save state.pt this often inside an epoch, so a restart loses little')
    a = ap.parse_args(argv)
    if a.epochs < 1 or a.batch < 1 or a.workers < 0 or a.threads < 1 or not a.lr > 0:
        ap.error('--epochs, --batch and --threads must be at least 1, --workers at least 0, --lr positive')
    if a.amp and a.device not in ('auto', 'cuda'):
        ap.error('--amp needs a CUDA device')
    return a


def main(argv=None):
    a = parse_args(argv)
    started = time.time()
    torch.set_num_threads(a.threads)
    random.seed(a.seed); torch.manual_seed(a.seed); np.random.seed(a.seed)
    device = pick_device(a.device)
    amp = a.amp and device.type == 'cuda'
    amp_dtype = torch.bfloat16 if amp and torch.cuda.is_bf16_supported() else torch.float16
    os.makedirs(a.out, exist_ok=True)

    train_dirs = [d for d in a.train.split(',') if d]
    train_sets = [(d, load(d)) for d in train_dirs]
    train_rows = [r for _, rows in train_sets for r in rows]
    val_rows = load(a.val)
    for name, rows in train_sets + [(a.val, val_rows)]:
        if not rows:
            raise SystemExit(f'no lines in {name} (missing or empty labels.tsv, and no .png + .gt.txt pairs?)')
    missing = next((p for p, _, _ in train_rows + val_rows if not os.path.exists(p)), None)
    if missing:
        raise SystemExit(f'image listed but missing: {missing}')
    print('checking that train, validation and held-out sets share no image ...', flush=True)
    check_disjoint(train_rows, val_rows, a.holdout)

    alphabet = [''] + sorted({c for _, t, _ in train_rows for c in t} | set(BASE_SYMBOLS))
    index = {c: i for i, c in enumerate(alphabet)}
    dropped = sum(c not in index for _, t, _ in val_rows for c in t)
    if dropped:
        print(f'note: {dropped} validation characters are outside the alphabet (always counted as errors)', flush=True)

    state_path = os.path.join(a.out, 'state.pt')
    resuming = a.resume and os.path.exists(state_path)
    alphabet_path = os.path.join(a.out, 'alphabet.json')
    if resuming and os.path.exists(alphabet_path):
        with open(alphabet_path, encoding='utf-8') as f:
            if json.load(f) != alphabet:
                raise SystemExit('the training data now gives a different alphabet than the run being resumed')
    write_json(alphabet_path, alphabet)

    manifest_path = os.path.join(a.out, 'manifest.json')
    manifest = {
        'format': 1,
        'architecture': 'CRNN: 5 conv + 2-layer BiLSTM (192) + linear, CTC, log-softmax output',
        'input': f'1x1x{HEIGHT}xW gray, 0..1, white paper = 1, W a multiple of 4; output [W/4, 1, symbols]',
        'train': [{'dir': os.path.abspath(d), 'lines': len(rows), 'sha256': set_digest(rows),
                   'fonts': sorted({f for _, _, f in rows if f})} for d, rows in train_sets],
        'val': {'dir': os.path.abspath(a.val), 'lines': len(val_rows), 'sha256': set_digest(val_rows)},
        'holdout': [{'dir': os.path.abspath(d), 'lines': len(load(d))} for d in a.holdout],
        'text_source': 'aksar bundled word list (ICU, Unicode License v3) + generated digits, punctuation, '
                       'Latin; plus the labels of any real line sets listed under train',
        'alphabet_size': len(alphabet),
        'settings': {k: v for k, v in vars(a).items() if k not in ('train', 'val', 'out', 'holdout')},
        'environment': {'git_commit': git_commit(), 'python': platform.python_version(), 'torch': torch.__version__,
                        'device': str(device) + (f' ({torch.cuda.get_device_name(device)})' if device.type == 'cuda' else ''),
                        'amp': str(amp_dtype).replace('torch.', '') if amp else None, 'platform': platform.platform()},
        'started': datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='seconds'),
    }
    if resuming and os.path.exists(manifest_path):
        with open(manifest_path, encoding='utf-8') as f:
            old = json.load(f)
        manifest['started'] = old.get('started', manifest['started'])
        manifest['resumed'] = old.get('resumed', []) + [datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='seconds')]
    write_json(manifest_path, manifest)

    model = CRNN(len(alphabet))
    print(f'{sum(p.numel() for p in model.parameters()) / 1e6:.2f} M parameters, {len(alphabet)} symbols, '
          f'{len(train_rows)} training lines, device {device}' + (f', {amp_dtype}' if amp else ''), flush=True)
    widths = [scaled_width(p) for p, _, _ in train_rows]
    val_widths = [scaled_width(p) for p, _, _ in val_rows]
    steps = a.epochs * len(batches(widths, a.batch, 0))
    if steps < 3:
        raise SystemExit('too few training steps: add data or epochs')
    if a.init and not resuming:
        st = torch.load(a.init, map_location='cpu', weights_only=True)
        if a.init_alphabet:
            with open(a.init_alphabet, encoding='utf-8') as f:
                old = {c: i for i, c in enumerate(json.load(f))}
            w, b = model.out.weight.data.clone(), model.out.bias.data.clone()
            for i, c in enumerate(alphabet):
                if c in old:
                    w[i], b[i] = st['out.weight'][old[c]], st['out.bias'][old[c]]
            st['out.weight'], st['out.bias'] = w, b
            print(f'{sum(c in old for c in alphabet)} of {len(alphabet)} symbols kept from the old model', flush=True)
        elif st['out.weight'].shape[0] != len(alphabet):
            raise SystemExit('--init model has a different alphabet: pass --init-alphabet')
        model.load_state_dict(st)
        print(f'initialized from {a.init}', flush=True)
    model.to(device)
    opt = torch.optim.AdamW(model.parameters(), lr=a.lr, weight_decay=1e-4)
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, max_lr=a.lr, total_steps=steps, pct_start=max(0.1, 2 / steps))
    scaler = torch.amp.GradScaler('cuda', enabled=amp and amp_dtype == torch.float16)
    ctc = nn.CTCLoss(blank=0, zero_infinity=True)
    # PyTorch has no CTC loss on Apple GPUs: compute that one step on the CPU there.
    loss_device = torch.device('cpu') if device.type == 'mps' else device
    loader = dict(collate_fn=collate, num_workers=a.workers, pin_memory=device.type == 'cuda',
                  persistent_workers=False)
    train_set, val_set = Lines(train_rows, index), Lines(val_rows, index)
    val = torch.utils.data.DataLoader(val_set, batch_sampler=batches(val_widths, 64, 0), **loader)
    best, first, skip, carried = float('inf'), 1, 0, (0.0, 0)
    if resuming:
        st = torch.load(state_path, map_location=device, weights_only=True)
        if st.get('total_steps') != steps:
            raise SystemExit(f"state.pt was made for {st.get('total_steps')} steps, this run has {steps}: "
                             'use the same data, --epochs and --batch, or start a new out dir')
        model.load_state_dict(st['model']); opt.load_state_dict(st['opt']); sched.load_state_dict(st['sched'])
        if 'scaler' in st:
            scaler.load_state_dict(st['scaler'])
        best, first = st['best'], st['epoch'] + 1
        # A mid-epoch checkpoint: skip the batches already trained on.
        skip, carried = st.get('batch', 0), (st.get('loss_sum', 0.0), st.get('batch', 0))
        where = f'epoch {st["epoch"]}' + (f' + {skip} batches' if skip else '')
        print(f'resumed after {where} (best val CER {100 * best:.2f}%)', flush=True)
    elif a.resume:
        print(f'no {state_path} yet: starting a new run', flush=True)
    log = open(os.path.join(a.out, 'log.tsv'), 'a' if first > 1 else 'w', encoding='utf-8')
    if first == 1:
        log.write('epoch\ttrain_loss\tval_cer\tseconds\n')

    def save(obj, name):
        # Atomic: export_onnx.py may read model.pt while training runs.
        tmp = os.path.join(a.out, name + '.tmp')
        torch.save(obj, tmp)
        os.replace(tmp, os.path.join(a.out, name))

    def state(epoch, **extra):
        return {'model': model.state_dict(), 'opt': opt.state_dict(), 'sched': sched.state_dict(),
                'scaler': scaler.state_dict(), 'epoch': epoch, 'best': best, 'total_steps': steps, **extra}

    for epoch in range(first, a.epochs + 1):
        model.train()
        t0, (total, n) = time.time(), carried
        order = list(batches(widths, a.batch, a.seed * 1000 + epoch))[skip:]
        skip, carried = 0, (0.0, 0)
        train = torch.utils.data.DataLoader(train_set, batch_sampler=order, **loader)
        last_ckpt = time.time()
        for x, y, ylen, xlen, _ in train:
            x = x.to(device, non_blocking=True)
            with torch.autocast(device_type='cuda', dtype=amp_dtype) if amp else contextlib.nullcontext():
                logp = model(x)
            xlen = xlen.clamp(max=logp.shape[0])
            loss = ctc(logp.to(loss_device), y.to(loss_device), xlen.to(loss_device), ylen.to(loss_device))
            if not torch.isfinite(loss):
                print('warning: non-finite loss, batch skipped', flush=True)
                opt.zero_grad(set_to_none=True)
                continue
            opt.zero_grad(set_to_none=True)
            scaler.scale(loss).backward()
            scaler.unscale_(opt)
            nn.utils.clip_grad_norm_(model.parameters(), 5.0)
            scaler.step(opt); scaler.update(); sched.step()
            total += loss.item(); n += 1
            if a.checkpoint_mins > 0 and time.time() - last_ckpt > 60 * a.checkpoint_mins:
                save(state(epoch - 1, batch=n, loss_sum=total), 'state.pt')
                last_ckpt = time.time()
        model.eval()
        errs = chars = 0
        with torch.no_grad():
            for x, _, _, xlen, texts in val:
                logp = model(x.to(device)).cpu()
                for pred, truth in zip(greedy(logp, alphabet, xlen), texts):
                    errs += edit(pred, truth); chars += len(truth)
        cer = errs / max(1, chars)
        secs = time.time() - t0
        print(f'epoch {epoch}/{a.epochs}: loss {total / max(1, n):.3f}, val CER {100 * cer:.2f}% ({secs:.0f} s)', flush=True)
        log.write(f'{epoch}\t{total / max(1, n):.4f}\t{cer:.4f}\t{secs:.0f}\n'); log.flush()
        if cer < best:
            best = cer
            save({k: v.detach().cpu() for k, v in model.state_dict().items()}, 'model.pt')
        save(state(epoch), 'state.pt')
    log.close()

    model = CRNN(len(alphabet))
    model.load_state_dict(torch.load(os.path.join(a.out, 'model.pt'), map_location='cpu', weights_only=True))
    onnx_path = export(model, a.out)
    manifest['finished'] = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='seconds')
    manifest['hours'] = round((time.time() - started) / 3600, 3)
    manifest['best_val_cer'] = round(best, 6)
    manifest['model'] = {'file': 'model.onnx', 'bytes': os.path.getsize(onnx_path), 'sha256': sha256_file(onnx_path),
                         'opset': OPSET}
    write_json(manifest_path, manifest)
    print(f'best val CER {100 * best:.2f}%; exported {onnx_path}\n'
          f'next: cargo run --release -p aksar-recog --example check_model -- {a.out}', flush=True)


if __name__ == '__main__':
    main()
