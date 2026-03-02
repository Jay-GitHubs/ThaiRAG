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

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(text: &str, max: usize) -> Vec<String> {
        MarkdownChunker::new().chunk(text, max, 0)
    }

    #[test]
    fn within_limit_single_chunk() {
        let result = chunk("Hello world", 100);
        assert_eq!(result, vec!["Hello world"]);
    }

    #[test]
    fn splits_on_paragraph_boundary() {
        let result = chunk("AAA\n\nBBB\n\nCCC", 5);
        assert_eq!(result, vec!["AAA", "BBB", "CCC"]);
    }

    #[test]
    fn empty_input() {
        let result = chunk("", 100);
        assert!(result.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let result = chunk("   \n\n   \n\n   ", 100);
        assert!(result.is_empty());
    }

    #[test]
    fn oversized_single_paragraph() {
        // A paragraph bigger than max_size still gets emitted as one chunk
        let result = chunk("abcdefghij", 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "abcdefghij");
    }

    #[test]
    fn content_integrity() {
        let input = "First paragraph.\n\nSecond paragraph.";
        let result = chunk(input, 1000);
        let reassembled = result.join("\n\n");
        assert_eq!(reassembled, input);
    }

    #[test]
    fn trims_paragraph_whitespace() {
        let result = chunk("  AAA  \n\n  BBB  ", 100);
        assert_eq!(result, vec!["AAA\n\nBBB"]);
    }
}
