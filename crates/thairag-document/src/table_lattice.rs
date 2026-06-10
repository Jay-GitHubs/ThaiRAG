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
/// A clustered boundary is only real if ruling-line segments at that position
/// cover at least this fraction of the table's extent along the boundary. Real
/// Thai government tables draw borders as many short per-cell segments and add
/// decorative/sub-cell rules; without this gate those fragments split a
/// 3-column table into 20+ spurious thin columns (low fill → rejected → the
/// table is silently dropped to flat text).
const BOUNDARY_COVER_FRAC: f32 = 0.5;
/// Adjacent boundaries closer than this (points) are merged — collapses doubled
/// borders that survive the snap step as separate hairline columns/rows.
const MIN_CELL_GAP: f32 = 6.0;
/// Glyphs on the same text line whose horizontal gap is at most this many
/// median glyph widths apart coalesce into one text run. Tuned so intra-word
/// (and intra-phrase Thai) gaps merge while genuine column gutters do not.
const RUN_GAP_EM: f32 = 1.5;
/// Glyphs whose vertical centers differ by at most this many median glyph
/// heights belong to the same text line.
const LINE_TOL_EM: f32 = 0.6;
/// A rescued boundary must keep at least this many median glyph heights of
/// clearance from the text runs beside it. Real borders have cell padding;
/// dotted/dashed underlines hug the text they decorate (1–3 pt), so a
/// candidate touching text this closely is decoration, not a border.
const RESCUE_CLEARANCE_EM: f32 = 0.4;

/// A reconstructed table — shared by the lattice (bordered) and stream
/// (borderless) paths. Content comes from the text layer (exact); only the
/// structure is inferred, and scored with `confidence`.
#[derive(Debug, Clone)]
pub struct ReconstructedTable {
    /// Faithful HTML (`<table>`, colspan/rowspan on the lattice path); cell text escaped.
    pub html: String,
    /// Row-linearized, merge-filled text for embedding/search.
    pub linearized: String,
    /// Heuristic confidence in [0,1].
    pub confidence: f32,
    /// Fraction of the page's input glyphs that fell inside the grid. Near 1.0
    /// means the table dominates the page; low means a small table amid prose.
    pub char_coverage: f32,
    pub n_rows: usize,
    pub n_cols: usize,
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

/// Segment spans (sorted or not) at one boundary position. Returns the spans
/// belonging to `p` (within [`SNAP`]).
fn spans_at(lines: &[Line], p: f32) -> Vec<(f32, f32)> {
    lines
        .iter()
        .filter(|l| (l.pos - p).abs() <= SNAP)
        .map(|l| l.span)
        .collect()
}

/// Union length of (possibly overlapping) segment spans, bridging gaps ≤ SNAP.
fn union_len(spans: &mut [(f32, f32)]) -> f32 {
    if spans.is_empty() {
        return 0.0;
    }
    spans.sort_by(|a, b| fcmp(a.0, b.0));
    let mut union = 0.0;
    let mut cur = spans[0];
    for s in &spans[1..] {
        if s.0 <= cur.1 + SNAP {
            cur.1 = cur.1.max(s.1);
        } else {
            union += cur.1 - cur.0;
            cur = *s;
        }
    }
    union + (cur.1 - cur.0)
}

/// Do the spans, walked left-to-right with gaps ≤ SNAP bridged, cover [lo, hi]?
fn union_covers(spans: &mut [(f32, f32)], lo: f32, hi: f32) -> bool {
    if hi <= lo {
        return false;
    }
    spans.sort_by(|a, b| fcmp(a.0, b.0));
    let mut reach = lo;
    for s in spans.iter() {
        if s.0 > reach + SNAP {
            break;
        }
        reach = reach.max(s.1);
    }
    reach >= hi
}

/// Split clustered boundary candidates along one axis into (kept, dropped).
/// `extent` is the table's size along the *other* axis (table height for
/// vertical lines → column boundaries; table width for horizontal lines → row
/// boundaries). A candidate is kept when the union of its segment spans covers
/// ≥ [`BOUNDARY_COVER_FRAC`] of `extent`; the rest are returned as `dropped`
/// so merge-heavy minority borders can be rescued by the text-aware pass
/// ([`rescue_boundary`]) instead of silently collapsing their cells.
fn boundary_split(lines: &[Line], extent: f32) -> (Vec<f32>, Vec<f32>) {
    let candidates = cluster(lines.iter().map(|l| l.pos).collect());
    let (kept, dropped) = candidates.into_iter().partition(|&p| {
        let mut spans = spans_at(lines, p);
        extent > 0.0 && union_len(&mut spans) / extent >= BOUNDARY_COVER_FRAC
    });
    (kept, dropped)
}

/// Merge boundaries closer than [`MIN_CELL_GAP`] (doubled borders). Input need
/// not be sorted; output is ascending.
fn merge_close(mut bounds: Vec<f32>) -> Vec<f32> {
    bounds.sort_by(|a, b| fcmp(*a, *b));
    let mut merged: Vec<f32> = Vec::new();
    for p in bounds {
        if let Some(last) = merged.last_mut()
            && p - *last < MIN_CELL_GAP
        {
            *last = (*last + p) / 2.0;
            continue;
        }
        merged.push(p);
    }
    merged
}

/// Axis-aligned bounding box of a coalesced glyph run (one visual word/phrase
/// on a single text line). Used to decide whether a candidate gridline cuts
/// through text (impossible for a real border) or sits in a genuine gutter.
#[derive(Debug, Clone, Copy)]
struct TextRun {
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

/// Coalesce glyphs into per-line text runs: cluster base glyphs (combining
/// marks and whitespace excluded) into text lines by vertical center, then
/// merge horizontally-adjacent glyphs whose gap ≤ [`RUN_GAP_EM`] × median
/// glyph width. The run boxes approximate true text columns. Also returns the
/// median glyph height (the em scale for clearance/tolerance decisions).
fn text_runs(chars: &[PositionedChar]) -> (Vec<TextRun>, f32) {
    let mut base: Vec<&PositionedChar> = chars
        .iter()
        .filter(|c| !is_thai_combining_mark(c.ch) && !c.ch.is_whitespace())
        .collect();
    if base.is_empty() {
        return (Vec::new(), 1.0);
    }
    let median = |mut vals: Vec<f32>| -> f32 {
        vals.sort_by(|a, b| fcmp(*a, *b));
        vals[vals.len() / 2].max(1.0)
    };
    let med_h = median(base.iter().map(|c| (c.y1 - c.y0).abs()).collect());
    let med_w = median(base.iter().map(|c| (c.x1 - c.x0).abs()).collect());

    base.sort_by(|a, b| fcmp((a.y0 + a.y1) / 2.0, (b.y0 + b.y1) / 2.0));
    let mut lines: Vec<Vec<&PositionedChar>> = Vec::new();
    let mut cur = vec![base[0]];
    let mut cur_y = (base[0].y0 + base[0].y1) / 2.0;
    for g in &base[1..] {
        let cy = (g.y0 + g.y1) / 2.0;
        if cy - cur_y <= med_h * LINE_TOL_EM {
            cur.push(g);
        } else {
            lines.push(std::mem::replace(&mut cur, vec![g]));
        }
        cur_y = cy;
    }
    lines.push(cur);

    let mut runs: Vec<TextRun> = Vec::new();
    for mut line in lines {
        line.sort_by(|a, b| fcmp(a.x0, b.x0));
        let mut run = TextRun {
            x0: line[0].x0,
            x1: line[0].x1,
            y0: line[0].y0,
            y1: line[0].y1,
        };
        for g in &line[1..] {
            if g.x0 - run.x1 <= med_w * RUN_GAP_EM {
                run.x1 = run.x1.max(g.x1);
                run.y0 = run.y0.min(g.y0);
                run.y1 = run.y1.max(g.y1);
            } else {
                runs.push(run);
                run = TextRun {
                    x0: g.x0,
                    x1: g.x1,
                    y0: g.y0,
                    y1: g.y1,
                };
            }
        }
        runs.push(run);
    }
    (runs, med_h)
}

/// Text-aware rescue for a boundary candidate dropped by the coverage gate.
///
/// Merge-heavy grids draw an internal border only across the minority of rows
/// (or columns) that are NOT merged — its union coverage falls under
/// [`BOUNDARY_COVER_FRAC`] and the whole boundary vanishes, fusing real cells
/// (the `merge_block` collapse). A dropped candidate is reinstated only when it
/// behaves like a real border somewhere:
/// 1. its segments fully cover ≥1 cell band between adjacent *kept*
///    perpendicular boundaries (decorative ticks and sub-cell rules never span
///    a full band — this is what keeps noisy real Thai PDFs unsegmented);
/// 2. in every band it fully covers, no text run straddles it (a border cannot
///    cut through glyphs);
/// 3. in every covered band, no text run sits closer than `clearance` to the
///    candidate (real borders have cell padding; dotted/dashed underlines hug
///    the text they decorate, and in rotated pages they masquerade as
///    full-band gridlines at text-line pitch);
/// 4. in at least one covered band, BOTH sub-cells it carves out of its
///    neighbouring kept same-axis boundaries contain text. Checking the
///    sub-cells (not just "text somewhere on each side") rejects the
///    double-stroke hairlines real Thai PDFs draw a few points away from a
///    true border — those would otherwise create empty sliver rows/columns.
///
/// `vertical` selects the axis: true = candidate is a column boundary at x=`p`
/// with `perp_bounds`=row boundaries and `axis_bounds`=kept column boundaries
/// (both ascending); false = row boundary with the roles swapped.
fn rescue_boundary(
    p: f32,
    segs: &[Line],
    perp_bounds: &[f32],
    axis_bounds: &[f32],
    runs: &[TextRun],
    clearance: f32,
    vertical: bool,
) -> bool {
    let mut spans = spans_at(segs, p);
    if spans.is_empty() {
        return false;
    }
    // Neighbouring kept boundaries on the candidate's own axis: the cell span
    // (prev, next) that `p` would split. Outside the kept range = not internal.
    let prev = axis_bounds
        .iter()
        .copied()
        .filter(|&b| b < p)
        .fold(f32::NEG_INFINITY, f32::max);
    let next = axis_bounds
        .iter()
        .copied()
        .filter(|&b| b > p)
        .fold(f32::INFINITY, f32::min);
    if !prev.is_finite() || !next.is_finite() {
        return false;
    }
    // (along boundary axis, perpendicular axis) extents of a run.
    let along = |r: &TextRun| if vertical { (r.x0, r.x1) } else { (r.y0, r.y1) };
    let perp = |r: &TextRun| if vertical { (r.y0, r.y1) } else { (r.x0, r.x1) };
    let mut separates_text = false;
    for w in perp_bounds.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if !union_covers(&mut spans, lo + SNAP, hi - SNAP) {
            continue;
        }
        let band_runs: Vec<&TextRun> = runs
            .iter()
            .filter(|r| {
                let (a, b) = perp(r);
                b > lo && a < hi // any overlap with the band
            })
            .collect();
        if band_runs.iter().any(|r| {
            let (a, b) = along(r);
            a < p - SNAP && b > p + SNAP
        }) {
            return false; // a real border cannot cut through text
        }
        if band_runs.iter().any(|r| {
            let (a, b) = along(r);
            (b <= p + SNAP && p - b < clearance) || (a >= p - SNAP && a - p < clearance)
        }) {
            return false; // hugging text = underline/decoration, not a border
        }
        let in_gap = |g_lo: f32, g_hi: f32| {
            band_runs.iter().any(|r| {
                let (a, b) = along(r);
                b > g_lo + SNAP && a < g_hi - SNAP
            })
        };
        if in_gap(prev, p) && in_gap(p, next) {
            separates_text = true;
        }
    }
    separates_text
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
pub fn reconstruct(chars: &[PositionedChar], lines: &[RuleLine]) -> Option<ReconstructedTable> {
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

    if h.is_empty() || v.is_empty() {
        return None;
    }
    // Table bounding box from all ruling lines (positions + span endpoints), so
    // a boundary's coverage can be judged against the table's true extent.
    let mut x_min = f32::INFINITY;
    let mut x_max = f32::NEG_INFINITY;
    let mut y_min = f32::INFINITY;
    let mut y_max = f32::NEG_INFINITY;
    for l in &v {
        x_min = x_min.min(l.pos);
        x_max = x_max.max(l.pos);
        y_min = y_min.min(l.span.0);
        y_max = y_max.max(l.span.1);
    }
    for l in &h {
        y_min = y_min.min(l.pos);
        y_max = y_max.max(l.pos);
        x_min = x_min.min(l.span.0);
        x_max = x_max.max(l.span.1);
    }
    let table_w = (x_max - x_min).max(1.0);
    let table_h = (y_max - y_min).max(1.0);

    // Distinct, significance-filtered row boundaries (y) and column boundaries
    // (x). Candidates failing the coverage gate are not discarded outright:
    // merge-heavy tables draw internal borders across only the unmerged
    // minority of a span, so each dropped candidate gets a text-aware second
    // chance against the kept perpendicular boundaries (see [`rescue_boundary`]).
    let (ys_keep, ys_drop) = boundary_split(&h, table_w);
    let (xs_keep, xs_drop) = boundary_split(&v, table_h);
    let ys_keep = merge_close(ys_keep);
    let xs_keep = merge_close(xs_keep);
    if ys_keep.len() < 2 || xs_keep.len() < 2 {
        return None; // not a ruled grid
    }
    let (runs, med_h) = text_runs(chars);
    let clearance = med_h * RESCUE_CLEARANCE_EM;
    let mut ys_all = ys_keep.clone();
    ys_all.extend(
        ys_drop
            .into_iter()
            .filter(|&p| rescue_boundary(p, &h, &xs_keep, &ys_keep, &runs, clearance, false)),
    );
    let mut xs_all = xs_keep.clone();
    xs_all.extend(
        xs_drop
            .into_iter()
            .filter(|&p| rescue_boundary(p, &v, &ys_keep, &xs_keep, &runs, clearance, true)),
    );
    let ys_asc = merge_close(ys_all);
    let xs = merge_close(xs_all);
    // Rows ordered top→bottom: PDF y increases upward, so descend.
    let mut ys: Vec<f32> = ys_asc.clone();
    ys.sort_by(|a, b| fcmp(*b, *a));

    let n_rows = ys.len() - 1;
    let n_cols = xs.len() - 1;

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

    Some(ReconstructedTable {
        html,
        linearized,
        confidence,
        char_coverage,
        n_rows,
        n_cols,
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
    fn ignores_short_decorative_fragments() {
        // A clean 2x2 grid plus short sub-cell tick marks (decorative fragments
        // covering a tiny fraction of the table) must NOT create extra columns
        // or rows — this is the real-Thai-PDF over-segmentation case.
        let mut lines = grid_2x2_lines();
        lines.push(vline(50.0, 0.0, 8.0)); // tiny vertical tick mid-column
        lines.push(vline(150.0, 190.0, 200.0)); // another short fragment
        lines.push(hline(50.0, 0.0, 6.0)); // tiny horizontal fragment
        let chars = vec![
            ch('a', 40.0, 150.0),
            ch('b', 140.0, 150.0),
            ch('c', 40.0, 50.0),
            ch('d', 140.0, 50.0),
        ];
        let t = reconstruct(&chars, &lines).expect("grid");
        assert_eq!(
            (t.n_rows, t.n_cols),
            (2, 2),
            "short fragments must not add boundaries, got {}x{}",
            t.n_rows,
            t.n_cols
        );
    }

    // A merge-heavy 3x3 grid whose internal borders exist only across the
    // unmerged minority: vertical x=200 only in row 0, horizontal y=100 only
    // under col 0 (the merge_block.pdf fixture shape). Coverage alone drops
    // both (1/3 < BOUNDARY_COVER_FRAC) and the grid used to collapse to 2x2.
    fn merge_heavy_lines() -> Vec<RuleLine> {
        vec![
            hline(0.0, 0.0, 300.0),
            hline(300.0, 0.0, 300.0),
            vline(0.0, 0.0, 300.0),
            vline(300.0, 0.0, 300.0),
            hline(200.0, 0.0, 300.0),   // full row divider
            hline(100.0, 0.0, 100.0),   // row divider only under col 0
            vline(100.0, 0.0, 300.0),   // full col divider
            vline(200.0, 200.0, 300.0), // col divider only in top row
        ]
    }

    #[test]
    fn rescues_minority_borders_in_merge_heavy_grid() {
        let chars = vec![
            ch('a', 40.0, 250.0), // row 0
            ch('b', 140.0, 250.0),
            ch('c', 240.0, 250.0),
            ch('d', 40.0, 150.0), // col 0, rows 1-2
            ch('e', 40.0, 50.0),
            ch('M', 140.0, 150.0), // anchor of the 2x2 merged block
        ];
        let t = reconstruct(&chars, &merge_heavy_lines()).expect("grid");
        assert_eq!(
            (t.n_rows, t.n_cols),
            (3, 3),
            "minority borders must be rescued, got {}x{}",
            t.n_rows,
            t.n_cols
        );
        assert!(
            t.html.contains("colspan=\"2\"") && t.html.contains("rowspan=\"2\""),
            "expected the 2x2 merged block, html: {}",
            t.html
        );
        assert_eq!(t.linearized, "a | b | c\nd | M | M\ne | M | M");
    }

    #[test]
    fn rejects_double_stroke_hairline() {
        // A clean 3x3 grid plus a minority hairline 8pt right of a kept column
        // border: the sliver it would carve out contains no text → rejected.
        let mut lines = vec![
            hline(0.0, 0.0, 300.0),
            hline(100.0, 0.0, 300.0),
            hline(200.0, 0.0, 300.0),
            hline(300.0, 0.0, 300.0),
            vline(0.0, 0.0, 300.0),
            vline(100.0, 0.0, 300.0),
            vline(200.0, 0.0, 300.0),
            vline(300.0, 0.0, 300.0),
        ];
        lines.push(vline(108.0, 200.0, 300.0)); // double stroke, top band only
        let chars = vec![
            ch('a', 40.0, 250.0),
            ch('b', 140.0, 250.0),
            ch('c', 240.0, 250.0),
        ];
        let t = reconstruct(&chars, &lines).expect("grid");
        assert_eq!(
            (t.n_rows, t.n_cols),
            (3, 3),
            "hairline must not be rescued, got {}x{}",
            t.n_rows,
            t.n_cols
        );
    }

    #[test]
    fn rejects_text_hugging_underline() {
        // A 2-row x 3-col grid plus a minority horizontal rule hugging the
        // text right above it (an underline, not a border) → rejected.
        let mut lines = vec![
            hline(0.0, 0.0, 300.0),
            hline(100.0, 0.0, 300.0),
            hline(200.0, 0.0, 300.0),
            vline(0.0, 0.0, 200.0),
            vline(100.0, 0.0, 200.0),
            vline(200.0, 0.0, 200.0),
            vline(300.0, 0.0, 200.0),
        ];
        lines.push(hline(140.0, 0.0, 100.0)); // covers col 0 band only
        let chars = vec![
            ch('a', 40.0, 142.0), // sits 2pt above the rule (underlined text)
            ch('x', 40.0, 110.0), // more text below, inside the same cell
            ch('b', 140.0, 150.0),
            ch('c', 40.0, 50.0),
        ];
        let t = reconstruct(&chars, &lines).expect("grid");
        assert_eq!(
            (t.n_rows, t.n_cols),
            (2, 3),
            "underline must not become a row border, got {}x{}",
            t.n_rows,
            t.n_cols
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
