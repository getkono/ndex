# ndex Technical Specifications

**Owns:** the spec-set's structure, ownership rules, and traceability map — no product facts live here.

This directory is the **single source of truth for technical detail** in ndex. It is
derived from the code as it exists today, not from intent: where [`PRD.md`](../PRD.md)
(the upstream requirements document) and the code diverge, the spec documents the code
and flags the divergence.

## Rules

1. **One fact, one home.** Every technical fact — a type's shape, a constant's value, a
   schema, a wire byte, an algorithm — is owned by exactly one document: the one covering
   the module where it is *defined* in code. All other documents link to the owner and
   never restate the value.
   *Exception:* `crates/ndex-core/src/constants.rs` is a cross-domain definitions file;
   each constant's **value** is owned by the domain doc that specifies its semantics
   (e.g. wire constants by [51-framing](50-protocol/51-framing.md), the query prefix by
   [34-embedding](30-ingest/34-embedding.md)), and [13-config](10-core/13-config.md)
   keeps a link index of all constants. Likewise, dependency version pins are owned by
   [71-toolchain](70-operations/71-toolchain.md); this README (a structure doc) and the
   architecture overview own no values at all.
2. **Specs follow code.** Each doc lists its source files. A change to a source file is
   incomplete until its owning spec is updated. Divergences between code and PRD (or code
   and its own comments) are recorded in the owning doc's *Divergences & open questions*
   section.
3. **Status is explicit.** Every specified item carries one of:

   | Marker | Meaning |
   |---|---|
   | ✅ | Implemented and exercised |
   | 🚧 | Partially implemented |
   | ⛔ | Stub — signature exists, body is `todo!()`; spec describes documented intent |
   | 📋 | Planned — specified (usually in PRD) but no code exists |

## Document map

| Doc | Owns |
|---|---|
| [00-architecture.md](00-architecture.md) | Two-binary split, 9-crate workspace graph, dependency-boundary rule, end-to-end data flow, crate-level status map |
| **10-core/** | *Foundation crate `ndex-core`* |
| [11-data-model.md](10-core/11-data-model.md) | Domain model types, file status lifecycle, index identity & schema version, token counting |
| [12-paths.md](10-core/12-paths.md) | Raw-bytes path handling (`NdexPath`), JSON escaping |
| [13-config.md](10-core/13-config.md) | Configuration file format, defaults, workspace constants |
| [14-errors.md](10-core/14-errors.md) | Error taxonomy (`NdexError`), result alias |
| [15-search-and-progress-types.md](10-core/15-search-and-progress-types.md) | `SearchMode`, `SearchFilters`, progress-reporting types |
| **20-store/** | *Persistence crate `ndex-store`* |
| [21-layout-and-locking.md](20-store/21-layout-and-locking.md) | `.ndex/` on-disk layout, `Store` handle, locking, identity file |
| [22-manifest.md](20-store/22-manifest.md) | `manifest.db` and `meta.db` SQLite schemas and access patterns |
| [23-fts.md](20-store/23-fts.md) | Tantivy full-text schema and writer configuration |
| [24-vectors.md](20-store/24-vectors.md) | USearch vector index |
| **30-ingest/** | *Indexing pipeline* |
| [31-reconcile.md](30-ingest/31-reconcile.md) | Walk → diff → process reconciliation engine, crash recovery, stale-refresh |
| [32-extraction.md](30-ingest/32-extraction.md) | MIME detection, encoding, language detection, extractor registry & formats, archive safety |
| [33-chunking.md](30-ingest/33-chunking.md) | Chunking strategy |
| [34-embedding.md](30-ingest/34-embedding.md) | ONNX embedding pipeline (model, tokenizer, embedder) |
| **40-search/** | *Query path* |
| [41-search.md](40-search/41-search.md) | Mode resolution, query construction, execution, hybrid fusion |
| **50-protocol/** | *Wire protocol crate `ndex-protocol`* |
| [51-framing.md](50-protocol/51-framing.md) | Frame layout, magic bytes, size limits, MessagePack codec |
| [52-handshake.md](50-protocol/52-handshake.md) | Hello exchange, version negotiation |
| [53-messages.md](50-protocol/53-messages.md) | Every message type and payload, wire representations |
| **60-interfaces/** | *Binaries* |
| [61-client-cli.md](60-interfaces/61-client-cli.md) | `ndex` client CLI surface and rendering |
| [62-client-transport.md](60-interfaces/62-client-transport.md) | Host resolution, session, SSH/subprocess transport |
| [63-remote.md](60-interfaces/63-remote.md) | `ndex-remote` standalone CLI, serve loop, command dispatch |
| **70-operations/** | *Build, CI, release* |
| [71-toolchain.md](70-operations/71-toolchain.md) | Toolchain pinning, dev tooling, hooks, lints, workspace policy |
| [72-ci.md](70-operations/72-ci.md) | CI pipeline |
| [73-release.md](70-operations/73-release.md) | Release process, installer, versioning |
| [80-testing.md](80-testing.md) | Characterization methodology, test/fixture/bench inventory, coverage gaps |

## Traceability: source → owning doc

| Source | Owning doc |
|---|---|
| `crates/ndex-core/src/model.rs`, `status.rs`, `identity.rs`, `tokens.rs`, `lib.rs` (re-exports) | [11-data-model](10-core/11-data-model.md) |
| `crates/ndex-core/src/path.rs` | [12-paths](10-core/12-paths.md) |
| `crates/ndex-core/src/config.rs`, `constants.rs` (link index; constant values owned by domain docs) | [13-config](10-core/13-config.md) |
| `crates/ndex-core/src/error.rs` | [14-errors](10-core/14-errors.md) |
| `crates/ndex-core/src/filters.rs`, `progress.rs` | [15-search-and-progress-types](10-core/15-search-and-progress-types.md) |
| `crates/ndex-store/src/lib.rs`, `lock.rs`, `identity.rs` | [21-layout-and-locking](20-store/21-layout-and-locking.md) |
| `crates/ndex-store/src/manifest.rs`, `meta.rs` | [22-manifest](20-store/22-manifest.md) |
| `crates/ndex-store/src/fts.rs` | [23-fts](20-store/23-fts.md) |
| `crates/ndex-store/src/vector.rs` | [24-vectors](20-store/24-vectors.md) |
| `crates/ndex-reconcile/src/*` , `benches/` | [31-reconcile](30-ingest/31-reconcile.md) |
| `crates/ndex-extract/src/*` except `chunk.rs` | [32-extraction](30-ingest/32-extraction.md) |
| `crates/ndex-extract/src/chunk.rs` | [33-chunking](30-ingest/33-chunking.md) |
| `crates/ndex-embed/src/*` | [34-embedding](30-ingest/34-embedding.md) |
| `crates/ndex-search/src/*` | [41-search](40-search/41-search.md) |
| `crates/ndex-protocol/src/frame.rs`, `codec.rs` | [51-framing](50-protocol/51-framing.md) |
| `crates/ndex-protocol/src/handshake.rs` | [52-handshake](50-protocol/52-handshake.md) |
| `crates/ndex-protocol/src/message.rs`, `lib.rs` (re-exports) | [53-messages](50-protocol/53-messages.md) |
| `crates/ndex/src/args.rs`, `commands.rs`, `render/`, `lib.rs`, `main.rs` | [61-client-cli](60-interfaces/61-client-cli.md) |
| `crates/ndex/src/hosts.rs`, `session.rs`, `transport.rs` | [62-client-transport](60-interfaces/62-client-transport.md) |
| `crates/ndex-remote/src/*` | [63-remote](60-interfaces/63-remote.md) |
| `Cargo.toml`, `rust-toolchain.toml`, `mise.toml`, `hk.pkl`, `*.toml` lint configs, `.gitignore`, `CONTRIBUTING.md` | [71-toolchain](70-operations/71-toolchain.md) |
| `.github/workflows/ci.yml` | [72-ci](70-operations/72-ci.md) |
| `RELEASING.md`, `scripts/install.sh`, `SECURITY.md` | [73-release](70-operations/73-release.md) |
| `crates/*/tests/*`, `tests/fixtures/`, `docs/END_TO_END.md` | [80-testing](80-testing.md) |

Workspace-level facts (crate graph, binary split) are owned by
[00-architecture](00-architecture.md).
