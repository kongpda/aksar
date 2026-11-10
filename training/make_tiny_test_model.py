"""Usage: python training/make_tiny_test_model.py testdata/models/tiny.onnx

Writes testdata/models/tiny.onnx: a 4-symbol CTC model shaped like aksar's
(input 1x1x48xW, output [W/4, 1, 4] log-probs). Weights are fixed so it reads
dark ink as symbol 1 ("ក") and paper as blank. For tests only."""
import sys, torch, torch.nn as nn
class Tiny(nn.Module):
    def __init__(self):
        super().__init__()
        self.conv = nn.Conv2d(1, 4, (48, 4), stride=(48, 4))
        with torch.no_grad():
            self.conv.weight.zero_(); self.conv.bias.zero_()
            # ink (low values) -> symbol 1, paper -> blank
            self.conv.weight[1].fill_(-1.0 / 192); self.conv.bias[1] = 1.0
            self.conv.weight[0].fill_(1.0 / 192)
            self.conv.bias[2] = -5.0; self.conv.bias[3] = -5.0
    def forward(self, x):
        f = self.conv(x).squeeze(2).permute(2, 0, 1)  # T x B x V
        return (f * 8).log_softmax(-1)
m = Tiny().eval()
torch.onnx.export(m, torch.ones(1, 1, 48, 64), sys.argv[1], input_names=['line'], output_names=['logprobs'],
                  dynamic_axes={'line': {3: 'width'}, 'logprobs': {0: 'steps'}}, opset_version=17, dynamo=False)
