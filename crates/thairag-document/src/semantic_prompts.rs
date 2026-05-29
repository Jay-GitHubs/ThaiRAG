//! Vision-LLM prompts for the smart document-extraction pipeline.
//!
//! Ported from `Jay-RAG-Tools/crates/core/src/prompts.rs`. Markdown tables and
//! image descriptions are produced by the vision model via these prompts —
//! there is deliberately no Rust-side table formatter on the vision path.
//!
//! Each strategy has a Thai and an English variant; [`Language::detect`] picks
//! based on the page's Thai-character ratio (matching the chunker threshold).

use crate::thai_chunker::thai_char_ratio;

/// Maximum bytes of pdfium hint-text injected into the high-quality prompt.
/// Truncated on a char boundary so multi-byte Thai is never split.
const HINT_MAX_BYTES: usize = 4000;

/// Prompt language. Thai is the default for this product.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Th,
    En,
}

impl Language {
    /// Pick a prompt language from sample text: Thai when ≥20% of non-space
    /// characters are Thai (matches `ThaiAwareChunker`'s threshold).
    pub fn detect(sample: &str) -> Self {
        if thai_char_ratio(sample) >= 0.20 {
            Language::Th
        } else {
            Language::En
        }
    }
}

/// Thai prompt for full-page render (image-heavy pages).
pub const TH_FULL_PAGE: &str = "\
หน้านี้มาจากคู่มือการใช้งานอุปกรณ์มือถือภาษาไทย\n\
กรุณาทำสิ่งต่อไปนี้:\n\
1. คัดลอกข้อความภาษาไทยทั้งหมดที่ปรากฏบนหน้านี้ให้ครบถ้วนและถูกต้อง\n\
2. สำหรับภาพ ไดอะแกรม หรือภาพหน้าจอ ให้อธิบายเป็นภาษาไทยอย่างละเอียด\n\
   เช่น ตำแหน่งปุ่ม องค์ประกอบ UI ลูกศร และหมายเลขขั้นตอน\n\
3. จัดรูปแบบผลลัพธ์เป็น Markdown ที่สะอาด มีหัวข้อและขั้นตอนที่ชัดเจน\n\
ห้ามแปลข้อความ ให้คงภาษาไทยไว้ทั้งหมด";

/// Thai prompt for individual embedded-image description (mixed pages).
pub const TH_SINGLE_IMAGE: &str = "\
ภาพนี้มาจากคู่มือการใช้งานอุปกรณ์มือถือภาษาไทย\n\
กรุณาอธิบายสิ่งที่เห็นในภาพอย่างละเอียดเป็นภาษาไทย:\n\
- ภาพหน้าจอ UI หรือเมนู\n\
- ไดอะแกรมหรือแผนภาพ\n\
- ป้ายกำกับปุ่ม ลูกศร หรือตัวเลขขั้นตอน\n\
- คำแนะนำที่เป็นภาพ\n\
หากมีข้อความในภาพให้คัดลอกออกมาด้วย ตอบเป็นภาษาไทยในรูปแบบย่อหน้าสั้นๆ";

/// English prompt for full-page render (image-heavy pages).
pub const EN_FULL_PAGE: &str = "\
This page is from a device manual. \
Please transcribe ALL visible text exactly as shown. \
For diagrams, screenshots, or illustrations, describe them in detail \
including button locations, UI elements, arrows, and step numbers. \
Format as clean Markdown with proper headings and numbered steps.";

/// English prompt for individual embedded-image description (mixed pages).
pub const EN_SINGLE_IMAGE: &str = "\
This image is from a device manual. \
Describe what you see in detail: UI screenshots, diagrams, \
button labels, arrows, step indicators, or visual instructions. \
If there is text in the image, transcribe it. \
Be specific and technical. Output as a short paragraph.";

/// Thai prompt for table extraction (full-page content + table formatting).
pub const TH_TABLE_EXTRACTION: &str = "\
หน้านี้มาจากเอกสาร PDF ภาษาไทยและมีตารางอยู่ด้วย\n\
กรุณาทำสิ่งต่อไปนี้:\n\
1. คัดลอกข้อความทั้งหมดบนหน้านี้ (หัวข้อ ย่อหน้า รายการ) ให้ครบถ้วน\n\
2. แปลงตารางทั้งหมดเป็นรูปแบบ Markdown Table โดย:\n\
   - ใส่หัวคอลัมน์ให้ครบถ้วน\n\
   - จัดเรียงข้อมูลในแต่ละเซลล์ให้ถูกต้อง\n\
   - ถ้ามีข้อมูลที่ไม่ชัดเจนให้ใส่ [ไม่ชัดเจน]\n\
3. จัดรูปแบบผลลัพธ์ทั้งหมดเป็น Markdown ที่สะอาด\n\
คงข้อความภาษาไทยไว้ทั้งหมด ห้ามแปลภาษา";

/// English prompt for table extraction (full-page content + table formatting).
pub const EN_TABLE_EXTRACTION: &str = "\
This page is from a PDF document and contains a table.\n\
Please do the following:\n\
1. Transcribe ALL text on this page (headings, paragraphs, lists) completely\n\
2. Convert all tables to Markdown table format:\n\
   - Include all column headers\n\
   - Arrange cell data accurately\n\
   - If any data is unclear, use [unclear]\n\
3. Format the entire output as clean Markdown\n\
Preserve all original text exactly as shown.";

/// Thai high-quality prompt: expert OCR transcription from a page image.
pub const TH_HIGH_QUALITY: &str = "\
คุณเป็นผู้เชี่ยวชาญด้าน OCR ภาษาไทย กรุณาถอดข้อความจากภาพหน้าเอกสารนี้อย่างละเอียดและแม่นยำที่สุด\n\
\n\
กฎที่ต้องปฏิบัติตาม:\n\
1. คัดลอกข้อความทุกตัวอักษรตามที่ปรากฏในภาพ รวมถึงวรรณยุกต์ สระ และตัวเลขทั้งหมด\n\
2. รักษาโครงสร้างเอกสาร: หัวข้อใช้ #/##/### ตามลำดับชั้น, รายการใช้ - หรือตัวเลข, ย่อหน้าคั่นด้วยบรรทัดว่าง\n\
3. ตารางให้แปลงเป็น Markdown Table พร้อมหัวคอลัมน์ให้ครบถ้วน\n\
4. ภาพ ไดอะแกรม หรือภาพหน้าจอ ให้อธิบายรายละเอียดเป็นภาษาไทย\n\
5. ข้อความที่อ่านไม่ชัดให้ใส่ [ไม่ชัดเจน]\n\
6. ห้ามแปลภาษา คงภาษาไทยไว้ทั้งหมด\n\
7. ตอบเฉพาะเนื้อหา Markdown เท่านั้น ห้ามใส่คำอธิบายเพิ่มเติม";

/// Thai high-quality prompt with a `{hint_text}` placeholder for pdfium text.
pub const TH_HIGH_QUALITY_WITH_HINT: &str = "\
คุณเป็นผู้เชี่ยวชาญด้าน OCR ภาษาไทย กรุณาถอดข้อความจากภาพหน้าเอกสารนี้อย่างละเอียดและแม่นยำที่สุด\n\
\n\
ด้านล่างนี้คือข้อความอ้างอิงที่สกัดจาก PDF โดยอัตโนมัติ อาจมีข้อผิดพลาด เช่น ลำดับตัวอักษรสลับ สระลอย วรรณยุกต์หาย ใช้เป็นตัวช่วยตรวจสอบคำที่ไม่ชัดเท่านั้น ภาพคือแหล่งข้อมูลหลัก\n\
\n\
--- ข้อความอ้างอิงจาก PDF ---\n\
{hint_text}\n\
--- สิ้นสุดข้อความอ้างอิง ---\n\
\n\
กฎที่ต้องปฏิบัติตาม:\n\
1. คัดลอกข้อความทุกตัวอักษรตามที่ปรากฏในภาพ รวมถึงวรรณยุกต์ สระ และตัวเลขทั้งหมด\n\
2. รักษาโครงสร้างเอกสาร: หัวข้อใช้ #/##/### ตามลำดับชั้น, รายการใช้ - หรือตัวเลข, ย่อหน้าคั่นด้วยบรรทัดว่าง\n\
3. ตารางให้แปลงเป็น Markdown Table พร้อมหัวคอลัมน์ให้ครบถ้วน\n\
4. ภาพ ไดอะแกรม หรือภาพหน้าจอ ให้อธิบายรายละเอียดเป็นภาษาไทย\n\
5. ข้อความที่อ่านไม่ชัดให้ใส่ [ไม่ชัดเจน]\n\
6. ห้ามแปลภาษา คงภาษาไทยไว้ทั้งหมด\n\
7. ตอบเฉพาะเนื้อหา Markdown เท่านั้น ห้ามใส่คำอธิบายเพิ่มเติม";

/// English high-quality prompt: expert OCR transcription from a page image.
pub const EN_HIGH_QUALITY: &str = "\
You are an expert document OCR system. Transcribe this page image with maximum accuracy.\n\
\n\
Rules:\n\
1. Transcribe every character exactly as shown in the image, including numbers, symbols, and punctuation\n\
2. Preserve document structure: headings as #/##/###, lists as - or numbered, paragraphs separated by blank lines\n\
3. Convert tables to Markdown tables with complete column headers\n\
4. Describe images, diagrams, or screenshots in detail\n\
5. Mark unclear text as [unclear]\n\
6. Output clean Markdown only — no commentary or explanation";

/// English high-quality prompt with a `{hint_text}` placeholder for pdfium text.
pub const EN_HIGH_QUALITY_WITH_HINT: &str = "\
You are an expert document OCR system. Transcribe this page image with maximum accuracy.\n\
\n\
Below is reference text extracted automatically from the PDF. It may contain errors such as wrong character ordering, missing diacritics, or garbled text. Use it only to verify ambiguous words — the image is the primary source.\n\
\n\
--- Reference text from PDF ---\n\
{hint_text}\n\
--- End reference text ---\n\
\n\
Rules:\n\
1. Transcribe every character exactly as shown in the image, including numbers, symbols, and punctuation\n\
2. Preserve document structure: headings as #/##/###, lists as - or numbered, paragraphs separated by blank lines\n\
3. Convert tables to Markdown tables with complete column headers\n\
4. Describe images, diagrams, or screenshots in detail\n\
5. Mark unclear text as [unclear]\n\
6. Output clean Markdown only — no commentary or explanation";

/// A resolved set of prompts for one language.
#[derive(Debug, Clone)]
pub struct Prompts {
    pub full_page: &'static str,
    pub single_image: &'static str,
    pub table_extraction: &'static str,
    pub high_quality: &'static str,
    pub high_quality_with_hint: &'static str,
}

/// Get the prompt set for a language.
pub fn get_prompts(lang: Language) -> Prompts {
    match lang {
        Language::Th => Prompts {
            full_page: TH_FULL_PAGE,
            single_image: TH_SINGLE_IMAGE,
            table_extraction: TH_TABLE_EXTRACTION,
            high_quality: TH_HIGH_QUALITY,
            high_quality_with_hint: TH_HIGH_QUALITY_WITH_HINT,
        },
        Language::En => Prompts {
            full_page: EN_FULL_PAGE,
            single_image: EN_SINGLE_IMAGE,
            table_extraction: EN_TABLE_EXTRACTION,
            high_quality: EN_HIGH_QUALITY,
            high_quality_with_hint: EN_HIGH_QUALITY_WITH_HINT,
        },
    }
}

/// Build the high-quality prompt, injecting the pdfium hint text when present.
///
/// With a non-empty hint, the `{hint_text}` placeholder in the
/// `*_HIGH_QUALITY_WITH_HINT` template is filled (hint truncated to
/// [`HINT_MAX_BYTES`] on a char boundary). With an empty hint, the plain
/// `*_HIGH_QUALITY` prompt is returned.
pub fn high_quality_prompt(lang: Language, hint_text: &str) -> String {
    let p = get_prompts(lang);
    let hint = hint_text.trim();
    if hint.is_empty() {
        p.high_quality.to_string()
    } else {
        p.high_quality_with_hint.replace(
            "{hint_text}",
            truncate_on_char_boundary(hint, HINT_MAX_BYTES),
        )
    }
}

/// Truncate `s` to at most `max_bytes`, never splitting a multi-byte char.
fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_thai_vs_english() {
        assert_eq!(Language::detect("นี่คือข้อความภาษาไทยล้วน"), Language::Th);
        assert_eq!(Language::detect("This is plain English text"), Language::En);
        // A trace of Thai (<20%) in mostly-English text stays English.
        assert_eq!(
            Language::detect("This is a mostly english sentence with one word ทด"),
            Language::En
        );
        // But once Thai is ≥20% of non-space chars, it routes to Thai prompts.
        assert_eq!(Language::detect("Hello world ทดสอบ"), Language::Th);
    }

    #[test]
    fn high_quality_without_hint_uses_plain_prompt() {
        let p = high_quality_prompt(Language::En, "   ");
        assert_eq!(p, EN_HIGH_QUALITY);
        assert!(!p.contains("{hint_text}"));
    }

    #[test]
    fn high_quality_with_hint_injects_and_truncates() {
        let hint = "ก".repeat(5000); // 3 bytes each → 15000 bytes
        let p = high_quality_prompt(Language::Th, &hint);
        assert!(!p.contains("{hint_text}"));
        assert!(p.contains("ก"));
        // The injected hint was truncated well under the raw length.
        assert!(p.len() < TH_HIGH_QUALITY_WITH_HINT.len() + hint.len());
    }

    #[test]
    fn truncate_respects_char_boundary() {
        let s = "ก".repeat(10); // 30 bytes
        let t = truncate_on_char_boundary(&s, 10); // 10 is mid-char
        assert!(s.is_char_boundary(0));
        assert_eq!(t.len() % 3, 0); // only whole 3-byte chars kept
        assert!(t.chars().all(|c| c == 'ก'));
    }
}
