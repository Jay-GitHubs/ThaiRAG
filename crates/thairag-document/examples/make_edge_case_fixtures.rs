//! Generate the PDF edge-case table fixtures used by the coverage matrix.
//! ASCII text only (geometry/merge/image logic is script-agnostic; Thai is
//! covered by the real fixtures). Writes to tests/fixtures/edge-cases/.
//!
//! Run: cargo run -p thairag-document --example make_edge_case_fixtures

use printpdf::image_crate::{DynamicImage, Rgb as ImgRgb, RgbImage};
use printpdf::*;
use std::fs;

fn out(name: &str) -> String {
    format!(
        "{}/../../tests/fixtures/edge-cases/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

/// Draw one straight ruling-line segment (mm coordinates).
fn seg(layer: &PdfLayerReference, x0: f32, y0: f32, x1: f32, y1: f32) {
    layer.set_outline_color(Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));
    layer.set_outline_thickness(0.6);
    layer.add_line(Line {
        points: vec![
            (Point::new(Mm(x0), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y1)), false),
        ],
        is_closed: false,
    });
}

fn small_image() -> DynamicImage {
    // A 48x48 solid block — a stand-in for an in-cell logo/photo/chart.
    DynamicImage::ImageRgb8(RgbImage::from_pixel(48, 48, ImgRgb([40u8, 90, 200])))
}

fn save(doc: PdfDocumentReference, name: &str) {
    let bytes = doc.save_to_bytes().expect("save pdf");
    let path = out(name);
    fs::write(&path, bytes).expect("write fixture");
    println!("wrote {path}");
}

fn main() {
    fs::create_dir_all(out("")).ok();

    // Column boundaries (x) and row boundaries (y, top→bottom) shared by the
    // bordered fixtures: 3 columns, 3 rows.
    let xs = [20.0f32, 73.0, 127.0, 180.0];
    let ys = [255.0f32, 235.0, 215.0, 195.0];
    let cx = |c: usize| (xs[c] + xs[c + 1]) / 2.0 - 12.0;
    let cy = |r: usize| (ys[r] + ys[r + 1]) / 2.0 - 1.5;

    // ── 1. merge_block: a 2x2 merged block at rows{1,2} x cols{1,2}. ─────────
    {
        let (doc, pg, ly) = PdfDocument::new("merge_block", Mm(210.0), Mm(297.0), "L");
        let f = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        let l = doc.get_page(pg).get_layer(ly);
        // Outer border.
        seg(&l, xs[0], ys[0], xs[3], ys[0]);
        seg(&l, xs[0], ys[3], xs[3], ys[3]);
        seg(&l, xs[0], ys[0], xs[0], ys[3]);
        seg(&l, xs[3], ys[0], xs[3], ys[3]);
        // Row borders: full at y[1]; y[2] only under col0 (omit across merge).
        seg(&l, xs[0], ys[1], xs[3], ys[1]);
        seg(&l, xs[0], ys[2], xs[1], ys[2]);
        // Col borders: full at x[1]; x[2] only in row0 (omit across merge).
        seg(&l, xs[1], ys[0], xs[1], ys[3]);
        seg(&l, xs[2], ys[0], xs[2], ys[1]);
        // Text.
        for (c, t) in ["Region", "Q1", "Q2"].iter().enumerate() {
            l.use_text(*t, 11.0, Mm(cx(c)), Mm(cy(0)), &f);
        }
        l.use_text("North", 11.0, Mm(cx(0)), Mm(cy(1)), &f);
        l.use_text("South", 11.0, Mm(cx(0)), Mm(cy(2)), &f);
        l.use_text("MERGED 2x2", 11.0, Mm(cx(1)), Mm(cy(1)), &f); // anchor of the block
        save(doc, "merge_block.pdf");
    }

    // ── 2. hierarchical_header: row0 "Sales" colspans cols{1,2}; "Region" ────
    //      rowspans rows{0,1}; row1 sub-headers Q1/Q2; then a data row. ───────
    {
        let yy = [255.0f32, 240.0, 225.0, 210.0];
        let cyy = |r: usize| (yy[r] + yy[r + 1]) / 2.0 - 1.5;
        let (doc, pg, ly) = PdfDocument::new("hierarchical_header", Mm(210.0), Mm(297.0), "L");
        let f = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        let l = doc.get_page(pg).get_layer(ly);
        seg(&l, xs[0], yy[0], xs[3], yy[0]); // top
        seg(&l, xs[0], yy[3], xs[3], yy[3]); // bottom
        seg(&l, xs[0], yy[0], xs[0], yy[3]); // left
        seg(&l, xs[3], yy[0], xs[3], yy[3]); // right
        seg(&l, xs[0], yy[1], xs[3], yy[1]); // header row1 divider
        seg(&l, xs[0], yy[2], xs[3], yy[2]); // data divider
        seg(&l, xs[1], yy[0], xs[1], yy[3]); // col0|rest (full → Region rowspan, Sales colspan)
        seg(&l, xs[2], yy[1], xs[2], yy[3]); // Q1|Q2 split only below the "Sales" header
        l.use_text("Region", 11.0, Mm(cx(0)), Mm(cyy(0)), &f); // rowspan 2
        l.use_text("Sales", 11.0, Mm(cx(1)), Mm(cyy(0)), &f); // colspan 2
        l.use_text("Q1", 11.0, Mm(cx(1)), Mm(cyy(1)), &f);
        l.use_text("Q2", 11.0, Mm(cx(2)), Mm(cyy(1)), &f);
        l.use_text("North", 11.0, Mm(cx(0)), Mm(cyy(2)), &f);
        l.use_text("100", 11.0, Mm(cx(1)), Mm(cyy(2)), &f);
        l.use_text("200", 11.0, Mm(cx(2)), Mm(cyy(2)), &f);
        save(doc, "hierarchical_header.pdf");
    }

    // ── 3. images_in_cells: a 2-col table; col1 cells contain an image. ──────
    {
        let (doc, pg, ly) = PdfDocument::new("images_in_cells", Mm(210.0), Mm(297.0), "L");
        let f = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        let l = doc.get_page(pg).get_layer(ly);
        let xb = [20.0f32, 90.0, 150.0];
        let yb = [255.0f32, 235.0, 215.0];
        // Full grid (2 cols x 2 rows).
        seg(&l, xb[0], yb[0], xb[2], yb[0]);
        seg(&l, xb[0], yb[1], xb[2], yb[1]);
        seg(&l, xb[0], yb[2], xb[2], yb[2]);
        seg(&l, xb[0], yb[0], xb[0], yb[2]);
        seg(&l, xb[1], yb[0], xb[1], yb[2]);
        seg(&l, xb[2], yb[0], xb[2], yb[2]);
        l.use_text(
            "Logo A",
            11.0,
            Mm(xb[0] + 4.0),
            Mm((yb[0] + yb[1]) / 2.0),
            &f,
        );
        l.use_text(
            "Logo B",
            11.0,
            Mm(xb[0] + 4.0),
            Mm((yb[1] + yb[2]) / 2.0),
            &f,
        );
        // Place an image inside each col1 cell (dpi sizes it to ~14mm).
        for r in 0..2 {
            Image::from_dynamic_image(&small_image()).add_to_layer(
                l.clone(),
                ImageTransform {
                    translate_x: Some(Mm(xb[1] + 18.0)),
                    translate_y: Some(Mm(yb[r + 1] + 3.0)),
                    dpi: Some(96.0),
                    ..Default::default()
                },
            );
        }
        save(doc, "images_in_cells.pdf");
    }

    // ── 4. multipage_table: same table layout, header repeated on page 2. ────
    {
        let (doc, pg1, ly1) = PdfDocument::new("multipage_table", Mm(210.0), Mm(297.0), "L");
        let f = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        let draw_page = |l: &PdfLayerReference, rows: &[[&str; 3]]| {
            let yb = [255.0f32, 240.0, 225.0, 210.0];
            seg(l, xs[0], yb[0], xs[3], yb[0]);
            for (i, _) in yb.iter().enumerate().skip(1) {
                seg(l, xs[0], yb[i], xs[3], yb[i]);
            }
            for x in xs {
                seg(l, x, yb[0], x, yb[3]);
            }
            for (r, row) in rows.iter().enumerate() {
                for (c, t) in row.iter().enumerate() {
                    l.use_text(*t, 11.0, Mm(cx(c)), Mm((yb[r] + yb[r + 1]) / 2.0 - 1.5), &f);
                }
            }
        };
        draw_page(
            &doc.get_page(pg1).get_layer(ly1),
            &[
                ["Region", "Q1", "Q2"],
                ["North", "100", "200"],
                ["South", "300", "400"],
            ],
        );
        let (pg2, ly2) = doc.add_page(Mm(210.0), Mm(297.0), "L2");
        draw_page(
            &doc.get_page(pg2).get_layer(ly2),
            &[
                ["Region", "Q1", "Q2"],
                ["East", "500", "600"],
                ["West", "700", "800"],
            ],
        );
        save(doc, "multipage_table.pdf");
    }

    // ── 5. borderless_merged: no ruling lines; header "Sales" sits over the ──
    //      two value columns (a merged header in a borderless table). ─────────
    {
        let (doc, pg, ly) = PdfDocument::new("borderless_merged", Mm(210.0), Mm(297.0), "L");
        let f = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        let l = doc.get_page(pg).get_layer(ly);
        let colx = [20.0f32, 90.0, 140.0];
        l.use_text("Region", 12.0, Mm(colx[0]), Mm(255.0), &f);
        l.use_text("Sales", 12.0, Mm(colx[1] + 10.0), Mm(255.0), &f); // spans the 2 value cols
        let rows = [
            ["North", "100", "200"],
            ["South", "300", "400"],
            ["East", "500", "600"],
            ["West", "700", "800"],
            ["Central", "900", "1000"],
        ];
        let mut y = 245.0;
        for r in rows {
            for (c, t) in r.iter().enumerate() {
                l.use_text(*t, 12.0, Mm(colx[c]), Mm(y), &f);
            }
            y -= 8.0;
        }
        save(doc, "borderless_merged.pdf");
    }
}
