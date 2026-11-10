# EKYC Khmer OCR benchmark (B0 set S4)

Real Khmer images (government documents printed and scanned, scene text, handwriting)
labelled with LabelMe. **License: MIT** (EKYC Solutions). The images are on Google Drive
(linked from the repo's README), which this project's cloud sandbox cannot reach, so download
them on your machine:

```sh
git clone https://github.com/EKYCSolutions/khmer-ocr-benchmark-dataset   # labels format, license
pip install gdown pillow
gdown --folder "https://drive.google.com/drive/folders/1D8gzeJe2S9iMHAoVAeIumHwbXSMkksWx" -O ekyc-raw
python3 research/khmer/ekyc/convert.py ekyc-raw ekyc-lines
python3 research/khmer/tesseract_bench.py <tessdata> ekyc-lines --psm 7 --out ekyc.jsonl
```

Report the results in `docs/OCR_RESULTS.md`. Don't commit the images: they are fetched, not
stored.
