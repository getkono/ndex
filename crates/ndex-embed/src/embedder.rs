//! ONNX-backed embedding inference (PRD §4.7).

use std::path::Path;

use ndex_core::error::Result;
use ndex_core::model::Embedding;

use crate::tokenizer::Tokenizer;

/// Produces embeddings for a batch of texts.
pub trait Embed {
    /// Embed a batch of document/query strings into MRL-truncated, L2-normalized vectors.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>>;
}

/// The ONNX embedder: an `ort::Session` plus the model's [`Tokenizer`].
///
/// `ort::Session` is `Send + Sync` and `run`s with `&self`, so a single `Embedder` is
/// shared by the embed thread for batched inference (PRD §4.7).
pub struct Embedder {
    session: ort::session::Session,
    tokenizer: Tokenizer,
}

impl Embedder {
    /// Load the ONNX model and tokenizer from a model directory, applying the configured
    /// intra-/inter-op thread counts and CPU execution provider (PRD §4.7).
    pub fn load(
        model_dir: &Path,
        intra_op_threads: usize,
        inter_op_threads: usize,
    ) -> Result<Self> {
        let _ = (model_dir, intra_op_threads, inter_op_threads);
        todo!()
    }
}

impl Embed for Embedder {
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        // TODO(skeleton): tokenize → ort run → 768→256 MRL truncate → L2-normalize → f16.
        let _ = texts;
        todo!()
    }
}
