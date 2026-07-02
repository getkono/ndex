# Full-text index: tantivy `content/`

**Owns:** The tantivy schema (every field's options), analyzer/tokenizer configuration, writer settings, commit/merge behavior, and the FTS search/snippet semantics of `FtsIndex`.

**Sources:** `crates/ndex-store/src/fts.rs`

The index lives in `.ndex/content/` (layout owned by [layout & locking](21-layout-and-locking.md)); `Chunk` is core-owned ([data model](../10-core/11-data-model.md)). Dependency: `tantivy` (version pin owned by [toolchain](../70-operations/71-toolchain.md)). Commit *cadence* (every 10,000 docs / 30 s, PRD §10.2) is the ingest pipeline's responsibility — see [reconcile](../30-ingest/31-reconcile.md); this module only exposes the primitive.

## Schema

Built by `build()` / exposed as `FtsIndex::build_schema()`. Field options are exactly:

| Field | tantivy type | Options | Written by `add_chunk`? | Status |
|---|---|---|---|---|
| `file_id` | i64 | `INDEXED \| STORED \| FAST` | yes | ✅ |
| `chunk_ord` | u64 | `STORED \| FAST` (not indexed) | yes | ✅ |
| `body` | text | `TEXT \| STORED` (default tokenizer, positions) | yes (`chunk.text`) | ✅ |
| `title` | text | `TEXT \| STORED` | yes (`meta.title`, optional; omitted when `None`) | ✅ |
| `path_text` | text | `TEXT \| STORED` (default tokenizer — *not* the PRD `path` tokenizer) | yes (`meta.path_text`) | ✅ |
| `mime` | text | `STRING \| STORED` (raw, untokenized) | yes (`meta.mime`) | ✅ |
| `lang` | text | `STRING \| STORED` | yes (`meta.lang`, optional; omitted when `None`) | ✅ |
| `byte_start` | u64 | `STORED` | yes | ✅ |
| `byte_end` | u64 | `STORED` | yes | ✅ |
| `size` | u64 | `STORED \| FAST` | yes (`meta.size`) | ✅ |
| `mtime` | i64 | `STORED \| FAST` | yes (`meta.mtime_ns`, unix **nanoseconds**) | ✅ |

`meta` above is the `FtsFileMeta<'a>` params struct — the per-file attributes stamped onto every chunk document of a file:

```rust
pub struct FtsFileMeta<'a> {
    pub mime: &'a str,
    pub lang: Option<&'a str>,
    pub path_text: &'a str, // display form of the file's path
    pub size: u64,
    pub mtime_ns: i64,      // unix ns; stored in the schema's `mtime` field
    pub title: Option<&'a str>,
}
```

`path_text`/`size`/`mtime_ns` mirror the manifest row; `title` comes from extracted document metadata (`doc_meta.title`) when available. Populating them is the caller's obligation — see [reconcile](../30-ingest/31-reconcile.md).

Deltas from the PRD §10.2 field table: `file_id` is i64 (PRD: u64); `mtime` is i64 (PRD: date); `mime`/`lang` are not FAST (PRD: fast = yes); `chunk_ord` is not indexed (sufficient — deletes key on `file_id` only). Core-fields presence is pinned by unit test `schema_has_core_fields` and characterization test `fts_index_add_commit_search` (`body`, `file_id`, `chunk_ord`).

### Analyzers 📋 (not implemented)

No tokenizers are registered — despite the `open_or_create` doc comment claiming "registering tokenizers (PRD §10.2)", the body performs no `tokenizers().register(...)` call. Consequently:

- `TEXT` fields use tantivy's built-in `default` analyzer (simple word splitting → 40-char long-token removal → lowercasing). The PRD §10.2 `default` chain — Unicode word tokenizer → LowerCaser → RemoveLongFilter(80) → AsciiFoldingFilter → English stemmer — is 📋 planned; today there is **no stemming and no ASCII folding**.
- The PRD `path` tokenizer (split on `/` and `.`, per-component trigrams) is 📋 planned; `path_text` is declared with the default analyzer and never written anyway.
- NFC normalization of indexed text and queries (PRD §10.2) is not applied here (nor upstream — see [extraction](../30-ingest/32-extraction.md)).
- `STRING` fields (`mime`, `lang`) are indexed raw (exact-match terms), untokenized.

Changing any of this later is a tokenizer/schema change gated by `fts.tokenizer_version` in the [identity](../10-core/11-data-model.md) (PRD §5.3).

## Writer & reader ✅

- `WRITER_HEAP_BYTES = 64 * 1024 * 1024` (64 MiB) — heap budget for the writer.
- `open_or_create(dir)`: `create_dir_all(dir)` → `MmapDirectory::open` → `Index::open_or_create(mmap, schema)` — opening an existing index whose stored schema differs errors out, which is the enforcement edge of the no-migrations policy (PRD §5) at the tantivy layer → `index.writer_with_num_threads(1, WRITER_HEAP_BYTES)` → `index.reader()` (default reload policy).
- `open_readonly(dir)`: identical, except **no `IndexWriter` is created** (the `writer` field is `Option<IndexWriter>`, `None` here). Tantivy's own writer lock (`.tantivy-writer.lock`) is exclusive, so writer-less handles are what let multiple processes search the same index concurrently — this backs [`Store::open_read`](21-layout-and-locking.md). Write operations on a read-only handle (`add_chunk`, `delete_file`, `commit`) fail with `NdexError::Index("FTS index is open read-only (no writer)")`. Note it still calls `create_dir_all` + `Index::open_or_create`, so opening a missing `content/` dir creates an empty index rather than erroring.
- **Threading model:** one `IndexWriter` per index, configured with a **single** indexing thread; `add_chunk`/`delete_file`/`commit` take `&mut self`, so document adds are serialized through the owner (the writer thread that holds the [store lock](21-layout-and-locking.md)). The `IndexReader` is cheap to clone and lock-free for concurrent search (PRD §10.2). PRD §10.2's "multiple extraction workers call `add_document()` concurrently" is **not** the implemented model.
- All tantivy errors map to `NdexError::Index` ([errors](../10-core/14-errors.md)).

## Operations

| Method | Behavior | Status |
|---|---|---|
| `add_chunk(file_id, chunk, meta: &FtsFileMeta)` | Builds one `TantivyDocument`: `file_id`, `chunk_ord` (u32→u64), `body` = `chunk.text`, plus every `meta` field per the schema table above (`title`/`lang` omitted when `None`). Not visible until `commit`. Fails on read-only handles. | ✅ |
| `delete_file(file_id)` | `writer.delete_term` on the `file_id` i64 term — deletes *all* chunks of the file (PRD §13.8). Used by the reconciler to purge stale chunks before re-processing a file. Takes effect at the next `commit`. Fails on read-only handles. | ✅ |
| `commit()` | `writer.commit()` then an explicit synchronous `reader.reload()`, so committed documents are searchable as soon as `commit` returns. Fails on read-only handles. | ✅ |
| `maybe_merge(segment_threshold)` | **No-op hook.** v0.1 relies on tantivy's default `LogMergePolicy`; the PRD §16.4 explicit `writer.merge()` above a segment threshold (default 8) and the reindex-time `merge().wait()` are 📋 planned. The parameter is ignored (works on read-only handles too, being a no-op). | 🚧 |
| `search(query, limit, title_boost)` | Delegates to `search_with_total`, discarding the total. | ✅ |
| `search_with_total(query, limit, title_boost)` | See below; returns `(Vec<FtsHit>, u64)` — the top-`limit` window plus the **true** total match count. | ✅ |
| `snippet(file_id, chunk_ord, query)` | See below. | ✅ |

### `search` / `search_with_total` ✅

BM25 query over `body` + `title` (PRD §10.7). `search_with_total` is the primitive; `search` delegates to it and drops the total:

1. `QueryParser::for_index` with default fields `[body, title]`; `set_field_boost(title, title_boost)` (the boost value and its default are owned by [search config](../10-core/13-config.md)). The full tantivy query syntax (quoted phrases, `AND`/`OR`, `field:term`) is passed through; parse failure is an `NdexError::Index`.
2. One searcher pass with the collector tuple `(TopDocs::with_limit(limit).order_by_score(), Count)` — the top-`limit` window **and** the true total number of matching documents (chunks, not files), independent of `limit`. Callers use the total for accurate `total`/`truncated` pagination values ([search](../40-search/41-search.md)).
3. Each hit is materialized to `FtsHit { file_id: i64, chunk_ord: u32, score: f32, byte_start: u64, byte_end: u64 }`. Missing stored values decode as `0` (`unwrap_or_default`). Return type: `(Vec<FtsHit>, u64)`.

With `title` now populated, `title_boost` has real effect: the effective score is `bm25(body) + bm25(title) × title_boost`. The PRD §10.7 `path_text × 0.5` component is still absent — `path_text` is indexed but not a default query field, so it only matches via explicit `path_text:term` query syntax. Result-set-level scoring (RRF fusion, normalization) is owned by [search](../40-search/41-search.md).

Pinned by characterization tests `fts_index_add_commit_search` (add one chunk → commit → `search("hello", 10, 2.0)` returns `file_id = 1` first), `fts_title_and_path_text_are_indexed` (title-only term matches; `path_text:` field syntax matches), and `fts_search_with_total_reports_true_count` (5 matching docs, `limit = 2` → 2 hits, total 5; `search` returns the same window).

### `snippet` ✅

Highlighted snippet for one specific `(file_id, chunk_ord)` hit (PRD §10.2):

1. New `QueryParser` over `body` **only** (no title), parse the query.
2. `SnippetGenerator::create` for `body`.
3. Re-run the search with a fixed internal `TopDocs::with_limit(64)`, linearly scan for the document whose stored `file_id` and `chunk_ord` match, and return `snippet.to_html()` for it.
4. If the target document is not among the **top 64** scored docs for that query (or doesn't match), returns `Ok(None)` — a silent recall limit.

The returned string is **HTML** (`<b>…</b>` marks from `to_html()`), whereas PRD §10.2 specifies the client receives raw snippet text and applies ANSI highlighting itself — see divergence 4. Snippet presence is pinned by `fts_index_add_commit_search` (`snippet(1, 0, "hello")` is `Some`).

## Test coverage

- Characterization `fts_index_add_commit_search` — add/commit/search/snippet happy path with a fully-populated `FtsFileMeta`.
- Characterization `fts_title_and_path_text_are_indexed` — a term appearing only in `title` matches via the default query fields; `path_text:term` field syntax matches.
- Characterization `fts_delete_file_purges_all_chunks` — 3 chunks of file 1 + 1 chunk of file 2 → `delete_file(1)` + `commit` → only file 2 remains.
- Characterization `fts_search_with_total_reports_true_count` — true total vs `limit` window; `search` delegation equality.
- Characterization `fts_readonly_searches_but_rejects_writes` — two coexisting `open_readonly` handles both search; `add_chunk`/`delete_file`/`commit` all fail on them.
- Unit `schema_has_core_fields`.
- Untested: `maybe_merge`, multi-segment behavior, reader visibility before/after commit, query-syntax errors, snippet beyond-top-64 miss, relative boost-magnitude effect on ranking, stored `size`/`mtime` round-trip (they are written but nothing reads them back).

## Divergences & open questions

*(Resolved here: the dead-fields divergence — `title`, `path_text`, `size`, `mtime` were declared but never written; `add_chunk` now populates all four via `FtsFileMeta`, making `title_boost` effective.)*

1. **Doc comment vs body:** `open_or_create` claims to register tokenizers; it registers none. The PRD §10.2 analyzer chains (stemming, ASCII folding, 80-char limit, `path` tokenizer with trigrams) are entirely absent — searches for `running` will not match `run`, and `café`/`cafe` do not fold. 📋
2. **Scoring formula gap (narrowed):** implemented score = `bm25(body) + bm25(title)×boost`; PRD §10.7 additionally wants a `bm25(path_text)×0.5` component. `path_text` is indexed but not a default query field, so path terms only match via explicit `path_text:` syntax. 🚧
3. **Field-type deltas from PRD §10.2:** `mtime` i64 (unix nanoseconds) vs date; `mime`/`lang` not FAST (would matter for fast filtering); `file_id` i64 vs u64 (consistent with the manifest's SQLite `INTEGER` file_id, so the PRD table appears wrong, not the code).
4. **Snippet output format:** HTML from `to_html()` vs the PRD's raw-text + client ANSI highlighting contract; and the top-64 rescan means low-ranked hits silently get no snippet.
5. **Merge policy decision is unverified:** PRD §16.4 says "verify LogMergePolicy is configured and not disabled" — nothing configures or asserts it; `maybe_merge` is a stub-shaped no-op that accepts and ignores its threshold.
6. **Commit durability is untested** — no test crashes between `add_chunk` and `commit`, or asserts that uncommitted docs are invisible/recoverable (crash-safety contract PRD §11.2 leans on this).
7. **`open_readonly` on a missing `content/` creates an empty index** (it shares `open_or_create`'s `create_dir_all` + `Index::open_or_create` path) instead of failing like a reader arguably should; harmless under `Store::open_read` (the identity check already gates on a real index) but observable when called directly.
