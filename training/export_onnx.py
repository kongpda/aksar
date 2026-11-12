#!/usr/bin/env python3
"""Export trained weights to ONNX, as train_crnn.py does at the end: e.g. to test a
checkpoint while training is still running.

Usage: export_onnx.py <weights.pt> <alphabet.json> <out-dir>

Writes <out-dir>/model.onnx, <out-dir>/reference.txt (PyTorch's output on a fixed line,
for the rten parity check) and a copy of the alphabet as <out-dir>/alphabet.json. Then check:
  cargo run --release -p aksar-recog --example check_model -- <out-dir>
"""
import argparse, json, os, shutil, sys

import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from train_crnn import CRNN, export  # noqa: E402


def main(argv=None):
    ap = argparse.ArgumentParser(description='Export aksar CRNN weights to ONNX with a parity reference.')
    ap.add_argument('weights', help='model.pt (or a state.pt checkpoint)')
    ap.add_argument('alphabet', help='the alphabet.json the weights were trained with')
    ap.add_argument('out', help='output directory')
    a = ap.parse_args(argv)
    with open(a.alphabet, encoding='utf-8') as f:
        alphabet = json.load(f)
    if not (isinstance(alphabet, list) and alphabet and alphabet[0] == '' and all(isinstance(c, str) for c in alphabet)):
        raise SystemExit(f'{a.alphabet}: expected a JSON array of strings starting with ""')
    st = torch.load(a.weights, map_location='cpu', weights_only=True)
    st = st.get('model', st)  # state.pt holds the weights under "model"
    if st['out.weight'].shape[0] != len(alphabet):
        raise SystemExit(f"{a.weights} has {st['out.weight'].shape[0]} outputs, {a.alphabet} {len(alphabet)} symbols")
    model = CRNN(len(alphabet))
    model.load_state_dict(st)
    os.makedirs(a.out, exist_ok=True)
    path = export(model, a.out)
    dst = os.path.join(a.out, 'alphabet.json')
    if not (os.path.exists(dst) and os.path.samefile(a.alphabet, dst)):
        shutil.copyfile(a.alphabet, dst)
    print(f'wrote {path} and reference.txt\n'
          f'next: cargo run --release -p aksar-recog --example check_model -- {a.out}')


if __name__ == '__main__':
    main()
