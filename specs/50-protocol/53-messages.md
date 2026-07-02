# 53 — Message Types & Payloads

**Owns:** every wire message enum and payload struct in the protocol crate, their field-level wire encodings (including how re-exported core types serialize), and the request/response pairing contract (PRD §12.4–§12.7).

**Sources:**
- `crates/ndex-protocol/src/message.rs` — all enums and payload structs below
- `crates/ndex-protocol/src/lib.rs` — public re-exports (all message types, plus the core wire types `DocMeta`, `MediaMeta`, `NdexPath`, `SearchFilters`, `SearchMode` for consumers that shouldn't depend on `ndex-core` paths)
- Wire encodings of core types follow their `Serialize` impls in `crates/ndex-core/src/` (`path.rs`, `filters.rs`, `model.rs`, `progress.rs`) — the types themselves are owned by [12-paths](../10-core/12-paths.md), [15-search-and-progress-types](../10-core/15-search-and-progress-types.md), and [11-data-model](../10-core/11-data-model.md)
- Pinned by `crates/ndex-protocol/tests/characterization.rs` and the unit tests in `message.rs`

Everything in this document is ✅ **implemented and round-trip-tested as a type**. Runtime *behavior* (who sends what when) is the ⛔ serve loop / session ([63-remote](../60-interfaces/63-remote.md), [62-client-transport](../60-interfaces/62-client-transport.md)); the pairing table in §4 is the contract they must implement.

## 1. Serialization rules ✅

All values are encoded via the named-mode codec ([51-framing](51-framing.md)); the rules below define what that produces.

**Enum tagging.** `ClientMessage` and `ServerMessage` are **externally tagged** — serde's default, deliberately: internally tagged (`#[serde(tag)]`) and adjacently tagged enums have known `rmp-serde` deserialization bugs (issues #153, #250; PRD §12.4) and must not be used.

- Unit variant → a bare MessagePack `str` of the variant name. Pinned byte-shape: `ClientMessage::CancelRequest` decodes as the string `"CancelRequest"`, `StatsRequest` as `"StatsRequest"` (`unit_variant_encodes_as_bare_variant_name`).
- Newtype (payload-carrying) variant → a **single-entry map** `{"VariantName": payload}`. Pinned: `DeleteRequest` and `Error` decode as one-key maps keyed by exactly the variant name (`tuple_variant_encodes_as_single_key_map_keyed_by_variant_name`). Matches PRD §12.4's example `{"SearchRequest": {"query": …}}`.
- Unit-only utility enums (`OutputFormat`, `ReindexTarget`, and core `SearchMode`) → the variant-name `str` (`"Pretty"`, `"Vectors"`, `"Hybrid"`, …). Rust-cased, not lowercased — CLI-level lowercase parsing is a client concern ([61-client-cli](../60-interfaces/61-client-cli.md)).

**Struct encoding.** Every payload struct serializes as a MessagePack map with field-name `str` keys. **Every field is always written** — there is no `skip_serializing_if` anywhere; `Option::None` is written as `nil`. Field order is declaration order (not relied upon: decoding is by key).

**Forward compatibility.** Every struct defined in `message.rs` carries container-level `#[serde(default)]`: a decoder fills any missing field with its `Default` value, and serde ignores unknown map keys (no `deny_unknown_fields`). Pinned by `payload_structs_roundtrip_at_their_defaults` (all 23 structs at their defaults). The compatibility rules this serves are owned by [52-handshake](52-handshake.md). Core-owned structs embedded in messages do **not** all carry it — see Divergences.

## 2. Wire encoding conventions ✅

| Rust type | MessagePack encoding | Notes |
|---|---|---|
| `u16` / `u32` / `u64` / `i64` | int (minimal-width, chosen by `rmp`) | |
| `bool` | bool | |
| `f32` / `f64` | float32 / float64 | |
| `String` | str | |
| `Vec<String>` | array of str | empty vec → empty array, still written |
| `Option<T>` | nil, or T's encoding | key always present |
| `NdexPath` | **bin** (raw bytes; `serialize_bytes` in `crates/ndex-core/src/path.rs`) | never a lossy string; non-UTF-8 bytes survive — pinned via `sample_path()` containing `0xff` in every path-bearing message. Decoder also tolerates str/array input (its visitor accepts bytes, str, and seq). Type semantics: [12-paths](../10-core/12-paths.md) |
| `Vec<u8>` (hash fields) | **array of ints** — *not* bin | serde has no `Vec<u8>` specialization and no `serde_bytes` is used; applies to `FileInfo.blake3`, `CorruptedFile.stored_hash` / `actual_hash`. See Divergences |
| unit-only enums | variant-name str | see §1 |
| nested structs | map | see §1 |

## 3. The two message enums ✅

### `ClientMessage` (client → server, PRD §12.4)

Nine variants — the count is pinned (`all_client_variants_are_covered`), and every variant round-trips fully populated (`every_client_message_roundtrips`).

| Variant | Payload | Purpose |
|---|---|---|
| `Handshake` | `HandshakeReq` | First frame; version/capability advertisement ([52-handshake](52-handshake.md)) |
| `SearchRequest` | `SearchRequestData` | Execute a search |
| `IndexRequest` | `IndexOptions` | Build/update the index |
| `InfoRequest` | `InfoRequestData` | Per-file metadata lookup |
| `StatsRequest` | *(unit)* | Whole-index summary |
| `VerifyRequest` | `VerifyRequestData` | Hash-verify file integrity |
| `ReindexRequest` | `ReindexRequestData` | Rebuild an index component |
| `DeleteRequest` | `DeleteRequestData` | Remove files from the index |
| `CancelRequest` | *(unit)* | Cancel the in-flight operation |

There is deliberately no `InitRequest` — remote init is out of scope for v0.1 (PRD §13.4); initialization happens via the standalone `ndex-remote init` ([63-remote](../60-interfaces/63-remote.md)). 📋 for v0.2.

### `ServerMessage` (server → client, PRD §12.4)

Nine variants — count pinned (`all_server_variants_are_covered`); all round-trip (`every_server_message_roundtrips`).

| Variant | Payload | Terminal? |
|---|---|---|
| `Handshake` | `HandshakeResp` | terminal (for the handshake) |
| `SearchResult` | `SearchResultData` | terminal |
| `IndexComplete` | `IndexCompleteData` | terminal |
| `InfoResult` | `InfoResultData` | terminal |
| `StatsResult` | `StatsResultData` | terminal |
| `VerifyResult` | `VerifyResultData` | terminal |
| `DeleteResult` | `DeleteResultData` | terminal |
| `Progress` | `ProgressEvent` | **never terminal** — interleaved before a terminal message |
| `Error` | `ErrorData` | terminal (for any request) |

- The enum carries `#[allow(clippy::large_enum_variant)]`: `InfoResult` embeds a large inline `FileInfo`, and the documented rationale (doc comment in `message.rs`) is that server messages are constructed and sent one at a time, never stored in bulk, so boxing isn't worth the friction.
- There is no `HandshakeErr` and no `ReindexResult`/`CancelAck` — see §4 and [52-handshake](52-handshake.md).

## 4. Request/response pairing

The protocol contract (PRD §12.4, §16.2 decision 2). Any request may instead receive `Error`. `Progress` may be interleaved any number of times before the terminal message. Enforcement is the ⛔ serve loop's job.

| Request | Terminal response | Notes |
|---|---|---|
| `Handshake` | `Handshake` \| `Error` (version-incompatible) | [52-handshake](52-handshake.md) |
| `SearchRequest` | `SearchResult` | |
| `IndexRequest` | `IndexComplete` | `Progress` streamed during the run |
| `InfoRequest` | `InfoResult` | |
| `StatsRequest` | `StatsResult` | |
| `VerifyRequest` | `VerifyResult` | |
| `ReindexRequest` | `IndexComplete` | **reused** — there is deliberately no `ReindexResult` (PRD §12.4 note); clients treat reindex completion identically to index completion |
| `DeleteRequest` | `DeleteResult` | |
| `CancelRequest` | *(none of its own)* | Asynchronous (PRD §16.2): the in-flight operation's normal terminal message arrives with `IndexCompleteData.cancelled = true`; there is no `CancelAck`. Cancel of a search is a no-op. |

## 5. Utility enums ✅

Owned here (defined in `message.rs`):

**`OutputFormat`** (PRD §13.2) — `Pretty` (default), `Plain`, `Json`, `Jsonl`, `Paths`, `Csv`. Every variant round-trips and the default is pinned (`enums_roundtrip_every_variant`). Wire: variant-name str.

**`ReindexTarget`** (PRD §12.7) — `All` (default), `Vectors`, `Fts`. Same pinning. Wire: variant-name str.

Core-owned enums appearing on the wire: `SearchMode` ([15-search-and-progress-types](../10-core/15-search-and-progress-types.md)) encodes as its variant-name str.

## 6. Payload structs, field by field ✅

All structs: container-level `#[serde(default)]`, `PartialEq`. Six are not `Eq` because they carry floats directly or via nested non-`Eq` types: `SearchRequestData` (via `SearchFilters`), `VerifyRequestData`, `SearchHit`, `SearchResultData`, `FileInfo` (via `MediaMeta`), `InfoResultData`. Wire types per §2.

### Handshake payloads (PRD §12.3, §12.7)

**`TerminalCaps`** — client terminal capabilities, sent so the server *could* tailor output (but see Divergences #6).

| Field | Type | Meaning |
|---|---|---|
| `width` / `height` | `u16` | Terminal cell dimensions |
| `color` | `bool` | ANSI color support |
| `hyperlinks` | `bool` | OSC 8 support |
| `unicode` | `bool` | Unicode-capable output |

**`HandshakeReq`** — the first client frame.

| Field | Type | Meaning |
|---|---|---|
| `min_protocol` / `max_protocol` | `u32` | Supported protocol range; values come from the constants in [52-handshake](52-handshake.md) |
| `client_version` | `String` | Client build version (e.g. `"0.1.0"`) |
| `capabilities` | `Vec<String>` | Undefined vocabulary in v0.1 ([52-handshake](52-handshake.md) §5) |
| `terminal` | `TerminalCaps` | nested map |

**`HandshakeResp`** — the server's reply.

| Field | Type | Meaning |
|---|---|---|
| `protocol_version` | `u32` | Negotiated version ([52-handshake](52-handshake.md)) |
| `server_version` | `String` | Server build version |
| `index_schema_version` | `u32` | Index schema identity ([11-data-model](../10-core/11-data-model.md)) |
| `index_model` | `String` | Embedding model shortname |
| `index_file_count` | `u64` | Files known to the manifest |
| `index_last_reconciled_ns` | `i64` | Unix ns of last reconcile, for client-side staleness display; **no "never" sentinel defined** (contrast `IndexSummary.last_reconciled_ns: Option<i64>`) |
| `capabilities` | `Vec<String>` | As above |
| `index_healthy` | `bool` | Server-judged index health |

### Request payloads (PRD §12.4, §12.7)

**`SearchRequestData`**

| Field | Type | Meaning |
|---|---|---|
| `query` | `String` | FTS syntax or natural language |
| `mode` | `SearchMode` | Requested mode ([15-search-and-progress-types](../10-core/15-search-and-progress-types.md)); str on wire |
| `filters` | `SearchFilters` | Nested map; fields owned by [15-search-and-progress-types](../10-core/15-search-and-progress-types.md). Only its `tags` field has `#[serde(default)]` — see Divergences #4 |
| `limit` | `u32` | Max hits to return (CLI default owned by [61-client-cli](../60-interfaces/61-client-cli.md)) |
| `offset` | `u32` | Pagination offset |
| `format` | `OutputFormat` | Client's chosen rendering format (see Divergences #6) |
| `explain` | `bool` | Requests per-component scores (`score_fts`/`score_vec`) in hits |

**`IndexOptions`** — payload of `IndexRequest`; mirrors the `ndex index` flags ([61-client-cli](../60-interfaces/61-client-cli.md)).

| Field | Type | Meaning |
|---|---|---|
| `full` | `bool` | Force full re-index |
| `verify` | `bool` | Recompute BLAKE3 for unchanged files |
| `dry_run` | `bool` | Report changes without writing |
| `jobs` | `Option<u32>` | Extraction parallelism; `nil` = server default |
| `batch_size` | `Option<u32>` | Embedding batch size; `nil` = server default |
| `no_vectors` | `bool` | Skip embedding |
| `enable_ner` | `bool` | **v0.1: accepted but ignored with a warning** (PRD §13.3) — kept on the wire so v0.2 clients interoperate with v0.1 servers |
| `max_file_size` | `Option<u64>` | Skip files larger than this many bytes |
| `only_new` | `bool` | Process only new files, skip modified |

**`InfoRequestData`** — `path: NdexPath` (bin). Single field.

**`VerifyRequestData`**

| Field | Type | Meaning |
|---|---|---|
| `paths` | `Option<Vec<NdexPath>>` | `nil` = verify the whole index; else array of bin |
| `sample` | `Option<f64>` | Sampling fraction (test fixture uses `0.01`) |

**`ReindexRequestData`** — `target: ReindexTarget` (str). Single field.

**`DeleteRequestData`**

| Field | Type | Meaning |
|---|---|---|
| `glob` | `String` | Path glob selecting index entries to remove |
| `dry_run` | `bool` | Report without deleting |

### Result payloads (PRD §12.4, §12.7)

**`SearchHit`** — one result row.

| Field | Type | Meaning |
|---|---|---|
| `file_id` | `u64` | Manifest file id (note: the manifest row type uses `i64` — Divergences #5) |
| `chunk_ord` | `u32` | Chunk ordinal within the file |
| `path` | `NdexPath` | bin |
| `score` | `f32` | Normalized `[0,1]` for display |
| `score_raw` | `f32` | Raw BM25 / cosine / RRF score |
| `score_fts` | `Option<f32>` | BM25 component; populated with `explain` |
| `score_vec` | `Option<f32>` | Cosine component; populated with `explain` |
| `mime` | `String` | MIME type |
| `size` | `u64` | File size, bytes |
| `mtime_ns` | `i64` | Unix ns mtime |
| `tags` | `Vec<String>` | |
| `snippet` | `Option<String>` | Highlighted, HTML-escaped excerpt (PRD §12.7) |
| `byte_start` / `byte_end` | `u64` | Chunk byte offsets in the source file |

**`SearchResultData`**

| Field | Type | Meaning |
|---|---|---|
| `hits` | `Vec<SearchHit>` | At most `limit` entries |
| `total` | `u64` | Total matches (not just returned) |
| `mode` | `SearchMode` | The mode actually executed (post-`Auto` resolution; [41-search](../40-search/41-search.md)) |
| `duration_ms` | `u64` | Server-side search time |
| `truncated` | `bool` | Result set was cut off |
| `stale_warning` | `Option<String>` | Human-readable staleness note |

**`IndexStats`** — per-run counters (PRD §12.7).

| Field | Type | Meaning |
|---|---|---|
| `new` / `modified` / `deleted` / `unchanged` | `u64` | Reconcile diff outcome ([31-reconcile](../30-ingest/31-reconcile.md)) |
| `processed` / `failed` / `skipped` | `u64` | Processing outcome |
| `duration_ms` | `u64` | Run duration |
| `timed_out` | `bool` | Run hit its time budget |

**`IndexCompleteData`** — `stats: IndexStats` + `cancelled: bool` (`true` iff stopped by `CancelRequest`, PRD §16.2).

**`FileInfo`** — detailed per-file record for `InfoResult`.

| Field | Type | Meaning |
|---|---|---|
| `file_id` | `u64` | |
| `path` | `NdexPath` | bin |
| `size` | `u64` | bytes |
| `mtime_ns` / `ctime_ns` | `i64` | Unix ns |
| `mime` | `Option<String>` | |
| `blake3` | `Option<Vec<u8>>` | 32-byte content hash — wire-encoded as an **int array**, not bin (§2) |
| `status` | `u8` | **Raw `u8` on the wire** (PRD §12.7); value meanings are the `FileStatus` lifecycle owned by [11-data-model](../10-core/11-data-model.md) |
| `fail_count` | `u32` | Consecutive extraction failures |
| `error_msg` | `Option<String>` | Last failure message |
| `tags` | `Vec<String>` | |
| `doc_meta` | `Option<DocMeta>` | Nested map; fields owned by [11-data-model](../10-core/11-data-model.md); no `#[serde(default)]` (Divergences #4) |
| `media_meta` | `Option<MediaMeta>` | Nested map; ditto — and it carries a `lens` field absent from PRD §12.7 (Divergences #3) |
| `chunk_count` | `u32` | |
| `in_fts` / `in_vectors` | `bool` | Presence in each index component |

**`InfoResultData`** — `file_info: FileInfo`. Single field.

**`IndexSummary`** — whole-index snapshot for `StatsResult` (PRD §12.7).

| Field | Type | Meaning |
|---|---|---|
| `total_files` | `u64` | Includes directory entries |
| `directories` | `u64` | Counted in `total_files`, never in `processed` |
| `indexed` / `pending` / `failed_transient` / `failed_permanent` / `skipped` / `deleted` | `u64` | Counts by status ([11-data-model](../10-core/11-data-model.md)) |
| `manifest_bytes` / `fts_bytes` / `vector_bytes` / `meta_bytes` | `u64` | On-disk component sizes |
| `last_reconciled_ns` | `Option<i64>` | `nil` = never reconciled |
| `schema_version` | `u32` | |
| `model_name` | `String` | |

**`StatsResultData`** — `index_stats: IndexSummary`. Single field.

**`CorruptedFile`** — one verification mismatch.

| Field | Type | Meaning |
|---|---|---|
| `file_id` | `u64` | |
| `path` | `NdexPath` | bin |
| `stored_hash` / `actual_hash` | `Vec<u8>` | int arrays on the wire (§2) |

**`VerifyResultData`** — `checked: u64` + `corrupted: Vec<CorruptedFile>`.

**`DeleteResultData`** — `deleted: u64` + `paths: Vec<NdexPath>` (array of bin; the removed entries).

### Streaming and error payloads

**`ProgressChild`** — one sub-task within a phase (an extraction worker, the embed/fts-write sub-pipelines).

| Field | Type | Meaning |
|---|---|---|
| `label` | `String` | e.g. `"worker-3"` |
| `current` | `u64` | |
| `total` | `Option<u64>` | `nil` = indeterminate |
| `message` | `Option<String>` | |

**`ProgressEvent`** — streamed during long operations; the server sends structure, the client decides rendering (PRD §13.7).

| Field | Type | Meaning |
|---|---|---|
| `phase` | `String` | Free-form on the wire; the vocabulary is defined by the [remote's `phase_name` mapping](../60-interfaces/63-remote.md) (PRD §13.7) |
| `current` | `u64` | |
| `total` | `Option<u64>` | `nil` = indeterminate |
| `message` | `Option<String>` | |
| `children` | `Vec<ProgressChild>` | Nested sub-task bars |

The engine-side twin (`ProgressUpdate`/`ProgressKind`, owned by [15-search-and-progress-types](../10-core/15-search-and-progress-types.md)) is mapped to `ProgressEvent` by a sink in `ndex-remote` — that mapping (including the `ProgressKind` → phase-string translation) is ⛔ unimplemented ([63-remote](../60-interfaces/63-remote.md)).

**`ErrorData`**

| Field | Type | Meaning |
|---|---|---|
| `code` | `u32` | Mirrors the CLI exit-code table owned by [14-errors](../10-core/14-errors.md) |
| `message` | `String` | Human-readable description |

## 7. Characterization coverage

`crates/ndex-protocol/tests/characterization.rs` pins, for this document's scope:

- Round-trip of **every** `ClientMessage`/`ServerMessage` variant, fully populated — the PRD §12.4-mandated format-stability proof — plus variant-count guards (9/9) so new variants can't ship without a sample.
- Round-trip of all 23 payload structs at `Default::default()` (the `#[serde(default)]` contract).
- Round-trip of every `OutputFormat`/`ReindexTarget` variant and their documented defaults.
- The external-tagging wire shapes (bare str for unit variants; single-key map for payload variants).
- Non-UTF-8 path bytes (`0xff`) surviving in every path-bearing payload (the bin encoding).
- Decode totality: truncated and garbage bytes → `Err`, never panic.

Not pinned: the byte-level shape of `Vec<u8>` hash fields, the phase-string vocabulary, and any cross-version decode (e.g. old bytes with a field missing, or extra unknown fields) — the defaults test covers same-version defaults only.

## Divergences & open questions

1. **Paths are `NdexPath`, not PRD's `Vec<u8>`.** PRD §12.4/§12.7 type all paths as `Vec<u8>`. Code uses `NdexPath`, whose custom impl produces MessagePack **bin**; a literal `Vec<u8>` under `rmp-serde` would produce an int array. Code wins; the PRD structs were never wire-exact.
2. **Two different encodings for byte blobs.** Paths go as bin, but the hash fields (`FileInfo.blake3`, `CorruptedFile.stored_hash`/`actual_hash`) remain plain `Vec<u8>` and therefore encode as arrays of ints (~2–3× the bytes of bin for a 32-byte hash, and shape-inconsistent). No test pins the hash byte shape, so switching to `serde_bytes`/bin would be invisible to the current suite.
3. **Wire `MediaMeta` includes `lens`.** Absent from PRD §12.7; the addition is a documented skeleton reconciliation on the type (owned by [11-data-model](../10-core/11-data-model.md)) but is wire-visible through `FileInfo.media_meta`.
4. **Forward-compat is uneven across the wire surface.** Every `message.rs` struct has container-level `#[serde(default)]`, but the embedded core types do not: `SearchFilters` has it only on its `tags` field, and `DocMeta`/`MediaMeta` not at all. Serde errors on a *missing* field without a default — so adding a field to any of those three structs breaks decoding of older peers' messages, violating the additive-fields rule in [52-handshake](52-handshake.md) §4. Invisible today only because the codec always writes every field.
5. **`file_id` signedness differs from the manifest.** Wire types use `file_id: u64`; the manifest row (`FileRecord`, [11-data-model](../10-core/11-data-model.md)) uses `i64` (SQLite rowid). The conversion point and overflow/negative policy are unspecified.
6. **The server receives rendering concerns.** PRD §13.7 states "the remote knows nothing about terminal capabilities — it sends structured progress, the client decides how to display", yet the handshake ships `TerminalCaps` (PRD §12.3) and `SearchRequestData` ships `format: OutputFormat` (PRD §12.4) to the server. The code faithfully implements both PRD sections; the tension is inherited from the PRD, and nothing in v0.1 defines what the server should do with either field.
7. **`HandshakeResp.index_last_reconciled_ns` has no "never" sentinel.** It is a bare `i64` (default `0`), while the equivalent `IndexSummary` field is `Option<i64>`. A fresh index presumably reports `0` — indistinguishable from a 1970 timestamp — but no code assigns it yet.
8. **Phase strings are unconstrained.** `ProgressEvent.phase` is free-form; PRD §13.7's vocabulary is convention only, and the core `ProgressKind` → string mapping does not exist yet.
