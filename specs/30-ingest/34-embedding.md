# 34 — Embedding (`ndex-embed`)

**Owns:** The `ndex-embed` crate — the embedding model registry and on-disk model store, the HuggingFace tokenizer wrapper, and ONNX-backed batch inference — including the model identity facts (model id, dimensions, precision), the asymmetric prefix strings, and the query token limit.

**Sources:**
- `crates/ndex-embed/src/lib.rs`
- `crates/ndex-embed/src/model.rs`
- `crates/ndex-embed/src/tokenizer.rs`
- `crates/ndex-embed/src/embedder.rs`
- `crates/ndex-embed/tests/characterization.rs`
- `crates/ndex-embed/Cargo.toml`
- PRD §4.7 (embedding pipeline), §7.4 (model fetching), §16.1 (download-failure decision)

## 1. Crate role and dependencies

`ndex-embed` wraps three concerns behind a small public surface (`crates/ndex-embed/src/lib.rs`):

| Module | Concern | Re-exports |
|---|---|---|
| `model` | offline model registry + fetch/verify/import/delete | `ModelInfo`, `REGISTRY`, `list`, `lookup`, `model_path`, `models_dir` |
| `tokenizer` | HuggingFace tokenizer + token counting | `Tokenizer`, `MAX_QUERY_TOKENS` |
| `embedder` | ONNX Runtime batch inference | `Embed` (trait), `Embedder` |

Crate-graph position: depends only on `ndex-core`. The `TokenCounter` trait it implements is deliberately defined in `ndex-core` (`crates/ndex-core/src/tokens.rs`) so the chunker can size chunks in model tokens without an `ndex-extract` → `ndex-embed` dependency edge (see [33-chunking.md](33-chunking.md)).

Key dependencies (`crates/ndex-embed/Cargo.toml`, versions pinned in the workspace `Cargo.toml` — see [71-toolchain.md](../70-operations/71-toolchain.md)):

- `ort` — exact-pinned with the **`load-dynamic`** feature (pin and default-features rationale owned by [71-toolchain.md](../70-operations/71-toolchain.md) §8.3); `load-dynamic` means `libonnxruntime.so` is `dlopen`'d at runtime. ⚠ Differs from PRD §4.7's recommendation (`download-binaries` for dev, `static` for distribution) — see Divergences.
- `tokenizers` — HuggingFace tokenizer runtime.
- `half` — f16 vector storage (the `Embedding` type itself is owned by [11-data-model.md](../10-core/11-data-model.md)).
- `blake3` — model artifact integrity verification.

## 2. Model registry (`model.rs`)

### 2.1 `ModelInfo` — ✅ implemented

Static description of an available model:

| Field | Meaning |
|---|---|
| `shortname: &'static str` | CLI shortname (e.g. `arctic`) |
| `full_name: &'static str` | full model name; also the on-disk directory name |
| `onnx_blake3: &'static str` | expected BLAKE3 of `model.onnx` (hex), pinned at release time |
| `tokenizer_blake3: &'static str` | expected BLAKE3 of `tokenizer.json` (hex) |
| `dimensions: u32` | native embedding dimensionality |
| `mrl_dimensions: u32` | MRL-truncated, stored/searched dimensionality |
| `languages: u32` | supported language count |
| `url: &'static str` | release download URL |

### 2.2 The v0.1 registry — 🚧 partial (entry real, hashes/URL placeholders)

`REGISTRY` ships exactly **one** model in v0.1:

| Fact | Value |
|---|---|
| Shortname | `arctic` (the default; `ndex-remote init --model default` maps to it) |
| Full name / model id | `snowflake-arctic-embed-m-v2.0` — canonical constant `DEFAULT_MODEL` in `crates/ndex-core/src/constants.rs` |
| Native dimensions | 768 |
| MRL (stored/searched) dimensions | 256 |
| Stored vector scalar | f16 (L2-normalized; the vector index itself is owned by [24-vectors.md](../20-store/24-vectors.md)) |
| Model file precision | INT8-quantized ONNX, ~297 MB (PRD §7.4; not yet encoded anywhere in code) |
| Languages | 74 |
| `onnx_blake3` / `tokenizer_blake3` | literal `"TODO"` — to be pinned at packaging time (see [73-release.md](../70-operations/73-release.md)) |
| `url` | `https://github.com/justy/ndex/releases/download/models/snowflake-arctic-embed-m-v2.0.tar.gz` — placeholder org (see Divergences) |

Lookups: `lookup(name)` matches **shortname or full name** (exact, case-sensitive); `list()` returns the whole registry. ✅ implemented.

Locked by characterization tests (`crates/ndex-embed/tests/characterization.rs`): `registry_ships_arctic_only_in_v0_1` (registry length 1, arctic identity, 768/256 dims), `lookup_by_shortname_or_full_name`. Unit test `arctic_dims_match_prd` re-pins 768/256.

## 3. Model storage and lifecycle (`model.rs`)

### 3.1 Paths — ✅ implemented

- `models_dir()` → `$HOME/.ndex/models` (built from the `HOME` env var; returns `NdexError::Config("HOME environment variable is not set")` if unset — error taxonomy owned by [14-errors.md](../10-core/14-errors.md)). Note this is the *user-global* model store, unrelated to a per-archive `.ndex/` index dir ([12-paths.md](../10-core/12-paths.md)).
- `model_path(info)` → `models_dir()/<full_name>`.

Locked by characterization tests `models_dir_is_under_dot_ndex` and `model_path_is_models_dir_joined_with_full_name`.

Intended per-model directory layout (PRD §7.4; only the path helpers exist in code):

```
~/.ndex/models/snowflake-arctic-embed-m-v2.0/
├── model.onnx        (~297 MB, INT8)
├── tokenizer.json    (~600 KB)
└── manifest.json     (model metadata, expected hashes)   📋 no code reads/writes this
```

### 3.2 Lifecycle operations — ⛔ stubs (`todo!()`)

All four bodies are `todo!()`; intended behavior from doc comments + PRD:

- `fetch(info)` ⛔ — download to a `.tmp` file, BLAKE3-verify against the registry hashes, then atomic rename into place (PRD §7.4, §16.1). On failure delete the `.tmp`; no resume (restart from scratch); if disk is full, fail with a clear message listing required space. The model is never "partially installed."
- `verify(info)` ⛔ — re-verify a downloaded model against the registry hashes; a model that was never fetched must return `Ok(false)`, not a hard error. Pinned by ignored test `verify_reports_integrity_against_registry_hashes` (`#[ignore = "impl pending: PR #3"]`).
- `import(tarball)` ⛔ — install a pre-staged model tarball for air-gapped servers (PRD §7.4 sneakernet flow).
- `delete(info)` ⛔ — remove a downloaded model from disk.

### 3.3 Consumers — 📋 planned

Auto-fetch-on-first-use during `ndex-remote index`, the `ndex-remote serve` refusal-to-start when the model is missing, and the `ndex-remote model {list,fetch,verify,delete,path,import}` CLI family (PRD §7.4) are the server's responsibility and are specified in [63-remote.md](../60-interfaces/63-remote.md); its `model` command dispatcher is itself a stub that plans to call the functions above. The only registry consumer wired today is `ndex-remote init`, which copies `ModelInfo` fields into the immutable index identity ([22-manifest.md](../20-store/22-manifest.md) / [11-data-model.md](../10-core/11-data-model.md)).

## 4. Tokenizer (`tokenizer.rs`)

Wraps a `tokenizers::Tokenizer` loaded from a model's `tokenizer.json` (HuggingFace format). Used for (a) token counting during chunking ([33-chunking.md](33-chunking.md)) and (b) tokenization before ONNX inference (PRD §4.7).

### 4.1 Prefix strings — ✅ implemented (owned here)

`snowflake-arctic-embed-m-v2.0` uses **asymmetric embedding**:

| Use | Prefix |
|---|---|
| Document chunks (index time) | *(none)* |
| Search queries (search time) | `"query: "` (trailing space included) |

The query prefix is the constant `QUERY_PREFIX` in `crates/ndex-core/src/constants.rs`; `Tokenizer::with_query_prefix(query)` returns `format!("{QUERY_PREFIX}{query}")`. It is hardcoded per model and **not** user-configurable; omitting it degrades retrieval quality significantly (PRD §4.7). Applied at search time by `ndex-search` ([41-search.md](../40-search/41-search.md)).

Locked by characterization test `query_prefix_is_the_asymmetric_marker` (including the empty-query case → `"query: "`).

### 4.2 Query token limit — ✅ constant defined, 🚧 unenforced

`MAX_QUERY_TOKENS = 512` (model limit, PRD §4.7). Queries exceeding it are to be truncated **with a warning** (PRD §4.7); no code path currently enforces the limit or emits the warning — `query.rs` in `ndex-search` delegates truncation to the embedder, whose body is a stub. Locked by characterization test `max_query_tokens_matches_model_limit`.

### 4.3 API surface

- `Tokenizer::load(tokenizer_json: &Path)` — ⛔ stub. Intended: `tokenizers::Tokenizer::from_file`.
- `Tokenizer::encode(&self, text) -> Result<Vec<u32>>` — ⛔ stub.
- `Tokenizer::with_query_prefix(query) -> String` — ✅ (associated fn, no tokenizer instance needed).
- `Tokenizer::truncate(ids, max) -> Vec<u32>` — ✅ pure `Vec::truncate` (caps to `max`, shorter input unchanged, `max = 0` yields empty). Locked by `truncate_caps_to_max_and_leaves_shorter_alone`.
- `impl TokenCounter for Tokenizer` (`count`) — ⛔ stub. Intended: `encode(text).map(|ids| ids.len()).unwrap_or(0)`.

The load/encode/count contract is pinned by ignored test `tokenizer_load_encode_and_count_agree`: loading a real `tokenizer.json`, `encode("hello world")` is non-empty and `count` equals the encoded length.

## 5. Embedder (`embedder.rs`)

### 5.1 The `Embed` trait — ✅ defined

```rust
pub trait Embed {
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>>;
}
```

Contract: one MRL-truncated, L2-normalized vector per input, in order. `Embedding` (a `Vec<f16>` newtype) is owned by [11-data-model.md](../10-core/11-data-model.md). The trait is object-safe and taken as `&dyn Embed` by both consumers — the reconcile pipeline ([31-reconcile.md](31-reconcile.md)) and search-time query embedding ([41-search.md](../40-search/41-search.md)) — which lets tests substitute fakes.

### 5.2 `Embedder` — ⛔ stubs

`Embedder` = `ort::session::Session` + the model's `Tokenizer`. Design rationale (doc comment): `ort::Session` is `Send + Sync` and `run`s with `&self`, so a single `Embedder` is shared by the embed thread for batched inference (PRD §4.7).

- `Embedder::load(model_dir, intra_op_threads, inter_op_threads)` — ⛔ stub. Intended: load `model.onnx` + `tokenizer.json` from the model directory, applying the configured intra-/inter-op thread counts and the CPU execution provider. v0.1 is **CPU-only** (CUDA/ROCm deferred to v0.2, PRD §4.7); the active EP is to be logged at startup. Thread-count and batch-size defaults come from the `[embedding]` config table (`embedding.intra_op_threads`, `embedding.inter_op_threads`, `embedding.threads` alias, `embedding.batch_size`) whose values are owned by [13-config.md](../10-core/13-config.md).
- `Embedder::embed_batch` — ⛔ stub. Intended pipeline (doc comment + PRD §4.7 query flow):

  ```
  tokenize → ort run → 768-d output → MRL-truncate to 256 dims → L2-normalize → cast to f16
  ```

  Pinned by ignored test `embedder_produces_mrl_truncated_normalized_vectors`: one output per input, `dims() == 256`, L2 norm within `1e-2` of 1.0 (norm computed over the f16 components widened to f32).

### 5.3 Batching and throughput

At index time, chunks are batched for inference; batch size is `embedding.batch_size` ([13-config.md](../10-core/13-config.md)). The reconcile Phase 3 pipeline receives an `Option<&dyn Embed>` (`None` ⇒ `--no-vectors`) but **currently ignores it** — v0.1 indexes FTS only; wiring is specified in [31-reconcile.md](31-reconcile.md). 📋 planned. PRD §4.7 sets an expectation of ~4,000 chunks/sec on a 16-core server (no benchmark exists in-repo).

## 6. Status summary

| Item | Status |
|---|---|
| `ModelInfo`, `REGISTRY`, `lookup`, `list` | ✅ (🚧 hashes/URL are placeholders) |
| `models_dir`, `model_path` | ✅ |
| `fetch`, `verify`, `import`, `delete` | ⛔ `todo!()` |
| `manifest.json` in model dir | 📋 no code |
| `Tokenizer::with_query_prefix`, `truncate`, `MAX_QUERY_TOKENS` | ✅ |
| `Tokenizer::load`, `encode`, `TokenCounter::count` | ⛔ `todo!()` |
| `Embed` trait | ✅ |
| `Embedder::load`, `embed_batch` | ⛔ `todo!()` |
| 512-token query truncation + warning | 📋 no code |
| Index-time embed wiring (reconcile) | 📋 (see [31-reconcile.md](31-reconcile.md)) |
| Auto-fetch / serve refusal / `model` CLI | 📋 (see [63-remote.md](../60-interfaces/63-remote.md)) |

## Divergences & open questions

1. **Placeholder release coordinates.** `REGISTRY` ships `onnx_blake3 = "TODO"`, `tokenizer_blake3 = "TODO"`, and a download URL under `github.com/justy/ndex` while the repository lives under `getkono`. PRD §7.4 requires artifacts published from the ndex GitHub releases with pinned hashes. Note `ndex-remote init` already copies `onnx_blake3` (`"TODO"`) into the immutable index identity as `model_hash` — indexes created before hash pinning will carry the placeholder forever.
2. **`ort` linkage contradicts PRD §4.7.** PRD recommends `download-binaries` for dev and static linking for distribution ("eliminates `libonnxruntime.so` runtime dependencies"). The workspace exact-pins `ort` with `load-dynamic` ([71-toolchain.md](../70-operations/71-toolchain.md) §8.3), which *requires* a runtime `libonnxruntime.so` — the opposite of the distribution goal. The workspace's default-features note (recorded in [71-toolchain.md](../70-operations/71-toolchain.md)) suggests the choice is a workaround for the pinned release candidate, but no doc records the intended distribution story.
3. **Query truncation warning is unowned.** PRD §4.7: queries over 512 tokens "are truncated with a warning." `MAX_QUERY_TOKENS` exists and `Tokenizer::truncate` exists, but nothing connects them, and no warning channel is designed (`embed_batch` returns only vectors).
4. **`manifest.json` has no code counterpart.** PRD §7.4's model directory includes `manifest.json` ("model metadata, expected hashes"); the code keeps hashes solely in the static registry and never reads/writes a manifest. One of the two should be declared authoritative.
5. **PRD example references a non-registry model.** The §7.4 air-gapped flow downloads `/tmp/minilm.tar.gz`; no "minilm" model exists in the v0.1 registry (arctic-only). Presumed stale PRD example.
6. **`embedding.threads` alias semantics undefined at this layer.** `Embedder::load` takes only `intra_op_threads`/`inter_op_threads`; how the legacy `threads` alias resolves into those (and what `0` = "all cores" maps to in `ort`) is unspecified in code — resolution order belongs to [13-config.md](../10-core/13-config.md) but must be decided before `load` is implemented.
