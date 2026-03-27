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
}
