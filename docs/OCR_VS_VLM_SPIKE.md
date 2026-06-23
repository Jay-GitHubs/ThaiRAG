# Spike: Deterministic OCR (PaddleOCR Thai) vs Vision-LLM OCR

Status: **Complete — GO, confirmed by graded CER** · Phase 3 spike + Phase 1b eval of
`DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md`

> **Phase 1b update (graded CER):** on 10 clean Thai gazette pages scored against the
> trustworthy text layer, **order-independent character accuracy is PaddleOCR 94.5% vs
> VLM 90.1%**, and PaddleOCR was more reliable (VLM had 1 timeout + 1 hallucination
> failure). Details in the "Phase 1b" section below. This confirms the spike's GO.

## Question

For the OCR-needing region classes (`CorruptedText`, `Scanned`, `ImageHeavy` — ~10% of
the measured corpus), is a **dedicated deterministic OCR engine** good enough on Thai to
replace the vision LLM for *text transcription*? (The VLM stays for *figure description*.)

## Method

Rendered 5 real Thai pages at 200 DPI (4 `CorruptedText` pages from the Micro Pay Digital
Fraud manual + 1 `ImageHeavy` diagram page from 084_2568) and ran each through:

- **PaddleOCR** `th_PP-OCRv5_mobile_rec` (dedicated Thai model), CPU, local.
- **qwen2.5-vl-7b** via the gateway.

Tools (committed, reusable):
`cargo run -p thairag-document --example dump_page_pngs -- <out> <pdf> <pages>`
then `THAIRAG_API_KEY=… python3 scripts/bench/ocr_vs_vlm.py <out>`.

## Results

| Page | PaddleOCR secs / chars / garble | VLM-7b secs / chars / garble |
|---|---|---|
| 084_2568 p2 (diagram) | 19.6 / 1227 / **0** | 19.1 / 609 / 0 |
| DigitalFraud p2 | 15.8 / 1090 / **0** | 24.8 / 1772 / 0 |
| DigitalFraud p3 | 22.5 / 1608 / **0** | 22.2 / 806 / 0 |
| DigitalFraud p4 | 14.5 / 451 / **0** | 17.7 / 578 / 0 |
| DigitalFraud p5 | 18.9 / 1070 / **0** | **524 timeout** |

`garble` = count of Latin-Extended CMap-leak chars (`Ļ Ŀ ļ`). Both engines score **0** — i.e.
both fix the `เรืĻอง` corruption that the text layer can't.

## Findings

1. **PaddleOCR's Thai is genuinely competitive.** Clean, coherent Thai (`เรื่อง`, not
   `เรืĻอง`; `การปิดบังข้อมูลสำคัญของผู้ใช้บริการแอปพลิเคชัน`). Zero CMap garble on every page.
2. **PaddleOCR is more *complete*.** On most pages it captured ~1.5–2× the characters of the
   VLM (e.g. 1608 vs 806), which tended to abbreviate/stop early. Spot-checking the
   transcripts, the extra PaddleOCR text is real body content, not noise.
3. **PaddleOCR is deterministic & reliable.** 5/5 pages succeeded; the **VLM failed 1/5**
   (gateway 524). PaddleOCR is local — no gateway dependency, no 5xx, and it **parallelizes
   freely** (unlike the single-instance VLM, which couldn't — see the concurrency finding).
4. **No hallucination/repetition.** PaddleOCR transcribes; it can't fabricate Thai numerals
   or loop the way a VLM can — directly relevant to the table-accuracy bottleneck.
5. **Speed is comparable here but on a bad footing for PaddleOCR:** ~14–22s/page is the
   *mobile* model on *CPU with no GPU*. On GPU or with the server model it is far faster, and
   being local it removes the gateway round-trip + flakiness entirely.

### Caveats (honest)
- **Small sample, no rigorous ground-truth scoring** — this is a directional spike, not a
  graded benchmark. A labeled eval (Phase 1b) should confirm before full commitment.
- **Minor PaddleOCR artifacts**: occasional missing spaces (`DIGITALSOLUTIONS`), a rare
  char slip (`E-Wallt`), and a stray replacement char at edges — light post-processing
  territory. The VLM had its own artifacts (markup leakage, truncation).
- **PaddleOCR does not *describe* figures** — it transcribes text only. The VLM is still
  required for the `Mixed`/diagram figure-description job. This confirms the design's
  **hybrid**, not a replacement.
- **Deployment cost**: PaddleOCR is Python; integrating means a sidecar or ONNX-via-`ort`.

## Recommendation: GO

Add a **deterministic OCR tier (PaddleOCR Thai)** as tier-2 for the OCR-needing classes,
keeping the VLM for figure description (tier-3). It is competitive-to-better on Thai text,
more complete, deterministic, reliable, local, and parallelizable — it removes the VLM's
slowness, gateway 5xx, and hallucination/repetition risks for the transcription job.

Before building the integration:
1. ~~Phase 1b labeled eval~~ — **done** (below).
2. **Decide deployment shape** (sidecar microservice vs ONNX-`ort`) and **GPU** for throughput.
3. **Light post-processing** for the minor space/char artifacts.

## Phase 1b — graded CER (no manual labeling)

**Ground truth without labeling:** a *clean* (non-garbled) PDF page's pdfium text layer is
correct, so it is the reference. `dump_page_pngs` writes each page PNG **and** its `.gt.txt`;
`scripts/bench/ocr_eval_cer.py` OCRs the PNG with each engine and scores CER. Corpus: 10
clean Thai pages from `tfac_gazette.pdf` (1.1k–2.4k Thai chars each, garble ratio 0.000).

Two metrics, because text-layer GT and OCR differ in **reading order** (multi-column +
headers/footnotes), which inflates an order-sensitive edit distance even when every
character is read correctly:

- **seq-CER** — standard, order-sensitive.
- **bag-CER** — edit distance on the *sorted* character multiset → order-independent; isolates
  "read the right characters" from "in the right order".

| Engine | seq-CER | **bag-CER** | char accuracy (1−bag) | reliability (n=10) |
|---|---|---|---|---|
| **PaddleOCR `th_PP-OCRv5`** | 0.155 | **0.055** | **94.5%** | 10/10 steady |
| qwen2.5-vl-7b | 0.185 | **0.099** | 90.1% | 1× HTTP 502, 1× catastrophic (0.62 bag-CER = hallucination) |

### Findings
1. **bag-CER ≈ ⅓ of seq-CER for both engines** → the high sequence error is dominated by
   **reading-order / page-furniture** differences vs the text layer, *not* character
   recognition. Absolute seq-CER here is an upper bound on true error, not the OCR's CER.
2. **On true character recognition PaddleOCR wins: 94.5% vs 90.1%.** This upgrades the
   spike from "competitive" to "competitive-to-better, with a number."
3. **PaddleOCR is more reliable** — the VLM timed out on one page (gateway 502) and produced a
   catastrophic hallucination on another (bag-CER 0.62); PaddleOCR was steady throughout.
4. **Reading-order reconstruction is a separate problem** (the router's job — multi-column
   ordering), affecting any OCR→text pipeline regardless of engine. Not an OCR-choice factor.

### Caveats
- Single fixture family (gazette prose); broaden to more layouts/fonts before final sizing.
- text-layer GT inherits its own quirks (the spurious-space artifact is neutralized by
  whitespace-stripping; reading order is measured separately via bag-CER).
- Run: `cargo run -p thairag-document --example dump_page_pngs -- <out> <pdf> <pages>` then
  `THAIRAG_API_KEY=… python3 scripts/bench/ocr_eval_cer.py <out>`.
