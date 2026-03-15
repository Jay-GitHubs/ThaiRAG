use thairag_core::PromptRegistry;

/// Prompt templates for AI document preprocessing agents.
///
/// Each function externalizes its prompt via [`PromptRegistry::render_or_default`],
/// using a `const DEFAULT_TEMPLATE` that mirrors the corresponding markdown file
/// under `prompts/document/`.

pub fn analyzer_prompt(
    prompts: &PromptRegistry,
    excerpt: &str,
    mime_type: &str,
    doc_size_bytes: usize,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a document analysis expert for Thai and English documents.
Analyze the following document excerpt (MIME type: {{mime_type}}, total size: {{doc_size_bytes}} bytes) and return a JSON object with these fields:

- primary_language: "th", "en", or "th+en" (for mixed Thai/English)
- content_type: one of "narrative", "tabular", "mixed", "form", "slides"
- structure_level: one of "well_structured", "semi_structured", "unstructured"
- needs_ocr_correction: true if text has OCR artifacts (garbled characters, broken Thai, missing spaces, random symbols)
- has_headers_footers: true if repeated header/footer patterns detected
- estimated_sections: integer count of distinct sections/topics
- confidence: 0.0 to 1.0 (how confident you are in this analysis)
- recommended_quality_threshold: float 0.3-1.0 — how strict the quality check should be for this document. Use lower values (0.4-0.6) for messy OCR/scanned docs where perfect conversion is unrealistic; higher (0.7-0.9) for clean, well-structured text.
- recommended_max_chunk_size: integer 300-3000 — ideal chunk size in characters. Use smaller chunks (300-600) for dense tabular/form data; medium (600-1200) for mixed content; larger (1200-2000) for narrative/well-structured docs with long coherent paragraphs.
- recommended_min_ai_size: integer 100-2000 — minimum document size (bytes) worth AI processing. Small forms/tables benefit from AI even at ~200 bytes; large narrative docs only need AI above ~500 bytes.

Return ONLY valid JSON, no explanation or markdown fences.

Document excerpt:
---
{{excerpt}}
---"#;

    let size_str = doc_size_bytes.to_string();
    prompts.render_or_default(
        "document.analyzer",
        DEFAULT_TEMPLATE,
        &[
            ("excerpt", excerpt),
            ("mime_type", mime_type),
            ("doc_size_bytes", &size_str),
        ],
    )
}

pub fn analyzer_vision_prompt(
    prompts: &PromptRegistry,
    mime_type: &str,
    doc_size_bytes: usize,
    ocr_text: &str,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a vision-capable document analysis expert for Thai and English documents.
You are given the ORIGINAL document image. Analyze it by reading directly from the image.

Return a JSON object with these fields:

- primary_language: "th", "en", or "th+en" (for mixed Thai/English)
- content_type: one of "narrative", "tabular", "mixed", "form", "slides"
- structure_level: one of "well_structured", "semi_structured", "unstructured"
- needs_ocr_correction: true if the document is scanned/photographed and text extraction would produce OCR artifacts
- has_headers_footers: true if repeated header/footer patterns detected
- estimated_sections: integer count of distinct sections/topics
- confidence: 0.0 to 1.0 (how confident you are in this analysis)
- recommended_quality_threshold: float 0.3-1.0 — lower (0.4-0.6) for messy OCR/scanned docs; higher (0.7-0.9) for clean text
- recommended_max_chunk_size: integer 300-3000 — smaller (300-600) for dense tabular/form; larger (1200-2000) for narrative
- recommended_min_ai_size: integer 100-2000 — minimum document size worth AI processing

MIME type: {{mime_type}}, total size: {{doc_size_bytes}} bytes.

For reference, here is the (possibly garbled) OCR-extracted text:
---
{{ocr_ref}}
---

Return ONLY valid JSON, no explanation or markdown fences."#;

    let size_str = doc_size_bytes.to_string();
    let ocr_ref = if ocr_text.len() > 2000 {
        format!("{}... [truncated]", &ocr_text[..2000])
    } else {
        ocr_text.to_string()
    };

    prompts.render_or_default(
        "document.analyzer_vision",
        DEFAULT_TEMPLATE,
        &[
            ("mime_type", mime_type),
            ("doc_size_bytes", &size_str),
            ("ocr_ref", &ocr_ref),
        ],
    )
}

pub fn converter_prompt(
    prompts: &PromptRegistry,
    text_segment: &str,
    primary_language: &str,
    content_type: &str,
    needs_ocr_correction: bool,
    has_headers_footers: bool,
) -> String {
    converter_prompt_with_page(
        prompts,
        text_segment,
        primary_language,
        content_type,
        needs_ocr_correction,
        has_headers_footers,
        None,
    )
}

/// Converter prompt with optional page context for page-aware processing.
pub fn converter_prompt_with_page(
    prompts: &PromptRegistry,
    text_segment: &str,
    primary_language: &str,
    content_type: &str,
    needs_ocr_correction: bool,
    has_headers_footers: bool,
    page_info: Option<(usize, usize)>, // (current_page, total_pages)
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a document converter. Convert the following text to clean, well-formatted Markdown.
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

Output clean Markdown only, no explanation:"#;

    let mut instructions = vec![
        "- Preserve ALL content; do not summarize or omit anything",
        "- Use proper Markdown headings (##, ###) for section boundaries",
        "- Normalize whitespace and fix broken line wraps",
        "- Preserve both Thai and English text accurately",
    ];

    if needs_ocr_correction {
        instructions.push("- Fix obvious OCR errors, especially broken Thai characters and garbled text");
    }
    if has_headers_footers {
        instructions.push("- Remove repeated headers and footers (page numbers, document titles repeated on every page)");
    }

    let instructions_str = instructions.join("\n");

    let page_context = match page_info {
        Some((page, total)) => {
            format!("\nThis is page {page} of {total}. Process this page individually.\n")
        }
        None => String::new(),
    };

    prompts.render_or_default(
        "document.converter",
        DEFAULT_TEMPLATE,
        &[
            ("page_context", &page_context),
            ("instructions", &instructions_str),
            ("primary_language", primary_language),
            ("content_type", content_type),
            ("text_segment", text_segment),
        ],
    )
}

/// Converter retry prompt with quality feedback from the previous attempt.
pub fn converter_feedback_prompt(
    prompts: &PromptRegistry,
    text_segment: &str,
    primary_language: &str,
    content_type: &str,
    needs_ocr_correction: bool,
    has_headers_footers: bool,
    page_info: Option<(usize, usize)>,
    previous_output: &str,
    issues: &[String],
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a document converter. Your previous conversion had quality issues. Fix them.
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

Output improved Markdown only, no explanation:"#;

    let mut instructions = vec![
        "- Preserve ALL content; do not summarize or omit anything",
        "- Use proper Markdown headings (##, ###) for section boundaries",
        "- Normalize whitespace and fix broken line wraps",
        "- Preserve both Thai and English text accurately",
    ];

    if needs_ocr_correction {
        instructions.push("- Fix obvious OCR errors, especially broken Thai characters and garbled text");
    }
    if has_headers_footers {
        instructions.push("- Remove repeated headers and footers (page numbers, document titles repeated on every page)");
    }

    let instructions_str = instructions.join("\n");

    let page_context = match page_info {
        Some((page, total)) => {
            format!("\nThis is page {page} of {total}. Process this page individually.\n")
        }
        None => String::new(),
    };

    let issues_list = issues
        .iter()
        .map(|i| format!("- {i}"))
        .collect::<Vec<_>>()
        .join("\n");

    // Truncate previous output to keep prompt size reasonable
    let prev_truncated = if previous_output.len() > 3000 {
        &previous_output[..3000]
    } else {
        previous_output
    };

    prompts.render_or_default(
        "document.converter_feedback",
        DEFAULT_TEMPLATE,
        &[
            ("page_context", &page_context),
            ("instructions", &instructions_str),
            ("primary_language", primary_language),
            ("content_type", content_type),
            ("issues_list", &issues_list),
            ("prev_truncated", prev_truncated),
            ("text_segment", text_segment),
        ],
    )
}

/// Vision-based converter prompt. The document image/PDF is attached separately.
/// raw_text is the OCR-extracted text for reference.
pub fn converter_vision_prompt(
    prompts: &PromptRegistry,
    primary_language: &str,
    content_type: &str,
    has_headers_footers: bool,
    raw_text: &str,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a vision-capable document converter specializing in Thai and English documents.

You are given the ORIGINAL document image/PDF. Convert it to clean, well-formatted Markdown by reading directly from the image.

Instructions:
- Read text directly from the document image — do NOT rely on the OCR text below
- Preserve ALL content accurately; do not summarize or omit anything
- Use proper Markdown headings (##, ###) for section boundaries
- Convert tables to Markdown table syntax
- Preserve code blocks with proper fencing
- Fix any OCR artifacts you see in the original — you can read the actual characters from the image{{header_footer_instruction}}
- For Thai text: ensure proper word segmentation and character rendering

Document language: {{primary_language}}
Content type: {{content_type}}

For reference, here is the (possibly garbled) OCR-extracted text:
---
{{ocr_ref}}
---

Output clean Markdown only, no explanation:"#;

    let header_footer_instruction = if has_headers_footers {
        "\n- Remove repeated headers and footers (page numbers, document titles repeated on every page)"
    } else {
        ""
    };

    let ocr_ref = if raw_text.len() > 2000 {
        format!("{}... [truncated]", &raw_text[..2000])
    } else {
        raw_text.to_string()
    };

    prompts.render_or_default(
        "document.converter_vision",
        DEFAULT_TEMPLATE,
        &[
            ("header_footer_instruction", header_footer_instruction),
            ("primary_language", primary_language),
            ("content_type", content_type),
            ("ocr_ref", &ocr_ref),
        ],
    )
}

/// Vision-based converter prompt for a single page.
pub fn converter_vision_page_prompt(
    prompts: &PromptRegistry,
    primary_language: &str,
    content_type: &str,
    has_headers_footers: bool,
    page_text: &str,
    page_num: usize,
    total_pages: usize,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a vision-capable document converter specializing in Thai and English documents.

You are given the ORIGINAL document. Focus on page {{page_num}} of {{total_pages}}. Convert this page to clean Markdown by reading directly from the image.

Instructions:
- Read text directly from the document image for page {{page_num}}
- Preserve ALL content; do not summarize
- Use proper Markdown headings and table syntax
- Fix any OCR artifacts by reading from the actual image{{header_footer_instruction}}
- For Thai text: ensure proper word segmentation

Document language: {{primary_language}}
Content type: {{content_type}}

OCR text for reference (page {{page_num}}):
---
{{ocr_ref}}
---

Output clean Markdown for page {{page_num}} only, no explanation:"#;

    let header_footer_instruction = if has_headers_footers {
        "\n- Remove repeated headers and footers"
    } else {
        ""
    };

    let ocr_ref = if page_text.len() > 1500 {
        format!("{}... [truncated]", &page_text[..1500])
    } else {
        page_text.to_string()
    };

    let page_num_str = page_num.to_string();
    let total_pages_str = total_pages.to_string();

    prompts.render_or_default(
        "document.converter_vision_page",
        DEFAULT_TEMPLATE,
        &[
            ("page_num", &page_num_str),
            ("total_pages", &total_pages_str),
            ("header_footer_instruction", header_footer_instruction),
            ("primary_language", primary_language),
            ("content_type", content_type),
            ("ocr_ref", &ocr_ref),
        ],
    )
}

pub fn quality_checker_prompt(
    prompts: &PromptRegistry,
    original_sample: &str,
    converted_sample: &str,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a document conversion quality checker.
Compare the original text with the converted Markdown and rate the conversion quality.

Original (excerpt):
---
{{original_sample}}
---

Converted Markdown (excerpt):
---
{{converted_sample}}
---

Rate the following on a scale of 0.0 to 1.0:
1. coherence_score: Is the converted text logically coherent and readable?
2. completeness_score: Does the conversion preserve all important content?
3. formatting_score: Is the Markdown well-formatted with proper headings, lists, tables?

Also list any specific issues found.

Return ONLY valid JSON, no explanation or markdown fences:
{"coherence_score": 0.0, "completeness_score": 0.0, "formatting_score": 0.0, "issues": []}"#;

    prompts.render_or_default(
        "document.quality_checker",
        DEFAULT_TEMPLATE,
        &[
            ("original_sample", original_sample),
            ("converted_sample", converted_sample),
        ],
    )
}

/// Vision-based quality checker prompt. The document image is attached separately.
pub fn quality_checker_vision_prompt(
    prompts: &PromptRegistry,
    converted_sample: &str,
    ocr_text: &str,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a vision-capable document conversion quality checker.
You are given the ORIGINAL document image. Compare it visually against the converted Markdown below.

Read the original document directly from the image — do NOT rely on the OCR text.

Converted Markdown (excerpt):
---
{{converted_ref}}
---

OCR-extracted text for reference (may contain errors):
---
{{ocr_ref}}
---

Rate the following on a scale of 0.0 to 1.0:
1. coherence_score: Is the converted text logically coherent and readable?
2. completeness_score: Does the conversion preserve all important content from the original image?
3. formatting_score: Is the Markdown well-formatted with proper headings, lists, tables?

Also list any specific issues found (e.g., missing sections, garbled Thai text, table formatting errors).

Return ONLY valid JSON, no explanation or markdown fences:
{"coherence_score": 0.0, "completeness_score": 0.0, "formatting_score": 0.0, "issues": []}"#;

    let ocr_ref = if ocr_text.len() > 1500 {
        format!("{}... [truncated]", &ocr_text[..1500])
    } else {
        ocr_text.to_string()
    };

    let converted_ref = if converted_sample.len() > 2000 {
        format!("{}... [truncated]", &converted_sample[..2000])
    } else {
        converted_sample.to_string()
    };

    prompts.render_or_default(
        "document.quality_checker_vision",
        DEFAULT_TEMPLATE,
        &[
            ("converted_ref", &converted_ref),
            ("ocr_ref", &ocr_ref),
        ],
    )
}

pub fn smart_chunker_prompt(prompts: &PromptRegistry, numbered_markdown: &str) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a document structure analyst for Thai and English documents.
Analyze the following Markdown document and identify logical sections for chunking.

For each section, provide:
- start_line: first line number (1-indexed)
- end_line: last line number (inclusive)
- topic: a concise topic label (in the document's primary language)
- section_title: the section heading if present, or null
- chunk_type: one of "paragraph", "table", "list", "code", "mixed"

Rules:
- Each section should be 200-1500 characters when possible
- Never split a table or code block across sections
- Prefer splitting at heading boundaries
- Cover ALL lines in the document (no gaps)

Document:
---
{{numbered_markdown}}
---

Return ONLY a JSON array, no explanation or markdown fences."#;

    prompts.render_or_default(
        "document.smart_chunker",
        DEFAULT_TEMPLATE,
        &[("numbered_markdown", numbered_markdown)],
    )
}

/// Smart chunker retry prompt with validation feedback.
pub fn smart_chunker_feedback_prompt(
    prompts: &PromptRegistry,
    numbered_markdown: &str,
    issues: &[String],
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a document structure analyst for Thai and English documents.
Your previous chunking had issues. Fix them and re-chunk.

Issues found:
{{issues_list}}

For each section, provide:
- start_line: first line number (1-indexed)
- end_line: last line number (inclusive)
- topic: a concise topic label (in the document's primary language)
- section_title: the section heading if present, or null
- chunk_type: one of "paragraph", "table", "list", "code", "mixed"

Rules:
- Each section should be 200-1500 characters when possible
- Never split a table or code block across sections
- Prefer splitting at heading boundaries
- Cover ALL lines in the document (no gaps)

Document:
---
{{numbered_markdown}}
---

Return ONLY a JSON array, no explanation or markdown fences."#;

    let issues_list = issues
        .iter()
        .map(|i| format!("- {i}"))
        .collect::<Vec<_>>()
        .join("\n");

    prompts.render_or_default(
        "document.smart_chunker_feedback",
        DEFAULT_TEMPLATE,
        &[
            ("issues_list", &issues_list),
            ("numbered_markdown", numbered_markdown),
        ],
    )
}

pub fn orchestrator_prompt(
    prompts: &PromptRegistry,
    snapshot: &thairag_core::types::PipelineSnapshot,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a document preprocessing pipeline orchestrator. Review the output of the stage that just completed and decide what happens next.

Pipeline state:
- Just completed: {{stage}}
- Document: {{mime}} ({{size}} bytes){{ocr}}
- Analysis: {{analysis}}
- Quality: {{quality}}
- Chunks: {{chunks}}
- Parameters: quality_threshold={{qt}}, max_chunk_size={{mcs}}
- Budget: {{calls}}/{{max_calls}} orchestrator calls used
- Previous decisions:
  {{history}}

Available actions (return one):
- "accept" — result is good, proceed to next stage
- "retry" — retry this agent with adjustments (include "adjustments" list of what to fix)
- "skip" — skip this stage, proceed with current results (include "reason")
- "fallback_mechanical" — abandon AI processing, use mechanical pipeline (include "reason")
- "flag_for_review" — accept result but mark document for human review (include "reason")
- "adjust_params" — change parameters for upcoming stages (include "params" object)

Guidelines:
- After analyzer: accept if confidence > 0.5. Retry with larger excerpt if 0.3-0.5. Fallback if < 0.3.
- After quality_checker: accept if score >= threshold. Retry converter if score is within 0.15 of threshold. Fallback if very low. For OCR docs, consider lowering threshold via adjust_params.
- After chunker: accept if no validation issues. Retry if minor issues. Flag_for_review if issues persist.
- You have {{remaining}} calls remaining. If at 0, you MUST accept or fallback.
- Be efficient — prefer accept when results are reasonable, not perfect.

Return ONLY valid JSON:
{"action": "accept|retry|skip|fallback_mechanical|flag_for_review|adjust_params", "reasoning": "one sentence", "confidence": 0.0-1.0, ...action-specific fields}"#;

    let history = if snapshot.decision_history.is_empty() {
        "None yet".to_string()
    } else {
        snapshot.decision_history.join("\n  ")
    };

    let analysis = match (
        &snapshot.analysis_confidence,
        &snapshot.analysis_language,
        &snapshot.analysis_content_type,
    ) {
        (Some(conf), Some(lang), Some(ct)) => {
            format!("confidence={conf:.2}, language={lang}, content_type={ct}")
        }
        _ => "not yet available".to_string(),
    };

    let quality = match (&snapshot.quality_overall, &snapshot.quality_issues) {
        (Some(score), Some(issues)) if !issues.is_empty() => {
            let issue_list = issues
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            format!("score={score:.2}, issues: {issue_list}")
        }
        (Some(score), _) => format!("score={score:.2}, no issues"),
        _ => "not yet checked".to_string(),
    };

    let chunks = match (&snapshot.chunk_count, &snapshot.chunk_issues) {
        (Some(count), Some(issues)) if !issues.is_empty() => {
            let issue_list = issues
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            format!("{count} chunks, issues: {issue_list}")
        }
        (Some(count), _) => format!("{count} chunks, no issues"),
        _ => "not yet chunked".to_string(),
    };

    let ocr = if snapshot.needs_ocr_correction.unwrap_or(false) {
        " (OCR document — be more lenient with quality)"
    } else {
        ""
    };

    let size = snapshot.doc_size_bytes.to_string();
    let qt = format!("{:.2}", snapshot.effective_quality_threshold);
    let mcs = snapshot.effective_max_chunk_size.to_string();
    let calls = snapshot.orchestrator_call_count.to_string();
    let max_calls = snapshot.max_orchestrator_calls.to_string();
    let remaining = snapshot
        .max_orchestrator_calls
        .saturating_sub(snapshot.orchestrator_call_count)
        .to_string();

    prompts.render_or_default(
        "document.orchestrator",
        DEFAULT_TEMPLATE,
        &[
            ("stage", &snapshot.completed_stage),
            ("mime", &snapshot.mime_type),
            ("size", &size),
            ("ocr", ocr),
            ("analysis", &analysis),
            ("quality", &quality),
            ("chunks", &chunks),
            ("qt", &qt),
            ("mcs", &mcs),
            ("calls", &calls),
            ("max_calls", &max_calls),
            ("remaining", &remaining),
            ("history", &history),
        ],
    )
}

/// Chunk enricher prompt — processes a batch of chunks to generate search-optimized metadata.
pub fn chunk_enricher_prompt(
    prompts: &PromptRegistry,
    chunks: &[(usize, &str)],
    document_title: &str,
    primary_language: &str,
    content_type: &str,
) -> String {
    const DEFAULT_TEMPLATE: &str = r#"You are a search optimization expert for Thai and English documents.
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
[{"chunk_index": 0, "context_prefix": "...", "summary": "...", "keywords": [...], "hypothetical_queries": [...]}]"#;

    let chunks_text = chunks
        .iter()
        .map(|(idx, content)| {
            let truncated = if content.len() > 1500 {
                format!("{}... [truncated]", &content[..1500])
            } else {
                content.to_string()
            };
            format!("[Chunk {idx}]\n{truncated}")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    prompts.render_or_default(
        "document.chunk_enricher",
        DEFAULT_TEMPLATE,
        &[
            ("document_title", document_title),
            ("primary_language", primary_language),
            ("content_type", content_type),
            ("chunks_text", &chunks_text),
        ],
    )
}
