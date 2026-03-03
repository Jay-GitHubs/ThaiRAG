use thairag_core::error::Result;
use thairag_core::traits::DocumentProcessor;
use thairag_core::ThaiRagError;

/// Converts raw document bytes to markdown/text.
pub struct MarkdownConverter;

impl MarkdownConverter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MarkdownConverter {
    fn default() -> Self {
        Self::new()
    }
}

/// Supported MIME types for document conversion.
pub const SUPPORTED_MIME_TYPES: &[&str] = &[
    "text/markdown",
    "text/plain",
    "text/csv",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
];

impl DocumentProcessor for MarkdownConverter {
    fn convert(&self, raw: &[u8], mime_type: &str) -> Result<String> {
        match mime_type {
            "text/markdown" | "text/plain" => {
                String::from_utf8(raw.to_vec())
                    .map_err(|e| ThaiRagError::Validation(e.to_string()))
            }
            "text/csv" => convert_csv(raw),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                convert_docx(raw)
            }
            _ => Err(ThaiRagError::Validation(format!(
                "Unsupported MIME type: {mime_type}. Supported types: {}",
                SUPPORTED_MIME_TYPES.join(", ")
            ))),
        }
    }
}

fn convert_csv(raw: &[u8]) -> Result<String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(raw);

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ThaiRagError::Validation(format!("Failed to read CSV headers: {e}")))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut output = String::new();
    for result in reader.records() {
        let record = result
            .map_err(|e| ThaiRagError::Validation(format!("Failed to read CSV row: {e}")))?;
        for (i, field) in record.iter().enumerate() {
            if let Some(header) = headers.get(i) {
                output.push_str(header);
                output.push_str(": ");
            }
            output.push_str(field);
            output.push('\n');
        }
        output.push('\n');
    }

    Ok(output)
}

fn convert_docx(raw: &[u8]) -> Result<String> {
    let docx = docx_rs::read_docx(raw)
        .map_err(|e| ThaiRagError::Validation(format!("Failed to read DOCX: {e}")))?;

    let mut output = String::new();
    extract_docx_text(&docx.document, &mut output);
    Ok(output)
}

fn extract_docx_text(document: &docx_rs::Document, output: &mut String) {
    for child in &document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => {
                for pc in &p.children {
                    if let docx_rs::ParagraphChild::Run(run) = pc {
                        for rc in &run.children {
                            if let docx_rs::RunChild::Text(t) = rc {
                                output.push_str(&t.text);
                            }
                        }
                    }
                }
                output.push('\n');
            }
            docx_rs::DocumentChild::Table(table) => {
                for row in &table.rows {
                    let docx_rs::TableChild::TableRow(tr) = row;
                    for cell in &tr.cells {
                        let docx_rs::TableRowChild::TableCell(tc) = cell;
                        for tc_child in &tc.children {
                            if let docx_rs::TableCellContent::Paragraph(p) = tc_child {
                                for pc in &p.children {
                                    if let docx_rs::ParagraphChild::Run(run) = pc {
                                        for rc in &run.children {
                                            if let docx_rs::RunChild::Text(t) = rc {
                                                output.push_str(&t.text);
                                            }
                                        }
                                    }
                                }
                                output.push('\t');
                            }
                        }
                    }
                    output.push('\n');
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::traits::DocumentProcessor;

    fn converter() -> MarkdownConverter {
        MarkdownConverter::new()
    }

    // ── Plaintext / Markdown ────────────────────────────────────────

    #[test]
    fn converts_plain_text() {
        let text = b"Hello, world!";
        let result = converter().convert(text, "text/plain").unwrap();
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn converts_markdown() {
        let md = b"# Title\n\nSome **bold** text.";
        let result = converter().convert(md, "text/markdown").unwrap();
        assert_eq!(result, "# Title\n\nSome **bold** text.");
    }

    #[test]
    fn rejects_invalid_utf8() {
        let bad = &[0xFF, 0xFE, 0xFD];
        let result = converter().convert(bad, "text/plain");
        assert!(result.is_err());
    }

    // ── CSV ─────────────────────────────────────────────────────────

    #[test]
    fn converts_csv_basic() {
        let csv = b"name,age,city\nAlice,30,Bangkok\nBob,25,Chiang Mai\n";
        let result = converter().convert(csv, "text/csv").unwrap();
        assert!(result.contains("name: Alice"));
        assert!(result.contains("age: 30"));
        assert!(result.contains("city: Bangkok"));
        assert!(result.contains("name: Bob"));
        assert!(result.contains("city: Chiang Mai"));
    }

    #[test]
    fn converts_csv_single_row() {
        let csv = b"key,value\nfoo,bar\n";
        let result = converter().convert(csv, "text/csv").unwrap();
        assert!(result.contains("key: foo"));
        assert!(result.contains("value: bar"));
    }

    #[test]
    fn converts_csv_empty_body() {
        let csv = b"col1,col2\n";
        let result = converter().convert(csv, "text/csv").unwrap();
        // Only headers, no data rows → empty output
        assert!(result.is_empty());
    }

    // ── DOCX ────────────────────────────────────────────────────────

    #[test]
    fn converts_docx_programmatic() {
        // Build a minimal DOCX in memory using docx-rs
        let docx = docx_rs::Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new()
                    .add_run(docx_rs::Run::new().add_text("Hello from DOCX")),
            )
            .add_paragraph(
                docx_rs::Paragraph::new()
                    .add_run(docx_rs::Run::new().add_text("Second paragraph")),
            );
        let mut buf = Vec::new();
        docx.build().pack(&mut std::io::Cursor::new(&mut buf)).unwrap();

        let result = converter()
            .convert(
                &buf,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            )
            .unwrap();
        assert!(result.contains("Hello from DOCX"));
        assert!(result.contains("Second paragraph"));
    }

    #[test]
    fn rejects_invalid_docx() {
        let bad = b"not a real docx file";
        let result = converter().convert(
            bad,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        assert!(result.is_err());
    }

    // ── Unsupported types ───────────────────────────────────────────

    #[test]
    fn rejects_unsupported_mime_type() {
        let result = converter().convert(b"data", "application/pdf");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unsupported MIME type"));
        assert!(err_msg.contains("application/pdf"));
    }

    #[test]
    fn rejects_unknown_mime_type() {
        let result = converter().convert(b"data", "application/x-custom");
        assert!(result.is_err());
    }

    #[test]
    fn supported_mime_types_list_is_correct() {
        assert!(SUPPORTED_MIME_TYPES.contains(&"text/plain"));
        assert!(SUPPORTED_MIME_TYPES.contains(&"text/markdown"));
        assert!(SUPPORTED_MIME_TYPES.contains(&"text/csv"));
        assert!(SUPPORTED_MIME_TYPES.contains(
            &"application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
        assert!(!SUPPORTED_MIME_TYPES.contains(&"application/pdf"));
    }
}
