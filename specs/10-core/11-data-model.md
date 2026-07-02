# Data Model

**Owns:** The shared domain types (file records, blocks/chunks, extracted metadata, embeddings), the file-status enum, the index identity file (`index.toml`) and schema-version gate, and the token-counting abstraction.

**Sources:** `crates/ndex-core/src/model.rs`, `crates/ndex-core/src/status.rs`, `crates/ndex-core/src/identity.rs`, `crates/ndex-core/src/tokens.rs`

These types are wire-agnostic: serialization behavior below is what the derived/custom serde impls produce in any self-describing format. The MessagePack-specific encoding is owned by [framing](../50-protocol/51-framing.md); the SQL schemas these types mirror are owned by [manifest](../20-store/22-manifest.md).

## `Blake3` ✅

```rust
pub type Blake3 = [u8; 32];
```

A 32-byte BLAKE3 content hash (PRD §4). Being a plain `[u8; 32]`, derived serde serializes it as a **32-element sequence of integers**, not as a byte string — this holds in JSON (array of numbers) and MessagePack (array, not `bin`).

## `FileRecord` ✅

One row of the manifest `files` table (PRD §10.1); the SQL DDL is owned by [manifest](../20-store/22-manifest.md). Derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` (plain derived struct serde: field-named map, no serde attributes).

| Field | Type | Notes |
|---|---|---|
| `file_id` | `i64` | SQLite `INTEGER PRIMARY KEY` (signed) |
| `path` | [`NdexPath`](12-paths.md) | raw bytes |
| `path_hash` | `u64` | [xxh3_64 of path bytes](12-paths.md) |
| `inode` | `Option<u64>` | `None` for archive members |
| `dev` | `Option<u64>` | |
| `size` | `u64` | |
| `mtime_ns` | `i64` | unix nanoseconds |
| `ctime_ns` | `i64` | unix nanoseconds |
| `mode` | `u32` | Unix mode bits |
| `uid` | `Option<u32>` | |
| `gid` | `Option<u32>` | |
| `blake3` | `Option<Blake3>` | `None` until processed |
| `mime_type` | `Option<String>` | |
| `status` | [`FileStatus`](#filestatus-) | serialized as bare integer, see below |
| `fail_count` | `u32` | transient-failure counter (PRD §11.5) |
| `first_seen_ns` | `i64` | |
| `last_verified_ns` | `i64` | |
| `error_msg` | `Option<String>` | last failure for diagnostics |
| `hard_link_of` | `Option<i64>` | canonical `file_id` if this path is a hard link (PRD §11.1); else `None` |
| `parent_archive_id` | `Option<i64>` | owning archive's `file_id` if archive member; else `None` |

Round-trip is pinned by `file_record_roundtrips` in `crates/ndex-core/tests/characterization.rs` (including a non-UTF-8 path and a `Some([0xab; 32])` hash).

## `WalkEntry` / `DirWalkEntry` ✅

Phase-1 walk metadata (PRD §11.1). Both derive `Debug, Clone, Copy, PartialEq, Eq` — **not** serde-serializable; they are in-process types consumed by [reconcile](../30-ingest/31-reconcile.md).

| Type | Fields |
|---|---|
| `WalkEntry` (regular file) | `size: u64`, `mtime_ns: i64`, `ctime_ns: i64`, `inode: u64`, `dev: u64`, `mode: u32` |
| `DirWalkEntry` (directory) | `mtime_ns: i64`, `ctime_ns: i64`, `inode: u64`, `dev: u64`, `mode: u32` (no `size` — PRD §11.1: directory size is meaningless) |

## Block model ✅

Produced by [extraction](../30-ingest/32-extraction.md), consumed by [chunking](../30-ingest/33-chunking.md). All three types derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` with no serde attributes.

### `BlockType`

```rust
pub enum BlockType {
    Heading(u8),
    Paragraph,
    CodeBlock(Option<String>),
    ListItem,
    Table,
    Quote,
    Raw,
}
```

Matches PRD §4.5's normalized block types (`Heading(level)`, `CodeBlock(lang)`). Derived externally-tagged serde: unit variants serialize as their name string (`"Paragraph"`, `"ListItem"`, `"Table"`, `"Quote"`, `"Raw"`); newtype variants as a single-key map (`{"Heading": 2}`, `{"CodeBlock": "rust"}`, `{"CodeBlock": null}`). All eight shapes round-trip per `block_types_all_roundtrip`.

### `Block`

An ordered, typed block with source byte offsets (PRD §4.5):

| Field | Type | Notes |
|---|---|---|
| `block_type` | `BlockType` | |
| `text` | `String` | |
| `byte_start` | `u64` | offset into the source file |
| `byte_end` | `u64` | |
| `heading_path` | `Vec<String>` | most-recent heading context, propagated to chunks (PRD §4.5) |

### `Chunk`

A unit of text to be indexed and embedded (PRD §4.5):

| Field | Type |
|---|---|
| `file_id` | `i64` |
| `chunk_ord` | `u32` |
| `byte_start` | `u64` |
| `byte_end` | `u64` |
| `block_type` | `BlockType` |
| `text` | `String` |

This carries exactly the PRD §4.5 chunk tuple `(file_id, chunk_ord, byte_start, byte_end, block_type)` plus the text itself. Round-trip pinned by `chunk_and_meta_roundtrip`.

## `ProcessedFile` ✅

Result of running one file through the extraction pipeline (PRD §4.3). Derives `Debug, Clone, PartialEq` only — **not** serde-serializable and not `Eq` (contains `MediaMeta` floats); it is the in-process hand-off between extraction and the index writers.

| Field | Type |
|---|---|
| `blake3` | `Blake3` |
| `mime_type` | `String` |
| `chunks` | `Vec<Chunk>` |
| `doc_meta` | `Option<DocMeta>` |
| `media_meta` | `Option<MediaMeta>` |
| `lang` | `Option<String>` |

## Extracted metadata ✅

All three mirror `meta.db` tables (PRD §10.4); the SQL is owned by [manifest/meta store](../20-store/22-manifest.md). All derive `Debug, Clone, Default, Serialize, Deserialize` and carry container-level `#[serde(default)]` — a payload missing any (or every) field decodes with the missing fields defaulted, satisfying the additive-evolution rule (PRD §12.3; wire consequences owned by [protocol messages](../50-protocol/53-messages.md)). Pinned by `meta_structs_msgpack_decode_from_empty_map` in `crates/ndex-core/tests/characterization.rs`. Every field is `Option`, so `..Default::default()` struct-update is the intended construction style (used throughout the characterization tests).

### `DocMeta` (`Eq`)

Fields, all `Option`: `title`, `author`, `subject`, `creator`, `producer`, `created_at`, `modified_at` (all `Option<String>`; timestamps are ISO 8601 strings, not parsed types), `page_count: Option<u32>`, `word_count: Option<u32>`, `lang: Option<String>` (ISO 639-1).

### `MediaMeta` (`PartialEq` only — float fields)

| Field | Type |
|---|---|
| `width`, `height` | `Option<u32>` |
| `duration_ms` | `Option<u64>` |
| `codec` | `Option<String>` |
| `bitrate` | `Option<u32>` |
| `fps` | `Option<f32>` |
| `camera_make`, `camera_model`, `lens` | `Option<String>` |
| `iso` | `Option<u32>` |
| `focal_length`, `aperture` | `Option<f32>` |
| `shutter_speed` | `Option<String>` |
| `gps_lat`, `gps_lon`, `gps_alt` | `Option<f64>` |
| `taken_at` | `Option<String>` |

`lens` is deliberately present: the doc comment declares it a "skeleton reconciliation of PRD §12.7" (see Divergences).

### `ArchiveMeta` (`Eq`)

Fields, all `Option`: `member_count: Option<u32>`, `total_size: Option<u64>` (uncompressed bytes), `format: Option<String>`, `extraction_status: Option<String>`. `extraction_status`'s documented vocabulary is `complete` | `partial` | `metadata_only` (PRD §10.4) but the type is a free-form string — nothing enforces the vocabulary.

## `Embedding` ✅

```rust
pub struct Embedding(pub Vec<f16>);   // half::f16
```

Doc-comment contract: a 256-dimensional, MRL-truncated, L2-normalized embedding stored as `f16` (PRD §10.3; quoting the `model.rs` rustdoc — the authoritative dimension spec is the model registry in [embedding](../30-ingest/34-embedding.md)). The type enforces **none** of this — any length is accepted (`embedding_dims_reports_length` exercises lengths 2 and 0). Derives `Debug, Clone, PartialEq, Serialize, Deserialize`. Single method: `dims(&self) -> usize` returns `self.0.len()`. Production/normalization is owned by [embedding](../30-ingest/34-embedding.md); storage by [vectors](../20-store/24-vectors.md).

## `FileStatus` ✅

`crates/ndex-core/src/status.rs`. The processing status of a manifest entry (PRD §10.1 `files.status`).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
#[repr(u8)]
pub enum FileStatus { ... }
```

| Variant | Discriminant | Meaning |
|---|---|---|
| `Pending` | 0 | inserted, not yet processed |
| `Indexed` | 1 | fully indexed |
| `FailedTransient` | 2 | failed; eligible for retry |
| `Deleted` | 3 | no longer present on disk |
| `FailedPermanent` | 4 | retry limit reached, unsupported, etc. |
| `Skipped` | 5 | intentionally not indexed (too large, binary, depth limit, …) |

Serialization is a **bare integer** in every format (never a tagged enum), via `#[serde(into = "u8", try_from = "u8")]`. `TryFrom<u8>` rejects any value outside 0..=5 with `InvalidFileStatus(u8)` — a standalone error type implementing `Display` (`"invalid file status discriminant: {n}"`) and `std::error::Error`; it is *not* an `NdexError` variant. Helper: `const fn as_u8(self) -> u8`.

Discriminant values, rejection of unknown integers, and bare-integer serde are pinned by `filestatus_discriminants_are_stable`, `filestatus_try_from_roundtrips_and_rejects_unknown`, `filestatus_serde_is_a_bare_integer` (characterization) plus the module's own `u8_roundtrip` / `serde_is_a_bare_integer` unit tests. Values match the PRD §10.1 comment (`0=pending … 5=skipped`).

## Index identity — `index.toml` ✅

`crates/ndex-core/src/identity.rs` (PRD §5.3). Written once at `init`, never modified; the filename constant lives in [config/constants](13-config.md), and the on-disk layout in [store layout](../20-store/21-layout-and-locking.md).

```rust
pub const SCHEMA_VERSION: u32 = 3;
```

Bumped on any breaking index-format change; ndex refuses to open an index with a different version and requires a full rebuild (PRD §5 no-migrations policy).

All four structs derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`, no serde attributes, so the TOML layout is four tables named after the fields:

| TOML table | Struct | Fields |
|---|---|---|
| `[identity]` | `Identity` | `schema_version: u32`, `created_by: String`, `created_at: String` (ISO 8601 by convention, not parsed) |
| `[embedding]` | `EmbeddingIdentity` | `model_name: String`, `model_hash: String` (BLAKE3 of the ONNX model file, hex; **empty = unpinned** — written when the registry has no release hash yet, or for `--model none`; see [embedding registry](../30-ingest/34-embedding.md)), `dimensions: u32`, `mrl_dimensions: u32`, `vector_scalar: String` (e.g. `"f16"`), `hnsw_m: u32`, `hnsw_ef_construction: u32` |
| `[hashing]` | `Hashing` | `algorithm: String` |
| `[fts]` | `FtsIdentity` | `tokenizer_version: u32` |

`IndexIdentity` (the root) has methods:

- `load(path: &Path) -> Result<Self>` — reads the file: a missing file (`ErrorKind::NotFound`) → `NdexError::IndexNotFound` carrying the path (exit 3); any other I/O failure → `NdexError::Io`; then `toml::from_str` (parse failure → `NdexError::Config`). Pinned by `identity_load_missing_file_is_index_not_found` and `identity_load_malformed_toml_is_config_error`. See [errors](14-errors.md) for the exit-code mapping.
- `to_toml(&self) -> Result<String>` — TOML render; serialize failure → `NdexError::Config`. Round-trip pinned by `identity_toml_roundtrip`.
- `check_compatible(&self) -> Result<()>` — errors with `NdexError::SchemaMismatch` iff `identity.schema_version != SCHEMA_VERSION`, message ``"index schema version {found} is not supported (this build expects {expected}); run `ndex reindex`"``. Pinned by `identity_schema_gate` (asserts the variant and its exit code). **Only** `schema_version` is checked — see Divergences.

## `TokenCounter` ✅

`crates/ndex-core/src/tokens.rs`.

```rust
pub trait TokenCounter {
    fn count(&self, text: &str) -> usize;
}
```

Counts model tokens in a string. Defined in `ndex-core` (not `ndex-embed`) specifically so the [chunker](../30-ingest/33-chunking.md) can size chunks in model tokens without an `ndex-extract` → `ndex-embed` dependency edge; the real implementation is [ndex-embed's tokenizer](../30-ingest/34-embedding.md). The trait is object-safe (`&dyn TokenCounter` usage pinned by `token_counter_is_object_safe`). No `Send`/`Sync` supertraits.

## Divergences & open questions

- **`MediaMeta.lens` vs PRD §12.7.** PRD §10.4's `media_meta` SQL table has a `lens` column, but PRD §12.7's wire `MediaMeta` struct omits it. The code includes `lens` in the single shared struct and its doc comment declares this a deliberate "skeleton reconciliation". The PRD is internally inconsistent; code standardizes on including `lens`.
- **`check_compatible` is narrower than PRD §5.3.** The PRD also requires: "If embedding model differs, disable semantic search with a warning." No core API compares `model_name`/`model_hash`, and nothing checks `fts.tokenizer_version` or `hashing.algorithm`. Either a higher layer must implement the model check, or it is unimplemented.
- **`file_id` signedness.** Core uses `i64` (matching SQLite `INTEGER PRIMARY KEY`); PRD §12.7's wire `SearchHit`/`FileInfo` declare `file_id: u64`. Whichever the protocol crate uses, one side converts.
- **Unenforced `Embedding` invariants.** Dimensionality (256), MRL truncation, and L2 normalization are doc-comment claims only; the type is an unvalidated `Vec<f16>` newtype.
- **Stringly-typed `ArchiveMeta.extraction_status`** carries a three-value vocabulary as `Option<String>` with no enum, unlike `FileStatus` which got the full validated-integer treatment.
- **Timestamps are strings.** `Identity.created_at`, `DocMeta.created_at/modified_at`, `MediaMeta.taken_at` are unparsed `Option<String>`/`String` despite `jiff` being a declared dependency of the crate (currently unused in `src/`).
