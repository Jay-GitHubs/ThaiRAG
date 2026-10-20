use std::sync::Arc;

use nlpo3::tokenizer::newmm::NewmmTokenizer;
use nlpo3::tokenizer::tokenizer_trait::Tokenizer as Nlpo3Tokenizer;
use thairag_core::traits::ThaiTokenizer;

/// Embedded PyThaiNLP dictionary (~62K Thai words).
const DICT_TEXT: &str = include_str!("../data/words_th.txt");

/// Dictionary-based Thai word segmenter using nlpo3's NEWMM algorithm.
pub struct DictionarySegmenter {
    inner: Arc<NewmmTokenizer>,
}

impl DictionarySegmenter {
    pub fn new() -> Self {
        let words: Vec<String> = DICT_TEXT
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        let tokenizer = NewmmTokenizer::from_word_list(words);
        Self {
            inner: Arc::new(tokenizer),
        }
    }

    /// Get a reference-counted handle to share with other components.
    pub fn shared(&self) -> Arc<NewmmTokenizer> {
        Arc::clone(&self.inner)
    }
}

impl Default for DictionarySegmenter {
    fn default() -> Self {
        Self::new()
    }
}

impl ThaiTokenizer for DictionarySegmenter {
    fn tokenize(&self, text: &str) -> Vec<String> {
        // `safe = false`, deliberately. nlpo3 1.4's "safe mode" (windowed
        // cutting for inputs of 140+ chars) returns a token stream whose
        // concatenation has the SAME LENGTH as the input but is not the same
        // text: earlier words are repeated and later words dropped. A 795-char
        // OCR line came back as 18 copies of its first 20–120 chars. Every
        // Thai line longer than ~140 chars — any OCR'd page, most paragraphs —
        // was garbled on its way into chunks (the clause splitter rebuilds
        // text from these tokens) and into the BM25 index. Plain one-cut
        // segmentation is exact; nlpo3's own MAX_GRAPH_SIZE guard already
        // bounds the lattice on long unspaced runs.
        self.inner
            .segment(text, false, false)
            .unwrap_or_else(|_| text.split_whitespace().map(String::from).collect())
    }
}

/// A real OCR'd line from a scanned Thai loan-program page (795 chars). Long
/// enough to trip nlpo3's safe-mode windowing (threshold 140 chars).
#[cfg(test)]
pub(crate) const LONG_OCR_LINE: &str = "7.ระเบียบการยืมหนังสือ สมาชิกต้องแสดงบัตรสมาชิกทุกครั้งที่ยืมหนังสือ และยืมได้ครั้งละไม่เกิน 5 เล่ม เป็นเวลา 14 วัน หากคืนล่าช้าจะมีค่าปรับวันละ 5 บาทต่อเล่ม สมาชิกที่ค้างค่าปรับเกิน 100 บาทจะถูกระงับสิทธิ์การยืมจนกว่าจะชำระครบถ้วน หนังสืออ้างอิง วารสารฉบับล่าสุด และวิทยานิพนธ์ไม่อนุญาตให้ยืมออกนอกห้องสมุด แต่สามารถถ่ายสำเนาได้ตามระเบียบว่าด้วยลิขสิทธิ์ 8. การจองหนังสือ สมาชิกสามารถจองหนังสือที่ถูกยืมอยู่ได้ผ่านระบบออนไลน์ เมื่อหนังสือถูกส่งคืน ระบบจะแจ้งเตือนทางอีเมล และเก็บหนังสือไว้ให้ 3 วัน 9. การต่ออายุการยืม ต่ออายุได้ไม่เกิน 2 ครั้ง หากไม่มีผู้จองคิวถัดไป 10. ห้องค้นคว้ากลุ่ม จองล่วงหน้าได้ไม่เกิน 7 วัน ใช้ได้ครั้งละ 3 ชั่วโมง สำหรับกลุ่มตั้งแต่ 4 ถึง 10 คน เกณฑ์การสมัครสมาชิก ผู้สมัครต้องเป็นนักศึกษาหรือบุคลากรของมหาวิทยาลัย มีอายุไม่ต่ำกว่า 18 ปี และยื่นเอกสารประกอบให้ครบถ้วน ณ วันสมัคร";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_thai_sentence() {
        let seg = DictionarySegmenter::new();
        // Use sentence where "ห้องสมุด" is NOT followed by a word that forms a
        // longer dictionary compound (e.g. "ห้องสมุดประชาชน").
        let tokens = seg.tokenize("ห้องสมุดเปิดให้บริการทุกวัน");
        assert!(
            tokens.contains(&"ห้องสมุด".to_string()),
            "Expected 'ห้องสมุด' in tokens: {tokens:?}"
        );
        assert!(
            tokens.contains(&"เปิด".to_string()),
            "Expected 'เปิด' in tokens: {tokens:?}"
        );
    }

    #[test]
    fn segment_compound_word() {
        let seg = DictionarySegmenter::new();
        // "ห้องสมุดประชาชน" is a compound word in the dictionary — NEWMM picks it.
        let tokens = seg.tokenize("ห้องสมุดประชาชน");
        assert!(
            tokens.contains(&"ห้องสมุดประชาชน".to_string()),
            "Expected compound word in tokens: {tokens:?}"
        );
    }

    #[test]
    fn segment_english_passthrough() {
        let seg = DictionarySegmenter::new();
        let tokens = seg.tokenize("hello world");
        assert_eq!(tokens, vec!["hello", " ", "world"]);
    }

    #[test]
    fn segment_empty() {
        let seg = DictionarySegmenter::new();
        let tokens = seg.tokenize("");
        assert!(tokens.is_empty() || tokens == vec![""]);
    }

    /// Regression: with nlpo3 safe mode the tokens of this 795-char line
    /// concatenated to 795 chars of the WRONG text (18 copies of the first
    /// 20–120 chars, the tail gone). Segmentation must be lossless.
    #[test]
    fn long_line_tokens_reconstruct_input_exactly() {
        let seg = DictionarySegmenter::new();
        let tokens = seg.tokenize(LONG_OCR_LINE);
        assert_eq!(tokens.concat(), LONG_OCR_LINE);
        assert_eq!(
            LONG_OCR_LINE.matches("7.ระเบียบ").count(),
            1,
            "fixture sanity: the line head occurs once"
        );
        assert!(
            tokens.iter().any(|t| t == "กู้"),
            "the tail of the line must still be segmented: {tokens:?}"
        );
    }

    /// Same guarantee for a long run with no whitespace at all, the shape of
    /// a Thai paragraph as the BM25 tokenizer sees it (script runs split only
    /// at non-Thai characters).
    #[test]
    fn long_unspaced_run_tokens_reconstruct_input_exactly() {
        let seg = DictionarySegmenter::new();
        let unspaced: String = LONG_OCR_LINE
            .chars()
            .filter(|c| ('\u{0E00}'..='\u{0E7F}').contains(c))
            .collect();
        assert!(unspaced.chars().count() > 400, "fixture sanity");
        let tokens = seg.tokenize(&unspaced);
        assert_eq!(tokens.concat(), unspaced);
    }
}
