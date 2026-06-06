//! Generate a deliberately complex bilingual (Thai/English) DOCX with merged
//! cells — a header cell spanning two columns (gridSpan) and a category cell
//! spanning two rows (vMerge) — plus surrounding prose. Used as an e2e fixture
//! to prove the office-table reconstruction preserves merged-cell structure.
//!
//! Run: `cargo run -p thairag-document --example make_complex_docx`
//! Writes: tests/fixtures/complex_table.docx

use docx_rs::*;
use std::io::Cursor;

fn cell(text: &str) -> TableCell {
    TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(text)))
}

fn main() {
    let table = Table::new(vec![
        // Header: "หมวด/Category" spans 2 columns; then Value, Note.
        TableRow::new(vec![
            cell("หมวด / Category").grid_span(2),
            cell("มูลค่า / Value"),
            cell("หมายเหตุ / Note"),
        ]),
        // "กลุ่ม A" spans two rows (restart), then per-row items + Thai numerals.
        TableRow::new(vec![
            cell("กลุ่ม A").vertical_merge(VMergeType::Restart),
            cell("รายการ 1 / Item 1"),
            cell("๑,๒๓๔"),
            cell("ok"),
        ]),
        TableRow::new(vec![
            cell("").vertical_merge(VMergeType::Continue),
            cell("รายการ 2 / Item 2"),
            cell("๕,๖๗๘"),
            cell("ok2"),
        ]),
        TableRow::new(vec![
            cell("กลุ่ม B"),
            cell("รายการ 3 / Item 3"),
            cell("๙,๐๑๒"),
            cell("ดี"),
        ]),
    ]);

    let docx = Docx::new()
        .add_paragraph(
            Paragraph::new()
                .add_run(Run::new().add_text("รายงานทดสอบตารางซับซ้อน / Complex Table Test")),
        )
        .add_table(table)
        .add_paragraph(
            Paragraph::new().add_run(
                Run::new().add_text("สรุป: ตารางด้านบนมีเซลล์ผสาน / Summary: the table above has merged cells."),
            ),
        );

    let mut buf = Cursor::new(Vec::new());
    docx.build().pack(&mut buf).expect("pack docx");
    let path = format!(
        "{}/../../tests/fixtures/complex_table.docx",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::write(&path, buf.into_inner()).expect("write fixture");
    println!("wrote {path}");
}
