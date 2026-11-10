"""Builds probe.ttf: maps the complex scripts to one blank glyph plus a dotted
circle, so HarfBuzz/harfrust can report broken clusters for any of them without
a real font. Measured: identical dotted-circle decisions to Noto fonts on 6,000
Khmer and 5,000 each Burmese, Devanagari, Thai and Lao random sequences.

    pip install fonttools && python make_probe_font.py   # -> probe.ttf (~5 KB)
"""
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

# Full coverage matters: HarfBuzz only decomposes split vowels (e.g. Khmer ោ)
# when the font maps the characters, which changes which clusters are broken.
RANGES = [(0x20, 0x7E), (0xA0, 0xA0), (0x0900, 0x0DFF), (0x0E00, 0x0EFF), (0x1000, 0x109F),
          (0x1780, 0x17FF), (0x19E0, 0x19FF), (0x1A20, 0x1AAF), (0x1B00, 0x1B7F),
          (0x200B, 0x200D), (0x25CC, 0x25CC), (0xA9E0, 0xA9FF), (0xAA60, 0xAA7F)]
cmap = {u: ("dottedcircle" if u == 0x25CC else "blank") for a, b in RANGES for u in range(a, b + 1)}

def box():
    p = TTGlyphPen(None)
    p.moveTo((100, 0)); p.lineTo((100, 500)); p.lineTo((400, 500)); p.lineTo((400, 0)); p.closePath()
    return p.glyph()

fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder([".notdef", "dottedcircle", "blank"])
fb.setupCharacterMap(cmap)
fb.setupGlyf({".notdef": box(), "dottedcircle": box(), "blank": TTGlyphPen(None).glyph()})
fb.setupHorizontalMetrics({".notdef": (500, 100), "dottedcircle": (500, 100), "blank": (500, 0)})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "aksar-probe", "styleName": "Regular"})
fb.setupOS2(); fb.setupPost()
fb.save("probe.ttf")
