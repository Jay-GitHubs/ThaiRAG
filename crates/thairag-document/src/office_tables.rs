//! Structured table extraction for office formats (DOCX, XLSX) → HTML.
//!
//! Like the PDF lattice path, this preserves a table's exact cell text and grid
//! structure instead of flattening to markdown (which loses merged cells and
//! desyncs columns). DOCX merges are recovered faithfully: `gridSpan` →
//! `colspan`, `vMerge` (restart/continue) → `rowspan`. XLSX is grid-faithful
//! (calamine 0.26 does not expose merged ranges, so merged cells render with
//! the value in the anchor cell — exact, never fabricated).
//!
//! Cell content always comes from the document model — never a model — so
//! numbers cannot be hallucinated.

use std::io::Cursor;

use calamine::{Reader, open_workbook_auto_from_rs};

use thairag_core::error::Result;

/// One reconstructed office table.
#[derive(Debug, Clone)]
pub struct OfficeTable {
    /// Escaped HTML (`<table>` with colspan/rowspan) — the chunk payload.
    pub html: String,
    /// Row-linearized, merge-filled text for embedding/search.
    pub linearized: String,
    pub rows: usize,
    pub cols: usize,
}

/// A structured office document: the canonical markdown (prose + inline
/// `<table>` HTML, in reading order) for display, the prose-only text (no
/// tables) for the chunker, and each table separately for atomic chunking.
#[derive(Debug, Clone)]
pub struct OfficeDoc {
    pub markdown: String,
    pub prose: String,
    pub tables: Vec<OfficeTable>,
}

/// A placed cell in the logical grid (top-left anchor of any span).
struct OutCell {
    row: usize,
    col: usize,
    text: String,
    colspan: usize,
    rowspan: usize,
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Assemble placed cells (with spans) into HTML + linearized text. `n_rows`/
/// `n_cols` are the logical grid dimensions.
fn assemble(cells: &[OutCell], n_rows: usize, n_cols: usize) -> OfficeTable {
    // HTML: emit each anchor at its (row,col); spans handle the rest.
    let mut html = String::from("<table>");
    for r in 0..n_rows {
        html.push_str("<tr>");
        for c in 0..n_cols {
            if let Some(cell) = cells.iter().find(|x| x.row == r && x.col == c) {
                let mut td = String::from("<td");
                if cell.colspan > 1 {
                    td.push_str(&format!(" colspan=\"{}\"", cell.colspan));
                }
                if cell.rowspan > 1 {
                    td.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
                }
                td.push('>');
                td.push_str(&escape_html(&cell.text));
                td.push_str("</td>");
                html.push_str(&td);
            }
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");

    // Linearized: dense grid with merged values filled across their span.
    let mut fill = vec![vec![String::new(); n_cols]; n_rows];
    for cell in cells {
        for dr in 0..cell.rowspan {
            for dc in 0..cell.colspan {
                if cell.row + dr < n_rows && cell.col + dc < n_cols {
                    fill[cell.row + dr][cell.col + dc] = cell.text.clone();
                }
            }
        }
    }
    let linearized = fill
        .iter()
        .map(|row| row.join(" | "))
        .collect::<Vec<_>>()
        .join("\n");

    OfficeTable {
        html,
        linearized,
        rows: n_rows,
        cols: n_cols,
    }
}

// ── DOCX ────────────────────────────────────────────────────────────

fn docx_cell_text(tc: &docx_rs::TableCell) -> String {
    let mut s = String::new();
    for content in &tc.children {
        if let docx_rs::TableCellContent::Paragraph(p) = content {
            for pc in &p.children {
                if let docx_rs::ParagraphChild::Run(run) = pc {
                    for rc in &run.children {
                        if let docx_rs::RunChild::Text(t) = rc {
                            s.push_str(&t.text);
                        }
                    }
                }
            }
            s.push(' ');
        }
    }
    s.trim().to_string()
}

/// Read `gridSpan` (→ colspan) and `vMerge` (restart/continue) from a cell's
/// properties. docx-rs keeps these private but serialises them (gridSpan as a
/// bare number, vMerge as "restart"/"continue"), so JSON is the read path.
fn docx_cell_spans(tc: &docx_rs::TableCell) -> (usize, Option<String>) {
    let v = serde_json::to_value(&tc.property).unwrap_or_default();
    let colspan = v
        .get("gridSpan")
        .and_then(|x| x.as_u64())
        .unwrap_or(1)
        .max(1) as usize;
    let vmerge = v
        .get("verticalMerge")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    (colspan, vmerge)
}

fn docx_table_to_office(table: &docx_rs::Table) -> Option<OfficeTable> {
    let mut cells: Vec<OutCell> = Vec::new();
    // For each logical column, the index into `cells` of the anchor currently
    // open for a vertical merge (so a "continue" row extends its rowspan).
    let mut col_owner: Vec<Option<usize>> = Vec::new();
    let mut n_cols = 0usize;
    let mut row_idx = 0usize;

    for row in &table.rows {
        let docx_rs::TableChild::TableRow(tr) = row;
        let mut col = 0usize;
        for cell in &tr.cells {
            let docx_rs::TableRowChild::TableCell(tc) = cell;
            let (colspan, vmerge) = docx_cell_spans(tc);
            if col + colspan > col_owner.len() {
                col_owner.resize(col + colspan, None);
            }
            match vmerge.as_deref() {
                Some("continue") => {
                    // Extend the merge that started above this column.
                    if let Some(owner) = col_owner.get(col).copied().flatten() {
                        cells[owner].rowspan += 1;
                    }
                }
                other => {
                    let idx = cells.len();
                    cells.push(OutCell {
                        row: row_idx,
                        col,
                        text: docx_cell_text(tc),
                        colspan,
                        rowspan: 1,
                    });
                    let owner = if other == Some("restart") {
                        Some(idx)
                    } else {
                        None
                    };
                    col_owner[col..col + colspan].fill(owner);
                }
            }
            col += colspan;
        }
        n_cols = n_cols.max(col);
        row_idx += 1;
    }

    if cells.is_empty() || n_cols == 0 {
        return None;
    }
    Some(assemble(&cells, row_idx, n_cols))
}

/// Extract every table in a DOCX, in document order.
pub fn docx_tables(raw: &[u8]) -> Result<Vec<OfficeTable>> {
    let docx = docx_rs::read_docx(raw)
        .map_err(|e| thairag_core::ThaiRagError::Validation(format!("Failed to read DOCX: {e}")))?;
    let mut out = Vec::new();
    for child in &docx.document.children {
        if let docx_rs::DocumentChild::Table(table) = child
            && let Some(t) = docx_table_to_office(table)
        {
            out.push(t);
        }
    }
    Ok(out)
}

fn docx_paragraph_text(p: &docx_rs::Paragraph) -> String {
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
    line
}

/// Walk a DOCX in reading order, emitting prose paragraphs as text and each
/// table as inline HTML, while collecting the tables for atomic chunking.
pub fn convert_docx_structured(raw: &[u8]) -> Result<OfficeDoc> {
    let docx = docx_rs::read_docx(raw)
        .map_err(|e| thairag_core::ThaiRagError::Validation(format!("Failed to read DOCX: {e}")))?;
    let mut markdown = String::new();
    let mut prose = String::new();
    let mut tables = Vec::new();
    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => {
                let line = docx_paragraph_text(p);
                if !line.trim().is_empty() {
                    markdown.push_str(&line);
                    markdown.push('\n');
                    prose.push_str(&line);
                    prose.push('\n');
                }
            }
            docx_rs::DocumentChild::Table(table) => {
                if let Some(t) = docx_table_to_office(table) {
                    markdown.push_str("\n\n");
                    markdown.push_str(&t.html);
                    markdown.push_str("\n\n");
                    tables.push(t);
                }
            }
            _ => {}
        }
    }
    Ok(OfficeDoc {
        markdown,
        prose,
        tables,
    })
}

/// Convert an XLSX to a structured doc: each sheet becomes one HTML table.
/// There is no prose, so the chunker input is empty and all chunks are tables.
pub fn convert_xlsx_structured(raw: &[u8]) -> Result<OfficeDoc> {
    let tables = xlsx_tables(raw)?;
    let mut markdown = String::new();
    for t in &tables {
        markdown.push_str(&t.html);
        markdown.push_str("\n\n");
    }
    Ok(OfficeDoc {
        markdown,
        prose: String::new(),
        tables,
    })
}

// ── XLSX ────────────────────────────────────────────────────────────

/// Extract every sheet of an XLSX as a grid-faithful table (one per sheet).
/// calamine 0.26 exposes no merged ranges, so merged cells appear as the value
/// in the anchor cell plus blanks — exact content, no fabrication.
pub fn xlsx_tables(raw: &[u8]) -> Result<Vec<OfficeTable>> {
    let mut wb = open_workbook_auto_from_rs(Cursor::new(raw))
        .map_err(|e| thairag_core::ThaiRagError::Validation(format!("Failed to read XLSX: {e}")))?;
    let mut out = Vec::new();
    let names: Vec<String> = wb.sheet_names().to_vec();
    for name in &names {
        let Ok(range) = wb.worksheet_range(name) else {
            continue;
        };
        let grid: Vec<Vec<String>> = range
            .rows()
            .map(|row| row.iter().map(|c| c.to_string()).collect())
            .collect();
        if grid.is_empty() {
            continue;
        }
        let n_cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
        if n_cols == 0 {
            continue;
        }
        let mut cells = Vec::new();
        for (r, row) in grid.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                if !text.is_empty() {
                    cells.push(OutCell {
                        row: r,
                        col: c,
                        text: text.clone(),
                        colspan: 1,
                        rowspan: 1,
                    });
                }
            }
        }
        out.push(assemble(&cells, grid.len(), n_cols));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::*;

    fn cell(text: &str) -> TableCell {
        TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(text)))
    }

    #[test]
    fn docx_colspan_from_grid_span() {
        // Header row: one cell spanning 2 columns; data row: two cells.
        let table = Table::new(vec![
            TableRow::new(vec![cell("Header").grid_span(2)]),
            TableRow::new(vec![cell("a"), cell("b")]),
        ]);
        let mut buf = Cursor::new(Vec::new());
        Docx::new()
            .add_table(table)
            .build()
            .pack(&mut buf)
            .expect("pack docx");
        let bytes = buf.into_inner();
        let tables = docx_tables(&bytes).expect("read");
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.cols, 2, "logical cols");
        assert!(t.html.contains("colspan=\"2\""), "html: {}", t.html);
        assert!(t.html.contains("Header"));
        assert!(t.html.contains("<td>a</td><td>b</td>"), "html: {}", t.html);
        // Linearized fills the merged header across both columns.
        assert_eq!(t.linearized, "Header | Header\na | b");
    }

    #[test]
    fn docx_rowspan_from_vmerge() {
        // Column 0 vertically merged across two rows (restart then continue).
        let table = Table::new(vec![
            TableRow::new(vec![
                cell("M").vertical_merge(VMergeType::Restart),
                cell("x"),
            ]),
            TableRow::new(vec![
                cell("").vertical_merge(VMergeType::Continue),
                cell("y"),
            ]),
        ]);
        let mut buf = Cursor::new(Vec::new());
        Docx::new()
            .add_table(table)
            .build()
            .pack(&mut buf)
            .expect("pack docx");
        let bytes = buf.into_inner();
        let tables = docx_tables(&bytes).expect("read");
        let t = &tables[0];
        assert!(t.html.contains("rowspan=\"2\""), "html: {}", t.html);
        assert!(t.html.contains("<td>x</td>"));
        assert!(t.html.contains("<td>y</td>"));
    }
}
