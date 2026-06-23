#!/usr/bin/env python3
"""Phase 1b: graded Thai OCR accuracy (CER) — PaddleOCR vs vision-LLM.

Ground truth without manual labeling: for a CLEAN PDF page (no CMap garble) the
pdfium text layer is correct, so it serves as reference. `dump_page_pngs` writes
both the page PNG and its `.gt.txt`; this harness OCRs the PNG with each engine
and scores Character Error Rate against the GT.

CER = edit_distance(ocr, gt) / len(gt), both whitespace-stripped (Thai has no
word spaces, and OCR vs text-layer differ in line wrapping / the text layer's own
spurious spaces — comparing the non-whitespace character sequence is the fair
measure). Lower is better.

Usage:
  THAIRAG_API_KEY=... python3 scripts/bench/ocr_eval_cer.py /tmp/ocr_eval
"""
import os
import sys
import glob
import json

import Levenshtein

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ocr_vs_vlm import paddle_text, vlm_text  # reuse the engine callers

MIN_THAI = 200       # GT must have real Thai content
MAX_GARBLE_RATIO = 0.01  # and a trustworthy (non-corrupted) text layer


def norm(s):
    return "".join(ch for ch in s if not ch.isspace())


def thai_alien(s):
    thai = sum(1 for c in s if 0x0E00 <= ord(c) <= 0x0E7F)
    alien = sum(1 for c in s if 0x0100 <= ord(c) <= 0x024F)
    return thai, alien


def cer(hyp, ref):
    """Sequence CER (order-sensitive)."""
    rn, hn = norm(ref), norm(hyp)
    if not rn:
        return None
    return Levenshtein.distance(hn, rn) / len(rn)


def bag_cer(hyp, ref):
    """Order-independent CER: edit distance on the SORTED character multiset.
    Isolates "read the right characters" from "in the right order" — if this is
    low while sequence CER is high, the errors are reading-order / page-furniture
    differences vs the text layer, not character recognition errors."""
    rn = "".join(sorted(norm(ref)))
    hn = "".join(sorted(norm(hyp)))
    if not rn:
        return None
    return Levenshtein.distance(hn, rn) / len(rn)


def main():
    if len(sys.argv) < 2:
        print("usage: ocr_eval_cer.py <dir-with-gt>")
        return
    d = sys.argv[1]
    base = os.environ.get("VLM_BASE", "https://llm.jay-tech-ai.com/v1")
    model = os.environ.get("VLM_MODEL", "qwen2.5-vl-7b")
    key = os.environ.get("THAIRAG_API_KEY", "")
    do_vlm = bool(key)

    # Select clean GT pages only.
    pages = []
    for gt in sorted(glob.glob(os.path.join(d, "*.gt.txt"))):
        png = gt[:-7] + ".png"
        if not os.path.exists(png):
            continue
        ref = open(gt).read()
        thai, alien = thai_alien(ref)
        if thai >= MIN_THAI and (alien / thai if thai else 1) < MAX_GARBLE_RATIO:
            pages.append((png, ref))
    if not pages:
        print("no clean GT pages found in", d)
        return
    print(f"{len(pages)} clean GT pages")

    from paddleocr import PaddleOCR
    ocr = PaddleOCR(lang="th", use_textline_orientation=False)
    paddle_text(ocr, pages[0][0])  # warmup

    rows = []
    print(f"\n{'page':<22} {'pdl_seq':>8} {'pdl_bag':>8} {'vlm_seq':>8} {'vlm_bag':>8}")
    print("-" * 58)
    for png, ref in pages:
        name = os.path.basename(png)[:20]
        ptxt = paddle_text(ocr, png)
        pc, pb = cer(ptxt, ref), bag_cer(ptxt, ref)
        row = {"page": name, "gt_chars": len(norm(ref)),
               "paddle_cer": round(pc, 4), "paddle_bag_cer": round(pb, 4)}
        vc = vb = None
        if do_vlm:
            try:
                vtxt = vlm_text(png, base, model, key)
                vc, vb = cer(vtxt, ref), bag_cer(vtxt, ref)
                row["vlm_cer"], row["vlm_bag_cer"] = round(vc, 4), round(vb, 4)
            except Exception as e:
                row["vlm_error"] = str(e)[:60]
        fmt = lambda x: ("%.4f" % x) if x is not None else "ERR"
        print(f"{name:<22} {fmt(pc):>8} {fmt(pb):>8} {fmt(vc):>8} {fmt(vb):>8}")
        rows.append(row)

    def mean(key):
        xs = [r[key] for r in rows if key in r]
        return (sum(xs) / len(xs), len(xs)) if xs else (None, 0)

    print("\n=== mean CER (lower is better) — seq=order-sensitive, bag=order-independent ===")
    for label, sk, bk in [("PaddleOCR(th)", "paddle_cer", "paddle_bag_cer"),
                          (model, "vlm_cer", "vlm_bag_cer")]:
        s, n = mean(sk)
        b, _ = mean(bk)
        if s is not None:
            print(f"  {label:<16} seq={s:.4f}  bag={b:.4f}  (n={n})")
    print("\n=== JSON ===")
    print(json.dumps({"model": model, "rows": rows}, ensure_ascii=False))


if __name__ == "__main__":
    main()
