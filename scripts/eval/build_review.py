#!/usr/bin/env python3
"""Build a self-contained local review page from candidates.json.

  python3 -I scripts/eval/build_review.py --work scripts/eval/work
  open scripts/eval/work/review.html

The page shows each candidate with its page image, question, proposed answer
and evidence. Decisions (approve / reject / corrected answer) are kept in the
browser's localStorage and exported with the Download button as
decisions.json, which `promote_gold.py` turns into the gold set.
Keyboard: A approve, R reject, J/K next/previous, F toggle flagged-only.
"""
import argparse
import json
import os

HTML = r"""<!doctype html>
<html lang="th"><head><meta charset="utf-8"><title>Gold-set review</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
:root{--bg:#f6f7f9;--card:#fff;--ink:#1b1f24;--muted:#5b6470;--ok:#1a7f37;--bad:#b42318;--warn:#b54708;--line:#e3e6ea}
@media (prefers-color-scheme:dark){:root{--bg:#0f1115;--card:#171a20;--ink:#e6e8eb;--muted:#9aa3ad;--line:#2a2f37}}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.5 -apple-system,"Sarabun","Noto Sans Thai",sans-serif}
header{position:sticky;top:0;background:var(--card);border-bottom:1px solid var(--line);padding:10px 16px;display:flex;gap:16px;align-items:center;flex-wrap:wrap}
header b{font-size:16px} header button{padding:6px 12px;border:1px solid var(--line);border-radius:8px;background:var(--card);color:var(--ink);cursor:pointer}
main{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:16px;padding:16px;max-width:1500px;margin:0 auto}
@media (max-width:900px){main{grid-template-columns:1fr}}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px}
.q{font-size:18px;font-weight:600;margin:4px 0 10px} .a{font-size:20px;color:var(--ok);font-weight:700}
.ev{background:var(--bg);border-left:3px solid var(--line);padding:8px 12px;margin:10px 0;white-space:pre-wrap;word-break:break-word}
.meta{color:var(--muted);font-size:13px} .flag{color:var(--warn);font-weight:600}
.img{width:100%;border:1px solid var(--line);border-radius:8px;background:#fff}
.btns{display:flex;gap:10px;margin-top:12px;flex-wrap:wrap}
.btns button{padding:10px 18px;border-radius:10px;border:1px solid var(--line);cursor:pointer;font-size:15px;color:var(--ink);background:var(--card)}
.ok{background:var(--ok)!important;color:#fff!important} .bad{background:var(--bad)!important;color:#fff!important}
input[type=text]{width:100%;padding:8px;border:1px solid var(--line);border-radius:8px;background:var(--bg);color:var(--ink);font-size:15px}
.status{font-weight:700} .status.approved{color:var(--ok)} .status.rejected{color:var(--bad)}
</style></head><body>
<header><b>Gold-set review</b><span id="pos"></span><span id="progress"></span>
<button onclick="go(-1)">◀ Prev (K)</button><button onclick="go(1)">Next (J) ▶</button>
<button onclick="toggleFlag()" id="flagbtn">Flagged only: off (F)</button>
<button onclick="exportDecisions()">⬇ Download decisions.json</button></header>
<main><div class="card" id="left"></div><div class="card"><img class="img" id="img" alt="page"></div></main>
<script>
const DATA = __DATA__;
const C = DATA.candidates; let i = 0, flaggedOnly = false;
const KEY = 'goldset-decisions-v1';
let D = {}; try { D = JSON.parse(localStorage.getItem(KEY) || '{}'); } catch (e) { D = {}; }
function save(){ try { localStorage.setItem(KEY, JSON.stringify(D)); } catch(e){} }
function list(){ return flaggedOnly ? C.map((c,k)=>[c,k]).filter(([c])=>!c.vlm_ok || c.shared_in_docs>0) : C.map((c,k)=>[c,k]); }
function render(){
  const L = list(); if(!L.length){ document.getElementById('left').innerHTML='<p>No items.</p>'; return; }
  if(i>=L.length) i=L.length-1; if(i<0) i=0;
  const [c,k] = L[i]; const d = D[c.id] || {};
  const n = Object.keys(D).length, ap = Object.values(D).filter(x=>x.decision==='approve').length;
  document.getElementById('pos').textContent = `${i+1} / ${L.length}`;
  document.getElementById('progress').textContent = `decided ${n}/${C.length} (approved ${ap})`;
  const flags = [];
  if(!c.vlm_ok) flags.push('vision model did NOT find this answer on the page — check carefully');
  if(c.shared_in_docs>0) flags.push(`same answer text also appears in ${c.shared_in_docs} other document(s)`);
  if(!c.answerable) flags.push('UNANSWERABLE candidate: approve only if the document really does not say this');
  document.getElementById('left').innerHTML = `
    <div class="meta">${esc(c.program)} · page ${c.page ?? '-'} · type <b>${esc(c.type)}</b> · id ${c.id}</div>
    <div class="q">${esc(c.question)}</div>
    <div>Proposed answer: <span class="a">${esc(c.answer) || '(none — unanswerable)'}</span></div>
    <div class="ev">${esc(c.evidence)}</div>
    ${flags.map(f=>`<div class="flag">⚠ ${esc(f)}</div>`).join('')}
    <div style="margin-top:12px">Corrected answer (optional):<br><input type="text" id="corr" value="${esc(d.corrected||'')}" oninput="setCorr(this.value)"></div>
    <div class="btns"><button class="${d.decision==='approve'?'ok':''}" onclick="decide('approve')">✓ Approve (A)</button>
    <button class="${d.decision==='reject'?'bad':''}" onclick="decide('reject')">✗ Reject (R)</button>
    <span class="status ${d.decision||''}">${d.decision? d.decision.toUpperCase():'undecided'}</span></div>`;
  document.getElementById('img').src = c.png || '';
}
function cur(){ const L=list(); return L.length? L[i][0] : null; }
function decide(v){ const c=cur(); if(!c) return; D[c.id] = Object.assign(D[c.id]||{}, {decision:v, id:c.id}); save(); render(); if(v) setTimeout(()=>go(1),120); }
function setCorr(v){ const c=cur(); if(!c) return; D[c.id] = Object.assign(D[c.id]||{}, {corrected:v, id:c.id}); save(); }
function go(dx){ i+=dx; render(); window.scrollTo(0,0); }
function toggleFlag(){ flaggedOnly=!flaggedOnly; i=0; document.getElementById('flagbtn').textContent=`Flagged only: ${flaggedOnly?'on':'off'} (F)`; render(); }
function exportDecisions(){
  const out = C.map(c => Object.assign({}, c, D[c.id]||{}));
  const blob = new Blob([JSON.stringify({decided_at:new Date().toISOString(), items: out}, null, 1)], {type:'application/json'});
  const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = 'decisions.json'; a.click();
}
function esc(s){ return String(s??'').replace(/[&<>"]/g, m=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[m])); }
document.addEventListener('keydown', e => { if(e.target.tagName==='INPUT') return;
  if(e.key==='a'||e.key==='A') decide('approve'); else if(e.key==='r'||e.key==='R') decide('reject');
  else if(e.key==='j'||e.key==='J') go(1); else if(e.key==='k'||e.key==='K') go(-1); else if(e.key==='f'||e.key==='F') toggleFlag(); });
render();
</script></body></html>
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default="scripts/eval/work")
    args = ap.parse_args()
    with open(os.path.join(args.work, "candidates.json"), encoding="utf-8") as fh:
        data = json.load(fh)
    html = HTML.replace("__DATA__", json.dumps(data, ensure_ascii=False).replace("</", "<\\/"))
    out = os.path.join(args.work, "review.html")
    with open(out, "w", encoding="utf-8") as fh:
        fh.write(html)
    print(f"wrote {out} with {len(data['candidates'])} candidates")


if __name__ == "__main__":
    main()
