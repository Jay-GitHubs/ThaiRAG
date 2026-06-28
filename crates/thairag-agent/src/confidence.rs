//! LLM self-rated answer confidence (1–10).
//!
//! After an answer is generated, this asks the response LLM to rate — on a
//! 1–10 scale — how well the retrieved context supports the answer. It's a
//! single short, low-token call (the rating is the only output). The score is
//! surfaced to the chat UI and the admin test-chat so reviewers can gauge how
//! grounded an answer is. Default-on but config-gated.

use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;

use crate::context_curator::CuratedContext;

/// Build a compact, bounded context excerpt for the scoring prompt — enough for
/// the model to judge grounding without re-sending the entire (possibly huge)
/// context.
fn context_excerpt(context: &CuratedContext, max_chars: usize) -> String {
    let mut out = String::new();
    for c in &context.chunks {
        if out.len() >= max_chars {
            break;
        }
        let remaining = max_chars - out.len();
        let body: String = c.content.chars().take(remaining.min(600)).collect();
        match &c.source_doc_title {
            Some(t) => out.push_str(&format!("[{}] ({})\n{}\n\n", c.index, t, body)),
            None => out.push_str(&format!("[{}]\n{}\n\n", c.index, body)),
        }
    }
    out
}

/// Extract the first integer in 1..=10 from the model's reply, clamped.
pub fn parse_score(text: &str) -> Option<u8> {
    let mut digits = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            // A two-digit run can only be a valid score if it's exactly "10".
            if digits.len() == 2 {
                break;
            }
        } else if !digits.is_empty() {
            break;
        }
    }
    let n: u32 = digits.parse().ok()?;
    if (1..=10).contains(&n) {
        Some(n as u8)
    } else if n == 0 {
        Some(1)
    } else {
        Some(10)
    }
}

/// Ask the LLM to rate how well the context supports the answer (1–10).
/// Returns `None` on any error or an unparseable reply (caller leaves the score
/// unset rather than guessing).
pub async fn assess(
    llm: &dyn LlmProvider,
    query: &str,
    answer: &str,
    context: &CuratedContext,
) -> Option<u8> {
    if answer.trim().is_empty() || context.chunks.is_empty() {
        return None;
    }
    let prompt = format!(
        "You are scoring a retrieval-augmented answer for confidence.\n\n\
         Question:\n{query}\n\n\
         Retrieved context:\n{ctx}\n\n\
         Answer:\n{answer}\n\n\
         On a scale of 1 to 10, how well is the Answer supported by the Retrieved \
         context? 10 = every claim is directly supported; 1 = unsupported, or the \
         answer states the context lacks the information. Reply with ONLY the integer.",
        query = query,
        ctx = context_excerpt(context, 4000),
        answer = answer.chars().take(2000).collect::<String>(),
    );
    let messages = [ChatMessage {
        role: "user".to_string(),
        content: prompt,
        images: vec![],
    }];
    match llm.generate(&messages, Some(8)).await {
        Ok(resp) => parse_score(&resp.content),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scores() {
        assert_eq!(parse_score("8"), Some(8));
        assert_eq!(parse_score("Confidence: 7/10"), Some(7));
        assert_eq!(parse_score("10"), Some(10));
        assert_eq!(parse_score("10/10"), Some(10));
        assert_eq!(parse_score("0"), Some(1));
        assert_eq!(parse_score("score is 3."), Some(3));
        assert_eq!(parse_score("none"), None);
    }
}
