use thairag_core::error::Result;
use thairag_core::traits::DocumentProcessor;

/// Converts raw document bytes to markdown/text.
/// Currently handles markdown and plaintext passthrough.
pub struct MarkdownConverter;

impl MarkdownConverter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MarkdownConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentProcessor for MarkdownConverter {
    fn convert(&self, raw: &[u8], mime_type: &str) -> Result<String> {
        match mime_type {
            "text/markdown" | "text/plain" => {
                String::from_utf8(raw.to_vec())
                    .map_err(|e| thairag_core::ThaiRagError::Validation(e.to_string()))
            }
            _ => todo!("Document conversion for {mime_type} not yet implemented"),
        }
    }
}
