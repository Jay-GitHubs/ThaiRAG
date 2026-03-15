use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use thairag_core::PromptRegistry;
use tracing::{debug, warn};

/// Default hardcoded template for conversation summarization.
const DEFAULT_SUMMARIZER_PROMPT: &str = "You are a conversation summarizer. Given a conversation, produce:\n\
1. A concise 2-3 sentence summary of what was discussed and any conclusions\n\
2. A list of key topics/subjects\n\n\
Output JSON only:\n\
{\"summary\":\"...\",\"topics\":[\"topic1\",\"topic2\"]}\n\n\
Rules:\n\
- Focus on factual content, user preferences, and conclusions\n\
- Ignore greetings and filler\n\
- Keep topics short (1-3 words each)\n\
- Output ONLY valid JSON";

/// Default hardcoded template for memory context injection.
const DEFAULT_MEMORY_CONTEXT_PROMPT: &str = "Previous conversation context (from past sessions):\n{{context}}\n\n\
Use this context to maintain continuity. Reference relevant past discussions \
when helpful, but don't force it.";

/// A lightweight summary of a past conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub summary: String,
    pub topics: Vec<String>,
    pub timestamp: i64,
}

/// Agent: Conversation Memory.
/// Summarizes conversations into lightweight entries for cross-session context.
pub struct ConversationMemory {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl ConversationMemory {
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
        Self { llm, max_tokens, prompts }
    }

    /// Summarize a conversation into a memory entry.
    pub async fn summarize(&self, messages: &[ChatMessage]) -> Result<MemoryEntry> {
        let conversation = messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.conversation_memory_summarizer",
                DEFAULT_SUMMARIZER_PROMPT,
                &[],
            ),
        };
        let user = ChatMessage {
            role: "user".into(),
            content: conversation,
        };

        match self.llm.generate(&[system, user], Some(self.max_tokens)).await {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<LlmMemory>(json_str) {
                    Ok(m) => {
                        debug!(topics = ?m.topics, "Conversation summarized");
                        Ok(MemoryEntry {
                            summary: m.summary,
                            topics: m.topics,
                            timestamp: chrono::Utc::now().timestamp(),
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse memory summary, using raw");
                        Ok(MemoryEntry {
                            summary: resp.content.chars().take(200).collect(),
                            topics: vec![],
                            timestamp: chrono::Utc::now().timestamp(),
                        })
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Memory summarization failed");
                Err(e)
            }
        }
    }

    /// Build a system message injecting past memories into the pipeline context.
    pub fn build_memory_context(
        memories: &[MemoryEntry],
        prompts: &PromptRegistry,
    ) -> Option<ChatMessage> {
        if memories.is_empty() {
            return None;
        }

        let context = memories
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let topics = if m.topics.is_empty() {
                    String::new()
                } else {
                    format!(" [Topics: {}]", m.topics.join(", "))
                };
                format!("{}. {}{}", i + 1, m.summary, topics)
            })
            .collect::<Vec<_>>()
            .join("\n");

        Some(ChatMessage {
            role: "system".into(),
            content: prompts.render_or_default(
                "chat.conversation_memory_context",
                DEFAULT_MEMORY_CONTEXT_PROMPT,
                &[("context", &context)],
            ),
        })
    }
}

#[derive(Deserialize)]
struct LlmMemory {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    topics: Vec<String>,
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            return &s[start..=end];
        }
    }
    s
}
