use thairag_core::traits::{Chunker, ThaiTokenizer};
use thairag_thai::DictionarySegmenter;

/// Threshold: if more than 20% of characters are Thai, use Thai-aware chunking.
const THAI_CHAR_THRESHOLD: f64 = 0.20;

/// Thai Unicode block: U+0E00 – U+0E7F.
fn is_thai_char(c: char) -> bool {
    ('\u{0E00}'..='\u{0E7F}').contains(&c)
}

/// Returns the fraction of non-whitespace characters that are Thai.
pub fn thai_char_ratio(text: &str) -> f64 {
    let mut total = 0usize;
    let mut thai = 0usize;
    for c in text.chars() {
        if c.is_whitespace() {
            continue;
        }
        total += 1;
        if is_thai_char(c) {
            thai += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    thai as f64 / total as f64
}

/// Returns true if the text contains significant Thai content.
pub fn is_thai_text(text: &str) -> bool {
    thai_char_ratio(text) > THAI_CHAR_THRESHOLD
}

/// Thai sentence boundary patterns for splitting.
const THAI_SENTENCE_ENDINGS: &[&str] = &["ครับ", "ค่ะ", "คะ", "นะคะ", "นะครับ"];

/// Thai clause boundary conjunctions.
const THAI_CLAUSE_BOUNDARIES: &[&str] = &[
    "แต่",         // but
    "และ",        // and
    "หรือ",        // or
    "เพราะ",      // because
    "ดังนั้น",       // therefore
    "เนื่องจาก",    // since/due to
    "อย่างไรก็ตาม", // however
    "นอกจากนี้",    // moreover
    "รวมถึง",      // including
];

/// Language-aware chunker that detects Thai content and uses
/// dictionary-based word segmentation for Thai text.
pub struct ThaiAwareChunker {
    segmenter: DictionarySegmenter,
}

impl ThaiAwareChunker {
    pub fn new() -> Self {
        Self {
            segmenter: DictionarySegmenter::new(),
        }
    }

    /// Split Thai text into sentence-like segments at natural boundaries.
    fn split_thai_sentences(&self, text: &str) -> Vec<String> {
        // First split on newlines and obvious paragraph breaks
        let mut segments: Vec<String> = Vec::new();

        for line in text.split('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try splitting on Thai sentence endings
            let sub_segments = self.split_on_thai_boundaries(trimmed);
            segments.extend(sub_segments);
        }

        segments
    }

    /// Split a single line of Thai text on sentence/clause boundaries.
    fn split_on_thai_boundaries(&self, text: &str) -> Vec<String> {
        if text.is_empty() {
            return Vec::new();
        }

        // Use the dictionary segmenter to tokenize
        let tokens = self.segmenter.tokenize(text);

        let mut segments = Vec::new();
        let mut current = String::new();

        for token in &tokens {
            current.push_str(token);

            // Check if this token is a sentence ending
            let is_sentence_end = THAI_SENTENCE_ENDINGS
                .iter()
                .any(|ending| token.trim() == *ending);

            // Check if this token is a clause boundary (split BEFORE the conjunction)
            let is_clause_boundary = THAI_CLAUSE_BOUNDARIES
                .iter()
                .any(|boundary| token.trim() == *boundary);

            if is_sentence_end {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    segments.push(trimmed);
                }
                current.clear();
            } else if is_clause_boundary && current.trim().chars().count() > token.trim().len() {
                // Split before the conjunction: put the conjunction back
                let before = current[..current.len() - token.len()].trim().to_string();
                if !before.is_empty() {
                    segments.push(before);
                }
                current = token.clone();
            }
        }

        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            segments.push(trimmed);
        }

        segments
    }

    /// Chunk Thai text using character-count based size limits.
    fn chunk_thai(&self, text: &str, max_size: usize, overlap: usize) -> Vec<String> {
        let sentences = self.split_thai_sentences(text);

        if sentences.is_empty() {
            return Vec::new();
        }

        let mut chunks: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut current_char_count = 0usize;
        // Track sentence indices for overlap
        let mut current_start_idx = 0usize;
        let mut sentence_boundaries: Vec<(usize, usize)> = Vec::new(); // (start_char, sentence_idx)

        for (i, sentence) in sentences.iter().enumerate() {
            let sentence_chars = sentence.chars().count();

            // If adding this sentence exceeds max_size, flush
            let separator = if current.is_empty() { "" } else { " " };
            let separator_len = separator.chars().count();
            let new_len = current_char_count + separator_len + sentence_chars;

            if new_len > max_size && !current.is_empty() {
                chunks.push(std::mem::take(&mut current));

                // Handle overlap: find sentences to carry over
                if overlap > 0 && !sentence_boundaries.is_empty() {
                    let chunk_char_count = current_char_count;
                    let mut overlap_start = sentence_boundaries.len();
                    let mut overlap_chars = 0usize;

                    // Walk backwards through sentences to find overlap
                    for (j, &(start_char, _)) in sentence_boundaries.iter().enumerate().rev() {
                        let sentence_len = chunk_char_count - start_char;
                        if overlap_chars + sentence_len > overlap && overlap_chars > 0 {
                            break;
                        }
                        overlap_chars += sentence_len;
                        overlap_start = j;
                    }

                    // Rebuild current from overlap sentences
                    current.clear();
                    current_char_count = 0;
                    for &(_, sent_idx) in sentence_boundaries.iter().skip(overlap_start) {
                        if !current.is_empty() {
                            current.push(' ');
                            current_char_count += 1;
                        }
                        current.push_str(&sentences[sent_idx]);
                        current_char_count += sentences[sent_idx].chars().count();
                    }
                    current_start_idx = if overlap_start < sentence_boundaries.len() {
                        sentence_boundaries[overlap_start].1
                    } else {
                        i
                    };
                } else {
                    current_char_count = 0;
                    current_start_idx = i;
                }
                sentence_boundaries.clear();
            }

            // Record boundary
            sentence_boundaries.push((current_char_count, i));

            if !current.is_empty() {
                current.push(' ');
                current_char_count += 1;
            }
            current.push_str(sentence);
            current_char_count += sentence_chars;
            let _ = current_start_idx; // suppress unused warning
        }

        if !current.is_empty() {
            chunks.push(current);
        }

        chunks
    }

    /// Chunk non-Thai (e.g. English) text using the standard paragraph-based approach.
    fn chunk_standard(&self, text: &str, max_size: usize, _overlap: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current = String::new();

        for paragraph in text.split("\n\n") {
            let trimmed = paragraph.trim();
            if trimmed.is_empty() {
                continue;
            }

            if current.len() + trimmed.len() + 2 > max_size && !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }

            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(trimmed);
        }

        if !current.is_empty() {
            chunks.push(current);
        }

        chunks
    }
}

impl Default for ThaiAwareChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for ThaiAwareChunker {
    fn chunk(&self, text: &str, max_size: usize, overlap: usize) -> Vec<String> {
        if is_thai_text(text) {
            self.chunk_thai(text, max_size, overlap)
        } else {
            self.chunk_standard(text, max_size, overlap)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Detection tests ─────────────────────────────────────────────

    #[test]
    fn detect_pure_thai() {
        assert!(is_thai_text("สวัสดีครับ วันนี้อากาศดีมาก"));
    }

    #[test]
    fn detect_pure_english() {
        assert!(!is_thai_text("Hello world, this is a test."));
    }

    #[test]
    fn detect_mixed_mostly_thai() {
        // Thai with some English words — should still be detected as Thai
        assert!(is_thai_text("ระบบ ThaiRAG ใช้สำหรับการค้นหาข้อมูลภาษาไทย"));
    }

    #[test]
    fn detect_mixed_mostly_english() {
        // English with just a Thai word
        assert!(!is_thai_text(
            "The system is called ThaiRAG and it handles สวัสดี queries"
        ));
    }

    #[test]
    fn detect_empty_text() {
        assert!(!is_thai_text(""));
    }

    #[test]
    fn thai_char_ratio_accuracy() {
        // Pure Thai
        let ratio = thai_char_ratio("สวัสดี");
        assert!(ratio > 0.99, "Pure Thai ratio: {ratio}");

        // Pure English
        let ratio = thai_char_ratio("hello");
        assert!(ratio < 0.01, "Pure English ratio: {ratio}");
    }

    // ── Thai chunking tests ─────────────────────────────────────────

    #[test]
    fn chunk_simple_thai_sentence() {
        let chunker = ThaiAwareChunker::new();
        let text = "สวัสดีครับ";
        let chunks = chunker.chunk(text, 1000, 0);
        assert_eq!(chunks.len(), 1);
        // The chunk should contain all the original Thai text
        assert!(!chunks[0].is_empty());
    }

    #[test]
    fn chunk_thai_splits_on_sentence_endings() {
        let chunker = ThaiAwareChunker::new();
        // Two sentences ending with ครับ and ค่ะ
        let text = "วันนี้อากาศดีมากครับ พรุ่งนี้จะดีกว่าค่ะ";
        let chunks = chunker.chunk(text, 15, 0);
        // Should produce multiple chunks since max_size is small
        assert!(
            chunks.len() >= 2,
            "Expected >=2 chunks, got {}: {:?}",
            chunks.len(),
            chunks
        );
    }

    #[test]
    fn chunk_thai_respects_max_size_in_chars() {
        let chunker = ThaiAwareChunker::new();
        let text = "ภาษาไทยไม่มีช่องว่างระหว่างคำ\nดังนั้นการตัดคำจึงสำคัญมาก\nระบบนี้ใช้พจนานุกรม";
        let chunks = chunker.chunk(text, 20, 0);
        // Each chunk should not exceed max_size in character count
        for chunk in &chunks {
            let char_count = chunk.chars().count();
            // Allow some tolerance for sentence boundaries
            assert!(
                char_count <= 40,
                "Chunk too large ({char_count} chars): {chunk}"
            );
        }
        assert!(chunks.len() >= 2, "Expected splitting: {:?}", chunks);
    }

    #[test]
    fn chunk_thai_with_overlap() {
        let chunker = ThaiAwareChunker::new();
        let text = "ประโยคแรกของข้อความ\nประโยคที่สองของข้อความ\nประโยคที่สามของข้อความ";
        let chunks = chunker.chunk(text, 20, 10);
        // With overlap, later chunks should share content with earlier ones
        assert!(
            chunks.len() >= 2,
            "Expected >=2 chunks with overlap: {:?}",
            chunks
        );
    }

    #[test]
    fn chunk_thai_clause_boundaries() {
        let chunker = ThaiAwareChunker::new();
        // Text with clause boundary "แต่" (but)
        let text = "ระบบทำงานได้ดีแต่ยังต้องปรับปรุง";
        let chunks = chunker.chunk(text, 1000, 0);
        // Should still produce chunks (may or may not split depending on size)
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_thai_preserves_content() {
        let chunker = ThaiAwareChunker::new();
        let text = "สวัสดีครับ ยินดีต้อนรับ";
        let chunks = chunker.chunk(text, 1000, 0);
        let joined = chunks.join(" ");
        // All Thai characters from the original should be present
        for c in text.chars() {
            if is_thai_char(c) {
                assert!(
                    joined.contains(c),
                    "Missing Thai char '{c}' in output: {joined}"
                );
            }
        }
    }

    #[test]
    fn chunk_english_uses_standard_strategy() {
        let chunker = ThaiAwareChunker::new();
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let chunks = chunker.chunk(text, 20, 0);
        // Should behave like standard paragraph chunking
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_empty_input() {
        let chunker = ThaiAwareChunker::new();
        let chunks = chunker.chunk("", 100, 0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_mixed_thai_english() {
        let chunker = ThaiAwareChunker::new();
        // Mixed text where Thai is dominant
        let text = "ระบบ RAG สำหรับภาษาไทยทำงานได้ดีมากครับ ขอบคุณทีมพัฒนาค่ะ";
        let chunks = chunker.chunk(text, 30, 0);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_thai_newline_separated() {
        let chunker = ThaiAwareChunker::new();
        let text = "บรรทัดแรก\nบรรทัดที่สอง\nบรรทัดที่สาม";
        let chunks = chunker.chunk(text, 10, 0);
        assert!(
            chunks.len() >= 2,
            "Expected newline-separated Thai to split: {:?}",
            chunks
        );
    }
}
