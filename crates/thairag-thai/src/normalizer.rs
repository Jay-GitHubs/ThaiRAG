/// Thai text normalizer.
/// Current implementation: basic whitespace normalization (stub).
pub struct ThaiNormalizer;

impl ThaiNormalizer {
    pub fn new() -> Self {
        Self
    }

    /// Normalize Thai text: collapse whitespace, trim.
    pub fn normalize(&self, text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

impl Default for ThaiNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_multiple_spaces() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("hello   world"), "hello world");
    }

    #[test]
    fn trim_leading_trailing() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("  hello  "), "hello");
    }

    #[test]
    fn mixed_whitespace() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("hello\t\nworld"), "hello world");
    }

    #[test]
    fn empty_string() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize(""), "");
    }

    #[test]
    fn whitespace_only() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("   \t  \n  "), "");
    }

    #[test]
    fn thai_text_preserved() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("สวัสดี  ครับ"), "สวัสดี ครับ");
    }

    #[test]
    fn single_word_unchanged() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("hello"), "hello");
    }
}
