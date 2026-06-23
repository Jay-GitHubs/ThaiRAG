#!/usr/bin/env python3
"""Phase 3 spike: deterministic OCR (PaddleOCR Thai) vs vision-LLM OCR.

Renders are produced by `cargo run -p thairag-document --example dump_page_pngs`.
This script runs each page through PaddleOCR (th_PP-OCRv5) and the gateway VLM,
then reports per-page text, timing, and Thai-quality signals so we can make a
go/no-go call on adding a deterministic OCR tier.

Usage:
  THAIRAG_API_KEY=... VLM_BASE=https://llm.jay-tech-ai.com/v1 VLM_MODEL=qwen2.5-vl-7b \\
  python3 scripts/bench/ocr_vs_vlm.py /tmp/ocr_bench

Outputs a comparison table and writes full transcripts to <dir>/out/.
"""
import os
import sys
import time
import glob
import json
import base64
import warnings

warnings.filterwarnings("ignore")
os.environ.setdefault("GLOG_minloglevel", "3")

import urllib.request

GARBLE = set("ĻŀĿļ")  # Latin-Extended leakage markers from a broken ToUnicode CMap


def thai_alien_counts(s):
    thai = sum(1 for c in s if 0x0E00 <= ord(c) <= 0x0E7F)
    alien = sum(1 for c in s if 0x0100 <= ord(c) <= 0x024F)
    return thai, alien


def paddle_text(ocr, path):
    try:
        res = ocr.predict(path)
    except AttributeError:
        res = ocr.ocr(path)
    out = []
    for r in res:
        if isinstance(r, dict):
            out += r.get("rec_texts", []) or []
        elif isinstance(r, list):
            for line in r:
                try:
                    out.append(line[1][0])
                except Exception:
                    pass
    return " ".join(out)


def vlm_text(path, base, model, key):
    with open(path, "rb") as f:
        b64 = base64.b64encode(f.read()).decode()
    body = {
        "model": model,
        "max_tokens": 4096,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "Transcribe ALL text in this image exactly, in reading order. Output only the transcribed text, no commentary."},
                {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{b64}"}},
            ],
        }],
    }
    req = urllib.request.Request(
        f"{base}/chat/completions",
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
            # The gateway WAF 403s the default python-urllib User-Agent.
            "User-Agent": "curl/8.4.0",
        },
    )
    with urllib.request.urlopen(req, timeout=180) as resp:
        d = json.load(resp)
    return d["choices"][0]["message"]["content"]


def main():
    if len(sys.argv) < 2:
        print("usage: ocr_vs_vlm.py <png_dir>")
        return
    png_dir = sys.argv[1]
    pngs = sorted(glob.glob(os.path.join(png_dir, "*.png")))
    if not pngs:
        print("no PNGs in", png_dir)
        return
    out_dir = os.path.join(png_dir, "out")
    os.makedirs(out_dir, exist_ok=True)

    base = os.environ.get("VLM_BASE", "https://llm.jay-tech-ai.com/v1")
    model = os.environ.get("VLM_MODEL", "qwen2.5-vl-7b")
    key = os.environ.get("THAIRAG_API_KEY", "")
    do_vlm = bool(key)

    from paddleocr import PaddleOCR
    t = time.time()
    ocr = PaddleOCR(lang="th", use_textline_orientation=False)
    print(f"PaddleOCR(th) init: {time.time()-t:.1f}s")
    # warmup (first predict compiles graph)
    paddle_text(ocr, pngs[0])

    rows = []
    print(f"\n{'page':<28} {'engine':<9} {'secs':>6} {'chars':>6} {'thai':>6} {'garble':>6}")
    print("-" * 70)
    for p in pngs:
        name = os.path.basename(p)[:26]
        # PaddleOCR
        t = time.time()
        ptxt = paddle_text(ocr, p)
        psec = time.time() - t
        pth, _ = thai_alien_counts(ptxt)
        pg = sum(ptxt.count(c) for c in GARBLE)
        print(f"{name:<28} {'paddle':<9} {psec:>6.1f} {len(ptxt):>6} {pth:>6} {pg:>6}")
        open(os.path.join(out_dir, os.path.basename(p) + ".paddle.txt"), "w").write(ptxt)
        row = {"page": name, "paddle_secs": round(psec, 1), "paddle_chars": len(ptxt),
               "paddle_thai": pth, "paddle_garble": pg}
        # VLM
        if do_vlm:
            try:
                t = time.time()
                vtxt = vlm_text(p, base, model, key)
                vsec = time.time() - t
                vth, _ = thai_alien_counts(vtxt)
                vg = sum(vtxt.count(c) for c in GARBLE)
                print(f"{name:<28} {model[:9]:<9} {vsec:>6.1f} {len(vtxt):>6} {vth:>6} {vg:>6}")
                open(os.path.join(out_dir, os.path.basename(p) + ".vlm.txt"), "w").write(vtxt)
                row.update({"vlm_secs": round(vsec, 1), "vlm_chars": len(vtxt),
                            "vlm_thai": vth, "vlm_garble": vg})
            except Exception as e:
                print(f"{name:<28} {model[:9]:<9}  VLM error: {str(e)[:40]}")
                row["vlm_error"] = str(e)[:80]
        rows.append(row)

    print("\n=== JSON ===")
    print(json.dumps(rows, ensure_ascii=False))
    print(f"\nFull transcripts in {out_dir}/")


if __name__ == "__main__":
    main()
