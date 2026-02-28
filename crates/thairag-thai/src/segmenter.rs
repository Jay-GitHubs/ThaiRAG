use thairag_core::traits::ThaiTokenizer;

/// Dictionary-based Thai word segmenter.
/// Current implementation: whitespace split (stub).
pub struct DictionarySegmenter;

impl DictionarySegmenter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DictionarySegmenter {
    fn default() -> Self {
        Self::new()
    }
}

impl ThaiTokenizer for DictionarySegmenter {
    fn tokenize(&self, text: &str) -> Vec<String> {
        // Stub: whitespace-based split. Replace with dictionary-based segmentation.
        text.split_whitespace().map(String::from).collect()
    }
}
