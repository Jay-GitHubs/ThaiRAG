//! Table linearization for reasoning retrieval (extraction-gap prototype).
//!
//! Thai government PDFs convert to HTML `<table>` blocks with merged cells
//! (`colspan`/`rowspan`) and scattered empty `<td>`s, so the 2D grid that binds a
//! value to its row + column headers is hard for the answer model to read — the
//! measured table ceiling (~50% for both vector and reasoning) is dominated by
//! this, not by the retrieval method or model size.
//!
//! This pass expands the merges into a **rectangular grid** and re-emits it as a
//! clean aligned markdown table (every row the same width, merged cells filled
//! in), so cross-row/column alignment is explicit. It is a deterministic
//! structural rewrite — it cannot recover information the OCR lost, but it stops
//! the *presentation* from hiding information the HTML did capture.

use std::sync::LazyLock;

use regex::Regex;

static TABLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<table[^>]*>.*?</table>").unwrap());
static ROW_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<tr[^>]*>(.*?)</tr>").unwrap());
static CELL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<t[dh]([^>]*)>(.*?)</t[dh]>").unwrap());
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]+>").unwrap());
static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// Replace every `<table>` block in `text` with a linearized, rectangular
/// markdown table. Non-table text is returned unchanged.
pub fn linearize_tables(text: &str) -> String {
    TABLE_RE
        .replace_all(text, |c: &regex::Captures| linearize_one(&c[0]))
        .into_owned()
}

fn span(attrs: &str, name: &str) -> usize {
    // e.g. colspan="5" or colspan=5
    let needle = format!("{name}=");
    attrs
        .find(&needle)
        .map(|i| &attrs[i + needle.len()..])
        .and_then(|s| {
            s.trim_start_matches('"')
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .unwrap_or(1)
        .max(1)
}

fn clean_cell(raw: &str) -> String {
    WS_RE
        .replace_all(&TAG_RE.replace_all(raw, " "), " ")
        .trim()
        .to_string()
}

/// Expand a single HTML table into a rectangular grid (merged cells filled).
fn parse_grid(table_html: &str) -> Vec<Vec<String>> {
    let mut grid: Vec<Vec<String>> = Vec::new();
    // Active rowspans: column -> (rows still to fill, text).
    let mut carry: std::collections::BTreeMap<usize, (usize, String)> =
        std::collections::BTreeMap::new();

    for rowcap in ROW_RE.captures_iter(table_html) {
        let mut row: Vec<String> = Vec::new();
        let mut c = 0usize;
        let put = |row: &mut Vec<String>, col: usize, text: &str| {
            while row.len() <= col {
                row.push(String::new());
            }
            row[col] = text.to_string();
        };

        for cell in CELL_RE.captures_iter(&rowcap[1]) {
            // First, lay down any carried rowspan columns at the cursor.
            while let Some((rem, text)) = carry.get(&c).cloned() {
                put(&mut row, c, &text);
                if rem <= 1 {
                    carry.remove(&c);
                } else {
                    carry.insert(c, (rem - 1, text));
                }
                c += 1;
            }
            let text = clean_cell(&cell[2]);
            let cs = span(&cell[1], "colspan");
            let rs = span(&cell[1], "rowspan");
            for _ in 0..cs {
                put(&mut row, c, &text);
                if rs > 1 {
                    carry.insert(c, (rs - 1, text.clone()));
                }
                c += 1;
            }
        }
        // Trailing carried rowspans past the last source cell.
        let trailing: Vec<usize> = carry.range(c..).map(|(k, _)| *k).collect();
        for col in trailing {
            if let Some((rem, text)) = carry.get(&col).cloned() {
                put(&mut row, col, &text);
                if rem <= 1 {
                    carry.remove(&col);
                } else {
                    carry.insert(col, (rem - 1, text));
                }
            }
        }
        grid.push(row);
    }

    let width = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    for r in &mut grid {
        while r.len() < width {
            r.push(String::new());
        }
    }
    grid
}

fn linearize_one(table_html: &str) -> String {
    let grid = parse_grid(table_html);
    if grid.is_empty() {
        return table_html.to_string();
    }
    let mut out = String::from("\n[table]\n");
    for row in &grid {
        out.push_str("| ");
        out.push_str(&row.join(" | "));
        out.push_str(" |\n");
    }
    out.push_str("[/table]\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_colspan_into_rectangular_grid() {
        // Row 0 spans 4 cols under one header; row 1 has 4 individual cells.
        let html = r#"<table><tr><td colspan="4">Header</td></tr><tr><td>a</td><td>b</td><td>c</td><td>d</td></tr></table>"#;
        let out = linearize_tables(html);
        // Both rows are now width-4 and aligned.
        assert!(out.contains("| Header | Header | Header | Header |"));
        assert!(out.contains("| a | b | c | d |"));
        assert!(!out.contains("<table>"));
    }

    #[test]
    fn fills_rowspan_down() {
        let html = r#"<table><tr><td rowspan="2">R</td><td>x</td></tr><tr><td>y</td></tr></table>"#;
        let out = linearize_tables(html);
        // The rowspan label R is duplicated into the second row's first column.
        assert!(out.contains("| R | x |"));
        assert!(out.contains("| R | y |"));
    }

    #[test]
    fn strips_inner_tags_and_whitespace() {
        let html = "<table><tr><td><b>3.0</b></td><td>  10.0\n</td></tr></table>";
        let out = linearize_tables(html);
        assert!(out.contains("| 3.0 | 10.0 |"));
    }

    #[test]
    fn non_table_text_unchanged() {
        let t = "## Page 1\njust prose, no table.";
        assert_eq!(linearize_tables(t), t);
    }
}
