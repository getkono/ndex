//! Hybrid-mode score fusion and display normalization (PRD §10.7).

/// Per-component score breakdown for `--explain` (PRD §10.7).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScoreExplain {
    pub bm25: Option<f32>,
    pub cosine: Option<f32>,
    pub rrf: Option<f32>,
}

/// Reciprocal Rank Fusion score for a document (PRD §10.7).
///
/// `rank_*` are 1-based ranks (`None` if the document is absent from that result list). The FTS
/// term is scaled by `fts_weight`:
/// `fts_weight / (k + rank_fts) + 1 / (k + rank_semantic)`.
pub fn rrf_score(
    rank_fts: Option<usize>,
    rank_semantic: Option<usize>,
    k: u32,
    fts_weight: f32,
) -> f32 {
    let k = k as f32;
    let term = |rank: Option<usize>| rank.map_or(0.0, |r| 1.0 / (k + r as f32));
    fts_weight * term(rank_fts) + term(rank_semantic)
}

/// Min-max normalize scores into `[0, 1]` for display (PRD §10.7). Ties / single elements map
/// to `1.0`. Raw scores are preserved separately by the caller (`score_raw`).
pub fn min_max_normalize(scores: &mut [f32]) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &s in scores.iter() {
        min = min.min(s);
        max = max.max(s);
    }
    let range = max - min;
    if range <= f32::EPSILON {
        for s in scores.iter_mut() {
            *s = 1.0;
        }
        return;
    }
    for s in scores.iter_mut() {
        *s = (*s - min) / range;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_rewards_presence_in_both_lists() {
        let both = rrf_score(Some(1), Some(1), 60, 1.0);
        let fts_only = rrf_score(Some(1), None, 60, 1.0);
        let sem_only = rrf_score(None, Some(1), 60, 1.0);
        assert!(both > fts_only);
        assert!(both > sem_only);
        // Absent from both ⇒ zero.
        assert_eq!(rrf_score(None, None, 60, 1.0), 0.0);
    }

    #[test]
    fn rrf_fts_weight_scales_fts_term() {
        let base = rrf_score(Some(1), None, 60, 1.0);
        let heavy = rrf_score(Some(1), None, 60, 2.0);
        assert!((heavy - 2.0 * base).abs() < 1e-6);
    }

    #[test]
    fn normalize_to_unit_range() {
        let mut s = [1.0, 2.0, 3.0];
        min_max_normalize(&mut s);
        assert_eq!(s, [0.0, 0.5, 1.0]);
    }

    #[test]
    fn normalize_handles_ties() {
        let mut s = [5.0, 5.0, 5.0];
        min_max_normalize(&mut s);
        assert_eq!(s, [1.0, 1.0, 1.0]);
    }
}
