# Multi-language research (2026-10-18)

Scripts behind `docs/LANGUAGES.md`. Inputs: UDHR texts from
`unicode-org/udhr` (`data/udhr/udhr_<lang>.xml`), ICU word lists from
`unicode-org/icu` (`icu4c/source/data/brkitr/dictionaries/*dict.txt`), Noto fonts,
pinned tessdata packs, Tesseract 5.3.4.

| file | what |
|------|------|
| `make_probe_font.py` | 5 KB font that lets harfrust/HarfBuzz flag broken clusters in any complex script |
| `dictfix.py`, `dictfix_eval.py` | prototype single-mark dictionary fixer for Thai/Lao/Burmese, CER before/after on Tesseract output |
