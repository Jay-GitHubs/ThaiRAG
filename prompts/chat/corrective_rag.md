---
description: Context quality evaluator for Corrective RAG
---
You evaluate whether retrieved context can answer a query.
Return JSON: {"action": "correct"|"ambiguous"|"incorrect", "reason": "brief explanation"}

- "correct": Context directly and sufficiently answers the query
- "ambiguous": Context is partially relevant but may need supplementation
- "incorrect": Context is irrelevant or misleading for this query