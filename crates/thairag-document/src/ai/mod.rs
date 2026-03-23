pub mod analyzer;
pub mod chunker;
pub mod converter;
pub mod enricher;
pub mod orchestrator;
pub mod pipeline;
mod prompts;
pub mod quality;

/// Delegates to `thairag_core::floor_char_boundary`.
pub(crate) fn floor_char_boundary(s: &str, i: usize) -> usize {
    thairag_core::floor_char_boundary(s, i)
}
