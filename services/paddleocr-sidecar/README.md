# PaddleOCR Thai sidecar — deterministic OCR tier-2

Deterministic OCR for the ThaiRAG document pipeline (Phase 3 of
`docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md`). A small FastAPI service that wraps
PaddleOCR behind a tiny HTTP API. It pins the lightweight **mobile** PP-OCRv5
models — `PP-OCRv5_mobile_det` for detection and `th_PP-OCRv5_mobile_rec` for Thai
recognition — and skips the heavy doc-orientation / unwarping stages, keeping
memory low (the full default pipeline OOM-kills a modest container) with no
Thai-accuracy loss on clean renders. Used for OCR-needing PDF pages (scanned /
corrupted-CMap text) where it is faster, local, and more accurate on Thai than
the vision LLM (94.5% vs 90.1% char accuracy — `docs/OCR_VS_VLM_SPIKE.md`), with
no hallucination. The vision LLM is kept for figure *description*.

PaddleOCR + PaddlePaddle + the PP-OCR models are Apache-2.0 (free, commercial-OK).
It runs as a **separate service** — a clean runtime dependency, no linkage to the
Rust binary.

## API
- `GET /health` → `{"status":"ok","lang":"th"}` (touches the engine).
- `POST /ocr` — body = raw image bytes (PNG/JPEG) → `{"text":"..."}` (reading order).

## Run

Local (dev):
```
pip install -r requirements.txt
uvicorn app:app --host 0.0.0.0 --port 8086
```

Docker (opt-in compose profile — does NOT start by default):
```
docker compose --profile ocr up -d --build paddleocr
```

## Enable it in ThaiRAG (default-off)

Point the pipeline at the sidecar (env or `.env` on the `thairag` service), then
restart:
```
THAIRAG__DOCUMENT__OCR_SIDECAR_URL=http://paddleocr:8086
```
With the URL empty/unset, the OCR tier is off and PDF extraction is unchanged.
Tunable at runtime too via the `document.ocr_sidecar_url` setting (km-store).

## Notes
- First container build pre-fetches the mobile det + Thai rec models (works air-gapped after).
- `paddlepaddle` is pinned to `3.2.2` — the latest with Linux wheels across arches
  (incl. arm64); `3.3.x` is macOS-only on PyPI. `paddleocr` is pinned to `3.7.0`.
- `OCR_LANG` (default `th`) selects the recognition language; the Dockerfile sets it to `th`.
- CPU works; a GPU image is far faster for high volume (the model is small).
- Telemetry: `ocr_pages_used` in the smart-PDF processing log shows how many pages
  the OCR tier handled vs `vision_pages_used`.
