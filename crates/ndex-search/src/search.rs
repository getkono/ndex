//! Search orchestration over the store + embedder (PRD §10.7).

use ndex_core::error::Result;
use ndex_core::{SearchFilters, SearchMode};
use ndex_embed::Embed;
use ndex_store::Store;

use crate::mode::Resolution;

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

/// The outcome of a search: ranked hits, the true match count, the mode that actually ran,
/// and any user-facing warnings (PRD §12.7, §16.3).
#[derive(Debug, Clone, Default)]
pub struct SearchOutcome {
    pub hits: Vec<Hit>,
    /// Corpus-wide match count (tantivy `Count` collector), independent of `limit`/`offset`.
    pub total: u64,
    /// The mode retrieval actually executed. Explicit `Semantic` with an empty vector index
    /// stays `Semantic` and returns zero hits — no silent FTS substitution (PRD §16.3).
    pub mode: SearchMode,
    /// `true` when matches exist beyond this page: `offset + hits.len() < total`.
    pub truncated: bool,
    /// User-facing warnings (e.g. the empty-vector fallback notices from
    /// [`crate::mode::resolve`]); callers surface these to the user (stderr in the CLI).
    pub warnings: Vec<String>,
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
    // v0.1 retrieves via FTS; hybrid/auto degrade to FTS while the vector index is absent
    // (PRD §16.3). `embedder`/`filters` are reserved for the semantic + filtered-search follow-up.
    let _ = (embedder, filters);
    let vectors_empty = store.vectors.as_ref().is_none_or(|v| v.is_empty());
    let Resolution { mode, warnings } = crate::mode::resolve(query, requested, vectors_empty);

    // Explicit semantic with no vectors runs nothing: zero hits, an honest `Semantic` mode,
    // and a warning explaining why (PRD §16.3) — never a silent BM25 substitution.
    if mode == SearchMode::Semantic && vectors_empty {
        return Ok(SearchOutcome {
            mode,
            warnings,
            ..SearchOutcome::default()
        });
    }

    // Fetch a window just deep enough to serve the requested page (tantivy's `TopDocs`
    // requires a limit ≥ 1, even when `limit == 0`). `total` is the true corpus-wide match
    // count from the `Count` collector — independent of the window size.
    let fetch = limit.saturating_add(offset).max(1);
    let (fts_hits, total) =
        store
            .fts
            .search_with_total(query, fetch, store.config.search.title_boost)?;

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

    let hits: Vec<Hit> = all.into_iter().skip(offset).take(limit).collect();
    // More matches exist past this page. With `limit == 0` the page is empty, `total` is
    // still the real count, and (at `offset == 0`) `truncated ⇔ total > 0`.
    let truncated = (offset as u64).saturating_add(hits.len() as u64) < total;
    Ok(SearchOutcome {
        hits,
        total,
        mode,
        truncated,
        warnings,
    })
}
