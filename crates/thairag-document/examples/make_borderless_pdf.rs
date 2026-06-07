//! Generate a borderless table PDF fixture — a table laid out purely by column
//! whitespace, with NO ruling lines, plus a prose paragraph above and below it.
//! Exercises the stream (borderless) reconstruction path and the prose-vs-table
//! region detection. ASCII/numbers only so it embeds no external font (the
//! borderless detection is script-agnostic; Thai char handling is covered by
//! the bordered real-Thai fixture + shared code).
//!
//! Run: `cargo run -p thairag-document --example make_borderless_pdf`

use printpdf::*;
use std::fs::File;
use std::io::BufWriter;

fn main() {
    let (doc, page, layer) = PdfDocument::new("Borderless Table", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let l = doc.get_page(page).get_layer(layer);

    // Short title (one line — excluded from the table region; must NOT leak
    // into a cell). Kept short so the page stays table-dominant.
    l.use_text("Quarterly Sales", 12.0, Mm(20.0), Mm(270.0), &font);

    // Borderless table: 3 columns at fixed x, no lines drawn. Many rows so the
    // table dominates the page (clears the table-dominance coverage gate).
    let cols_mm = [20.0_f32, 90.0, 150.0];
    let rows = [
        ["Region", "Q1 Sales", "Q2 Sales"],
        ["North", "100", "200"],
        ["South", "300", "400"],
        ["East", "500", "600"],
        ["West", "700", "800"],
        ["Central", "900", "1000"],
        ["Northeast", "1100", "1200"],
        ["Northwest", "1300", "1400"],
        ["Southeast", "1500", "1600"],
        ["Southwest", "1700", "1800"],
    ];
    let mut y = 255.0_f32;
    for r in rows {
        for (c, cell) in r.iter().enumerate() {
            l.use_text(*cell, 12.0, Mm(cols_mm[c]), Mm(y), &font);
        }
        y -= 8.0;
    }

    let path = format!(
        "{}/../../tests/fixtures/borderless_table.pdf",
        env!("CARGO_MANIFEST_DIR")
    );
    doc.save(&mut BufWriter::new(File::create(&path).unwrap()))
        .unwrap();
    println!("wrote {path}");
}
