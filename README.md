# ndex

**Offline, append-optimized deep file indexer for archival storage.**

`ndex` builds deep content indices over large archival pools (multi-hundred-TB ZFS
arrays, tape-backed stores) entirely offline — no cloud, no network databases — and
exposes full-text **and** semantic search over SSH:

```sh
ndex search nas:/pool/archive "quarterly earnings"
```

> **Status:** v0.1 skeleton. Interfaces, types, schemas, and tooling are finalized;
> product-logic bodies are `todo!()`. See [`PRD.md`](PRD.md) for the full design.

## Architecture

Two binaries communicate over length-prefixed MessagePack on stdin/stdout (SSH or a
local subprocess):

- **`ndex`** — thin client: CLI parsing, SSH/subprocess transport, terminal rendering.
  *No* extraction, embedding, or index logic.
- **`ndex-remote`** — fat server: reconciler, format extractors, ONNX embedder
  (`snowflake-arctic-embed-m-v2.0`), tantivy FTS, usearch vectors, SQLite metadata.

The Cargo workspace splits these into nine crates; the dependency graph *enforces*
the thin-client/fat-remote boundary — `ndex` cannot reference any engine crate.

```
ndex-core ← ndex-protocol
   ↑  ↑↖ store ↖ extract ↖ embed ↖ search ↖ reconcile
   │                                            ↑
   ndex(client) ──────── ndex-remote(server) ───┘
```

## Quick start (development)

```sh
mise trust && mise install        # dev tools (NOT rust — rustup owns that)
rustup show                       # installs the pinned toolchain
mise run hooks-install            # git hooks via hk
mise run ci                       # fmt-check + lint + clippy + test
```

## Commands (v0.1)

| Command | Purpose |
|---|---|
| `init` | Initialize a new index |
| `index` | Build / update the index |
| `search` | FTS / semantic / hybrid search |
| `info` | Metadata for a file |
| `stats` | Index statistics |
| `verify` | BLAKE3 integrity check |
| `delete` | Remove files from the index |
| `reindex` | Rebuild from scratch |
| `config` | View configuration |
| `completions` | Shell completions |

`tag`, `dedup`, and `compact` are compiled stubs (planned for v0.2).

## License

[MIT](LICENSE) © 2026 Justin Chung
