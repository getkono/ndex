//! Characterization tests for the public `ndex-embed` interface.
//!
//! The model registry and the pure tokenizer helpers are REAL. ONNX inference, tokenizer
//! loading/encoding, and model fetch/verify/import/delete are `todo!()`; their contracts are
//! pinned by `#[ignore = "impl pending: PR #3"]` tests that compile against the real signatures.

use std::path::Path;

use ndex_core::constants::DEFAULT_MODEL;
use ndex_embed::model::{self, ModelInfo};
use ndex_embed::{Embedder, MAX_QUERY_TOKENS, REGISTRY, Tokenizer, list, lookup};

// ---------------------------------------------------------------------------
// Model registry (PRD §7.4) — all real.
// ---------------------------------------------------------------------------

#[test]
fn registry_ships_arctic_only_in_v0_1() {
    assert_eq!(REGISTRY.len(), 1);
    assert_eq!(list().len(), 1);
    let m: &ModelInfo = &REGISTRY[0];
    assert_eq!(m.shortname, "arctic");
    assert_eq!(m.full_name, DEFAULT_MODEL);
    assert_eq!(m.dimensions, 768);
    assert_eq!(m.mrl_dimensions, 256);
    assert!(m.languages >= 1);
}

#[test]
fn lookup_by_shortname_or_full_name() {
    assert_eq!(lookup("arctic").unwrap().full_name, DEFAULT_MODEL);
    assert!(lookup(DEFAULT_MODEL).is_some());
    assert!(lookup("nope").is_none());
}

#[test]
fn models_dir_is_under_dot_ndex() {
    let dir = model::models_dir().unwrap();
    assert!(dir.ends_with("models"), "{dir:?}");
    assert!(dir.to_string_lossy().contains(".ndex"));
}

#[test]
fn model_path_is_models_dir_joined_with_full_name() {
    let info = lookup("arctic").unwrap();
    let path = model::model_path(info).unwrap();
    assert!(path.ends_with(info.full_name), "{path:?}");
    assert_eq!(path.parent().unwrap(), model::models_dir().unwrap());
}

// ---------------------------------------------------------------------------
// Tokenizer pure helpers (PRD §4.7) — all real.
// ---------------------------------------------------------------------------

#[test]
fn query_prefix_is_the_asymmetric_marker() {
    assert_eq!(
        Tokenizer::with_query_prefix("quarterly earnings"),
        "query: quarterly earnings"
    );
    assert_eq!(Tokenizer::with_query_prefix(""), "query: ");
}

#[test]
fn truncate_caps_to_max_and_leaves_shorter_alone() {
    assert_eq!(Tokenizer::truncate(vec![1, 2, 3, 4, 5], 3), vec![1, 2, 3]);
    assert_eq!(Tokenizer::truncate(vec![1, 2], 5), vec![1, 2]);
    assert_eq!(Tokenizer::truncate(vec![1, 2, 3], 0), Vec::<u32>::new());
    assert_eq!(Tokenizer::truncate(vec![], 5), Vec::<u32>::new());
}

#[test]
fn max_query_tokens_matches_model_limit() {
    assert_eq!(MAX_QUERY_TOKENS, 512);
}

// ---------------------------------------------------------------------------
// todo!() contracts (PR #3 targets).
// ---------------------------------------------------------------------------

#[test]
#[ignore = "impl pending: PR #3"]
fn tokenizer_load_encode_and_count_agree() {
    // Spec: load a model's tokenizer.json, encode text to non-empty ids, and `count` (the
    // TokenCounter impl) equals the encoded length.
    use ndex_core::tokens::TokenCounter;
    let tok = Tokenizer::load(Path::new("tests/fixtures/tokenizer.json")).unwrap();
    let ids = tok.encode("hello world").unwrap();
    assert!(!ids.is_empty());
    assert_eq!(tok.count("hello world"), ids.len());
}

#[test]
#[ignore = "impl pending: PR #3"]
fn embedder_produces_mrl_truncated_normalized_vectors() {
    // Spec: embed_batch returns one 256-dim (MRL) L2-normalized vector per input.
    let dir = Path::new("tests/fixtures/model");
    let embedder = Embedder::load(dir, 1, 1).unwrap();
    use ndex_embed::Embed;
    let out = embedder.embed_batch(&["query: hello".to_string()]).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].dims(), 256);
    let norm: f32 = out[0]
        .0
        .iter()
        .map(|h| f32::from(*h).powi(2))
        .sum::<f32>()
        .sqrt();
    assert!(
        (norm - 1.0).abs() < 1e-2,
        "vector should be L2-normalized, got {norm}"
    );
}

#[test]
#[ignore = "impl pending: PR #3"]
fn verify_reports_integrity_against_registry_hashes() {
    // Spec: a model that has not been fetched fails verification rather than erroring out hard.
    let info = lookup("arctic").unwrap();
    assert!(!model::verify(info).unwrap());
}
