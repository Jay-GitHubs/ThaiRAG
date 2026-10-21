#!/usr/bin/env python3
"""Draft gold-set candidates from a folder of PDFs, with two independent page
readings so a human reviewer only has to confirm.

Per document:
  1. render pages to PNG (pdftoppm inside the ThaiRAG image, same DPI as ingest)
  2. OCR each page with the PaddleOCR sidecar          -> page-N.ocr.txt
  3. transcribe each page with the vision LLM          -> page-N.vlm.txt
  4. draft factual Q/A pairs from the OCR text (LLM)   -> page-N.cands.json
  5. verify each candidate:
       ocr_ok   evidence quote is verbatim in the OCR text   (hard requirement)
       vlm_ok   answer is found in the vision transcript     (fuzzy, flag if not)
       shared   number of OTHER documents whose OCR text also contains the answer
  6. draft a few "unanswerable" questions per document and keep those whose
     key term appears nowhere in the corpus.

Everything is cached under WORK/<doc>/ so a rerun resumes. Stdlib only.

  python3 -I scripts/eval/gen_candidates.py --pdf-dir DIR --work scripts/eval/work \
      [--llm http://host:port/v1] [--ocr http://localhost:8086] [--per-page 5] \
      [--max-per-doc 12] [--limit-docs N] [--limit-pages N] [--skip-vlm]
"""
import argparse
import base64
import difflib
import glob
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request

# Pages are rendered inside the RUNNING ThaiRAG container (it ships pdftoppm),
# via docker cp + docker exec: `docker run` of a fresh container hung in the
# Created state on Docker Desktop here, while exec/cp against the stack work.
CONTAINER = os.environ.get("THAIRAG_CONTAINER", "thairag-thairag-1")
DPI = 150


# ── helpers ──────────────────────────────────────────────────────────────────
def norm(t):
    return re.sub(r"\s+", "", t or "").lower()


def fuzzy_in(needle, hay, threshold=0.8):
    needle, hay = norm(needle), norm(hay)
    if not needle:
        return False
    if needle in hay:
        return True
    n = len(needle)
    step = 1 if n < 12 else 2
    for i in range(0, max(1, len(hay) - n + 1), step):
        if difflib.SequenceMatcher(None, needle, hay[i : i + n + 4]).ratio() >= threshold:
            return True
    return False


def program_name(filename):
    """'ปี <year>_<n>. <name>_.pdf' -> '<name>'"""
    name = os.path.splitext(filename)[0]
    name = re.sub(r"^ปี\s*\d+_\d+\.\s*", "", name)
    return name.rstrip("_ ").strip()


def slug_for(filename):
    m = re.search(r"_(\d+)\.", filename)
    return f"doc{int(m.group(1)):02d}" if m else re.sub(r"[^A-Za-z0-9]+", "-", filename)[:40]


def cached(path, producer):
    if os.path.exists(path):
        with open(path, encoding="utf-8") as fh:
            return fh.read()
    out = producer()
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(out)
    return out


class Llm:
    def __init__(self, base):
        self.base = base.rstrip("/")

    def chat(self, messages, max_tokens=1200, json_mode=False, timeout=900, retries=3):
        body = {"model": "qwen", "messages": messages, "max_tokens": max_tokens, "temperature": 0, "seed": 42}
        if json_mode:
            body["response_format"] = {"type": "json_object"}
        req = urllib.request.Request(
            self.base + "/chat/completions", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"}
        )
        for attempt in range(retries):
            try:
                with urllib.request.urlopen(req, timeout=timeout) as r:
                    return json.loads(r.read())["choices"][0]["message"]["content"]
            except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as e:
                print(f"    llm retry {attempt + 1}: {e}", flush=True)
                time.sleep(5 * (attempt + 1))
        raise RuntimeError("llm unavailable")


def parse_json(text):
    t = text.strip()
    t = re.sub(r"^```[a-zA-Z]*\s*", "", t)
    t = re.sub(r"\s*```$", "", t)
    try:
        return json.loads(t)
    except json.JSONDecodeError:
        m = re.search(r"\{.*\}", t, re.S)
        return json.loads(m.group(0)) if m else {}


# ── page readings ────────────────────────────────────────────────────────────
def render(pdf_dir, filename, out_dir):
    os.makedirs(out_dir, exist_ok=True)
    if glob.glob(os.path.join(out_dir, "page-*.png")):
        return
    slug = os.path.basename(out_dir.rstrip("/"))
    remote = f"/tmp/render/{slug}"
    subprocess.run(["docker", "exec", CONTAINER, "/bin/sh", "-c", f"rm -rf {remote} && mkdir -p {remote}"], check=True, timeout=60)
    subprocess.run(["docker", "cp", os.path.join(pdf_dir, filename), f"{CONTAINER}:{remote}/doc.pdf"], check=True, timeout=300)
    subprocess.run(
        ["docker", "exec", CONTAINER, "pdftoppm", "-r", str(DPI), "-png", f"{remote}/doc.pdf", f"{remote}/page"],
        check=True,
        timeout=600,
    )
    subprocess.run(["docker", "cp", f"{CONTAINER}:{remote}/.", out_dir], check=True, timeout=300)
    for leftover in glob.glob(os.path.join(out_dir, "doc.pdf")):
        os.remove(leftover)
    subprocess.run(["docker", "exec", CONTAINER, "rm", "-rf", remote], check=False, timeout=60)


def page_no(png_path):
    m = re.search(r"page-(\d+)\.png$", png_path)
    return int(m.group(1)) if m else 0


def pages_of(out_dir):
    return sorted(glob.glob(os.path.join(out_dir, "page-*.png")), key=page_no)


def ocr_page(ocr_base, png_path):
    with open(png_path, "rb") as fh:
        data = fh.read()
    req = urllib.request.Request(ocr_base.rstrip("/") + "/ocr", data=data, headers={"Content-Type": "image/png"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.loads(r.read()).get("text", "")


def vlm_page(llm, png_path):
    with open(png_path, "rb") as fh:
        b64 = base64.b64encode(fh.read()).decode()
    return llm.chat(
        [
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": "ถอดความข้อความทั้งหมดในหน้านี้แบบคำต่อคำ เรียงตามลำดับการอ่าน "
                        "ไม่ต้องสรุป ไม่ต้องอธิบาย ไม่ต้องแก้คำ ให้ผลลัพธ์เป็นข้อความธรรมดา",
                    },
                    {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{b64}"}},
                ],
            }
        ],
        max_tokens=2500,
    )


# ── drafting ─────────────────────────────────────────────────────────────────
DRAFT_PROMPT = """จากข้อความ OCR ของเอกสารหนึ่งหน้าด้านล่าง (อาจมีคำผิดจาก OCR บ้าง) ให้สร้างคำถาม-คำตอบเชิงข้อเท็จจริง ไม่เกิน {n} ข้อ

กติกา:
- ถามแบบที่ลูกค้าหรือพนักงานทั่วไปจะถามจริง เป็นภาษาไทยธรรมชาติ และ "ห้าม" ใส่ชื่อโครงการ/ชื่อเอกสารในคำถาม
- ให้ความสำคัญกับข้อเท็จจริงที่แยกโครงการนี้จากโครงการอื่น: วงเงิน อัตราดอกเบี้ย ค่าธรรมเนียม ระยะเวลา LTV หลักประกัน คุณสมบัติผู้กู้ เงื่อนไขพิเศษ
- answer ต้องสั้น (ตัวเลข วันที่ ชื่อ หรือเงื่อนไขสั้นๆ) และต้องมีอยู่ในข้อความตามตัวอักษร
- evidence คือข้อความต้นฉบับ "คัดลอกตามตัวอักษร" จาก OCR ที่มี answer อยู่ (ยาว 20-150 ตัวอักษร)
- type เป็นหนึ่งใน: number, date, name, condition, table
- ข้ามข้อมูลที่ OCR อ่านไม่ออก

ตอบเป็น JSON object เดียวเท่านั้น รูปแบบ:
{{"items":[{{"question":"...","answer":"...","evidence":"...","type":"number"}}]}}

ข้อความ OCR:
{text}"""

UNANSWERABLE_PROMPT = """ด้านล่างคือข้อความ OCR ของเอกสารโครงการสินเชื่อหนึ่งฉบับ ให้สร้างคำถาม 3 ข้อที่ "ฟังดูเกี่ยวข้อง" กับเอกสารนี้ แต่เอกสาร "ไม่ได้ระบุคำตอบไว้เลย" (เช่น ถามถึงเงื่อนไข ตัวเลข หรือหัวข้อที่ไม่มีในเอกสาร)
สำหรับแต่ละข้อให้ระบุ key_term = คำหรือวลีสำคัญของคำถาม (1 คำ/วลี) ที่ถ้าค้นในเอกสารแล้วต้องไม่พบ
ห้ามใส่ชื่อโครงการในคำถาม ตอบเป็น JSON object เดียว: {{"items":[{{"question":"...","key_term":"..."}}]}}

ข้อความ OCR:
{text}"""


# The Studio's llama-server slot has an 8192-token context; Thai runs at
# roughly 1 token per 1.3 chars, so prompts are capped well below that.
DRAFT_CHARS, UNANSWERABLE_CHARS = 3500, 3000


def draft_page(llm, ocr_text, n):
    if len(norm(ocr_text)) < 80:
        return []
    out = llm.chat([{"role": "user", "content": DRAFT_PROMPT.format(n=n, text=ocr_text[:DRAFT_CHARS])}], max_tokens=1200, json_mode=True)
    items = parse_json(out).get("items", [])
    return [i for i in items if isinstance(i, dict) and i.get("question") and i.get("answer") and i.get("evidence")]


def draft_unanswerable(llm, doc_text):
    out = llm.chat([{"role": "user", "content": UNANSWERABLE_PROMPT.format(text=doc_text[:UNANSWERABLE_CHARS])}], max_tokens=600, json_mode=True)
    items = parse_json(out).get("items", [])
    return [i for i in items if isinstance(i, dict) and i.get("question") and i.get("key_term")]


def cached_or_skip(path, producer, label):
    """Like `cached`, but a failing producer logs, returns [] and caches
    nothing, so a rerun retries it instead of the whole run dying."""
    try:
        return json.loads(cached(path, lambda: json.dumps(producer(), ensure_ascii=False)))
    except Exception as e:  # noqa: BLE001 - any model/parse failure is skippable
        print(f"    skipped {label}: {str(e)[:100]}", flush=True)
        return []


# ── main ─────────────────────────────────────────────────────────────────────
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pdf-dir", required=True)
    ap.add_argument("--work", default="scripts/eval/work")
    ap.add_argument("--llm", default="http://192.168.0.200:8931/v1")
    ap.add_argument("--ocr", default="http://localhost:8086")
    ap.add_argument("--per-page", type=int, default=5)
    ap.add_argument("--max-per-doc", type=int, default=12)
    ap.add_argument("--limit-docs", type=int, default=0)
    ap.add_argument("--limit-pages", type=int, default=0)
    ap.add_argument("--skip-vlm", action="store_true")
    args = ap.parse_args()

    llm = Llm(args.llm)
    os.makedirs(args.work, exist_ok=True)
    pdfs = sorted(f for f in os.listdir(args.pdf_dir) if f.lower().endswith(".pdf"))
    if args.limit_docs:
        pdfs = pdfs[: args.limit_docs]

    docs = {}
    for f in pdfs:
        slug = slug_for(f)
        out_dir = os.path.join(args.work, slug)
        t0 = time.time()
        render(args.pdf_dir, f, out_dir)
        pages = pages_of(out_dir)
        if args.limit_pages:
            pages = pages[: args.limit_pages]
        doc = {"slug": slug, "file": f, "program": program_name(f), "pages": []}
        for png in pages:
            pn = page_no(png)
            stem = os.path.join(out_dir, f"page-{pn}")
            ocr = cached(stem + ".ocr.txt", lambda: ocr_page(args.ocr, png))
            vlm = "" if args.skip_vlm else cached(stem + ".vlm.txt", lambda: vlm_page(llm, png))
            cands = cached_or_skip(stem + ".cands.json", lambda: draft_page(llm, ocr, args.per_page), f"{slug} p{pn} drafting")
            doc["pages"].append({"n": pn, "png": os.path.relpath(png, args.work), "ocr": ocr, "vlm": vlm, "cands": cands})
            print(f"  {slug} p{pn}: ocr={len(ocr)}c vlm={len(vlm)}c drafted={len(cands)}", flush=True)
        doc_text = "\n".join(p["ocr"] for p in doc["pages"])
        doc["unanswerable"] = cached_or_skip(os.path.join(out_dir, "unanswerable.json"), lambda: draft_unanswerable(llm, doc_text), f"{slug} unanswerable")
        docs[slug] = doc
        print(f"{slug} done in {time.time() - t0:.0f}s ({len(pages)} pages)", flush=True)

    # ── verification across the corpus ──
    corpus_text = {s: norm("\n".join(p["ocr"] for p in d["pages"])) for s, d in docs.items()}
    candidates = []
    for slug, d in docs.items():
        kept = 0
        seen = []  # (norm answer, question) already kept for this document
        for p in d["pages"]:
            for c in p["cands"]:
                if kept >= args.max_per_doc:
                    break
                ocr_ok = norm(c["evidence"]) in norm(p["ocr"]) and norm(c["answer"]) in norm(c["evidence"])
                if not ocr_ok:
                    continue  # the hard requirement: evidence and answer are literally on the page
                # Near-duplicate guard: same answer and a similar question from
                # another page of the same document adds nothing to the set.
                if any(
                    norm(c["answer"]) == a and difflib.SequenceMatcher(None, norm(c["question"]), q).ratio() > 0.5
                    for a, q in seen
                ):
                    continue
                seen.append((norm(c["answer"]), norm(c["question"])))
                vlm_ok = True if args.skip_vlm else fuzzy_in(c["answer"], p["vlm"])
                shared = [s for s, t in corpus_text.items() if s != slug and norm(c["answer"]) in t]
                candidates.append(
                    {
                        "id": f"{slug}-p{p['n']}-{kept + 1}",
                        "doc": d["file"],
                        "program": d["program"],
                        "page": p["n"],
                        "png": p["png"],
                        "question": c["question"].strip(),
                        "answer": c["answer"].strip(),
                        "evidence": c["evidence"].strip(),
                        "type": c.get("type", "condition"),
                        "vlm_ok": vlm_ok,
                        "shared_in_docs": len(shared),
                        "answerable": True,
                    }
                )
                kept += 1
        for i, u in enumerate(d["unanswerable"], 1):
            absent = not any(norm(u["key_term"]) in t for t in corpus_text.values())
            if absent:
                candidates.append(
                    {
                        "id": f"{slug}-u{i}",
                        "doc": d["file"],
                        "program": d["program"],
                        "page": None,
                        "png": d["pages"][0]["png"] if d["pages"] else None,
                        "question": u["question"].strip(),
                        "answer": "",
                        "evidence": f"(ไม่มีในเอกสาร; key term: {u['key_term']})",
                        "type": "unanswerable",
                        "vlm_ok": True,
                        "shared_in_docs": 0,
                        "answerable": False,
                    }
                )

    out = os.path.join(args.work, "candidates.json")
    with open(out, "w", encoding="utf-8") as fh:
        json.dump({"docs": [{"slug": s, "file": d["file"], "program": d["program"], "pages": len(d["pages"])} for s, d in docs.items()], "candidates": candidates}, fh, ensure_ascii=False, indent=1)
    n_flag = sum(1 for c in candidates if not c["vlm_ok"])
    n_unans = sum(1 for c in candidates if not c["answerable"])
    print(f"\ncandidates: {len(candidates)} ({n_unans} unanswerable, {n_flag} flagged by vision disagreement) -> {out}")


if __name__ == "__main__":
    sys.exit(main())
