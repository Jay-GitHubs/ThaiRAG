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
