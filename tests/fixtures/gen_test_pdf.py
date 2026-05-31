#!/usr/bin/env python3
"""Generate a test PDF that reproduces the split-table problem.

The doc mimics a PowerPoint-exported "Micro SME Product Management" deck with a
large bilingual (Thai/English) "Prohibited Business / Cautions" table. The
table's extracted markdown is several thousand characters — well over the
default max_chunk_size of 512 — so the chunker splits it across fragments.
Raising max_chunk_size (per-workspace) keeps it atomic.
"""
from fpdf import FPDF
from fpdf.fonts import FontFace

FONT = "/System/Library/Fonts/Supplemental/Ayuthaya.ttf"
OUT = "tests/fixtures/micro_sme_prohibited_business.pdf"

# (No., Business type, Reason / prohibition, Cautions)
ROWS = [
    ("1", "อาวุธและยุทโธปกรณ์ / Weapons & munitions",
     "ผิดกฎหมายควบคุมอาวุธ ห้ามปล่อยสินเชื่อทุกกรณี / Prohibited under arms-control law; no lending in any case",
     "ตรวจสอบใบอนุญาตและบัญชีรายชื่อต้องห้ามระหว่างประเทศ / Verify licences and international watchlists"),
    ("2", "การพนันและบ่อนคาสิโน / Gambling & casinos",
     "เป็นธุรกิจผิดกฎหมายในประเทศ มีความเสี่ยงด้านชื่อเสียง / Illegal domestically; high reputational risk",
     "ปฏิเสธคำขอและบันทึกเหตุผลการปฏิเสธ / Decline and record the rejection reason"),
    ("3", "ยาเสพติดและสารควบคุม / Narcotics & controlled substances",
     "ขัดต่อกฎหมายอาญา ห้ามให้บริการทางการเงิน / Violates criminal law; no financial services allowed",
     "รายงานธุรกรรมที่น่าสงสัยต่อ ปปง. / Report suspicious transactions to AMLO"),
    ("4", "สินค้าละเมิดลิขสิทธิ์ / Counterfeit goods",
     "เสี่ยงต่อการฟ้องร้องและยึดทรัพย์ / Risk of litigation and asset seizure",
     "ขอหลักฐานสิทธิ์ในตราสินค้าก่อนพิจารณา / Require brand-rights evidence before review"),
    ("5", "ธุรกิจค้าประเวณี / Prostitution-related business",
     "ผิดกฎหมายและขัดนโยบายความเสี่ยงของธนาคาร / Illegal and against the bank's risk policy",
     "ห้ามอนุมัติ และแจ้งหน่วยงานกำกับหากจำเป็น / Do not approve; notify regulator if needed"),
    ("6", "การฟอกเงิน / Money laundering schemes",
     "ความผิดร้ายแรงตามกฎหมาย ปปง. / Serious offence under AML law",
     "ใช้กระบวนการ KYC/CDD เข้มงวดทุกขั้นตอน / Apply strict KYC/CDD at every step"),
    ("7", "สกุลเงินดิจิทัลที่ไม่ได้รับอนุญาต / Unlicensed crypto exchange",
     "ยังไม่มีใบอนุญาตจาก ก.ล.ต. ถือว่าผิดระเบียบ / No SEC licence; treated as non-compliant",
     "ตรวจสอบสถานะใบอนุญาตกับ ก.ล.ต. ก่อนเสมอ / Always check licence status with the SEC"),
    ("8", "ธุรกิจแชร์ลูกโซ่ / Pyramid / Ponzi schemes",
     "หลอกลวงประชาชนและผิดกฎหมายขายตรง / Defrauds the public; violates direct-sales law",
     "ระวังอัตราผลตอบแทนที่สูงผิดปกติ / Beware abnormally high promised returns"),
    ("9", "การค้าสัตว์ป่าคุ้มครอง / Protected-wildlife trade",
     "ผิดอนุสัญญา CITES และกฎหมายสงวนพันธุ์ / Breaches CITES and wildlife-protection law",
     "ขอเอกสารถิ่นกำเนิดและใบอนุญาตส่งออก / Request origin documents and export permits"),
    ("10", "ดอกไม้เพลิงและวัตถุระเบิด / Fireworks & explosives",
     "ต้องมีใบอนุญาตพิเศษ ความเสี่ยงด้านความปลอดภัยสูง / Needs special permit; high safety risk",
     "จำกัดวงเงินและตรวจสถานที่จัดเก็บ / Cap the limit and inspect the storage site"),
    ("11", "ธุรกิจกู้นอกระบบ / Informal loan-shark lending",
     "คิดดอกเบี้ยเกินอัตราที่กฎหมายกำหนด / Charges interest above the legal ceiling",
     "ปฏิเสธและให้คำแนะนำสินเชื่อในระบบ / Decline and advise on formal credit options"),
    ("12", "สินค้าเลียนแบบยาและอาหารเสริม / Fake drugs & supplements",
     "เป็นอันตรายต่อผู้บริโภคและผิดกฎหมาย อย. / Endangers consumers; violates FDA law",
     "ขอเลขทะเบียน อย. และตรวจสอบความถูกต้อง / Require FDA registration and verify it"),
]

HEADER = ("ลำดับ\nNo.", "ประเภทธุรกิจ\nBusiness type",
          "เหตุผล / ข้อห้าม\nReason / prohibition", "ข้อควรระวัง\nCautions")

pdf = FPDF(orientation="L", unit="mm", format="A4")
pdf.add_font("ayuthaya", "", FONT)
pdf.set_font("ayuthaya", "", 18)
pdf.add_page()

pdf.cell(0, 10, "การจัดการผลิตภัณฑ์สำหรับ Micro SME", new_x="LMARGIN", new_y="NEXT")
pdf.set_font("ayuthaya", "", 13)
pdf.cell(0, 8, "Micro SME Product Management — Slide 7: Prohibited Business & Cautions",
         new_x="LMARGIN", new_y="NEXT")
pdf.ln(2)
pdf.set_font("ayuthaya", "", 10)
pdf.multi_cell(0, 5,
    "ตารางต่อไปนี้สรุปประเภทธุรกิจต้องห้ามสำหรับการพิจารณาสินเชื่อ Micro SME "
    "พร้อมเหตุผลและข้อควรระวังของเจ้าหน้าที่ / The table below summarises business "
    "types that are prohibited for Micro SME credit assessment, with the reason and "
    "the officer's cautions for each.",
    new_x="LMARGIN", new_y="NEXT")
pdf.ln(2)

pdf.set_font("ayuthaya", "", 8)
with pdf.table(col_widths=(8, 24, 36, 32), text_align="LEFT",
               line_height=5, first_row_as_headings=True,
               headings_style=FontFace(emphasis="")) as table:
    table.row(HEADER)
    for r in ROWS:
        table.row(r)

pdf.output(OUT)

# Report the size of the table content so we can reason about chunk splitting.
table_chars = sum(len(c) for r in ROWS for c in r) + sum(len(h) for h in HEADER)
print(f"wrote {OUT}")
print(f"table cell chars (approx markdown size): {table_chars}")
print(f"default max_chunk_size=512 -> would split into ~{max(1, table_chars // 512)} fragments")
