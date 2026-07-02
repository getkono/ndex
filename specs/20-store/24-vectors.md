# Vector index: usearch `vectors/` + sidecar

**Owns:** The usearch HNSW vector index wrapper, the sidecar label-mapping file format (magic, header, record layout), the save-ordering / crash-repair contract, and the `VecHit` result type.

**Sources:** `crates/ndex-store/src/vector.rs`

**Overall status: ⛔ stub.** Every I/O method body is `todo!()`. The types, constants, and doc comments encode the intended design (PRD §10.3), which this doc specs as intent. Nothing constructs a `VectorIndex` today — [`Store`](21-layout-and-locking.md) hard-codes `vectors: None` and `.ndex/vectors/` is never created. Dependencies: `usearch` and `half` — version pins owned by [toolchain](../70-operations/71-toolchain.md).

## Intended on-disk format (PRD §10.3, §10.6) 📋

Two files under `.ndex/vectors/` (directory name constant in [config/constants](../10-core/13-config.md)): the vector index file — the usearch HNSW graph, mmap-served (`view()`) — and the sidecar carrying the label → `(file_id, chunk_ord)` mapping. Both file names are owned by the [on-disk layout](21-layout-and-locking.md); they appear only in the PRD and the layout table — no code constant exists yet.

### Sidecar format

- **Magic** ✅ (real constant): `SIDECAR_MAGIC: &[u8; 8] = b"NDEXVEC\0"`. Deliberately distinct from the IPC preamble ([framing](../50-protocol/51-framing.md)) — pinned by characterization test `sidecar_magic_is_distinct_and_fixed_width` (equality with `b"NDEXVEC\0"`, length 8, non-collision with `MAGIC_PREAMBLE`).
- **Header** 📋: 128 bytes — magic, version, entry count, model name, dimensions (PRD §10.3). Field offsets, integer widths, endianness, and model-name encoding are **unspecified** anywhere.
- **Records** 📋: fixed 24-byte entries. The in-memory type is real ✅: `SidecarEntry { label: u64, file_id: i64, chunk_ord: u32 }` — whose fields total 20 bytes; the remaining 4 bytes of the on-disk record are unspecified (padding/reserved — open question 2).

### `Sidecar` type

| Item | Behavior | Status |
|---|---|---|
| `Sidecar::new()` / `default()` | Empty entry vector. | ✅ |
| `len()` / `is_empty()` | Entry count / emptiness. Pinned by characterization test `empty_sidecar_is_empty` and unit test `empty_sidecar_reports_empty` (fresh sidecar: `is_empty()`, `len() == 0`). | ✅ |
| `load(path)` | Intent: read from disk, validating the magic header. | ⛔ |
| `save(path)` | Intent: write via temp-file + atomic rename. | ⛔ |

## Intended index parameters

Per PRD §10.3: inner-product metric over L2-normalized vectors (= cosine), `f16` scalar storage (~768 bytes/vector total incl. HNSW overhead at the default `hnsw_m` chosen by [`build_identity`](../60-interfaces/63-remote.md)). The concrete per-index values — dimensions (MRL truncation), `hnsw_m`, `hnsw_ef_construction` — are recorded in the index's `IndexIdentity` embedding section ([data model](../10-core/11-data-model.md)); `ef_search` is a config knob ([config](../10-core/13-config.md)). The API's vector type is `&[f16]` (`half::f16`), matching the stored scalar; how `half::f16` buffers hand off to usearch's native f16 is an implementation detail not yet written.

## `VectorIndex` API

`VectorIndex { idx: usearch::Index, sidecar: Sidecar }`. Writes go through the single instance (single writer thread, coordinated with the sidecar append); reads are intended to be lock-free via usearch `view()` (PRD §10.3 threading).

| Method | Documented intent | Status |
|---|---|---|
| `open_or_create(dir, dims)` | Open or create the index under `dir` for `dims`-dimensional vectors. | ⛔ |
| `add(file_id, chunk_ord, vector)` | Add one vector under a fresh usearch label and append the corresponding `SidecarEntry`. | ⛔ |
| `search(vector, k)` | k-NN query; resolve returned labels through the sidecar to `VecHit { file_id: i64, chunk_ord: u32, distance: f32 }` (inner-product distance). | ⛔ |
| `tombstone(file_id)` | Mark all of a file's vectors deleted (usearch tombstones). Space reclaimed only by `ndex compact` (v0.2); tombstoned vectors must not appear in results — PRD §13.8 says the *sidecar lookup skips deleted file_ids* (mechanism unspecified — open question 3). Raw f16 values remain on disk until compaction (PRD §13.8 security note). | ⛔ |
| `len()` | Live vector count (usearch `size()`). | ⛔ |
| `is_empty()` | `len() == 0`; drives the auto-mode FTS fallback (PRD §16.3, heuristics owned by [search](../40-search/41-search.md)). Real body, but it delegates to the stubbed `len()` — calling it today panics. | 🚧 |
| `save(dir)` | Persist **sidecar first, then** the usearch index, each via temp-file + atomic rename. | ⛔ |
| `load_and_validate(dir)` | Load both files; validate sidecar entry count against usearch `size()`; auto-repair the sidecar-ahead case when the gap is ≤ 100 entries; otherwise demand `ndex reindex --vectors`. | ⛔ |

## Crash-safety contract (intent, PRD §10.3 / §11.2) 📋

1. **Atomic writes:** usearch `save()` is not atomic → always save to `<name>.tmp` and `rename()` (atomic on POSIX incl. ZFS). Same pattern for the sidecar file. Stale `.tmp` files are deleted on startup.
2. **Save ordering — sidecar FIRST:** a crash between the two saves leaves the sidecar *ahead* of usearch, which is harmless (extra sidecar entries with no vector are ignored on lookup). The reverse (usearch ahead) would resolve labels to missing/zero entries — a data-integrity failure — and is precluded by the mandated ordering.
3. **Startup validation:** count mismatch ⇒ warn; if sidecar-ahead by ≤ 100, truncate the sidecar to usearch's count (auto-repair); any other mismatch ⇒ refuse and require `ndex reindex --vectors`.

## Test coverage

- Real behavior pinned: `sidecar_magic_is_distinct_and_fixed_width`, `empty_sidecar_is_empty` (characterization), `empty_sidecar_reports_empty` (unit).
- Intended contract pinned but ignored: characterization `vector_index_add_search_save_load` (`#[ignore = "impl pending: PR #3"]`) — open_or_create(dims=256) → add(1, 0, v) → `len() == 1` → self-search returns `file_id 1` first → save → `load_and_validate` → `len() == 1`; unit `save_then_load_validates_counts` (`#[ignore = "skeleton…"]`).
- Nothing tests: header round-trip, 24-byte record layout, save-ordering crash windows, auto-repair threshold behavior, tombstone visibility.

## Divergences & open questions

1. **Unreachable engine:** `Store` never constructs a `VectorIndex` (always `None`), yet the ignored store roundtrip test expects `vectors.is_some()` after a default-model create — see [layout & locking](21-layout-and-locking.md) divergence 2. Until wired, hybrid/auto search can only take the FTS fallback path, and explicit semantic returns zero hits with a warning (policy owned by [search](../40-search/41-search.md)).
2. **24-byte record vs 20 bytes of fields:** `SidecarEntry`'s declared fields (u64 + i64 + u32) leave 4 bytes of the documented fixed 24-byte record unaccounted for. Padding? Reserved flags (e.g., a tombstone bit — see 3)? Must be fixed before `save`/`load` are written, since it defines the file format.
3. **Tombstone lookup mechanism unspecified:** PRD §13.8 says "the sidecar lookup skips deleted file_ids", but `SidecarEntry` has no deleted flag and no deleted-set is defined. Options (flag byte in the reserved record space, in-memory set from the manifest's `status = 3` rows, usearch-level filtering) are all unchosen.
4. **Header layout unspecified:** the 128-byte header's exact byte layout (version width, count width, model-name length/truncation, endianness) exists nowhere; `Sidecar::load`'s "validating the magic header" is the only committed check.
5. **`is_empty()` panics today** (delegates to `todo!()` `len()`), so even the documented FTS-fallback probe cannot be called on a constructed index; the fallback currently works only because `Store.vectors` is `None`.
6. **`ef_search` application point unspecified:** PRD §10.3 makes it tunable via config, but `search(vector, k)` takes no ef parameter and no config is consulted in this module.
