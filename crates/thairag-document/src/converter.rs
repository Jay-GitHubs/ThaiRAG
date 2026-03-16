use std::io::Cursor;

use calamine::{Reader, open_workbook_auto_from_rs};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::DocumentProcessor;

/// Result of a document conversion including metadata.
#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub text: String,
    pub image_count: i32,
    pub table_count: i32,
}

/// Converts raw document bytes to markdown/text.
pub struct MarkdownConverter;

impl MarkdownConverter {
    pub fn new() -> Self {
        Self
    }

    /// Convert and return metadata (image/table counts) alongside the text.
    pub fn convert_with_stats(&self, raw: &[u8], mime_type: &str) -> Result<ConversionResult> {
        let text = self.convert(raw, mime_type)?;
        let image_count = count_image_refs(&text);
        let table_count = count_markdown_tables(&text);
        Ok(ConversionResult {
            text,
            image_count,
            table_count,
        })
    }
}

/// Count markdown image references: `![...](...)`
fn count_image_refs(text: &str) -> i32 {
    let mut count = 0i32;
    // Markdown images: ![alt](url)
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '!' {
            if chars.peek() == Some(&'[') {
                count += 1;
            }
        }
    }
    // HTML images: <img
    count += text.matches("<img ").count() as i32;
    count += text.matches("<img>").count() as i32;
    count
}

/// Count markdown tables (lines starting with |)
fn count_markdown_tables(text: &str) -> i32 {
    let mut count = 0i32;
    let mut in_table = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            if !in_table {
                count += 1;
                in_table = true;
            }
        } else {
            in_table = false;
        }
    }
    count
}

impl MarkdownConverter {
    /// Page-aware extraction. Returns (page_number, text) pairs.
    /// For PDFs, extracts text per page. For other formats, returns a single "page 1".
    pub fn convert_by_pages(&self, raw: &[u8], mime_type: &str) -> Result<Vec<(usize, String)>> {
        if mime_type == "application/pdf" {
            convert_pdf_by_pages(raw)
        } else {
            let text = self.convert(raw, mime_type)?;
            if text.trim().is_empty() {
                Ok(vec![])
            } else {
                Ok(vec![(1, text)])
            }
        }
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
    "text/html",
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
];

impl DocumentProcessor for MarkdownConverter {
    fn convert(&self, raw: &[u8], mime_type: &str) -> Result<String> {
        match mime_type {
            "text/markdown" | "text/plain" => {
                String::from_utf8(raw.to_vec()).map_err(|e| ThaiRagError::Validation(e.to_string()))
            }
            "text/csv" => convert_csv(raw),
            "text/html" => convert_html(raw),
            "application/pdf" => convert_pdf(raw),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                convert_docx(raw)
            }
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
                convert_xlsx(raw)
            }
            _ => Err(ThaiRagError::Validation(format!(
                "Unsupported MIME type: {mime_type}. Supported types: {}",
                SUPPORTED_MIME_TYPES.join(", ")
            ))),
        }
    }
}

fn convert_csv(raw: &[u8]) -> Result<String> {
    let mut reader = csv::ReaderBuilder::new().has_headers(true).from_reader(raw);

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ThaiRagError::Validation(format!("Failed to read CSV headers: {e}")))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    if headers.is_empty() {
        return Ok(String::new());
    }

    let mut output = String::new();

    // Header row
    output.push_str("| ");
    output.push_str(&headers.join(" | "));
    output.push_str(" |\n");

    // Separator row
    output.push_str("| ");
    output.push_str(
        &headers
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join(" | "),
    );
    output.push_str(" |\n");

    // Data rows
    for result in reader.records() {
        let record =
            result.map_err(|e| ThaiRagError::Validation(format!("Failed to read CSV row: {e}")))?;
        let cells: Vec<&str> = record.iter().collect();
        output.push_str("| ");
        // Pad with empty cells if needed
        let mut row_cells: Vec<String> = Vec::with_capacity(headers.len());
        for i in 0..headers.len() {
            row_cells.push(cells.get(i).unwrap_or(&"").to_string());
        }
        output.push_str(&row_cells.join(" | "));
        output.push_str(" |\n");
    }

    Ok(output)
}

fn convert_html(raw: &[u8]) -> Result<String> {
    let html_str =
        String::from_utf8(raw.to_vec()).map_err(|e| ThaiRagError::Validation(e.to_string()))?;

    let document = scraper::Html::parse_document(&html_str);
    let mut output = String::new();

    // Extract tables as markdown tables
    if let Ok(table_sel) = scraper::Selector::parse("table") {
        let tr_sel = scraper::Selector::parse("tr").unwrap();
        let th_sel = scraper::Selector::parse("th").unwrap();
        let td_sel = scraper::Selector::parse("td").unwrap();

        for table in document.select(&table_sel) {
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut has_header = false;

            for tr in table.select(&tr_sel) {
                let ths: Vec<String> = tr
                    .select(&th_sel)
                    .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                    .collect();
                if !ths.is_empty() {
                    has_header = true;
                    rows.push(ths);
                } else {
                    let tds: Vec<String> = tr
                        .select(&td_sel)
                        .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                        .collect();
                    if !tds.is_empty() {
                        rows.push(tds);
                    }
                }
            }

            if !rows.is_empty() {
                let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);

                // If first row is header, emit it with separator
                let start = if has_header {
                    let header = &rows[0];
                    output.push_str("| ");
                    let mut cells: Vec<String> = Vec::new();
                    for i in 0..ncols {
                        cells.push(header.get(i).cloned().unwrap_or_default());
                    }
                    output.push_str(&cells.join(" | "));
                    output.push_str(" |\n| ");
                    output.push_str(&(0..ncols).map(|_| "---").collect::<Vec<_>>().join(" | "));
                    output.push_str(" |\n");
                    1
                } else {
                    // No header, create generic column names
                    output.push_str("| ");
                    let cols: Vec<String> = (1..=ncols).map(|i| format!("Col {i}")).collect();
                    output.push_str(&cols.join(" | "));
                    output.push_str(" |\n| ");
                    output.push_str(&(0..ncols).map(|_| "---").collect::<Vec<_>>().join(" | "));
                    output.push_str(" |\n");
                    0
                };

                for row in &rows[start..] {
                    output.push_str("| ");
                    let mut cells: Vec<String> = Vec::new();
                    for i in 0..ncols {
                        cells.push(row.get(i).cloned().unwrap_or_default());
                    }
                    output.push_str(&cells.join(" | "));
                    output.push_str(" |\n");
                }
                output.push('\n');
            }
        }
    }

    // Extract images
    if let Ok(img_sel) = scraper::Selector::parse("img") {
        for img in document.select(&img_sel) {
            let src = img.value().attr("src").unwrap_or("");
            let alt = img.value().attr("alt").unwrap_or("image");
            if !src.is_empty() {
                output.push_str(&format!("![{alt}]({src})\n\n"));
            }
        }
    }

    // Extract text from body (excluding tables which we already processed)
    let root = if let Some(body) = scraper::Selector::parse("body")
        .ok()
        .and_then(|sel| document.select(&sel).next())
    {
        body
    } else {
        document.root_element()
    };

    let text: String = root.text().collect::<Vec<_>>().join(" ");
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if !normalized.is_empty() {
        output.push_str(&normalized);
        output.push('\n');
    }

    Ok(output.trim().to_string())
}

fn convert_pdf(raw: &[u8]) -> Result<String> {
    pdf_extract::extract_text_from_mem(raw)
        .map_err(|e| ThaiRagError::Validation(format!("Failed to read PDF: {e}")))
}

/// Extract PDF text page by page. Returns Vec of (1-indexed page number, text).
fn convert_pdf_by_pages(raw: &[u8]) -> Result<Vec<(usize, String)>> {
    let pages = pdf_extract::extract_text_from_mem_by_pages(raw)
        .map_err(|e| ThaiRagError::Validation(format!("Failed to read PDF: {e}")))?;
    Ok(pages
        .into_iter()
        .enumerate()
        .map(|(i, text)| (i + 1, text))
        .filter(|(_, text)| !text.trim().is_empty())
        .collect())
}

fn convert_xlsx(raw: &[u8]) -> Result<String> {
    let cursor = Cursor::new(raw);
    let mut workbook = open_workbook_auto_from_rs(cursor)
        .map_err(|e| ThaiRagError::Validation(format!("Failed to read XLSX: {e}")))?;

    let mut output = String::new();
    let sheet_names: Vec<String> = workbook.sheet_names().to_vec();

    for name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(name) {
            let rows: Vec<Vec<String>> = range
                .rows()
                .map(|row| row.iter().map(|c| c.to_string()).collect())
                .collect();

            if rows.is_empty() {
                continue;
            }

            if sheet_names.len() > 1 {
                output.push_str(&format!("## {name}\n\n"));
            }

            let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
            if ncols == 0 {
                continue;
            }

            // First row as header
            let header = &rows[0];
            output.push_str("| ");
            let mut cells: Vec<String> = Vec::with_capacity(ncols);
            for i in 0..ncols {
                cells.push(header.get(i).cloned().unwrap_or_default());
            }
            output.push_str(&cells.join(" | "));
            output.push_str(" |\n| ");
            output.push_str(&(0..ncols).map(|_| "---").collect::<Vec<_>>().join(" | "));
            output.push_str(" |\n");

            // Data rows
            for row in &rows[1..] {
                output.push_str("| ");
                let mut cells: Vec<String> = Vec::with_capacity(ncols);
                for i in 0..ncols {
                    cells.push(row.get(i).cloned().unwrap_or_default());
                }
                output.push_str(&cells.join(" | "));
                output.push_str(" |\n");
            }
            output.push('\n');
        }
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

fn extract_docx_cell_text(cell: &docx_rs::TableCell) -> String {
    let mut text = String::new();
    for tc_child in &cell.children {
        if let docx_rs::TableCellContent::Paragraph(p) = tc_child {
            for pc in &p.children {
                if let docx_rs::ParagraphChild::Run(run) = pc {
                    for rc in &run.children {
                        if let docx_rs::RunChild::Text(t) = rc {
                            text.push_str(&t.text);
                        }
                    }
                }
            }
        }
    }
    text.trim().to_string()
}

fn extract_docx_text(document: &docx_rs::Document, output: &mut String) {
    for child in &document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => {
                let mut line = String::new();
                for pc in &p.children {
                    if let docx_rs::ParagraphChild::Run(run) = pc {
                        for rc in &run.children {
                            if let docx_rs::RunChild::Text(t) = rc {
                                line.push_str(&t.text);
                            }
                        }
                    }
                }
                if !line.trim().is_empty() {
                    output.push_str(&line);
                }
                output.push('\n');
            }
            docx_rs::DocumentChild::Table(table) => {
                let mut rows: Vec<Vec<String>> = Vec::new();
                for row in &table.rows {
                    let docx_rs::TableChild::TableRow(tr) = row;
                    let mut cells: Vec<String> = Vec::new();
                    for cell in &tr.cells {
                        let docx_rs::TableRowChild::TableCell(tc) = cell;
                        cells.push(extract_docx_cell_text(tc));
                    }
                    rows.push(cells);
                }

                if !rows.is_empty() {
                    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
                    if ncols > 0 {
                        // First row as header
                        output.push_str("| ");
                        let mut hcells: Vec<String> = Vec::with_capacity(ncols);
                        for i in 0..ncols {
                            hcells.push(rows[0].get(i).cloned().unwrap_or_default());
                        }
                        output.push_str(&hcells.join(" | "));
                        output.push_str(" |\n| ");
                        output.push_str(&(0..ncols).map(|_| "---").collect::<Vec<_>>().join(" | "));
                        output.push_str(" |\n");

                        for row in &rows[1..] {
                            output.push_str("| ");
                            let mut cells: Vec<String> = Vec::with_capacity(ncols);
                            for i in 0..ncols {
                                cells.push(row.get(i).cloned().unwrap_or_default());
                            }
                            output.push_str(&cells.join(" | "));
                            output.push_str(" |\n");
                        }
                    }
                }
                output.push('\n');
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
        assert!(result.contains("| name | age | city |"));
        assert!(result.contains("| --- | --- | --- |"));
        assert!(result.contains("| Alice | 30 | Bangkok |"));
        assert!(result.contains("| Bob | 25 | Chiang Mai |"));
    }

    #[test]
    fn converts_csv_single_row() {
        let csv = b"key,value\nfoo,bar\n";
        let result = converter().convert(csv, "text/csv").unwrap();
        assert!(result.contains("| key | value |"));
        assert!(result.contains("| foo | bar |"));
    }

    #[test]
    fn converts_csv_empty_body() {
        let csv = b"col1,col2\n";
        let result = converter().convert(csv, "text/csv").unwrap();
        // Headers + separator but no data rows
        assert!(result.contains("| col1 | col2 |"));
        assert!(result.contains("| --- | --- |"));
    }

    // ── DOCX ────────────────────────────────────────────────────────

    #[test]
    fn converts_docx_programmatic() {
        // Build a minimal DOCX in memory using docx-rs
        let docx = docx_rs::Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Hello from DOCX")),
            )
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Second paragraph")),
            );
        let mut buf = Vec::new();
        docx.build()
            .pack(&mut std::io::Cursor::new(&mut buf))
            .unwrap();

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
        let result = converter().convert(b"data", "application/x-custom-unsupported");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unsupported MIME type"));
        assert!(err_msg.contains("application/x-custom-unsupported"));
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
        assert!(SUPPORTED_MIME_TYPES.contains(&"text/html"));
        assert!(SUPPORTED_MIME_TYPES.contains(&"application/pdf"));
        assert!(
            SUPPORTED_MIME_TYPES.contains(
                &"application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            )
        );
        assert!(
            SUPPORTED_MIME_TYPES
                .contains(&"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
        );
    }

    // ── HTML ────────────────────────────────────────────────────────

    #[test]
    fn converts_html_basic() {
        let html = b"<html><body><h1>Title</h1><p>Hello world</p></body></html>";
        let result = converter().convert(html, "text/html").unwrap();
        assert!(result.contains("Title"));
        assert!(result.contains("Hello world"));
    }

    #[test]
    fn converts_html_no_body_tag() {
        let html = b"<h1>Title</h1><p>Content here</p>";
        let result = converter().convert(html, "text/html").unwrap();
        assert!(result.contains("Title"));
        assert!(result.contains("Content here"));
    }

    #[test]
    fn rejects_invalid_html_utf8() {
        let bad = &[0xFF, 0xFE, 0xFD];
        let result = converter().convert(bad, "text/html");
        assert!(result.is_err());
    }

    // ── PDF ─────────────────────────────────────────────────────────

    #[test]
    fn rejects_invalid_pdf() {
        let bad = b"not a real pdf file";
        let result = converter().convert(bad, "application/pdf");
        assert!(result.is_err());
    }

    // ── XLSX ────────────────────────────────────────────────────────

    #[test]
    fn rejects_invalid_xlsx() {
        let bad = b"not a real xlsx file";
        let result = converter().convert(
            bad,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        );
        assert!(result.is_err());
    }

    // ── Conversion stats ────────────────────────────────────────────

    #[test]
    fn convert_with_stats_counts_tables() {
        let csv = b"a,b\n1,2\n3,4\n";
        let result = converter().convert_with_stats(csv, "text/csv").unwrap();
        assert_eq!(result.table_count, 1);
        assert_eq!(result.image_count, 0);
    }

    #[test]
    fn convert_with_stats_counts_images() {
        let md = b"# Doc\n\n![photo](img.png)\n\ntext\n\n![chart](chart.jpg)\n";
        let result = converter().convert_with_stats(md, "text/markdown").unwrap();
        assert_eq!(result.image_count, 2);
    }

    #[test]
    fn html_table_to_markdown() {
        let html = b"<html><body><table><tr><th>Name</th><th>Age</th></tr><tr><td>Alice</td><td>30</td></tr></table></body></html>";
        let result = converter().convert(html, "text/html").unwrap();
        assert!(result.contains("| Name | Age |"));
        assert!(result.contains("| --- | --- |"));
        assert!(result.contains("| Alice | 30 |"));
    }

    #[test]
    fn docx_table_to_markdown() {
        let docx = docx_rs::Docx::new().add_table(docx_rs::Table::new(vec![
            docx_rs::TableRow::new(vec![
                docx_rs::TableCell::new().add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Header1")),
                ),
                docx_rs::TableCell::new().add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Header2")),
                ),
            ]),
            docx_rs::TableRow::new(vec![
                docx_rs::TableCell::new().add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Val1")),
                ),
                docx_rs::TableCell::new().add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Val2")),
                ),
            ]),
        ]));
        let mut buf = Vec::new();
        docx.build()
            .pack(&mut std::io::Cursor::new(&mut buf))
            .unwrap();

        let result = converter()
            .convert(
                &buf,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            )
            .unwrap();
        assert!(result.contains("| Header1 | Header2 |"));
        assert!(result.contains("| Val1 | Val2 |"));
    }
}
