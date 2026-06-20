# PRD: `ndex` — Deep File Indexer for Archival Storage

**Version:** 0.3.0-draft
**Date:** 2026-03-30
**Status:** Draft (pre-implementation amendments applied 2026-03-23; format coverage amendments applied 2026-03-29; additional format coverage amendments applied 2026-03-30; implementation-readiness amendments applied 2026-03-30)

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
5. **Zero idle footprint.** No daemon. ndex-remote is spawned on demand per SSH session. mmap'd index files are retained in the OS page cache across processes — the kernel provides warm-cache benefits without a daemon. If cold start ever matters on slow storage, use vmtouch.
6. **No index migrations.** Index schema changes require a full rebuild. Correctness over convenience. The index is always rebuildable from source files.

---

## 3. Architecture Overview

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
│  ndex-remote (fat binary, ~80-100 MB statically linked + ~300 MB model on disk) │
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
│  └── meta.db           (SQLite: doc/media metadata, tags)        │
└──────────────────────────────────────────────────────────────────┘
```

**Local mode:** When no `HOST:` prefix is given, `ndex` spawns `ndex-remote` as a local subprocess (not over SSH). Communication uses the same msgpack protocol over stdin/stdout pipes. This means `ndex-remote` must be installed locally for local operation. The thin client never embeds extraction/indexing logic.

`ndex-remote` also works standalone for local-only use. All commands that the thin client proxies are available directly: `ndex-remote search --root /pool "query"`, `ndex-remote index --root /pool`, etc. The thin client adds SSH transport, host aliases, and nicer terminal rendering, but is not required for local operation.

---

## 4. Hashing: BLAKE3 Everywhere

### 4.1 Decision

**Use BLAKE3 as the single hash for all content hashing purposes.** No xxHash3, no split strategy.

> **Scope clarification:** "BLAKE3 Everywhere" means content hashing only — file integrity, dedup, model verification. xxHash3-64 is still used as a non-cryptographic path lookup accelerator in the `path_hash` column of `manifest.db` (see §10.1). This is an internal implementation detail, not a content hash — it is not stored alongside files and has no integrity meaning.

### 4.2 Rationale

**The reconciliation fast path doesn't hash at all.** Phase 1 and Phase 2 use only filesystem metadata: `(size, mtime_ns, inode)`. Content hashing happens in Phase 3 (process), where we're already reading the entire file for text extraction — BLAKE3 at 4-6 GB/s is never the bottleneck. Fast path doesn't hash; BLAKE3 happens during extraction; dedup matches on `(blake3, size)`.

### 4.3 When BLAKE3 Is Computed

BLAKE3 is computed **exactly once per file, during the extraction phase (Phase 3).** The extraction workers read the entire file into memory (or via mmap), hash the raw bytes with BLAKE3, then pass the bytes to the format-specific extractor.

> **Architecture note:** BLAKE3 hashing is streaming and forward-only. However, most format extractors are **not** — PDF parsers need random access (xref table at EOF), DOCX is a ZIP archive (central directory at end), and tree-sitter parses full in-memory buffers. The actual pattern is: read all bytes into a buffer (or mmap the file), hash the buffer, then extract from the buffer. The conceptual "streaming side-effect" applies only to plaintext and simple line-based formats.

```rust
fn process_file(path: &Path) -> Result<ProcessedFile> {
    // Read full file into memory — required by most extractors (PDF xref, ZIP CD, tree-sitter)
    let bytes = std::fs::read(path)?;

    // BLAKE3 is streaming-capable but we hash the full buffer for simplicity
    let hash = blake3::hash(&bytes);

    // Extractor receives full bytes; format determines whether it needs random access
    let extractor = get_extractor(mime_type);
    let text = extractor.extract(&bytes)?;

    Ok(ProcessedFile { hash, text, ... })
}
```

For very large files (up to `max_file_size`, default 2 GiB), memory-mapped I/O (`mmap`) is used instead of reading into a heap buffer. BLAKE3 hashes the mmap'd bytes directly via `update_rayon()` for parallelism.

No separate "hashing pass." No lazy computation. No optional anything. Every indexed file has a BLAKE3 hash, period.

**For files that fail extraction** (status = 2 or 4): We still compute the hash during the read attempt. Even if text extraction fails, the hash is stored. This enables dedup detection even for files we can't read.

BLAKE3 hashes are stored in the `blake3` column of the `files` table in `manifest.db`. For `ndex verify`, hashes are read from the manifest and compared against freshly computed hashes.

### 4.4 Library Selection for Format Extraction

| Format | Primary library | Rationale |
|---|---|---|
| PDF | `pdf_oxide` | Actively maintained (2026-01-07 release), 5× faster than `pdf-extract`, 100% pass rate on 3,830 real-world PDFs |
| PDF (fallback) | `pdfium` | Handles edge cases pdf_oxide cannot; requires bundled native lib (~25 MB, see §4.4 note) |
| DOCX | `docx-rust` | Full reading support for paragraph styles, headings, tables; `docx-rs` (bokuweb) is primarily a writer |
| DOCX (v0.1 fallback) | Best-effort paragraph splitting | If structured extraction fails, fall back to paragraph-boundary splitting |
| Markdown | `pulldown-cmark` | Widely used, battle-tested |
| HTML | `lol-html` | Streaming; `scraper` for DOM queries when structure needed |
| Code | `tree-sitter` | Parses full in-memory buffer; language-specific grammars |
| Images (EXIF) | `kamadak-exif` | Pure-Rust EXIF extraction; v0.1 image metadata only |
| MIME detection | `infer` + `mime_guess` | Magic-byte sniffing (primary) + extension fallback; see §4.8 |

> **v0.1 scope:** For DOCX, accept best-effort structure extraction. If `docx-rust` cannot extract heading styles (e.g., malformed file), fall back to paragraph-boundary splitting. Log a warning and set `status=2` (failed_transient) only if no text can be extracted at all.

> **pdfium distribution:** The `pdfium` fallback requires a native shared library (`libpdfium.so` / `pdfium.dll` / `libpdfium.dylib`, ~25 MB). Distribution strategy for v0.1: bundle the prebuilt `pdfium` binary inside the `ndex-remote` release tarball alongside the binary, and load it via `LD_LIBRARY_PATH` or a relative `rpath`. The install script places it in the same directory as `ndex-remote`. The `pdfium-render` crate's `auto` feature can download prebuilt pdfium at build time for dev convenience. For distribution builds, pin to a specific pdfium version and include in the release artifact. Users on unsupported architectures can disable the pdfium fallback at compile time (`--no-default-features`).

> **MIME type detection:** ndex uses a two-pass detection strategy: `infer` crate (magic-byte sniffing, ~400 known signatures) as the primary source, falling back to `mime_guess` (extension-based) when `infer` returns `None`. When extension and magic bytes disagree (e.g., a `.txt` file that is actually gzip-compressed), the magic-byte result takes precedence, logged at `DEBUG` level. Extensionless files rely on magic bytes alone; if unidentifiable, they are classified as `application/octet-stream` and skipped (`status=5`). The detected `mime_type` is stored in the manifest and determines which extractor is invoked. See §4.8 for format-specific routing decisions and edge cases.

> **tree-sitter language detection:** Language selection for the code extractor follows three steps: (1) **Extension lookup** — a static table maps extensions to tree-sitter grammars (`.rs`→Rust, `.py`→Python, `.js`→JavaScript, `.ts`→TypeScript, `.c`/`.h`→C, `.cpp`/`.cxx`/`.hpp`→C++, etc.). (2) **Shebang detection** — if no extension match and the first line starts with `#!`, the interpreter name is parsed (e.g., `#!/usr/bin/env python3` → Python). (3) **Plaintext fallback** — if no grammar is available for the detected language, the file falls through to the plaintext extractor, logged at `DEBUG` level. Ambiguous cases in v0.1: `.h` → C (not C++), `.m` → Objective-C. These are pragmatic defaults for archive workloads where C is more prevalent.

> **v0.1 grammar set:** The following tree-sitter grammars are bundled: Rust, Python, JavaScript, TypeScript, TSX, C, C++, Go, Java, Ruby, PHP, C#, Swift, Kotlin, Scala, Lua, Bash/Shell, Haskell, Elixir, Erlang, OCaml, R, TOML, YAML, JSON, HTML, CSS, SQL, Dockerfile, Makefile, Protobuf, HCL (Terraform). Languages without a bundled grammar (Perl, Dart, Zig, Nim, etc.) fall through to the plaintext extractor. The grammar set is compiled into the `ndex-remote` binary — adding new grammars requires a release.

### 4.5 Chunking Strategy

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
| PDF | `pdf_oxide` / `pdfium` (fallback) | Section breaks, page breaks |
| DOCX | `docx-rust` (reading) | Paragraph styles, headings |
| Code | `tree-sitter` | Functions, classes, modules |
| Plaintext | Recursive splitter | `\n\n` > `\n` > `. ` > ` ` |
| Logs | Line-based | Timestamp patterns, fixed line batches |
| CSV/TSV | Record-based | Row boundaries; header propagation; delimiter auto-detected (`,`, `\t`, `;`, `|`) via first-line heuristic; `.tsv` and `text/tab-separated-values` routed to same extractor; quoted newlines handled |
| JSON | Record-based | Variant-aware: single object → split at top-level keys; JSON array → split at element boundaries; NDJSON → split at line boundaries |
| SQL | Statement-based | `;` delimiters |

> **Code structure signals:** The tree-sitter code extractor uses AST node types as chunk boundaries. Top-level declarations (functions, methods, classes, struct/enum definitions, impl blocks, module declarations) are treated as section-level boundaries (priority 1 in the boundary hierarchy). Within a function body, statement boundaries serve as paragraph-level boundaries. The function/class/module name is propagated as heading context (like `heading_prefix` for document headings), so a chunk from inside `fn process_file()` is prefixed with `process_file` for better search relevance.

> **Log file detection:** Files are routed to the log chunker when: (1) the extension is `.log`, or (2) the MIME type is `text/plain` and the first 10 lines match a common log timestamp pattern (ISO 8601, syslog `MMM DD HH:MM:SS`, Apache/nginx combined log format, or `YYYY-MM-DD HH:MM:SS.mmm`). If no timestamp pattern is detected, the file falls through to the plaintext splitter. Files with extensions `.out` and `.err` are not auto-detected as logs — they use the plaintext path unless timestamp patterns are found.

> **Markdown frontmatter:** YAML frontmatter (delimited by `---`) and TOML frontmatter (delimited by `+++`) in Markdown files is parsed by `pulldown-cmark` as raw text. In v0.1, frontmatter fields (`title`, `author`, `date`, etc.) are **not** extracted into `doc_meta` — they are indexed as regular text content within the first chunk. Structured frontmatter extraction into `doc_meta` is deferred to v0.2.

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

### 4.6 Large File Strategy

**Text-extractable files (text/\*, pdf, docx, code, md, html, etc.):**
- Stream through extractor regardless of size (BLAKE3 + extraction in single pass)
- Configurable `max_file_size` (default: 2 GiB) — files above this get metadata-only indexing with `status=5` (skipped), logged as warning
- Rationale: PDF extractors and some format parsers may buffer entire file; the cap prevents OOM (see config reference in §17)

**Media files (image/\*, video/\*, audio/\*):**
- **Images:** EXIF metadata extraction in v0.1 (see §4.8 EXIF matrix). `status=1`.
- **Video** (MP4, MKV, AVI, MOV, WebM, FLV, WMV, MPEG, etc.): `status=1` (indexed) with a `media_meta` row containing all fields NULL. No metadata extraction in v0.1 — codec, duration, resolution, bitrate require `ffprobe` or a dedicated parser (deferred to v0.2). The file is searchable by path, filename, size, and mtime.
- **Audio** (MP3, FLAC, WAV, AAC, OGG, M4A, WMA, AIFF, OPUS, etc.): Same as video — `status=1`, `media_meta` row with all fields NULL. ID3/Vorbis tag extraction (artist, album, title, track) deferred to v0.2.
- No transcription/OCR in v0.1 (deferred to v0.3).

> **Rationale for `status=1` not `status=5`:** Video and audio files are valid indexed entries — they appear in path searches, `ndex info`, and `ndex stats`. Marking them `status=5` (skipped) would imply they are excluded from the index entirely. `status=1` with empty metadata accurately reflects "indexed, no content extracted yet."

> **Sparse files:** On filesystems that support sparse files (ZFS, ext4, XFS), the file's logical size (from `stat()`) may greatly exceed its physical size on disk. The `max_file_size` check uses the logical size, so a 100 GiB sparse file with 1 MiB actual data is correctly skipped. For sparse files under `max_file_size`, `std::fs::read()` reads the full logical size (holes become zero bytes). This is wasteful but correct. mmap (`memmap2`) handles sparse files more efficiently (zero-page holes are not physically read). The mmap threshold (files above which mmap is used instead of `read()`) is 64 MiB — most sparse files encountered in practice exceed this threshold.

**Archives (zip, tar, gz, 7z, rar):**

Archive content is indexed in v0.1. Archives are progressively extracted in-memory and their members processed through the standard pipeline (MIME detect → extractor → chunker → BLAKE3 → embedder → index), just like regular files.

**Supported archive formats (v0.1):**

| Format | Crate | Notes |
|---|---|---|
| ZIP | `zip` | Random access; read each member sequentially |
| TAR | `tar` | Streaming sequential read |
| TAR.GZ / TGZ | `flate2` + `tar` | Streaming decompression → tar reader |
| TAR.BZ2 | `bzip2` + `tar` | Same |
| TAR.XZ | `xz2` / `liblzma` + `tar` | Same |
| GZ/BZ2/XZ (single file) | `flate2`/`bzip2`/`xz2` | Decompress inner file in-memory, index the result |

**Deferred to v0.2:** 7Z (`sevenz-rust`), RAR (`unrar` — native C dependency). These remain metadata-only (`status=1`, `extraction_status='metadata_only'` in `archive_meta`) in v0.1.

**OOXML exclusion:** DOCX, XLSX, PPTX are ZIP-based but are NOT recursively unpacked — their dedicated extractors handle the ZIP structure internally. The archive extractor skips files whose MIME type already has a dedicated extractor.

**Extraction flow:**
1. Archive file is detected by MIME type during Phase 3.
2. The archive is opened as a **streaming reader** — the archive file is read sequentially from disk (or mmap), never fully loaded into a second buffer.
3. Each member entry is processed:
   a. Member metadata is read (path, compressed size, uncompressed size).
   b. Safety checks applied (see §4.9 archive safety).
   c. Member content is decompressed into an in-memory `Vec<u8>` bounded at `max_file_size`.
   d. The in-memory buffer goes through the standard pipeline: MIME detect → extractor → chunker → BLAKE3 hash → embedder → index writes.
   e. Buffer is dropped before the next member — sequential, one member at a time.
4. No temporary files are written to disk.

**Member path convention:** `<archive_path>!/<member_internal_path>` — the `!/` delimiter (JAR URL convention) distinguishes archive boundaries from filesystem separators. Example: `/pool/docs/reports.tar.gz!/2024/Q3-report.pdf`. Nested archives: `outer.zip!/inner.tar.gz!/file.txt`.

**Re-indexing:** When an archive's `(size, mtime_ns)` changes, all existing members (`parent_archive_id = archive_file_id`) are marked `status=3` (deleted) and the archive is re-extracted from scratch.

When an archive is deleted, its entry is marked `status=3` and all members are cascade-marked `status=3`.

**Display:** Archive member paths in search results:
- Pretty format: `reports.tar.gz → 2024/Q3-report.pdf` (arrow separator)
- JSON/paths format: `reports.tar.gz!/2024/Q3-report.pdf` (raw `!/` delimiter)
- OSC 8 hyperlinks: link to the archive file itself (members cannot be directly opened)

`ndex info` on an archive shows member count, total uncompressed size, and extraction status.

### 4.7 Embedding Pipeline

#### Hardware

**v0.1 is CPU-only.** ONNX Runtime uses multi-threaded CPU inference. CUDA/ROCm GPU acceleration is deferred to v0.2.

**ONNX Runtime crate:** ndex uses the `ort` crate (pykeio/ort) for ONNX Runtime bindings. Recommended configuration:
- **Static linking** for distribution builds: bundle ONNX Runtime into the binary to avoid shared library dependencies on the server. This increases the binary size from ~30 MB to ~80-100 MB but eliminates `libonnxruntime.so` runtime dependencies.
- **`download-binaries` feature** for development builds: pulls ONNX Runtime prebuilt binaries at compile time, simplifying the dev environment setup.
- In `Cargo.toml`: `ort = { version = "2", features = ["download-binaries"] }` for dev; for distribution, use static linking via `ort = { version = "2", features = ["static"] }` or bundle the ONNX Runtime library manually.

> **Binary size note (§3, §7):** Static linking of ONNX Runtime raises `ndex-remote`'s binary size from ~30 MB to ~80-100 MB. The ~30 MB figure in the architecture diagram (§3) and server self-installation section (§7) refers to a dynamically-linked build. Distribution builds should document the actual size (~80-100 MB statically linked). The ~300 MB model download is unchanged.

**ONNX session tuning:** The `ort` crate exposes several session-level configuration knobs that can be surfaced via `config.toml`:
- **Intra-op parallelism** (`intra_op_num_threads`): threads used within a single ONNX op (e.g., matrix multiply). Defaults to all cores. Configurable via `embedding.intra_op_threads`.
- **Inter-op parallelism** (`inter_op_num_threads`): threads for scheduling independent graph nodes in parallel. Defaults to 1. Configurable via `embedding.inter_op_threads`.
- **Execution providers**: CPU (default), CUDA (v0.2), CoreML (future). The active EP is logged at startup.
- **Memory arena settings**: ONNX Runtime pre-allocates a memory arena; on memory-constrained systems, disable with `OrtArenaCfg` (exposed via `ort` API). Not user-configurable in v0.1 but implementers should be aware.

Default v0.1 behavior: intra-op uses all cores, inter-op = 1, CPU EP only.

#### Query Prefix

`snowflake-arctic-embed-m-v2.0` uses asymmetric embedding: documents and queries use different prefixes. ndex applies these automatically:

| Use | Prefix |
|---|---|
| Document chunks (at index time) | *(none)* |
| Search queries (at search time) | `"query: "` |

Omitting the query prefix degrades retrieval quality significantly. The prefix is hardcoded per model and not user-configurable.

#### Tokenizer

The `tokenizer.json` bundled with each model (HuggingFace format, loaded via `tokenizers` crate) is used for:
- **Token counting** during chunking (§4.5): chunks are sized in model tokens, not characters
- **Tokenization** before ONNX inference

#### Query Flow

1. User issues `ndex search /pool "quarterly earnings"`
2. Client sends `SearchRequest { query: "quarterly earnings", mode: Auto, ... }` to server
3. Server applies mode heuristics (§10.7) to determine FTS / semantic / hybrid
4. For semantic/hybrid: prepend query prefix, tokenize, run ONNX inference → 768d vector → MRL truncate to 256d → L2 normalize → USearch ANN query
5. Results merged and returned

#### Query Length and Truncation

Max query length: **512 tokens** (model limit). Queries exceeding this are truncated with a warning. In practice, search queries are short (5-15 tokens); this limit is never reached in normal use.

#### Batch Size and Throughput

At index time, chunks are batched for inference.

Throughput on a 16-core server: ~4,000 chunks/sec. At 512 tokens/chunk, this is the primary bottleneck for large text archives. (see config reference in §17)

### 4.8 Format Coverage Policy and Edge Cases

#### OOXML Formats (XLSX, PPTX)

XLSX and PPTX are ZIP archives containing XML (OOXML format), like DOCX. In v0.1:

- **XLSX** (`application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`): Detected by magic bytes. Treated as archive metadata-only (file count, member listing) — the same path as ZIP. Text extraction from spreadsheet cells is deferred to v0.2.
- **PPTX** (`application/vnd.openxmlformats-officedocument.presentationml.presentation`): Same policy. Metadata-only in v0.1.

The rationale for not treating these as plaintext: OOXML XML internals (shared-strings tables, formula references, cell coordinates) produce noisy, low-quality text when naively extracted. A dedicated XLSX/PPTX extractor is required for useful results.

**Unknown ZIP-based document formats:** Any ZIP file whose MIME type is not explicitly handled (e.g., an ODS or EPUB file not yet in the v0.1 format table) falls through to the archive metadata path — file count, total size, member listing, `status=1` (indexed as archive). It is not classified as `status=5` (skipped) or `status=4` (failed_permanent).

#### XML

XML files (`.xml`, `application/xml`, `text/xml`) are handled by the HTML extractor path (`lol-html` / `scraper`). lol-html's lenient tag handling produces acceptable results for well-formed XML. For severely malformed XML that `lol-html` cannot parse, the plaintext fallback applies. A dedicated XML extractor is not warranted in v0.1 given the diversity of XML dialects.

#### SVG

SVG files (`image/svg+xml`, detected by magic bytes `<svg` or `<?xml` with SVG namespace) are routed to the **HTML/XML text extraction path**, not the image EXIF path. Rationale: SVG files contain searchable text content (`<text>`, `<title>`, `<desc>` elements, labels, annotations) that is valuable for full-text search. Pixel dimensions (`width`/`height` attributes) are extracted into `media_meta` if present, but no `doc_meta` row is created. EXIF does not apply to SVG.

#### Config and Markup Formats (Plaintext Path)

The following formats are treated as plaintext (recursive `\n\n` splitter, §4.5). They are not listed in the §4.5 extraction table because their handling is identical to plain text — no per-format structure signals are used.

| Extension / Format | MIME type | Notes |
|---|---|---|
| `.yaml`, `.yml` | `text/yaml` / `application/yaml` | Human-readable config; plaintext splitter works well |
| `.toml` | `application/toml` | Same |
| `.ini`, `.cfg`, `.conf` | `text/plain` | Same |
| `.rst` (reStructuredText) | `text/x-rst` | Heading underline markers (`===`, `---`) act as paragraph boundaries |
| `.adoc`, `.asciidoc` (AsciiDoc) | `text/asciidoc` | Treated as plaintext |
| `.tex` (LaTeX) | `application/x-tex` | Treated as plaintext; TeX commands appear as-is in indexed text |

#### Extensionless and Dotfile Text Detection

Files that `infer` (magic bytes) and `mime_guess` (extension) both fail to identify — including extensionless files (`Makefile`, `Dockerfile`, `Vagrantfile`, `Procfile`, `Jenkinsfile`, `Gemfile`, `Rakefile`, `CMakeLists.txt`) and dotfiles (`.bashrc`, `.zshrc`, `.gitconfig`, `.editorconfig`, `.dockerignore`, `.eslintrc`, `.prettierrc`) — undergo a **text-or-binary heuristic** before being classified as `application/octet-stream`:

1. Read the first 8192 bytes of the file.
2. If the buffer contains a **null byte** (`\x00`), classify as binary → `application/octet-stream` → `status=5` (skipped).
3. If no null byte is found, classify as `text/plain` → route to the plaintext extractor.

This heuristic matches Git's text/binary detection and ensures common extensionless text files are indexed rather than silently skipped. The heuristic runs **after** MIME detection fails, not as a replacement — files with recognized MIME types bypass this check entirely.

> **Known file table (static):** As an optimization, a hardcoded table maps common extensionless filenames to MIME types: `Makefile` → `text/x-makefile`, `Dockerfile` → `text/x-dockerfile`, `CMakeLists.txt` → `text/x-cmake`. These are routed to the plaintext extractor (or tree-sitter if a grammar exists). The table is small (~20 entries) and not user-configurable in v0.1.

#### JSON Variants

JSON in archives comes in three forms, distinguished by heuristic inspection of the first non-whitespace character and initial structure:

| Variant | Detection heuristic | Chunking strategy |
|---|---|---|
| Single JSON object | File starts with `{` | Indexed as single document; split at top-level key boundaries if > `target_tokens` |
| JSON array of objects | File starts with `[` | Split at array element boundaries (`}, {`) |
| NDJSON / JSON Lines | Multiple top-level values, one per line | Split at line boundaries (each line is a record) |

Files that do not parse as valid JSON fall back to the plaintext extractor.

#### GZ/BZ2/XZ: Single-File vs. Tarball

See §4.6 for the definitive policy. Summary:

| Type | Detection | v0.1 behavior |
|---|---|---|
| `.tar.gz`, `.tgz`, `.tar.bz2`, `.tar.xz` | Inner stream is a valid tar archive | Full content extraction: members indexed |
| `.gz`, `.bz2`, `.xz` (single file) | Inner stream is not a tar archive | Decompress inner file in-memory, index the result |

Both tarball and single-file compressed formats are fully extracted in v0.1. See §4.6 for the complete archive extraction flow.

#### Image EXIF: Per-Format Support Matrix

`kamadak-exif` provides EXIF data for a subset of image formats. The `image` crate is used for pixel dimensions on all formats.

| Format | EXIF support | Fields populated in `media_meta` |
|---|---|---|
| JPEG / `.jpg` | Full | All fields: camera, GPS, taken_at, ISO, aperture, focal_length, shutter_speed |
| TIFF | Full | Same as JPEG |
| HEIC / HEIF | Full (EXIF in HEIC container) | Same as JPEG |
| WebP | Partial | WebP with EXIF segment: full fields. Lossless WebP (VP8L) with no EXIF segment: `width`, `height` only |
| PNG | None (PNG uses tEXt/iTXt chunks, not EXIF) | `width`, `height` only; all EXIF fields NULL |
| GIF | None | `width`, `height` only; all EXIF fields NULL |
| BMP | None | `width`, `height` only via `image` crate; all EXIF fields NULL |
| CR2 (Canon Raw) | Full via TIFF-based container | All EXIF fields (camera, GPS, taken_at, ISO, aperture, focal_length, shutter_speed) |
| NEF (Nikon Raw) | Full via TIFF-based container | Same as CR2 |
| ARW (Sony Raw) | Full via TIFF-based container | Same as CR2 |
| DNG (Adobe Raw) | Full (TIFF-based, standardized) | Same as CR2 |
| ORF (Olympus Raw) | Partial — `kamadak-exif` best-effort | Core EXIF fields; some proprietary MakerNote tags may be missing |
| RW2 (Panasonic Raw) | Partial — `kamadak-exif` best-effort | Same as ORF |

For PNG, GIF, and BMP, a `media_meta` row is still inserted with `width` and `height` populated. NULL EXIF fields are not an error — these formats simply do not carry EXIF.

> **Raw format note:** Most raw camera formats (CR2, NEF, ARW, DNG) use TIFF-based containers, and `kamadak-exif` parses them via its TIFF reader. Proprietary MakerNote tags (camera-specific extensions) are not decoded — only standard EXIF/TIFF tags are extracted. This covers all fields in the `media_meta` table. Formats with non-TIFF containers (ORF, RW2) have best-effort support; if `kamadak-exif` fails to parse, `width`/`height` are populated via the `image` crate and EXIF fields are NULL.

> **PNG tEXt/iTXt metadata** (e.g., `Author`, `Comment`, `Creation Time`): not extracted in v0.1. Deferred to v0.2.

#### PDF: Scanned / Image-Only Documents

A scanned PDF with no text layer is a permanent extraction failure in v0.1 (no OCR until v0.3):

- `pdf_oxide` extracts zero text bytes from the document.
- **Classification: `status=4` (failed_permanent).** Rationale: the zero-text condition is permanent until OCR is available. Marking as `status=2` (failed_transient) would cause repeated retry attempts at every reconciliation with no benefit.
- `error_msg`: `"[DEFERRED] PDF has no extractable text layer (possibly scanned). Re-index after OCR support is enabled (planned for v0.3)."`.
- The file is still searchable by path, filename, and filesystem metadata.

**Detection heuristic:** A PDF is classified as image-only if `pdf_oxide` extracts fewer than 20 characters from a document with at least 1 page. A legitimate single-page text PDF virtually always exceeds this threshold.

#### PDF: Encrypted / Password-Protected Documents

- Both `pdf_oxide` and the `pdfium` fallback fail on encrypted PDFs without a password.
- **Classification: `status=4` (failed_permanent).**
- `error_msg`: `"[UNSUPPORTED] PDF is encrypted or password-protected. Text extraction requires the password."`.
- Indexed by path and filesystem metadata only; no text content in FTS or vectors.

#### Zero-Byte Files

Files with `size = 0` bytes:

- No extraction is attempted.
- `status=1` (indexed). A zero-byte file is not a failure; it is a valid filesystem entry.
- BLAKE3 is computed (BLAKE3 of an empty input is defined and stored).
- No chunks are produced; no FTS or vector entries are created.
- `doc_meta` / `media_meta` rows are not created.
- The file appears in path-based searches and `ndex info`.

#### Very Small Files (< `min_tokens`)

Files that produce fewer than `min_tokens` (default: 32) tokens after extraction:

- The extracted text is indexed as a **single chunk** regardless of size.
- `min_tokens` is the minimum size for the *splitter* — it does not gate whether a file is indexed. A 5-token file produces one 5-token chunk and is fully indexed.
- `status=1` (indexed).

#### Text Encoding and BOM Handling

ndex assumes UTF-8 for all text content. Non-UTF-8 files are handled with detection-and-transcode:

1. **BOM detection (first pass):** If the file starts with a BOM (UTF-8 `\xEF\xBB\xBF`, UTF-16LE `\xFF\xFE`, UTF-16BE `\xFE\xFF`), the BOM determines the encoding. The BOM bytes are stripped before extraction. UTF-16 content is transcoded to UTF-8 via the `encoding_rs` crate.

2. **Encoding detection (second pass, if no BOM):** If the byte buffer is not valid UTF-8 (`std::str::from_utf8()` fails), ndex uses the `chardetng` crate (Mozilla's encoding detector, same algorithm as Firefox) to guess the encoding. If confidence is sufficient, the content is transcoded to UTF-8 via `encoding_rs`. If detection fails or confidence is too low, lossy UTF-8 conversion is applied (`String::from_utf8_lossy()`), replacing invalid sequences with U+FFFD.

3. **HTML charset override:** For HTML files, if a `<meta charset="...">` or `<meta http-equiv="Content-Type" content="...; charset=...">` tag specifies an encoding, that encoding takes precedence over `chardetng` detection. Parsed by `lol-html` before full extraction.

4. **Logging:** Transcoded files are logged at `DEBUG` level: `"Transcoded <path> from <encoding> to UTF-8"`. Lossy conversions (U+FFFD replacements) are logged at `WARN` level.

**Library:** `chardetng` (Mozilla, same algorithm as Firefox) for detection; `encoding_rs` (Mozilla, same as Firefox/Chromium) for transcoding. Both are pure-Rust, no native dependencies.

#### Jupyter Notebooks

> **Jupyter Notebooks (`.ipynb`):** Detected as `application/json`. The JSON extractor handles them, but results are noisy — base64-encoded cell outputs, execution counts, and cell metadata are indexed alongside code and markdown content. A dedicated `.ipynb` extractor (extracting only code cells and markdown cells, stripping outputs and metadata) is deferred to v0.2.

#### Deferred Formats

| Format | Deferred to | Notes |
|---|---|---|
| EPUB (`.epub`) | v0.2 | ZIP-based; HTML/XML content; requires dedicated extractor for chapter structure |
| EML / MBOX | v0.2 | Planned alongside archive content extraction |
| MSG (Outlook email) | v0.2 | Binary compound document; requires dedicated parser |
| RTF | v0.2 | Common in Windows archives; naive extraction produces noisy text |
| ODS / ODT / ODP (OpenDocument) | v0.2 | ZIP-based like OOXML |
| XLSX text extraction | v0.2 | Metadata-only in v0.1 (see OOXML above) |
| PPTX text extraction | v0.2 | Metadata-only in v0.1 (see OOXML above) |

> **v0.1 behavior for deferred formats:** When a file matches a deferred format (RTF, EPUB, EML, MBOX, MSG, ODS, ODT, ODP), it is classified as `status=4` (failed_permanent) with `error_msg`: `"[DEFERRED] Format '<mime_type>' extraction not yet supported. Planned for <version>."` The file is indexed by path and filesystem metadata only. This differs from `status=5` (skipped) which implies the file was intentionally excluded (e.g., over size limit). `status=4` with `[DEFERRED]` prefix signals that extraction will be available in a future version and the file can be automatically re-processed when that version is installed.

#### Explicitly Out-of-Scope

The following formats produce `status=4` (unsupported format) or archive/binary skip. They are not on any current roadmap:

- PST / OST (Outlook data files) — proprietary compound format
- DWG / DXF (CAD files)
- Proprietary creative-tool formats (`.psd`, `.ai`, `.sketch`, `.fig`) — indexed by path/metadata only
- `.db`, `.sqlite` database files — not content-indexed (binary format; SQL dumps as `.sql` are indexed normally)
- Font files (`.ttf`, `.otf`, `.woff`, `.woff2`, `.eot`) — binary format with no searchable text content; indexed by path/metadata only, `status=5` (skipped)

#### `status=4` Error Message Conventions

To distinguish deferred formats (re-processable in a future version) from permanently unsupported formats, the `error_msg` column uses the following prefixes:

- **Deferred formats:** `error_msg` starts with `"[DEFERRED] Format '<mime>' not yet supported. Planned for <version>."` (e.g., RTF, EPUB, 7Z in v0.1)
- **Out-of-scope formats:** `error_msg` starts with `"[UNSUPPORTED] Format '<mime>' is not supported."` (e.g., PST, DWG, PSD)
- **Scanned PDFs:** `error_msg` starts with `"[DEFERRED] PDF has no extractable text layer (possibly scanned). Re-index after OCR support is enabled (planned for v0.3)."` (since OCR is planned for v0.3)
- **Encrypted PDFs:** `error_msg` starts with `"[UNSUPPORTED] PDF is encrypted or password-protected. Text extraction requires the password."`

This prefix scheme allows `ndex stats` to report deferred vs. unsupported separately, and allows future versions to automatically re-process deferred files when support is added (by scanning for `[DEFERRED]` prefix in `error_msg`).

### 4.9 Archive Safety

Archive decompression and extraction must never crash the indexing process regardless of input. The following limits and safeguards are enforced:

#### Decompression Bomb Protection

| Check | Limit | Behavior when exceeded |
|---|---|---|
| **Compression ratio** | 200:1 per member | Abort member; `status=4`, `error_msg = "[DEFERRED] Compression ratio <N>:1 exceeds limit (possible decompression bomb)"`. Continue to next member. |
| **Total extracted size per archive** | `max_archive_total_size` (default: 8 GiB) | Stop extracting further members. Members extracted so far are kept. Archive `extraction_status = 'partial'`. Log `WARN`. |
| **Per-member size** | `max_file_size` (default: 2 GiB) | Skip member; `status=5` (skipped). |
| **Member count** | `max_archive_members` (default: 100,000) | Stop after N members. Archive `extraction_status = 'partial'`. |

Ratio detection: track `bytes_read` (compressed) and `bytes_written` (decompressed) during streaming decompression. Check ratio every 1 MiB of output.

#### Recursive Archive Depth Limit

- `max_archive_depth` (default: 3, configurable).
- Archives nested deeper than the limit are treated as opaque files: `status=5` (skipped), `error_msg = "Archive nesting depth <N> exceeds limit of <max>"`.
- The depth counter is threaded through the extraction pipeline: regular files have depth 0, members of a top-level archive have depth 1, etc.

#### Malformed Archive Handling

- **Corrupted headers:** If the archive crate fails to open the archive, `status=4` (permanent), `error_msg` captures the library error. No members extracted.
- **Truncated archive (unexpected EOF mid-stream):** All members extracted so far are kept. Archive gets `extraction_status = 'partial'` and `status=2` (transient) for retry — the file may be incomplete due to a concurrent write.
- **Individual member extraction failure:** Isolated to that member. Other members continue normally.
- **Invalid member paths:** Archive members with paths containing `../` (path traversal), absolute paths, or null bytes are skipped: `status=4`, `error_msg = "Unsafe member path"`.

#### Panic Isolation

All archive extraction (including calls into `zip`, `tar`, `flate2`, `bzip2`, `xz2` crates) is wrapped in `std::panic::catch_unwind`. If a third-party crate panics on malformed input, the panic is caught, the archive gets `status=2` (transient), and processing continues with the next file. The panic is logged at `ERROR` level. This does NOT protect against `abort()` or stack overflow in native code.

#### Memory Bounding

- Each member's decompressed content is read into a single `Vec<u8>`, pre-allocated to `min(member_header_size, max_file_size)`.
- Only one member is decompressed in memory at a time (sequential processing). Peak memory per archive: `max_file_size` for the member buffer + streaming archive reader overhead (~64 KiB buffers).
- The archive file itself is read via streaming I/O or mmap — NOT loaded into a second heap buffer.

#### Archive Config Keys (add to §17 config reference)

```toml
[archive]
max_archive_total_size = "8GiB"    # total decompressed bytes per archive
max_archive_members = 100000        # max members to extract per archive
max_archive_depth = 3               # max nesting depth for recursive archives
compression_ratio_limit = 200       # max decompressed:compressed ratio per member
```

#### Archive Logging

- `INFO`: `"Extracting archive <path>: <N> members, <size> total"`
- `WARN`: `"Archive <path> extraction partial: hit member limit (100000)"`, `"Compression ratio exceeded for <archive>!/<member>"`
- `DEBUG`: Per-member extraction: `"Extracted <archive>!/<member> (<size>, <mime>)"`
- `ERROR`: Panic caught during archive extraction, corrupted archive header

---

## 5. No Index Migrations

### 5.1 Policy

**When the index schema version changes, ndex refuses to open the old index and requires a full rebuild.** No migration code, no upgrade scripts, no in-place mutation of index files.

### 5.2 Justification

The index is derived data — always rebuildable from source files. Migration code is the #1 source of subtle corruption bugs, and rebuild cost (hours for 10M files) is acceptable given schema changes are rare.

### 5.3 Implementation

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
vector_scalar = "f16"               # stored embedding precision; model inference uses INT8 ONNX weights
hnsw_m = 32
hnsw_ef_construction = 200

[hashing]
algorithm = "blake3"

[fts]
tokenizer_version = 1               # bumped if tokenizer pipeline changes
```

**Version check on open:** If schema_version doesn't match, refuse to open and print reindex command. If embedding model differs, disable semantic search with a warning.

**`ndex-remote reindex`:** Moves `.ndex/` to `.ndex.old/`, creates a fresh index, rebuilds. If rebuild succeeds, deletes `.ndex.old/`. If it fails, restores `.ndex.old/`. Atomic from the user's perspective.

---

## 6. Stale Index Auto-Refresh on Search

### 6.1 Problem

On append-only archival storage, files accumulate between explicit `ndex index` runs. If a user searches without indexing first, they may miss recently added files. We want search to be automatically up-to-date without requiring a manual `ndex index` step every time.

### 6.2 Design: Opportunistic Reconciliation

When `ndex search` (or any read command) opens the index, the remote checks how stale the index is and optionally runs a fast reconciliation before returning results.

**Staleness heuristic:**

**Staleness thresholds:** If index age < `threshold` (default: 1h) → skip refresh. If age < `warn_threshold` (default: 7d) → run quick reconcile. If age ≥ `warn_threshold` → warn user. Age measured from `last_reconciliation_ns` in manifest.

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

> **Note (HDD users):** The default `timeout_secs = 30` is useless on HDD arrays at large scale — a Phase 1 walk on HDD RAIDZ2 at 10M files takes 3-5 minutes, so the timeout fires before any files are indexed. The 30-second budget still imposes I/O seek cost on HDD with no benefit.
>
> **Rotational media detection:** On Linux, ndex detects rotational storage via `/sys/block/<dev>/queue/rotational`. If the index root is on rotational media and `timeout_secs` is at its default (30), ndex adjusts the effective `auto_refresh.enabled = false` with a one-time warning: "Auto-refresh disabled: rotational storage detected. Run `ndex index` on a schedule for up-to-date results." This avoids the useless I/O overhead. Users can override with `auto_refresh.enabled = true` and a higher `timeout_secs` in config.

**User override flags:**

```bash
ndex search /pool "query" --no-refresh    # skip auto-refresh, search stale index
ndex search /pool "query" --refresh       # force refresh even if fresh
ndex search /pool "query" --refresh-timeout 60  # extend the budget
```

### 6.3 Manifest Schema Addition

The `reconciliation_runs` table is defined in §10.1 (manifest schema). See that section for the full SQL.

---

## 7. Remote Binary: Server Self-Installation

### 7.1 Principle

The client **never** sends the `ndex-remote` binary to the server. The client may be on a bandwidth-constrained link (remote location, metered connection, satellite). Transferring an ~80-100 MB binary (statically-linked build including ONNX Runtime) over that link is unacceptable.

The server installs `ndex-remote` itself, using its own (presumably fast) network connection to download from the release source.

### 7.2 Installation Methods

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

> **Security note:** `curl | sh` is a supply-chain vector. If the release server is compromised, both the binary and its checksum are controlled by the attacker, making checksum verification alone insufficient. Mitigations:
> - The install script is hosted in the ndex GitHub repository (not an opaque CDN) for transparency and auditability.
> - Release artifacts are signed with GPG (or sigstore/cosign). The install script verifies the signature against a hardcoded public key before executing.
> - For security-conscious environments, use Method 3 (manual tarball) and verify the GPG signature out-of-band, or install from a package manager with its own trust chain (Method 2).
> - The sigstore/cosign public key and verification instructions are published at `https://github.com/ndex-dev/ndex/blob/main/SECURITY.md`.

The install script:
1. Detects `uname -m` for architecture (x86_64, aarch64)
2. Detects OS (Linux, macOS, FreeBSD)
3. Downloads the release tarball from GitHub releases
4. **Verifies the GPG/cosign signature** against the hardcoded public key
5. Extracts `ndex-remote` to the install path
6. Does NOT download the embedding model (that happens automatically on first use — §7.4)

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

### 7.3 Self-Update

> **v0.1:** `ndex-remote self-update` is a stub that prints: `"Self-update is planned for v0.2. Update manually via your package manager or: curl -fsSL https://get.ndex.dev/install.sh | sh"`. The full self-update mechanism described below is implemented in v0.2.

`ndex-remote` can update itself when the server admin requests it (v0.2+):

```bash
# On the server:
ndex-remote self-update                    # update to latest
ndex-remote self-update --version 0.1.0    # specific version
ndex-remote self-update --check            # just check, don't install
```

Self-update process:
1. Fetch version manifest from release server
2. Download new binary to a temporary file
3. **Verify GPG/cosign signature** (not just checksum — the checksum is only as trustworthy as the server hosting it)
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

### 7.4 Automatic Model Fetching

Embedding models are **auto-fetched on first use.** When `ndex-remote` needs a model and doesn't have it:

```
$ ndex-remote index /pool/archive

  Embedding model 'snowflake-arctic-embed-m-v2.0' not found.
  Downloading to ~/.ndex/models/snowflake-arctic-embed-m-v2.0/...
  ████████████████████████████░░░░ 250 MB / 297 MB  [3s ETA]

  Model verified (blake3: a3f2e8...). Ready.

  Reconciling...
  ...
```

This happens:
- On the first `ndex-remote index` after installation (auto-fetch during direct indexing on server)
- **`ndex-remote serve` refuses to start if the model is missing.** Pre-download the model before serving: `ndex-remote model fetch arctic`
- Automatically, silently, exactly once per model (during index operations)

**Model management commands:**

```bash
ndex-remote model list                       # show available + downloaded models
ndex-remote model fetch arctic               # pre-download default model
ndex-remote model fetch --all                # download all available models
ndex-remote model delete --all               # remove all models
ndex-remote model verify                     # re-verify downloaded model integrity
ndex-remote model path arctic                # print the path to the model file
```

**Model storage:**

```
~/.ndex/models/
└── snowflake-arctic-embed-m-v2.0/
    ├── model.onnx          (~297 MB, INT8)
    ├── tokenizer.json      (600 KB)
    └── manifest.json       (model metadata, expected hashes)
```

**Available models:**

| Shortname | Full name | Size (ONNX INT8) | Dims | MRL | Languages | BEIR/MIRACL |
|---|---|---|---|---|---|---|
| `arctic` (default) | snowflake-arctic-embed-m-v2.0 | ~297 MB | 768 | yes (256d) | 74 languages | MIRACL 55.2 |

Additional models planned for v0.2.

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

## 8. Cross-Platform Path Handling

### 8.1 The Problem

Filesystem paths are not strings. They're byte sequences on Unix (`[u8]`) and UTF-16 sequences on Windows (`[u16]`). Neither is guaranteed to be valid UTF-8. Real-world archives contain:

- Filenames in legacy encodings (Shift-JIS, GB2312, Latin-1) that are not valid UTF-8
- macOS NFD normalization artifacts (café stored as `cafe\xCC\x81`)
- Windows filenames with characters illegal on Unix (`:`, `*`, `?`, `<`, `>`)
- Filenames with embedded newlines, control characters, or null bytes (except null — that's the one universal prohibition)

### 8.2 Design

Paths are stored as `BLOB` in SQLite (raw bytes, not TEXT — preserves non-UTF-8 faithfully). On the wire: raw bytes in msgpack `bin` type. For display: lossy UTF-8 with `U+FFFD` replacement. JSON output uses `\uXXXX` escapes for non-UTF-8 bytes.

ndex is a Unix tool (Linux, macOS, FreeBSD). Windows is a non-goal for v0.1; this design doesn't preclude it.

---

## 9. Why No ZFS Snapshot-Based Reconciliation

`zfs diff` between ndex-managed snapshots was considered as an O(changes) alternative to a full walk. Rejected due to: snapshot namespace pollution with tools like sanoid, surprising `zfs diff` behavior (reports block-level not file-level changes), elevated privilege requirements, untestable in CI, and the full walk being fast enough:

| Filesystem | Files | Walk time |
|---|---|---|
| NVMe (ext4/ZFS) | 10M | ~30s |
| NVMe (ext4/ZFS) | 50M | ~2.5min |
| HDD RAIDZ2 | 10M | ~3-5min |
| HDD RAIDZ2 | 50M | ~15-20min |

With the auto-refresh heuristic (§6), most searches trigger a walk against a recently-reconciled manifest where only a few thousand new files exist — the diff phase dominates and completes in seconds.

### 9.1 What We Keep from ZFS Awareness

- **Dataset detection:** `ndex init` detects ZFS and stores the dataset name in config. Used for informational purposes (`ndex stats` shows pool/dataset info).
- **ZFS property reading:** `ndex info` can show ZFS-specific metadata (compression ratio, checksum algorithm, recordsize) for context.
- **Integrity note:** `ndex verify` on ZFS reminds the user that `zpool scrub` is the canonical integrity check and that ndex's BLAKE3 verification is defense-in-depth, not a replacement.

---

## 10. Index Catalogue

All indices live under `<root>/.ndex/`. Each is independent, rebuildable, and individually compactable.

### 10.1 Manifest Index — `manifest.db` (SQLite, WAL mode)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;       -- 256 MB page cache
PRAGMA mmap_size = 1073741824;     -- 1 GB mmap

CREATE TABLE files (
    file_id        INTEGER PRIMARY KEY,
    path           BLOB NOT NULL,       -- platform-native bytes (see §8)
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
    error_msg      TEXT,
    hard_link_of   INTEGER REFERENCES files(file_id), -- NULL if not a hard link; canonical file_id if hard link
    parent_archive_id INTEGER REFERENCES files(file_id) -- NULL for regular files; archive's file_id for members
);

CREATE UNIQUE INDEX idx_path ON files(path);  -- prevents duplicate path entries at DB level
CREATE INDEX idx_path_hash ON files(path_hash);
CREATE INDEX idx_status ON files(status) WHERE status NOT IN (1, 3);
CREATE INDEX idx_blake3 ON files(blake3) WHERE blake3 IS NOT NULL;
CREATE INDEX idx_mtime ON files(mtime_ns);
CREATE INDEX idx_mime ON files(mime_type) WHERE mime_type IS NOT NULL;
CREATE INDEX idx_size ON files(size);
CREATE INDEX idx_hard_link ON files(hard_link_of) WHERE hard_link_of IS NOT NULL;
CREATE INDEX idx_parent_archive ON files(parent_archive_id) WHERE parent_archive_id IS NOT NULL;

-- index_progress: presence of a row means "successfully completed for this index",
-- NOT "attempted". A row is inserted only after the index write for this file+index_name
-- has been fully committed. Missing row = not yet indexed (or failed).
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

**`reconciliation_runs` column values:**

- `kind`: `"full"` | `"incremental"` | `"auto_refresh"`
- `method`: `"metadata_diff"` (Phase 1+2 only; status=pending files reprocessed) | `"full_verify"` (re-hash all unchanged files)

**Retention policy:** `reconciliation_runs` grows unboundedly without pruning. ndex automatically deletes rows beyond the last 1000 runs after each successful reconciliation (keeping the most recent 1000 by `run_id`). Users can also manually prune with `DELETE FROM reconciliation_runs WHERE run_id < (SELECT run_id FROM reconciliation_runs ORDER BY run_id DESC LIMIT 1 OFFSET 999);`.

**`schema_info` initial rows:**

Rows inserted at `ndex init`: `schema_version` (value: current schema integer as TEXT), `created_at` (ISO 8601 timestamp). Row updated after each completed reconciliation: `last_reconciliation_ns` (unix nanoseconds of last non-timed-out `completed_ns`; absent/NULL if never reconciled). This is denormalized from `reconciliation_runs` for O(1) staleness checks in §6.2.

**Scale notes:**

- `path` as BLOB: preserves non-UTF-8 paths faithfully (§8).
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

### 10.2 Full-Text Content Index — `content/` (Tantivy)

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

CJK tokenization deferred to v0.2. CJK text uses default Unicode word tokenizer in v0.1.

**Snippet generation:** Search result snippets are generated via Tantivy's `SnippetGenerator` using the stored `body` field. The snippet generator highlights matched terms and returns at most `--context` sentence-boundary-aligned fragments. The client receives the raw snippet text; term highlighting markers `\x1b[1;33m...\x1b[0m` are applied by the client renderer.

**Language detection:** Language detection uses the `whatlang` crate (pure-Rust, 70 languages). `whatlang` supports 70 languages, closely matching the model's 74-language support, and provides confidence scoring. Detection runs on the full extracted text at the document level. The detected ISO 639-1 code is stored in `doc_meta.lang` and propagated to all chunks as the `lang` FTS field. If the text is shorter than 20 characters or detection confidence is below the `whatlang` threshold, `lang` is set to NULL.

> **Crate choice:** `whatlang` (not `whichlang`) is used. `whichlang` supports only 16 languages, which is insufficient for the model's 74-language coverage and would produce incorrect language labels for most non-English content.

**Unicode normalization:** All indexed text and search queries are NFC-normalized before tokenization (`unicode-normalization` crate). This ensures `café` (NFD: `cafe\u0301`) and `café` (NFC: `caf\u00e9`) match identically. macOS writes NFD paths/content; Linux writes NFC; normalization to NFC at ingest time prevents missed matches across platforms.

**Commit strategy:** Tantivy `IndexWriter::commit()` is called periodically during indexing to flush segments to disk and make documents visible to readers. ndex commits every **10,000 documents** or every **30 seconds**, whichever comes first. A final commit is issued at the end of each Phase 3 batch. This balances write throughput (fewer commits = less overhead) against crash recovery granularity (more frequent commits = fewer documents to re-index after a crash). The commit interval is not currently user-configurable.

**Directory entries in Tantivy:** Directories get a Tantivy document with `path_text` populated (tokenized by the path tokenizer — split on `/` and `.`, trigrams per component) and empty `body`/`title`. This enables path searches like `--path 'src/components/*'` and FTS queries matching directory names. Directories do NOT receive vector embeddings (no text content to embed). They do not appear in semantic search results.

**Threading:** Tantivy's `IndexWriter` uses a single writer with internal per-thread document buffers. Multiple extraction workers prepare documents and call `add_document()` concurrently — Tantivy handles the internal synchronization. Only one `IndexWriter` instance should exist per index. Search readers (`IndexReader`) are fully concurrent and lock-free.

### 10.3 Semantic Vector Index — `vectors/` (USearch)

| Parameter | Value |
|---|---|
| Dimensions | 256 (MRL truncation of 768d snowflake-arctic-embed-m-v2.0) |
| Metric | Inner product (on L2-normalized vectors = cosine) |
| Scalar type | f16 |
| Storage per vector (raw) | 512 bytes (256d × 2 bytes) |
| HNSW graph overhead (M=32) | ~256 bytes/node |
| **Total per vector** | **~768 bytes** |
| Connectivity M | 32 |
| ef_construction | 200 |
| ef_search | 128 (tunable via config.toml) |

**Sidecar** — `vectors/sidecar.bin`:

The sidecar binary file has a 128-byte header (magic `NDEXVEC\0`, version, entry count, model name, dimensions) followed by fixed-size entries (24 bytes each: USearch label, file_id, chunk_ord).

**Why USearch:** USearch is the only actively maintained Rust-compatible ANN library with native f16 SIMD support, mmap-based serving (`view()` API), concurrent lock-free reads, and filter predicates. Alternatives (hnswlib Rust bindings, hora, annoy-rs) are abandoned since 2021 and lack required features.

**Crash safety:** USearch `save()` is not atomic. ndex uses save-to-temp-then-rename:
```rust
index.save("vectors/index.usearch.tmp")?;
std::fs::rename("vectors/index.usearch.tmp", "vectors/index.usearch")?;
```
`rename()` is atomic on POSIX filesystems (including ZFS). If the process crashes mid-save, only the `.tmp` file is corrupted; the previous `index.usearch` remains valid. On startup, stale `.tmp` files are deleted.

The sidecar (`sidecar.bin`) uses the same save-to-temp-then-rename pattern.

**Save ordering (atomicity gap):** The sidecar and USearch index must be saved in a specific order to ensure crash safety:
1. **Save the sidecar FIRST**, then save the USearch index.
2. Rationale: a sidecar-ahead state (sidecar has more entries than USearch's `size()`) is harmless — extra sidecar entries that have no corresponding USearch vector are simply ignored on lookup. The reverse (USearch ahead of sidecar) would cause vector lookups to return file_id=0 / missing entries, which is a data integrity issue.

**Startup validation:** On startup, ndex validates that the sidecar entry count matches USearch's `size()`. If they are mismatched (USearch count ≠ sidecar count), ndex logs a warning and triggers vector repair:
```
Warning: USearch index size (42,317) does not match sidecar count (42,298).
         Vector index may be partially corrupted. Run: ndex reindex --vectors
```
Auto-repair (truncating the sidecar to match USearch's count) is applied if the discrepancy is small (≤ 100 entries) and the sidecar has more entries than USearch (sidecar-ahead case). Otherwise, a full `ndex reindex --vectors` is required. See §11.2 for crash recovery details.

**Threading:** USearch `view()`-based readers are lock-free and `Send + Sync` (concurrent HNSW traversal over mmap'd data). Writes go through a single `Index` instance with per-node bit-level locks — thread-safe for concurrent `add()` calls, but ndex uses a single writer thread for simplicity and to coordinate with the sidecar append.

### 10.4 Metadata Index — `meta.db` (SQLite)

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
-- NOTE: These tables exist for forward-compatibility but are EMPTY in v0.1.
-- ndex tag (tagging command) is v0.2. In v0.1, tags/file_tags are created but never populated.
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

-- archive_meta: summary metadata for archive files
CREATE TABLE archive_meta (
    file_id           INTEGER PRIMARY KEY,
    member_count      INTEGER,
    total_size        INTEGER,    -- total uncompressed size in bytes
    format            TEXT,       -- 'zip', 'tar', 'tar.gz', 'tar.bz2', 'tar.xz', 'gz', 'bz2', 'xz', '7z', 'rar'
    extraction_status TEXT        -- 'complete', 'partial' (hit member/size limit), 'metadata_only' (unsupported format)
);

-- NER entities table: deferred to v0.2. Schema will be defined when NER is implemented.
-- Adding it will require a schema version bump and reindex (per §5 no-migration policy).
```

> **doc_meta population:**
> - **PDF:** `pdf_oxide` extracts the PDF Info dictionary (Title, Author, Subject, Creator, Producer, CreationDate, ModDate). Page count is derived from the page tree. If `pdf_oxide` fails and `pdfium` fallback is used, `pdfium` provides the same metadata via its document info API.
> - **DOCX:** `docx-rust` reads `docProps/core.xml` (Dublin Core: title, creator, subject, language, created/modified dates) and `docProps/app.xml` (page count, word count, application name). If properties XML is missing or malformed, fields are NULL.
> - **HTML:** `<title>` → `title`, `<meta name="author">` → `author`, `<meta name="description">` → `subject`. Other fields NULL.
> - **Markdown:** `title` is set to the first `# heading` if present. Other fields NULL (or populated from frontmatter in v0.2).
> - **All other formats:** `doc_meta` row is not created for plaintext, code, CSV, JSON, SQL, log, config formats. These have no structured document metadata.
> - **Archive members:** Members inherit filesystem metadata from the parent archive file (`mtime_ns`, `ctime_ns`, `uid`, `gid`, `dev`). The member's own `mtime_ns` from the archive header overrides if available. `inode` is NULL for archive members. `size` is the uncompressed member size. `hard_link_of` is NULL for archive members. `blake3` is computed from the decompressed member content.

### 10.5 Thumbnail Store — `thumbs/`

Deferred to v0.2.

### 10.6 On-Disk Layout

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
└── meta.db             (SQLite: doc/media metadata, tags)
```

**Index overhead: ~0.5% of archive size** for typical mixed-content archives. Higher for text-heavy (all PDFs: ~1-2%). Lower for media-heavy (all photos: ~0.1%).

### 10.7 Search Scoring

#### BM25 (Full-Text Search via Tantivy)

Tantivy uses **BM25** (Best Match 25) for FTS scoring. Parameters: `k1 = 1.2`, `b = 0.75` (Tantivy defaults). BM25 scores reflect term frequency, inverse document frequency, and field length normalization.

**Field boosts** apply at query time:
```
score_fts = bm25(title) × title_boost + bm25(body) + bm25(path_text) × 0.5
```
Default `title_boost = 2.0`. Configurable via `search.title_boost` in config.toml. The `path_text` boost of 0.5 ensures path matches contribute but don't overwhelm content matches.

#### Cosine Similarity (Semantic Search via USearch)

USearch stores L2-normalized f16 vectors and uses **inner product** as the distance metric. For normalized vectors, inner product equals cosine similarity. Scores are in `[-1.0, 1.0]`; in practice, positive matches range from ~0.6 to 0.99.

#### Reciprocal Rank Fusion (Hybrid Mode)

Hybrid mode merges FTS and semantic results using **Reciprocal Rank Fusion (RRF)**:

```
rrf_score(d) = 1/(k + rank_fts(d)) + 1/(k + rank_semantic(d))
```

Where:
- `k` is the RRF constant (default: 60, configurable via config.toml). Higher k de-emphasizes rank differences between top and bottom results. `k=60` is the empirically established default from the original RRF paper.
- `rank_fts(d)` is the 1-based rank of document `d` in the FTS result list (∞ if absent)
- `rank_semantic(d)` is the 1-based rank in the semantic result list (∞ if absent)

A document only in FTS results gets `1/(k + rank) + 1/(k + ∞) = 1/(k + rank)`. A document in both gets a combined score.

**`fts_weight` in hybrid mode:** The `search.fts_weight` config key applies a multiplier to the FTS component of the RRF sum:
```
rrf_score(d) = fts_weight × 1/(k + rank_fts(d)) + 1/(k + rank_semantic(d))
```
Default `fts_weight = 1.0`. Set `search.fts_weight = 2.0` in config.toml to weight FTS results more heavily (useful for keyword-heavy queries).

> **Config key clarification:** `search.title_boost` controls the BM25 field weight for the title field within FTS scoring. `search.fts_weight` controls the RRF component multiplier for FTS results in hybrid mode. These are distinct tuning knobs that were previously both referred to as `fts_boost` — the split disambiguates their purpose.

#### `auto` Mode Heuristics

The server determines the actual search mode based on query characteristics:

| Query characteristic | Selected mode |
|---|---|
| Contains quoted phrases (`"exact phrase"`) | `fts` |
| Uses FTS operators (`AND`, `OR`, `NOT`, `field:term`) | `fts` |
| Short (≤ 3 tokens) AND no stop words (the, a, an, is, are, was, were, in, on, at, for, to, of, with, by, from, how, what, why, when, where, which, who) | `fts` |
| Longer natural language (> 3 tokens, no operators) | `hybrid` |
| Vector index absent or empty | `fts` (fallback with warning) |

The selected mode is reported in `SearchResult.mode` and displayed in the result footer.

> **Stop-word routing rationale:** The presence of a stop word in a short query signals natural language intent (e.g., "what is X"), routing to `hybrid`. Pure keyword queries (e.g., "blake3", "config.toml", "ERR_TIMEOUT") route to `fts`. Queries longer than 3 tokens always route to `hybrid` regardless of stop words.

#### Score Normalization for Display

Display scores are min-max normalized to [0,1] within the returned result set. JSON includes raw scores as `score_raw`. `--explain` shows per-component breakdown (FTS BM25, semantic cosine, RRF).

---

## 11. Reconciliation Engine

### 11.1 Three-Phase Design

**Ignore file behavior:**

Ignore hierarchy (evaluated in order, first match wins):
1. `.gitignore` files — respected by default (via `ignore` crate's native support). Follows standard `.gitignore` semantics: per-directory, parent directories consulted, root `.gitignore` at archive root.
2. `.ndexignore` files — same gitignore-compatible syntax, same per-directory hierarchy. Takes precedence over `.gitignore` (can un-ignore files that `.gitignore` excludes via `!pattern`).
3. `--exclude` CLI flags — additive on top of both ignore files.

Rationale: Archives often contain `.gitignore` files from checked-out repos. Respecting them avoids indexing `node_modules/`, `target/`, `.venv/`, build artifacts, etc. — which is almost always the desired behavior. (see config reference in §17)

> **Sensitive file patterns:** ndex does not have a built-in sensitive-file exclusion list. Files like `.env`, `credentials.json`, `id_rsa`, `*.pem` are indexed if not excluded by `.gitignore` or `.ndexignore`. Users managing archives with sensitive files should add patterns to `.ndexignore`:
> ```
> # .ndexignore
> .env*
> *.pem
> *.key
> **/credentials.json
> **/secrets/**
> ```
> ndex logs a `WARN` at the end of indexing if any indexed files match a hardcoded sensitive-file heuristic (filenames containing `secret`, `credential`, `password`, `private_key`, or extensions `.pem`, `.key`, `.p12`, `.pfx`): `"Warning: 3 potentially sensitive files were indexed. Review with: ndex search /pool --path '*.pem' --path '*.key'"`. This is advisory only — the files are still indexed.

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

> **Directory entries:** In addition to regular files, Phase 1 also collects directory entries. The `ignore` crate's `WalkBuilder` yields `DirEntry` for both files and directories. Directory entries are collected into a separate `DashMap<PathBuf, DirWalkEntry>` where `DirWalkEntry = { mtime_ns, ctime_ns, inode, dev, mode }` (no `size` — directory size is meaningless on most filesystems). Directory entries are added to the manifest with `mime_type = 'inode/directory'`, `status = 1` (indexed), `blake3 = NULL`, `size = 0`. No chunks, no FTS `body`/`title`, no vectors, no `doc_meta`/`media_meta`. Directories participate in Phase 2 diff (new → insert, deleted → `status=3`, mtime changed → update `mtime_ns`). No Phase 3 processing needed for directories.

> **Non-regular files:** The Phase 1 walker skips non-regular files: FIFOs (named pipes), Unix domain sockets, block/character device files, and broken symlinks (symlinks whose target does not exist). These are silently excluded — no manifest entry is created, no warning is logged (these are expected filesystem entries, not errors). Only regular files, directories, and followed symlinks to regular files are indexed. The `ignore` crate's `WalkBuilder` handles this via `file_type()` filtering.

> **Memory requirements:** Phase 1 walk builds a `DashMap` of all file metadata (~270 bytes/file). Phase 2 diff loads the manifest into a second `HashMap` (~200 bytes/file). Total: ~470 bytes/file across both phases simultaneously. At 10M files: ~5 GB RAM. At 50M files: ~25 GB RAM. Systems indexing 50M+ files should have at least 32 GB RAM available for ndex-remote.
>
> **Known v0.1 limitation:** The Phase 2 manifest HashMap is loaded entirely into memory. Systems indexing **>10M files should have 16+ GB RAM available** for `ndex-remote`. This is a documented limitation for v0.1. The optimization path — cursor-based SQLite batching during Phase 2 diff to reduce peak memory — is planned for **v0.2** (not v0.3, moved forward due to practical impact). Consider querying the manifest via SQLite during diff rather than loading all into a HashMap; this is the v0.2 implementation target.

> **Pre-flight disk space check:** Before starting Phase 1, ndex estimates the required index storage and warns if available disk space is insufficient. Estimate: ~0.5% of the total size of files to be indexed (for typical mixed-content archives). If available disk space on the `.ndex/` filesystem is below this estimate, ndex emits a warning:
> ```
> Warning: Estimated index size ~4.5 GB (0.5% of 900 GB archive).
>          Only 2.1 GB free on /pool/archive. Index may fail mid-run.
>          Free up space or use a different filesystem for .ndex/ (v0.2: --index-dir).
> ```
> This is a warning, not an abort — the estimate is imprecise and the user may have already allocated space. The check uses `statvfs()` on Linux/macOS.

> **Hard limit:** `ndex-remote` checks available system memory before Phase 1. If estimated memory for the reconciliation (file_count_estimate × 500 bytes, accounting for walk map + manifest HashMap) exceeds 75% of available RAM, it aborts with a clear error:
> ```
> Error: Estimated 25.5 GB RAM needed for 51M files, but only 7.8 GB available.
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

> **Hard link dedup (MEDIUM):** Multiple paths may resolve to the same inode (`dev + inode` pair). During Phase 2 diff, track `(dev, inode)` pairs already queued. If a new path maps to an already-queued inode, insert it in the manifest with its own `file_id` but mark it with the canonical `file_id` in a `hard_link_of` column (NULL if not a hard link). During Phase 3, skip re-extraction for hard link paths — insert duplicate USearch vectors pointing to the new `file_id`. Both paths are independently searchable and appear as separate results. This avoids the complexity of inode-sharing in the vector index at the cost of duplicated vectors.
>
> **Search dedup (v0.1):** When search results contain multiple hard-linked paths to the same content (same `blake3` hash), all paths appear in results. No dedup is applied — hard links are semantically distinct filesystem entries and the user may want to find all locations. `ndex dedup` (v0.2) is the tool for identifying and acting on duplicates.

> **TOCTOU guard (MEDIUM):** After extracting a file, re-stat it and compare `(size, mtime_ns)` against the values captured during Phase 1 walk. If they differ (file was modified between walk and extraction), discard the extraction result and mark the file `status=2` (failed_transient) for retry on the next run. This prevents stale content from being indexed.
>
> **ENOENT between Phase 1 and Phase 3:** If `std::fs::read()` returns `ENOENT` for a file that was present during the Phase 1 walk, this means the file was deleted between the walk and extraction. This should be classified as `status=3` (deleted), **not** `status=2` (failed_transient). Rationale: the walk proved the file existed at walk time; `ENOENT` during extraction means it was subsequently removed. A transient classification would cause repeated retry attempts on a file that is gone. Set `status=3` directly and log at `INFO` level: "File deleted between walk and extraction: <path>". Other I/O errors during extraction remain `status=2`.

Multi-threaded pipeline with backpressure:

- N extraction workers (rayon pool, N = num_cpus)
- Each reads file, computes BLAKE3 as streaming side-effect, extracts text, chunks
- Chunks fed to bounded crossbeam channel (cap 4096)
- Tantivy writer (internal thread pool) consumes chunks for FTS
- Embedding thread batches chunks → ONNX inference → USearch writer
- SQLite writer serialized through single-writer channel

> **Extraction worker memory note:** With N workers (default: `num_cpus`) each potentially reading files up to `max_file_size` (default: 2 GiB) into memory simultaneously, peak memory during Phase 3 can reach N × 2 GiB for a worst-case workload of all-large-files. On a 16-core machine, this is theoretically 32 GiB just for file buffers. In practice, most files are small and the bounded channel (cap 4096) limits pipeline depth. For installations with many large files, consider reducing `--jobs` (e.g., `--jobs 4`) or lowering `max_file_size`. A semaphore limiting concurrent large-file reads (files > 100 MiB) is a future optimization.

### 11.2 Crash Safety

Two-phase commit per file:

1. Manifest insert with `status = 0` (intent)
2. Index writes (FTS, vectors, metadata)
3. `index_progress` rows per completed index
4. Manifest update to `status = 1` only after all progress rows exist

Crash recovery: resume from `status = 0` files, re-process missing indices per `index_progress`.

**USearch/sidecar crash recovery:** If a crash occurs between saving the sidecar and saving the USearch index, the sidecar will have more entries than USearch (`size()`). On the next startup, ndex detects this mismatch and applies the appropriate repair (see §10.3 for the save ordering specification and mismatch handling). The sidecar-ahead state is the safe failure mode and is auto-repairable; USearch-ahead (caused only by sidecar save failure after USearch was saved) is not expected given the mandated save ordering.

**Cross-database write ordering (manifest.db ↔ meta.db):** When writing a fully-processed file, the write ordering is:
1. Write to `meta.db` (`doc_meta` or `media_meta` row)
2. Write `index_progress` rows to `manifest.db`
3. Update `files.status = 1` in `manifest.db`

This ordering ensures that on crash recovery, a file with `status=0` (intent-written) or a missing `index_progress` row will be re-processed, overwriting any partial `meta.db` entry. A `meta.db` row without a corresponding `index_progress` row is stale-overwritten on retry — this is safe because metadata extraction is deterministic. The reverse ordering (manifest first, then meta.db) could leave `status=1` files without metadata, which would be harder to detect.

### 11.3 Concurrency

`flock()` on `.ndex/lock` for write exclusion. Multiple readers (search sessions) run concurrently — SQLite WAL, tantivy readers, and USearch mmap reads all support concurrent access. Readers never block on a writer (WAL mode — readers see the last committed state).

**Single flock, both databases:** The single `.ndex/lock` flock guards write access across **both** SQLite databases (`manifest.db` and `meta.db`). Both databases are opened by the same `ndex-remote` process that holds the lock. There is no per-database locking — the flock is process-level, and the lock-holding process serializes all writes to both databases. This is correct because only one `ndex-remote` instance can hold the write lock at a time.

Search sessions always see the last committed state (WAL isolation). Results are never partial or corrupted, but may miss files currently being indexed in an ongoing `ndex index` run.

> **NFS warning:** `flock()` on NFS can silently fail to provide mutual exclusion (depending on NFS version and server configuration) or block indefinitely. At index open time, ndex detects whether `.ndex/` is on an NFS mount via `statfs()`. If NFS is detected, ndex aborts with:
> ```
> Error: .ndex/ is on an NFS filesystem. flock() cannot guarantee exclusive access on NFS.
> The .ndex/ directory must reside on a local filesystem (ZFS, ext4, xfs, etc.).
>
> Workarounds for v0.1 (--index-dir flag is planned for v0.2):
>   1. Run ndex-remote directly on the NFS server (where the filesystem appears local).
>   2. Place the archive on a locally-attached filesystem instead of NFS.
>   3. Use a bind-mount or symlink to redirect .ndex/ to a local path manually:
>        mkdir -p /local/fast/storage/ndex-index
>        ln -s /local/fast/storage/ndex-index /pool/archive/.ndex
> ```
> (The `--index-dir` flag for relocating `.ndex/` is planned for v0.2. The v0.1 error message must not reference `--index-dir` as it does not exist yet.)

### 11.4 Symlink Handling

**Policy:** ndex follows symlinks by default, matching `find -L` behavior. Symlink cycles are detected by tracking `(dev, inode)` pairs during the walk; a cycle is logged as a warning and the symlink target is skipped.

Symlinks pointing outside the index root are **not followed** — the index only covers files under the root path. This prevents a symlink at `/pool/archive/link → /etc` from pulling in system files. (see config reference in §17)

Set `follow_symlinks = false` in config to index only regular files and skip all symlinks.

### 11.5 Error Handling Strategy

#### Failure Classification

Each file processing failure is classified as transient or permanent:

| Failure type | Classification | `status` | Retry? |
|---|---|---|---|
| I/O error (read timeout, disk error) | Transient | 2 | Yes |
| Extraction error (malformed PDF, truncated DOCX) | Transient (first 3 attempts) | 2 | Yes (up to limit) |
| Extraction error (persistent after retry limit) | Permanent | 4 | No |
| File too large (> `max_file_size`) | Permanent/skipped | 5 | No |
| Unsupported format | Permanent | 4 | No |
| Embedding failure (ONNX error) | Critical — stops processing | — | See below |
| Model load failure | Critical — stops processing | — | See below |
| Disk full during index write | Critical — stops processing | — | See below |
| Index corruption detected | Critical — requires reindex | — | See below |

#### Retry Policy

Files with `status = 2` (failed_transient) are retried on the next reconciliation run. After `fail_count` reaches the configured limit, the status is promoted to `status = 4` (failed_permanent):

```toml
[extraction]
max_retries = 3    # promote to failed_permanent after this many transient failures
```

Each retry increments `fail_count` in the manifest. The `error_msg` column stores the last error for diagnostics.

#### Critical Error Handling

Critical errors stop the indexing run immediately and are reported to the user:

- **Model load failure:** ndex-remote exits with a clear message to re-fetch the model (`ndex-remote model fetch --force`). Does not corrupt the index.
- **Disk full:** ndex-remote aborts after flushing SQLite WAL. Files in `status=0` (intent-written) will be retried on next run (crash recovery per §11.2).
- **Index corruption:** Detected via SQLite integrity check and USearch header validation on open. If corruption is detected, ndex refuses to proceed and prompts for `ndex reindex`.
- **ONNX inference error:** A single chunk failing inference is logged and skipped (that chunk gets no vector). If inference errors are persistent (> 10 consecutive failures), the embedding pipeline is halted and `--no-vectors` behavior applies for the remainder of the run.

#### Logging

All errors are logged to stderr. Per-file errors at `WARN` level. Critical errors at `ERROR` level. The `fail_count` and `error_msg` in the manifest provide persistent per-file diagnostics.

---

## 12. IPC Protocol

### 12.1 Why MessagePack

MessagePack via rmp-serde. Compact binary, no codegen, forward-compatible via `#[serde(default)]`.

### 12.2 Wire Protocol

Length-prefixed frames:

```
┌─────────────┬────────────────────────────┐
│ length: u32 │ payload: [u8; length]       │
│ (big-endian)│ (msgpack-encoded Message)   │
└─────────────┴────────────────────────────┘
Max frame: 16 MiB

**Frame overflow handling:** If a received frame's length prefix exceeds 16 MiB, the receiver closes the connection immediately with an `Error` message (if possible) containing: `"Frame size <N> exceeds 16 MiB limit."` The receiver does NOT attempt to read the oversized payload. The sender is responsible for ensuring payloads fit within the limit. In practice, the only messages approaching this limit are `SearchResult` with many hits, which are paginated by `limit` (default 20). If a single `SearchHit` somehow exceeds 16 MiB (pathological snippet), the server truncates the snippet before framing.
```

**Magic preamble (SSH stdout contamination mitigation):**

Shell login scripts (`.bashrc`, `.zshrc`, `/etc/profile`) may write garbage to stdout before `ndex-remote serve` gets to write anything — SSH does not suppress this by default. To prevent this from corrupting the msgpack framing, `ndex-remote serve` writes a magic preamble as the very first bytes on stdout:

```
NDEX\x00\x01
```

(6 bytes: ASCII "NDEX" + null byte + version byte 0x01)

The client scans for this preamble after spawning the remote, discarding up to **4096 bytes** of preceding garbage. If the preamble is not found within the first 4096 bytes, the client fails with a clear error:

```
Error: Failed to establish protocol session with 'nas.local'.
Shell startup scripts may be writing to stdout, contaminating the msgpack channel.
Fix: Ensure your shell startup files (~/.bashrc, ~/.profile, /etc/profile) do not
     write to stdout when running non-interactively.
     Check: ssh -T nas.local "ndex-remote serve --root /pool" 2>/dev/null | xxd | head
```

**Recommended invocation:** `ndex-remote serve` should be invoked as a non-login, non-interactive shell to minimize contamination risk:
- Use SSH `-T` flag (disables pseudo-tty allocation, typically suppresses MOTD/login banners)
- For dedicated ndex access, configure `command="ndex-remote serve --root /path"` in `~/.ssh/authorized_keys` on the server — this prevents the interactive shell from running at all

**Troubleshooting:** If the preamble error occurs, diagnose with:
```bash
ssh -T nas "ndex-remote serve --root /pool/archive" 2>/dev/null | xxd | head -4
# Should start with: 4e 44 45 58 00 01 (NDEX\x00\x01)
# If garbage appears before it, check ~/.bashrc, ~/.profile, /etc/bash_profile
```

### 12.3 Version Negotiation

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

### 12.4 Message Types

> **Serialization:** Always use `rmp_serde::to_vec_named()` / `rmp_serde::from_slice()`. The `_named` variant serializes field names as strings (required for correct enum deserialization). **Externally tagged enums (serde default, no attributes) are used.** Internally tagged (`#[serde(tag = "kind")]`) and adjacently tagged (`#[serde(tag = "kind", content = "data")]`) enums have known deserialization bugs in `rmp-serde` (issues #153, #250) and must not be used. Wire format: `{"SearchRequest": {"query": "...", "mode": "Auto", ...}}`. **Write comprehensive round-trip tests for every message variant before considering the protocol stable.**

```rust
#[derive(Serialize, Deserialize)]
enum ClientMessage {
    Handshake(HandshakeReq),
    SearchRequest(SearchRequestData),
    IndexRequest(IndexOptions),
    InfoRequest(InfoRequestData),
    StatsRequest,
    VerifyRequest(VerifyRequestData),
    ReindexRequest(ReindexRequestData),
    DeleteRequest(DeleteRequestData),
    CancelRequest,
}

struct SearchRequestData {
    query: String,
    mode: SearchMode,          // Auto, Fts, Semantic, Hybrid
    filters: SearchFilters,
    limit: u32,
    offset: u32,
    format: OutputFormat,
    explain: bool,
}

struct InfoRequestData {
    path: Vec<u8>,
}

struct VerifyRequestData {
    paths: Option<Vec<Vec<u8>>>,
    sample: Option<f64>,
}

struct ReindexRequestData {
    target: ReindexTarget,     // All, Vectors, Fts
}
// Note: ReindexRequest response is IndexComplete (reused). There is no separate ReindexResult
// in ServerMessage. Clients handle reindex completion the same as index completion.

struct DeleteRequestData {
    glob: String,
    dry_run: bool,
}

#[derive(Serialize, Deserialize)]
enum ServerMessage {
    Handshake(HandshakeResp),
    SearchResult(SearchResultData),
    IndexComplete(IndexCompleteData),
    InfoResult(InfoResultData),
    StatsResult(StatsResultData),
    VerifyResult(VerifyResultData),
    DeleteResult(DeleteResultData),
    Progress(ProgressEvent),
    Error(ErrorData),
}

struct SearchResultData {
    hits: Vec<SearchHit>,
    total: u64,
    mode: SearchMode,
    duration_ms: u64,
    truncated: bool,
    stale_warning: Option<String>,
}

struct IndexCompleteData {
    stats: IndexStats,
    cancelled: bool,    // true if the operation was stopped by CancelRequest
}

struct InfoResultData {
    file_info: FileInfo,
}

struct StatsResultData {
    index_stats: IndexSummary,
}

struct VerifyResultData {
    checked: u64,
    corrupted: Vec<CorruptedFile>,
}

struct DeleteResultData {
    deleted: u64,
    paths: Vec<Vec<u8>>,
}

struct ErrorData {
    code: u32,
    message: String,
}
```

### 12.5 Keepalive / Heartbeat

Long indexing runs (hours) are vulnerable to silent SSH disconnection due to idle TCP timeout or firewall state expiration. ndex does not implement an application-level heartbeat in v0.1. Mitigation:

**Recommended SSH client settings** (add to `~/.ssh/config` or pass via `--ssh-option`):
```
ServerAliveInterval 30
ServerAliveCountMax 3
```
This causes SSH to send a keepalive every 30 seconds and disconnect after 3 consecutive non-responses (~90 seconds of silence). This keeps NAT/firewall state alive and detects broken connections within ~90 seconds rather than hanging indefinitely.

ndex passes these as defaults via its SSH invocation unless the user overrides them. Application-level heartbeat (`Progress` pings during idle periods) is deferred to v0.2.

### 12.6 Remote Discovery

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

> **SSH host key deadlock prevention:** ndex always passes `-o BatchMode=yes` in its SSH invocation. This causes SSH to fail immediately (exit 255) instead of prompting for host key acceptance — which would deadlock because stdin/stdout are occupied by the msgpack channel. If the host key is not yet trusted, ndex fails with a clear error:
> ```
> Error: SSH host key verification failed for 'nas.local'.
> Accept the host key first: ssh -o BatchMode=no nas.local exit
> ```
> Users must accept the host key once via a direct SSH session before using ndex.

### 12.7 Payload Type Definitions

Complete definitions for all types referenced in §12.4 `ClientMessage`/`ServerMessage`:

```rust
struct SearchHit {
    file_id: u64,
    chunk_ord: u32,
    path: Vec<u8>,          // raw bytes per §8
    score: f32,             // normalized [0,1] for display
    score_raw: f32,         // raw BM25 / cosine / RRF score
    score_fts: Option<f32>, // BM25 component (with --explain)
    score_vec: Option<f32>, // cosine component (with --explain)
    mime: String,
    size: u64,
    mtime_ns: i64,
    tags: Vec<String>,
    snippet: Option<String>, // highlighted HTML-escaped text (from Tantivy SnippetGenerator)
    byte_start: u64,
    byte_end: u64,
}

struct SearchFilters {
    mime: Option<String>,        // glob, e.g. "image/*"
    after_ns: Option<i64>,
    before_ns: Option<i64>,
    larger: Option<u64>,
    smaller: Option<u64>,
    path_glob: Option<String>,
    tags: Vec<String>,           // OR semantics
    lang: Option<String>,        // ISO 639-1
}

struct IndexOptions {
    full: bool,
    verify: bool,
    dry_run: bool,
    jobs: Option<u32>,
    batch_size: Option<u32>,
    no_vectors: bool,
    enable_ner: bool,
    max_file_size: Option<u64>,
    only_new: bool,
}

struct FileInfo {
    file_id: u64,
    path: Vec<u8>,
    size: u64,
    mtime_ns: i64,
    ctime_ns: i64,
    mime: Option<String>,
    blake3: Option<Vec<u8>>,    // 32 bytes
    status: u8,
    fail_count: u32,
    error_msg: Option<String>,
    tags: Vec<String>,
    doc_meta: Option<DocMeta>,
    media_meta: Option<MediaMeta>,
    chunk_count: u32,
    in_fts: bool,
    in_vectors: bool,
}

struct DocMeta {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    creator: Option<String>,
    producer: Option<String>,
    created_at: Option<String>,
    modified_at: Option<String>,
    page_count: Option<u32>,
    word_count: Option<u32>,
    lang: Option<String>,
}

struct MediaMeta {
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    codec: Option<String>,
    bitrate: Option<u32>,
    fps: Option<f32>,
    camera_make: Option<String>,
    camera_model: Option<String>,
    iso: Option<u32>,
    focal_length: Option<f32>,
    aperture: Option<f32>,
    shutter_speed: Option<String>,
    gps_lat: Option<f64>,
    gps_lon: Option<f64>,
    gps_alt: Option<f64>,
    taken_at: Option<String>,
}

struct IndexStats {
    new: u64,
    modified: u64,
    deleted: u64,
    unchanged: u64,
    processed: u64,
    failed: u64,
    skipped: u64,
    duration_ms: u64,
    timed_out: bool,
}

struct IndexSummary {
    total_files: u64,
    directories: u64,       // directory entries (counted in total_files, not in processed)
    indexed: u64,
    pending: u64,
    failed_transient: u64,
    failed_permanent: u64,
    skipped: u64,
    deleted: u64,
    manifest_bytes: u64,
    fts_bytes: u64,
    vector_bytes: u64,
    meta_bytes: u64,
    last_reconciled_ns: Option<i64>,
    schema_version: u32,
    model_name: String,
}

struct CorruptedFile {
    file_id: u64,
    path: Vec<u8>,
    stored_hash: Vec<u8>,
    actual_hash: Vec<u8>,
}

struct TerminalCaps {
    width: u16,
    height: u16,
    color: bool,
    hyperlinks: bool,
    unicode: bool,
}

enum ReindexTarget { All, Vectors, Fts }
```

---

## 13. CLI Design

### 13.1 Command Reference

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

> **v0.2+ command stubs:** Commands marked v0.2+ in the milestone table (§15) are compiled into the v0.1 binary as stubs. Invoking them prints: `Error: 'ndex <cmd>' is planned for v0.2 and not yet available.` They appear in `ndex --help` output for discoverability.

### 13.2 `ndex search`

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

SSH:
    --ssh-key <PATH>        SSH private key
    --ssh-port <PORT>       Port [default: 22]
    --ssh-user <USER>       Username [default: $USER]
    --ssh-option <OPT>      Pass-through SSH option
    --remote-path <PATH>    ndex-remote path on server
```

### 13.3 `ndex index`

```
ndex index [HOST:]<PATH> [OPTIONS]

    --full              Force full re-index
    --verify            Recompute BLAKE3 for unchanged files
    --dry-run           Show changes without writing
    --jobs <N>          Extraction parallelism [default: num_cpus]
    --batch-size <N>    Embedding batch size [default: 64]
    --no-vectors        Skip vector embedding
    --enable-ner        Enable named entity recognition
                        [v0.1: accepted but ignored with warning — NER is planned for v0.2]
    --max-file-size <S> Skip files above this size
    --only-new          Process only new files (skip modified)
    --status            Show current indexing status and exit
```

Running `ndex index` after a crash automatically retries `status=0` files (crash recovery per §11.2).

> **v0.1 `--enable-ner` behavior:** The flag is accepted but ignored with a warning: `"NER is not available in v0.1. Flag ignored."` The flag exists in the protocol and CLI for forward-compatibility — clients built for v0.2 will send it, and v0.1 servers must not reject the message.

### 13.4 `ndex init`

```
ndex init <PATH> [OPTIONS]

    --model <MODEL>     default | none
                        default = snowflake-arctic-embed-m-v2.0 (~297 MB, 74 langs)
                        Additional models planned for v0.2.
                        With --model none: no vectors/ directory created, semantic search
                        returns error, hybrid falls back to FTS, auto always selects FTS.
    --exclude <PAT>     Gitignore-style exclude (repeatable)
    --no-fts            Disable full-text index
    --no-meta           Disable metadata extraction
```

> **v0.1 local-only:** `ndex init` is **local-only in v0.1**. The thin client (`ndex`) cannot initialize a remote index via SSH — there is no `InitRequest` in the IPC protocol (§12.4). To initialize an index on a remote server, SSH into the server and run `ndex-remote init <PATH>` directly:
> ```bash
> ssh user@nas "ndex-remote init /pool/archive"
> ```
> After initialization, the thin client can connect normally with `ndex search nas:/pool/archive "query"`. Remote `init` support via `ndex init HOST:PATH` is planned for v0.2.

### 13.5 `ndex info`, `ndex stats`, `ndex verify`

```
ndex info [HOST:]<PATH> <FILE>
    Show metadata for a specific file or directory in the index.
    Outputs: path, size, mtime, mime, blake3, status, tags,
             doc/media metadata, chunk count, index membership.
    For directories: path, mtime, children count from manifest.
    For archives: member count, total uncompressed size, extraction status.
    -f, --format <FMT>      pretty | json

ndex stats [HOST:]<PATH>
    Show index statistics.
    Outputs: total files, indexed/pending/failed/skipped counts,
             index sizes (manifest, FTS, vectors, meta),
             last reconciliation time, model info, schema version.
    -f, --format <FMT>      pretty | json

ndex verify [HOST:]<PATH> [OPTIONS]
    Verify file integrity by recomputing BLAKE3 hashes.
    --sample <FRAC>         Verify random sample (0.01 = 1%)
    --path <GLOB>           Verify files matching glob
    --fail-fast             Stop on first corruption
    -f, --format <FMT>      pretty | json
```

### 13.6 `ndex reindex`

```
ndex reindex [HOST:]<PATH> [OPTIONS]

    --vectors           Re-embed vectors only (FTS/meta preserved)
    --fts               Rebuild FTS only
    --all               Full rebuild (default)
    --confirm           Skip interactive confirmation prompt

Moves .ndex/ → .ndex.old/, rebuilds, then removes .ndex.old/ on success.
Restores .ndex.old/ on failure.
```

### 13.7 Terminal Features

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

struct ProgressChild {
    label: String,          // e.g., "worker-3", "embed", "fts-write"
    current: u64,
    total: Option<u64>,
    message: Option<String>,
}
```

`children` represents sub-tasks within a phase (e.g., individual extraction workers during Phase 3, or the embed/fts-write sub-pipelines). The client renders these as nested progress bars when the terminal is tall enough, or as a summary line otherwise.

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

**Client options vs. server config precedence:** When the client sends options in `IndexOptions` (e.g., `jobs`, `batch_size`, `max_file_size`, `no_vectors`), these **override** the corresponding server `config.toml` values for that session. The server config provides defaults; client-supplied options take precedence. This allows users to run `ndex index --jobs 2` on a server configured for `jobs = 8` without editing the server config. Options not specified in the request fall back to server config defaults.

### 13.8 `ndex delete`

```
ndex delete [HOST:]<PATH> <GLOB> [OPTIONS]
    Remove matching files from all indices (manifest, FTS, vectors, meta).
    --dry-run           Show what would be deleted
    --confirm           Skip interactive confirmation

    Example: ndex delete /pool "secrets/**/*.key"
```

This sets `status=3` in the manifest and removes entries from FTS and meta. The files on disk are not touched.

> **Vector tombstones:** Deleted file vectors are **not immediately removed** from the USearch index. USearch marks them as tombstones internally, but the entries remain on disk until `ndex compact` is run (v0.2). Users should be aware that:
> - Tombstoned vectors do not appear in search results (the sidecar lookup skips deleted file_ids).
> - The vectors persist on disk as tombstones. `ndex compact` (v0.2) is the only way to fully reclaim the space and remove the data from the HNSW graph.

> **Security note for sensitive files:** Tombstoned vectors are **not zeroed** — the raw f16 embedding values remain on disk in `vectors/index.usearch` after `ndex delete`. For security-conscious users: embedding vectors encode semantic content of the original text. While reversing a vector to recover the exact original text is not trivially possible with current techniques, partial semantic information about the content is recoverable in principle (the vector encodes the meaning of the document). If sensitive file content must be removed from the index completely:
> 1. Run `ndex delete` to remove from FTS, manifest, and metadata (these are fully removed).
> 2. Run `ndex compact` (v0.2) to rebuild the USearch index without tombstoned vectors, fully removing the embedding data from disk.
> 3. Until `ndex compact` is available in v0.2, the only way to fully remove embedding data is `ndex reindex --vectors` (rebuilds from scratch, omitting deleted files).

### 13.9 `ndex compact`

> **v0.2 command.** Compiled as a stub in v0.1 — invoking it prints: `"ndex compact is planned for v0.2. To reclaim vector space in v0.1, use: ndex reindex --vectors"`. See §13.8 `ndex delete` for the v0.1 workaround for reclaiming vector tombstone space.

```
ndex compact [HOST:]<PATH> [OPTIONS]
    Optimize index storage by reclaiming space from deleted/updated entries.

    Performs:
    - SQLite VACUUM on manifest.db and meta.db
    - Tantivy segment merge (reduces segment count, reclaims deleted docs)
    - USearch rebuild (removes tombstoned vectors, re-optimizes HNSW graph)

    --dry-run           Show estimated space savings
    --only <INDEX>      Compact specific index: manifest | fts | vectors | meta
```

> **USearch rebuild detail:** USearch "rebuild" during compact iterates all non-deleted sidecar entries, constructs a fresh `Index`, re-adds all active vectors, saves to `index.usearch.tmp`, then renames atomically. The old index is discarded. O(n) but required to reclaim graph-level space. Tombstoned entries accumulate at a low rate (only from `ndex delete` and `ndex reindex --vectors`) so manual compact is infrequent.

### 13.10 `ndex config`

```
ndex config [HOST:]<PATH>              Print config as TOML
ndex config [HOST:]<PATH> get <KEY>    Read a single key (e.g. "auto_refresh.threshold")
```

Write support (`set`, `edit`) deferred to v0.2.

### 13.11 `ndex-remote serve`

```
ndex-remote serve --root <PATH> [--read-only] [--timeout <S>]

Starts a msgpack session on stdin/stdout. The server writes the magic
preamble NDEX\x00\x01 immediately on startup (before any handshake),
then the client sends HandshakeReq to begin. Server exits when stdin
closes (SSH disconnect). All ClientMessage variants are handled within
one session.

    --root <PATH>       Index root directory (required)
    --read-only         Reject write operations (IndexRequest, DeleteRequest, ReindexRequest)
    --timeout <S>       Exit after S seconds of inactivity [default: 0 = no timeout]
```

This is the command the thin client invokes via SSH: `ssh user@host "ndex-remote serve --root /path"`. The session is persistent for the duration of the SSH connection. The server exits on EOF on stdin (SSH disconnect) — no daemon, no lingering processes.

**Graceful shutdown on SSH disconnect:** When stdin reaches EOF or a write to stdout returns `EPIPE`, or when `SIGHUP` is received (SSH hangup), `ndex-remote serve` initiates a graceful shutdown:
1. Stop accepting new `ClientMessage` frames
2. Complete the current in-flight extraction (if any) — cannot abort safely mid-file
3. Flush the SQLite WAL (`PRAGMA wal_checkpoint(PASSIVE)`)
4. Flush Tantivy's pending documents and commit
5. Exit cleanly

Crash recovery (§11.2) handles any state that was not flushed — files in `status=0` are retried on the next run. The shutdown is best-effort: if SIGKILL is received, crash recovery applies.

`ndex-remote` also has a full standalone CLI for local/admin use without the thin client: `ndex-remote index`, `ndex-remote search`, `ndex-remote model`, `ndex-remote self-update`, etc. These are pass-through to the same internals as the serve session, but communicate directly without the msgpack protocol layer.

---

## 14. Search Result Rendering

Paths are displayed relative to the index root. JSON output (`-f json`) includes the `root` field for absolute path reconstruction.

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

## 15. Milestones

### v0.1 — Core

- Reconciler (parallel walk, metadata diff, BLAKE3 hashing)
- Manifest (SQLite WAL), FTS (tantivy), vectors (USearch + snowflake-arctic-embed-m-v2.0, 256d MRL)
- Metadata index (doc_meta, media_meta)
- Extraction: pdf, docx, txt, md, html, code, images (EXIF), archives (ZIP, TAR family, single-file GZ/BZ2/XZ)
- CLI: `init`, `index`, `search` (fts/semantic/hybrid), `info`, `stats`, `reindex`, **`delete`**, `verify`, `config`, `completions`
- `delete` is v0.1 because users must be able to remove accidentally indexed sensitive files without a full reindex
- SSH remote with version negotiation
- Auto-refresh on search
- Auto model fetch on remote
- OSC 8 hyperlinks, progress bars, shell completions
- No-migration index versioning
- .ndexignore, crash-safe incremental indexing
- **Not in v0.1:** thumbnails, multi-model support, `ndex doctor`, `--camera` filter, search tuning CLI flags (`--ef`, `--fts-boost`, `--rrf-k`)

**Command-to-milestone mapping:**

| Command | Milestone | Notes |
|---|---|---|
| `ndex init` | v0.1 | |
| `ndex index` | v0.1 | |
| `ndex search` | v0.1 | fts/semantic/hybrid |
| `ndex info` | v0.1 | |
| `ndex stats` | v0.1 | |
| `ndex reindex` | v0.1 | |
| `ndex delete` | v0.1 | safety valve for sensitive files |
| `ndex config` | v0.1 | view/edit config; write not required for launch |
| `ndex completions` | v0.1 | |
| `ndex verify` | v0.1 | simple: read file, compute BLAKE3, compare against manifest |
| `ndex tag` | v0.2 | stub in v0.1: prints planned-for-v0.2 message |
| `ndex dedup` | v0.2 | stub in v0.1: prints planned-for-v0.2 message |
| `ndex compact` | v0.2 | stub in v0.1: see §13.9 |

**v0.2 command descriptions (for help text authoring):**

> **`ndex dedup [HOST:]<PATH>`** — Finds duplicate files by BLAKE3 hash. Groups files sharing the same `(blake3, size)` pair. Outputs: groups of duplicate paths, total wasted space. Options: `--format pretty|json`, `--min-size <SIZE>` (ignore small files), `--delete-duplicates` (interactive, keeps one copy). v0.2.

> **`ndex tag [HOST:]<PATH> <SUBCOMMAND>`** — Manage tags on indexed files. Subcommands: `add <FILE> <TAG>`, `remove <FILE> <TAG>`, `list [FILE]`, `search <TAG>`. Tags are stored in `meta.db` (schema exists, see §10.4). v0.2.

### v0.2 — Breadth

- CJK tokenizers, 7Z and RAR archive content extraction, email indexing
- Tags, NER, dedup, compact
- Thumbnails (deferred from v0.1), CUDA embedding
- `ndex-remote self-update`
- `--index-dir` flag for relocating `.ndex/` (NFS mitigation)
- Multi-model support (granite-small and future models)
- `ndex config` write mode (`set`, `edit`)
- `ndex doctor` (index health checks)
- `--camera` search filter, search tuning CLI flags (`--ef`, `--fts-boost`, `--rrf-k`)

### v0.3 — Scale + Intelligence

- CLIP image semantic search
- OCR for scanned PDFs
- Product quantization for 100M+ vector scale
- Per-dataset sharded manifests
- Optional local web UI

---

## 16. Implementation Decisions

### v0.1 (resolved)

1. **RESOLVED: Model download failure mid-stream.** What happens if the model download is interrupted (network drop, disk full)?
   - **Decision:** Download to a `.tmp` file. On failure, delete the `.tmp`. On next run, restart the download from scratch (no resume). If disk is full, fail with a clear message listing required space. The model is never "partially installed."

2. **RESOLVED: CancelRequest behavior.** `CancelRequest` is defined in the protocol (§12.4) but its behavior is unspecified.
   - **Decision:** On receiving `CancelRequest`, the server finishes the current in-flight extraction (cannot be interrupted safely mid-file), flushes the SQLite WAL, and sends `IndexComplete` with whatever was indexed before the cancel. For search, cancel is a no-op (search completes fast enough). For long-running index operations, the current batch commits and the server exits cleanly.
   - **Response type:** The response to a `CancelRequest` is always the in-progress operation's normal completion message — `IndexComplete` for index/reindex operations. There is no separate `CancelAck` message type. The `IndexComplete` message indicates processing stopped early via `cancelled: bool` in `IndexCompleteData` (see §12.4). Clients should treat a `CancelRequest` as asynchronous: after sending it, they wait for the next `IndexComplete` or `Error` message.

3. **RESOLVED: Search with empty vector index.** On first run, no vectors exist. What does `ndex search` return in `semantic` or `hybrid` mode?
   - **Decision:** If the vector index is empty (0 entries), semantic mode returns 0 results with a warning: "Vector index is empty. Run `ndex index` to build the index." Hybrid mode falls back to FTS-only with a warning. `auto` mode selects `fts` if the vector index is empty (per the heuristics in §10.7).

4. **RESOLVED: Tantivy segment merge configuration.** Tantivy creates many small segments during incremental indexing. Without periodic merging, segment count grows unboundedly, degrading search performance.
   - **Decision:** After each incremental indexing batch, call `writer.merge()` if segment count exceeds a threshold (default: 8). Full reindex triggers a final `writer.merge().wait()` to produce a single optimized segment. Expose `ndex compact --only fts` for manual merge. Tantivy's `MergePolicy::LogMergePolicy` (the default) handles this automatically — verify it's configured and not disabled.
   - **Merge blocking vs. async:** Tantivy's merge is **async by default** — `writer.merge()` schedules the merge and returns immediately; it does not block the indexing pipeline. The merge runs in a background thread. `writer.merge().wait()` blocks until complete (used only during `ndex reindex`). During rapid incremental indexing, multiple merges can be queued; Tantivy serializes them. If segments accumulate faster than merges complete (e.g., very high throughput), segment count may temporarily exceed the threshold — this is acceptable and self-correcting. Search performance degrades gracefully with higher segment counts (logarithmically, not catastrophically).

### v0.3+ (deferred)

5. **Multi-root.** Single ndex instance spanning `/pool/photos` + `/pool/docs`. Requires root registry and cross-root dedup. v0.3.

6. **Embedding model migration.** Current: schema mismatch → full re-embed via `ndex reindex --vectors`. Should we support old vectors for FTS-only search while re-embedding in background? v0.3.

7. **Index portability.** All paths are relative to root. Copying `.ndex/` to another machine with the same layout should work. Needs testing. v0.2.

---

## 17. Logging and Diagnostics

### Log Output

- All logs go to **stderr** (stdout is reserved for search results and machine-readable output).
- Verbosity levels controlled by `-v` / `-vv` / `-vvv` (maps to `INFO` / `DEBUG` / `TRACE`).
- Default level: `WARN` (quiet unless something is wrong).
- Structured log format: `YYYY-MM-DDTHH:MM:SS.mmm LEVEL target: message [key=value ...]`
- Log filtering via `NDEX_LOG` env var (same syntax as `RUST_LOG`): e.g., `NDEX_LOG=ndex_remote=debug,tantivy=warn`.

### Log File

For long indexing runs, logs can be redirected to a file:
```bash
ndex index /pool --log-file /var/log/ndex-index.log
```
The log file receives all output (including progress events as structured log lines). stderr still receives `ERROR` and `WARN`.

### Verbosity Semantics

| Level | `-v` flags | Content |
|---|---|---|
| `WARN` (default) | (none) | Errors, warnings, per-file failures |
| `INFO` | `-v` | Phase transitions, batch summaries, timing |
| `DEBUG` | `-vv` | Per-file processing, embedding batches, SQL queries |
| `TRACE` | `-vvv` | Raw protocol frames, ONNX tensor shapes, walk entries |

### Server-Side Config Reference

Complete list of `config.toml` keys (consolidated from §4.5, §4.6, §4.7, §6.2, §11.1, §11.4):

```toml
[chunking]
target_tokens = 512
overlap_tokens = 128
min_tokens = 32
heading_prefix = true

[extraction]
max_file_size = "2GiB"
max_retries = 3

[embedding]
batch_size = 64
threads = 0               # alias for intra_op_threads (backward compat)
intra_op_threads = 0      # threads within a single ONNX op (0 = all cores)
inter_op_threads = 1      # threads for independent graph node scheduling (default: 1)

[auto_refresh]
enabled = true
threshold = "1h"
warn_threshold = "7d"
timeout_secs = 30
index_new_only = true

[ignore]
respect_gitignore = true
respect_ndexignore = true

[walk]
follow_symlinks = true
hidden = true           # true = index dotfiles (default), false = skip hidden files

[search]
default_limit = 20
rrf_k = 60
title_boost = 2.0     # BM25 field weight for title field in FTS scoring
fts_weight = 1.0      # RRF component multiplier for FTS score in hybrid mode
ef_search = 128

[archive]
max_archive_total_size = "8GiB"    # total decompressed bytes per archive
max_archive_members = 100000        # max members to extract per archive
max_archive_depth = 3               # max nesting depth for recursive archives
compression_ratio_limit = 200       # max decompressed:compressed ratio per member
```

### 17.1 Instrumentation

ndex uses `tracing` (not `log`) as the sole instrumentation layer across all crates. `tracing` subsumes `log` via the `tracing-log` compatibility layer.

**Setup:**
- `tracing-subscriber` with `fmt` layer for human-readable stderr output.
- `EnvFilter` driven by `NDEX_LOG` (replaces a raw `log`-based approach). Format: `ndex_remote=debug,tantivy=warn`.
- The `--log-file` flag uses `tracing_appender` for non-blocking file output.

**Instrumentation policy:**
- `#[tracing::instrument]` on all public API boundaries and cross-component calls: reconciler phases, extractor entry points, embedding batches, search query handlers, IPC message handlers, SQLite transactions.
- **Skip** `#[instrument]` on tight inner loops where overhead would be measurable: per-token chunking, per-byte BLAKE3 streaming, per-vector USearch add.

**Structured span fields:**
- `file_id`, `path` (lossy UTF-8), `phase`, `mime_type`, `chunk_count`, `batch_size`, `query` — enables searchability in structured log sinks.

**Async span propagation:**
- `tracing::Span` context is propagated across async boundaries. Crossbeam channels carry span context for the extraction → embedding → write pipeline.

**Wire protocol:**
- `TRACE`-level span per frame with fields `msg_kind`, `payload_len`.

**Future-proofing:**
- `tracing` supports `opentelemetry` and `tokio-console` subscribers. No v0.1 requirement to enable these — the tracing foundation makes them zero-cost additions in v0.2+.

---

## 18. Testing Strategy

### 18.1 Test Categories

**Unit tests:** Per-module. Focus areas:
- Extractors: one test per format with a fixture file covering the happy path
- Chunking: boundary conditions (empty input, single token, exactly `target_tokens`, exactly `target_tokens + 1`, all-heading document)
- BLAKE3 computation: known-good hash vectors
- Path handling: non-UTF-8 filenames, null bytes, extremely long paths
- Search scoring: BM25 field boost math, cosine similarity on known vectors, RRF fusion with known ranks

**Integration tests:** Full `init → index → search` round-trip against a temporary directory with fixture files. Covers all v0.1 formats (PDF, DOCX, Markdown, HTML, code, images, archives, plaintext). Validates:
- Manifest state after indexing (correct status, BLAKE3, MIME)
- FTS search returns expected results for known queries
- Semantic search returns expected results (top-k accuracy on fixture corpus)
- Metadata populated correctly (`doc_meta`, `media_meta`)
- Archive members indexed with correct synthetic paths

**Protocol round-trip tests:** Serialize and deserialize every `ClientMessage` and `ServerMessage` variant using `rmp_serde::to_vec_named()` / `from_slice()`. Verify that the round-trip produces identical values. Run these before the protocol is considered stable (§12.4).

**Crash recovery tests:**
- Kill `ndex-remote` mid-index (SIGKILL) at various points (post-walk, mid-Phase 3, mid-embed)
- Restart and verify recovery from `status=0` files
- Verify sidecar/USearch mismatch repair (inject a mismatch by corrupting the sidecar count)

**SSH transport tests:** Use a local SSH server (or `ssh localhost`) to test the full thin-client → SSH → ndex-remote → SSH → thin-client path, including:
- Magic preamble detection with injected stdout garbage
- Protocol version negotiation
- CancelRequest over SSH

**Performance regression tests (advisory, non-blocking):** Track wall-clock time for walk, diff, extract, embed, and search phases on a fixed corpus (10,000 files representative of the v0.1 format mix). Run in CI; alert on > 20% regression. These tests are advisory — they do not block merge, but regressions are investigated before release.

### 18.2 Test Corpus

Maintain a `tests/fixtures/` directory with representative files for each format. Include edge cases:
- Zero-byte file
- BOM-prefixed UTF-16 text
- Non-UTF-8 filename (Latin-1 encoded path component)
- Scanned PDF (no text layer) → expected `status=4`
- Encrypted PDF → expected `status=4`
- Hard-linked pair (two paths, one inode)
- Symlink cycle (A → B → A)
- `.ndexignore` pattern file (verify excluded paths are not indexed)
- Archive with path traversal member (`../../../etc/passwd`) → expected member skip
- Archive with compression ratio > 200:1 (decompression bomb) → expected member skip
- Deeply nested archive (depth > 3) → expected depth-limit skip

### 18.3 Backup and Recovery

#### Safe Backup Methods

`.ndex/` cannot be safely copied while `ndex-remote` is running with active WAL transactions. Safe backup methods:

1. **WAL checkpoint then copy:** Run `ndex-remote checkpoint --root /pool` (issues `PRAGMA wal_checkpoint(TRUNCATE)` on both databases, folding WAL into main DB files). Then copy `.ndex/` while no ndex-remote process is running.

2. **ZFS/Btrfs snapshots:** Safe because they are atomic at the filesystem level. The snapshot captures a consistent point-in-time, and SQLite's WAL recovery handles any in-flight transactions on restore.

3. **SQLite online backup API:** `sqlite3 manifest.db ".backup /tmp/manifest.db"` for per-database hot backup while ndex-remote is running. Both databases must be backed up in sequence (manifest.db first, then meta.db).

#### Recovery from Backup

Copy `.ndex/` back to its original location. `ndex-remote` runs WAL recovery on open. If the backup was taken mid-index, crash recovery (§11.2) handles any `status=0` files.

#### Full Rebuild as Disaster Recovery

The index is derived data — always rebuildable from source files via `ndex reindex`. Backup is a convenience optimization to avoid rebuild time, not a requirement for correctness.


## TODOs

- Plan out TUI that lets you use the index to search through content.
- TUI should support rendering images and streaming over for preview (compress if needed, use rawshift if needed).
