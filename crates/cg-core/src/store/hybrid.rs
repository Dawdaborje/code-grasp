//! Reciprocal Rank Fusion (RRF) for dense + sparse retrieval lists.

use std::collections::HashMap;

const RRF_K: f64 = 60.0;

/// Fuse two ranked lists of chunk ids (1-based rank order) into a single score per id.
pub fn reciprocal_rank_fusion(dense_order: &[i64], sparse_order: &[i64]) -> Vec<(i64, f64)> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for (i, &id) in dense_order.iter().enumerate() {
        let rank = (i + 1) as f64;
        *scores.entry(id).or_insert(0.0) += 1.0 / (RRF_K + rank);
    }
    for (i, &id) in sparse_order.iter().enumerate() {
        let rank = (i + 1) as f64;
        *scores.entry(id).or_insert(0.0) += 1.0 / (RRF_K + rank);
    }
    let mut v: Vec<(i64, f64)> = scores.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v
}

#[cfg(test)]
mod tests {
    use super::reciprocal_rank_fusion;

    #[test]
    fn rrf_prefers_top_of_both_lists() {
        let dense = vec![10, 20, 30];
        let sparse = vec![20, 40, 10];
        let fused = reciprocal_rank_fusion(&dense, &sparse);
        assert!(!fused.is_empty());
        // id 20 appears high in both lists → should rank first
        assert_eq!(fused[0].0, 20);
    }
}
