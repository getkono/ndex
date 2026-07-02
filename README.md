# ndex

**Offline, append-optimized deep file indexer for archival storage.**

`ndex` builds deep content indices over large archival pools (multi-hundred-TB ZFS
arrays, tape-backed stores) entirely offline — no cloud, no network databases — and
exposes full-text **and** semantic search over SSH:

```sh
ndex search nas:/pool/archive "quarterly earnings"
```

> **Status:** v0.1 in progress. The **local pipeline works end to end** — `ndex-remote`
> can `init` → `index` → `search` a directory (SQLite manifest + tantivy full-text search).
> Semantic embeddings, exotic format extractors, and the SSH thin-client transport are the
> next increments. See [`docs/END_TO_END.md`](docs/END_TO_END.md) and [`PRD.md`](PRD.md).

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

## Try it (works today)

```sh
cargo build -p ndex-remote
BIN=target/debug/ndex-remote

$BIN init   /path/to/archive                  # create /path/to/archive/.ndex
$BIN index  /path/to/archive                  # walk → diff → extract → FTS index
$BIN search /path/to/archive "quarterly earnings"
$BIN search /path/to/archive blake3 --format paths
$BIN stats  /path/to/archive
```

The SSH thin-client form (`ndex search nas:/pool "…"`) is wired in a later increment; for now
run `ndex-remote` on the host that holds the data. Full walkthrough:
[`docs/END_TO_END.md`](docs/END_TO_END.md).

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

`init`, `index`, `search`, `info`, and `stats` are implemented (standalone `ndex-remote`).
`verify`, `delete`, `config`, and `reindex` are in progress; `tag`, `dedup`, and `compact`
are compiled stubs (planned for v0.2). See [`docs/END_TO_END.md`](docs/END_TO_END.md) for status.

## License

[MIT](LICENSE) © 2026 Justin Chung
