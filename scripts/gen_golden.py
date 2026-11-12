"""Generate reference data from HarfBuzz (uharfbuzz) for aksar's tests.

    python -m venv .venv && .venv/bin/pip install uharfbuzz
    .venv/bin/python scripts/gen_golden.py

Writes:
  testdata/golden/noto-sans-khmer.tsv  corpus line -> HarfBuzz glyphs
  testdata/golden/validity.tsv         random sequence -> 1 if HarfBuzz inserts a dotted circle
"""
import pathlib
import random

import uharfbuzz as hb

ROOT = pathlib.Path(__file__).resolve().parent.parent
FONT = ROOT / "testdata/fonts/NotoSansKhmer-Regular.ttf"

blob = hb.Blob.from_file_path(str(FONT))
font = hb.Font(hb.Face(blob))
DOTTED = font.get_nominal_glyph(0x25CC)


def run(text):
    buf = hb.Buffer()
    # UTF-8 input so clusters are byte offsets, as in Rust.
    buf.add_utf8(text.encode("utf-8"))
    buf.direction = "ltr"
    buf.script = "Khmr"
    buf.language = "km"
    buf.flags = hb.BufferFlags.BOT | hb.BufferFlags.EOT
    hb.shape(font, buf)
    return buf


def esc(s):
    return " ".join(f"{ord(c):04X}" for c in s)


def corpus():
    out = [f"# HarfBuzz {hb.__version__} / {FONT.name}. text<TAB>gid:cluster:x_adv:x_off:y_off ..."]
    for line in (ROOT / "testdata/corpus/km.txt").read_text("utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        b = run(line)
        glyphs = " ".join(
            f"{i.codepoint}:{i.cluster}:{p.x_advance}:{p.x_offset}:{p.y_offset}"
            for i, p in zip(b.glyph_infos, b.glyph_positions)
        )
        out.append(f"{esc(line)}\t{glyphs}")
    (ROOT / "testdata/golden/noto-sans-khmer.tsv").write_text("\n".join(out) + "\n", "utf-8")


ALPHABET = (
    "កខគរសអ"            # consonants incl. RO
    "ឥឪ"                # independent vowels
    "្្្"               # coeng (weighted)
    "៉៊៌"               # shifters, robat
    "ាិីុូួើឿៀេែៃោៅ"    # dependent vowels incl. split
    "ំះៈ់៍៎៏័៑៝"        # signs
    "1-\u2010×១\u17D9"   # HarfBuzz placeholder bases: digits, hyphens, ×, phnaek muan
    "‌‍  a"
)


def validity(n=6000, seed=17):
    rng = random.Random(seed)
    seen, out = set(), ["# HarfBuzz broken-cluster reference: codepoints<TAB>1 if dotted circle inserted"]
    while len(seen) < n:
        # Mostly start from a base so valid and broken cases are both common.
        head = rng.choice("កខគរសអឥ") if rng.random() < 0.85 else ""
        s = head + "".join(rng.choice(ALPHABET) for _ in range(rng.randint(1, 5)))
        if s in seen:
            continue
        seen.add(s)
        broken = any(i.codepoint == DOTTED for i in run(s).glyph_infos)
        out.append(f"{esc(s)}\t{int(broken)}")
    (ROOT / "testdata/golden/validity.tsv").write_text("\n".join(out) + "\n", "utf-8")


if __name__ == "__main__":
    corpus()
    validity()
