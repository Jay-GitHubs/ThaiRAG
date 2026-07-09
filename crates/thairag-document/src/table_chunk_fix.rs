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
    // not. Debris = the chunk carries a code fence (which never belongs in
    // document content) and next to nothing else survives removing it.
    let has_fence = content.lines().any(|l| l.trim_start().starts_with("```"));
    if !has_fence {
        return false;
    }
    let alnum = content
        .lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .flat_map(|l| l.chars())
        .filter(|c| c.is_alphanumeric())
        .count();
    alnum < MIN_CONTENT_ALNUM
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

    for (i, c) in chunks.iter_mut().enumerate() {
        c.chunk_index = i;
    }

    if dropped > 0 || repaired > 0 {
        tracing::info!(
            dropped_junk = dropped,
            headers_repaired = repaired,
            "Table chunk hygiene applied"
        );
    }
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
