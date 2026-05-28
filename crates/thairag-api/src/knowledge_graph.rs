use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, DocId, WorkspaceId};
use tracing::{info, warn};

use crate::store::KmStoreTrait;

/// Extracted entity: (name, type).
pub type ExtractedEntity = (String, String);

/// Extracted relation: (from_entity_name, to_entity_name, relation_type, confidence).
pub type ExtractedRelation = (String, String, String, f32);

#[derive(Debug, Deserialize)]
struct LlmExtractionResponse {
    #[serde(default)]
    entities: Vec<LlmEntity>,
    #[serde(default)]
    relations: Vec<LlmRelation>,
}

#[derive(Debug, Deserialize)]
struct LlmEntity {
    name: String,
    #[serde(rename = "type")]
    entity_type: String,
}

#[derive(Debug, Deserialize)]
struct LlmRelation {
    from: String,
    to: String,
    relation_type: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

fn default_confidence() -> f32 {
    0.8
}

const EXTRACTION_PROMPT: &str = r#"You are a knowledge graph extraction system. Extract entities and relationships from the given text.

Return ONLY valid JSON (no markdown, no explanation) with this exact structure:
{
  "entities": [
    {"name": "Entity Name", "type": "Person|Organization|Location|Concept|Event|Technology|Product"}
  ],
  "relations": [
    {"from": "Entity A", "to": "Entity B", "relation_type": "works_at|located_in|related_to|part_of|created_by|uses|manages|reports_to|collaborates_with|depends_on", "confidence": 0.9}
  ]
}

Rules:
- Entity types must be one of: Person, Organization, Location, Concept, Event, Technology, Product
- Normalize entity names (capitalize properly, remove duplicates)
- Only extract entities that are clearly identifiable
- Confidence should be between 0.0 and 1.0
- Extract both Thai and English entities
- Keep entity names concise but unambiguous"#;

/// Truncate `text` to at most `max_bytes` bytes, snapping to the nearest
/// char boundary at or below the limit. Naive byte slicing (`&text[..N]`)
/// panics when `N` lands inside a multi-byte UTF-8 codepoint — common with
/// Thai (3 bytes per char) once `text.len() > N`.
///
/// Returns the original `text` unchanged if it's already within the budget.
fn truncate_at_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut idx = max_bytes;
    while !text.is_char_boundary(idx) {
        idx -= 1;
    }
    &text[..idx]
}

/// Extract entities from text using an LLM.
///
/// Returns a list of (entity_name, entity_type) pairs.
pub async fn extract_entities_from_text(
    llm: &Arc<dyn LlmProvider>,
    text: &str,
) -> Vec<ExtractedEntity> {
    // Truncate text to avoid exceeding context window (char-boundary safe).
    let truncated = truncate_at_char_boundary(text, 8000);

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: EXTRACTION_PROMPT.to_string(),
            images: vec![],
        },
        ChatMessage {
            role: "user".into(),
            content: format!("Extract entities and relationships from this text:\n\n{truncated}"),
            images: vec![],
        },
    ];

    match llm.generate(&messages, Some(2048)).await {
        Ok(response) => {
            let content = response.content.trim();
            // Try to parse JSON from the response (handle markdown code blocks)
            let json_str = extract_json_from_response(content);
            match serde_json::from_str::<LlmExtractionResponse>(json_str) {
                Ok(parsed) => {
                    info!("LLM extracted {} entities", parsed.entities.len());
                    parsed
                        .entities
                        .into_iter()
                        .map(|e| (e.name, normalize_entity_type(&e.entity_type)))
                        .collect()
                }
                Err(e) => {
                    warn!("Failed to parse LLM extraction response: {e}");
                    fallback_extract_entities(text)
                }
            }
        }
        Err(e) => {
            warn!("LLM extraction failed, using fallback: {e}");
            fallback_extract_entities(text)
        }
    }
}

/// Extract relations from text using an LLM, given known entities.
///
/// Returns a list of (from_name, to_name, relation_type, confidence).
pub async fn extract_relations_from_text(
    llm: &Arc<dyn LlmProvider>,
    text: &str,
    entities: &[ExtractedEntity],
) -> Vec<ExtractedRelation> {
    if entities.len() < 2 {
        return vec![];
    }

    let truncated = truncate_at_char_boundary(text, 8000);

    let entity_list: Vec<String> = entities
        .iter()
        .map(|(name, etype)| format!("- {name} ({etype})"))
        .collect();

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: EXTRACTION_PROMPT.to_string(),
            images: vec![],
        },
        ChatMessage {
            role: "user".into(),
            content: format!(
                "Known entities:\n{}\n\nExtract relationships between these entities from this text:\n\n{truncated}",
                entity_list.join("\n")
            ),
            images: vec![],
        },
    ];

    match llm.generate(&messages, Some(2048)).await {
        Ok(response) => {
            let content = response.content.trim();
            let json_str = extract_json_from_response(content);
            match serde_json::from_str::<LlmExtractionResponse>(json_str) {
                Ok(parsed) => {
                    info!("LLM extracted {} relations", parsed.relations.len());
                    parsed
                        .relations
                        .into_iter()
                        .map(|r| (r.from, r.to, r.relation_type, r.confidence.clamp(0.0, 1.0)))
                        .collect()
                }
                Err(e) => {
                    warn!("Failed to parse LLM relation response: {e}");
                    vec![]
                }
            }
        }
        Err(e) => {
            warn!("LLM relation extraction failed: {e}");
            vec![]
        }
    }
}

/// Extract entities and relations from a document's chunks and persist them
/// to the knowledge graph. Shared by the manual `/extract` route and the
/// on-ingest path.
///
/// Per-chunk extraction (rather than a single truncated whole-document call)
/// keeps long documents from silently losing entities. Entity names are
/// deduped case-insensitively across chunks, so a name appearing in several
/// chunks yields one entity with one doc link. At most `max_chunks` chunks
/// are processed to bound LLM cost.
///
/// Never panics; per-item failures are logged and skipped. Returns
/// `(entities_created, relations_created)`.
pub async fn extract_and_persist_graph(
    store: &Arc<dyn KmStoreTrait>,
    llm: &Arc<dyn LlmProvider>,
    workspace_id: WorkspaceId,
    doc_id: DocId,
    chunk_texts: &[String],
    max_chunks: usize,
) -> (usize, usize) {
    let cap = max_chunks.max(1);
    // Case-insensitive name → EntityId, shared across all chunks of the doc.
    let mut entity_ids: HashMap<String, thairag_core::types::EntityId> = HashMap::new();
    let mut relations_created = 0usize;

    for (i, chunk) in chunk_texts.iter().take(cap).enumerate() {
        if chunk.trim().is_empty() {
            continue;
        }
        let entities = extract_entities_from_text(llm, chunk).await;
        for (name, entity_type) in &entities {
            let key = name.to_lowercase();
            if let Some(&id) = entity_ids.get(&key) {
                // Already seen in an earlier chunk — just ensure the doc link.
                let _ = store.add_entity_doc_link(id, doc_id);
                continue;
            }
            match store.upsert_entity(name, entity_type, workspace_id, serde_json::json!({})) {
                Ok(entity) => {
                    let _ = store.add_entity_doc_link(entity.id, doc_id);
                    entity_ids.insert(key, entity.id);
                }
                Err(e) => warn!(%doc_id, chunk = i, "KG: upsert_entity '{name}' failed: {e}"),
            }
        }

        let relations = extract_relations_from_text(llm, chunk, &entities).await;
        for (from_name, to_name, rel_type, confidence) in &relations {
            if let (Some(&from_id), Some(&to_id)) = (
                entity_ids.get(&from_name.to_lowercase()),
                entity_ids.get(&to_name.to_lowercase()),
            ) && store
                .insert_relation(from_id, to_id, rel_type, *confidence, doc_id)
                .is_ok()
            {
                relations_created += 1;
            }
        }
    }

    (entity_ids.len(), relations_created)
}

/// Fallback regex-based entity extraction: extract capitalized proper nouns.
pub fn fallback_extract_entities(text: &str) -> Vec<ExtractedEntity> {
    let mut entities = std::collections::HashSet::new();

    // Match capitalized words (2+ words, proper nouns)
    let re = regex::Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b").unwrap();
    for cap in re.captures_iter(text) {
        let name = cap[1].to_string();
        // Skip very short or very common patterns
        if name.len() > 3 && name.split_whitespace().count() <= 5 {
            entities.insert(name);
        }
    }

    // Also extract Thai proper nouns in quotes or specific patterns
    let thai_re = regex::Regex::new(r#""([ก-๛]{2,}[ก-๛\s]*[ก-๛]{2,})""#).unwrap();
    for cap in thai_re.captures_iter(text) {
        let name = cap[1].to_string();
        if name.len() > 4 {
            entities.insert(name);
        }
    }

    let result: Vec<ExtractedEntity> = entities
        .into_iter()
        .map(|name| (name, "Concept".to_string()))
        .collect();

    info!("Fallback extraction found {} entities", result.len());
    result
}

/// Extract JSON from a response that might be wrapped in markdown code blocks.
fn extract_json_from_response(content: &str) -> &str {
    // Try to find ```json ... ``` block
    if let Some(start) = content.find("```json") {
        let json_start = start + 7;
        if let Some(end) = content[json_start..].find("```") {
            return content[json_start..json_start + end].trim();
        }
    }
    // Try to find ``` ... ``` block
    if let Some(start) = content.find("```") {
        let json_start = start + 3;
        if let Some(end) = content[json_start..].find("```") {
            return content[json_start..json_start + end].trim();
        }
    }
    // Try to find raw JSON (starts with {)
    if let Some(start) = content.find('{')
        && let Some(end) = content.rfind('}')
    {
        return &content[start..=end];
    }
    content
}

/// Normalize entity type to one of the standard types.
fn normalize_entity_type(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "person" | "people" | "individual" => "Person",
        "organization" | "org" | "company" | "institution" => "Organization",
        "location" | "place" | "city" | "country" | "region" => "Location",
        "concept" | "idea" | "topic" | "theme" => "Concept",
        "event" | "meeting" | "conference" | "incident" => "Event",
        "technology" | "tech" | "tool" | "framework" | "language" => "Technology",
        "product" | "service" | "software" | "application" => "Product",
        _ => "Concept",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use thairag_core::error::Result as CoreResult;
    use thairag_core::types::{LlmResponse, LlmUsage, VisionMessage};

    use crate::store::memory::MemoryKmStore;

    /// Mock LLM returning a fixed extraction JSON; counts `generate` calls.
    struct MockLlm {
        json: String,
        calls: AtomicUsize,
        fail: bool,
    }

    impl MockLlm {
        fn new(json: &str) -> Self {
            Self {
                json: json.to_string(),
                calls: AtomicUsize::new(0),
                fail: false,
            }
        }
        fn failing() -> Self {
            Self {
                json: String::new(),
                calls: AtomicUsize::new(0),
                fail: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockLlm {
        async fn generate(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
        ) -> CoreResult<LlmResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(thairag_core::error::ThaiRagError::LlmProvider(
                    "mock failure".into(),
                ));
            }
            Ok(LlmResponse {
                content: self.json.clone(),
                usage: LlmUsage::default(),
            })
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn generate_vision(
            &self,
            _messages: &[VisionMessage],
            _max_tokens: Option<u32>,
        ) -> CoreResult<LlmResponse> {
            Ok(LlmResponse {
                content: self.json.clone(),
                usage: LlmUsage::default(),
            })
        }
    }

    const TWO_ENTITY_JSON: &str = r#"{"entities":[{"name":"Acme","type":"Organization"},
        {"name":"Bob","type":"Person"}],
        "relations":[{"from":"Bob","to":"Acme","relation_type":"works_at","confidence":0.9}]}"#;

    #[tokio::test]
    async fn extract_and_persist_graph_dedups_entities_across_chunks() {
        let store: Arc<dyn KmStoreTrait> = Arc::new(MemoryKmStore::new());
        let llm: Arc<dyn LlmProvider> = Arc::new(MockLlm::new(TWO_ENTITY_JSON));
        let chunks = vec![
            "chunk one text".to_string(),
            "chunk two text".to_string(),
            "chunk three text".to_string(),
        ];
        let (entities, relations) =
            extract_and_persist_graph(&store, &llm, WorkspaceId::new(), DocId::new(), &chunks, 10)
                .await;
        // Same two entities across three chunks → deduped to two.
        assert_eq!(entities, 2);
        // One relation per processed chunk.
        assert_eq!(relations, 3);
    }

    #[tokio::test]
    async fn extract_and_persist_graph_respects_max_chunks() {
        let store: Arc<dyn KmStoreTrait> = Arc::new(MemoryKmStore::new());
        let mock = Arc::new(MockLlm::new(TWO_ENTITY_JSON));
        let llm: Arc<dyn LlmProvider> = mock.clone();
        let chunks: Vec<String> = (0..50).map(|i| format!("chunk {i}")).collect();
        extract_and_persist_graph(&store, &llm, WorkspaceId::new(), DocId::new(), &chunks, 3).await;
        // 3 chunks × 2 LLM calls (entities + relations) each.
        assert_eq!(mock.calls.load(Ordering::SeqCst), 6);
    }

    #[tokio::test]
    async fn extract_and_persist_graph_survives_llm_error() {
        let store: Arc<dyn KmStoreTrait> = Arc::new(MemoryKmStore::new());
        let llm: Arc<dyn LlmProvider> = Arc::new(MockLlm::failing());
        // Text with no proper nouns → fallback also extracts nothing.
        let chunks = vec!["lowercase words only here".to_string()];
        let (entities, relations) =
            extract_and_persist_graph(&store, &llm, WorkspaceId::new(), DocId::new(), &chunks, 10)
                .await;
        assert_eq!(entities, 0);
        assert_eq!(relations, 0);
    }

    #[tokio::test]
    async fn extract_and_persist_graph_empty_chunks_noop() {
        let store: Arc<dyn KmStoreTrait> = Arc::new(MemoryKmStore::new());
        let llm: Arc<dyn LlmProvider> = Arc::new(MockLlm::new(TWO_ENTITY_JSON));
        let (entities, relations) =
            extract_and_persist_graph(&store, &llm, WorkspaceId::new(), DocId::new(), &[], 10)
                .await;
        assert_eq!((entities, relations), (0, 0));
    }

    #[test]
    fn test_fallback_extraction() {
        let text = "John Smith works at Acme Corporation in New York City. \
                    The project uses Rust Programming Language.";
        let entities = fallback_extract_entities(text);
        let names: Vec<&str> = entities.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"John Smith"));
        assert!(names.contains(&"Acme Corporation"));
        assert!(names.contains(&"New York City"));
    }

    #[test]
    fn test_extract_json_from_response() {
        let wrapped = "Here is the result:\n```json\n{\"entities\":[]}\n```\nDone.";
        assert_eq!(extract_json_from_response(wrapped), r#"{"entities":[]}"#);

        let raw = r#"{"entities":[],"relations":[]}"#;
        assert_eq!(extract_json_from_response(raw), raw);
    }

    #[test]
    fn test_normalize_entity_type() {
        assert_eq!(normalize_entity_type("PERSON"), "Person");
        assert_eq!(normalize_entity_type("company"), "Organization");
        assert_eq!(normalize_entity_type("city"), "Location");
        assert_eq!(normalize_entity_type("unknown"), "Concept");
    }

    // ── truncate_at_char_boundary ──────────────────────────────────
    //
    // Regression coverage for the same bug class fixed in PR #72
    // (builtin_plugins.rs:137 panic on Thai char boundary). KG extraction
    // hit it on any Thai document > 8000 bytes during doc-ingest.

    #[test]
    fn truncate_at_char_boundary_passes_short_text_through() {
        let s = "hello";
        assert_eq!(truncate_at_char_boundary(s, 8000), s);
    }

    #[test]
    fn truncate_at_char_boundary_truncates_ascii_to_exact_byte_limit() {
        let s = "a".repeat(100);
        assert_eq!(truncate_at_char_boundary(&s, 80).len(), 80);
    }

    #[test]
    fn truncate_at_char_boundary_never_panics_mid_thai_char() {
        // Each Thai character is 3 bytes. Build a long Thai string and
        // truncate at byte limits that deliberately land mid-character.
        // Pre-fix `&s[..n]` would panic with "byte index N is not a char
        // boundary".
        let thai =
            "เมื่อเริ่มแข่ง กระต่ายก็พุ่งออกไปอย่างรวดเร็ว ทิ้งเต่าไว้ไกลลิบ ส่วนเต่าก็ค่อย ๆ เดินต่อ และจะไม่ยอมแพ้";
        let long_thai = thai.repeat(100);
        assert!(long_thai.len() > 8000);

        // Try several limits that are very likely to land mid-codepoint.
        for limit in [80, 8000, 9000, 12345] {
            let result = truncate_at_char_boundary(&long_thai, limit);
            assert!(result.len() <= limit, "must not exceed byte budget");
            assert!(
                long_thai.is_char_boundary(result.len()),
                "result must end on a char boundary"
            );
            // Round-trip: the slice must be valid UTF-8 (implicit in &str)
            // and a prefix of the original.
            assert!(long_thai.starts_with(result));
        }
    }

    #[test]
    fn truncate_at_char_boundary_handles_limit_equal_to_text_len() {
        let s = "abcdef";
        assert_eq!(truncate_at_char_boundary(s, s.len()), s);
    }
}
