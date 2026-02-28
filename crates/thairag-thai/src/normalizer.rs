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
