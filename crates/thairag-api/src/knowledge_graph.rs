use std::sync::Arc;

use serde::Deserialize;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{info, warn};

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

/// Extract entities from text using an LLM.
///
/// Returns a list of (entity_name, entity_type) pairs.
pub async fn extract_entities_from_text(
    llm: &Arc<dyn LlmProvider>,
    text: &str,
) -> Vec<ExtractedEntity> {
    // Truncate text to avoid exceeding context window
    let truncated = if text.len() > 8000 {
        &text[..8000]
    } else {
        text
    };

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

    let truncated = if text.len() > 8000 {
        &text[..8000]
    } else {
        text
    };

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
}
