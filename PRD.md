# PRD: `ndex` — Deep File Indexer for Archival Storage

**Version:** 0.3.0-draft
**Date:** 2026-03-17
**Status:** Draft

---

## 1. Problem Statement

Large archival storage pools (multi-hundred-TB ZFS arrays, tape-backed stores) accumulate files over years with no efficient way to search their contents. Current options are either online SaaS products (unacceptable for air-gapped / private archives), brute-force `find` + `grep` (unusably slow at scale), or desktop search tools (not designed for headless servers or remote access).

We need a system that:

- Builds deep content indices **entirely offline** — no cloud services, no network databases
- Supports both **full-text search** and **semantic search**
- Handles **append-only workloads** efficiently — new files are the hot path
- Runs a **fast reconciliation pass** in Rust — seconds, not hours
- Exposes a **CLI over SSH** from remote machines: `ndex search nas:/pool/archive "quarterly earnings"`

---

## 2. Design Principles

1. **Offline-first, offline-only.** Zero network dependencies at runtime. Embedding models are local ONNX weights, auto-fetched once by the server.
2. **Append-optimized reconciliation.** Metadata-first fast path (mtime + size + inode), content hashing only during extraction. New files are the hot path.
3. **Index-per-concern separation.** Each index type is independent, individually rebuildable. A corrupt vector index doesn't take down full-text search.
4. **Fat remote, thin client.** `ndex-remote` carries all logic (extraction, embedding, indexing, search). `ndex` is a thin SSH + display shell. The server is the source of truth for everything.
5. **Zero idle footprint.** No daemon, no background processes, no resident memory. `ndex-remote` is spawned on demand per SSH session, does its work, exits. The server pays zero resource cost between operations.
6. **No index migrations.** Index schema changes require a full rebuild. Correctness over convenience. The index is always rebuildable from source files.

---

## 3. Why No Daemon

A persistent `ndexd` daemon was considered and rejected:

**The supposed benefit:** keeping tantivy segments, USearch HNSW graph, and SQLite pages warm in the kernel page cache across searches, avoiding cold-start latency.

**Why it doesn't matter:**

1. **mmap handles this already.** Tantivy segments, USearch index, and SQLite databases are all mmap'd. After the first search, the OS page cache retains hot pages automatically. A subsequent `ndex-remote` process spawned via SSH will mmap the same files and hit the warm page cache. The kernel doesn't care which process populated the cache — it's per-inode, not per-process.

2. **Cold start cost is small.** On first access, the cost is page faults as the OS loads pages from disk. For a typical search against a 50GB index: tantivy reads a few segment files (maybe 100 MB of hot pages for the inverted index), USearch reads the HNSW graph entry points (a few MB), SQLite reads the manifest pages it needs. Total cold-start penalty: 1-3 seconds on HDD, sub-second on SSD. This is acceptable for an archival search tool.

3. **A daemon consumes resources when idle.** Archival servers often run other workloads (media serving, backups, VMs). A daemon holding 500 MB+ of resident memory for an index that gets queried a few times a day is wasteful. The whole point of append-only archival storage is that it's mostly at rest.

4. **Complexity cost.** A daemon needs: process management (systemd unit), socket management, connection multiplexing, graceful shutdown, log rotation, health monitoring, and a separate IPC path. All of this for saving 1-3 seconds of cold start. Not worth it.

5. **Concurrent searches already work.** Multiple SSH sessions spawn multiple `ndex-remote` processes. Each independently mmap's the index files (read-only for search). The OS deduplicates the physical pages. There's no coordination problem that a daemon would solve.

**If cold start ever becomes a problem** (extremely large indices on slow storage), the correct fix is `vmtouch` or `mlock` on the hot index files via a cron job, not a custom daemon.

---

## 4. Architecture Overview

```
┌──────────────────────────────────────────────────────────────────┐
│                     Client Machine                               │
│                                                                  │
│  ndex (thin binary, ~2-3 MB)                                    │
│  ┌────────────────────────────────────┐                          │
│  │  - CLI parsing (clap)              │                          │
│  │  - SSH transport (shells out)      │                          │
│  │  - Result display + formatting     │                          │
│  │  - Terminal capability detection   │                          │
│  │  - Progress bar rendering          │                          │
│  │  - NO extraction, NO embedding     │                          │
│  │  - NO index logic                  │                          │
│  └──────────┬─────────────────────────┘                          │
│             │ ssh user@host "ndex-remote serve --root /path"     │
│             │ stdin/stdout: length-prefixed msgpack frames        │
└─────────────┼────────────────────────────────────────────────────┘
              │
┌─────────────┴────────────────────────────────────────────────────┐
│                  Server (archive host)                            │
│                                                                  │
│  ndex-remote (fat binary, ~30 MB + ~300 MB model on disk)       │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  Spawned on demand. No daemon. Exits when SSH closes.       │ │
│  │                                                              │ │
│  │  ┌──────────────────────────────────────────────────────┐   │ │
│  │  │  Reconciler     (parallel walk, diff, hash)          │   │ │
│  │  │  Extractors     (pdf, docx, code, media, ...)        │   │ │
│  │  │  Embedder       (ONNX Runtime, batched inference)    │   │ │
│  │  │  FTS engine     (tantivy)                            │   │ │
│  │  │  Vector engine  (usearch)                            │   │ │
│  │  │  Meta engine    (SQLite)                             │   │ │
│  │  │  Thumb gen      (image, video, pdf)                  │   │ │
│  │  └──────────────────────────────────────────────────────┘   │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                  │
│  /pool/archive/.ndex/                                            │
│  ├── lock              (flock, prevents concurrent writers)      │
│  ├── index.toml        (index identity: schema version, model)   │
│  ├── config.toml       (user-editable settings)                  │
│  ├── manifest.db       (SQLite WAL)                              │
│  ├── content/          (tantivy segments)                        │
│  ├── vectors/          (usearch HNSW + sidecar)                  │
│  ├── meta.db           (SQLite: doc/media metadata, tags)        │
│  └── thumbs/           (WebP thumbnails, sharded)                │
└──────────────────────────────────────────────────────────────────┘
```

**Local mode:** When no `HOST:` prefix is given, `ndex` spawns `ndex-remote` as a local subprocess (not over SSH). Communication uses the same msgpack protocol over stdin/stdout pipes. This means `ndex-remote` must be installed locally for local operation. The thin client never embeds extraction/indexing logic.

`ndex-remote` also works standalone for local-only use. All commands that the thin client proxies are available directly: `ndex-remote search --root /pool "query"`, `ndex-remote index --root /pool`, etc. The thin client adds SSH transport, host aliases, and nicer terminal rendering, but is not required for local operation.

---

## 5. Hashing: BLAKE3 Everywhere

### 5.1 Decision

**Use BLAKE3 as the single hash for all purposes.** No xxHash3, no split strategy.

### 5.2 Rationale

The v0.2 draft proposed xxHash3-128 for speed on the reconciliation fast path and BLAKE3 for integrity. On reflection, the split adds complexity for negligible benefit:

**The reconciliation fast path doesn't hash at all.** Phase 1 (walk) and Phase 2 (diff) use only filesystem metadata: `(size, mtime_ns, inode)`. Content hashing happens in Phase 3 (process), where we're already reading the entire file for text extraction. At that point, the hash computation is free — BLAKE3 at 4-6 GB/s is never the bottleneck when the actual bottleneck is PDF parsing at ~50 MB/s or embedding at ~4000 chunks/sec.

**BLAKE3 is fast enough.** On modern CPUs with AVX2/AVX-512, BLAKE3 sustains 4-6 GB/s per core. That's within 2x of raw sequential disk read speed on most NVMe drives and well above HDD throughput. We're never CPU-bound on hashing.

**One hash means one codepath.** No confusion about which hash is stored where, no "optional companion file," no "compute on demand" logic. Every file gets one 32-byte BLAKE3 digest, stored in the `blake3` column of the `files` table in `manifest.db`.

**Collision resistance.** BLAKE3 has a 256-bit output. Birthday bound: collision probability reaches 1-in-10^38 at 10^19 files. At 10 billion files (10^10), the probability is ~10^-58. This is not a concern at any conceivable scale.

**Belt-and-suspenders for dedup.** When reporting duplicates, we match on `(blake3, size)`. Two files are only considered duplicates if both the hash AND size match. This means even a hypothetical BLAKE3 collision (which will never happen in practice) would require the colliding files to also be the same size — vanishingly unlikely. For the truly paranoid, `ndex dedup --byte-verify` does a byte-for-byte comparison of candidate pairs.

### 5.3 When BLAKE3 Is Computed

BLAKE3 is computed **exactly once per file, during the extraction phase (Phase 3).** The extraction workers are already reading the file from disk to extract text. They hash the raw bytes as a streaming side-effect of reading:

```rust
fn process_file(path: &Path) -> Result<ProcessedFile> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(1 << 20, file);  // 1 MB buffer
    let mut hasher = blake3::Hasher::new();
    let mut extractor = get_extractor(mime_type);

    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() { break; }
        hasher.update(buf);                   // hash the raw bytes
        extractor.feed(buf)?;                 // feed to format-specific extractor
        let len = buf.len();
        reader.consume(len);
    }

    let hash = hasher.finalize();
    let text = extractor.finish()?;
    Ok(ProcessedFile { hash, text, ... })
}
```

No separate "hashing pass." No lazy computation. No optional anything. Every indexed file has a BLAKE3 hash, period.

**For files that fail extraction** (status = 2 or 4): We still compute the hash during the read attempt. Even if text extraction fails, the hash is stored. This enables dedup detection even for files we can't read.

BLAKE3 hashes are stored in the `blake3` column of the `files` table in `manifest.db`. For `ndex verify`, hashes are read from the manifest and compared against freshly computed hashes.

### 5.5 Chunking Strategy

**Strategy: Recursive structure-aware splitting**

| Parameter | Default | Config key |
|---|---|---|
| Target chunk size | 512 tokens | `chunking.target_tokens` |
| Overlap | 128 tokens (25%) | `chunking.overlap_tokens` |
| Min chunk size | 32 tokens | `chunking.min_tokens` |
| Hard max | 8192 tokens | (model context limit) |

**Boundary priority** (split at largest semantic unit first):
1. Section/heading boundaries (markdown `##`, HTML `<h*>`, docx headings, PDF sections)
2. Paragraph boundaries (`\n\n`)
3. Sentence boundaries (`. `, `! `, `? ` + Unicode sentence break rules)
4. Word boundaries (fallback)

**Per-format extraction → normalized intermediate representation:**

| Format | Extractor | Structure signals |
|---|---|---|
| Markdown | `pulldown-cmark` | Headings, code blocks, lists |
| HTML | `scraper` / `lol-html` | `<h*>`, `<p>`, `<pre>`, `<li>` |
| PDF | `pdf-extract` / `pdfium` | Section breaks, page breaks |
| DOCX | `docx-rs` | Paragraph styles, headings |
| Code | `tree-sitter` | Functions, classes, modules |
| Plaintext | Recursive splitter | `\n\n` > `\n` > `. ` > ` ` |
| Logs | Line-based | Timestamp patterns, fixed line batches |
| CSV/JSON | Record-based | Row/object boundaries, header propagation |
| SQL | Statement-based | `;` delimiters |

**Normalized block types:** `Heading(level)`, `Paragraph`, `CodeBlock(lang)`, `ListItem`, `Table`, `Quote`, `Raw`

**Chunking algorithm:**
1. Extractor produces ordered list of typed blocks with byte offsets
2. Chunker merges consecutive small blocks up to target size (512 tokens)
3. Splits large blocks at sentence boundaries, adding overlap
4. Each chunk carries: `(file_id, chunk_ord, byte_start, byte_end, block_type)`
5. Heading context propagated: chunks inherit the most recent heading text as prefix (configurable)

**Config:**

```toml
[chunking]
target_tokens = 512
overlap_tokens = 128
min_tokens = 32
heading_prefix = true   # prepend section heading to each chunk
```

### 5.6 Large File Strategy

**Text-extractable files (text/\*, pdf, docx, code, md, html, etc.):**
- Stream through extractor regardless of size (BLAKE3 + extraction in single pass)
- Configurable `max_file_size` (default: 2 GiB) — files above this get metadata-only indexing with `status=5` (skipped), logged as warning
- Rationale: PDF extractors and some format parsers may buffer entire file; the cap prevents OOM

**Media files (image/\*, video/\*, audio/\*):**
- Metadata-only in v0.1: EXIF, codec info, duration, resolution
- Thumbnail generation for images
- No transcription/OCR in v0.1 (deferred to v0.3)

**Archives (zip, tar, gz, 7z, rar):**
- Metadata-only in v0.1 (file count, total size, listing)
- Content indexing (extract-and-index member files) deferred to v0.2

**Config:**

```toml
[extraction]
max_file_size = "2GiB"
```

---

## 6. No Index Migrations

### 6.1 Policy

**When the index schema version changes, ndex refuses to open the old index and requires a full rebuild.** No migration code, no upgrade scripts, no in-place mutation of index files.

### 6.2 Justification

1. **Correctness is paramount.** Migration code is the #1 source of subtle data corruption bugs in database systems. A migration that silently drops a field, mishandles a type conversion, or partially completes leaves the index in an unknown state. Since the index is always rebuildable from source files (the archive itself), a full rebuild is the safe path.

2. **The index is derived data.** Unlike a primary database, ndex indices are projections of filesystem content. The source of truth is the files on disk. Destroying and rebuilding the index loses nothing — it just costs time.

3. **Rebuild cost is bounded.** On an archival server, a full re-index of 10M files takes hours, not days. This is a one-time cost per schema change, and schema changes should be rare (a few times per year at most).

4. **Simplicity.** Zero migration code means zero migration bugs. The `index.toml` identity file is the gatekeeper — if the schema version doesn't match, the index is rejected cleanly.

### 6.3 Implementation

**`index.toml`** — written at `ndex init`, never modified:

```toml
# This file identifies the index. Do not edit.
# Changing any value here will cause ndex to refuse to open the index.

[identity]
schema_version = 3                  # bumped on any breaking index format change
created_by = "ndex-remote 0.1.0"    # version that created this index
created_at = "2026-03-17T08:00:00Z"

[embedding]
model_name = "snowflake-arctic-embed-m-v2.0"
model_hash = "a3f2e8..."            # BLAKE3 of the ONNX model file
dimensions = 768
mrl_dimensions = 256                # MRL truncation (stored/search dimension)
quantization = "f16"

[hashing]
algorithm = "blake3"

[fts]
tokenizer_version = 1               # bumped if tokenizer pipeline changes
```

**Version check on open:**

```rust
fn open_index(root: &Path, my_version: &Version) -> Result<Index> {
    let identity = read_index_toml(root.join(".ndex/index.toml"))?;

    if identity.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(Error::SchemaMismatch {
            index_schema: identity.schema_version,
            binary_schema: CURRENT_SCHEMA_VERSION,
            message: format!(
                "Index was created with schema v{}, but this ndex-remote uses schema v{}.\n\
                 \n\
                 The index must be rebuilt:\n\
                 \n\
                 ndex-remote reindex --root {}\n\
                 \n\
                 This will delete the existing index and rebuild from scratch.\n\
                 Your files are not affected — only the .ndex/ directory is modified.",
                identity.schema_version,
                CURRENT_SCHEMA_VERSION,
                root.display()
            ),
        });
    }

    // Also check embedding model matches for vector searches
    if identity.model_name != current_model_name() {
        // Vectors are unusable but FTS still works
        warn!("Embedding model mismatch: index uses {}, binary has {}. \
               Semantic search disabled. Run `ndex-remote reindex --vectors` to re-embed.",
              identity.model_name, current_model_name());
    }

    Ok(Index::open(root)?)
}
```

**`ndex-remote reindex`:** Moves `.ndex/` to `.ndex.old/`, creates a fresh index, rebuilds. If rebuild succeeds, deletes `.ndex.old/`. If it fails, restores `.ndex.old/`. Atomic from the user's perspective.

---

## 7. Stale Index Auto-Refresh on Search

### 7.1 Problem

On append-only archival storage, files accumulate between explicit `ndex index` runs. If a user searches without indexing first, they may miss recently added files. We want search to be automatically up-to-date without requiring a manual `ndex index` step every time.

### 7.2 Design: Opportunistic Reconciliation

When `ndex search` (or any read command) opens the index, the remote checks how stale the index is and optionally runs a fast reconciliation before returning results.

**Staleness heuristic:**

```rust
struct StalenessCheck {
    last_reconciliation_ns: i64,   // from manifest.reconciliation_runs
    age: Duration,                 // now - last_reconciliation
}

enum RefreshDecision {
    Skip,                          // index is fresh, search immediately
    QuickReconcile,                // run metadata walk + diff, index only new files
    Warn,                          // index is very stale, warn user
}

fn decide_refresh(staleness: &StalenessCheck, config: &Config) -> RefreshDecision {
    let threshold = config.auto_refresh_threshold;  // default: 1 hour
    let warn_threshold = config.stale_warn_threshold;  // default: 7 days

    if staleness.age < threshold {
        RefreshDecision::Skip
    } else if staleness.age < warn_threshold {
        RefreshDecision::QuickReconcile
    } else {
        RefreshDecision::Warn
    }
}
```

**What "quick reconcile" does:**

It runs Phase 1 (walk) and Phase 2 (diff) of the reconciler — the metadata-only fast path. If it finds new or modified files, it runs Phase 3 (extract + index) for those files before executing the search.

Key constraints:

- **Time-boxed.** The quick reconcile has a wall-clock budget (default: 30 seconds). If the walk/diff/process phases exceed this, it stops processing, runs the search against whatever has been indexed so far, and appends a warning to the results: "Index is being updated. 2,341 new files found; 847 indexed before search timeout. Run `ndex index` for a complete update."
- **Write-locked.** Uses `flock()` with `LOCK_NB` (non-blocking). If another process holds the write lock (an explicit `ndex index` is running), the quick reconcile is skipped silently and the search proceeds against the current state.
- **Phase 3 prioritizes by relevance (when possible).** If the search query is available before processing starts (it always is — the client sends the query first), the reconciler can prioritize processing files whose paths match the query terms. E.g., searching for "invoice" prioritizes new files in `invoices/` directories. This is a best-effort optimization, not guaranteed.

**Configuration:**

```toml
[auto_refresh]
enabled = true                     # master switch
threshold = "1h"                   # don't refresh if indexed within this window
warn_threshold = "7d"              # warn if older than this
timeout_secs = 30                  # max wall time for pre-search reconciliation
index_new_only = true              # only index new files (skip modified, faster)
```

> **Note (HDD users):** The default `timeout_secs = 30` may be insufficient on HDD arrays for large archives (10M+ files). A Phase 1 walk on HDD RAIDZ2 at 10M files takes 3-5 minutes. If you have a large HDD archive and want search to be always current, run `ndex index` explicitly on a schedule (e.g., nightly cron) rather than relying on auto-refresh.

**User override flags:**

```bash
ndex search /pool "query" --no-refresh    # skip auto-refresh, search stale index
ndex search /pool "query" --refresh       # force refresh even if fresh
ndex search /pool "query" --refresh-timeout 60  # extend the budget
```

### 7.3 Manifest Schema Addition

```sql
-- Added to manifest.db: lightweight reconciliation tracking
-- Used by the staleness heuristic
CREATE TABLE reconciliation_runs (
    run_id       INTEGER PRIMARY KEY,
    started_ns   INTEGER NOT NULL,
    completed_ns INTEGER,
    kind         TEXT NOT NULL,        -- 'full', 'quick', 'incremental'
    method       TEXT NOT NULL,        -- 'full_walk', 'partial_walk'
    total_files  INTEGER,
    new_files    INTEGER,
    modified     INTEGER,
    deleted      INTEGER,
    unchanged    INTEGER,
    processed    INTEGER,              -- how many new/modified were actually indexed
    duration_ms  INTEGER,
    timed_out    INTEGER DEFAULT 0,    -- 1 if hit the wall-clock budget
    error        TEXT
);
```

---

## 8. Remote Binary: Server Self-Installation

### 8.1 Principle

The client **never** sends the `ndex-remote` binary to the server. The client may be on a bandwidth-constrained link (remote location, metered connection, satellite). Transferring a 30 MB binary over that link is unacceptable.

The server installs `ndex-remote` itself, using its own (presumably fast) network connection to download from the release source.

### 8.2 Installation Methods

**Method 1: Direct install script (recommended)**

The server admin runs this once:

```bash
# On the server:
curl -fsSL https://get.ndex.dev/install.sh | sh

# Or with explicit version/architecture:
curl -fsSL https://get.ndex.dev/install.sh | sh -s -- --version 0.1.0 --arch x86_64

# Installs to ~/.local/bin/ndex-remote by default
# Or /usr/local/bin/ndex-remote with --system
```

The install script:
1. Detects `uname -m` for architecture (x86_64, aarch64)
2. Detects OS (Linux, macOS, FreeBSD)
3. Downloads the release tarball from GitHub releases
4. Extracts `ndex-remote` to the install path
5. Verifies the binary with a bundled signature or checksum
6. Does NOT download the embedding model (that happens automatically on first use — §8.4)

**Method 2: Package manager**

```bash
# Homebrew (macOS/Linux)
brew install ndex

# Cargo (from source)
cargo install ndex-remote

# Nix
nix profile install nixpkgs#ndex-remote

# Arch Linux (AUR)
yay -S ndex-remote-bin
```

**Method 3: Manual tarball**

```bash
wget https://github.com/.../ndex-remote-v0.1.0-linux-x86_64.tar.gz
tar xzf ndex-remote-v0.1.0-linux-x86_64.tar.gz
cp ndex-remote ~/.local/bin/
```

### 8.3 Self-Update

`ndex-remote` can update itself when the server admin requests it:

```bash
# On the server:
ndex-remote self-update                    # update to latest
ndex-remote self-update --version 0.1.0    # specific version
ndex-remote self-update --check            # just check, don't install
```

Self-update process:
1. Fetch version manifest from release server
2. Download new binary to a temporary file
3. Verify checksum
4. Atomic rename over the old binary (`rename()` on the same filesystem)
5. Print old → new version
6. The next SSH session will use the new binary

The **client** can detect an outdated remote and suggest an upgrade:

```
Warning: ndex-remote on 'nas' is version 0.0.9, but your client is 0.1.0.
Some features may be unavailable. To upgrade on the server, run:

    ssh nas "ndex-remote self-update"
```

The client never initiates the update itself.

### 8.4 Automatic Model Fetching

Embedding models are **auto-fetched on first use.** When `ndex-remote` needs a model and doesn't have it:

```
$ ndex-remote index /pool/archive

  Embedding model 'snowflake-arctic-embed-m-v2.0' not found.
  Downloading to ~/.ndex/models/snowflake-arctic-embed-m-v2.0/...
  ████████████████████████████░░░░ 100 MB / 130 MB  [3s ETA]

  Model verified (blake3: a3f2e8...). Ready.

  Reconciling...
  ...
```

This happens:
- On the first `ndex-remote index` after installation
- On the first `ndex-remote serve` that receives a search request requiring vectors
- Automatically, silently, exactly once per model

**Model management commands:**

```bash
ndex-remote model list                       # show available + downloaded models
ndex-remote model fetch arctic               # pre-download default model
ndex-remote model fetch granite-small        # download english-only model
ndex-remote model fetch --all                # download all available models
ndex-remote model delete granite-small       # remove a downloaded model
ndex-remote model delete --all               # remove all models
ndex-remote model verify                     # re-verify downloaded model integrity
ndex-remote model path arctic                # print the path to the model file
```

**Model storage:**

```
~/.ndex/models/
├── snowflake-arctic-embed-m-v2.0/
│   ├── model.onnx          (~130 MB, INT8)
│   ├── tokenizer.json      (600 KB)
│   └── manifest.json       (model metadata, expected hashes)
└── granite-embedding-small-english-r2/
    ├── model.onnx          (48 MB, INT8)
    ├── tokenizer.json
    └── manifest.json
```

**Available models:**

| Shortname | Full name | Size (ONNX INT8) | Dims | MRL | Languages | BEIR/MIRACL |
|---|---|---|---|---|---|---|
| `arctic` (default) | snowflake-arctic-embed-m-v2.0 | ~130 MB | 768 | yes (256d) | 74 languages | MIRACL 55.2 |
| `granite-small` | granite-embedding-small-english-r2 | ~48 MB | 384 | no | English only | BEIR 55.6 |

> **ONNX model sourcing:** ndex downloads pre-built ONNX INT8 models from the ndex GitHub releases (not HuggingFace directly). The release pipeline exports and quantizes models using `optimum-cli`, verifies output dimensions and MRL truncation correctness, and publishes the artifacts alongside each ndex release. This ensures reproducibility and avoids dependency on third-party ONNX exports.

> **Note:** Version numbers in examples (0.1.0) reflect the v0.1 milestone. The PRD document version (0.3.0-draft) tracks the document revision, not the software release.

**Offline/air-gapped servers:** For servers with no internet access, models can be pre-staged:

```bash
# On a machine with internet:
ndex-remote model fetch --download-only --output /tmp/minilm.tar.gz

# Copy to the air-gapped server via sneakernet/USB:
scp /tmp/minilm.tar.gz server:/tmp/

# On the air-gapped server:
ndex-remote model import /tmp/minilm.tar.gz
```

---

## 9. Cross-Platform Path Handling

### 9.1 The Problem

Filesystem paths are not strings. They're byte sequences on Unix (`[u8]`) and UTF-16 sequences on Windows (`[u16]`). Neither is guaranteed to be valid UTF-8. Real-world archives contain:

- Filenames in legacy encodings (Shift-JIS, GB2312, Latin-1) that are not valid UTF-8
- macOS NFD normalization artifacts (café stored as `cafe\xCC\x81`)
- Windows filenames with characters illegal on Unix (`:`, `*`, `?`, `<`, `>`)
- Filenames with embedded newlines, control characters, or null bytes (except null — that's the one universal prohibition)

### 9.2 Decision: Platform-Native `OsStr` with WTF-8 Wire Encoding

The index stores and transmits paths in the **Rust `OsStr` semantics**, which map to:

| Platform | Internal representation | In Rust |
|---|---|---|
| Linux / macOS / FreeBSD | Raw bytes (`[u8]`), no encoding guarantee | `OsStr` backed by `[u8]` |
| Windows | WTF-16 (`[u16]`), may contain unpaired surrogates | `OsStr` backed by WTF-8 (`[u8]`) |

**Wire format (msgpack):** Paths are transmitted as raw byte arrays (msgpack `bin` type), representing:
- On Unix: the raw path bytes as-is
- On Windows: the WTF-8 encoding of the path (which is what Rust's `OsStr` uses internally on Windows)

This means:
- Unix paths with non-UTF-8 bytes are preserved losslessly
- Windows paths with unpaired surrogates are preserved losslessly (via WTF-8)
- Valid UTF-8 paths (the vast majority) are identical on both platforms

**SQLite storage:** Paths are stored as `BLOB`, not `TEXT`. SQLite TEXT columns apply UTF-8 validation on storage and can silently replace invalid bytes with `U+FFFD`. BLOB preserves arbitrary byte sequences faithfully.

**Display:** When rendering paths for human consumption, use lossy UTF-8 conversion with replacement characters (`U+FFFD`). For programmatic output (`-f json`, `-f paths`), emit the raw bytes (JSON uses `\uXXXX` escapes for non-UTF-8, paths format emits raw bytes).

### 9.3 Practical Scope

ndex is primarily a Unix tool (Linux, macOS, FreeBSD) because that's where archival storage lives (ZFS, SSH servers). Windows is a non-goal for v0.1 but the path encoding design doesn't preclude it.

The **client** (ndex) could run on Windows (e.g., Windows developer laptop SSHing to a Linux server). Since the client only displays paths and never interprets them as local filesystem paths, this works: the wire format carries the server's Unix path bytes, the client renders them as lossy UTF-8.

The **remote** (ndex-remote) on Windows would need to handle WTF-16 paths. The Rust `std::path` and `os_str` types already handle this transparently. Tantivy and SQLite would need testing with non-UTF-8 paths. This is a future concern.

### 9.4 Why Not Just UTF-8 Everywhere

The tempting simpler approach: require all indexed paths to be valid UTF-8, reject non-UTF-8 filenames.

This is wrong for archival storage because:
1. Multi-decade archives often contain legacy-encoded filenames. Silently skipping them means incomplete search results with no clear error.
2. Even on modern systems, tools like `tar` can create non-UTF-8 filenames when extracting archives from other locales.
3. macOS's NFD normalization means the same visual filename can have different byte representations; treating paths as opaque bytes sidesteps normalization issues entirely.

We handle non-UTF-8 paths; we don't require them to not exist.

---

## 10. Why No ZFS Snapshot-Based Reconciliation

### 10.1 The Proposed Optimization

The v0.2 draft proposed using `zfs diff` between ndex-managed snapshots to get a list of changed files in O(changes) time instead of walking the entire filesystem in O(files) time.

### 10.2 Why It's Rejected

**1. Snapshot namespace pollution.**

Archival ZFS servers already use automated snapshot tools: `sanoid`, `zfs-auto-snapshot`, `zrepl`, `syncoid`. Adding ndex-managed snapshots (`ndex-auto-*`) into the mix creates confusion:

- Which snapshots are safe to prune? Users have to know that `ndex-auto-*` snapshots are ndex's and must not be deleted manually.
- Sanoid's snapshot retention policies might prune ndex snapshots if they match a pattern, or ndex might confuse sanoid snapshots for its own.
- Recursive snapshots on parent datasets (`zfs snapshot -r pool@snap`) interact unpredictably with per-dataset ndex snapshots.

**2. `zfs diff` has surprising behavior.**

- `zfs diff` reports changes at the ZFS block/record level, not the file level. A single `write()` that doesn't change file content (e.g., overwriting with identical bytes) still shows up as modified.
- On datasets with `recordsize=1M` and large files, `zfs diff` output can be enormous even for small changes.
- `zfs diff` can be slow on datasets with many snapshots or heavy COW churn. On a pool with 1000+ snapshots (common with hourly sanoid), it may take longer than a full walk.
- Renamed directories report only the directory rename, not the files inside. ndex would need to recursively stat the renamed subtree anyway.

**3. Requires elevated privileges.**

`zfs diff` requires `zfs allow diff` delegation or root access. Many archival server setups run applications as non-root users. Adding a ZFS permission dependency complicates deployment.

**4. Testing is prohibitively difficult.**

ZFS snapshot behavior can't be mocked in CI. Testing requires actual ZFS pools, which means either:
- ZFS in a VM (slow, flaky, not available on all CI platforms)
- ZFS on Linux with zfs.ko (kernel module, requires root, CI environment dependent)
- No tests, and hoping the behavior is correct

For a feature that saves 20-30 seconds on the reconciliation walk (on NVMe — more on HDD, but still bounded), the testing burden is too high.

**5. The full walk is fast enough.**

The `ignore` crate (ripgrep's walker) with parallel stat achieves:

| Filesystem | Files | Walk time |
|---|---|---|
| NVMe (ext4/ZFS) | 10M | ~30s |
| NVMe (ext4/ZFS) | 50M | ~2.5min |
| HDD RAIDZ2 | 10M | ~3-5min |
| HDD RAIDZ2 | 50M | ~15-20min |

For append-only archival storage, the HDD case is the worst case, and even 20 minutes for a 50M-file walk is acceptable for a full reconciliation. With the auto-refresh heuristic (§7), most searches trigger a walk against a recently-reconciled manifest where only a few thousand new files exist — the diff phase dominates and completes in seconds.

### 10.3 What We Keep from ZFS Awareness

- **Dataset detection:** `ndex init` detects ZFS and stores the dataset name in config. Used for informational purposes (`ndex stats` shows pool/dataset info).
- **ZFS property reading:** `ndex info` can show ZFS-specific metadata (compression ratio, checksum algorithm, recordsize) for context.
- **Integrity note:** `ndex verify` on ZFS reminds the user that `zpool scrub` is the canonical integrity check and that ndex's BLAKE3 verification is defense-in-depth, not a replacement.

---

## 11. Index Catalogue

All indices live under `<root>/.ndex/`. Each is independent, rebuildable, and individually compactable.

### 11.1 Manifest Index — `manifest.db` (SQLite, WAL mode)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;       -- 256 MB page cache
PRAGMA mmap_size = 1073741824;     -- 1 GB mmap

CREATE TABLE files (
    file_id        INTEGER PRIMARY KEY,
    path           BLOB NOT NULL,       -- platform-native bytes (see §9)
    path_hash      INTEGER NOT NULL,    -- xxh3_64(path bytes), for fast lookup
    inode          INTEGER,
    dev            INTEGER,
    size           INTEGER NOT NULL,
    mtime_ns       INTEGER NOT NULL,
    ctime_ns       INTEGER NOT NULL,
    mode           INTEGER NOT NULL,
    uid            INTEGER,
    gid            INTEGER,
    blake3         BLOB,                -- 32-byte BLAKE3 (NULL until processed)
    mime_type      TEXT,
    status         INTEGER NOT NULL DEFAULT 0,
    -- 0=pending, 1=indexed, 2=failed_transient, 3=deleted,
    -- 4=failed_permanent, 5=skipped
    fail_count     INTEGER NOT NULL DEFAULT 0,
    first_seen_ns  INTEGER NOT NULL,
    last_verified_ns INTEGER NOT NULL,
    error_msg      TEXT
);

CREATE INDEX idx_path_hash ON files(path_hash);
CREATE INDEX idx_status ON files(status) WHERE status NOT IN (1, 3);
CREATE INDEX idx_blake3 ON files(blake3) WHERE blake3 IS NOT NULL;
CREATE INDEX idx_mtime ON files(mtime_ns);
CREATE INDEX idx_mime ON files(mime_type) WHERE mime_type IS NOT NULL;
CREATE INDEX idx_size ON files(size);

CREATE TABLE index_progress (
    file_id    INTEGER NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    index_name TEXT NOT NULL,
    schema_ver INTEGER NOT NULL,
    indexed_at_ns INTEGER NOT NULL,
    PRIMARY KEY (file_id, index_name)
) WITHOUT ROWID;

CREATE TABLE reconciliation_runs (
    run_id       INTEGER PRIMARY KEY,
    started_ns   INTEGER NOT NULL,
    completed_ns INTEGER,
    kind         TEXT NOT NULL,
    method       TEXT NOT NULL,
    total_files  INTEGER,
    new_files    INTEGER,
    modified     INTEGER,
    deleted      INTEGER,
    unchanged    INTEGER,
    processed    INTEGER,
    duration_ms  INTEGER,
    timed_out    INTEGER DEFAULT 0,
    error        TEXT
);

CREATE TABLE schema_info (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
```

**Scale notes:**

- `path` as BLOB: preserves non-UTF-8 paths faithfully (§9).
- `path_hash` (xxh3_64 of path bytes): O(1) lookup. Note: xxh3_64 is used ONLY for path lookup acceleration, not content hashing. It's a hash of the path string, not the file content. Collisions are handled by the `AND path = ?` equality check.
- Partial indexes on `status`: skips the 99%+ of files that are `status=1` (indexed) or `status=3` (deleted).
- `WITHOUT ROWID` on `index_progress`: clustered on composite PK, ~40% smaller.

**Estimated sizes:**

| Files | manifest.db | Notes |
|---|---|---|
| 1M | ~300 MB | Comfortable |
| 10M | ~3 GB | Fine |
| 50M | ~15 GB | Approaching practical limit for single SQLite |
| 100M+ | ~30 GB+ | Consider per-dataset sharding (v0.3) |

### 11.2 Full-Text Content Index — `content/` (Tantivy)

**Schema fields:**

| Field | Type | Stored | Fast | Tokenizer |
|---|---|---|---|---|
| `file_id` | u64 | yes | yes | — |
| `chunk_ord` | u64 | yes | yes | — |
| `body` | text | yes | no | `default` |
| `title` | text | yes | no | `default` |
| `path_text` | text | yes | no | `path` |
| `mime` | text | yes | yes | — |
| `lang` | text | yes | yes | — |
| `mtime` | date | yes | yes | — |
| `size` | u64 | yes | yes | — |
| `byte_start` | u64 | yes | no | — |
| `byte_end` | u64 | yes | no | — |

**Tokenizers:**

`default`:
```
UnicodeWordTokenizer → LowerCaser → RemoveLongFilter(80)
→ AsciiFoldingFilter → Stemmer(English)
```

`path`:
```
Split on '/' and '.' → LowerCaser → also emit trigrams per component
```

CJK: `LinderaTokenizer` (Japanese), `CangJieTokenizer` (Chinese), auto-detected per chunk.

**Threading:** Tantivy's `IndexWriter` uses a single writer with internal per-thread document buffers. Multiple extraction workers prepare documents and call `add_document()` concurrently — Tantivy handles the internal synchronization. Only one `IndexWriter` instance should exist per index. Search readers (`IndexReader`) are fully concurrent and lock-free.

### 11.3 Semantic Vector Index — `vectors/` (USearch)

| Parameter | Value |
|---|---|
| Dimensions | 256 (MRL truncation of 768d snowflake-arctic-embed-m-v2.0) |
| Metric | Inner product (on L2-normalized vectors = cosine) |
| Scalar type | f16 |
| Storage per vector | 512 bytes (256d × 2 bytes) |
| Connectivity M | 32 |
| ef_construction | 200 |
| ef_search | 128 (tunable via `--ef`) |

**Sidecar** — `vectors/sidecar.bin`:

```rust
#[repr(C)]
struct SidecarHeader {     // 128 bytes
    magic: [u8; 8],        // b"NDEXVEC\0"
    version: u32,
    entry_count: u64,
    model_name: [u8; 64],
    dimensions: u32,
    _reserved: [u8; 40],
}

#[repr(C)]
struct SidecarEntry {      // 24 bytes (4 bytes padding after chunk_ord for alignment)
    label: u64,
    file_id: u64,
    chunk_ord: u32,
    _pad: u32,
}
```

**Why USearch:** USearch is the only actively maintained Rust-compatible ANN library with native f16 SIMD support, mmap-based serving (`view()` API), concurrent lock-free reads, and filter predicates. Alternatives (hnswlib Rust bindings, hora, annoy-rs) are abandoned since 2021 and lack required features.

**Crash safety:** USearch `save()` is not atomic. ndex uses save-to-temp-then-rename:
```rust
index.save("vectors/index.usearch.tmp")?;
std::fs::rename("vectors/index.usearch.tmp", "vectors/index.usearch")?;
```
`rename()` is atomic on POSIX filesystems (including ZFS). If the process crashes mid-save, only the `.tmp` file is corrupted; the previous `index.usearch` remains valid. On startup, stale `.tmp` files are deleted.

The sidecar (`sidecar.bin`) uses the same save-to-temp-then-rename pattern.

**Threading:** USearch `view()`-based readers are lock-free and `Send + Sync` (concurrent HNSW traversal over mmap'd data). Writes go through a single `Index` instance with per-node bit-level locks — thread-safe for concurrent `add()` calls, but ndex uses a single writer thread for simplicity and to coordinate with the sidecar append.

### 11.4 Metadata Index — `meta.db` (SQLite)

Same pragmas as manifest.

```sql
-- doc_meta: extracted document metadata
CREATE TABLE doc_meta (
    -- file_id references manifest.db:files(file_id) by convention.
    -- Cross-database foreign keys are not enforced by SQLite.
    -- Orphan cleanup is handled by ndex compact.
    file_id     INTEGER PRIMARY KEY,
    title       TEXT,
    author      TEXT,
    subject     TEXT,
    creator     TEXT,        -- application that created the file
    producer    TEXT,        -- PDF producer
    created_at  TEXT,        -- document creation date (ISO 8601)
    modified_at TEXT,        -- document modification date
    page_count  INTEGER,
    word_count  INTEGER,
    lang        TEXT         -- detected language (ISO 639-1)
);

-- media_meta: image/video/audio metadata
CREATE TABLE media_meta (
    -- file_id references manifest.db:files(file_id) by convention.
    -- Cross-database foreign keys are not enforced by SQLite.
    -- Orphan cleanup is handled by ndex compact.
    file_id       INTEGER PRIMARY KEY,
    width         INTEGER,
    height        INTEGER,
    duration_ms   INTEGER,
    codec         TEXT,
    bitrate       INTEGER,
    fps           REAL,
    camera_make   TEXT,
    camera_model  TEXT,
    lens          TEXT,
    iso           INTEGER,
    focal_length  REAL,
    aperture      REAL,
    shutter_speed TEXT,
    gps_lat       REAL,
    gps_lon       REAL,
    gps_alt       REAL,
    taken_at      TEXT         -- EXIF DateTimeOriginal (ISO 8601)
);

-- tags: user-defined and auto-generated tags
CREATE TABLE tags (
    tag_id   INTEGER PRIMARY KEY,
    name     TEXT NOT NULL UNIQUE,
    kind     TEXT NOT NULL DEFAULT 'user'  -- 'user', 'auto', 'ner'
);

CREATE TABLE file_tags (
    -- file_id references manifest.db:files(file_id) by convention.
    -- Cross-database foreign keys are not enforced by SQLite.
    -- Orphan cleanup is handled by ndex compact.
    file_id  INTEGER NOT NULL,
    tag_id   INTEGER NOT NULL REFERENCES tags(tag_id),
    PRIMARY KEY (file_id, tag_id)
) WITHOUT ROWID;

-- NER entities table: deferred to v0.2. Schema will be defined when NER is implemented.
-- Adding it will require a schema version bump and reindex (per §6 no-migration policy).
```

### 11.5 Thumbnail Store — `thumbs/`

```
thumbs/{blake3_hex[0:2]}/{file_id}.sm.webp   (150x150 max)
thumbs/{blake3_hex[0:2]}/{file_id}.md.webp   (600x600 max)
```

256 shards by first byte of the file's BLAKE3 hash, which is uniformly distributed. The filename uses `file_id` for uniqueness.

### 11.6 On-Disk Layout

```
/pool/archive/.ndex/
├── lock                (flock advisory lock)
├── index.toml          (identity: schema version, model, hashing — NEVER modified)
├── config.toml         (user-editable settings)
├── manifest.db         (SQLite WAL)
├── manifest.db-wal
├── manifest.db-shm
├── content/            (tantivy)
│   ├── meta.json
│   ├── .managed.json
│   └── *.{idx,pos,store,fast,fieldnorm,term}
├── vectors/
│   ├── index.usearch   (mmap'd HNSW)
│   └── sidecar.bin     (mmap'd label→file mapping)
├── meta.db             (SQLite: doc/media metadata, tags)
└── thumbs/
    ├── 00/ ... ff/     (256 shards)
```

**Index overhead: ~0.5% of archive size** for typical mixed-content archives. Higher for text-heavy (all PDFs: ~1-2%). Lower for media-heavy (all photos: ~0.1%).

---

## 12. Reconciliation Engine

### 12.1 Three-Phase Design

**Ignore file behavior:**

Ignore hierarchy (evaluated in order, first match wins):
1. `.gitignore` files — respected by default (via `ignore` crate's native support). Follows standard `.gitignore` semantics: per-directory, parent directories consulted, root `.gitignore` at archive root.
2. `.ndexignore` files — same gitignore-compatible syntax, same per-directory hierarchy. Takes precedence over `.gitignore` (can un-ignore files that `.gitignore` excludes via `!pattern`).
3. `--exclude` CLI flags — additive on top of both ignore files.

Rationale: Archives often contain `.gitignore` files from checked-out repos. Respecting them avoids indexing `node_modules/`, `target/`, `.venv/`, build artifacts, etc. — which is almost always the desired behavior.

```toml
[ignore]
respect_gitignore = true    # default: true
respect_ndexignore = true   # default: true
```

**Phase 1: Walk** — parallel filesystem traversal via `ignore` crate.

```rust
let walker = WalkBuilder::new(root)
    .hidden(false)               // index hidden files (dotfiles)
    .git_ignore(true)            // respect .gitignore by default (config-gated)
    .add_custom_ignore_filename(".ndexignore")
    .threads(num_cpus::get())
    .build_parallel();
```

If `ignore.respect_gitignore = false` in config, pass `.git_ignore(false)`.

Produces `DashMap<PathBuf, WalkEntry>` where `WalkEntry = { size, mtime_ns, ctime_ns, inode, dev, mode }`.

> **Memory requirements:** Phase 1 walk and Phase 2 diff hold all file metadata in memory. Estimated ~200 bytes per file entry (path + metadata). At 10M files: ~2 GB RAM. At 50M files: ~10 GB RAM. Systems indexing 50M+ files should have at least 16 GB RAM available for ndex-remote. A streaming/disk-backed approach for very large file counts is deferred to v0.3.

> **Hard limit:** `ndex-remote` checks available system memory before Phase 1. If estimated memory for the walk (file_count_estimate × 200 bytes) exceeds 75% of available RAM, it aborts with a clear error:
> ```
> Error: Estimated 10.2 GB RAM needed for 51M files, but only 7.8 GB available.
> Options:
>   - Index a subdirectory: ndex index /pool/archive/docs
>   - Increase system RAM
>   - Wait for streaming reconciliation (planned for v0.3)
> ```
> The file count estimate comes from the manifest (for re-index) or a quick `stat()` on the root inode's link count (for first index). This is best-effort — the walk may still OOM if the estimate is wrong, but it catches the common case.

**Phase 2: Diff** — compare walk results against manifest.

Load manifest into `HashMap<u64 /* path_hash */, Vec<FileRecord>>`. For each walked file, compute `path_hash`, look up, compare `(size, mtime_ns)`:

- No match → `new`
- Size or mtime changed → `modified`
- Match → `unchanged` (update `last_verified_ns`)
- Manifest entries not in walk → `deleted` (set `status = 3`)

Phase 2 is parallelized via `rayon::par_iter()` over walk results. The manifest hashmap is read-only and shared.

> Note: xxh3_64 collisions are extremely rare (birthday probability ~0.007% at 50M files), so the `Vec<FileRecord>` per path_hash bucket is almost always length 1. The `Vec` exists for correctness — on collision, the `AND path = ?` check disambiguates. An alternative is `HashMap<u64, FileRecord>` with a separate collision map, but the `Vec` approach is simpler and the overhead is negligible.

**Phase 3: Process** — extract, hash, chunk, embed, index.

Multi-threaded pipeline with backpressure:

- N extraction workers (rayon pool, N = num_cpus)
- Each reads file, computes BLAKE3 as streaming side-effect, extracts text, chunks
- Chunks fed to bounded crossbeam channel (cap 4096)
- Tantivy writer (internal thread pool) consumes chunks for FTS
- Embedding thread batches chunks → ONNX inference → USearch writer
- SQLite writer serialized through single-writer channel
- Thumbnail workers on a small rayon sub-pool

### 12.2 Crash Safety

Two-phase commit per file:

1. Manifest insert with `status = 0` (intent)
2. Index writes (FTS, vectors, metadata, thumbnails)
3. `index_progress` rows per completed index
4. Manifest update to `status = 1` only after all progress rows exist

Crash recovery: resume from `status = 0` files, re-process missing indices per `index_progress`.

### 12.3 Concurrency

`flock()` on `.ndex/lock` for write exclusion. Multiple readers (search sessions) run concurrently — SQLite WAL, tantivy readers, and USearch mmap reads all support concurrent access. Readers never block on a writer (WAL mode — readers see the last committed state).

### 12.4 Symlink Handling

**Policy:** ndex follows symlinks by default, matching `find -L` behavior. Symlink cycles are detected by tracking `(dev, inode)` pairs during the walk; a cycle is logged as a warning and the symlink target is skipped.

Symlinks pointing outside the index root are **not followed** — the index only covers files under the root path. This prevents a symlink at `/pool/archive/link → /etc` from pulling in system files.

```toml
[walk]
follow_symlinks = true       # default: true
```

Set `follow_symlinks = false` to index only regular files and skip all symlinks.

---

## 13. IPC Protocol

### 13.1 Why MessagePack

MessagePack was chosen over JSON (too verbose), Protobuf (codegen required, schema file management), Cap'n Proto (complexity), and CBOR (less ecosystem support). Key advantages: schema-less, no codegen, compact binary, tolerates field additions via `#[serde(default)]`, and excellent Rust support via `rmp-serde`.

### 13.2 Wire Protocol

Length-prefixed frames:

```
┌─────────────┬────────────────────────────┐
│ length: u32 │ payload: [u8; length]       │
│ (big-endian)│ (msgpack-encoded Message)   │
└─────────────┴────────────────────────────┘
Max frame: 16 MiB
```

### 13.3 Version Negotiation

First message is always a handshake. Client sends `(min_protocol, max_protocol)` range. Server responds with negotiated version or `HandshakeErr` with a clear upgrade instruction.

```rust
#[derive(Serialize, Deserialize)]
struct HandshakeReq {
    min_protocol: u32,
    max_protocol: u32,
    client_version: String,
    capabilities: Vec<String>,
    terminal: TerminalCaps,
}

#[derive(Serialize, Deserialize)]
struct HandshakeResp {
    protocol_version: u32,          // negotiated
    server_version: String,
    index_schema_version: u32,
    index_model: String,
    index_file_count: u64,
    index_last_reconciled_ns: i64,  // for client-side staleness display
    capabilities: Vec<String>,
    index_healthy: bool,
}
```

**Compatibility contract:**

- Adding optional fields to existing messages: **compatible** (old parsers ignore unknown fields)
- Adding new `MessageKind` variants: **compatible** (unknown variants get `Error` response)
- Removing required fields or changing semantics: **breaking** → bump protocol version
- Protocol version bumps should be rare (years apart)

### 13.4 Message Types

> **Serialization:** Always use `rmp_serde::to_vec_named()` / `rmp_serde::from_slice()`. The `_named` variant serializes field names as strings (required for tagged enum deserialization). Internally tagged enums (`#[serde(tag = "kind")]` without `content`) have known issues in `rmp-serde` and must not be used.

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
enum ClientMessage {
    Handshake(HandshakeReq),
    SearchRequest {
        query: String,
        mode: SearchMode,          // Auto, Fts, Semantic, Hybrid
        filters: SearchFilters,
        limit: u32,
        offset: u32,
        format: OutputFormat,
        explain: bool,
    },
    IndexRequest {
        options: IndexOptions,     // full, verify, dry_run, jobs, etc.
    },
    InfoRequest { path: Vec<u8> },
    StatsRequest {},
    VerifyRequest { paths: Option<Vec<Vec<u8>>>, sample: Option<f64> },
    ReindexRequest { target: ReindexTarget },  // All, Vectors, Fts
    CancelRequest {},
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
enum ServerMessage {
    Handshake(HandshakeResp),
    SearchResult {
        hits: Vec<SearchHit>,
        total: u64,
        mode: SearchMode,
        duration_ms: u64,
        truncated: bool,
        stale_warning: Option<String>,
    },
    IndexComplete {
        stats: IndexStats,
    },
    InfoResult { file_info: FileInfo },
    StatsResult { index_stats: IndexSummary },
    VerifyResult { checked: u64, corrupted: Vec<CorruptedFile> },
    Progress(ProgressEvent),
    Error { code: u32, message: String },
}
```

### 13.5 Remote Discovery

Client probes for `ndex-remote` on the server:

1. `--remote-path` flag if specified
2. `NDEX_REMOTE_PATH` env var
3. `which ndex-remote` (PATH lookup)
4. `~/.local/bin/ndex-remote`
5. `/usr/local/bin/ndex-remote`

On failure:

```
Error: ndex-remote not found on 'nas.local'.

Install on the server:
  ssh nas.local "curl -fsSL https://get.ndex.dev/install.sh | sh"

Or specify the path:
  ndex search nas.local:/pool "query" --remote-path /opt/ndex/ndex-remote
```

Note: the install command runs ON the server (via SSH), not from the client. No client bandwidth used.

---

## 14. CLI Design

### 14.1 Command Reference

```
ndex — deep file indexer for archival storage

USAGE:
    ndex [OPTIONS] <COMMAND>

GLOBAL OPTIONS:
    -v, --verbose           Increase verbosity (repeat: -vv, -vvv)
    -q, --quiet             Suppress non-essential output
    --color <WHEN>          auto (default), always, never
    --no-hyperlinks         Disable OSC 8 hyperlinks
    --config <PATH>         Override config file
    -h, --help              Show help
    -V, --version           Print version

COMMANDS:
    search      Search an index
    index       Build or update the index
    init        Initialize a new index
    info        Show metadata for a file
    stats       Index statistics
    tag         Manage tags
    verify      Verify file integrity
    delete      Remove files from the index
    dedup       Find duplicate files
    compact     Optimize index storage
    reindex     Rebuild index from scratch
    config      View/edit configuration
    completions Generate shell completions
```

### 14.2 `ndex search`

```
ndex search [HOST:]<PATH> <QUERY> [OPTIONS]

ARGUMENTS:
    [HOST:]<PATH>    Local path or remote host:path
    <QUERY>          Search query (FTS syntax or natural language for semantic)

SEARCH MODE:
    -m, --mode <MODE>       auto | fts | semantic | hybrid  [default: auto]

FILTERS:
    --mime <GLOB>           MIME filter ("image/*", "application/pdf")
    --after <DATE>          Modified after (ISO 8601 or relative: "2w", "3m")
    --before <DATE>         Modified before
    --larger <SIZE>         Min size ("10MB", "1GiB")
    --smaller <SIZE>        Max size
    --path <GLOB>           Path glob ("invoices/**/*.pdf")
    --tag <TAG>             Tag filter (repeatable, OR)
    --lang <CODE>           Language filter (ISO 639-1)
    --camera <MODEL>        Camera model (partial match)

OUTPUT:
    -n, --limit <N>         Max results [default: 20]
    --offset <N>            Pagination offset
    -f, --format <FMT>      pretty | plain | json | jsonl | paths | csv
    -c, --context <N>       Context lines [default: 2]
    --no-context            Omit context snippets
    --no-score              Omit scores
    --count                 Print result count only
    --explain               Show scoring breakdown
    --fail-no-results       Exit with code 7 if no results (useful for scripting)

REFRESH:
    --no-refresh            Skip auto-refresh, search stale index
    --refresh               Force refresh even if fresh
    --refresh-timeout <S>   Override refresh time budget [default: 30]

TUNING:
    --ef <N>                USearch ef_search override [default: 128]
    --fts-boost <F:N>       Field boost (e.g., title:3.0)
    --rrf-k <N>             RRF constant [default: 60]

SSH:
    --ssh-key <PATH>        SSH private key
    --ssh-port <PORT>       Port [default: 22]
    --ssh-user <USER>       Username [default: $USER]
    --ssh-option <OPT>      Pass-through SSH option
    --remote-path <PATH>    ndex-remote path on server
```

### 14.3 `ndex index`

```
ndex index [HOST:]<PATH> [OPTIONS]

    --full              Force full re-index
    --verify            Recompute BLAKE3 for unchanged files
    --dry-run           Show changes without writing
    --jobs <N>          Extraction parallelism [default: num_cpus]
    --batch-size <N>    Embedding batch size [default: 64]
    --no-vectors        Skip vector embedding
    --no-thumbs         Skip thumbnails
    --enable-ner        Enable named entity recognition
    --max-file-size <S> Skip files above this size
    --resume            Resume interrupted run
    --only-new          Process only new files (skip modified)
    --status            Show current indexing status and exit
```

### 14.4 `ndex init`

```
ndex init <PATH> [OPTIONS]

    --model <MODEL>     multilingual (default) | english-only | none
                        multilingual = snowflake-arctic-embed-m-v2.0 (~130 MB, 74 langs)
                        english-only = granite-embedding-small-english-r2 (48 MB, EN)
    --exclude <PAT>     Gitignore-style exclude (repeatable)
    --no-fts            Disable full-text index
    --no-meta           Disable metadata extraction
    --no-thumbs         Disable thumbnails
```

### 14.5 `ndex info`, `ndex stats`, `ndex verify`

```
ndex info [HOST:]<PATH> <FILE>
    Show metadata for a specific file in the index.
    Outputs: path, size, mtime, mime, blake3, status, tags,
             doc/media metadata, chunk count, index membership.
    -f, --format <FMT>      pretty | json

ndex stats [HOST:]<PATH>
    Show index statistics.
    Outputs: total files, indexed/pending/failed/skipped counts,
             index sizes (manifest, FTS, vectors, meta, thumbs),
             last reconciliation time, model info, schema version.
    -f, --format <FMT>      pretty | json

ndex verify [HOST:]<PATH> [OPTIONS]
    Verify file integrity by recomputing BLAKE3 hashes.
    --sample <FRAC>         Verify random sample (0.01 = 1%)
    --path <GLOB>           Verify files matching glob
    --fail-fast             Stop on first corruption
    -f, --format <FMT>      pretty | json
```

### 14.6 `ndex reindex`

```
ndex reindex [HOST:]<PATH> [OPTIONS]

    --vectors           Re-embed vectors only (FTS/meta preserved)
    --fts               Rebuild FTS only
    --all               Full rebuild (default)
    --confirm           Skip interactive confirmation prompt

Moves .ndex/ → .ndex.old/, rebuilds, then removes .ndex.old/ on success.
Restores .ndex.old/ on failure.
```

### 14.7 Terminal Features

**OSC 8 Hyperlinks:**

Detected via `$TERM_PROGRAM` (ghostty, kitty, WezTerm, iTerm, foot, rio, vscode) or `NDEX_HYPERLINKS=1`. File paths in search results are clickable:

```
\x1b]8;;file:///pool/archive/docs/contract.pdf\x1b\\docs/contract.pdf\x1b]8;;\x1b\\
```

For remote results, use `file-host://` with the remote hostname:

```
\x1b]8;;file://nas.local/pool/archive/docs/contract.pdf\x1b\\docs/contract.pdf\x1b]8;;\x1b\\
```

(Some terminals, e.g. ghostty, can open remote files via SSH when given `file://host/path`.)

**Color scheme** (semantic palette, not hardcoded RGB):

| Element | ANSI |
|---|---|
| File path | Bold (`\x1b[1m`) |
| Match highlight | Bold yellow (`\x1b[1;33m`) |
| Score | Dim (`\x1b[2m`) |
| MIME type | Cyan (`\x1b[36m`) |
| Size | Green (`\x1b[32m`) |
| Date | Blue (`\x1b[34m`) |
| Error | Red (`\x1b[31m`) |
| Tag | Magenta (`\x1b[35m`) |

Respects `NO_COLOR`, `NDEX_COLOR=never`, and `--color never`.

**Progress bars:**

Multi-bar rendering via `indicatif` when interactive. Falls back to periodic line updates when piped.

The remote sends `ProgressEvent` messages over the wire:

```rust
struct ProgressEvent {
    phase: String,          // "walk", "diff", "extract", "embed", "fts", "meta"
    current: u64,
    total: Option<u64>,
    message: Option<String>,
    children: Vec<ProgressChild>,
}
```

The client renders these. The remote knows nothing about terminal capabilities — it sends structured progress, the client decides how to display.

**Piped output:**

When stdout is not a TTY, all formatting is stripped. Default format changes from `pretty` to `plain`. Color, hyperlinks, and progress bars are suppressed.

```bash
ndex search /pool "invoice" -f paths | xargs cp -t /tmp/invoices/
ndex search /pool "invoice" -f jsonl | jq '.path'
```

**Shell completions** (via `clap_complete`):

```bash
ndex completions bash > /etc/bash_completion.d/ndex
ndex completions zsh > ~/.zfunc/_ndex
ndex completions fish > ~/.config/fish/completions/ndex.fish
```

**Exit codes:**

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | General error |
| 2 | Usage error |
| 3 | Index not found |
| 4 | Remote connection failed |
| 5 | Remote version incompatible |
| 6 | Index schema mismatch (needs rebuild) |
| 7 | No results (with `--fail-no-results`) |
| 78 | Configuration error |
| 130 | Interrupted (Ctrl-C) |

**Environment variables:**

| Variable | Purpose |
|---|---|
| `NDEX_COLOR` | `always` / `never` / `auto` |
| `NDEX_HYPERLINKS` | `1` / `0` / `auto` |
| `NDEX_SSH_COMMAND` | Override SSH binary |
| `NDEX_REMOTE_PATH` | Default `ndex-remote` path on servers |
| `NDEX_CONFIG_DIR` | Override `~/.config/ndex/` |
| `NDEX_LOG` | Log filter (`ndex=debug,tantivy=warn`) |
| `NO_COLOR` | [no-color.org](https://no-color.org) convention |

**Client config** (`~/.config/ndex/hosts.toml`):

```toml
[hosts.nas]
hostname = "nas.local"
user = "admin"
port = 22
key = "~/.ssh/nas_ed25519"
remote_path = "/usr/local/bin/ndex-remote"
default_root = "/pool/archive"

# Shorthand: ndex search nas: "query"
```

**Client global config** (`~/.config/ndex/config.toml`):

```toml
[display]
color = "auto"        # auto | always | never
hyperlinks = "auto"   # auto | always | never
format = "pretty"     # default output format

[ssh]
default_key = "~/.ssh/id_ed25519"
default_user = ""     # empty = $USER
```

CLI flags and env vars override config file values. Per-host settings in `hosts.toml` override global settings.

### 14.8 `ndex delete`

```
ndex delete [HOST:]<PATH> <GLOB> [OPTIONS]
    Remove matching files from all indices (manifest, FTS, vectors, meta, thumbs).
    --dry-run           Show what would be deleted
    --confirm           Skip interactive confirmation

    Example: ndex delete /pool "secrets/**/*.key"
```

This sets `status=3` in the manifest and removes entries from FTS, vectors, meta, and thumbs. The files on disk are not touched.

### 14.9 `ndex compact`

```
ndex compact [HOST:]<PATH> [OPTIONS]
    Optimize index storage by reclaiming space from deleted/updated entries.

    Performs:
    - SQLite VACUUM on manifest.db and meta.db
    - Tantivy segment merge (reduces segment count, reclaims deleted docs)
    - USearch rebuild (removes tombstoned vectors, re-optimizes HNSW graph)
    - Thumbnail cleanup (removes orphaned thumbs for deleted files)

    --dry-run           Show estimated space savings
    --only <INDEX>      Compact specific index: manifest | fts | vectors | meta | thumbs
```

---

## 15. Search Result Rendering

Full example of the `pretty` format in an interactive terminal:

```
 ❯ ndex search /pool/archive "quarterly earnings" --after 2024-01-01

 1. docs/finance/Q3-2024-earnings.pdf                        0.847
    application/pdf · 2.3 MiB · 2024-10-15 · [finance] [Q3]
    ...the company reported strong quarterly earnings growth of 23% year
    over year, driven primarily by...

 2. docs/finance/board-meeting-2024-08.docx                  0.791
    application/vnd.openxmlformats · 847 KiB · 2024-08-22
    ...agenda item 4: review of quarterly earnings projections
    for the upcoming fiscal year...

 3. reports/annual/2024-annual-report.pdf                     0.723
    application/pdf · 14.1 MiB · 2025-01-30 · [annual] [report]
    ...consolidated quarterly earnings summary (see appendix B
    for detailed breakdown by segment)...

 ── 47 results (1-20) · hybrid · 23ms ──
```

Each file path is an OSC 8 hyperlink (when supported). Matched query terms are highlighted in bold yellow within the context snippets. Tags are in magenta brackets. The footer shows total count, mode, and query time.

---

## 16. Milestones

### v0.1 — Core

- Reconciler (parallel walk, metadata diff, BLAKE3 hashing)
- Manifest (SQLite WAL), FTS (tantivy), vectors (USearch + snowflake-arctic-embed-m-v2.0, 256d MRL)
- Metadata index (doc_meta, media_meta)
- Extraction: pdf, docx, txt, md, html, code, images (EXIF)
- CLI: init, index, search (fts/semantic/hybrid), info, stats, reindex
- SSH remote with version negotiation
- Auto-refresh on search
- Auto model fetch on remote
- OSC 8 hyperlinks, progress bars, shell completions
- No-migration index versioning
- .ndexignore, crash-safe incremental indexing

### v0.2 — Breadth

- CJK tokenizers, archive indexing, email indexing
- Tags, NER, dedup, verify, compact
- Thumbnails, CUDA embedding
- `ndex-remote self-update`

### v0.3 — Scale + Intelligence

- CLIP image semantic search
- OCR for scanned PDFs
- Product quantization for 100M+ vector scale
- Per-dataset sharded manifests
- Optional local web UI

---

## 17. Open Questions

1. **Multi-root.** Single ndex instance spanning `/pool/photos` + `/pool/docs`. Requires root registry and cross-root dedup. v0.3?

2. **Embedding model migration.** Current: schema mismatch → full re-embed via `ndex reindex --vectors`. Should we support old vectors for FTS-only search while re-embedding in background?

3. **Index portability.** All paths are relative to root. Copying `.ndex/` to another machine with the same layout should work. Needs testing.
