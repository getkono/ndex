# End-to-end testing guide

This document gives the **exact command sequences** to exercise the full `ndex` suite — the
automated test gate and a manual walkthrough of the working product.

> **Status (v0.1 implementation).** The **local standalone pipeline** is implemented and works end
> to end: `ndex-remote init` → `index` → `search`/`stats`/`info` over a real directory, backed by
> SQLite (manifest + metadata) and a tantivy full-text index. See
> [What works vs. deferred](#what-works-vs-deferred) at the bottom.

---

## 1. Prerequisites

Rust is managed by `rustup` via `rust-toolchain.toml`; the other dev tools come from `mise`.

```bash
# one-time contributor bootstrap
mise trust && mise install            # taplo, typos, actionlint, pkl, hk, nextest, cargo-deny
rustup show                           # installs the pinned toolchain (1.96.0)
mise run hooks-install                # hk git hooks (optional)
```

Native build deps (Linux): `cmake`, `clang`, `libclang-dev`, `libssl-dev`, `pkg-config`
(usearch C++, rusqlite, tree-sitter, ort).

---

## 2. The automated gate (mirrors CI)

```bash
mise run ci          # fmt-check → lint (typos/actionlint/deny) → clippy -D warnings → test
```

…or each step directly:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace            # 217 pass, 0 fail, 14 ignored (deferred contracts)
cargo doc --workspace --no-deps
```

### Run the implemented end-to-end test explicitly

```bash
cargo test -p ndex-remote --test integration init_index_search_roundtrip
```

This drives the real binary (`assert_cmd`): `init` → `index` (asserts 2 new / 2 processed /
0 failed) → `search` (FTS hit) → `search --format paths` → re-`index` (idempotent: 2 unchanged) →
`stats`.

### Ignored contracts (the remaining work)

Everything still `todo!()` is pinned by an `#[ignore = "impl pending: PR #3"]` (or equivalent) test.
List them with:

```bash
cargo test --workspace -- --ignored --list
grep -rn 'impl pending' crates/*/tests crates/*/src
```

---

## 3. Manual product walkthrough

Build the server binary once, then drive it against a sample tree.

```bash
cargo build -p ndex-remote
BIN=target/debug/ndex-remote

# a sample archive to index
DEMO=$(mktemp -d)
mkdir -p "$DEMO/docs" "$DEMO/src"
printf '# Q3 report\n\nQuarterly earnings grew sharply across every segment.\n' > "$DEMO/docs/report.md"
printf 'blake3 is a fast cryptographic hash used for content addressing.\n'      > "$DEMO/docs/notes.txt"
printf 'fn main() { println!("hello reconciliation pipeline"); }\n'              > "$DEMO/src/main.rs"
echo 'build/' > "$DEMO/.ndexignore"      # ignore rules are honored
```

### init — create the index

```bash
$BIN init "$DEMO"
# → Initialized ndex index at <DEMO>/.ndex (model: default)
```

Creates `<DEMO>/.ndex/` with `index.toml`, `config.toml`, `manifest.db`, `meta.db`, and the
tantivy `content/` directory.

### index — walk, diff, extract, index

```bash
$BIN index "$DEMO"
# → 3 new, 0 modified, 0 deleted, 0 unchanged, 3 processed, 0 failed (NN ms)

$BIN index "$DEMO" --status            # summary without reconciling
$BIN index "$DEMO" --dry-run           # show changes without writing
```

Re-running is idempotent — unchanged files (by size+mtime) are skipped:

```bash
$BIN index "$DEMO"
# → 0 new, 0 modified, 0 deleted, 3 unchanged, 0 processed, 0 failed (NN ms)
```

### search — full-text query

```bash
$BIN search "$DEMO" earnings
#   1. [1.000] <DEMO>/docs/report.md
#        Quarterly **earnings** grew sharply across every segment

$BIN search "$DEMO" blake3 --format paths    # just paths, for xargs/piping
# → <DEMO>/docs/notes.txt

$BIN search "$DEMO" "reconciliation" -n 5    # limit results
$BIN search "$DEMO" zzznomatch               # → No results. (exit 0)
```

### stats / info

```bash
$BIN stats "$DEMO"
# index:  <DEMO>/.ndex
# model:  snowflake-arctic-embed-m-v2.0
# files:  3
# last reconcile: <ns> ns

$BIN info "$DEMO" "$DEMO/docs/notes.txt"     # per-file manifest record
```

### Verify incremental behavior

```bash
echo 'an extra paragraph about margins' >> "$DEMO/docs/report.md"
$BIN index "$DEMO"
# → 0 new, 1 modified, 0 deleted, 2 unchanged, 1 processed, 0 failed

rm "$DEMO/docs/notes.txt"
$BIN index "$DEMO"
# → 0 new, 0 modified, 1 deleted, 2 unchanged, 0 processed, 0 failed
$BIN search "$DEMO" blake3                    # → No results. (dropped from the index)

rm -rf "$DEMO"
```

---

## What works vs. deferred

**Implemented (works end to end):**
- `ndex-remote init / index / search / stats / info` (standalone, local)
- Three-phase reconciliation: parallel-capable walk (gitignore/ndexignore, `.ndex/` excluded) →
  manifest diff (new/modified/unchanged/deleted) → extract → chunk → FTS index, with per-file
  panic isolation so an unsupported file can't crash a run
- SQLite manifest + metadata, tantivy BM25 search with title boost and highlighted snippets
- Text-family extraction (plaintext/markdown/csv/json/sql/log), encoding detection (BOM/UTF-16/
  legacy), NFC normalization, language detection, token-window chunking

**Deferred (next increments — tracked by `#[ignore]`d contracts):**
- Semantic search: the ONNX embedder (`ndex-embed`) + the usearch vector index (`ndex-store`)
- Exotic extractors: PDF, DOCX, HTML, images (EXIF), archives, tree-sitter code structure
- The IPC serve loop (`ndex-remote serve`) and the **thin client** (`ndex`) SSH/subprocess
  transport — today, drive `ndex-remote` directly on the host holding the data
- Maintenance commands: `reindex` (atomic rebuild), `verify`, `delete`, `config`, `checkpoint`
