# PaddleOCR Thai sidecar — deterministic OCR tier-2 for the ThaiRAG pipeline.
FROM python:3.11-slim

# PaddleOCR/PaddlePaddle runtime deps (OpenCV, etc.).
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgl1 libglib2.0-0 libgomp1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Pre-fetch the Thai detection + recognition models at build time so the first
# request doesn't pay the download (and the image works air-gapped).
RUN python -c "import warnings; warnings.filterwarnings('ignore'); \
from paddleocr import PaddleOCR; PaddleOCR(lang='th', use_textline_orientation=False)"

COPY app.py .

ENV OCR_LANG=th
EXPOSE 8086
CMD ["uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8086"]
