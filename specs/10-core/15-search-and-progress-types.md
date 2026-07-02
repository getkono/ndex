# Search Request & Progress Types

**Owns:** `SearchMode` and `SearchFilters` (the search request vocabulary), and the core-native progress-reporting types (`ProgressKind`, `ProgressUpdate`, `ProgressChildUpdate`, `ProgressSink`, `NullSink`).

**Sources:** `crates/ndex-core/src/filters.rs`, `crates/ndex-core/src/progress.rs`

Both modules exist in `ndex-core` for dependency-graph reasons: `ndex-search` and `ndex-reconcile` must not depend on `ndex-protocol`, so the shared request/progress vocabulary lives at the bottom of the graph and the [protocol](../50-protocol/53-messages.md) re-exports or maps it.

## `SearchMode` ✅

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SearchMode {
    #[default]
    Auto,
    Fts,
    Semantic,
    Hybrid,
}
```

Search execution mode (PRD §10.7). Derived serde on a unit-variant enum: serializes as the PascalCase variant-name string — `"Auto"`, `"Fts"`, `"Semantic"`, `"Hybrid"`. Default is `Auto`. Pinned by `search_mode_default_is_auto_and_serializes_by_name` in `crates/ndex-core/tests/characterization.rs`. The `auto`-mode resolution heuristics (quoted phrases → fts, >3 tokens → hybrid, etc.) are owned by [search](../40-search/41-search.md); the CLI's lowercase `--mode` values map to these variants in the [client CLI](../60-interfaces/61-client-cli.md).

## `SearchFilters` ✅

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchFilters { ... }
```

Filters applied to a search (PRD §12.7); embedded in the wire `SearchRequestData` (owned by [protocol messages](../50-protocol/53-messages.md)) and consumed by [search](../40-search/41-search.md). Field names and types match PRD §12.7's `SearchFilters` exactly:

| Field | Type | Semantics |
|---|---|---|
| `mime` | `Option<String>` | MIME glob, e.g. `image/*` |
| `after_ns` | `Option<i64>` | modified-after, unix nanoseconds |
| `before_ns` | `Option<i64>` | modified-before, unix nanoseconds |
| `larger` | `Option<u64>` | minimum size in bytes |
| `smaller` | `Option<u64>` | maximum size in bytes |
| `path_glob` | `Option<String>` | path glob, e.g. `invoices/**/*.pdf` |
| `tags` | `Vec<String>` | tag filter, OR semantics |
| `lang` | `Option<String>` | language filter, ISO 639-1 |

The container-level `#[serde(default)]` means a payload may omit **any** field and it takes its default (`tags` → empty vec; the `Option` fields → `None`, which serde's built-in Option handling would also give). This satisfies the additive-evolution rule (PRD §12.3): fields added to `SearchFilters` later must be defaulted, and payloads from older peers that lack them still decode — pinned at the MessagePack level by `search_filters_msgpack_decodes_with_missing_fields`. `Default` is all-`None`/empty (pinned along with round-trip by `search_filters_default_is_empty_and_roundtrips`). Filter matching semantics (glob dialects, inclusive/exclusive bounds) are not defined by this type — they are owned by [search](../40-search/41-search.md).

## Progress reporting ✅

The reconciler emits `ProgressUpdate`s through a `ProgressSink`; `ndex-remote` provides a sink mapping them to the wire `ProgressEvent` (mapping owned by [remote](../60-interfaces/63-remote.md), wire shape by [protocol messages](../50-protocol/53-messages.md)), so `ndex-reconcile` never depends on `ndex-protocol`.

### `ProgressKind`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressKind { Walk, Diff, Extract, Embed, Fts, Meta }
```

The reconciliation phase an update belongs to. The six variants correspond 1:1 to PRD §13.7's `ProgressEvent.phase` strings; the lowercase wire vocabulary is owned by `ndex-remote`'s `phase_name` mapping ([remote](../60-interfaces/63-remote.md)), while core's own derived serde emits PascalCase variant names (`"Walk"`, …).

### `ProgressUpdate` / `ProgressChildUpdate`

Both derive `Debug, Clone, PartialEq, Serialize, Deserialize`, no serde attributes (round-trip pinned by `progress_update_roundtrips`).

| `ProgressUpdate` field | Type |
|---|---|
| `kind` | `ProgressKind` |
| `current` | `u64` |
| `total` | `Option<u64>` (`None` = indeterminate) |
| `message` | `Option<String>` |
| `children` | `Vec<ProgressChildUpdate>` |

| `ProgressChildUpdate` field | Type |
|---|---|
| `label` | `String` (e.g. an extraction worker or the embed sub-pipeline) |
| `current` | `u64` |
| `total` | `Option<u64>` |
| `message` | `Option<String>` |

These mirror PRD §13.7's `ProgressEvent`/`ProgressChild` except that `kind` is the typed enum where the wire uses a `phase: String`.

### `ProgressSink` / `NullSink`

```rust
pub trait ProgressSink: Send + Sync {
    fn emit(&self, update: &ProgressUpdate);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;   // emit() discards the update
```

`Send + Sync` supertraits allow sinks to be shared across the reconciler's worker threads. `emit` takes `&self` and borrows the update — implementors clone what they keep. `NullSink` discards everything (for local/silent runs). Object-safety and dispatch are pinned by `null_sink_swallows_updates` and `custom_sink_receives_updates`.

## Divergences & open questions

- **`ProgressKind` has two serialized spellings.** Core's derived serde emits `"Walk"`-style PascalCase, but PRD §13.7 and the wire `ProgressEvent` use lowercase phase strings via a separate hand-maintained mapping in `ndex-remote`. If a `ProgressUpdate` were ever serialized directly (e.g. into a log sink), its phase names would not match wire/PRD vocabulary. Nothing enforces that the `phase_name` mapping stays exhaustive against new variants beyond the `match` itself.
- **`SearchFilters` derives `PartialEq` but not `Eq`** although every field is `Eq`-capable — inconsistent with sibling types and presumably an oversight (harmless).
- **Filter semantics are entirely unspecified at the type level**: glob dialect for `mime`/`path_glob`, whether `after_ns`/`larger` bounds are inclusive, and interaction between `larger`/`smaller` are all deferred to the search implementation, which is currently the only place those decisions can be pinned.
- **`ProgressSink::emit` provides no throttling or sequencing contract** (no update IDs, no monotonicity requirement on `current`). The wire protocol's flow-control expectations, if any, must be imposed by the remote's sink implementation.
