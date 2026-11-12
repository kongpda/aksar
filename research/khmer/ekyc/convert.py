#!/usr/bin/env python3
"""Convert the EKYC Khmer OCR benchmark (LabelMe JSON + images, MIT licensed,
github.com/EKYCSolutions/khmer-ocr-benchmark-dataset) into line crops with
ground truth, for research/khmer/tesseract_bench.py (--psm 7) and
error_report.py.

Usage: convert.py <downloaded-level-folder> <out-dir>

Every LabelMe shape (polygon or rectangle) with a text label becomes
<out>/<task>_<image>_<n>.png + .gt.txt (label normalized to NFC), cropped to
the shape's bounding box plus 4 px. A manifest.tsv (file, lang, dpi,
category) records the task folder as the category.
Needs Pillow.
"""
import json, os, sys, unicodedata
from PIL import Image, ImageOps

def main():
    src, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    rows, n_lines = [], 0
    for root, _, files in os.walk(src):
        for f in sorted(files):
            if not f.endswith('.json'):
                continue
            meta = json.load(open(os.path.join(root, f), encoding='utf-8'))
            img_path = os.path.join(root, meta.get('imagePath', f[:-5] + '.png'))
            if not os.path.exists(img_path):
                # Some labels point at another folder or extension: use the image beside the JSON.
                for ext in ('.tiff', '.tif', '.jpg', '.JPG', '.jpeg', '.png'):
                    if os.path.exists(os.path.join(root, f[:-5] + ext)):
                        img_path = os.path.join(root, f[:-5] + ext)
                        break
            if not os.path.exists(img_path):
                print(f'missing image for {f}', file=sys.stderr)
                continue
            # Phone photos carry an EXIF rotation; LabelMe draws boxes on the rotated view.
            img = ImageOps.exif_transpose(Image.open(img_path)).convert('L')
            # Some labels were drawn on a larger copy of the image: scale the boxes.
            sx = img.width / (meta.get('imageWidth') or img.width)
            sy = img.height / (meta.get('imageHeight') or img.height)
            if abs(sx - sy) > 0.02 * max(sx, sy):
                print(f'skipped {f}: image {img.size} and labels have different shapes', file=sys.stderr)
                continue
            rel = os.path.relpath(root, src)
            task = 'root' if rel == '.' else rel.replace(os.sep, '_')
            for k, shape in enumerate(meta.get('shapes', [])):
                # Labels mark word gaps with a literal backslash-t.
                text = (shape.get('label') or '').replace('\\t', ' ').replace('\\n', ' ').replace('\u200b', '')
                text = unicodedata.normalize('NFC', ' '.join(text.split()))
                pts = shape.get('points') or []
                if not text or len(pts) < 2:
                    continue
                xs, ys = [p[0] * sx for p in pts], [p[1] * sy for p in pts]
                box = (max(0, int(min(xs)) - 4), max(0, int(min(ys)) - 4),
                       min(img.width, int(max(xs)) + 4), min(img.height, int(max(ys)) + 4))
                if box[2] <= box[0] or box[3] <= box[1]:
                    continue
                stem = f'{task}_{os.path.splitext(f)[0]}_{k:03d}'
                img.crop(box).save(os.path.join(out, stem + '.png'))
                open(os.path.join(out, stem + '.gt.txt'), 'w', encoding='utf-8').write(text + '\n')
                khm = any('ក' <= c <= '៿' for c in text)
                rows.append((stem + '.png', 'khm+eng' if khm else 'eng', '300', task))
                n_lines += 1
    with open(os.path.join(out, 'manifest.tsv'), 'w', encoding='utf-8') as m:
        m.write('file\tlang\tdpi\tcategory\n')
        for r in rows:
            m.write('\t'.join(r) + '\n')
    print(f'{n_lines} lines written to {out}', file=sys.stderr)

if __name__ == '__main__':
    main()
