use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{
    ChatMessage, CompactionResult, MemoryId, PersonalMemory, PersonalMemoryType, UserId,
    estimate_tokens,
};
use tracing::{debug, warn};

// ── Prompts ──────────────────────────────────────────────────────────

const DEFAULT_COMPACTION_PROMPT: &str = "\
You are a conversation compactor. Given a conversation history, produce:

1. A concise summary that preserves key context: decisions, facts, preferences, and ongoing topics.
   Write it as a narrative that the AI can use to continue the conversation seamlessly.
2. Personal memories worth remembering for future conversations.

Output JSON only:
{
  \"summary\": \"Narrative summary of the conversation so far...\",
  \"memories\": [
    {
      \"type\": \"preference|fact|decision|conversation|correction\",
      \"summary\": \"concise description\",
      \"topics\": [\"keyword1\", \"keyword2\"],
      \"importance\": 0.0-1.0
    }
  ]
}

Rules:
- The summary should enable conversation continuity (the user should not notice compaction happened)
- Extract ONLY information useful in FUTURE conversations as memories
- Types: preference (user likes/dislikes), fact (about user/context), decision (choices made), \
  conversation (topic summary), correction (user corrected something)
- Skip greetings, acknowledgements, and generic filler
- importance: 1.0 = critical (user preference/correction), 0.5 = useful, 0.1 = minor context
- Output ONLY valid JSON";

// ── Context Compactor ────────────────────────────────────────────────

/// Agent: Context Compactor.
/// Compacts conversation history when approaching the model's context window limit.
/// Extracts personal memories for vector storage during compaction.
pub struct ContextCompactor {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl ContextCompactor {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self {
            llm,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            prompts,
        }
    }

    /// Check if compaction is needed based on estimated token usage.
    pub fn needs_compaction(
        messages: &[ChatMessage],
        model_context_window: usize,
        threshold: f32,
        rag_budget: usize,
    ) -> bool {
        if model_context_window == 0 || messages.len() <= 4 {
            return false;
        }
        let msg_tokens = Self::estimate_messages_tokens(messages);
        let total_estimated = msg_tokens + rag_budget;
        let limit = (model_context_window as f32 * threshold) as usize;
        total_estimated > limit
    }

    /// Estimate total tokens for a set of messages.
    pub fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
        messages
            .iter()
            .map(|m| estimate_tokens(&m.content) + estimate_tokens(&m.role) + 4) // 4 for message framing
            .sum()
    }

    /// Compact older messages into a summary, keeping recent messages intact.
    /// Returns the compacted result with extracted personal memories.
    pub async fn compact(
        &self,
        messages: &[ChatMessage],
        keep_recent: usize,
        user_id: UserId,
    ) -> Result<CompactionResult> {
        let total = messages.len();
        let keep = keep_recent.min(total);
        let compact_end = total.saturating_sub(keep);

        if compact_end <= 1 {
            // Nothing meaningful to compact
            return Ok(CompactionResult {
                summary: String::new(),
                extracted_memories: vec![],
                messages_compacted: 0,
                messages_kept: total,
            });
        }

        let to_compact = &messages[..compact_end];

        // Build conversation text for LLM
        let conversation = to_compact
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.context_compactor",
                DEFAULT_COMPACTION_PROMPT,
                &[],
            ),
        };
        let user = ChatMessage {
            role: "user".into(),
            content: conversation,
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<CompactionOutput>(json_str) {
                    Ok(output) => {
                        let memory_count = output.memories.len();
                        let memories = output
                            .memories
                            .into_iter()
                            .filter(|m| m.importance >= 0.1)
                            .map(|m| PersonalMemory {
                                id: MemoryId::new(),
                                user_id,
                                memory_type: parse_memory_type(&m.memory_type),
                                summary: m.summary,
                                topics: m.topics,
                                importance: m.importance.clamp(0.0, 1.0),
                                created_at: chrono::Utc::now().timestamp(),
                                last_accessed_at: chrono::Utc::now().timestamp(),
                                relevance_score: 1.0,
                            })
                            .collect();

                        debug!(
                            compacted = compact_end,
                            kept = keep,
                            memories_extracted = memory_count,
                            "Context compacted"
                        );

                        Ok(CompactionResult {
                            summary: output.summary,
                            extracted_memories: memories,
                            messages_compacted: compact_end,
                            messages_kept: keep,
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse compaction output, using raw summary");
                        // Fallback: use the raw LLM output as summary, no memories extracted
                        Ok(CompactionResult {
                            summary: resp.content.chars().take(500).collect(),
                            extracted_memories: vec![],
                            messages_compacted: compact_end,
                            messages_kept: keep,
                        })
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Context compaction LLM call failed");
                Err(e)
            }
        }
    }

    /// Build a compacted message list: summary as system message + recent messages.
    pub fn build_compacted_messages(
        summary: &str,
        recent_messages: &[ChatMessage],
    ) -> Vec<ChatMessage> {
        let mut result = Vec::with_capacity(recent_messages.len() + 1);
        if !summary.is_empty() {
            result.push(ChatMessage {
                role: "system".into(),
                content: format!("[Conversation context (earlier messages summarized)]\n{summary}"),
            });
        }
        result.extend_from_slice(recent_messages);
        result
    }
}

// ── Internal types ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct CompactionOutput {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    memories: Vec<MemoryExtract>,
}

#[derive(Deserialize)]
struct MemoryExtract {
    #[serde(default, rename = "type")]
    memory_type: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default = "default_importance")]
    importance: f32,
}

fn default_importance() -> f32 {
    0.5
}

fn parse_memory_type(s: &str) -> PersonalMemoryType {
    match s.to_lowercase().as_str() {
        "preference" => PersonalMemoryType::Preference,
        "fact" => PersonalMemoryType::Fact,
        "decision" => PersonalMemoryType::Decision,
        "correction" => PersonalMemoryType::Correction,
        _ => PersonalMemoryType::Conversation,
    }
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{')
        && let Some(end) = s.rfind('}')
    {
        return &s[start..=end];
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_compaction_false_when_small() {
        let msgs = vec![
            ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "hi there".into(),
            },
        ];
        assert!(!ContextCompactor::needs_compaction(
            &msgs, 128_000, 0.8, 4096
        ));
    }

    #[test]
    fn test_needs_compaction_false_when_window_zero() {
        let msgs = vec![ChatMessage {
            role: "user".into(),
            content: "x".repeat(100_000),
        }];
        // Window 0 = auto-detect not available, skip compaction
        assert!(!ContextCompactor::needs_compaction(&msgs, 0, 0.8, 4096));
    }

    #[test]
    fn test_needs_compaction_true_when_large() {
        let mut msgs = Vec::new();
        for i in 0..50 {
            msgs.push(ChatMessage {
                role: "user".into(),
                content: format!("question {} with some content to fill tokens", i),
            });
            msgs.push(ChatMessage {
                role: "assistant".into(),
                content: format!("answer {} with a detailed explanation of the topic at hand that takes up many tokens in the context window", i),
            });
        }
        // Small window to trigger compaction
        assert!(ContextCompactor::needs_compaction(&msgs, 2000, 0.8, 500));
    }

    #[test]
    fn test_estimate_messages_tokens() {
        let msgs = vec![
            ChatMessage {
                role: "user".into(),
                content: "Hello world".into(), // ~3 tokens
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Hi there".into(), // ~2 tokens
            },
        ];
        let est = ContextCompactor::estimate_messages_tokens(&msgs);
        assert!(est > 0);
        assert!(est < 100); // sanity check
    }

    #[test]
    fn test_build_compacted_messages() {
        let recent = vec![
            ChatMessage {
                role: "user".into(),
                content: "latest question".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "latest answer".into(),
            },
        ];
        let result = ContextCompactor::build_compacted_messages("We discussed Rust.", &recent);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, "system");
        assert!(result[0].content.contains("We discussed Rust."));
        assert_eq!(result[1].content, "latest question");
    }

    #[test]
    fn test_build_compacted_messages_empty_summary() {
        let recent = vec![ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let result = ContextCompactor::build_compacted_messages("", &recent);
        assert_eq!(result.len(), 1); // no system message for empty summary
    }

    #[test]
    fn test_parse_memory_type() {
        assert_eq!(
            parse_memory_type("preference"),
            PersonalMemoryType::Preference
        );
        assert_eq!(parse_memory_type("FACT"), PersonalMemoryType::Fact);
        assert_eq!(parse_memory_type("decision"), PersonalMemoryType::Decision);
        assert_eq!(
            parse_memory_type("correction"),
            PersonalMemoryType::Correction
        );
        assert_eq!(
            parse_memory_type("unknown"),
            PersonalMemoryType::Conversation
        );
    }

    #[test]
    fn test_estimate_tokens() {
        // English text
        assert!(estimate_tokens("Hello world") > 0);
        // Thai text (should count at ~2 chars/token)
        assert!(estimate_tokens("สวัสดีครับ") > 0);
        // Mixed
        let mixed = estimate_tokens("Hello สวัสดี world");
        assert!(mixed > 0);
    }
}
