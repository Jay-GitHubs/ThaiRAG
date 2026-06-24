"""PaddleOCR Thai sidecar — deterministic OCR tier-2 for the ThaiRAG document
pipeline (Phase 3 of docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md).

A tiny HTTP wrapper around PaddleOCR's `th_PP-OCRv5` model: POST a rendered page
image, get back the transcribed text in reading order. Deterministic, local, no
hallucination — used for OCR-needing pages (scanned / corrupted-CMap text) while
the vision LLM is reserved for figure description.

Run locally:  uvicorn app:app --host 0.0.0.0 --port 8086
Validated: PaddleOCR Thai = 94.5% char accuracy vs VLM 90.1% (docs/OCR_VS_VLM_SPIKE.md).
"""
import io
import os
import warnings

warnings.filterwarnings("ignore")
os.environ.setdefault("GLOG_minloglevel", "3")

from fastapi import FastAPI, Request, HTTPException
from fastapi.responses import JSONResponse

app = FastAPI(title="ThaiRAG PaddleOCR sidecar")

# Single shared engine (models download once at import / first init).
_ocr = None


def get_ocr():
    global _ocr
    if _ocr is None:
        from paddleocr import PaddleOCR

        lang = os.environ.get("OCR_LANG", "th")
        # Lightweight pipeline: skip the heavy doc-orientation (PP-LCNet) and
        # unwarping (UVDoc) stages and use the MOBILE detection model instead of
        # the server one. The full default pipeline OOM-kills a modest container;
        # this keeps memory low with no Thai-accuracy loss on clean renders.
        # Pin BOTH the mobile detection and the Thai mobile recognition model:
        # overriding the detector alone makes PaddleOCR default the recognizer to a
        # non-Thai model (garbage output), so the Thai rec model must be explicit.
        rec = "th_PP-OCRv5_mobile_rec" if lang == "th" else None
        kwargs = dict(
            lang=lang,
            use_textline_orientation=False,
            use_doc_orientation_classify=False,
            use_doc_unwarping=False,
            text_detection_model_name="PP-OCRv5_mobile_det",
        )
        if rec:
            kwargs["text_recognition_model_name"] = rec
        _ocr = PaddleOCR(**kwargs)
    return _ocr


def _extract_text(result) -> str:
    out = []
    for r in result:
        if isinstance(r, dict):
            out += r.get("rec_texts", []) or []
        elif isinstance(r, list):
            for line in r:
                try:
                    out.append(line[1][0])
                except Exception:
                    pass
    return " ".join(out)


@app.get("/health")
def health():
    # Touch the engine so readiness reflects model availability.
    try:
        get_ocr()
        return {"status": "ok", "lang": os.environ.get("OCR_LANG", "th")}
    except Exception as e:  # pragma: no cover
        return JSONResponse(status_code=503, content={"status": "error", "detail": str(e)[:200]})


@app.post("/ocr")
async def ocr(request: Request):
    """Body: raw image bytes (image/png or image/jpeg). Returns {"text": "..."}."""
    data = await request.body()
    if not data:
        raise HTTPException(status_code=400, detail="empty body")
    # PaddleOCR predict() takes a path or ndarray; decode via PIL → ndarray.
    try:
        import numpy as np
        from PIL import Image

        img = Image.open(io.BytesIO(data)).convert("RGB")
        arr = np.array(img)
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"bad image: {str(e)[:120]}")
    try:
        ocr = get_ocr()
        try:
            result = ocr.predict(arr)
        except AttributeError:
            result = ocr.ocr(arr)
        return {"text": _extract_text(result)}
    except Exception as e:  # pragma: no cover
        raise HTTPException(status_code=500, detail=f"ocr failed: {str(e)[:200]}")
