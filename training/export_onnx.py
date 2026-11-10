#!/usr/bin/env python3
"""Export trained weights to ONNX (what train_crnn.py does at the end), e.g. to test
a checkpoint while training is still running.

Usage: export_onnx.py <weights.pt> <alphabet.json> <out.onnx>
"""
import json, os, sys

import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from train_crnn import CRNN, HEIGHT  # noqa: E402

weights, alphabet, out = sys.argv[1:4]
model = CRNN(len(json.load(open(alphabet, encoding='utf-8'))))
model.load_state_dict(torch.load(weights, weights_only=True))
model.eval()
torch.onnx.export(model, torch.ones(1, 1, HEIGHT, 256), out, input_names=['line'], output_names=['logprobs'],
                  dynamic_axes={'line': {3: 'width'}, 'logprobs': {0: 'steps'}}, opset_version=17, dynamo=False)
print(f'wrote {out}')
