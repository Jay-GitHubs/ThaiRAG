# Accuracy evaluation: the single measuring stick

Every accuracy claim about ThaiRAG comes from `eval.py` run against a frozen
gold set. Nothing else counts. The design follows the usual RAG evaluation
split: measure ingestion, retrieval and answers separately so that a failure
points at one stage, keep the test set frozen, and tune only on the dev split.

## Pieces

| Script | Role |
|---|---|
| `gen_candidates.py` | Drafts gold-set candidates from a folder of PDFs. Each page is read twice independently (PaddleOCR sidecar + the vision LLM); a candidate survives only if its evidence quote is verbatim in the OCR text, and it is flagged when the vision reading disagrees. Also drafts unanswerable questions whose key term appears nowhere in the corpus. Output: `work/candidates.json` plus page images. Resumable. |
| `build_review.py` | Builds `work/review.html`, a local page for the human reviewer: page image, question, proposed answer, evidence, flags; approve / reject / correct with keyboard shortcuts; decisions persist in the browser and export as `decisions.json`. |
| `promote_gold.py` | Turns reviewed decisions into `gold/<name>.json`: approved items only, corrected answers applied, program-named question variant, dev/test split by document. |
| `eval.py` | `run` the three layers against a workspace, `report` an HTML page, `compare` two runs. Results go under `results/`, stamped with git commit, config and timestamp. |

## Data handling

Nothing derived from a real document is tracked in git. `work/` (page images, OCR text,
candidates), `gold/` (questions, answers and evidence quotes) and `results/` (questions and
the system's answers) are all gitignored. Keep the gold set in private storage if it must
outlive this machine; the tools read it from any path. Only the scripts are committed, and
unit-test fixtures elsewhere in the repository use synthetic text.

## Layers and metrics

- **Fidelity**: the gold evidence quote is found in the document's stored chunks (fuzzy, adjacent chunk pairs included). Deterministic. Catches OCR, chunking and segmentation loss.
- **Retrieval**: `doc_hit@k`, `evidence_hit@k` and MRR against the gold document through `POST /v2/search`. Deterministic, seconds to run, the CI candidate. `page_hit@k` is reported only when the API returns page numbers.
- **Answers**: `POST test-query`, N runs per question. Deterministic value match (numbers must appear, Thai digits folded) plus an LLM judge: `correct / partial / incorrect`, `refusal`, `supported`. Reported with a Wilson 95% interval. Unanswerable questions score on refusal.

Every item gets a triage bucket: `ingestion`, `retrieval`, `generation`, `reference` (the value is present but the judge disagreed: check the reference), `refusal`, `ok`.

## Two layouts, two numbers

- **Per product per workspace**: the documented deployment layout. Plain questions. This is the primary number.
- **Shared workspace with near-clone documents**: the stress test. Use `--named` so questions name the program; without it, "which document" is undefined for most questions.

Arm isolation (dense only, keyword only) is a deployment setting: restart the stack with `THAIRAG__SEARCH__VECTOR_WEIGHT` / `TEXT_WEIGHT` and run again with another label.

## Typical cycle

```bash
# 1. draft candidates (hours: vision transcription dominates), then review
python3 -I scripts/eval/gen_candidates.py --pdf-dir ~/Downloads/test_materials --work scripts/eval/work
python3 -I scripts/eval/build_review.py --work scripts/eval/work && open scripts/eval/work/review.html
# 2. promote the exported decisions
python3 -I scripts/eval/promote_gold.py --decisions ~/Downloads/decisions.json --out scripts/eval/gold/sme_v1.json
# 3. measure (retrieval only is fast; add --answers 3 for the full picture)
python3 -I scripts/eval/eval.py run --gold scripts/eval/gold/sme_v1.json --ws <workspace> \
    --label main-$(git rev-parse --short HEAD) --out scripts/eval/results/main.json --ingest ~/Downloads/test_materials
python3 -I scripts/eval/eval.py compare scripts/eval/results/main.json scripts/eval/results/candidate.json
```

Exit criteria are agreed before the first run and recorded with the run label. One change per cycle, measured on dev, confirmed on test, at most four cycles; what remains is documented as the ceiling. Summary numbers may be quoted in docs or PRs; question text and answers never are.
