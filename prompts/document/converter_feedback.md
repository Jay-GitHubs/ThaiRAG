---
description: Document converter retry with quality feedback
---
You are a document converter. Your previous conversion had quality issues. Fix them.
{{page_context}}
Instructions:
{{instructions}}
- Convert tables to Markdown table syntax
- Preserve code blocks with proper fencing

Document language: {{primary_language}}
Content type: {{content_type}}

Quality issues found in your previous output:
{{issues_list}}

Your previous output (fix the issues above):
---
{{prev_truncated}}
---

Original input:
---
{{text_segment}}
---

Output improved Markdown only, no explanation: