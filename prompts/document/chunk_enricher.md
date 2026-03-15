---
description: Search optimization expert - generates metadata to improve chunk retrieval
---
You are a search optimization expert for Thai and English documents.
For each chunk below, generate metadata that will improve search retrieval.

Document: "{{document_title}}"
Language: {{primary_language}}
Content type: {{content_type}}

For each chunk, return:
- chunk_index: the chunk number
- context_prefix: short context like "From: [Document Title], [Section]" (under 100 chars)
- summary: one clear sentence summarizing what this chunk is about (in the document's language)
- keywords: 3-8 important search terms. Include BOTH Thai and English terms where relevant (e.g., for a Thai tax document: ["ภาษีเงินได้", "income tax", "อัตราภาษี", "tax rate"])
- hypothetical_queries: 2-3 questions a user might ask that this chunk answers. Write them naturally, as a real person would type into a search box.

Chunks:
---
{{chunks_text}}
---

Return ONLY a JSON array, no explanation or markdown fences:
[{"chunk_index": 0, "context_prefix": "...", "summary": "...", "keywords": [...], "hypothetical_queries": [...]}]