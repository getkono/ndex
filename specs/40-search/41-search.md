# 41 — Search (`ndex-search`)

**Owns:** The `ndex-search` crate — `auto`-mode resolution rules (including the stop-word list), query preparation for FTS and semantic retrieval, search execution/pagination (the engine-level `Hit`/`SearchOutcome` types and the fetch-depth rule), and hybrid score fusion (the RRF formula and display normalization).

**Sources:**
- `crates/ndex-search/src/lib.rs`
- `crates/ndex-search/src/mode.rs`
- `crates/ndex-search/src/query.rs`
- `crates/ndex-search/src/search.rs`
- `crates/ndex-search/src/fuse.rs`
- `crates/ndex-search/tests/characterization.rs`
- `crates/ndex-search/Cargo.toml`
- PRD §10.7 (search scoring), §16.3 (empty-vector-index decision)

## 1. Crate role and data flow

`ndex-search` is the retrieval engine between the store and the server:

```
query + requested mode
  → mode::resolve            (auto heuristics, empty-vector fallback)
  → query prep               (FTS: raw string; semantic: QUERY_PREFIX-prefixed embedding)
  → retrieval via ndex-store (tantivy FTS / usearch ANN)
  → fuse::rrf_score          (hybrid only)
  → fuse::min_max_normalize  (display scores)
  → paginate → SearchOutcome
```

It returns engine-level `Hit`s keyed by `(file_id, chunk_ord)`; the **server** joins the manifest (path, mime, size, mtime) and generates snippets to build wire `SearchHit`s ([63-remote.md](../60-interfaces/63-remote.md), message shapes in [53-messages.md](../50-protocol/53-messages.md)). Dependencies: `ndex-core`, `ndex-store`, `ndex-embed`.

Types referenced but owned elsewhere: `SearchMode` / `SearchFilters` ([15-search-and-progress-types.md](../10-core/15-search-and-progress-types.md)), the tantivy schema, BM25 parameters, and field-boost application ([23-fts.md](../20-store/23-fts.md)), the usearch index and cosine/inner-product scoring ([24-vectors.md](../20-store/24-vectors.md)), `Embedding` ([11-data-model.md](../10-core/11-data-model.md)), config defaults `search.{default_limit,rrf_k,title_boost,fts_weight,ef_search}` ([13-config.md](../10-core/13-config.md)).

## 2. Mode resolution (`mode.rs`) — ✅ implemented

`resolve(query, requested, vectors_empty) -> SearchMode` is a pure function. `vectors_empty` is supplied by the caller (in `search::run`: vector index absent **or** zero entries — see [24-vectors.md](../20-store/24-vectors.md)).

### 2.1 Rules (owned here)

Explicit modes:

| Requested | `vectors_empty = false` | `vectors_empty = true` |
|---|---|---|
| `Fts` | `Fts` | `Fts` |
| `Semantic` | `Semantic` | `Fts` (fallback) |
| `Hybrid` | `Hybrid` | `Fts` (fallback) |
| `Auto` | heuristics below | `Fts` |

`Auto` heuristics, evaluated in order (first match wins):

1. **Vector index empty** → `Fts`.
2. **Quoted phrase** — the query contains ≥ 2 `"` characters (anywhere) → `Fts`.
3. **FTS operators** — the query contains a `:` character (anywhere), or any whitespace-delimited token is exactly `AND`, `OR`, or `NOT` (uppercase only) → `Fts`.
4. **Short pure-keyword** — ≤ 3 whitespace-delimited tokens **and** no token is a stop word → `Fts`.
5. **Otherwise** (natural language: > 3 tokens, or a stop word present) → `Hybrid`.

Stop-word list (23 words; token match is exact after ASCII lowercasing):

```
the a an is are was were in on at for to of with by from
how what why when where which who
```

`Auto` never selects `Semantic`; semantic-only retrieval is explicit-opt-in.

### 2.2 Edge-case semantics (consequences of the implementation)

- Empty/whitespace-only query → rule 4 (0 tokens, no stop words) → `Fts`.
- Any colon triggers rule 3: URLs (`http://…`), times (`12:30`), and `mime:application/pdf` all route to `Fts`.
- Punctuation is not stripped before stop-word matching: `"the,"` is not a stop word, `"The"` is.
- Lowercase `and`/`or`/`not` are ordinary tokens, not operators.
- A single `"` character is not a phrase.

Locked by characterization tests (`crates/ndex-search/tests/characterization.rs`): `explicit_modes_pass_through_when_vectors_present`, `semantic_and_hybrid_fall_back_to_fts_without_vectors`, `auto_short_keyword_queries_pick_fts`, `auto_phrases_and_operators_pick_fts`, `auto_natural_language_picks_hybrid` (including short-with-stop-word `"what is blake3"` → `Hybrid`), `auto_with_empty_vectors_always_picks_fts`.

The selected mode is reported in `SearchOutcome.mode` and surfaced to the user (PRD §10.7).

## 3. Query preparation (`query.rs`) — ✅ implemented

### 3.1 Query embedding

`embed_query(embedder: &dyn Embed, query: &str) -> Result<Embedding>`:

1. Prefix the query with the asymmetric query prefix via `Tokenizer::with_query_prefix` (prefix string owned by [34-embedding.md](../30-ingest/34-embedding.md)).
2. Call `embedder.embed_batch(&[prefixed])` (a single-element batch).
3. Return the first vector; if the embedder returns none, error `NdexError::Model("embedder returned no vectors for the query")` ([14-errors.md](../10-core/14-errors.md)).

Truncation to the model's max query length is delegated to the embedder ([34-embedding.md](../30-ingest/34-embedding.md) — currently unenforced there). Locked by characterization test `embed_query_applies_asymmetric_prefix`, which uses a recording fake `Embed` to pin the exact string sent to inference (`"query: quarterly earnings"`). No caller invokes `embed_query` yet — semantic retrieval is not wired (§4).

### 3.2 FTS query construction

There is none in this crate: the **raw user query string is passed verbatim** to `FtsIndex::search` — no rewriting, sanitization, or operator escaping. Parsing semantics (tantivy `QueryParser`, field boosts, BM25) are owned by [23-fts.md](../20-store/23-fts.md). `ndex-search` supplies the `title_boost` argument from `store.config.search.title_boost` (default owned by [13-config.md](../10-core/13-config.md)).

## 4. Execution (`search.rs`) — 🚧 partial (FTS-only)

### 4.1 Result types — ✅ implemented

`Hit` (engine-level; `Debug, Clone, PartialEq`):

| Field | Meaning |
|---|---|
| `file_id: i64` | manifest file id |
| `chunk_ord: u32` | chunk ordinal within the file |
| `score: f32` | display score, min-max normalized to `[0, 1]` |
| `score_raw: f32` | raw fused/BM25/cosine score |
| `score_fts: Option<f32>` | BM25 component (doc comment: "with `--explain`") |
| `score_vec: Option<f32>` | cosine component (doc comment: "with `--explain`") |
| `byte_start: u64`, `byte_end: u64` | chunk byte span in the source file |

`SearchOutcome`: `hits: Vec<Hit>`, `total: u64`, `mode: SearchMode`, `truncated: bool`. `Default` is empty hits, `total = 0`, `mode = Auto`, `truncated = false` — locked by `search_outcome_default_is_empty_auto`; `Hit` construction/equality locked by `hit_is_constructible_and_comparable`.

### 4.2 `run` — 🚧 partial

```rust
pub fn run(store: &Store, embedder: Option<&dyn Embed>, query: &str,
           requested: SearchMode, filters: &SearchFilters,
           limit: usize, offset: usize) -> Result<SearchOutcome>
```

Implemented behavior (v0.1):

1. `vectors_empty` = `store.vectors` is `None` **or** `is_empty()`.
2. `mode = mode::resolve(query, requested, vectors_empty)` — computed and **reported only**; retrieval does not branch on it.
3. **Fetch depth rule (owned here):** `fetch = max(limit + offset, 1)` (saturating add).
4. FTS retrieval: `store.fts.search(query, fetch, store.config.search.title_boost)` — always, regardless of resolved mode.
5. Display scores: copy the raw BM25 scores, `min_max_normalize` **over the entire fetched candidate list** (before pagination).
6. Build `Hit`s: `score` = normalized, `score_raw` = raw BM25, `score_fts = Some(score_raw)` **unconditionally**, `score_vec = None` always.
7. `total` = number of fetched hits (≤ `offset + limit`; **not** the corpus-wide match count).
8. Paginate: `skip(offset).take(limit)`.
9. `truncated = total > offset + hits.len()` — provably always `false` given step 3's fetch cap (see Divergences).

Not implemented:

- **Semantic retrieval** — 📋 planned: `embed_query` → usearch ANN ([24-vectors.md](../20-store/24-vectors.md), `search.ef_search` in [13-config.md](../10-core/13-config.md)). The `embedder` parameter is accepted and ignored (`None` is passed by the only current caller, `ndex-remote search`).
- **Hybrid fusion wiring** — 📋 planned: `fuse::rrf_score` exists (§5) but has no caller.
- **Filters** — 📋 planned: `filters: &SearchFilters` accepted and ignored (type owned by [15-search-and-progress-types.md](../10-core/15-search-and-progress-types.md)).
- **Fallback warnings** — 📋: PRD §10.7/§16.3 require a user-visible warning on empty-vector fallback; neither the engine nor the server emits one.

The intended end-to-end contract is pinned by ignored test `run_returns_ranked_hits_in_resolved_mode` (`#[ignore = "impl pending: PR #3"]`): over a populated index, `run` resolves the mode, retrieves, fuses, applies the limit, and reports resolved mode + truncation. (Its comment claims `Store::open` is also todo; that is stale — the store opens today.)

## 5. Fusion and normalization (`fuse.rs`) — ✅ implemented, not yet wired

### 5.1 Reciprocal Rank Fusion (formula owned here)

```rust
pub fn rrf_score(rank_fts: Option<usize>, rank_semantic: Option<usize>,
                 k: u32, fts_weight: f32) -> f32
```

```
rrf_score(d) = fts_weight × 1/(k + rank_fts(d)) + 1/(k + rank_semantic(d))
```

- Ranks are **1-based**; `None` (absent from that result list) contributes a `0.0` term — the code's realization of the PRD's `rank = ∞`.
- `k` is the RRF constant; `fts_weight` scales **only** the FTS term. Runtime values come from `search.rrf_k` and `search.fts_weight` ([13-config.md](../10-core/13-config.md)); nothing in this crate reads them yet, since `rrf_score` has no production caller.
- Absent from both lists ⇒ `0.0`.

Matches the PRD §10.7 formula exactly (including the `fts_weight` multiplier and the `title_boost` vs `fts_weight` disambiguation: `title_boost` acts inside BM25 field scoring, [23-fts.md](../20-store/23-fts.md); `fts_weight` acts here in the RRF sum).

Locked by characterization tests: `rrf_rewards_presence_in_both_lists` (both-lists > single-list; absent-from-both = 0), `rrf_better_rank_scores_higher`, `rrf_weight_scales_only_the_fts_term` (semantic term invariant under `fts_weight`), `rrf_larger_k_flattens_scores`.

### 5.2 Display normalization

`min_max_normalize(scores: &mut [f32])` — in-place min-max to `[0, 1]`:

- General case: `(s − min) / (max − min)`.
- Degenerate case (`max − min ≤ f32::EPSILON`): **all elements set to `1.0`** — covers ties, single-element sets, and (vacuously, no panic) empty sets. Raw scores are preserved separately by the caller (`score_raw`).
- Negative inputs are handled by the same formula.

Locked by characterization tests: `normalize_spreads_to_unit_range`, `normalize_ties_and_singletons_map_to_one` (ties → all `1.0`, singleton → `1.0`, empty → no panic), `normalize_handles_negative_scores`.

### 5.3 `ScoreExplain` — ✅ type, 📋 wiring

Per-component breakdown for `--explain`: `{ bm25: Option<f32>, cosine: Option<f32>, rrf: Option<f32> }`, `Default` = all `None` (locked by `score_explain_defaults_to_all_absent`). Exported but constructed nowhere; `--explain` plumbing through the protocol/CLI is 📋 ([53-messages.md](../50-protocol/53-messages.md), [61-client-cli.md](../60-interfaces/61-client-cli.md)).

## 6. Status summary

| Item | Status |
|---|---|
| `mode::resolve` + heuristics + stop-word list | ✅ |
| `query::embed_query` (prefix + single-item batch) | ✅ (no production caller) |
| FTS execution, normalization, pagination in `run` | ✅ |
| `Hit` / `SearchOutcome` types | ✅ |
| Semantic retrieval in `run` | 📋 |
| Hybrid RRF wiring in `run` | 📋 (`rrf_score` math ✅) |
| `SearchFilters` application | 📋 |
| `min_max_normalize` | ✅ |
| `ScoreExplain` / `--explain` wiring | ✅ type / 📋 wiring |
| Empty-vector fallback warning | 📋 |
| Accurate `total` / `truncated` | 🚧 (see Divergences) |

## Divergences & open questions

1. **Explicit `semantic` + empty vector index contradicts PRD §16.3.** The PRD decision: semantic mode returns **0 results with a warning** ("Vector index is empty. Run `ndex index`…"); only hybrid falls back to FTS. `mode.rs` resolves `Semantic → Fts` (returning FTS hits) while its own doc comment cites §16.3 as the authority. Locked-in by characterization test `semantic_and_hybrid_fall_back_to_fts_without_vectors`, so changing it is a spec decision, not a refactor.
2. **`SearchOutcome.mode` can misreport retrieval.** `run` executes FTS unconditionally but reports the *resolved* mode. Today `vectors_empty` is always true (embedding is stubbed) so resolution collapses to `Fts`/`Hybrid→Fts`… except explicit `Semantic`/`Hybrid` with a non-empty vector index would be labeled semantic/hybrid while serving BM25 results. Latent until vectors land, but the field's meaning ("mode actually used", PRD §10.7) is violated by construction.
3. **`truncated` is dead logic.** Because `fetch = limit + offset`, `total ≤ offset + hits.len()` always holds, so `truncated` is always `false`. Relatedly, `total` is the fetched-candidate count, not the corpus-wide match count PRD §10.7/§12 imply — clients cannot render "N total matches" or paginate reliably.
4. **Normalization window vs PRD.** PRD: display scores are min-max normalized "within the returned result set." Code normalizes over the full fetched candidate set *before* `skip(offset)` — pages beyond the first won't span `[0, 1]`. Arguably more useful (scores comparable across pages), but it contradicts the PRD's wording; pick one.
5. **`score_fts` populated unconditionally.** Field docs (and PRD) tie the component breakdown to `--explain`; `run` always sets `score_fts = Some(raw)` and there is no explain flag anywhere in the engine signature. Harmless but contradicts the field's own doc comment; also redundant with `score_raw` in FTS-only mode.
6. **Operator detection is broader than PRD.** PRD lists `field:term` as the operator form; code treats *any* colon (URLs, timestamps) as an operator, and only uppercase `AND`/`OR`/`NOT`. Whether tantivy's parser actually honors a given `field:` name is decided in [23-fts.md](../20-store/23-fts.md) — auto-routing to FTS on a colon does not guarantee the query parses as the user intends.
7. **Fallback warnings unimplemented end-to-end.** PRD §10.7 fallback row says "with warning"; neither `resolve` (pure), `run`, nor the `ndex-remote search` handler emits one, and `SearchOutcome` has no warnings channel to carry it.
8. **Stale characterization header.** `crates/ndex-search/tests/characterization.rs` opens with "`query::embed_query` and `search::run` are `todo!()`" — both are now implemented (`embed_query` fully; `run` FTS-only), and `embed_query_applies_asymmetric_prefix` runs un-ignored.
9. **Config knobs defined but unread.** `search.rrf_k`, `search.fts_weight`, `search.ef_search`, and `search.default_limit` exist in config ([13-config.md](../10-core/13-config.md)) but nothing in `ndex-search` consumes them yet; only `search.title_boost` is live.
