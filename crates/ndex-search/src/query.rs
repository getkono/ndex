//! Query preparation for FTS and semantic search (PRD §4.7, §10.7).

use ndex_core::error::Result;
use ndex_core::model::Embedding;
use ndex_embed::Embed;

/// Embed a search query into a vector, applying the asymmetric `"query: "` prefix and
/// truncating to the model's max query length before inference (PRD §4.7).
pub fn embed_query(embedder: &dyn Embed, query: &str) -> Result<Embedding> {
    // TODO(skeleton): Tokenizer::with_query_prefix → embedder.embed_batch(&[prefixed])[0].
    let _ = (embedder, query);
    todo!()
}
