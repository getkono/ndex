//! Characterization tests for the public `ndex-search` interface.
//!
//! Everything here is REAL and exercised live: `mode::resolve` and the `fuse` math as pure
//! functions, `query::embed_query` via a recording fake `Embed`, and `search::run` end-to-end
//! over a `Store` fixture populated through the FTS writer. v0.1 retrieval is FTS-only;
//! semantic/hybrid retrieval is a follow-up (explicit `semantic` with an empty vector index
//! returns zero hits with a warning — PRD §16.3).

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
// Mode resolution (PRD §10.7, §16.3) — exhaustive heuristic table + warnings.
// ---------------------------------------------------------------------------

/// Exact warning strings (owned by `mode.rs`, pinned here — the CLI prints them to stderr).
const WARN_SEMANTIC: &str = "Vector index is empty; semantic search returned no results. Run \
     `ndex index` to build it, or use `--mode auto` to fall back to full-text search.";
const WARN_HYBRID: &str = "Vector index is empty; hybrid search fell back to full-text only. \
     Run `ndex index` to enable semantic retrieval.";
const WARN_AUTO: &str = "Vector index is empty; semantic ranking skipped (results are full-text \
     only). Run `ndex index` to enable it.";

#[test]
fn explicit_modes_pass_through_when_vectors_present() {
    for m in [SearchMode::Fts, SearchMode::Semantic, SearchMode::Hybrid] {
        let r = resolve("anything", m, false);
        assert_eq!(r.mode, m);
        assert!(r.warnings.is_empty());
    }
}

#[test]
fn semantic_without_vectors_stays_semantic_with_warning() {
    // PRD §16.3: explicit semantic never silently serves BM25 — it stays `Semantic`
    // (zero hits, see `run_explicit_semantic_without_vectors_returns_zero_hits`) and warns.
    let r = resolve("x", SearchMode::Semantic, true);
    assert_eq!(r.mode, SearchMode::Semantic);
    assert_eq!(r.warnings, vec![WARN_SEMANTIC.to_owned()]);
}

#[test]
fn hybrid_without_vectors_falls_back_to_fts_with_warning() {
    let r = resolve("x", SearchMode::Hybrid, true);
    assert_eq!(r.mode, SearchMode::Fts);
    assert_eq!(r.warnings, vec![WARN_HYBRID.to_owned()]);
    // Explicit FTS is unaffected — no fallback, no warning.
    let fts = resolve("x", SearchMode::Fts, true);
    assert_eq!(fts.mode, SearchMode::Fts);
    assert!(fts.warnings.is_empty());
}

#[test]
fn auto_short_keyword_queries_pick_fts() {
    assert_eq!(
        resolve("blake3", SearchMode::Auto, false).mode,
        SearchMode::Fts
    );
    assert_eq!(
        resolve("config.toml", SearchMode::Auto, false).mode,
        SearchMode::Fts
    );
    assert_eq!(
        resolve("usearch hnsw", SearchMode::Auto, false).mode,
        SearchMode::Fts
    );
}

#[test]
fn auto_phrases_and_operators_pick_fts() {
    assert_eq!(
        resolve("\"exact phrase\"", SearchMode::Auto, false).mode,
        SearchMode::Fts
    );
    assert_eq!(
        resolve("invoice AND 2024", SearchMode::Auto, false).mode,
        SearchMode::Fts
    );
    assert_eq!(
        resolve("a OR b", SearchMode::Auto, false).mode,
        SearchMode::Fts
    );
    assert_eq!(
        resolve("mime:application/pdf", SearchMode::Auto, false).mode,
        SearchMode::Fts
    );
}

#[test]
fn auto_natural_language_picks_hybrid() {
    let r = resolve(
        "how do I configure the embedding model",
        SearchMode::Auto,
        false,
    );
    assert_eq!(r.mode, SearchMode::Hybrid);
    assert!(r.warnings.is_empty());
    // Short but stop-word present ⇒ natural-language intent ⇒ hybrid.
    assert_eq!(
        resolve("what is blake3", SearchMode::Auto, false).mode,
        SearchMode::Hybrid
    );
}

#[test]
fn auto_with_empty_vectors_always_picks_fts_with_warning() {
    let r = resolve(
        "how do I configure the embedding model",
        SearchMode::Auto,
        true,
    );
    assert_eq!(r.mode, SearchMode::Fts);
    assert_eq!(r.warnings, vec![WARN_AUTO.to_owned()]);
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
    assert!(o.warnings.is_empty());
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
// Query embedding (PRD §10.7) — via a recording fake `Embed`.
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
fn embed_query_applies_asymmetric_prefix() {
    let fake = RecordingEmbed {
        seen: Mutex::new(vec![]),
    };
    let _ = ndex_search::embed_query(&fake, "quarterly earnings").unwrap();
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.as_slice(), &["query: quarterly earnings".to_string()]);
}

// ---------------------------------------------------------------------------
// End-to-end `run` over a real store (PRD §10.7, §16.3) — totals, pagination,
// warnings, semantic zero-hit policy. FTS-only in v0.1.
// ---------------------------------------------------------------------------

fn test_identity() -> ndex_core::identity::IndexIdentity {
    use ndex_core::identity::{
        EmbeddingIdentity, FtsIdentity, Hashing, Identity, IndexIdentity, SCHEMA_VERSION,
    };
    IndexIdentity {
        identity: Identity {
            schema_version: SCHEMA_VERSION,
            created_by: "test".into(),
            created_at: "2026-06-19T00:00:00Z".into(),
        },
        embedding: EmbeddingIdentity {
            model_name: ndex_core::constants::DEFAULT_MODEL.into(),
            model_hash: "abc".into(),
            dimensions: 768,
            mrl_dimensions: 256,
            vector_scalar: "f16".into(),
            hnsw_m: 32,
            hnsw_ef_construction: 200,
        },
        hashing: Hashing {
            algorithm: "blake3".into(),
        },
        fts: FtsIdentity {
            tokenizer_version: 1,
        },
    }
}

fn chunk(chunk_ord: u32, text: &str) -> ndex_core::model::Chunk {
    ndex_core::model::Chunk {
        file_id: 0,
        chunk_ord,
        byte_start: 0,
        byte_end: text.len() as u64,
        block_type: ndex_core::model::BlockType::Paragraph,
        text: text.into(),
    }
}

fn fts_meta(path_text: &str) -> ndex_store::fts::FtsFileMeta<'_> {
    ndex_store::fts::FtsFileMeta {
        mime: "text/plain",
        lang: Some("eng"),
        path_text,
        size: 1024,
        mtime_ns: 1_700_000_000_000_000_000,
        title: None,
    }
}

/// A store whose FTS index holds `n` single-chunk files all matching the term `needle`.
/// The vector index is absent (v0.1), so `vectors_empty` is true inside `run`.
fn needle_store(n: i64) -> (tempfile::TempDir, ndex_store::Store) {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = ndex_store::Store::create(
        tmp.path(),
        test_identity(),
        ndex_core::config::Config::default(),
    )
    .unwrap();
    for id in 1..=n {
        store
            .fts
            .add_chunk(id, &chunk(0, "needle in a haystack"), &fts_meta("f.txt"))
            .unwrap();
    }
    store.fts.commit().unwrap();
    (tmp, store)
}

#[test]
fn run_reports_true_total_and_truncation() {
    let (_tmp, store) = needle_store(5);
    let filters = SearchFilters::default();

    // First page: `total` is the corpus-wide match count, not the fetched window.
    let out = ndex_search::run(&store, None, "needle", SearchMode::Fts, &filters, 2, 0).unwrap();
    assert_eq!(out.hits.len(), 2);
    assert_eq!(out.total, 5);
    assert!(out.truncated);
    assert_eq!(out.mode, SearchMode::Fts);
    assert!(out.warnings.is_empty()); // explicit FTS never warns

    // Last page: offset + hits.len() == total ⇒ not truncated.
    let last = ndex_search::run(&store, None, "needle", SearchMode::Fts, &filters, 2, 4).unwrap();
    assert_eq!(last.hits.len(), 1);
    assert_eq!(last.total, 5);
    assert!(!last.truncated);

    // Offset past the end: empty page, real total, nothing further.
    let past = ndex_search::run(&store, None, "needle", SearchMode::Fts, &filters, 2, 10).unwrap();
    assert!(past.hits.is_empty());
    assert_eq!(past.total, 5);
    assert!(!past.truncated);
}

#[test]
fn run_with_zero_limit_is_a_count_query() {
    let (_tmp, store) = needle_store(3);
    let filters = SearchFilters::default();

    let out = ndex_search::run(&store, None, "needle", SearchMode::Fts, &filters, 0, 0).unwrap();
    assert!(out.hits.is_empty());
    assert_eq!(out.total, 3);
    assert!(out.truncated); // matches exist beyond the (empty) page

    let none = ndex_search::run(&store, None, "nomatch", SearchMode::Fts, &filters, 0, 0).unwrap();
    assert!(none.hits.is_empty());
    assert_eq!(none.total, 0);
    assert!(!none.truncated);
}

#[test]
fn run_explicit_semantic_without_vectors_returns_zero_hits() {
    let (_tmp, store) = needle_store(3);
    let filters = SearchFilters::default();
    let out = ndex_search::run(
        &store,
        None,
        "needle",
        SearchMode::Semantic,
        &filters,
        10,
        0,
    )
    .unwrap();
    // PRD §16.3: no silent BM25 substitution — the mode stays honest and nothing runs.
    assert_eq!(out.mode, SearchMode::Semantic);
    assert!(out.hits.is_empty());
    assert_eq!(out.total, 0);
    assert!(!out.truncated);
    assert_eq!(out.warnings, vec![WARN_SEMANTIC.to_owned()]);
}

#[test]
fn run_auto_and_hybrid_without_vectors_serve_fts_with_warning() {
    let (_tmp, store) = needle_store(2);
    let filters = SearchFilters::default();

    let auto = ndex_search::run(&store, None, "needle", SearchMode::Auto, &filters, 10, 0).unwrap();
    assert_eq!(auto.mode, SearchMode::Fts);
    assert_eq!(auto.hits.len(), 2);
    assert_eq!(auto.warnings, vec![WARN_AUTO.to_owned()]);

    let hybrid =
        ndex_search::run(&store, None, "needle", SearchMode::Hybrid, &filters, 10, 0).unwrap();
    assert_eq!(hybrid.mode, SearchMode::Fts);
    assert_eq!(hybrid.hits.len(), 2);
    assert_eq!(hybrid.warnings, vec![WARN_HYBRID.to_owned()]);
}
