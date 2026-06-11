use std::sync::Arc;

use nlpo3::tokenizer::newmm::NewmmTokenizer;
use nlpo3::tokenizer::tokenizer_trait::Tokenizer as Nlpo3Tokenizer;
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

/// Returns true if the character is in the Thai Unicode block (U+0E00..U+0E7F).
fn is_thai(c: char) -> bool {
    ('\u{0E00}'..='\u{0E7F}').contains(&c)
}

/// Thai stopwords filtered out of the BM25 token stream. The tokenizer is
/// registered on the index field, so the SAME filtering applies at index and
/// query time — consistency is structural, not by convention.
///
/// Deliberately conservative: pure function words only (relativizers,
/// prepositions, conjunctions, aspect particles, politeness particles).
/// NOT included, on purpose:
/// - "ไม่" (negation — reverses meaning),
/// - verbs like "มี/เป็น/คือ/ทำ/อยู่" (content-bearing in factual queries),
/// - demonstratives "นี้/นั้น",
/// - nominalizer prefixes "การ/ความ" (nlpo3 keeps real compounds like
///   "การเงิน" as one token; a standalone token is rare enough that IDF
///   handles it).
const THAI_STOPWORDS: &[&str] = &[
    "ที่",
    "ซึ่ง",
    "ของ",
    "ใน",
    "กับ",
    "แก่",
    "ต่อ",
    "โดย",
    "ด้วย",
    "จาก",
    "ถึง",
    "เพื่อ",
    "และ",
    "หรือ",
    "แต่",
    "ก็",
    "จึง",
    "เมื่อ",
    "ว่า",
    "ให้",
    "จะ",
    "ได้",
    "แล้ว",
    "ณ",
    "ๆ",
    "นะ",
    "ครับ",
    "ค่ะ",
    "คะ",
];

fn is_thai_stopword(word: &str) -> bool {
    THAI_STOPWORDS.contains(&word)
}

/// Normalize a Thai token's text for matching. Tone marks are NOT stripped —
/// they are phonemic in Thai (มา/ม้า differ by tone mark alone). The only safe
/// normalization is unifying the decomposed SARA AM sequence
/// (NIKHAHIT U+0E4D + SARA AA U+0E32) with the precomposed SARA AM (U+0E33),
/// which PDF extractors emit inconsistently.
fn normalize_thai_token(word: &str) -> String {
    if word.contains('\u{0E4D}') {
        word.replace("\u{0E4D}\u{0E32}", "\u{0E33}")
    } else {
        word.to_string()
    }
}

/// A Tantivy tokenizer that uses nlpo3 for Thai text and simple
/// whitespace/punctuation splitting for non-Thai text.
#[derive(Clone)]
pub struct ThaiTantivyTokenizer {
    segmenter: Arc<NewmmTokenizer>,
}

impl ThaiTantivyTokenizer {
    pub fn new(segmenter: Arc<NewmmTokenizer>) -> Self {
        Self { segmenter }
    }
}

impl Tokenizer for ThaiTantivyTokenizer {
    type TokenStream<'a> = ThaiTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = tokenize_mixed(text, &self.segmenter);
        ThaiTokenStream {
            tokens,
            index: usize::MAX, // before first advance()
        }
    }
}

/// Eager token stream backed by a pre-computed Vec<Token>.
pub struct ThaiTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for ThaiTokenStream {
    fn advance(&mut self) -> bool {
        if self.index == usize::MAX {
            self.index = 0;
        } else {
            self.index += 1;
        }
        self.index < self.tokens.len()
    }

    fn token(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index]
    }
}

/// Segment mixed Thai/non-Thai text into Tantivy tokens with correct byte offsets.
fn tokenize_mixed(text: &str, segmenter: &NewmmTokenizer) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut position: usize = 0;

    // Split text into contiguous runs of Thai vs non-Thai characters.
    let mut chars = text.char_indices().peekable();

    while let Some(&(byte_start, c)) = chars.peek() {
        let thai_run = is_thai(c);

        // Consume all chars belonging to this script run.
        let mut byte_end = byte_start;
        while let Some(&(bi, ch)) = chars.peek() {
            if is_thai(ch) == thai_run {
                byte_end = bi + ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }

        let run = &text[byte_start..byte_end];

        if thai_run {
            // Segment Thai run using nlpo3.
            let words = segmenter
                .segment(run, true, false)
                .unwrap_or_else(|_| vec![run.to_string()]);

            let mut offset = byte_start;
            for word in &words {
                let trimmed = word.trim();
                if trimmed.is_empty() {
                    offset += word.len();
                    continue;
                }
                // Find the actual position of this word in the run.
                // nlpo3 returns tokens in order matching the original text.
                let word_byte_start = offset;
                let word_byte_end = offset + word.len();
                offset = word_byte_end;
                if is_thai_stopword(trimmed) {
                    // Keep the position gap so phrase distances stay honest.
                    position += 1;
                    continue;
                }
                tokens.push(Token {
                    offset_from: word_byte_start,
                    offset_to: word_byte_end,
                    position,
                    text: normalize_thai_token(trimmed),
                    position_length: 1,
                });
                position += 1;
            }
        } else {
            // Non-Thai: split on whitespace and punctuation.
            for word in split_non_thai(run) {
                let word_offset = byte_start + word.0;
                tokens.push(Token {
                    offset_from: word_offset,
                    offset_to: word_offset + word.1.len(),
                    position,
                    text: word.1.to_lowercase(),
                    position_length: 1,
                });
                position += 1;
            }
        }
    }

    tokens
}

/// Split non-Thai text on whitespace and punctuation, returning (byte_offset_within_run, word).
fn split_non_thai(text: &str) -> Vec<(usize, &str)> {
    let mut results = Vec::new();
    let mut start = None;

    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start {
            results.push((s, &text[s..i]));
            start = None;
        }
    }
    // Flush last word.
    if let Some(s) = start {
        results.push((s, &text[s..]));
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segmenter::DictionarySegmenter;

    fn make_tokenizer() -> ThaiTantivyTokenizer {
        let seg = DictionarySegmenter::new();
        ThaiTantivyTokenizer::new(seg.shared())
    }

    #[test]
    fn tokenize_thai_text() {
        let mut tok = make_tokenizer();
        let mut stream = tok.token_stream("ห้องสมุดเปิดแล้ว");
        let mut words: Vec<String> = Vec::new();
        while stream.advance() {
            words.push(stream.token().text.clone());
        }
        assert!(
            words.contains(&"ห้องสมุด".to_string()),
            "Expected 'ห้องสมุด' in: {words:?}"
        );
        assert!(
            words.contains(&"เปิด".to_string()),
            "Expected 'เปิด' in: {words:?}"
        );
    }

    #[test]
    fn tokenize_english_text() {
        let mut tok = make_tokenizer();
        let mut stream = tok.token_stream("hello world");
        let mut words: Vec<String> = Vec::new();
        while stream.advance() {
            words.push(stream.token().text.clone());
        }
        assert_eq!(words, vec!["hello", "world"]);
    }

    #[test]
    fn tokenize_mixed_script() {
        let mut tok = make_tokenizer();
        let mut stream = tok.token_stream("ห้องสมุดเปิด library");
        let mut words: Vec<String> = Vec::new();
        while stream.advance() {
            words.push(stream.token().text.clone());
        }
        assert!(
            words.contains(&"ห้องสมุด".to_string()),
            "Expected 'ห้องสมุด' in: {words:?}"
        );
        assert!(
            words.contains(&"library".to_string()),
            "Expected 'library' in: {words:?}"
        );
    }

    fn collect(text: &str) -> Vec<String> {
        let mut tok = make_tokenizer();
        let mut stream = tok.token_stream(text);
        let mut words = Vec::new();
        while stream.advance() {
            words.push(stream.token().text.clone());
        }
        words
    }

    #[test]
    fn thai_stopwords_are_filtered() {
        // "ข้อควรระวังของธุรกิจที่ให้บริการ" — ของ/ที่/ให้ are function words.
        let words = collect("ข้อควรระวังของธุรกิจที่ให้บริการ");
        assert!(!words.contains(&"ของ".to_string()), "{words:?}");
        assert!(!words.contains(&"ที่".to_string()), "{words:?}");
        assert!(words.contains(&"ธุรกิจ".to_string()), "{words:?}");
        // nlpo3 keeps the compound verb "ให้บริการ" as one token — the
        // stoplist only removes STANDALONE function words.
        assert!(words.contains(&"ให้บริการ".to_string()), "{words:?}");
    }

    #[test]
    fn negation_is_never_a_stopword() {
        let words = collect("ธุรกิจไม่อนุญาต");
        assert!(words.contains(&"ไม่".to_string()), "{words:?}");
    }

    #[test]
    fn decomposed_sara_am_normalizes_to_precomposed() {
        // "นํา" (NIKHAHIT + SARA AA) must index identically to "นำ" (SARA AM).
        let decomposed = collect("น\u{0E4D}\u{0E32}");
        let precomposed = collect("น\u{0E33}");
        assert_eq!(decomposed, precomposed, "SARA AM forms must unify");
        assert_eq!(precomposed, vec!["น\u{0E33}".to_string()]);
    }

    #[test]
    fn byte_offsets_are_correct() {
        let text = "hello ห้องสมุดเปิด";
        let mut tok = make_tokenizer();
        let mut stream = tok.token_stream(text);
        while stream.advance() {
            let t = stream.token();
            let slice = &text[t.offset_from..t.offset_to];
            // Thai tokens: exact match; non-Thai tokens: lowercased.
            let expected = if slice.chars().any(is_thai) {
                slice.trim().to_string()
            } else {
                slice.trim().to_lowercase()
            };
            assert_eq!(expected, t.text, "Offset mismatch for token '{}'", t.text);
        }
    }
}
