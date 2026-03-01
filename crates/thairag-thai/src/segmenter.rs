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
        self.inner
            .segment(text, true, false)
            .unwrap_or_else(|_| text.split_whitespace().map(String::from).collect())
    }
}

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
}
