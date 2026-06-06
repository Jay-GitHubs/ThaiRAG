//! Generate a DOCX with a two-level (grouped) header — the canonical
//! column-merged-header case: "ยอดขาย / Sales" spans two quarter columns
//! (Q1, Q2), and "ภูมิภาค / Region" spans the two header rows. Used to verify
//! that a question answerable only if the colspan header is parsed correctly
//! (e.g. "Q2 sales of the North region") returns the right number.
//!
//! Run: `cargo run -p thairag-document --example make_grouped_header_docx`

use docx_rs::*;
use std::io::Cursor;

fn cell(text: &str) -> TableCell {
    TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(text)))
}

fn main() {
    let table = Table::new(vec![
        // Row 0: Region (rowspan 2) | Sales (colspan 2)
        TableRow::new(vec![
            cell("ภูมิภาค / Region").vertical_merge(VMergeType::Restart),
            cell("ยอดขาย / Sales").grid_span(2),
        ]),
        // Row 1: (region continues) | Q1 | Q2
        TableRow::new(vec![
            cell("").vertical_merge(VMergeType::Continue),
            cell("ไตรมาส 1 / Q1"),
            cell("ไตรมาส 2 / Q2"),
        ]),
        // Data
        TableRow::new(vec![cell("เหนือ / North"), cell("๑๐๐"), cell("๒๐๐")]),
        TableRow::new(vec![cell("ใต้ / South"), cell("๓๐๐"), cell("๔๐๐")]),
    ]);

    let docx = Docx::new()
        .add_paragraph(
            Paragraph::new().add_run(Run::new().add_text("ยอดขายรายไตรมาส / Quarterly Sales")),
        )
        .add_table(table);

    let mut buf = Cursor::new(Vec::new());
    docx.build().pack(&mut buf).expect("pack docx");
    let path = format!(
        "{}/../../tests/fixtures/grouped_header.docx",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::write(&path, buf.into_inner()).expect("write fixture");
    println!("wrote {path}");
}
