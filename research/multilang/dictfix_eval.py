import sys, time, unicodedata as ud
sys.path.insert(0, sys.argv[1]); from dictfix import Fixer, load
def clean(s): return [c for c in ud.normalize("NFC", s) if not c.isspace() and c not in "​‌‍"]
def lev(a, b):
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a):
        cur = [i + 1]
        for j, cb in enumerate(b): cur.append(min(prev[j+1] + 1, cur[j] + 1, prev[j] + (ca != cb)))
        prev = cur
    return prev[-1]
cfg = {"khm": ("khmerdict", 0x1780, 0x17FF), "tha": ("thaidict", 0x0E00, 0x0E7F), "lao": ("laodict", 0x0E80, 0x0EFF), "mya": ("burmesedict", 0x1000, 0x109F)}
lang = sys.argv[2]; d, lo, hi = cfg[lang]
f = Fixer(load(f"{sys.argv[1]}/icudict/{d}.txt"), lo, hi)
tot = before = after = fixed_tokens = 0; t0 = time.time()
for pg in (0, 1):
    gt = open(f"{lang}_{pg}.gt.txt").read(); got = open(f"{lang}_{pg}.txt").read()
    out = []
    for line in got.splitlines():
        toks = []
        for tok in line.split(" "):
            r = f.check(ud.normalize("NFC", tok)) if tok else None
            if r: fixed_tokens += 1
            toks.append(r or tok)
        out.append(" ".join(toks))
    a = clean(gt); tot += len(a); before += lev(a, clean(got)); after += lev(a, clean("\n".join(out)))
print(f"{lang}: CER {100*before/tot:.2f}% -> {100*after/tot:.2f}%  ({fixed_tokens} tokens changed, {time.time()-t0:.0f}s)")
