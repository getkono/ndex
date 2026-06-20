//! Search orchestration over the store + embedder (PRD §10.7).

use ndex_core::error::Result;
use ndex_core::{SearchFilters, SearchMode};
use ndex_embed::Embed;
use ndex_store::Store;

/// One engine-level hit. The server enriches this into a wire `SearchHit` by joining the
/// manifest (path, mime, size, mtime) and generating a snippet.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub file_id: i64,
    pub chunk_ord: u32,
    /// Display score, min-max normalized to `[0, 1]`.
    pub score: f32,
    /// Raw fused/BM25/cosine score.
    pub score_raw: f32,
    /// BM25 component (with `--explain`).
    pub score_fts: Option<f32>,
    /// Cosine component (with `--explain`).
    pub score_vec: Option<f32>,
    pub byte_start: u64,
    pub byte_end: u64,
}

/// The outcome of a search: ranked hits plus the resolved mode (PRD §12.7).
#[derive(Debug, Clone, Default)]
pub struct SearchOutcome {
    pub hits: Vec<Hit>,
    pub total: u64,
    pub mode: SearchMode,
    pub truncated: bool,
}

/// Run a search: resolve the mode ([`crate::mode::resolve`]), execute FTS and/or semantic
/// retrieval through the store, fuse with RRF for hybrid ([`crate::fuse`]), apply filters, and
/// return ranked engine hits (PRD §10.7).
pub fn run(
    store: &Store,
    embedder: Option<&dyn Embed>,
    query: &str,
    requested: SearchMode,
    filters: &SearchFilters,
    limit: usize,
    offset: usize,
) -> Result<SearchOutcome> {
    // v0.1 retrieves via FTS; semantic/hybrid degrade to FTS while the vector index is absent
    // (PRD §16.3). `embedder`/`filters` are reserved for the semantic + filtered-search follow-up.
    let _ = (embedder, filters);
    let vectors_empty = store.vectors.as_ref().is_none_or(|v| v.is_empty());
    let mode = crate::mode::resolve(query, requested, vectors_empty);

    let fetch = limit.saturating_add(offset).max(1);
    let fts_hits = store
        .fts
        .search(query, fetch, store.config.search.title_boost)?;

    let raw: Vec<f32> = fts_hits.iter().map(|h| h.score).collect();
    let mut display = raw.clone();
    crate::fuse::min_max_normalize(&mut display);

    let all: Vec<Hit> = fts_hits
        .iter()
        .zip(&display)
        .zip(&raw)
        .map(|((h, &score), &score_raw)| Hit {
            file_id: h.file_id,
            chunk_ord: h.chunk_ord,
            score,
            score_raw,
            score_fts: Some(score_raw),
            score_vec: None,
            byte_start: h.byte_start,
            byte_end: h.byte_end,
        })
        .collect();

    let total = all.len() as u64;
    let hits: Vec<Hit> = all.into_iter().skip(offset).take(limit).collect();
    let truncated = total > offset as u64 + hits.len() as u64;
    Ok(SearchOutcome {
        hits,
        total,
        mode,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "skeleton: end-to-end search over a fixture index (fts/semantic/hybrid)"]
    fn search_round_trip() {
        todo!()
    }
}
