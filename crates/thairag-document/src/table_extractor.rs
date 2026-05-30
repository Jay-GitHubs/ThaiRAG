use thairag_core::types::ExtractedTable;

/// Extract tables from text content using heuristic detection.
///
/// Detects:
/// - Pipe-separated tables (markdown-style: `| col1 | col2 |`)
/// - Tab-separated tables (TSV-like blocks)
/// - Aligned column tables (fixed-width, space-padded)
pub fn extract_tables(text: &str) -> Vec<ExtractedTable> {
    let mut tables = Vec::new();

    // 1. Pipe-separated tables (most common in markdown/PDF text)
    tables.extend(extract_pipe_tables(text));

    // 2. Tab-separated tables
    tables.extend(extract_tsv_tables(text));

    tables
}

/// Heuristic: does this text *look* like it contains a table?
///
/// Used by the smart-PDF strategy selector to decide whether to route a page
/// through the vision model's table-extraction prompt. Ported from
/// `Jay-RAG-Tools/crates/core/src/table.rs`. Two independent signals; either
/// triggers:
/// 1. **Multi-space columns** — ≥40% of non-empty lines have ≥2 groups of 2+
///    consecutive spaces/tabs.
/// 2. **Row consistency** — ≥6 consecutive lines each with ≥3 whitespace
///    tokens whose counts vary by ≤2 (catches pdfium collapsing column gaps to
///    single spaces). The run threshold of 6 avoids false positives from
///    bullet lists and tables of contents.
pub fn looks_like_table(text: &str) -> bool {
    let non_empty: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if non_empty.len() < 3 {
        return false;
    }

    // Method 1: multi-space column detection.
    let tabular_lines = non_empty
        .iter()
        .filter(|line| {
            let mut space_groups = 0;
            let mut in_spaces = false;
            let mut space_count = 0;
            for ch in line.chars() {
                if ch == ' ' || ch == '\t' {
                    space_count += 1;
                    if space_count >= 2 && !in_spaces {
                        space_groups += 1;
                        in_spaces = true;
                    }
                } else {
                    space_count = 0;
                    in_spaces = false;
                }
            }
            space_groups >= 2
        })
        .count();

    if (tabular_lines as f64 / non_empty.len() as f64) >= 0.4 {
        return true;
    }

    // Method 2: row consistency — consecutive lines with similar token counts.
    let token_counts: Vec<usize> = non_empty
        .iter()
        .map(|line| line.split_whitespace().count())
        .collect();

    let mut best_run = 1;
    let mut current_run = 1;
    for i in 1..token_counts.len() {
        let prev = token_counts[i - 1];
        let curr = token_counts[i];
        if prev >= 3 && curr >= 3 && ((prev as isize) - (curr as isize)).abs() <= 2 {
            current_run += 1;
            best_run = best_run.max(current_run);
        } else {
            current_run = 1;
        }
    }

    best_run >= 6
}

/// Convert an `ExtractedTable` to a markdown table string.
pub fn table_to_markdown(table: &ExtractedTable) -> String {
    if table.headers.is_empty() && table.rows.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();

    // Header row
    if !table.headers.is_empty() {
        let header = format!("| {} |", table.headers.join(" | "));
        let separator = format!(
            "| {} |",
            table
                .headers
                .iter()
                .map(|h| "-".repeat(h.len().max(3)))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        lines.push(header);
        lines.push(separator);
    }

    // Data rows
    for row in &table.rows {
        let line = format!("| {} |", row.join(" | "));
        lines.push(line);
    }

    lines.join("\n")
}

// ── Table-aware chunking ─────────────────────────────────────────────
//
// Tables embed and retrieve badly when a chunker splits them: a chunk of data
// rows without the header `| Col | Col |` row is meaningless to the LLM at
// inference. These helpers let a chunker keep a markdown (pipe) table intact as
// one chunk, and — when a table is too large for one chunk — split it by rows
// while repeating the header (+ separator) on every fragment so each piece is
// self-describing.

/// The raw lines of a contiguous markdown (pipe) table: the header row, an
/// optional `|---|---|` separator, and the data rows. Lines are kept verbatim.
#[derive(Debug, Clone, PartialEq)]
pub struct TableBlock {
    pub header: String,
    pub separator: Option<String>,
    pub rows: Vec<String>,
}

/// A span of source text: either free text (chunk it normally) or a table
/// (keep atomic / header-propagate). Produced by [`split_table_segments`].
#[derive(Debug, Clone, PartialEq)]
pub enum TextSegment {
    Text(String),
    Table(TableBlock),
}

/// Split `text` into alternating free-text and table segments. A table segment
/// is a run of ≥2 consecutive pipe-rows that parses as a real table; everything
/// else (including isolated pipe-bearing prose lines) stays free text.
pub fn split_table_segments(text: &str) -> Vec<TextSegment> {
    let lines: Vec<&str> = text.lines().collect();
    let mut segments = Vec::new();
    let mut text_buf: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if count_pipes(lines[i]) >= 2 {
            let start = i;
            while i < lines.len() && count_pipes(lines[i]) >= 2 {
                i += 1;
            }
            let block = &lines[start..i];
            if block.len() >= 2
                && let Some(tb) = parse_table_block_lines(block)
            {
                if !text_buf.is_empty() {
                    segments.push(TextSegment::Text(text_buf.join("\n")));
                    text_buf.clear();
                }
                segments.push(TextSegment::Table(tb));
                continue;
            }
            // Not a real table — fall through and treat the run as text.
            text_buf.extend_from_slice(block);
        } else {
            text_buf.push(lines[i]);
            i += 1;
        }
    }

    if !text_buf.is_empty() {
        segments.push(TextSegment::Text(text_buf.join("\n")));
    }
    segments
}

/// Validate a run of pipe-lines is a real table and split it into header /
/// separator / rows (verbatim lines). Returns `None` if it isn't a table.
fn parse_table_block_lines(lines: &[&str]) -> Option<TableBlock> {
    // Reuse the cell-level parser purely as a validity gate.
    parse_pipe_block(lines)?;

    let has_separator = lines.len() > 1 && is_separator_line(lines[1]);
    let (separator, rows) = if has_separator {
        (
            Some(lines[1].to_string()),
            lines[2..].iter().map(|l| l.to_string()).collect(),
        )
    } else {
        (None, lines[1..].iter().map(|l| l.to_string()).collect())
    };

    Some(TableBlock {
        header: lines[0].to_string(),
        separator,
        rows,
    })
}

/// Chunk a single table block. If the whole table fits in `max_size` it is
/// emitted as one atomic chunk; otherwise rows are split into groups that fit,
/// each prefixed with the header (and separator) so no fragment loses its
/// column context. Sizes are measured in characters. A lone row larger than
/// `max_size` is still emitted whole (with its header) rather than dropped.
pub fn chunk_table_block(block: &TableBlock, max_size: usize) -> Vec<String> {
    let head = match &block.separator {
        Some(sep) => format!("{}\n{}", block.header, sep),
        None => block.header.clone(),
    };
    let head_len = head.chars().count();

    let mut whole = head.clone();
    for row in &block.rows {
        whole.push('\n');
        whole.push_str(row);
    }
    if whole.chars().count() <= max_size {
        return vec![whole];
    }

    let mut chunks = Vec::new();
    let mut current = head.clone();
    let mut current_len = head_len;
    let mut has_rows = false;

    for row in &block.rows {
        let row_len = row.chars().count() + 1; // + newline
        if has_rows && current_len + row_len > max_size {
            chunks.push(std::mem::take(&mut current));
            current = head.clone();
            current_len = head_len;
        }
        current.push('\n');
        current.push_str(row);
        current_len += row_len;
        has_rows = true;
    }
    if has_rows {
        chunks.push(current);
    }
    chunks
}

/// Wrap an inner chunking strategy with table-awareness. Free-text spans go to
/// `inner` unchanged; table spans are kept atomic / header-propagated via
/// [`chunk_table_block`]. When the text contains no tables, `inner` is called
/// on the original text verbatim (zero behavioral change).
pub fn chunk_table_aware<F>(text: &str, max_size: usize, overlap: usize, inner: F) -> Vec<String>
where
    F: Fn(&str, usize, usize) -> Vec<String>,
{
    let segments = split_table_segments(text);

    // Fast path: no table → behave exactly like `inner` on the whole input.
    if !segments.iter().any(|s| matches!(s, TextSegment::Table(_))) {
        return inner(text, max_size, overlap);
    }

    let mut out = Vec::new();
    for seg in segments {
        match seg {
            TextSegment::Text(t) => {
                if !t.trim().is_empty() {
                    out.extend(inner(&t, max_size, overlap));
                }
            }
            TextSegment::Table(tb) => out.extend(chunk_table_block(&tb, max_size)),
        }
    }
    out
}

/// Detect pipe-separated tables in text.
/// A pipe table is a consecutive block of lines where each line contains `|`.
fn extract_pipe_tables(text: &str) -> Vec<ExtractedTable> {
    let mut tables = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Look for a line with pipes
        if count_pipes(lines[i]) >= 2 {
            let start = i;
            // Collect consecutive pipe-containing lines
            while i < lines.len() && count_pipes(lines[i]) >= 2 {
                i += 1;
            }
            let block = &lines[start..i];

            if block.len() >= 2
                && let Some(table) = parse_pipe_block(block)
            {
                tables.push(table);
            }
        } else {
            i += 1;
        }
    }

    tables
}

/// Parse a block of pipe-separated lines into an ExtractedTable.
fn parse_pipe_block(lines: &[&str]) -> Option<ExtractedTable> {
    let parse_row = |line: &str| -> Vec<String> {
        line.split('|')
            .map(|cell| cell.trim().to_string())
            .filter(|cell| !cell.is_empty())
            .collect()
    };

    let first_row = parse_row(lines[0]);
    if first_row.is_empty() {
        return None;
    }

    // Check if second line is a separator (e.g., |---|---|)
    let has_separator = lines.len() > 1 && is_separator_line(lines[1]);
    let data_start = if has_separator { 2 } else { 1 };

    let headers = first_row;
    let rows: Vec<Vec<String>> = lines[data_start..]
        .iter()
        .map(|line| parse_row(line))
        .filter(|row| !row.is_empty())
        .collect();

    if rows.is_empty() && !has_separator {
        // Single row without separator — not a table
        return None;
    }

    Some(ExtractedTable {
        headers,
        rows,
        source_page: None,
    })
}

/// Check if a line is a markdown table separator (e.g., `|---|---|`).
fn is_separator_line(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }
    // Every cell between pipes should be only dashes, colons, and spaces
    trimmed
        .split('|')
        .filter(|s| !s.trim().is_empty())
        .all(|cell| {
            let c = cell.trim();
            !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
        })
}

/// Count the number of pipe characters in a line.
fn count_pipes(line: &str) -> usize {
    line.chars().filter(|&c| c == '|').count()
}

/// Detect tab-separated tables: consecutive lines each containing at least one tab,
/// with a consistent number of columns.
fn extract_tsv_tables(text: &str) -> Vec<ExtractedTable> {
    let mut tables = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].contains('\t') {
            let start = i;
            let first_cols = lines[i].split('\t').count();

            // Collect consecutive tab-containing lines with similar column count
            while i < lines.len()
                && lines[i].contains('\t')
                && (lines[i].split('\t').count() as isize - first_cols as isize).unsigned_abs() <= 1
            {
                i += 1;
            }

            let block = &lines[start..i];
            if block.len() >= 2 {
                let headers: Vec<String> =
                    block[0].split('\t').map(|s| s.trim().to_string()).collect();
                let rows: Vec<Vec<String>> = block[1..]
                    .iter()
                    .map(|line| line.split('\t').map(|s| s.trim().to_string()).collect())
                    .collect();

                tables.push(ExtractedTable {
                    headers,
                    rows,
                    source_page: None,
                });
            }
        } else {
            i += 1;
        }
    }

    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_markdown_pipe_table() {
        let text = "\
Some text before the table.

| Name | Age | City |
|------|-----|------|
| Alice | 30 | NYC |
| Bob | 25 | LA |

Some text after.";

        let tables = extract_tables(text);
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.headers, vec!["Name", "Age", "City"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0], vec!["Alice", "30", "NYC"]);
        assert_eq!(t.rows[1], vec!["Bob", "25", "LA"]);
    }

    #[test]
    fn extract_tsv_table() {
        let text = "Name\tAge\tCity\nAlice\t30\tNYC\nBob\t25\tLA\n";

        let tables = extract_tables(text);
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.headers, vec!["Name", "Age", "City"]);
        assert_eq!(t.rows.len(), 2);
    }

    #[test]
    fn table_to_markdown_roundtrip() {
        let table = ExtractedTable {
            headers: vec!["Col1".into(), "Col2".into()],
            rows: vec![vec!["A".into(), "B".into()], vec!["C".into(), "D".into()]],
            source_page: None,
        };
        let md = table_to_markdown(&table);
        assert!(md.contains("| Col1 | Col2 |"));
        assert!(md.contains("| A | B |"));
        assert!(md.contains("| C | D |"));
        // Separator line
        assert!(md.contains("---"));
    }

    #[test]
    fn no_tables_in_plain_text() {
        let text = "This is just some plain text.\nNo tables here at all.\nJust paragraphs.";
        let tables = extract_tables(text);
        assert!(tables.is_empty());
    }

    #[test]
    fn empty_table_to_markdown() {
        let table = ExtractedTable {
            headers: vec![],
            rows: vec![],
            source_page: None,
        };
        let md = table_to_markdown(&table);
        assert!(md.is_empty());
    }

    #[test]
    fn multiple_pipe_tables() {
        let text = "\
| A | B |
|---|---|
| 1 | 2 |

Some text between tables.

| X | Y | Z |
|---|---|---|
| a | b | c |
| d | e | f |";

        let tables = extract_tables(text);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].headers.len(), 2);
        assert_eq!(tables[1].headers.len(), 3);
    }

    // ── Table-aware chunking ────────────────────────────────────────

    #[test]
    fn split_segments_separates_text_and_table() {
        let text = "\
Intro line.

| Name | Age |
|------|-----|
| Alice | 30 |
| Bob | 25 |

Closing line.";
        let segs = split_table_segments(text);
        assert_eq!(segs.len(), 3);
        assert!(matches!(&segs[0], TextSegment::Text(t) if t.contains("Intro")));
        assert!(matches!(&segs[1], TextSegment::Table(_)));
        assert!(matches!(&segs[2], TextSegment::Text(t) if t.contains("Closing")));
        if let TextSegment::Table(tb) = &segs[1] {
            assert_eq!(tb.header, "| Name | Age |");
            assert_eq!(tb.separator.as_deref(), Some("|------|-----|"));
            assert_eq!(tb.rows.len(), 2);
        }
    }

    #[test]
    fn split_segments_isolated_pipe_line_is_text() {
        // A single pipe-bearing prose line is not a table.
        let text = "Use a | b syntax for alternatives.";
        let segs = split_table_segments(text);
        assert_eq!(segs.len(), 1);
        assert!(matches!(&segs[0], TextSegment::Text(_)));
    }

    #[test]
    fn chunk_table_block_atomic_when_fits() {
        let block = TableBlock {
            header: "| Name | Age |".into(),
            separator: Some("|------|-----|".into()),
            rows: vec!["| Alice | 30 |".into(), "| Bob | 25 |".into()],
        };
        let chunks = chunk_table_block(&block, 1000);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("| Name | Age |"));
        assert!(chunks[0].contains("| Alice | 30 |"));
        assert!(chunks[0].contains("| Bob | 25 |"));
    }

    #[test]
    fn chunk_table_block_propagates_header_when_split() {
        let block = TableBlock {
            header: "| Name | Age |".into(),
            separator: Some("|------|-----|".into()),
            rows: vec![
                "| Alice | 30 |".into(),
                "| Bob | 25 |".into(),
                "| Carol | 40 |".into(),
            ],
        };
        // Header (~29 chars incl. separator+newline) + one row per chunk.
        let chunks = chunk_table_block(&block, 45);
        assert!(chunks.len() >= 2, "expected a split, got {chunks:?}");
        // Every fragment repeats the header and separator.
        for c in &chunks {
            assert!(c.contains("| Name | Age |"), "missing header in: {c}");
            assert!(c.contains("|------|-----|"), "missing separator in: {c}");
        }
        // No data row is lost.
        let joined = chunks.join("\n");
        assert!(joined.contains("| Alice | 30 |"));
        assert!(joined.contains("| Bob | 25 |"));
        assert!(joined.contains("| Carol | 40 |"));
    }

    #[test]
    fn chunk_table_block_oversized_row_kept_whole() {
        let block = TableBlock {
            header: "| H |".into(),
            separator: None,
            rows: vec!["| this single row is far larger than max |".into()],
        };
        let chunks = chunk_table_block(&block, 5);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("far larger than max"));
        assert!(chunks[0].contains("| H |"));
    }

    #[test]
    fn chunk_table_aware_no_table_is_passthrough() {
        let text = "just some\nplain text\nno pipes";
        let called = std::cell::Cell::new(false);
        let out = chunk_table_aware(text, 100, 0, |t, _m, _o| {
            called.set(true);
            // Identity inner: prove the original text is passed verbatim.
            assert_eq!(t, text);
            vec![t.to_string()]
        });
        assert!(called.get());
        assert_eq!(out, vec![text.to_string()]);
    }

    #[test]
    fn chunk_table_aware_routes_table_atomically() {
        let text = "\
Before.

| K | V |
|---|---|
| a | 1 |
| b | 2 |

After.";
        // Inner splits text on blank lines; tables must bypass it and stay whole.
        let out = chunk_table_aware(text, 1000, 0, |t, _m, _o| {
            t.split("\n\n")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });
        // The table chunk should contain header + both rows intact.
        let table_chunk = out
            .iter()
            .find(|c| c.contains("| K | V |"))
            .expect("table chunk present");
        assert!(table_chunk.contains("| a | 1 |"));
        assert!(table_chunk.contains("| b | 2 |"));
        // Surrounding text preserved.
        assert!(out.iter().any(|c| c == "Before."));
        assert!(out.iter().any(|c| c == "After."));
    }
}
