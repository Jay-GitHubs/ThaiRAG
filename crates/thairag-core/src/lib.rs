pub mod error;
pub mod models;
pub mod permission;
pub mod prompt_registry;
pub mod traits;
pub mod types;

pub use error::{Result, ThaiRagError};
pub use prompt_registry::PromptRegistry;

/// Return the largest byte index `<= max_bytes` that is a valid UTF-8 char
/// boundary.  Replacement for the unstable `str::floor_char_boundary`.
///
/// Use this instead of `&s[..n]` whenever `n` might land inside a multi-byte
/// character (Thai = 3 bytes, CJK = 3 bytes, emoji = 4 bytes).
pub fn floor_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut pos = max_bytes;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Truncate a string to at most `max_bytes` bytes, rounding down to the
/// nearest char boundary so it never panics on multi-byte UTF-8 text.
pub fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    &s[..floor_char_boundary(s, max_bytes)]
}
