"""Prototype of aksar's single-mark dictionary fixer for Thai, Lao, Burmese (and Khmer as control)."""
import sys, unicodedata as ud, regex
MAXW = 14
def load(path):
    w = set()
    for l in open(path, encoding="utf-8-sig"):
        l = l.strip()
        if l and not l.startswith("#"): w.add(ud.normalize("NFC", l.split("\t")[0]))
    return w
def graphemes(s): return regex.findall(r"\X", s)
def in_script(c, lo, hi): return lo <= ord(c) <= hi
class Fixer:
    def __init__(self, words, lo, hi):
        self.w, self.lo, self.hi = words, lo, hi
        self.maxlen = max(len(x) for x in words)
        self.marks = sorted({c for x in words for c in x if ud.category(c) in ("Mn", "Mc")})
    def wordy(self, g): return any(in_script(c, self.lo, self.hi) and ud.category(c)[0] in "LM" for c in g)
    def cost(self, cl):
        n = len(cl); best = [(10**9, 10**9)] * (n + 1); best[0] = (0, 0)
        for i in range(n):
            u, k = best[i]
            if u >= 10**9: continue
            single = (u + (1 if self.wordy(cl[i]) and cl[i] not in self.w else 0), k + 1)
            if single < best[i + 1]: best[i + 1] = single
            if not self.wordy(cl[i]): continue
            s = cl[i]
            for j in range(i + 1, min(n, i + MAXW)):
                s += cl[j]
                if len(s) > self.maxlen: break
                if s in self.w and (u, k + 1) < best[j + 1]: best[j + 1] = (u, k + 1)
        return best[n]
    def check(self, token):
        cl = graphemes(token)
        if self.cost(cl)[0] == 0: return None
        chars = list(token); found = {}
        cands = []
        for i, c in enumerate(chars):
            if c in self.marks:
                cands.append(chars[:i] + chars[i+1:])
                for m in self.marks:
                    if m != c: cands.append(chars[:i] + [m] + chars[i+1:])
            if ud.category(c) == "Lo" and in_script(c, self.lo, self.hi):
                for m in self.marks: cands.append(chars[:i+1] + [m] + chars[i+1:])
        for v in cands:
            s = ud.normalize("NFC", "".join(v))
            if s == token or s in found: continue
            c = self.cost(graphemes(s))
            if c[0] == 0: found[s] = c[1]
        if not found: return None
        few = min(found.values()); win = [s for s, k in found.items() if k == few]
        return win[0] if len(win) == 1 else None
