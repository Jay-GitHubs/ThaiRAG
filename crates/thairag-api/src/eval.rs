use thairag_core::types::DocId;

/// Compute Discounted Cumulative Gain at rank k.
///
/// If `relevance_scores` is provided, uses graded relevance (matching by doc_id position).
/// Otherwise, binary relevance: 1.0 if the doc is in `relevant`, 0.0 otherwise.
fn dcg(retrieved: &[DocId], relevant: &[DocId], relevance_scores: Option<&[f32]>, k: usize) -> f64 {
    let limit = retrieved.len().min(k);
    let mut sum = 0.0_f64;
    for (rank, doc) in retrieved.iter().take(limit).enumerate() {
        let rel = if let Some(scores) = relevance_scores {
            // Find this doc in the relevant list and use its graded score
            relevant
                .iter()
                .position(|d| d == doc)
                .map(|idx| scores.get(idx).copied().unwrap_or(1.0) as f64)
                .unwrap_or(0.0)
        } else {
            // Binary relevance
            if relevant.contains(doc) { 1.0 } else { 0.0 }
        };
        sum += rel / ((rank as f64 + 2.0).log2()); // log2(rank + 2) since rank is 0-indexed
    }
    sum
}

/// Compute NDCG (Normalized Discounted Cumulative Gain) at rank k.
///
/// NDCG = DCG / ideal DCG (perfect ranking). Returns 0.0 if no relevant docs exist.
pub fn compute_ndcg(
    retrieved: &[DocId],
    relevant: &[DocId],
    relevance_scores: Option<&[f32]>,
    k: usize,
) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }

    let actual_dcg = dcg(retrieved, relevant, relevance_scores, k);

    // Ideal DCG: relevant docs sorted by relevance score descending
    let mut ideal_order: Vec<DocId> = relevant.to_vec();
    if let Some(scores) = relevance_scores {
        // Sort by descending relevance
        let mut scored: Vec<(DocId, f32)> = ideal_order
            .into_iter()
            .enumerate()
            .map(|(i, d)| (d, scores.get(i).copied().unwrap_or(1.0)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ideal_order = scored.into_iter().map(|(d, _)| d).collect();
    }
    let ideal_dcg = dcg(&ideal_order, relevant, relevance_scores, k);

    if ideal_dcg == 0.0 {
        0.0
    } else {
        actual_dcg / ideal_dcg
    }
}

/// Compute Mean Reciprocal Rank.
///
/// Returns 1/rank of the first relevant document found, or 0.0 if none.
pub fn compute_mrr(retrieved: &[DocId], relevant: &[DocId]) -> f64 {
    for (rank, doc) in retrieved.iter().enumerate() {
        if relevant.contains(doc) {
            return 1.0 / (rank as f64 + 1.0);
        }
    }
    0.0
}

/// Compute Precision@k: fraction of retrieved docs (up to k) that are relevant.
pub fn compute_precision_at_k(retrieved: &[DocId], relevant: &[DocId], k: usize) -> f64 {
    let limit = retrieved.len().min(k);
    if limit == 0 {
        return 0.0;
    }
    let hits = retrieved
        .iter()
        .take(limit)
        .filter(|d| relevant.contains(d))
        .count();
    hits as f64 / limit as f64
}

/// Compute Recall@k: fraction of relevant docs that appear in the top-k retrieved.
pub fn compute_recall_at_k(retrieved: &[DocId], relevant: &[DocId], k: usize) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let limit = retrieved.len().min(k);
    let hits = retrieved
        .iter()
        .take(limit)
        .filter(|d| relevant.contains(d))
        .count();
    hits as f64 / relevant.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn doc(n: u128) -> DocId {
        DocId(Uuid::from_u128(n))
    }

    // ── NDCG Tests ──────────────────────────────────────────────────

    #[test]
    fn ndcg_perfect_ranking_binary() {
        // Retrieved = [1, 2, 3], Relevant = [1, 2, 3]
        let retrieved = vec![doc(1), doc(2), doc(3)];
        let relevant = vec![doc(1), doc(2), doc(3)];
        let ndcg = compute_ndcg(&retrieved, &relevant, None, 5);
        assert!(
            (ndcg - 1.0).abs() < 1e-10,
            "Perfect ranking => NDCG=1.0, got {ndcg}"
        );
    }

    #[test]
    fn ndcg_no_relevant_docs() {
        let retrieved = vec![doc(1), doc(2)];
        let relevant = vec![];
        let ndcg = compute_ndcg(&retrieved, &relevant, None, 5);
        assert!((ndcg - 0.0).abs() < 1e-10);
    }

    #[test]
    fn ndcg_no_retrieved_docs() {
        let retrieved = vec![];
        let relevant = vec![doc(1), doc(2)];
        let ndcg = compute_ndcg(&retrieved, &relevant, None, 5);
        assert!((ndcg - 0.0).abs() < 1e-10);
    }

    #[test]
    fn ndcg_imperfect_ranking() {
        // Relevant = [1, 2], Retrieved = [3, 1, 2] => doc 1 at rank 2, doc 2 at rank 3
        let retrieved = vec![doc(3), doc(1), doc(2)];
        let relevant = vec![doc(1), doc(2)];
        let ndcg = compute_ndcg(&retrieved, &relevant, None, 5);
        assert!(
            ndcg > 0.0 && ndcg < 1.0,
            "Imperfect => 0 < NDCG < 1, got {ndcg}"
        );
    }

    #[test]
    fn ndcg_graded_relevance() {
        // Doc 1 has relevance 3.0, Doc 2 has relevance 1.0
        // Perfect order: [1, 2]
        let relevant = vec![doc(1), doc(2)];
        let scores = vec![3.0, 1.0];

        // Perfect order
        let retrieved_perfect = vec![doc(1), doc(2)];
        let ndcg_perfect = compute_ndcg(&retrieved_perfect, &relevant, Some(&scores), 5);
        assert!(
            (ndcg_perfect - 1.0).abs() < 1e-10,
            "Perfect graded => 1.0, got {ndcg_perfect}"
        );

        // Reversed order: [2, 1] — less relevant doc first
        let retrieved_reversed = vec![doc(2), doc(1)];
        let ndcg_reversed = compute_ndcg(&retrieved_reversed, &relevant, Some(&scores), 5);
        assert!(
            ndcg_reversed < 1.0,
            "Reversed graded => < 1.0, got {ndcg_reversed}"
        );
    }

    #[test]
    fn ndcg_k_limits_rank() {
        // Relevant doc at position 6, k=5 => not counted
        let mut retrieved = vec![doc(10), doc(11), doc(12), doc(13), doc(14), doc(1)];
        let relevant = vec![doc(1)];

        let ndcg_at_5 = compute_ndcg(&retrieved, &relevant, None, 5);
        assert!((ndcg_at_5 - 0.0).abs() < 1e-10, "Doc beyond k=5 => 0.0");

        let ndcg_at_10 = compute_ndcg(&retrieved, &relevant, None, 10);
        assert!(ndcg_at_10 > 0.0, "Doc within k=10 => > 0.0");

        // Move relevant doc to position 1
        retrieved = vec![doc(1), doc(10), doc(11), doc(12), doc(13), doc(14)];
        let ndcg_first = compute_ndcg(&retrieved, &relevant, None, 5);
        assert!((ndcg_first - 1.0).abs() < 1e-10);
    }

    // ── MRR Tests ───────────────────────────────────────────────────

    #[test]
    fn mrr_first_result_relevant() {
        let retrieved = vec![doc(1), doc(2), doc(3)];
        let relevant = vec![doc(1)];
        let mrr = compute_mrr(&retrieved, &relevant);
        assert!((mrr - 1.0).abs() < 1e-10);
    }

    #[test]
    fn mrr_second_result_relevant() {
        let retrieved = vec![doc(10), doc(1), doc(2)];
        let relevant = vec![doc(1)];
        let mrr = compute_mrr(&retrieved, &relevant);
        assert!((mrr - 0.5).abs() < 1e-10);
    }

    #[test]
    fn mrr_no_relevant_found() {
        let retrieved = vec![doc(10), doc(11), doc(12)];
        let relevant = vec![doc(1)];
        let mrr = compute_mrr(&retrieved, &relevant);
        assert!((mrr - 0.0).abs() < 1e-10);
    }

    #[test]
    fn mrr_empty_retrieved() {
        let mrr = compute_mrr(&[], &[doc(1)]);
        assert!((mrr - 0.0).abs() < 1e-10);
    }

    #[test]
    fn mrr_multiple_relevant_returns_first() {
        // Both doc 2 and doc 3 are relevant; MRR uses rank of first found
        let retrieved = vec![doc(10), doc(2), doc(3)];
        let relevant = vec![doc(2), doc(3)];
        let mrr = compute_mrr(&retrieved, &relevant);
        assert!((mrr - 0.5).abs() < 1e-10, "First relevant at rank 2 => 0.5");
    }

    // ── Precision@k Tests ───────────────────────────────────────────

    #[test]
    fn precision_all_relevant() {
        let retrieved = vec![doc(1), doc(2), doc(3)];
        let relevant = vec![doc(1), doc(2), doc(3)];
        let p = compute_precision_at_k(&retrieved, &relevant, 3);
        assert!((p - 1.0).abs() < 1e-10);
    }

    #[test]
    fn precision_none_relevant() {
        let retrieved = vec![doc(10), doc(11), doc(12)];
        let relevant = vec![doc(1), doc(2)];
        let p = compute_precision_at_k(&retrieved, &relevant, 3);
        assert!((p - 0.0).abs() < 1e-10);
    }

    #[test]
    fn precision_partial() {
        let retrieved = vec![doc(1), doc(10), doc(2), doc(11), doc(3)];
        let relevant = vec![doc(1), doc(2), doc(3)];
        // P@5 = 3/5 = 0.6
        let p = compute_precision_at_k(&retrieved, &relevant, 5);
        assert!((p - 0.6).abs() < 1e-10);
    }

    #[test]
    fn precision_k_limits() {
        let retrieved = vec![doc(1), doc(10), doc(2)];
        let relevant = vec![doc(1), doc(2)];
        // P@1 = 1/1 = 1.0 (only first doc considered)
        let p = compute_precision_at_k(&retrieved, &relevant, 1);
        assert!((p - 1.0).abs() < 1e-10);
    }

    #[test]
    fn precision_empty() {
        assert!((compute_precision_at_k(&[], &[doc(1)], 5) - 0.0).abs() < 1e-10);
    }

    // ── Recall@k Tests ──────────────────────────────────────────────

    #[test]
    fn recall_all_found() {
        let retrieved = vec![doc(1), doc(2), doc(3)];
        let relevant = vec![doc(1), doc(2)];
        let r = compute_recall_at_k(&retrieved, &relevant, 5);
        assert!((r - 1.0).abs() < 1e-10);
    }

    #[test]
    fn recall_none_found() {
        let retrieved = vec![doc(10), doc(11)];
        let relevant = vec![doc(1), doc(2)];
        let r = compute_recall_at_k(&retrieved, &relevant, 5);
        assert!((r - 0.0).abs() < 1e-10);
    }

    #[test]
    fn recall_partial() {
        let retrieved = vec![doc(1), doc(10), doc(11)];
        let relevant = vec![doc(1), doc(2)];
        // 1 out of 2 relevant found
        let r = compute_recall_at_k(&retrieved, &relevant, 5);
        assert!((r - 0.5).abs() < 1e-10);
    }

    #[test]
    fn recall_k_limits() {
        // Doc 2 is beyond k=1
        let retrieved = vec![doc(10), doc(2)];
        let relevant = vec![doc(2)];
        let r_at_1 = compute_recall_at_k(&retrieved, &relevant, 1);
        assert!((r_at_1 - 0.0).abs() < 1e-10);
        let r_at_2 = compute_recall_at_k(&retrieved, &relevant, 2);
        assert!((r_at_2 - 1.0).abs() < 1e-10);
    }

    #[test]
    fn recall_no_relevant_docs() {
        let r = compute_recall_at_k(&[doc(1)], &[], 5);
        assert!((r - 0.0).abs() < 1e-10);
    }
}
