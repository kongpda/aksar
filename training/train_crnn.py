#!/usr/bin/env python3
"""Train aksar's Khmer line recognizer (CNN + BiLSTM + CTC) on aksar-render lines.

Usage:
  train_crnn.py <train-dir> <val-dir> <out-dir> [--epochs 10] [--batch 32] [--threads 4]
                [--init weights.pt [--init-alphabet old-alphabet.json]] [--resume]

<train-dir>/<val-dir>: output of `cargo run -p aksar-render --example make_lines`
(PNG files + labels.tsv). Writes to <out-dir>:
  alphabet.json   the output symbols (index 0 = CTC blank): Unicode code points
  model.pt        best weights (lowest validation CER)
  model.onnx      exported with a dynamic line width (input: 1x1x48xW, 0..1, white=1)
  log.tsv         epoch, train loss, validation CER
  state.pt        model + optimizer + schedule, after each epoch and every
                  --checkpoint-mins inside one (for --resume)
  manifest.json   data sources, counts and settings (needed before any model release)

The model emits log-probabilities [T, 1, V]; decode them with aksar-decode
(grammar-constrained beam search). See docs/OCR_PLAN.md §4.1 and §6.
"""
import argparse, json, math, os, random, sys, time, unicodedata

import numpy as np
import torch
import torch.nn as nn
from PIL import Image

HEIGHT = 48


def load(dir_):
    rows = []
    for line in open(os.path.join(dir_, 'labels.tsv'), encoding='utf-8'):
        if '\t' not in line:
            continue  # blank or malformed line
        f, text, *rest = line.rstrip('\n').split('\t')
        rows.append((os.path.join(dir_, f), unicodedata.normalize('NFC', text), rest[0] if rest else ''))
    return rows


def scaled_width(path):
    """Width after scaling to HEIGHT (reads only the PNG header)."""
    with Image.open(path) as img:
        return max(1, round(img.width * HEIGHT / img.height))


def image(path):
    # Same as aksar-recog: scale to HEIGHT (Pillow bilinear, which aksar_image::fit_height
    # matches); narrow lines are padded with paper by collate, not stretched.
    img = Image.open(path).convert('L')
    img = img.resize((max(1, round(img.width * HEIGHT / img.height)), HEIGHT), Image.BILINEAR)
    return np.asarray(img, dtype=np.float32) / 255.0


# Symbols the model always has, seen in training or not, so new data rarely
# needs a new alphabet: printable ASCII, Khmer, Khmer symbols, common quotes.
BASE_SYMBOLS = ([chr(c) for c in range(0x20, 0x7F)] + [chr(c) for c in range(0x1780, 0x17DE)]
                + [chr(c) for c in range(0x17E0, 0x17EA)] + [chr(c) for c in range(0x17F0, 0x17FA)]
                + [chr(c) for c in range(0x19E0, 0x1A00)] + list('«»“”‘’…–—•'))


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
        return self.out(f).log_softmax(-1)    # T x B x V


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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('train'); ap.add_argument('val'); ap.add_argument('out')
    ap.add_argument('--epochs', type=int, default=10); ap.add_argument('--batch', type=int, default=32)
    ap.add_argument('--threads', type=int, default=4); ap.add_argument('--lr', type=float, default=1e-3)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--init', help='start from these weights, new schedule')
    ap.add_argument('--init-alphabet', help="the --init model's alphabet.json when it differs: shared symbols keep their output rows")
    ap.add_argument('--resume', action='store_true', help='continue from <out>/state.pt')
    ap.add_argument('--checkpoint-mins', type=float, default=10,
                    help='also save state.pt this often inside an epoch, so a restart loses little')
    a = ap.parse_args()
    torch.set_num_threads(a.threads)
    random.seed(a.seed); torch.manual_seed(a.seed); np.random.seed(a.seed)
    os.makedirs(a.out, exist_ok=True)

    train_rows, val_rows = load(a.train), load(a.val)
    for name, rows, path in (('train', train_rows, a.train), ('val', val_rows, a.val)):
        if not rows:
            sys.exit(f'no {name} lines in {path} (missing or empty labels.tsv?)')
    alphabet = [''] + sorted({c for _, t, _ in train_rows for c in t} | set(BASE_SYMBOLS))
    index = {c: i for i, c in enumerate(alphabet)}
    json.dump(alphabet, open(os.path.join(a.out, 'alphabet.json'), 'w'), ensure_ascii=False)
    json.dump({
        'train_dir': os.path.abspath(a.train), 'val_dir': os.path.abspath(a.val),
        'train_lines': len(train_rows), 'val_lines': len(val_rows),
        'fonts': sorted({f for _, _, f in train_rows}),
        'text_source': 'aksar bundled word list (ICU, Unicode License v3) + generated digits, punctuation, Latin',
        'images': 'aksar-render make_lines (seeded), synthetic only',
        'alphabet_size': len(alphabet), 'epochs': a.epochs, 'batch': a.batch, 'lr': a.lr, 'seed': a.seed,
    }, open(os.path.join(a.out, 'manifest.json'), 'w'), ensure_ascii=False, indent=1)

    model = CRNN(len(alphabet))
    print(f'{sum(p.numel() for p in model.parameters()) / 1e6:.2f} M parameters, {len(alphabet)} symbols', flush=True)
    opt = torch.optim.AdamW(model.parameters(), lr=a.lr, weight_decay=1e-4)
    widths = [scaled_width(p) for p, _, _ in train_rows]
    val_widths = [scaled_width(p) for p, _, _ in val_rows]
    steps = a.epochs * len(batches(widths, a.batch, 0))
    if steps < 3:
        raise SystemExit('too few training steps: add data or epochs')
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, max_lr=a.lr, total_steps=steps, pct_start=max(0.1, 2 / steps))
    ctc = nn.CTCLoss(blank=0, zero_infinity=True)
    train_set, val_set = Lines(train_rows, index), Lines(val_rows, index)
    val = torch.utils.data.DataLoader(val_set, batch_sampler=batches(val_widths, 64, 0), collate_fn=collate,
                                      num_workers=2)
    best, first, skip, carried = float('inf'), 1, 0, (0.0, 0)
    state_path = os.path.join(a.out, 'state.pt')
    if a.resume and os.path.exists(state_path):
        st = torch.load(state_path, weights_only=True)
        if st.get('total_steps') != steps:
            raise SystemExit(f"state.pt was made for {st.get('total_steps')} steps, this run has {steps}: "
                             'use the same data, --epochs and --batch, or start a new out dir')
        model.load_state_dict(st['model']); opt.load_state_dict(st['opt']); sched.load_state_dict(st['sched'])
        best, first = st['best'], st['epoch'] + 1
        # A mid-epoch checkpoint: skip the batches already trained on.
        skip, carried = st.get('batch', 0), (st.get('loss_sum', 0.0), st.get('batch', 0))
        where = f'epoch {st["epoch"]}' + (f' + {skip} batches' if skip else '')
        print(f'resumed after {where} (best val CER {100 * best:.2f}%)', flush=True)
    elif a.init:
        st = torch.load(a.init, weights_only=True)
        if a.init_alphabet:
            old = {c: i for i, c in enumerate(json.load(open(a.init_alphabet, encoding='utf-8')))}
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
    log = open(os.path.join(a.out, 'log.tsv'), 'a' if first > 1 else 'w')
    def save(obj, name):
        # Atomic: export_onnx.py may read model.pt while training runs.
        tmp = os.path.join(a.out, name + '.tmp')
        torch.save(obj, tmp)
        os.replace(tmp, os.path.join(a.out, name))

    for epoch in range(first, a.epochs + 1):
        model.train()
        t0, (total, n) = time.time(), carried
        order = list(batches(widths, a.batch, a.seed * 1000 + epoch))[skip:]
        skip, carried = 0, (0.0, 0)
        train = torch.utils.data.DataLoader(train_set, batch_sampler=order, collate_fn=collate, num_workers=2)
        last_ckpt = time.time()
        for x, y, ylen, xlen, _ in train:
            logp = model(x)
            loss = ctc(logp, y, xlen.clamp(max=logp.shape[0]), ylen)
            opt.zero_grad(); loss.backward()
            nn.utils.clip_grad_norm_(model.parameters(), 5.0)
            opt.step(); sched.step()
            total += loss.item(); n += 1
            if a.checkpoint_mins > 0 and time.time() - last_ckpt > 60 * a.checkpoint_mins:
                save({'model': model.state_dict(), 'opt': opt.state_dict(), 'sched': sched.state_dict(),
                      'epoch': epoch - 1, 'batch': n, 'loss_sum': total, 'best': best, 'total_steps': steps},
                     'state.pt')
                last_ckpt = time.time()
        model.eval()
        errs = chars = 0
        with torch.no_grad():
            for x, _, _, xlen, texts in val:
                for pred, truth in zip(greedy(model(x), alphabet, xlen), texts):
                    errs += edit(pred, truth); chars += len(truth)
        cer = errs / max(1, chars)
        print(f'epoch {epoch}: loss {total / n:.3f}, val CER {100 * cer:.2f}% ({time.time() - t0:.0f} s)', flush=True)
        log.write(f'{epoch}\t{total / n:.4f}\t{cer:.4f}\n'); log.flush()
        if cer < best:
            best = cer
            save(model.state_dict(), 'model.pt')
        save({'model': model.state_dict(), 'opt': opt.state_dict(), 'sched': sched.state_dict(),
              'epoch': epoch, 'best': best, 'total_steps': steps}, 'state.pt')

    model.load_state_dict(torch.load(os.path.join(a.out, 'model.pt'), weights_only=True))
    model.eval()
    torch.onnx.export(model, torch.ones(1, 1, HEIGHT, 256), os.path.join(a.out, 'model.onnx'),
                      input_names=['line'], output_names=['logprobs'],
                      dynamic_axes={'line': {3: 'width'}, 'logprobs': {0: 'steps'}}, opset_version=17,
                      dynamo=False)
    print(f'best val CER {100 * best:.2f}%; exported model.onnx', flush=True)


if __name__ == '__main__':
    main()
