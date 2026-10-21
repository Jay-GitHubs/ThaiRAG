#!/usr/bin/env python3
"""ThaiRAG evaluation tool: the single measuring stick for accuracy.

Three layers, each reported on its own so a failure points at one stage:

  fidelity   is the gold evidence present in the document's stored chunks?
             (deterministic; catches ingestion/OCR/chunking loss)
  retrieval  evidence recall@k and MRR against the gold document + page
             through POST /v2/search (deterministic, no LLM)
  answers    POST test-query, N runs per question; deterministic value match
             plus an LLM judge (correct / partial / incorrect, refusal,
             supported-by-context)

Every row gets a triage bucket:
  ingestion   evidence not in stored chunks
  retrieval   evidence stored but not retrieved
  generation  evidence retrieved, answer wrong
  reference   answer contains the gold value but the judge said wrong (check the reference)
  refusal     unanswerable question answered, or answerable question refused
  ok

Usage
  python3 -I scripts/eval/eval.py run --gold scripts/eval/gold/sme_v1.json --ws WS \
      --label main-2026-10-21 --out scripts/eval/results/main-2026-10-21.json \
      [--ingest PDF_DIR] [--split test|dev|all] [--named] [--top-k 5] \
      [--answers 3] [--judge http://192.168.0.200:8931/v1]
  python3 -I scripts/eval/eval.py report  results/X.json          -> results/X.html
  python3 -I scripts/eval/eval.py compare results/A.json results/B.json

Arm isolation (dense-only / keyword-only) is a deployment setting, not a
request parameter: restart the stack with THAIRAG__SEARCH__VECTOR_WEIGHT /
TEXT_WEIGHT and run again with another --label.

Stdlib only. Env: THAIRAG_API, THAIRAG_EMAIL, THAIRAG_PASSWORD.
"""
import argparse
import difflib
import json
import math
import os
import re
import sys
import time
import urllib.error
import urllib.request
import uuid

API = os.environ.get("THAIRAG_API", "http://localhost:8080")
EMAIL = os.environ.get("THAIRAG_EMAIL", "playwright@test.com")
PASSWORD = os.environ.get("THAIRAG_PASSWORD", "Test1234!")
PENDING = ("pending", "processing", "queued")
THAI_DIGITS = str.maketrans("๐๑๒๓๔๕๖๗๘๙", "0123456789")


# ── text helpers ─────────────────────────────────────────────────────────────
def norm(t):
    return re.sub(r"\s+", "", (t or "").translate(THAI_DIGITS)).lower()


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


def numbers_in(t):
    t = (t or "").translate(THAI_DIGITS).replace(",", "")
    out = set()
    for m in re.findall(r"\d+(?:\.\d+)?", t):
        out.add(m[:-2] if m.endswith(".0") else m)
    return out


def value_match(gold, answer):
    """Deterministic correctness for value answers: every number in the gold
    answer must appear in the model answer; otherwise fuzzy text containment."""
    g_nums = numbers_in(gold)
    if g_nums:
        return g_nums.issubset(numbers_in(answer))
    return fuzzy_in(gold, answer, 0.8)


def wilson(p, n, z=1.96):
    if n == 0:
        return (0.0, 0.0)
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


# ── API client ───────────────────────────────────────────────────────────────
class Client:
    def __init__(self):
        self.token = None

    def _req(self, method, path, data=None, headers=None, timeout=120):
        h = {"Authorization": f"Bearer {self.token}"} if self.token else {}
        h.update(headers or {})
        req = urllib.request.Request(API + path, data=data, headers=h, method=method)
        for attempt in range(10):
            try:
                with urllib.request.urlopen(req, timeout=timeout) as r:
                    return json.loads(r.read().decode() or "null")
            except urllib.error.HTTPError as e:
                if e.code != 429:
                    raise
                time.sleep(2 + 2 * attempt)
        raise RuntimeError(f"rate-limited: {method} {path}")

    def json(self, method, path, payload=None, timeout=120):
        return self._req(method, path, json.dumps(payload).encode() if payload is not None else None, {"Content-Type": "application/json"}, timeout)

    def login(self):
        self.token = self.json("POST", "/api/auth/login", {"email": EMAIL, "password": PASSWORD}, 30)["token"]

    def upload(self, ws, file_path, filename):
        b = uuid.uuid4().hex
        with open(file_path, "rb") as fh:
            content = fh.read()
        body = (f"--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/pdf\r\n\r\n").encode() + content + f"\r\n--{b}--\r\n".encode()
        return self._req("POST", f"/api/km/workspaces/{ws}/documents/upload", body, {"Content-Type": f"multipart/form-data; boundary={b}"}, 900)


def unwrap(d, *keys):
    if isinstance(d, list):
        return d
    for k in keys:
        if isinstance(d, dict) and isinstance(d.get(k), list):
            return d[k]
    return []


def list_docs(c, ws):
    return unwrap(c.json("GET", f"/api/km/workspaces/{ws}/documents", None, 60), "data", "documents")


def doc_title(d):
    return d.get("title") or d.get("filename") or ""


def ensure_ingested(c, ws, pdf_dir, needed):
    """Upload sequentially (the OCR sidecar is single-process) and wait."""
    have = {norm(doc_title(d)) for d in list_docs(c, ws)}
    for f in sorted(needed):
        if norm(f) in have or norm(os.path.splitext(f)[0]) in have:
            continue
        t0 = time.time()
        c.upload(ws, os.path.join(pdf_dir, f), f)
        for _ in range(360):
            docs = list_docs(c, ws)
            if docs and not any(str(d.get("status", "")).lower() in PENDING for d in docs):
                break
            time.sleep(5)
        print(f"  ingested {f[:60]} in {time.time() - t0:.0f}s", flush=True)


# ── layers ───────────────────────────────────────────────────────────────────
def layer_fidelity(c, ws, items, doc_ids):
    chunks_by_doc = {}
    for it in items:
        did = doc_ids.get(norm(it["doc"]))
        if not did:
            it["fidelity_ok"] = False
            it["bucket_note"] = "document not in workspace"
            continue
        if did not in chunks_by_doc:
            r = c.json("GET", f"/api/km/workspaces/{ws}/documents/{did}/chunks", None, 120)
            chunks_by_doc[did] = [(ch.get("index"), ch.get("page"), ch.get("text") or ch.get("content") or "") for ch in unwrap(r, "chunks", "data")]
        if not it["answerable"]:
            it["fidelity_ok"] = True
            continue
        chunks = chunks_by_doc[did]
        found = [i for i, _, t in chunks if fuzzy_in(it["evidence"], t, 0.85)]
        if not found:
            # An evidence quote can straddle a chunk boundary: check adjacent pairs.
            found = [chunks[k][0] for k in range(len(chunks) - 1) if fuzzy_in(it["evidence"], chunks[k][2] + " " + chunks[k + 1][2], 0.85)]
            it["fidelity_split"] = bool(found)
        it["fidelity_ok"] = bool(found)
        it["fidelity_chunks"] = found[:5]


def layer_retrieval(c, ws, items, doc_ids, top_k, named):
    for it in items:
        q = it["question_named"] if named else it["question"]
        hits = unwrap(c.json("POST", "/v2/search", {"query": q, "workspace_ids": [ws], "top_k": top_k}, 180), "results")
        want = doc_ids.get(norm(it["doc"]))
        rows = []
        for h in hits:
            meta = h.get("metadata") or {}
            rows.append({"doc_id": h.get("doc_id"), "pages": meta.get("page_numbers") or [], "score": h.get("score"), "text": h.get("content") or ""})
        it["hits"] = [{k: v for k, v in r.items() if k != "text"} for r in rows]
        it["doc_hit"] = any(r["doc_id"] == want for r in rows)
        # Page-level hit only when the API reports page numbers; otherwise
        # leave it out of the summary rather than reporting a false 0%.
        if it.get("page") and any(r["pages"] for r in rows):
            it["page_hit"] = any(r["doc_id"] == want and it["page"] in r["pages"] for r in rows)
        ev_rank = next((i + 1 for i, r in enumerate(rows) if r["doc_id"] == want and fuzzy_in(it["evidence"], r["text"], 0.8)), 0)
        it["evidence_hit"] = ev_rank > 0
        it["evidence_rank"] = ev_rank
        it["answer_in_context"] = any(fuzzy_in(it["answer"], r["text"], 0.8) for r in rows) if it["answerable"] else None


JUDGE_PROMPT = """You grade a question-answering system on Thai documents. Compare the system answer with the gold answer and evidence.
Reply with ONE JSON object only:
{{"verdict":"correct|partial|incorrect","refusal":true|false,"supported":true|false,"reason":"<short Thai or English>"}}
- verdict: correct if the system answer states the gold value/fact (wording may differ, extra context is fine); partial if only part of it; incorrect otherwise.
- refusal: true if the system answer says the information is not available / cannot be found.
- supported: true if every claim in the system answer is backed by the retrieved context below.
{unanswerable_note}
QUESTION: {question}
GOLD ANSWER: {gold}
GOLD EVIDENCE: {evidence}
SYSTEM ANSWER: {answer}
RETRIEVED CONTEXT (truncated):
{context}"""


def judge(base, it, answer, context):
    body = {
        "model": "qwen", "temperature": 0, "seed": 42, "max_tokens": 300,
        "response_format": {"type": "json_object"},
        "messages": [{"role": "user", "content": JUDGE_PROMPT.format(
            unanswerable_note=("NOTE: the gold says this question is NOT answerable from the documents; a refusal is the correct behaviour." if not it["answerable"] else ""),
            question=it["question"], gold=it["answer"] or "(not answerable)", evidence=it["evidence"][:400], answer=answer[:1500], context=context[:3000])}],
    }
    req = urllib.request.Request(base.rstrip("/") + "/chat/completions", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=600) as r:
            txt = json.loads(r.read())["choices"][0]["message"]["content"]
        txt = re.sub(r"^```[a-zA-Z]*\s*|\s*```$", "", txt.strip())
        m = re.search(r"\{.*\}", txt, re.S)
        return json.loads(m.group(0)) if m else {}
    except Exception as e:  # judge failure is recorded, never fatal
        return {"error": str(e)[:120]}


def layer_answers(c, ws, items, runs, judge_base, named):
    for it in items:
        q = it["question_named"] if named else it["question"]
        it["runs"] = []
        for _ in range(runs):
            t0 = time.time()
            try:
                r = c.json("POST", f"/api/km/workspaces/{ws}/test-query", {"query": q}, 900)
            except Exception as e:
                it["runs"].append({"error": str(e)[:160]})
                continue
            answer = r.get("answer") or ""
            ctx = "\n---\n".join((ch.get("content") or "")[:800] for ch in unwrap(r, "chunks"))
            run = {"answer": answer, "ms": int((time.time() - t0) * 1000)}
            if it["answerable"]:
                run["value_match"] = value_match(it["answer"], answer)
            if judge_base:
                run["judge"] = judge(judge_base, it, answer, ctx)
            it["runs"].append(run)
        ok = []
        for run in it["runs"]:
            if "error" in run:
                ok.append(False)
                continue
            jv = (run.get("judge") or {}).get("verdict")
            refusal = bool((run.get("judge") or {}).get("refusal"))
            if not it["answerable"]:
                ok.append(refusal)
            elif jv in ("correct", "partial", "incorrect"):
                ok.append(jv == "correct" or (jv == "partial" and run.get("value_match", False)))
            else:
                ok.append(bool(run.get("value_match")))
        it["answer_correct_rate"] = sum(ok) / len(ok) if ok else None
        print(("✓" if it["answer_correct_rate"] == 1 else "~" if it["answer_correct_rate"] else "✗"), it["question"][:70], flush=True)


def triage(it, have_answers):
    if not it["answerable"]:
        if not have_answers or it.get("answer_correct_rate") is None:
            return "n/a"
        return "ok" if it["answer_correct_rate"] >= 0.5 else "refusal"
    if not it.get("fidelity_ok", True):
        return "ingestion"
    if not it.get("evidence_hit", True):
        return "retrieval"
    if not have_answers or it.get("answer_correct_rate") is None:
        return "ok"
    if it["answer_correct_rate"] >= 0.5:
        return "ok"
    runs = [r for r in it.get("runs", []) if "error" not in r]
    if runs and all(bool((r.get("judge") or {}).get("refusal")) for r in runs):
        return "refusal"
    if runs and any(r.get("value_match") for r in runs):
        return "reference"
    return "generation"


def summarize(items, label, have_answers):
    ans = [i for i in items if i["answerable"]]
    unans = [i for i in items if not i["answerable"]]
    n = len(ans)
    s = {"label": label, "n_items": len(items), "n_answerable": n, "n_unanswerable": len(unans)}
    def rate(key, pool):
        vals = [bool(i.get(key)) for i in pool if key in i]
        return (sum(vals) / len(vals)) if vals else None
    s["fidelity"] = rate("fidelity_ok", ans)
    s["doc_hit"] = rate("doc_hit", ans)
    s["page_hit"] = rate("page_hit", ans)
    s["evidence_hit"] = rate("evidence_hit", ans)
    ranks = [1 / i["evidence_rank"] for i in ans if i.get("evidence_rank")]
    s["mrr"] = (sum(ranks) / n) if n else None
    if have_answers:
        rates = [i["answer_correct_rate"] for i in ans if i.get("answer_correct_rate") is not None]
        s["answer_correct"] = (sum(rates) / len(rates)) if rates else None
        s["answer_correct_ci95"] = wilson(s["answer_correct"], len(rates)) if rates else None
        urates = [i["answer_correct_rate"] for i in unans if i.get("answer_correct_rate") is not None]
        s["refusal_correct"] = (sum(urates) / len(urates)) if urates else None
        sup = [bool((r.get("judge") or {}).get("supported")) for i in ans for r in i.get("runs", []) if r.get("judge") and "verdict" in r["judge"]]
        s["supported"] = (sum(sup) / len(sup)) if sup else None
    if s.get("evidence_hit") is not None:
        s["evidence_hit_ci95"] = wilson(s["evidence_hit"], n)
    buckets = {}
    for i in items:
        buckets[i["bucket"]] = buckets.get(i["bucket"], 0) + 1
    s["buckets"] = buckets
    return s


# ── report / compare ─────────────────────────────────────────────────────────
def pct(v):
    return "-" if v is None else f"{100 * v:.1f}%"


def write_report(path_json):
    with open(path_json, encoding="utf-8") as fh:
        d = json.load(fh)
    s, rows = d["summary"], d["items"]
    out = os.path.splitext(path_json)[0] + ".html"
    cards = "".join(f"<div class=card><div class=k>{k}</div><div class=v>{pct(s.get(k)) if isinstance(s.get(k), float) else s.get(k)}</div></div>" for k in ("fidelity", "doc_hit", "page_hit", "evidence_hit", "mrr", "answer_correct", "refusal_correct", "supported"))
    bk = ", ".join(f"{k}: {v}" for k, v in sorted(s["buckets"].items()))
    trs = []
    for r in rows:
        a0 = (r.get("runs") or [{}])[0]
        trs.append("<tr data-b='%s'><td>%s</td><td>%s</td><td class=b>%s</td><td>%s</td><td>%s</td><td>%s</td><td>%s</td><td>%s</td><td>%s</td><td>%s</td><td>%s</td></tr>" % (
            r["bucket"], r["id"], r["type"], r["bucket"], "✓" if r.get("fidelity_ok") else "✗", "✓" if r.get("doc_hit") else "✗", "✓" if r.get("evidence_hit") else "✗",
            pct(r.get("answer_correct_rate")), esc(r["question"]), esc(r["answer"]), esc((a0.get("answer") or "")[:300]), esc(((a0.get("judge") or {}).get("reason") or "")[:200])))
    html = f"""<!doctype html><html lang=th><head><meta charset=utf-8><title>Eval {esc(s['label'])}</title><style>
body{{font:14px/1.45 -apple-system,"Sarabun",sans-serif;margin:16px;color:#1b1f24}} .cards{{display:flex;gap:10px;flex-wrap:wrap;margin:10px 0}}
.card{{border:1px solid #e3e6ea;border-radius:10px;padding:10px 14px;min-width:120px}} .k{{color:#5b6470;font-size:12px}} .v{{font-size:22px;font-weight:700}}
table{{border-collapse:collapse;width:100%}} td,th{{border-bottom:1px solid #e3e6ea;padding:6px;vertical-align:top;text-align:left;font-size:13px}} .b{{font-weight:600}}
tr[data-b=ingestion] .b{{color:#b42318}} tr[data-b=retrieval] .b{{color:#b54708}} tr[data-b=generation] .b{{color:#6f42c1}} tr[data-b=ok] .b{{color:#1a7f37}}
</style></head><body><h2>Evaluation: {esc(s['label'])}</h2><div class=cards>{cards}</div>
<p>items {s['n_items']} (answerable {s['n_answerable']}, unanswerable {s['n_unanswerable']}) · buckets: {esc(bk)}</p>
<p>Filter bucket: <select onchange="for(const t of document.querySelectorAll('tbody tr')) t.style.display=(!this.value||t.dataset.b===this.value)?'':'none'"><option value="">all</option>{''.join(f'<option>{k}</option>' for k in sorted(s['buckets']))}</select></p>
<table><thead><tr><th>id</th><th>type</th><th>bucket</th><th>fid</th><th>doc</th><th>evid</th><th>answer ok</th><th>question</th><th>gold</th><th>system answer (run 1)</th><th>judge</th></tr></thead><tbody>{''.join(trs)}</tbody></table></body></html>"""
    with open(out, "w", encoding="utf-8") as fh:
        fh.write(html)
    print("wrote", out)


def esc(s):
    return str(s if s is not None else "").replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def compare(a_path, b_path):
    a, b = (json.load(open(p, encoding="utf-8")) for p in (a_path, b_path))
    sa, sb = a["summary"], b["summary"]
    print(f"{'metric':22} {sa['label'][:14]:>14} {sb['label'][:14]:>14}")
    for k in ("fidelity", "doc_hit", "page_hit", "evidence_hit", "mrr", "answer_correct", "refusal_correct", "supported"):
        if sa.get(k) is not None or sb.get(k) is not None:
            print(f"{k:22} {pct(sa.get(k)):>14} {pct(sb.get(k)):>14}")
    ra = {r["id"]: r for r in a["items"]}
    changes = [(r["id"], ra[r["id"]]["bucket"], r["bucket"], r["question"][:60]) for r in b["items"] if r["id"] in ra and ra[r["id"]]["bucket"] != r["bucket"]]
    print(f"\nbucket changes: {len(changes)}")
    for cid, fa, fb, q in changes:
        print(f"  {cid:14} {fa:>10} -> {fb:<10} {q}")


# ── main ─────────────────────────────────────────────────────────────────────
def cmd_run(args):
    with open(args.gold, encoding="utf-8") as fh:
        gold = json.load(fh)
    items = [dict(i) for i in gold["items"] if args.split == "all" or i.get("split", "test") == args.split]
    if not items:
        sys.exit("no gold items for that split")
    c = Client()
    c.login()
    if args.ingest:
        ensure_ingested(c, args.ws, args.ingest, {i["doc"] for i in items})
    docs = list_docs(c, args.ws)
    doc_ids = {}
    for d in docs:
        doc_ids[norm(doc_title(d))] = d["id"]
        doc_ids[norm(os.path.splitext(doc_title(d))[0])] = d["id"]
    print(f"gold items: {len(items)} | workspace docs: {len(docs)}", flush=True)
    layer_fidelity(c, args.ws, items, doc_ids)
    print(f"fidelity: {sum(bool(i.get('fidelity_ok')) for i in items)}/{len(items)}", flush=True)
    layer_retrieval(c, args.ws, items, doc_ids, args.top_k, args.named)
    print(f"evidence_hit@{args.top_k}: {sum(bool(i.get('evidence_hit')) for i in items if i['answerable'])}/{sum(i['answerable'] for i in items)}", flush=True)
    if args.answers > 0:
        layer_answers(c, args.ws, items, args.answers, args.judge, args.named)
    for it in items:
        it["bucket"] = triage(it, args.answers > 0)
    summary = summarize(items, args.label, args.answers > 0)
    summary.update({"gold": os.path.basename(args.gold), "gold_version": gold.get("version"), "split": args.split, "named": args.named, "top_k": args.top_k, "answer_runs": args.answers, "api": API, "ts": time.strftime("%Y-%m-%dT%H:%M:%S")})
    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump({"summary": summary, "items": items}, fh, ensure_ascii=False, indent=1)
    print(json.dumps({k: (pct(v) if isinstance(v, float) else v) for k, v in summary.items() if k not in ("api",)}, ensure_ascii=False, indent=1))
    write_report(args.out)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--gold", required=True)
    r.add_argument("--ws", required=True)
    r.add_argument("--label", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--ingest", metavar="PDF_DIR")
    r.add_argument("--split", default="test", choices=["test", "dev", "all"])
    r.add_argument("--named", action="store_true", help="use the program-named question variant (shared scope)")
    r.add_argument("--top-k", type=int, default=5)
    r.add_argument("--answers", type=int, default=0, help="answer runs per question (0 = retrieval only)")
    r.add_argument("--judge", default="http://192.168.0.200:8931/v1")
    p = sub.add_parser("report")
    p.add_argument("json")
    cmp_ = sub.add_parser("compare")
    cmp_.add_argument("a")
    cmp_.add_argument("b")
    args = ap.parse_args()
    if args.cmd == "run":
        cmd_run(args)
    elif args.cmd == "report":
        write_report(args.json)
    else:
        compare(args.a, args.b)


if __name__ == "__main__":
    main()
