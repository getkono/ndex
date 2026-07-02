# Configuration & Constants

**Owns:** The server-side `config.toml` model (all sections, field types, defaults, parse/render semantics), the `ByteSize`/`DurationSetting` value types, and the link index of every compile-time constant in `ndex-core` — a constant's value is owned by the domain doc that specs its semantics; the remaining store/config constants are owned here.

**Sources:** `crates/ndex-core/src/config.rs`, `crates/ndex-core/src/constants.rs`

## `ByteSize` ✅

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSize(pub u64);           // .bytes() -> u64
```

**`FromStr` grammar** (`"2GiB"`, `"512MB"`, `"1024"`): trim the input; split at the first ASCII-alphabetic char into number and unit; parse the number as `f64` (so fractions like `"1.5KiB"` = 1536 work, truncating toward zero); the unit is trimmed and lowercased. Whitespace between number and unit is tolerated (`"  2 GiB  "`). Empty input, non-numeric prefix, and unknown units are `Err(String)`. A negative number is `Err("negative byte size")` — the same wording as the serde-integer path.

| Unit (case-insensitive) | Multiplier |
|---|---|
| *(none)*, `b` | 1 |
| `k`, `kb` | 10³ |
| `kib` | 2¹⁰ |
| `m`, `mb` | 10⁶ |
| `mib` | 2²⁰ |
| `g`, `gb` | 10⁹ |
| `gib` | 2³⁰ |
| `t`, `tb` | 10¹² |
| `tib` | 2⁴⁰ |

The final value is `num * mult` computed in `f64` and range-checked before the cast: a product that is non-finite or `>= u64::MAX as f64` (i.e. 2⁶⁴ — the smallest `f64` above `u64::MAX`, so exactly the values a `u64` cannot hold) is `Err("byte size out of range (max {u64::MAX} bytes): …")` rather than saturating. Consequence of the `f64` path: exact `u64::MAX` input rounds up to 2⁶⁴ and is rejected; the practical ceiling is the largest `f64` below 2⁶⁴. Grammar pinned by `bytesize_decimal_and_binary_units`, `bytesize_accepts_fractional_and_whitespace`, `bytesize_rejects_garbage`, `bytesize_rejects_negative_strings`, `bytesize_rejects_overflow` in `crates/ndex-core/tests/characterization.rs`.

**Serde:** serializes as a raw `u64` byte count; deserializes from `u64`, from `i64` (negative → error `"negative byte size"`), or from a string via `FromStr`. Pinned by `bytesize_serializes_as_raw_u64`. Consequence: `Config::to_toml` re-renders human strings as integers (e.g. `"2GiB"` → `2147483648`); round-trip is value-lossless but not text-preserving.

## `DurationSetting` ✅

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationSetting(pub Duration);   // .as_duration(), .secs() -> u64
```

**`FromStr` grammar** (`"1h"`, `"7d"`, `"30"`): same trim/split scheme as `ByteSize`, but the number parses as **`u64`** — fractions and negatives are rejected (unlike `ByteSize`, which accepts fractions). The unit multiplication is `checked_mul`: a product overflowing `u64` seconds is `Err("duration out of range (overflows u64 seconds): …")`.

| Unit (case-insensitive) | Seconds |
|---|---|
| *(none)*, `s`, `sec`, `secs` | ×1 |
| `m`, `min`, `mins` | ×60 |
| `h`, `hr`, `hrs` | ×3 600 |
| `d`, `day`, `days` | ×86 400 |
| `w`, `wk`, `wks` | ×604 800 |

**Serde:** serializes as a raw `u64` second count; deserializes from `u64`/`i64` (negative → `"negative duration"`) or a string via `FromStr`. Pinned by `duration_units`, `duration_as_duration_and_serde`, and `duration_rejects_overflow`.

## `Config` — the server `config.toml` ✅

```rust
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config { chunking, extraction, embedding, auto_refresh, ignore, walk, search, archive }
```

Every section struct also carries `#[serde(default)]` and a hand-written `Default`, so a missing or partial file loads with per-field defaults (PRD §17): empty TOML == `Config::default()` (pinned by `config_empty_toml_is_all_defaults`, `config_partial_toml_fills_defaults_and_roundtrips`). Unknown keys are silently ignored (no `deny_unknown_fields`).

Methods: `load(&Path)` (I/O failure → `NdexError::Io`), `from_toml(&str)` / `to_toml()` (TOML failure → `NdexError::Config`; variant and its exit code pinned by `config_invalid_toml_is_config_error`). Exit-code values are owned by [errors](14-errors.md).

Defaults below are pinned wholesale by `config_default_matches_prd_section_17` and match PRD §17's consolidated reference. Semantics of each knob are owned by the consuming subsystem: [chunking](../30-ingest/33-chunking.md), [extraction](../30-ingest/32-extraction.md), [embedding](../30-ingest/34-embedding.md), [reconcile](../30-ingest/31-reconcile.md), [search](../40-search/41-search.md).

### `[chunking]` — `Chunking` (PRD §4.5)

| Field | Type | Default |
|---|---|---|
| `target_tokens` | `usize` | `512` |
| `overlap_tokens` | `usize` | `128` |
| `min_tokens` | `usize` | `32` |
| `heading_prefix` | `bool` | `true` |

PRD §4.5's "hard max 8192 tokens (model context limit)" has no config field — by design, it is not user-tunable.

### `[extraction]` — `Extraction` (PRD §4.6, §11.5)

| Field | Type | Default |
|---|---|---|
| `max_file_size` | `ByteSize` | `2 * (1 << 30)` (2 GiB) |
| `max_retries` | `u32` | `3` |

### `[embedding]` — `EmbeddingConfig` (PRD §4.7)

| Field | Type | Default |
|---|---|---|
| `batch_size` | `u32` | `64` |
| `threads` | `u32` | `0` — documented as a backward-compatible alias for `intra_op_threads` (PRD §17) |
| `intra_op_threads` | `u32` | `0` (= all cores) |
| `inter_op_threads` | `u32` | `1` |

The alias is representation-only here: `threads` and `intra_op_threads` are two independent fields; no resolution logic exists in core (see Divergences).

### `[auto_refresh]` — `AutoRefresh` (PRD §6)

| Field | Type | Default |
|---|---|---|
| `enabled` | `bool` | `true` |
| `threshold` | `DurationSetting` | 3 600 s (`"1h"`) |
| `warn_threshold` | `DurationSetting` | 604 800 s (`"7d"`) |
| `timeout_secs` | `u64` | `30` — a bare integer, *not* a `DurationSetting` (matches the PRD key name `timeout_secs`) |
| `index_new_only` | `bool` | `true` |

### `[ignore]` — `Ignore` (PRD §11.1)

| Field | Type | Default |
|---|---|---|
| `respect_gitignore` | `bool` | `true` |
| `respect_ndexignore` | `bool` | `true` |

### `[walk]` — `Walk` (PRD §11.1, §11.4)

| Field | Type | Default |
|---|---|---|
| `follow_symlinks` | `bool` | `true` |
| `hidden` | `bool` | `true` (= index dotfiles) |

### `[search]` — `Search` (PRD §10.7; `PartialEq` only — float fields)

| Field | Type | Default |
|---|---|---|
| `default_limit` | `u32` | `20` |
| `rrf_k` | `u32` | `60` |
| `title_boost` | `f32` | `2.0` |
| `fts_weight` | `f32` | `1.0` |
| `ef_search` | `u32` | `128` |

### `[archive]` — `Archive` (PRD §4.9)

| Field | Type | Default |
|---|---|---|
| `max_archive_total_size` | `ByteSize` | `8 * (1 << 30)` (8 GiB) |
| `max_archive_members` | `u32` | `100_000` |
| `max_archive_depth` | `u8` | `3` |
| `compression_ratio_limit` | `u32` | `200` |

## Constants ✅

`crates/ndex-core/src/constants.rs`. Values pinned by `constants_are_pinned` (which covers `MAGIC_PREAMBLE`, `MAX_FRAME_BYTES`, `MAX_PREAMBLE_SCAN_BYTES`, `NDEX_DIR`, `LOCK_FILE`, `QUERY_PREFIX`, and that `DEFAULT_MODEL` contains `"arctic"`).

| Constant | Type | Value | Consumer |
|---|---|---|---|
| `MAGIC_PREAMBLE` | `&[u8]` | owned by [framing](../50-protocol/51-framing.md) | written by `ndex-remote serve` before any framing; scanned by the client (PRD §12.2) |
| `MAX_FRAME_BYTES` | `usize` | owned by [framing](../50-protocol/51-framing.md) | max IPC frame payload |
| `MAX_PREAMBLE_SCAN_BYTES` | `usize` | owned by [framing](../50-protocol/51-framing.md) | max stdout garbage the preamble scan (`FrameReader::scan_preamble` in `ndex-protocol`) tolerates before giving up |
| `NDEX_DIR` | `&str` | `".ndex"` | index directory at the archive root ([layout](../20-store/21-layout-and-locking.md)) |
| `NDEX_OLD_DIR` | `&str` | `".ndex.old"` | staging directory during `reindex` (PRD §5.3) |
| `LOCK_FILE` | `&str` | `"lock"` | advisory write-lock file within `.ndex/` |
| `INDEX_TOML` | `&str` | `"index.toml"` | [identity file](11-data-model.md), never modified after `init` |
| `CONFIG_TOML` | `&str` | `"config.toml"` | user-editable settings (this doc) |
| `MANIFEST_DB` | `&str` | `"manifest.db"` | SQLite manifest ([manifest](../20-store/22-manifest.md)) |
| `META_DB` | `&str` | `"meta.db"` | SQLite metadata DB |
| `CONTENT_DIR` | `&str` | `"content"` | Tantivy FTS directory ([fts](../20-store/23-fts.md)) |
| `VECTORS_DIR` | `&str` | `"vectors"` | usearch vector directory ([vectors](../20-store/24-vectors.md)) |
| `QUERY_PREFIX` | `&str` | owned by [embedding](../30-ingest/34-embedding.md) | query-time embedding prefix for the asymmetric arctic model (PRD §4.7) |
| `DEFAULT_MODEL` | `&str` | owned by [embedding](../30-ingest/34-embedding.md) | default embedding model shortname (PRD §7.4) |
| `NDEXIGNORE_FILE` | `&str` | `".ndexignore"` | ignore-file name (PRD §11.1) |

No constant exists for the thumbnail store (`thumbs/`) — consistent with PRD §10.5, which defers thumbnails to v0.2.

## Divergences & open questions

- **`embedding.threads` alias is unimplemented.** PRD §17 documents `threads` as an alias for `intra_op_threads`, but core stores them as independent fields both defaulting to 0, with no precedence rule anywhere in the crate. A user setting only `threads` gets no effect unless a downstream consumer implements the aliasing.
- **Unknown config keys are silently ignored** (derived serde without `deny_unknown_fields`). A misspelled section or key (e.g. `max_filesize` for `max_file_size`) produces defaults with no warning. The PRD does not specify strictness; worth an explicit decision.
- **Style inconsistency:** `auto_refresh.timeout_secs` is a bare `u64` of seconds while its sibling keys are `DurationSetting`. This matches the PRD's key naming but forecloses `timeout_secs = "1m"`.
- **Fractional asymmetry:** `ByteSize` accepts `"1.5KiB"`, `DurationSetting` rejects `"1.5h"`. Undocumented in the PRD either way.
