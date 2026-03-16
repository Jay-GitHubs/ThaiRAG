pub mod analyzer;
pub mod chunker;
pub mod converter;
pub mod enricher;
pub mod orchestrator;
pub mod pipeline;
mod prompts;
pub mod quality;

/// Stable replacement for `str::floor_char_boundary` (unstable as of Rust 1.88).
/// Returns the largest byte index `<= i` that is a valid char boundary.
pub(crate) fn floor_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut pos = i;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}
