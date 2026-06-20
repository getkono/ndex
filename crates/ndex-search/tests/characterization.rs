//! Characterization tests for the public `ndex-search` interface.
//!
//! `mode::resolve` and the `fuse` math are REAL and exercised live. `query::embed_query` and
//! `search::run` are `todo!()`; their contracts are pinned by `#[ignore = "impl pending: PR #3"]`
//! tests that still compile against the real signatures.

use std::sync::Mutex;

use ndex_core::error::Result;
use ndex_core::model::Embedding;
use ndex_core::{SearchFilters, SearchMode};
use ndex_embed::Embed;
use ndex_search::{Hit, ScoreExplain, SearchOutcome, min_max_normalize, resolve, rrf_score};

// ---------------------------------------------------------------------------
// RRF fusion (PRD §10.7).
// ---------------------------------------------------------------------------

#[test]
fn rrf_rewards_presence_in_both_lists() {
    let both = rrf_score(Some(1), Some(1), 60, 1.0);
    assert!(both > rrf_score(Some(1), None, 60, 1.0));
    assert!(both > rrf_score(None, Some(1), 60, 1.0));
    assert_eq!(rrf_score(None, None, 60, 1.0), 0.0);
}

#[test]
fn rrf_better_rank_scores_higher() {
    // Rank 1 (best) must beat rank 10 in the same list.
    assert!(rrf_score(Some(1), None, 60, 1.0) > rrf_score(Some(10), None, 60, 1.0));
}

#[test]
fn rrf_weight_scales_only_the_fts_term() {
    let base = rrf_score(Some(1), None, 60, 1.0);
    let heavy = rrf_score(Some(1), None, 60, 2.0);
    assert!((heavy - 2.0 * base).abs() < 1e-6);
    // The semantic-only term is unaffected by fts_weight.
    let sem = rrf_score(None, Some(3), 60, 1.0);
    assert!((rrf_score(None, Some(3), 60, 9.0) - sem).abs() < 1e-6);
}

#[test]
fn rrf_larger_k_flattens_scores() {
    assert!(rrf_score(Some(1), None, 10, 1.0) > rrf_score(Some(1), None, 1000, 1.0));
}

// ---------------------------------------------------------------------------
// Min-max normalization (PRD §10.7).
// ---------------------------------------------------------------------------

#[test]
fn normalize_spreads_to_unit_range() {
    let mut s = [1.0, 2.0, 3.0];
    min_max_normalize(&mut s);
    assert_eq!(s, [0.0, 0.5, 1.0]);
}

#[test]
fn normalize_ties_and_singletons_map_to_one() {
    let mut tie = [5.0, 5.0, 5.0];
    min_max_normalize(&mut tie);
    assert_eq!(tie, [1.0, 1.0, 1.0]);

    let mut single = [42.0];
    min_max_normalize(&mut single);
    assert_eq!(single, [1.0]);

    let mut empty: [f32; 0] = [];
    min_max_normalize(&mut empty); // must not panic
}

#[test]
fn normalize_handles_negative_scores() {
    let mut s = [-2.0, 0.0, 2.0];
    min_max_normalize(&mut s);
    assert_eq!(s, [0.0, 0.5, 1.0]);
}

#[test]
fn score_explain_defaults_to_all_absent() {
    let e = ScoreExplain::default();
    assert_eq!(
        e,
        ScoreExplain {
            bm25: None,
            cosine: None,
            rrf: None
        }
    );
}

// ---------------------------------------------------------------------------
// Mode resolution (PRD §10.7) — exhaustive heuristic table.
// ---------------------------------------------------------------------------

#[test]
fn explicit_modes_pass_through_when_vectors_present() {
    for m in [SearchMode::Fts, SearchMode::Semantic, SearchMode::Hybrid] {
        assert_eq!(resolve("anything", m, false), m);
    }
}

#[test]
fn semantic_and_hybrid_fall_back_to_fts_without_vectors() {
    assert_eq!(resolve("x", SearchMode::Semantic, true), SearchMode::Fts);
    assert_eq!(resolve("x", SearchMode::Hybrid, true), SearchMode::Fts);
    // Explicit FTS is unaffected.
    assert_eq!(resolve("x", SearchMode::Fts, true), SearchMode::Fts);
}

#[test]
fn auto_short_keyword_queries_pick_fts() {
    assert_eq!(resolve("blake3", SearchMode::Auto, false), SearchMode::Fts);
    assert_eq!(
        resolve("config.toml", SearchMode::Auto, false),
        SearchMode::Fts
    );
    assert_eq!(
        resolve("usearch hnsw", SearchMode::Auto, false),
        SearchMode::Fts
    );
}

#[test]
fn auto_phrases_and_operators_pick_fts() {
    assert_eq!(
        resolve("\"exact phrase\"", SearchMode::Auto, false),
        SearchMode::Fts
    );
    assert_eq!(
        resolve("invoice AND 2024", SearchMode::Auto, false),
        SearchMode::Fts
    );
    assert_eq!(resolve("a OR b", SearchMode::Auto, false), SearchMode::Fts);
    assert_eq!(
        resolve("mime:application/pdf", SearchMode::Auto, false),
        SearchMode::Fts
    );
}

#[test]
fn auto_natural_language_picks_hybrid() {
    assert_eq!(
        resolve(
            "how do I configure the embedding model",
            SearchMode::Auto,
            false
        ),
        SearchMode::Hybrid
    );
    // Short but stop-word present ⇒ natural-language intent ⇒ hybrid.
    assert_eq!(
        resolve("what is blake3", SearchMode::Auto, false),
        SearchMode::Hybrid
    );
}

#[test]
fn auto_with_empty_vectors_always_picks_fts() {
    assert_eq!(
        resolve(
            "how do I configure the embedding model",
            SearchMode::Auto,
            true
        ),
        SearchMode::Fts
    );
}

// ---------------------------------------------------------------------------
// Engine result types.
// ---------------------------------------------------------------------------

#[test]
fn search_outcome_default_is_empty_auto() {
    let o = SearchOutcome::default();
    assert!(o.hits.is_empty());
    assert_eq!(o.total, 0);
    assert_eq!(o.mode, SearchMode::Auto);
    assert!(!o.truncated);
}

#[test]
fn hit_is_constructible_and_comparable() {
    let h = Hit {
        file_id: 7,
        chunk_ord: 2,
        score: 1.0,
        score_raw: 0.34,
        score_fts: Some(0.2),
        score_vec: None,
        byte_start: 0,
        byte_end: 100,
    };
    assert_eq!(h.clone(), h);
}

// ---------------------------------------------------------------------------
// todo!() contracts (PR #3 targets).
// ---------------------------------------------------------------------------

/// An `Embed` impl that records the exact strings it was asked to embed.
struct RecordingEmbed {
    seen: Mutex<Vec<String>>,
}
impl Embed for RecordingEmbed {
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        self.seen.lock().unwrap().extend(texts.iter().cloned());
        // An empty vector needs no `half` import; dims() == 0 is enough for the contract.
        Ok(texts.iter().map(|_| Embedding(vec![])).collect())
    }
}

#[test]
#[ignore = "impl pending: PR #3"]
fn embed_query_applies_asymmetric_prefix() {
    let fake = RecordingEmbed {
        seen: Mutex::new(vec![]),
    };
    let _ = ndex_search::embed_query(&fake, "quarterly earnings").unwrap();
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.as_slice(), &["query: quarterly earnings".to_string()]);
}

#[test]
#[ignore = "impl pending: PR #3"]
fn run_returns_ranked_hits_in_resolved_mode() {
    // Spec: over a populated index, `run` resolves the mode, retrieves, fuses, applies the limit,
    // and reports the resolved mode + truncation. Needs Store::create/open (also todo).
    let tmp = tempfile::tempdir().unwrap();
    let store = ndex_store::Store::open(tmp.path()).unwrap();
    let filters = SearchFilters::default();
    let outcome =
        ndex_search::run(&store, None, "hello", SearchMode::Fts, &filters, 10, 0).unwrap();
    assert!(outcome.hits.len() <= 10);
    assert_eq!(outcome.mode, SearchMode::Fts);
}
