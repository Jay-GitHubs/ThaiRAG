use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, SearchResult};
use tracing::{debug, warn};

/// An extracted entity from a document chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
}

/// A relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub from: String,
    pub to: String,
    pub relation_type: String,
}

/// A knowledge graph built from extracted entities and relationships.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub entities: HashMap<String, Entity>,
    pub relationships: Vec<Relationship>,
    /// Adjacency: entity_name → [(target_name, relation_type)]
    adjacency: HashMap<String, Vec<(String, String)>>,
}

impl KnowledgeGraph {
    pub fn add_entity(&mut self, entity: Entity) {
        let key = entity.name.to_lowercase();
        for alias in &entity.aliases {
            self.entities
                .entry(alias.to_lowercase())
                .or_insert_with(|| entity.clone());
        }
        self.entities.entry(key).or_insert(entity);
    }

    pub fn add_relationship(&mut self, rel: Relationship) {
        let from_key = rel.from.to_lowercase();
        let to_key = rel.to.to_lowercase();
        self.adjacency
            .entry(from_key.clone())
            .or_default()
            .push((to_key.clone(), rel.relation_type.clone()));
        self.adjacency
            .entry(to_key)
            .or_default()
            .push((from_key, rel.relation_type.clone()));
        self.relationships.push(rel);
    }

    /// Traverse the graph starting from seed entities up to max_depth hops.
    pub fn traverse(&self, seeds: &[String], max_depth: u32) -> Vec<String> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: Vec<(String, u32)> = Vec::new();
        let mut related = Vec::new();

        for seed in seeds {
            let key = seed.to_lowercase();
            if self.entities.contains_key(&key) {
                queue.push((key.clone(), 0));
                visited.insert(key);
            }
        }

        while let Some((node, depth)) = queue.pop() {
            related.push(node.clone());
            if depth >= max_depth {
                continue;
            }
            if let Some(neighbors) = self.adjacency.get(&node) {
                for (target, _rel) in neighbors {
                    if visited.insert(target.clone()) {
                        queue.push((target.clone(), depth + 1));
                    }
                }
            }
        }

        related
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn relationship_count(&self) -> usize {
        self.relationships.len()
    }
}

/// Graph RAG agent: extracts entities from search results and traverses
/// the knowledge graph to find additional relevant context.
pub struct GraphRag {
    llm: Arc<dyn LlmProvider>,
    max_entities: u32,
    max_depth: u32,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl GraphRag {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        max_entities: u32,
        max_depth: u32,
        max_tokens: u32,
    ) -> Self {
        Self {
            llm,
            max_entities,
            max_depth,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_entities: u32,
        max_depth: u32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_entities,
            max_depth,
            max_tokens,
            prompts,
        }
    }

    /// Extract entities and relationships from a text chunk.
    pub async fn extract_entities(&self, text: &str) -> Result<ExtractionResult> {
        const DEFAULT_GRAPH_RAG_PROMPT: &str = r#"Extract named entities and relationships from the given text.
Return JSON only:
{
  "entities": [{"name": "...", "entity_type": "Person|Organization|Location|Event|Concept|Product|Policy", "aliases": ["alt name"]}],
  "relationships": [{"from": "entity1", "to": "entity2", "relation_type": "works_at|located_in|part_of|related_to|created_by|manages|..."}]
}

Extract up to {{max}} entities. Focus on the most important named entities.
For Thai text, extract Thai names and transliterate if there's an English equivalent."#;

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.graph_rag",
                DEFAULT_GRAPH_RAG_PROMPT,
                &[("max", &self.max_entities.to_string())],
            ),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!("Text:\n{}", truncate(text, 2000)),
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<ExtractionResult>(json_str) {
                    Ok(result) => {
                        debug!(
                            entities = result.entities.len(),
                            relationships = result.relationships.len(),
                            "Graph RAG: extracted"
                        );
                        Ok(result)
                    }
                    Err(e) => {
                        warn!(error = %e, "Graph RAG: parse failed");
                        Ok(ExtractionResult::default())
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Graph RAG: LLM call failed");
                Ok(ExtractionResult::default())
            }
        }
    }

    /// Extract query entities (simpler prompt, just entity names from the query).
    pub async fn extract_query_entities(&self, query: &str) -> Result<Vec<String>> {
        let system = ChatMessage {
            role: "system".into(),
            content: r#"Extract named entities from the query. Return JSON: {"entities": ["entity1", "entity2"]}"#.into(),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}"),
        };

        match self.llm.generate(&[system, user], Some(128)).await {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                #[derive(Deserialize)]
                struct QE {
                    entities: Vec<String>,
                }
                match serde_json::from_str::<QE>(json_str) {
                    Ok(qe) => Ok(qe.entities),
                    Err(_) => Ok(Vec::new()),
                }
            }
            Err(_) => Ok(Vec::new()),
        }
    }

    /// Enhance search results using graph traversal.
    /// 1. Extract entities from the query
    /// 2. Find related entities in the graph
    /// 3. Score search results higher if they mention related entities
    pub async fn enhance_results(
        &self,
        query: &str,
        results: &[SearchResult],
        graph: &KnowledgeGraph,
    ) -> Result<Vec<SearchResult>> {
        let query_entities = self.extract_query_entities(query).await?;
        if query_entities.is_empty() {
            return Ok(results.to_vec());
        }

        let related = graph.traverse(&query_entities, self.max_depth);
        let related_set: HashSet<String> = related.into_iter().collect();

        debug!(
            query_entities = ?query_entities,
            related_count = related_set.len(),
            "Graph RAG: traversal complete"
        );

        let mut enhanced: Vec<SearchResult> = results
            .iter()
            .map(|r| {
                let mut result = r.clone();
                let content_lower = r.chunk.content.to_lowercase();
                let entity_boost: f32 = related_set
                    .iter()
                    .filter(|e| content_lower.contains(e.as_str()))
                    .count() as f32
                    * 0.05;
                result.score = (result.score + entity_boost).min(1.0);
                result
            })
            .collect();

        enhanced.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(enhanced)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ExtractionResult {
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') && let Some(end) = s.rfind('}') {
        return &s[start..=end];
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}
