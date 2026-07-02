# 41 — Search (`ndex-search`)

**Owns:** The `ndex-search` crate — `auto`-mode resolution rules (including the stop-word list), the empty-vector fallback policy and its warning strings, query preparation for FTS and semantic retrieval, search execution/pagination (the engine-level `Hit`/`SearchOutcome` types, the fetch-window rule, and the `total`/`truncated` semantics), and hybrid score fusion (the RRF formula and display normalization).

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
  → mode::resolve            (auto heuristics, empty-vector fallback + warnings)
  → query prep               (FTS: raw string; semantic: QUERY_PREFIX-prefixed embedding)
  → retrieval via ndex-store (tantivy FTS / usearch ANN)
  → fuse::rrf_score          (hybrid only)
  → fuse::min_max_normalize  (display scores)
  → paginate → SearchOutcome (hits + true total + truncated + warnings)
```

It returns engine-level `Hit`s keyed by `(file_id, chunk_ord)`; the **server** joins the manifest (path, mime, size, mtime) and generates snippets to build wire `SearchHit`s ([63-remote.md](../60-interfaces/63-remote.md), message shapes in [53-messages.md](../50-protocol/53-messages.md)). Dependencies: `ndex-core`, `ndex-store`, `ndex-embed`.

Types referenced but owned elsewhere: `SearchMode` / `SearchFilters` ([15-search-and-progress-types.md](../10-core/15-search-and-progress-types.md)), the tantivy schema, BM25 parameters, and field-boost application ([23-fts.md](../20-store/23-fts.md)), the usearch index and cosine/inner-product scoring ([24-vectors.md](../20-store/24-vectors.md)), `Embedding` ([11-data-model.md](../10-core/11-data-model.md)), config defaults `search.{default_limit,rrf_k,title_boost,fts_weight,ef_search}` ([13-config.md](../10-core/13-config.md)).

## 2. Mode resolution (`mode.rs`) — ✅ implemented

`resolve(query, requested, vectors_empty) -> Resolution` is a pure function. `Resolution` (`Debug, Clone, PartialEq, Eq`) is `{ mode: SearchMode, warnings: Vec<String> }`: the mode retrieval will actually execute plus any user-facing degradation warnings (at most one per resolution today). `vectors_empty` is supplied by the caller (in `search::run`: vector index absent **or** zero entries — see [24-vectors.md](../20-store/24-vectors.md)).

### 2.1 Rules (owned here)

Explicit modes:

| Requested | `vectors_empty = false` | `vectors_empty = true` |
|---|---|---|
| `Fts` | `Fts` | `Fts` (no warning) |
| `Semantic` | `Semantic` | **`Semantic`** + warning — retrieval runs nothing and returns zero hits (§4.2) |
| `Hybrid` | `Hybrid` | `Fts` (fallback) + warning |
| `Auto` | heuristics below | `Fts` + warning |

Empty-vector policy (PRD §16.3, §10.7 "fallback with warning"): explicit `semantic` is opt-in and never silently serves BM25 — the resolution stays `Semantic`, retrieval short-circuits to zero hits, and the warning suggests `--mode auto` for FTS fallback. `hybrid`'s FTS half is still meaningful, so it degrades to `Fts` with a warning. `auto` selects `Fts` with a warning that semantic ranking was skipped (emitted whenever the vector index is empty, even for queries the heuristics would have routed to `Fts` anyway).

Warning strings (owned here; carried in `SearchOutcome.warnings`, §4.1; the interface layer prints them to stderr — [63-remote.md](../60-interfaces/63-remote.md)):

| Trigger | Warning |
|---|---|
| `Semantic` + empty vectors | ``Vector index is empty; semantic search returned no results. Run `ndex index` to build it, or use `--mode auto` to fall back to full-text search.`` |
| `Hybrid` + empty vectors | ``Vector index is empty; hybrid search fell back to full-text only. Run `ndex index` to enable semantic retrieval.`` |
| `Auto` + empty vectors | ``Vector index is empty; semantic ranking skipped (results are full-text only). Run `ndex index` to enable it.`` |

`Auto` heuristics (`resolve_auto`; reached only when vectors exist), evaluated in order (first match wins):

1. **Quoted phrase** — the query contains ≥ 2 `"` characters (anywhere) → `Fts`.
2. **FTS operators** — the query contains a `:` character (anywhere), or any whitespace-delimited token is exactly `AND`, `OR`, or `NOT` (uppercase only) → `Fts`.
3. **Short pure-keyword** — ≤ 3 whitespace-delimited tokens **and** no token is a stop word → `Fts`.
4. **Otherwise** (natural language: > 3 tokens, or a stop word present) → `Hybrid`.

Stop-word list (23 words; token match is exact after ASCII lowercasing):

```
the a an is are was were in on at for to of with by from
how what why when where which who
```

`Auto` never selects `Semantic`; semantic-only retrieval is explicit-opt-in.

### 2.2 Edge-case semantics (consequences of the implementation)

- Empty/whitespace-only query → rule 3 (0 tokens, no stop words) → `Fts`.
- Any colon triggers rule 2: URLs (`http://…`), times (`12:30`), and `mime:application/pdf` all route to `Fts`.
- Punctuation is not stripped before stop-word matching: `"the,"` is not a stop word, `"The"` is.
- Lowercase `and`/`or`/`not` are ordinary tokens, not operators.
- A single `"` character is not a phrase.

Locked by characterization tests (`crates/ndex-search/tests/characterization.rs`): `explicit_modes_pass_through_when_vectors_present` (modes pass through, no warnings), `semantic_without_vectors_stays_semantic_with_warning`, `hybrid_without_vectors_falls_back_to_fts_with_warning` (and explicit `Fts` unaffected), `auto_short_keyword_queries_pick_fts`, `auto_phrases_and_operators_pick_fts`, `auto_natural_language_picks_hybrid` (including short-with-stop-word `"what is blake3"` → `Hybrid`), `auto_with_empty_vectors_always_picks_fts_with_warning`. The exact warning strings are pinned as independent literals in the characterization file.

The selected mode is reported in `SearchOutcome.mode` and surfaced to the user (PRD §10.7).

## 3. Query preparation (`query.rs`) — ✅ implemented

### 3.1 Query embedding

`embed_query(embedder: &dyn Embed, query: &str) -> Result<Embedding>`:

1. Prefix the query with the asymmetric query prefix via `Tokenizer::with_query_prefix` (prefix string owned by [34-embedding.md](../30-ingest/34-embedding.md)).
2. Call `embedder.embed_batch(&[prefixed])` (a single-element batch).
3. Return the first vector; if the embedder returns none, error `NdexError::Model("embedder returned no vectors for the query")` ([14-errors.md](../10-core/14-errors.md)).

Truncation to the model's max query length is delegated to the embedder ([34-embedding.md](../30-ingest/34-embedding.md) — currently unenforced there). Locked by characterization test `embed_query_applies_asymmetric_prefix`, which uses a recording fake `Embed` to pin the exact string sent to inference (`"query: quarterly earnings"`). No caller invokes `embed_query` yet — semantic retrieval is not wired (§4).

### 3.2 FTS query construction

There is none in this crate: the **raw user query string is passed verbatim** to `FtsIndex::search_with_total` — no rewriting, sanitization, or operator escaping. Parsing semantics (tantivy `QueryParser`, field boosts, BM25) are owned by [23-fts.md](../20-store/23-fts.md). `ndex-search` supplies the `title_boost` argument from `store.config.search.title_boost` (default owned by [13-config.md](../10-core/13-config.md)).

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

`SearchOutcome` (`Debug, Clone, Default` — no serde derives; the wire shape is owned by [53-messages.md](../50-protocol/53-messages.md)):

| Field | Meaning |
|---|---|
| `hits: Vec<Hit>` | the requested page of ranked hits |
| `total: u64` | **corpus-wide** match count (tantivy `Count` collector via `FtsIndex::search_with_total`, [23-fts.md](../20-store/23-fts.md)), independent of `limit`/`offset` |
| `mode: SearchMode` | the mode retrieval actually executed (explicit `Semantic` + empty vectors stays `Semantic` with zero hits, §4.2) |
| `truncated: bool` | matches exist beyond this page: `offset + hits.len() < total` |
| `warnings: Vec<String>` | user-facing degradation warnings from mode resolution (§2.1), surfaced by the caller |

`Default` is empty hits, `total = 0`, `mode = Auto`, `truncated = false`, empty warnings — locked by `search_outcome_default_is_empty_auto`; `Hit` construction/equality locked by `hit_is_constructible_and_comparable`.

### 4.2 `run` — 🚧 partial

```rust
pub fn run(store: &Store, embedder: Option<&dyn Embed>, query: &str,
           requested: SearchMode, filters: &SearchFilters,
           limit: usize, offset: usize) -> Result<SearchOutcome>
```

Implemented behavior (v0.1):

1. `vectors_empty` = `store.vectors` is `None` **or** `is_empty()`.
2. `Resolution { mode, warnings } = mode::resolve(query, requested, vectors_empty)` (§2.1); `mode` and `warnings` are carried into the outcome.
3. **Semantic short-circuit:** resolved `Semantic` + `vectors_empty` → return immediately with zero hits, `total = 0`, `truncated = false`, `mode = Semantic`, and the §2.1 warning. No retrieval runs (PRD §16.3) — the reported mode is honest.
4. **Fetch-window rule (owned here):** `fetch = max(limit + offset, 1)` (saturating add) — the `TopDocs` window passed to the store (tantivy requires a window ≥ 1, even when `limit == 0`).
5. FTS retrieval: `store.fts.search_with_total(query, fetch, store.config.search.title_boost)` → `(window, total)` — for every mode that reaches this step (FTS is the only retrieval in v0.1). `total` is the corpus-wide match count from tantivy's `Count` collector, independent of the window.
6. Display scores: copy the raw BM25 scores, `min_max_normalize` **over the entire fetched window** (before pagination).
7. Build `Hit`s: `score` = normalized, `score_raw` = raw BM25, `score_fts = Some(score_raw)` **unconditionally**, `score_vec = None` always.
8. Paginate: `skip(offset).take(limit)`.
9. `truncated = offset + hits.len() < total` (saturating). `limit == 0` is well-defined: zero hits, real `total`, and (at `offset = 0`) `truncated ⇔ total > 0`. An `offset` past the last match yields an empty page with `truncated = false`.

Not implemented:

- **Semantic retrieval** — 📋 planned: `embed_query` → usearch ANN ([24-vectors.md](../20-store/24-vectors.md), `search.ef_search` in [13-config.md](../10-core/13-config.md)). The `embedder` parameter is accepted and ignored (`None` is passed by the only current caller, `ndex-remote search`).
- **Hybrid fusion wiring** — 📋 planned: `fuse::rrf_score` exists (§5) but has no caller.
- **Filters** — 📋 planned: `filters: &SearchFilters` accepted and ignored (type owned by [15-search-and-progress-types.md](../10-core/15-search-and-progress-types.md)).

Locked by characterization tests over a real `Store::create` + FTS-writer fixture: `run_reports_true_total_and_truncation` (corpus-wide `total` beyond the window, `truncated` on a first page, last-page and past-the-end pagination), `run_with_zero_limit_is_a_count_query`, `run_explicit_semantic_without_vectors_returns_zero_hits`, `run_auto_and_hybrid_without_vectors_serve_fts_with_warning`.

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
| Empty-vector fallback warnings (engine: `Resolution` → `SearchOutcome.warnings`) | ✅ (stderr surfacing owned by [63-remote.md](../60-interfaces/63-remote.md)) |
| Explicit-semantic zero-hit policy (PRD §16.3) | ✅ |
| Accurate `total` / `truncated` (corpus-wide `Count`) | ✅ |

## Divergences & open questions

1. **`SearchOutcome.mode` can still misreport retrieval with a *non-empty* vector index.** The empty-vector cases are now honest (explicit `Semantic` short-circuits to zero hits; `Hybrid`/`Auto` report the `Fts` they actually serve). But retrieval past the short-circuit is FTS-only and does not branch on the resolved mode: explicit `Semantic`/`Hybrid` with a **non-empty** vector index would be labeled semantic/hybrid while serving BM25 results. Latent until vectors land (`store.vectors` is always `None` in v0.1); resolves itself when semantic retrieval is wired.
2. **Normalization window vs PRD.** PRD: display scores are min-max normalized "within the returned result set." Code normalizes over the full fetched window *before* `skip(offset)` — pages beyond the first won't span `[0, 1]`. Arguably more useful (scores comparable across pages), but it contradicts the PRD's wording; pick one.
3. **`score_fts` populated unconditionally.** Field docs (and PRD) tie the component breakdown to `--explain`; `run` always sets `score_fts = Some(raw)` and there is no explain flag anywhere in the engine signature. Harmless but contradicts the field's own doc comment; also redundant with `score_raw` in FTS-only mode.
4. **Operator detection is broader than PRD.** PRD lists `field:term` as the operator form; code treats *any* colon (URLs, timestamps) as an operator, and only uppercase `AND`/`OR`/`NOT`. Whether tantivy's parser actually honors a given `field:` name is decided in [23-fts.md](../20-store/23-fts.md) — auto-routing to FTS on a colon does not guarantee the query parses as the user intends.
5. **Config knobs defined but unread.** `search.rrf_k`, `search.fts_weight`, `search.ef_search`, and `search.default_limit` exist in config ([13-config.md](../10-core/13-config.md)) but nothing in `ndex-search` consumes them yet; only `search.title_boost` is live.
