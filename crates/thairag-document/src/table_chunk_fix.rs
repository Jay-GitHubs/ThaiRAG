//! Post-chunking table hygiene.
//!
//! Two defects observed live on a vision-rescued table corpus (rd_tp4, 30
//! chunks) that no chunker strategy prevents by itself:
//!
//! 1. **Junk chunks** — stray markdown code fences survive as whole chunks
//!    ("```", "```markdown"), diluting the document's retrieval budget.
//! 2. **Headerless table chunks** — a long markdown table split across chunks
//!    leaves continuation chunks with rows but no header, so a row-level query
//!    ("อัตราภาษีของลำดับที่ 10") can't rank them: the cell values carry no
//!    column vocabulary.
//!
//! `fix_table_chunks` runs after any chunker (AI or mechanical): it drops
//! content-free chunks and prepends the nearest preceding table header (header
//! row + separator) to continuation chunks, then re-numbers `chunk_index` so
//! downstream order-based joins stay consistent.

use thairag_core::types::DocumentChunk;

/// Minimum alphanumeric characters (Thai included) for a chunk to be worth
/// storing. Below this a chunk is formatting debris, not content.
const MIN_CONTENT_ALNUM: usize = 20;

fn is_md_separator(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn is_table_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// First (header row, separator row) pair in the content, if any.
fn header_of(content: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    for w in lines.windows(2) {
        if is_table_row(w[0]) && !is_md_separator(w[0]) && is_md_separator(w[1]) {
            return Some((w[0].to_string(), w[1].to_string()));
        }
    }
    None
}

fn is_junk(content: &str) -> bool {
    // Only formatting DEBRIS is junk — a short but real document ("Hello") is
    // not. Two debris shapes, both observed live: (a) a code fence with next
    // to nothing else, (b) pure markdown structure — a lone horizontal rule or
    // a bare "## Page N" heading the chunker split off on its own (zero
    // alphanumeric content once structure lines are removed).
    fn is_structure(line: &str) -> bool {
        let t = line.trim();
        t.starts_with("```")
            || t.starts_with('#')
            || (!t.is_empty() && t.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')))
    }
    let has_fence = content.lines().any(|l| l.trim_start().starts_with("```"));
    let non_structure_alnum = content
        .lines()
        .filter(|l| !is_structure(l))
        .flat_map(|l| l.chars())
        .filter(|c| c.is_alphanumeric())
        .count();
    if non_structure_alnum == 0 {
        return true;
    }
    has_fence && non_structure_alnum < MIN_CONTENT_ALNUM
}

/// See module docs. Order-preserving; safe on non-table documents (no-op).
pub fn fix_table_chunks(chunks: &mut Vec<DocumentChunk>) {
    let before = chunks.len();
    chunks.sort_by_key(|c| c.chunk_index);
    // Never empty a document: the zero-chunk guard has already passed by the
    // time this runs, so dropping the last chunk would store an unsearchable
    // doc without the loud failure that guard exists to give.
    if chunks.iter().any(|c| !is_junk(&c.content)) {
        chunks.retain(|c| !is_junk(&c.content));
    }
    let dropped = before - chunks.len();

    let mut last_header: Option<(String, String)> = None;
    let mut repaired = 0usize;
    for chunk in chunks.iter_mut() {
        // Atomic HTML table chunks manage their own structure — skip.
        if chunk.content.trim_start().starts_with("<table") {
            continue;
        }
        if let Some(h) = header_of(&chunk.content) {
            last_header = Some(h);
            continue;
        }
        let rows = chunk.content.lines().filter(|l| is_table_row(l)).count();
        if rows >= 2
            && let Some((header, sep)) = &last_header
            && !chunk.content.contains(header.trim())
        {
            chunk.content = format!("{header}\n{sep}\n{}", chunk.content);
            repaired += 1;
        }
    }

    // Row-index line: sibling table chunks are near-identical to a dense
    // embedder except their row keys ("ลำดับที่ 8..12"), so a row-level query
    // cannot rank the right window. Surfacing the first-column values as an
    // explicit line gives BM25 and the embedder the differentiator vocabulary.
    let mut indexed = 0usize;
    for chunk in chunks.iter_mut() {
        if let Some(line) = row_index_line(&chunk.content) {
            chunk.content = format!("{line}\n{}", chunk.content);
            indexed += 1;
        }
    }

    for (i, c) in chunks.iter_mut().enumerate() {
        c.chunk_index = i;
    }

    if dropped > 0 || repaired > 0 || indexed > 0 {
        tracing::info!(
            dropped_junk = dropped,
            headers_repaired = repaired,
            row_indexed = indexed,
            "Table chunk hygiene applied"
        );
    }
}

/// Row-label spellings that identify the entry-number line of a TRANSPOSED
/// grid (attributes as rows, entries as columns — the shape the lattice
/// reconstructor emits for the Revenue-Dept tables). Matched against the whole
/// first cell, lowercased. Both ำ (composed) and ํา (decomposed sara-am)
/// spellings occur in extracted text.
const ID_ROW_LABELS: &[&str] = &["ลำดับที่", "ลําดับที่", "ลำดับ", "ลําดับ", "no.", "item", "ข้อที่"];

/// Build the row-index line for a table chunk, e.g. `ลำดับที่: 8., 9., 10.`.
/// Two sources, in preference order:
/// 1. An identifier-labeled row (`| ลำดับที่ | 8. | 9. | … |`) — transposed
///    grids, where sibling chunks differ ONLY in these cell values.
/// 2. Column-0 values of a row-major table.
///
/// `None` when neither yields ≥3 distinct short values.
fn row_index_line(content: &str) -> Option<String> {
    const MAX_VALUE_CHARS: usize = 24;
    const MAX_VALUES: usize = 40;

    fn cells(line: &str) -> Vec<&str> {
        line.trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect()
    }
    // Strictly column 0 — drifting to "first non-empty cell" would index
    // different columns on different rows of the same table.
    fn first_cell(line: &str) -> Option<&str> {
        let cell = line
            .trim()
            .trim_start_matches('|')
            .split('|')
            .next()?
            .trim();
        (!cell.is_empty()).then_some(cell)
    }
    fn accept(values: Vec<&str>) -> Option<Vec<&str>> {
        let distinct: std::collections::HashSet<&&str> = values.iter().collect();
        (distinct.len() >= 3).then_some(values)
    }

    let lines: Vec<&str> = content.lines().collect();

    // Source 1: transposed-grid identifier row.
    for line in &lines {
        if !is_table_row(line) || is_md_separator(line) {
            continue;
        }
        let cs = cells(line);
        let Some((label, rest)) = cs.split_first() else {
            continue;
        };
        if ID_ROW_LABELS.contains(&label.to_lowercase().as_str()) {
            let vals: Vec<&str> = rest
                .iter()
                .copied()
                .filter(|v| !v.is_empty() && v.chars().count() <= MAX_VALUE_CHARS)
                .collect();
            if let Some(mut vals) = accept(vals) {
                vals.truncate(MAX_VALUES);
                return Some(format!("{label}: {}", vals.join(", ")));
            }
        }
    }

    // Source 2: row-major column-0 values.
    let mut header_name: Option<&str> = None;
    let mut values: Vec<&str> = Vec::new();
    let mut data_rows = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if !is_table_row(line) || is_md_separator(line) {
            continue;
        }
        // The header row is the one directly above a separator.
        if lines.get(i + 1).is_some_and(|n| is_md_separator(n)) {
            header_name = header_name.or_else(|| first_cell(line));
            continue;
        }
        data_rows += 1;
        if let Some(v) = first_cell(line)
            && v.chars().count() <= MAX_VALUE_CHARS
            && values.last() != Some(&v)
        {
            values.push(v);
        }
    }
    // Row-shaped enough: ≥3 rows, most rows keyed, ≥3 distinct keys.
    if data_rows < 3 || values.len() * 2 < data_rows {
        return None;
    }
    let mut values = accept(values)?;
    values.truncate(MAX_VALUES);
    Some(format!(
        "{}: {}",
        header_name.unwrap_or("แถว"),
        values.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{ChunkId, DocId, WorkspaceId};

    fn chunk(idx: usize, content: &str) -> DocumentChunk {
        DocumentChunk {
            chunk_id: ChunkId::new(),
            doc_id: DocId::new(),
            workspace_id: WorkspaceId::new(),
            content: content.to_string(),
            chunk_index: idx,
            embedding: None,
            metadata: None,
        }
    }

    const HEADER: &str = "| ลำดับที่ | ประเภทเงินได้ | อัตราภาษีร้อยละ |";
    const SEP: &str = "|---|---|---|";

    #[test]
    fn fence_only_chunks_are_dropped_and_indices_renumbered() {
        let mut cs = vec![
            chunk(0, "```markdown"),
            chunk(1, &format!("{HEADER}\n{SEP}\n| 1 | เงินเดือน | 5.0 |")),
            chunk(2, "```"),
            chunk(3, "เนื้อหาปกติของเอกสารยาวพอสมควรสำหรับการทดสอบนี้"),
        ];
        fix_table_chunks(&mut cs);
        assert_eq!(cs.len(), 2);
        assert_eq!(
            cs.iter().map(|c| c.chunk_index).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn continuation_rows_get_the_preceding_header() {
        let mut cs = vec![
            chunk(0, &format!("{HEADER}\n{SEP}\n| 1 | เงินเดือน | 5.0 |")),
            chunk(1, "| 9 | ดอกเบี้ย | 1.0 |\n| 10 | เงินปันผล | 3.0 |"),
        ];
        fix_table_chunks(&mut cs);
        assert!(cs[1].content.starts_with(HEADER), "{}", cs[1].content);
        assert!(cs[1].content.contains("| 10 |"));
    }

    #[test]
    fn chunks_with_their_own_header_are_left_alone() {
        let with_header = format!("{HEADER}\n{SEP}\n| 7 | ค่าเช่า | 5.0 |");
        let mut cs = vec![chunk(0, &with_header), chunk(1, &with_header)];
        fix_table_chunks(&mut cs);
        assert_eq!(cs[1].content, with_header);
    }

    #[test]
    fn prose_and_html_tables_are_untouched() {
        let prose = "ย่อหน้าปกติที่ยาวเพียงพอ ไม่มีตารางอยู่ภายในเนื้อหานี้เลย";
        let html = "<table><tr><td>x</td></tr></table> เนื้อหาประกอบตารางเอชทีเอ็มแอล";
        let mut cs = vec![chunk(0, prose), chunk(1, html)];
        fix_table_chunks(&mut cs);
        assert_eq!(cs[0].content, prose);
        assert_eq!(cs[1].content, html);
    }

    #[test]
    fn structure_only_chunks_are_dropped_but_tiny_prose_survives() {
        let mut cs = vec![
            chunk(0, "---"),
            chunk(1, "## Page 2"),
            chunk(2, "Hello async"),
            chunk(3, "## หัวข้อ\nเนื้อหาจริงที่ตามหลังหัวข้อ"),
        ];
        fix_table_chunks(&mut cs);
        let texts: Vec<&str> = cs.iter().map(|c| c.content.as_str()).collect();
        assert_eq!(texts, vec!["Hello async", "## หัวข้อ\nเนื้อหาจริงที่ตามหลังหัวข้อ"]);
    }

    #[test]
    fn transposed_grid_indexes_the_identifier_row() {
        // Attributes as rows, entries as columns — the lattice shape for the
        // Revenue-Dept tables. The differentiator is the ลำดับที่ row's cells.
        let grid = "| กำหนดเวลานำส่ง | 7 วัน | 7 วัน | 7 วัน |\n|---|---|---|---|\n| อัตราภาษีร้อยละ | 3.0 | 5.0 | 2.0 |\n| ลำดับที่ | 8. | 9. | 10. |";
        let mut cs = vec![chunk(0, grid)];
        fix_table_chunks(&mut cs);
        assert!(
            cs[0].content.starts_with("ลำดับที่: 8., 9., 10."),
            "{}",
            cs[0].content
        );
    }

    #[test]
    fn row_index_line_names_first_column_values() {
        let grid = format!(
            "{HEADER}\n{SEP}\n| 8 | ก | 1.0 |\n| 9 | ข | 2.0 |\n| 10 | ค | 3.0 |\n| 11 | ง | 2.0 |"
        );
        let mut cs = vec![chunk(0, &grid)];
        fix_table_chunks(&mut cs);
        assert!(
            cs[0].content.starts_with("ลำดับที่: 8, 9, 10, 11"),
            "{}",
            cs[0].content
        );
    }

    #[test]
    fn row_index_skips_empty_or_repetitive_first_columns() {
        // Empty first cells (real shape from rd_withholding): no index line.
        let empty_col = "|  | แบบ/กำหนด | เวลานำส่ง |\n|---|---|---|\n|  | ก 12345678 | x |\n|  | ข 23456789 | y |\n|  | ค 34567890 | z |";
        // A single repeated key offers no differentiator vocabulary either.
        let repetitive =
            format!("{HEADER}\n{SEP}\n| ก | 1 | 1.0 |\n| ก | 2 | 2.0 |\n| ก | 3 | 3.0 |");
        let mut cs = vec![chunk(0, empty_col), chunk(1, &repetitive)];
        fix_table_chunks(&mut cs);
        assert!(!cs[0].content.starts_with("แบบ"), "{}", cs[0].content);
        assert!(cs[1].content.starts_with(HEADER), "{}", cs[1].content);
    }

    #[test]
    fn single_stray_pipe_line_is_not_treated_as_a_table() {
        let content = "ข้อความที่มีอักขระไปป์อยู่หนึ่งบรรทัด\n| แค่บรรทัดเดียว |\nต่อด้วยข้อความปกติ";
        let mut cs = vec![
            chunk(0, &format!("{HEADER}\n{SEP}\n| 1 | ก | 1.0 |")),
            chunk(1, content),
        ];
        fix_table_chunks(&mut cs);
        assert_eq!(cs[1].content, content);
    }
}
