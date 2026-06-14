use std::collections::HashMap;

pub const DEFAULT_RRF_K: u64 = 60;

pub fn reciprocal_rank_fusion(list_a: &[String], list_b: &[String], k: u64) -> Vec<(String, f32)> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for (rank, id) in list_a.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k as f32 + rank as f32 + 1.0);
    }
    for (rank, id) in list_b.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k as f32 + rank as f32 + 1.0);
    }
    let mut fused: Vec<(String, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

pub fn reciprocal_rank_fusion_multi(lists: &[&[String]], k: u64) -> Vec<(String, f32)> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k as f32 + rank as f32 + 1.0);
        }
    }
    let mut fused: Vec<(String, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_basic() {
        let a = vec!["a".into(), "b".into(), "c".into()];
        let b = vec!["b".into(), "a".into(), "d".into()];
        let fused = reciprocal_rank_fusion(&a, &b, 60);
        assert_eq!(fused.len(), 4);
        assert!(fused[0].1 > fused[2].1);
    }

    #[test]
    fn test_rrf_multi() {
        let a = vec!["a".into(), "b".into()];
        let b = vec!["b".into(), "c".into()];
        let c = vec!["a".into(), "c".into()];
        let fused = reciprocal_rank_fusion_multi(&[&a, &b, &c], 60);
        assert_eq!(fused.len(), 3);
    }
}
