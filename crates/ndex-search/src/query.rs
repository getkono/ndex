//! Query preparation for FTS and semantic search (PRD §4.7, §10.7).

use ndex_core::error::{NdexError, Result};
use ndex_core::model::Embedding;
use ndex_embed::{Embed, Tokenizer};

/// Embed a search query into a vector, applying the asymmetric `"query: "` prefix before
/// inference (PRD §4.7). Truncation to the model's max query length happens inside the embedder.
pub fn embed_query(embedder: &dyn Embed, query: &str) -> Result<Embedding> {
    let prefixed = Tokenizer::with_query_prefix(query);
    embedder
        .embed_batch(&[prefixed])?
        .into_iter()
        .next()
        .ok_or_else(|| NdexError::Model("embedder returned no vectors for the query".into()))
}
