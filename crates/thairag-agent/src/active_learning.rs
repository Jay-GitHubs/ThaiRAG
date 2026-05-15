use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tracing::debug;

/// Active Learning: tracks feedback signals at the chunk level to identify:
/// 1. Low-quality chunks that consistently appear in negatively-rated responses
/// 2. High-value chunks that appear in positively-rated responses
/// 3. Low-confidence queries that may need additional training data
///
/// This information can drive:
/// - Chunk quality scoring adjustments in search results
/// - Identification of documents needing re-processing
/// - Signals for embedding model fine-tuning
pub struct ActiveLearning {
    /// Chunk-level feedback tracking: chunk_id → (positive_count, negative_count)
    chunk_feedback: Arc<RwLock<HashMap<String, ChunkFeedback>>>,
    /// Low-confidence queries for review
    low_confidence_queries: Arc<RwLock<Vec<LowConfidenceQuery>>>,
    /// Total feedback entries processed
    total_processed: AtomicU64,
    /// Minimum interactions before a chunk's quality score is adjusted
    min_interactions: u32,
    /// Maximum low-confidence queries to track
    max_low_confidence: usize,
}

/// Feedback statistics for a single chunk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChunkFeedback {
    pub positive: u32,
    pub negative: u32,
    pub total: u32,
    /// Computed quality adjustment: positive rate above/below 0.5 baseline
    pub quality_delta: f32,
}

impl ChunkFeedback {
    fn update(&mut self) {
        self.total = self.positive + self.negative;
        if self.total > 0 {
            let rate = self.positive as f32 / self.total as f32;
            self.quality_delta = rate - 0.5; // -0.5 to +0.5 range
        }
    }
}

/// A query that resulted in low-confidence responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LowConfidenceQuery {
    pub query: String,
    pub avg_relevance: f32,
    pub timestamp: u64,
}

/// Statistics about the active learning system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveLearningStats {
    pub total_feedback_processed: u64,
    pub tracked_chunks: usize,
    pub positive_chunks: usize,
    pub negative_chunks: usize,
    pub low_confidence_queries: usize,
}

impl ActiveLearning {
    pub fn new(min_interactions: u32, max_low_confidence: usize) -> Self {
        Self {
            chunk_feedback: Arc::new(RwLock::new(HashMap::new())),
            low_confidence_queries: Arc::new(RwLock::new(Vec::new())),
            total_processed: AtomicU64::new(0),
            min_interactions,
            max_low_confidence,
        }
    }

    /// Record feedback for chunks that appeared in a response.
    pub fn record_chunk_feedback(&self, chunk_ids: &[String], thumbs_up: bool) {
        let mut feedback = self.chunk_feedback.write().unwrap();
        for chunk_id in chunk_ids {
            let entry = feedback.entry(chunk_id.clone()).or_default();
            if thumbs_up {
                entry.positive += 1;
            } else {
                entry.negative += 1;
            }
            entry.update();
        }
        self.total_processed.fetch_add(1, Ordering::Relaxed);
        debug!(
            chunks = chunk_ids.len(),
            thumbs_up, "Active learning: recorded chunk feedback"
        );
    }

    /// Record a low-confidence query for later review.
    pub fn record_low_confidence_query(&self, query: &str, avg_relevance: f32) {
        let mut queries = self.low_confidence_queries.write().unwrap();

        // Prevent duplicates
        if queries.iter().any(|q| q.query == query) {
            return;
        }

        queries.push(LowConfidenceQuery {
            query: query.to_string(),
            avg_relevance,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });

        // Trim to max size, keeping most recent
        if queries.len() > self.max_low_confidence {
            queries.sort_by_key(|q| std::cmp::Reverse(q.timestamp));
            queries.truncate(self.max_low_confidence);
        }
    }

    /// Get the quality adjustment for a chunk (-0.5 to +0.5).
    /// Returns 0.0 if not enough data or chunk not tracked.
    pub fn get_chunk_quality_delta(&self, chunk_id: &str) -> f32 {
        let feedback = self.chunk_feedback.read().unwrap();
        if let Some(fb) = feedback.get(chunk_id)
            && fb.total >= self.min_interactions
        {
            return fb.quality_delta;
        }
        0.0
    }

    /// Adjust search result scores based on accumulated chunk feedback.
    pub fn adjust_scores(&self, results: &mut [thairag_core::types::SearchResult]) {
        let feedback = self.chunk_feedback.read().unwrap();
        let mut adjusted = 0u32;

        for result in results.iter_mut() {
            let key = result.chunk.chunk_id.to_string();
            if let Some(fb) = feedback.get(&key)
                && fb.total >= self.min_interactions
            {
                // Apply a bounded adjustment: max +-10% of score
                let adjustment = fb.quality_delta * 0.1;
                result.score = (result.score + adjustment).clamp(0.0, 1.0);
                adjusted += 1;
            }
        }

        if adjusted > 0 {
            debug!(adjusted, "Active learning: adjusted search scores");
        }
    }

    /// Get learning statistics.
    pub fn stats(&self) -> ActiveLearningStats {
        let feedback = self.chunk_feedback.read().unwrap();
        let positive = feedback
            .values()
            .filter(|f| f.quality_delta > 0.0 && f.total >= self.min_interactions)
            .count();
        let negative = feedback
            .values()
            .filter(|f| f.quality_delta < 0.0 && f.total >= self.min_interactions)
            .count();
        let low_conf = self.low_confidence_queries.read().unwrap().len();

        ActiveLearningStats {
            total_feedback_processed: self.total_processed.load(Ordering::Relaxed),
            tracked_chunks: feedback.len(),
            positive_chunks: positive,
            negative_chunks: negative,
            low_confidence_queries: low_conf,
        }
    }
}
