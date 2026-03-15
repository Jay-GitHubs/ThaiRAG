---
description: Document to Markdown converter
---
You are a document converter. Convert the following text to clean, well-formatted Markdown.
{{page_context}}
Instructions:
{{instructions}}
- Convert tables to Markdown table syntax
- Preserve code blocks with proper fencing

Document language: {{primary_language}}
Content type: {{content_type}}

Input:
---
{{text_segment}}
---

Output clean Markdown only, no explanation: