use thairag_core::traits::Chunker;

/// Markdown-aware chunker.
/// Current implementation: paragraph-based splitting (stub).
pub struct MarkdownChunker;

impl MarkdownChunker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MarkdownChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for MarkdownChunker {
    fn chunk(&self, text: &str, max_size: usize, _overlap: usize) -> Vec<String> {
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
