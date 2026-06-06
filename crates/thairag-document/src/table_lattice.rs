//! Deterministic "lattice" table reconstruction from PDF geometry.
//!
//! Given the exact glyph boxes and ruling-line segments of a page (from
//! [`crate::pdfium_engine::PageGeometry`]), rebuild a bordered table the way a
//! human reads gridlines: vertical lines mark column edges, horizontal lines
//! mark row edges, a *missing* internal border means two cells are merged, and
//! each glyph falls inside exactly one cell. Cell CONTENT comes straight from
//! the text layer, so numbers are exact and can never be fabricated — only the
//! grid structure is inferred (and scored with a confidence value).
//!
//! This module is pure (no pdfium handles) so it is unit-testable with
//! synthetic geometry.

use std::cmp::Ordering;

use crate::pdfium_engine::{PositionedChar, RuleLine};

/// Total-order float compare (NaN treated as equal) — safe for `sort_by`.
fn fcmp(a: f32, b: f32) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// Snap tolerance (points) for clustering near-coincident gridlines.
const SNAP: f32 = 3.0;
/// How close a glyph's neighbour must be (points) — unused placeholder kept for
/// future word-grouping; cell assignment uses glyph centers directly.
const _RESERVED: f32 = 0.0;

/// A reconstructed table.
#[derive(Debug, Clone)]
pub struct LatticeTable {
    /// Faithful HTML (`<table>` with colspan/rowspan); cell text is escaped.
    pub html: String,
    /// Row-linearized, merge-filled text for embedding/search.
    pub linearized: String,
    /// Heuristic confidence in [0,1] (fraction of grid cells that held text).
    pub confidence: f32,
    /// Fraction of the page's input glyphs that fell inside the grid. Near 1.0
    /// means the table dominates the page; low means a small table amid prose
    /// (caller may decline to replace the whole page body).
    pub char_coverage: f32,
    pub n_rows: usize,
    pub n_cols: usize,
    /// Grid bounding box in PDF point space, so callers can separate the
    /// surrounding prose (text above/below the table).
    pub x_min: f32,
    pub x_max: f32,
    pub y_min: f32,
    pub y_max: f32,
}

/// Cluster a set of scalar coordinates into representative positions, merging
/// any within `SNAP` points. Returns sorted ascending.
fn cluster(mut vals: Vec<f32>) -> Vec<f32> {
    if vals.is_empty() {
        return Vec::new();
    }
    vals.sort_by(|a, b| fcmp(*a, *b));
    let mut out = Vec::new();
    let mut group = vec![vals[0]];
    for &v in &vals[1..] {
        if v - *group.last().unwrap() <= SNAP {
            group.push(v);
        } else {
            out.push(group.iter().sum::<f32>() / group.len() as f32);
            group = vec![v];
        }
    }
    out.push(group.iter().sum::<f32>() / group.len() as f32);
    out
}

/// Thai combining marks — above/below vowels and tone marks (zero-advance
/// glyphs positioned relative to a base consonant). They must inherit their
/// base glyph's cell rather than be placed by their own offset bounding box.
fn is_thai_combining_mark(c: char) -> bool {
    matches!(c,
        '\u{0E31}'                 // MAI HAN-AKAT
        | '\u{0E34}'..='\u{0E3A}'  // SARA I .. PHINTHU (above/below vowels)
        | '\u{0E47}'..='\u{0E4E}'  // MAITAIKHU .. YAMAKKAN (tone marks etc.)
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

struct Line {
    pos: f32,         // the constant axis (y for horizontal, x for vertical)
    span: (f32, f32), // extent along the other axis (min, max)
}

/// Reconstruct a bordered table from page geometry. Returns `None` when the
/// geometry does not form a grid (fewer than 2 distinct row or column borders)
/// — i.e. this is not a lattice (ruled) table and the caller should fall back.
pub fn reconstruct(chars: &[PositionedChar], lines: &[RuleLine]) -> Option<LatticeTable> {
    // Split ruling lines into horizontal and vertical, capturing position +
    // extent so we can later test whether an internal border covers a cell.
    let mut h: Vec<Line> = Vec::new();
    let mut v: Vec<Line> = Vec::new();
    for l in lines {
        let dx = (l.x1 - l.x0).abs();
        let dy = (l.y1 - l.y0).abs();
        if dy <= SNAP && dx > SNAP {
            h.push(Line {
                pos: (l.y0 + l.y1) / 2.0,
                span: (l.x0.min(l.x1), l.x0.max(l.x1)),
            });
        } else if dx <= SNAP && dy > SNAP {
            v.push(Line {
                pos: (l.x0 + l.x1) / 2.0,
                span: (l.y0.min(l.y1), l.y0.max(l.y1)),
            });
        }
    }

    // Distinct row boundaries (y) and column boundaries (x).
    let ys_asc = cluster(h.iter().map(|l| l.pos).collect());
    let xs = cluster(v.iter().map(|l| l.pos).collect());
    if ys_asc.len() < 2 || xs.len() < 2 {
        return None; // not a ruled grid
    }
    // Rows ordered top→bottom: PDF y increases upward, so descend.
    let mut ys: Vec<f32> = ys_asc.clone();
    ys.sort_by(|a, b| fcmp(*b, *a));

    let n_rows = ys.len() - 1;
    let n_cols = xs.len() - 1;
    let x_min = *xs.first().unwrap();
    let x_max = *xs.last().unwrap();
    let y_min = *ys_asc.first().unwrap();
    let y_max = *ys_asc.last().unwrap();

    // Does a vertical line sit at column-boundary `b` (1..n_cols) and cover the
    // y-span of row `r`? Boundary x = xs[b]; row band = [ys[r+1], ys[r]].
    let vborder_covers = |r: usize, b: usize| -> bool {
        let bx = xs[b];
        let (row_lo, row_hi) = (ys[r + 1], ys[r]);
        v.iter().any(|l| {
            (l.pos - bx).abs() <= SNAP
                && l.span.0 <= row_hi - SNAP // overlaps the row band
                && l.span.1 >= row_lo + SNAP
        })
    };
    // Does a horizontal line sit at row-boundary `b` (1..n_rows) and cover the
    // x-span of column `c`? Boundary y = ys[b]; col band = [xs[c], xs[c+1]].
    let hborder_covers = |b: usize, c: usize| -> bool {
        let by = ys[b];
        let (col_lo, col_hi) = (xs[c], xs[c + 1]);
        h.iter().any(|l| {
            (l.pos - by).abs() <= SNAP && l.span.0 <= col_hi - SNAP && l.span.1 >= col_lo + SNAP
        })
    };

    // Assign each glyph to its base cell (row, col) by center point.
    let col_of = |cx: f32| -> Option<usize> {
        (0..n_cols).find(|&c| cx >= xs[c] - SNAP && cx <= xs[c + 1] + SNAP)
    };
    let row_of = |cy: f32| -> Option<usize> {
        // ys is descending: row r band is [ys[r+1], ys[r]] (bottom, top).
        (0..n_rows).find(|&r| cy <= ys[r] + SNAP && cy >= ys[r + 1] - SNAP)
    };
    // Per base cell: glyphs in pdfium's emission order. We DELIBERATELY do not
    // re-sort by geometry: Thai (and other complex scripts) position combining
    // vowels/tone marks above/below their base consonant — different y — so a
    // positional sort scrambles them away from their base. pdfium already
    // yields characters in logical order (the project's trusted Thai text
    // path), so preserving insertion order keeps cell text correct.
    let mut cell_glyphs: Vec<Vec<char>> = vec![Vec::new(); n_rows * n_cols];
    let mut last_idx: Option<usize> = None;
    for ch in chars {
        // Thai combining marks (above/below vowels, tone marks) are zero-advance
        // glyphs whose box sits over/under the base consonant and can fall just
        // outside the base's cell at a column edge. Keep them in the preceding
        // glyph's cell so they never orphan into a neighbouring column.
        let idx = if is_thai_combining_mark(ch.ch) {
            last_idx
        } else {
            let cx = (ch.x0 + ch.x1) / 2.0;
            let cy = (ch.y0 + ch.y1) / 2.0;
            match (col_of(cx), row_of(cy)) {
                (Some(c), Some(r)) => Some(r * n_cols + c),
                _ => None,
            }
        };
        if let Some(i) = idx {
            cell_glyphs[i].push(ch.ch);
            last_idx = Some(i);
        }
    }

    let base_text = |r: usize, c: usize| -> String {
        cell_glyphs[r * n_cols + c]
            .iter()
            .collect::<String>()
            .trim()
            .to_string()
    };

    // Compute merged-cell spans by scanning for missing internal borders.
    let mut consumed = vec![false; n_rows * n_cols];
    struct Anchor {
        r: usize,
        c: usize,
        rs: usize,
        cs: usize,
    }
    let mut anchors: Vec<Anchor> = Vec::new();
    for r in 0..n_rows {
        for c in 0..n_cols {
            if consumed[r * n_cols + c] {
                continue;
            }
            // Extend right while the vertical border to the right is absent.
            let mut cs = 1;
            while c + cs < n_cols && !vborder_covers(r, c + cs) {
                cs += 1;
            }
            // Extend down while the horizontal border below (under col c) is absent.
            let mut rs = 1;
            while r + rs < n_rows && !hborder_covers(r + rs, c) {
                rs += 1;
            }
            for dr in 0..rs {
                for dc in 0..cs {
                    consumed[(r + dr) * n_cols + (c + dc)] = true;
                }
            }
            anchors.push(Anchor { r, c, rs, cs });
        }
    }

    // Anchor text = concatenation of its covered base cells (reading order).
    let anchor_text = |a: &Anchor| -> String {
        let mut parts: Vec<String> = Vec::new();
        for dr in 0..a.rs {
            for dc in 0..a.cs {
                let t = base_text(a.r + dr, a.c + dc);
                if !t.is_empty() {
                    parts.push(t);
                }
            }
        }
        parts.join(" ")
    };

    // Build HTML rows. Emit each anchor only at its top-left cell.
    let mut html = String::from("<table>");
    for r in 0..n_rows {
        html.push_str("<tr>");
        for c in 0..n_cols {
            if let Some(a) = anchors.iter().find(|a| a.r == r && a.c == c) {
                let mut td = String::from("<td");
                if a.cs > 1 {
                    td.push_str(&format!(" colspan=\"{}\"", a.cs));
                }
                if a.rs > 1 {
                    td.push_str(&format!(" rowspan=\"{}\"", a.rs));
                }
                td.push('>');
                td.push_str(&escape_html(&anchor_text(a)));
                td.push_str("</td>");
                html.push_str(&td);
            }
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");

    // Linearized, merge-filled text for embedding: each row a "a | b | c" line,
    // with a merged cell's value repeated across every column/row it covers so
    // each row is self-contained for retrieval.
    let mut fill: Vec<Vec<String>> = vec![vec![String::new(); n_cols]; n_rows];
    for a in &anchors {
        let t = anchor_text(a);
        for dr in 0..a.rs {
            for dc in 0..a.cs {
                fill[a.r + dr][a.c + dc] = t.clone();
            }
        }
    }
    let linearized = fill
        .iter()
        .map(|row| row.join(" | "))
        .collect::<Vec<_>>()
        .join("\n");

    // Confidence = fraction of base cells that received any text. Empty cells
    // are legitimate, but a very low fill ratio signals a misdetected grid.
    let filled = (0..n_rows * n_cols)
        .filter(|&i| !cell_glyphs[i].is_empty())
        .count();
    let confidence = filled as f32 / (n_rows * n_cols) as f32;
    let assigned: usize = cell_glyphs.iter().map(|c| c.len()).sum();
    let char_coverage = if chars.is_empty() {
        0.0
    } else {
        assigned as f32 / chars.len() as f32
    };

    Some(LatticeTable {
        html,
        linearized,
        confidence,
        char_coverage,
        n_rows,
        n_cols,
        x_min,
        x_max,
        y_min,
        y_max,
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
            x1: x + 5.0,
            y1: y + 8.0,
        }
    }
    // Horizontal line at y across [x0,x1]; vertical line at x across [y0,y1].
    fn hline(y: f32, x0: f32, x1: f32) -> RuleLine {
        RuleLine {
            x0,
            y0: y,
            x1,
            y1: y,
        }
    }
    fn vline(x: f32, y0: f32, y1: f32) -> RuleLine {
        RuleLine {
            x0: x,
            y0,
            x1: x,
            y1,
        }
    }

    // A clean 2x2 grid: x in {0,100,200}, y in {0,100,200}.
    fn grid_2x2_lines() -> Vec<RuleLine> {
        vec![
            hline(0.0, 0.0, 200.0),
            hline(100.0, 0.0, 200.0),
            hline(200.0, 0.0, 200.0),
            vline(0.0, 0.0, 200.0),
            vline(100.0, 0.0, 200.0),
            vline(200.0, 0.0, 200.0),
        ]
    }

    #[test]
    fn reconstructs_simple_2x2() {
        // Row 0 (top, y~150): a,b ; Row 1 (y~50): c,d
        let chars = vec![
            ch('a', 40.0, 150.0),
            ch('b', 140.0, 150.0),
            ch('c', 40.0, 50.0),
            ch('d', 140.0, 50.0),
        ];
        let t = reconstruct(&chars, &grid_2x2_lines()).expect("grid");
        assert_eq!((t.n_rows, t.n_cols), (2, 2));
        assert!(t.html.contains("<td>a</td><td>b</td>"), "html: {}", t.html);
        assert!(t.html.contains("<td>c</td><td>d</td>"), "html: {}", t.html);
        assert!(!t.html.contains("colspan"));
        assert_eq!(t.linearized, "a | b\nc | d");
        assert!(t.confidence > 0.99);
    }

    #[test]
    fn merges_cell_when_internal_border_missing() {
        // Drop the vertical border between cols in the TOP row only, so row 0 is
        // one cell spanning both columns; row 1 stays split.
        let mut lines = vec![
            hline(0.0, 0.0, 200.0),
            hline(100.0, 0.0, 200.0),
            hline(200.0, 0.0, 200.0),
            vline(0.0, 0.0, 200.0),
            vline(200.0, 0.0, 200.0),
            // vertical mid border ONLY across the bottom row band [0,100]:
            vline(100.0, 0.0, 100.0),
        ];
        lines.shrink_to_fit();
        let chars = vec![
            ch('H', 90.0, 150.0), // spans top
            ch('c', 40.0, 50.0),
            ch('d', 140.0, 50.0),
        ];
        let t = reconstruct(&chars, &lines).expect("grid");
        assert_eq!((t.n_rows, t.n_cols), (2, 2));
        assert!(
            t.html.contains("colspan=\"2\""),
            "expected a merged top cell, html: {}",
            t.html
        );
    }

    #[test]
    fn not_a_table_without_grid() {
        let chars = vec![ch('x', 10.0, 10.0)];
        assert!(reconstruct(&chars, &[]).is_none());
        // Only one boundary each → still not a grid.
        let lines = vec![hline(0.0, 0.0, 100.0), vline(0.0, 0.0, 100.0)];
        assert!(reconstruct(&chars, &lines).is_none());
    }
}
