# Errors & Exit Codes

**Owns:** `NdexError` (the crate-wide error enum), the `Result` alias, and the error → process-exit-code mapping.

**Sources:** `crates/ndex-core/src/error.rs`

## `Result` alias ✅

```rust
pub type Result<T, E = NdexError> = std::result::Result<T, E>;
```

Used throughout the workspace; the default error parameter allows `Result<T>` or `Result<T, OtherError>`.

## `NdexError` ✅

`#[derive(Debug, Error)]` (thiserror). Not `Clone`/`PartialEq` (contains `std::io::Error`). Not serde-serializable — wire error representation is owned by [protocol messages](../50-protocol/53-messages.md). The only `From` conversion is `#[from] std::io::Error` on `Io` (pinned by `io_errors_convert_via_from` in `crates/ndex-core/tests/characterization.rs`; the source message is preserved in `Display`).

| Variant | Payload | `Display` format | Exit code |
|---|---|---|---|
| `Io` | `std::io::Error` (`#[from]`) | `I/O error: {0}` | 1 |
| `Config` | `String` | `configuration error: {0}` | **78** |
| `IndexNotFound` | `String` | `index not found: {0}` | **3** |
| `SchemaMismatch` | `String` | `index schema mismatch: {0}` | **6** |
| `RemoteConnection` | `String` | `remote connection failed: {0}` | **4** |
| `VersionIncompatible` | `String` | `remote version incompatible: {0}` | **5** |
| `Protocol` | `String` | `protocol error: {0}` | 1 |
| `ExtractionTransient` | `String` | `transient extraction failure: {0}` | 1 |
| `ExtractionPermanent` | `String` | `permanent extraction failure: {0}` | 1 |
| `Unsupported` | `String` | `unsupported format: {0}` | 1 |
| `TooLarge` | `String` | `file too large: {0}` | 1 |
| `Encoding` | `String` | `text encoding error: {0}` | 1 |
| `Model` | `String` | `embedding model error: {0}` | 1 |
| `Index` | `String` | `index engine error: {0}` | 1 |
| `Lock` | `String` | `lock error: {0}` | 1 |
| `Nfs` | `String` | `.ndex/ is on an NFS filesystem: {0}` | **4** |
| `Interrupted` | *(none)* | `operation interrupted` | **130** |
| `NoResults` | *(none)* | `no results` | **7** |
| `Other` | `String` | `{0}` | 1 |

## `exit_code()` ✅

`pub fn exit_code(&self) -> i32` maps variants to the PRD §13.7 exit-code table:

| Code | Variants | PRD §13.7 meaning |
|---|---|---|
| 1 | everything not listed below | general error |
| 2 | *(no variant)* | usage error — emitted by `clap` in the [client CLI](../60-interfaces/61-client-cli.md), never by `NdexError` |
| 3 | `IndexNotFound` | index not found |
| 4 | `RemoteConnection`, `Nfs` | remote connection failed |
| 5 | `VersionIncompatible` | remote version incompatible |
| 6 | `SchemaMismatch` | index schema mismatch (needs rebuild) |
| 7 | `NoResults` | no results (with `--fail-no-results`) |
| 78 | `Config` | configuration error |
| 130 | `Interrupted` | interrupted (Ctrl-C) |

The full mapping — including the collapse of the extraction/engine family (`Protocol`, `ExtractionTransient`, `ExtractionPermanent`, `Unsupported`, `TooLarge`, `Encoding`, `Model`, `Index`, `Lock`, `Other`, `Io`) to 1 — is pinned exhaustively by `every_error_variant_maps_to_documented_exit_code` plus the module unit test `exit_codes_match_prd`.

## Relationship to per-file failure classification

`ExtractionTransient` / `ExtractionPermanent` / `Unsupported` / `TooLarge` correspond to the PRD §11.5 failure classes that drive [`FileStatus`](11-data-model.md) transitions (transient → status 2, permanent/unsupported → 4, too-large → 5); that mapping is applied by [reconcile](../30-ingest/31-reconcile.md), not by this type. These variants exit 1 only when a failure escapes to the process boundary.

## Divergences & open questions

- **`Nfs` → exit 4 is a code-side extension.** PRD §13.7's table has no NFS row; PRD §11.3 specifies an NFS abort with a detailed message but no exit code. The code reuses 4 ("remote connection failed"), which mislabels a local-filesystem condition; scripts branching on exit 4 cannot distinguish "SSH failed" from ".ndex/ on NFS".
- **`Lock` → exit 1.** PRD §6.2 requires that a *held* lock during auto-refresh be skipped silently, so `Lock` reaching the process boundary is a real failure — but PRD assigns it no dedicated code, and 1 loses the "another indexer is running" signal.
- **Doc comment claims "the process exit codes documented in PRD §13.7"** yet the enum encodes one mapping (Nfs) the PRD lacks; the exhaustive characterization test locks in the code's version, making the code the de-facto spec.
- **No structured error data.** Every variant except `Io` carries a bare `String`; downstream code cannot match on error causes (e.g. which file was too large) without string parsing. Acceptable for v0.1 CLI surfaces, but the wire protocol will need its own structured error payload (owned by [protocol messages](../50-protocol/53-messages.md)).
