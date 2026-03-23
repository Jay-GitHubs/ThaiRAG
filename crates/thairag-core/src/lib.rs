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

/// Strip `<think>...</think>` blocks from LLM output.
///
/// Reasoning models (Qwen3, DeepSeek-R1, QwQ) wrap chain-of-thought in
/// `<think>` tags. This must be removed before JSON parsing. Handles multiple
/// blocks and unclosed tags (model hit token limit mid-thought).
pub fn strip_thinking_tags(s: &str) -> String {
    if !s.contains("<think>") {
        return s.to_string();
    }
    let mut result = String::with_capacity(s.len());
    let mut rest = s;

    while let Some(start) = rest.find("<think>") {
        result.push_str(&rest[..start]);
        rest = &rest[start + 7..];

        if let Some(end) = rest.find("</think>") {
            rest = &rest[end + 8..];
        } else {
            // Unclosed <think> — model ran out of tokens mid-thought.
            return result.trim().to_string();
        }
    }

    result.push_str(rest);
    result.trim().to_string()
}

/// Extract the first JSON object `{...}` from LLM output, stripping
/// `<think>` blocks and markdown fences first.
pub fn extract_json(s: &str) -> &str {
    let cleaned = strip_think_prefix(s);
    if let Some(start) = cleaned.find('{')
        && let Some(end) = cleaned.rfind('}')
    {
        return &cleaned[start..=end];
    }
    cleaned
}

/// Extract the first JSON array `[...]` from LLM output, stripping
/// `<think>` blocks and markdown fences first.
pub fn extract_json_array(s: &str) -> &str {
    let cleaned = strip_think_prefix(s);
    if let Some(start) = cleaned.find('[')
        && let Some(end) = cleaned.rfind(']')
    {
        return &cleaned[start..=end];
    }
    cleaned
}

/// Strip leading `<think>...</think>` block from a string reference without
/// allocating, returning the remaining `&str`. Only handles a single leading
/// block (the common case for LLM output).
fn strip_think_prefix(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + 8..].trim();
        }
        // Unclosed — entire string is thinking
        return "";
    }
    trimmed
}
