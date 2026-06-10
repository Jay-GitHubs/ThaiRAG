//! Borderless ("stream") table reconstruction from PDF text geometry.
//!
//! Bordered tables give us ruling lines (see [`crate::table_lattice`]).
//! Borderless tables have none — the only structure signal is *whitespace*: the
//! vertical gutters that stay empty across many text lines mark the columns. We
//! cluster characters into rows, find the consistent gutters, then drop each
//! glyph into its row/column by position (Thai combining marks inherit their
//! base glyph's cell, as in the lattice path). Cell content always comes from
//! the text layer, so numbers stay exact — only the column/row STRUCTURE is
//! inferred, and scored with a confidence value.
//!
//! Conservative by design: if there isn't a clear, consistent multi-column
//! structure across enough rows, we return `None` (the page is treated as
//! prose). Column-spanning cells (merged headers) ARE detected — a word that
//! floats in the gutter between two columns, or overlaps several columns'
//! core spans, can only be a cell spanning them — but row spans are not
//! (unreliable without borders).

use crate::pdfium_engine::PositionedChar;
use crate::table_lattice::ReconstructedTable;

/// Min rows for a region to count as a table.
const MIN_ROWS: usize = 3;
/// Gutter width as a multiple of median glyph width to separate columns.
const COL_GAP_MULT: f32 = 2.5;
/// Intra-cell merge gap (multiple of median glyph width) when unioning glyph
/// spans — gaps smaller than this don't open a gutter.
const MERGE_GAP_MULT: f32 = 1.2;
/// Minimum fraction of region rows that must span >=2 columns.
const MIN_COL_CONSISTENCY: f32 = 0.6;

fn is_thai_combining_mark(c: char) -> bool {
    matches!(c,
        '\u{0E31}'
        | '\u{0E34}'..='\u{0E3A}'
        | '\u{0E47}'..='\u{0E4E}'
    )
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

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// One clustered text line: its base-glyph vertical center and the x-spans of
/// its words (used for region + gutter detection).
struct Row {
    cy: f32,
    words: Vec<(f32, f32)>, // (x0, x1) per word, left→right
}

/// Column x-ranges from a bag of word spans: union spans whose gap ≤
/// `word_gap` (intra-cell), then split the blocks at gaps ≥ `col_gap`
/// (gutters). Returns blocks sorted left→right.
fn column_blocks(mut spans: Vec<(f32, f32)>, word_gap: f32, col_gap: f32) -> Vec<(f32, f32)> {
    if spans.is_empty() {
        return Vec::new();
    }
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for s in spans {
        if let Some(last) = merged.last_mut()
            && s.0 - last.1 <= word_gap
        {
            last.1 = last.1.max(s.1);
            continue;
        }
        merged.push(s);
    }
    let mut cols: Vec<(f32, f32)> = Vec::new();
    let mut block = merged[0];
    for m in &merged[1..] {
        if m.0 - block.1 >= col_gap {
            cols.push(block);
            block = *m;
        } else {
            block.1 = block.1.max(m.1);
        }
    }
    cols.push(block);
    cols
}

/// Reconstruct a borderless table from page geometry, or `None` if the glyphs
/// don't form a clear multi-column grid (treat as prose).
pub fn reconstruct(chars: &[PositionedChar]) -> Option<ReconstructedTable> {
    if chars.len() < 6 {
        return None;
    }
    let med_w = median(chars.iter().map(|c| (c.x1 - c.x0).abs()).collect()).max(1.0);
    let med_h = median(chars.iter().map(|c| (c.y1 - c.y0).abs()).collect()).max(1.0);
    let word_gap = med_w * MERGE_GAP_MULT;
    let col_gap = med_w * COL_GAP_MULT;
    let row_tol = med_h * 0.6;

    // ── Cluster base glyphs into rows by vertical center (marks ignored for
    // clustering; they ride with their base later). ──────────────────────────
    let mut bases: Vec<&PositionedChar> = chars
        .iter()
        .filter(|c| !is_thai_combining_mark(c.ch) && !c.ch.is_whitespace())
        .collect();
    bases.sort_by(|a, b| {
        let (ay, by) = ((a.y0 + a.y1) / 2.0, (b.y0 + b.y1) / 2.0);
        by.partial_cmp(&ay).unwrap_or(std::cmp::Ordering::Equal)
    });
    if bases.is_empty() {
        return None;
    }

    let mut row_groups: Vec<Vec<&PositionedChar>> = Vec::new();
    let mut cur: Vec<&PositionedChar> = vec![bases[0]];
    let mut cur_cy = (bases[0].y0 + bases[0].y1) / 2.0;
    for &c in &bases[1..] {
        let cy = (c.y0 + c.y1) / 2.0;
        if (cur_cy - cy).abs() <= row_tol {
            cur.push(c);
        } else {
            row_groups.push(std::mem::take(&mut cur));
            cur = vec![c];
        }
        cur_cy = cy;
    }
    row_groups.push(cur);

    // Build Row records (words = glyph runs split on gaps > word_gap).
    let rows: Vec<Row> = row_groups
        .iter()
        .map(|g| {
            let mut gs: Vec<&PositionedChar> = g.clone();
            gs.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
            let cy = gs.iter().map(|c| (c.y0 + c.y1) / 2.0).sum::<f32>() / gs.len() as f32;
            let mut words: Vec<(f32, f32)> = Vec::new();
            let (mut wx0, mut wx1) = (gs[0].x0, gs[0].x1);
            for c in &gs[1..] {
                if c.x0 - wx1 > word_gap {
                    words.push((wx0, wx1));
                    wx0 = c.x0;
                }
                wx1 = wx1.max(c.x1);
            }
            words.push((wx0, wx1));
            Row { cy, words }
        })
        .collect();

    // ── Candidate region: maximal run of consecutive rows that look
    // multi-column (>=2 words with a column-sized gap between them). ──────────
    let multi_col = |r: &Row| -> bool { r.words.windows(2).any(|w| w[1].0 - w[0].1 >= col_gap) };
    let (mut best_start, mut best_len) = (0usize, 0usize);
    let (mut i, n) = (0usize, rows.len());
    while i < n {
        if multi_col(&rows[i]) {
            let start = i;
            while i < n && multi_col(&rows[i]) {
                i += 1;
            }
            if i - start > best_len {
                best_len = i - start;
                best_start = start;
            }
        } else {
            i += 1;
        }
    }
    if best_len < MIN_ROWS {
        return None;
    }
    let region = &rows[best_start..best_start + best_len];

    let n_rows = region.len();

    // ── Column-spanning cells (merged headers). A word aligned with a column
    // overlaps that column's span as computed from the OTHER rows; a merged
    // header centered over k columns instead floats in their gutter (short
    // text) or overlaps several of them (long text). Leave-one-row-out keeps
    // the row's own words from polluting the columns it is judged against.
    // Judging against ≥2-column structures only; if the other rows alone do
    // not form columns, nothing can be inferred about this row's words.
    let crosses = |w0: f32, w1: f32, blocks: &[(f32, f32)]| -> bool {
        let first = blocks.iter().position(|&(a, b)| w0 < b && w1 > a);
        let last = blocks.iter().rposition(|&(a, b)| w0 < b && w1 > a);
        match (first, last) {
            // Overlaps several columns' core spans.
            (Some(f), Some(l)) => l > f,
            // Overlaps none: wholly inside a gutter between two columns — it
            // can only be a cell spanning both.
            (None, None) => {
                (0..blocks.len() - 1).any(|g| w0 >= blocks[g].1 && w1 <= blocks[g + 1].0)
            }
            _ => false,
        }
    };
    let mut spanning: Vec<Vec<usize>> = (0..n_rows).map(|_| Vec::new()).collect();
    for ri in 0..n_rows {
        let loo = column_blocks(
            region
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != ri)
                .flat_map(|(_, r)| r.words.iter().copied())
                .collect(),
            word_gap,
            col_gap,
        );
        if loo.len() < 2 {
            continue;
        }
        for (wi, &(w0, w1)) in region[ri].words.iter().enumerate() {
            if crosses(w0, w1, &loo) {
                spanning[ri].push(wi);
            }
        }
    }

    // ── Column boundaries from the NON-spanning words: a floating merged
    // header must not widen a column (or mint a phantom one). Gaps >= col_gap
    // between the remaining word blocks are the gutters. ─────────────────────
    let spanning_ref = &spanning;
    let cols = column_blocks(
        region
            .iter()
            .enumerate()
            .flat_map(|(ri, r)| {
                r.words
                    .iter()
                    .enumerate()
                    .filter(move |(wi, _)| !spanning_ref[ri].contains(wi))
                    .map(|(_, w)| *w)
            })
            .collect(),
        word_gap,
        col_gap,
    );
    let n_cols = cols.len();
    if n_cols < 2 {
        return None;
    }

    // Map each spanning word onto the final columns it covers.
    struct ColSpan {
        x0: f32,
        x1: f32,
        first: usize,
        last: usize,
    }
    let mut row_spans: Vec<Vec<ColSpan>> = (0..n_rows).map(|_| Vec::new()).collect();
    for (ri, word_idxs) in spanning.iter().enumerate() {
        for &wi in word_idxs {
            let (w0, w1) = region[ri].words[wi];
            let first = cols.iter().position(|&(a, b)| w0 < b && w1 > a);
            let last = cols.iter().rposition(|&(a, b)| w0 < b && w1 > a);
            let span = match (first, last) {
                (Some(f), Some(l)) if l > f => Some((f, l)),
                (None, None) => (0..n_cols - 1)
                    .find(|&g| w0 >= cols[g].1 && w1 <= cols[g + 1].0)
                    .map(|g| (g, g + 1)),
                _ => None, // collapsed to a single column — not a span here
            };
            if let Some((f, l)) = span {
                row_spans[ri].push(ColSpan {
                    x0: w0,
                    x1: w1,
                    first: f,
                    last: l,
                });
            }
        }
    }

    // Row y-bands (sorted top→bottom) from region row centers.
    let row_cy: Vec<f32> = region.iter().map(|r| r.cy).collect();
    let col_of = |cx: f32| -> Option<usize> {
        cols.iter()
            .position(|(a, b)| cx >= a - col_gap / 2.0 && cx <= b + col_gap / 2.0)
    };
    let row_of = |cy: f32| -> Option<usize> {
        let mut best = None;
        let mut bestd = f32::MAX;
        for (i, &ry) in row_cy.iter().enumerate() {
            let d = (ry - cy).abs();
            if d < bestd {
                bestd = d;
                best = Some(i);
            }
        }
        // Only accept if reasonably close to a row band.
        if bestd <= row_tol * 2.0 { best } else { None }
    };

    // Assign every glyph (whole page) to a cell; combining marks inherit the
    // previous glyph's cell so Thai stays in logical order. A glyph inside a
    // column-spanning word goes to the span's anchor (first) column.
    let mut cells: Vec<Vec<char>> = vec![Vec::new(); n_rows * n_cols];
    let mut assigned = 0usize;
    let mut last: Option<usize> = None;
    for c in chars {
        if c.ch == '\n' || c.ch == '\r' || c.ch == '\t' {
            continue;
        }
        // A space (or Thai combining mark) joins the current cell rather than
        // being positioned on its own — this preserves intra-cell word spacing
        // ("Q1 Sales") and keeps Thai marks with their base glyph.
        let idx = if c.ch == ' ' || is_thai_combining_mark(c.ch) {
            last
        } else {
            let cx = (c.x0 + c.x1) / 2.0;
            let cy = (c.y0 + c.y1) / 2.0;
            row_of(cy).and_then(|r| {
                let spanned = row_spans[r]
                    .iter()
                    .find(|s| cx >= s.x0 && cx <= s.x1)
                    .map(|s| s.first);
                spanned.or_else(|| col_of(cx)).map(|cc| r * n_cols + cc)
            })
        };
        if let Some(i) = idx {
            cells[i].push(c.ch);
            last = Some(i);
            if c.ch != ' ' {
                assigned += 1;
            }
        }
    }

    let cell_text = |r: usize, c: usize| {
        cells[r * n_cols + c]
            .iter()
            .collect::<String>()
            .trim()
            .to_string()
    };

    // Confidence: fraction of rows spanning >=2 non-empty columns.
    let multi_rows = (0..n_rows)
        .filter(|&r| (0..n_cols).filter(|&c| !cell_text(r, c).is_empty()).count() >= 2)
        .count();
    let col_consistency = multi_rows as f32 / n_rows as f32;
    if col_consistency < MIN_COL_CONSISTENCY {
        return None;
    }
    let total_nonspace = chars
        .iter()
        .filter(|c| !c.ch.is_whitespace())
        .count()
        .max(1);
    let char_coverage = assigned as f32 / total_nonspace as f32;
    let filled = (0..n_rows * n_cols)
        .filter(|&i| !cells[i].is_empty())
        .count();
    let confidence = (col_consistency + filled as f32 / (n_rows * n_cols) as f32) / 2.0;

    // Per row: the anchor column of every column (itself when unmerged).
    let mut anchor: Vec<Vec<usize>> = (0..n_rows).map(|_| (0..n_cols).collect()).collect();
    for (r, spans) in row_spans.iter().enumerate() {
        for s in spans {
            for a in &mut anchor[r][s.first..=s.last] {
                *a = (*a).min(s.first);
            }
        }
    }

    // HTML (colspan on anchors) + linearized (merged value filled across every
    // column it covers, so each row is self-contained for retrieval).
    let mut grid: Vec<Vec<String>> = Vec::with_capacity(n_rows);
    let mut html = String::from("<table>");
    for r in 0..n_rows {
        // Fold any stray covered-cell text into its anchor, in reading order.
        let mut texts: Vec<String> = (0..n_cols).map(|c| cell_text(r, c)).collect();
        for c in 0..n_cols {
            let a = anchor[r][c];
            if a != c {
                let t = std::mem::take(&mut texts[c]);
                if !t.is_empty() {
                    if !texts[a].is_empty() {
                        texts[a].push(' ');
                    }
                    texts[a].push_str(&t);
                }
            }
        }
        html.push_str("<tr>");
        let mut c = 0usize;
        while c < n_cols {
            let span = (c..n_cols)
                .take_while(|&cc| anchor[r][cc] == c)
                .count()
                .max(1);
            if span > 1 {
                html.push_str(&format!("<td colspan=\"{span}\">"));
            } else {
                html.push_str("<td>");
            }
            html.push_str(&escape_html(&texts[c]));
            html.push_str("</td>");
            c += span;
        }
        html.push_str("</tr>");
        grid.push((0..n_cols).map(|c| texts[anchor[r][c]].clone()).collect());
    }
    html.push_str("</table>");
    let linearized = grid
        .iter()
        .map(|row| row.join(" | "))
        .collect::<Vec<_>>()
        .join("\n");

    Some(ReconstructedTable {
        html,
        linearized,
        confidence,
        char_coverage,
        n_rows,
        n_cols,
        col_xs: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: char, x: f32, y: f32) -> PositionedChar {
        PositionedChar {
            ch: c,
            x0: x,
            y0: y,
            x1: x + 6.0,
            y1: y + 8.0,
        }
    }
    // Place a left-aligned word at (x,y), chars 6 wide adjacent.
    fn word(s: &str, x: f32, y: f32) -> Vec<PositionedChar> {
        s.chars()
            .enumerate()
            .map(|(i, c)| ch(c, x + i as f32 * 6.0, y))
            .collect()
    }

    #[test]
    fn reconstructs_borderless_3col() {
        // 3 columns at x=10/120/240, 4 rows at y=100/80/60/40. Wide gutters.
        let mut chars = Vec::new();
        let rows = [
            ("No", "Region", "100"),
            ("1", "North", "200"),
            ("2", "South", "300"),
            ("3", "East", "400"),
        ];
        for (i, (a, b, c)) in rows.iter().enumerate() {
            let y = 100.0 - i as f32 * 20.0;
            chars.extend(word(a, 10.0, y));
            chars.extend(word(b, 120.0, y));
            chars.extend(word(c, 240.0, y));
        }
        let t = reconstruct(&chars).expect("table");
        assert_eq!((t.n_rows, t.n_cols), (4, 3), "{}", t.html);
        assert!(t.html.contains("<td>North</td>"), "{}", t.html);
        assert!(t.html.contains("<td>300</td>"), "{}", t.html);
        assert_eq!(t.linearized.lines().next().unwrap(), "No | Region | 100");
        assert!(t.confidence > 0.8);
    }

    #[test]
    fn gutter_floating_header_spans_columns() {
        // Header "Sales" centered over the two value columns: it sits in the
        // gutter between them (overlapping neither column's data core), which
        // can only be a cell spanning both. Data rows align with their cores.
        let mut chars = Vec::new();
        chars.extend(word("Region", 10.0, 100.0));
        chars.extend(word("Sales", 160.0, 100.0)); // 160..190, between cols
        let rows = [
            ("North", "100", "200"),
            ("South", "300", "400"),
            ("East", "500", "600"),
        ];
        for (i, (a, b, c)) in rows.iter().enumerate() {
            let y = 80.0 - i as f32 * 20.0;
            chars.extend(word(a, 10.0, y));
            chars.extend(word(b, 120.0, y)); // col2 core 120..138
            chars.extend(word(c, 240.0, y)); // col3 core 240..258
        }
        let t = reconstruct(&chars).expect("table");
        assert_eq!((t.n_rows, t.n_cols), (4, 3), "{}", t.html);
        assert!(
            t.html.contains("<td colspan=\"2\">Sales</td>"),
            "expected spanning header, html: {}",
            t.html
        );
        assert_eq!(
            t.linearized.lines().next().unwrap(),
            "Region | Sales | Sales",
            "merged value must fill the columns it covers"
        );
        // Data rows untouched.
        assert!(t.linearized.contains("North | 100 | 200"));
    }

    #[test]
    fn wide_header_overlapping_both_cores_spans_columns() {
        // A long header overlapping BOTH value columns' cores also spans them.
        let mut chars = Vec::new();
        chars.extend(word("Region", 10.0, 100.0));
        chars.extend(word("QuarterlySalesTotals", 130.0, 100.0)); // 130..250
        let rows = [
            ("North", "100", "200"),
            ("South", "300", "400"),
            ("East", "500", "600"),
        ];
        for (i, (a, b, c)) in rows.iter().enumerate() {
            let y = 80.0 - i as f32 * 20.0;
            chars.extend(word(a, 10.0, y));
            chars.extend(word(b, 120.0, y));
            chars.extend(word(c, 240.0, y));
        }
        let t = reconstruct(&chars).expect("table");
        assert_eq!((t.n_rows, t.n_cols), (4, 3), "{}", t.html);
        assert!(t.html.contains("colspan=\"2\""), "html: {}", t.html);
    }

    #[test]
    fn column_aligned_headers_do_not_merge() {
        // Headers that sit on their own columns must stay separate cells.
        let mut chars = Vec::new();
        let rows = [
            ("Region", "Q1", "Q2"),
            ("North", "100", "200"),
            ("South", "300", "400"),
            ("East", "500", "600"),
        ];
        for (i, (a, b, c)) in rows.iter().enumerate() {
            let y = 100.0 - i as f32 * 20.0;
            chars.extend(word(a, 10.0, y));
            chars.extend(word(b, 120.0, y));
            chars.extend(word(c, 240.0, y));
        }
        let t = reconstruct(&chars).expect("table");
        assert!(!t.html.contains("colspan"), "html: {}", t.html);
        assert_eq!(t.linearized.lines().next().unwrap(), "Region | Q1 | Q2");
    }

    #[test]
    fn prose_is_not_a_table() {
        // Continuous prose lines: words separated by normal spaces (small gaps),
        // no consistent wide gutters → not a table.
        let mut chars = Vec::new();
        for (i, line) in [
            "this is a normal paragraph of text",
            "with several words per line here",
            "and a third sentence of prose too",
        ]
        .iter()
        .enumerate()
        {
            let y = 100.0 - i as f32 * 20.0;
            let mut x = 10.0;
            for w in line.split(' ') {
                chars.extend(word(w, x, y));
                x += w.len() as f32 * 6.0 + 8.0; // ~1.3 glyph-width space, below col_gap
            }
        }
        assert!(reconstruct(&chars).is_none(), "prose must not be a table");
    }

    #[test]
    fn too_few_rows_is_none() {
        let mut chars = Vec::new();
        chars.extend(word("A", 10.0, 100.0));
        chars.extend(word("B", 120.0, 100.0));
        assert!(reconstruct(&chars).is_none());
    }
}
