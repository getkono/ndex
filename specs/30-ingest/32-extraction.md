# 32 — Format Extraction

**Owns:** MIME-type detection and text/binary classification, text encoding detection/transcoding/normalization, document language detection, archive-safety primitives, the `Extractor` trait and MIME → extractor routing, and every per-format extractor.

**Sources:**
- `crates/ndex-extract/src/extractor.rs`
- `crates/ndex-extract/src/mime.rs`
- `crates/ndex-extract/src/encoding.rs`
- `crates/ndex-extract/src/lang.rs`
- `crates/ndex-extract/src/archive_safety.rs`
- `crates/ndex-extract/src/formats/{mod,text,markdown,html,code,docx,pdf,image,archive}.rs`
- `crates/ndex-extract/src/lib.rs`
- Tests: `crates/ndex-extract/tests/characterization.rs`

Extraction is stage 2 of the ingest pipeline: the reconciler ([31-reconcile.md](31-reconcile.md)) reads a file's bytes, calls `mime::detect`, routes through `router(&mime)` — a `Route::Skip` result means the file is skipped rather than extracted — and passes the resulting `Extraction` to the chunker ([33-chunking.md](33-chunking.md)). The crate depends only on `ndex-core`; token counting is injected via `ndex_core::tokens::TokenCounter` so there is no dependency on `ndex-embed`.

---

## 1. Extractor trait and extraction output — ✅ implemented

`crates/ndex-extract/src/extractor.rs`

| Item | Shape |
|---|---|
| `Extractor` | Object-safe trait: `fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction>` |
| `ExtractCtx<'a>` | `mime: &str` (detected MIME), `path: &NdexPath` (for language detection / archive member naming / logging), `tokens: &dyn TokenCounter`, `depth: u8` (archive nesting; `0` for regular files), `config: &Config` |
| `Extraction` | `blocks: Vec<Block>`, `doc_meta: Option<DocMeta>`, `media_meta: Option<MediaMeta>`, `lang: Option<String>`; derives `Default` (all empty/`None`) |

`Block`, `DocMeta`, `MediaMeta`, and `Chunk` are core types — see [11-data-model.md](../10-core/11-data-model.md). `Config` sections are specified in [13-config.md](../10-core/13-config.md). Errors surface as `ndex_core::error::NdexError` — see [14-errors.md](../10-core/14-errors.md).

Extractors work on a fully materialized `&[u8]`; there is no streaming extraction interface in v0.1 (the reconciler does `std::fs::read`).

## 2. MIME detection — ✅ implemented

`crates/ndex-extract/src/mime.rs` (PRD §4.4, §4.8)

### 2.1 Detection chain (`mime::detect`)

First match wins, in this exact order:

| # | Stage | Mechanism | Result |
|---|---|---|---|
| 1 | Magic bytes | `infer::get(bytes)` | `kind.mime_type()` |
| 2 | Extension | `mime_guess::from_path(<file name>).first()` | `guess.essence_str()` |
| 3 | Known filename | static `known_filename` table (§2.3) | table MIME |
| 4 | Text/binary heuristic | `is_text(bytes)` (§2.2) | `text/plain`, else `application/octet-stream` |

Magic bytes beating a lying extension is locked in by the characterization test `magic_bytes_beat_extension` (a PNG named `masquerade.txt` detects as `image/png`); the extension and octet-stream fallbacks by `extension_and_fallbacks` (which also pins that an extensionless textual `README` yields `text/plain`).

The filename used by stages 2–3 is the final `/`-separated component of the `NdexPath` and requires the whole path to be valid UTF-8; non-UTF-8 paths skip stages 2–3 entirely and fall through to the byte heuristic.

### 2.2 Text/binary heuristic (`mime::is_text`)

Git-style: a file is **binary** iff a NUL byte (`0x00`) appears within the first `TEXT_SNIFF_BYTES` = **8192** bytes (constant defined in this module). Empty input is text. A NUL beyond the 8192-byte window does not flip the classification — locked in by `is_text_uses_nul_heuristic`.

### 2.3 Known-filename table (`mime::known_filename`)

| Filename | MIME |
|---|---|
| `Makefile`, `makefile`, `GNUmakefile` | `text/x-makefile` |
| `Dockerfile` | `text/x-dockerfile` |
| `CMakeLists.txt` | `text/x-cmake` |
| `Vagrantfile`, `Rakefile`, `Gemfile`, `Procfile`, `Jenkinsfile` | `text/plain` |

Locked in by `known_filenames_and_extension_languages`. Note: within the `detect` chain, the `CMakeLists.txt` entry is unreachable — stage 2 (`mime_guess`) resolves the `.txt` extension to `text/plain` first (see Divergences).

### 2.4 Extension → code language (`mime::extension_language`)

Maps the last-`.`-suffix to a language name intended for the tree-sitter router (§8.4):

| Extensions | Language |
|---|---|
| `rs` | `rust` |
| `py` | `python` |
| `js`, `mjs`, `cjs` | `javascript` |
| `ts` | `typescript` |
| `c`, `h` | `c` |
| `cpp`, `cc`, `cxx`, `hpp` | `cpp` |
| `go` | `go` |
| `sh`, `bash` | `bash` |

Pragmatic v0.1 defaults per PRD §4.4: `.h` → C (not C++) — locked in by `known_filenames_and_extension_languages`. PRD also specifies `.m` → Objective-C, which is **not** in the table. Nothing currently calls this function outside tests (see Divergences). Shebang detection (PRD §4.4 step 2) is 📋 planned — no code.

## 3. MIME → extractor routing — ✅ implemented (many targets are stubs)

`crates/ndex-extract/src/extractor.rs` — `router(mime) -> Route`, checked top-to-bottom.

| Item | Shape |
|---|---|
| `Route` | `pub enum Route { Extract(Box<dyn Extractor>), Skip }` — the routing decision for a detected MIME type |

`Route::Skip` means no extractor handles the type: the caller must not extract and should record the file as skipped (`status=5`, PRD §4.8). Wiring the skip disposition through the reconciler is owned by [31-reconcile.md](31-reconcile.md).

| MIME(s) | Route | Extractor status |
|---|---|---|
| `application/pdf` | `formats::pdf::PdfExtractor` | ⛔ stub |
| `application/vnd.openxmlformats-officedocument.wordprocessingml.document` | `formats::docx::DocxExtractor` | ⛔ stub |
| `text/markdown`, `text/x-markdown` | `formats::markdown::MarkdownExtractor` | 🚧 partial |
| `text/html`, `application/xhtml+xml`, `application/xml`, `text/xml`, `image/svg+xml` | `formats::html::HtmlExtractor` | ⛔ stub |
| `application/json` | `formats::text::JsonExtractor` | 🚧 partial |
| `text/csv`, `text/tab-separated-values` | `formats::text::CsvExtractor` | 🚧 partial |
| `application/sql`, `text/x-sql` | `formats::text::SqlExtractor` | 🚧 partial |
| any `is_archive_mime` (see below) | `formats::archive::ArchiveExtractor` | ⛔ stub |
| any other `image/*` | `formats::image::ImageExtractor` | ⛔ stub |
| any other `text/*` | `formats::text::PlaintextExtractor` | ✅ implemented |
| everything else (catch-all `_`), including `application/octet-stream` | **`Route::Skip`** | — |

`is_archive_mime` recognizes exactly: `application/zip`, `application/x-tar`, `application/gzip`, `application/x-gzip`, `application/x-bzip2`, `application/x-xz`, `application/x-7z-compressed`, `application/vnd.rar` (7Z/RAR are metadata-only per PRD §4.6, but still route to `ArchiveExtractor`). Locked in by `archive_mimes_recognized`.

Notes on the dispatch:

- `image/svg+xml` is matched **before** the `image/*` prefix branch, so SVG routes to the HTML/XML text path per PRD §4.8 (searchable `<text>`/`<title>`/`<desc>` content), never to EXIF.
- There is **no code-MIME branch**: `text/x-rust` and friends match the `text/*` family branch and go to `PlaintextExtractor`. Nothing consults `extension_language` for code routing (see Divergences).
- `Route::Skip` covers `application/octet-stream`, `video/*`, `audio/*`, `font/*`, and every unrecognized `application/*`. That last group includes some *textual* MIMEs `mime_guess` can emit — notably `application/x-sh` for `.sh` — which previously fell through to plaintext extraction and are now skipped (see Divergences).
- `LogExtractor` and `CodeExtractor` are never returned by the router; they are currently unreachable in production code.

Locked in by `router_dispatches_supported_mimes_and_skips_the_rest` (`tests/characterization.rs`): every extractor branch constructs without panicking (even where `extract()` is `todo!()`), and octet-stream / `video/mp4` / `font/woff2` yield `Route::Skip`. The in-module tests `router_dispatches_supported_mimes` and `router_skips_mimes_with_no_extractor` duplicate this.

## 4. Encoding detection, transcoding, normalization — ✅ implemented

`crates/ndex-extract/src/encoding.rs` (PRD §4.8, §10.2)

### 4.1 BOM handling

| BOM | Bytes | `Bom::byte_len()` |
|---|---|---|
| `Bom::Utf8` | `EF BB BF` | 3 |
| `Bom::Utf16Le` | `FF FE` | 2 |
| `Bom::Utf16Be` | `FE FF` | 2 |

`detect_bom` matches these prefixes; `strip_bom` returns the slice after the marker (identity when no BOM). Locked in by `bom_detection_lengths_and_stripping`.

### 4.2 Decode chain (`decode_to_utf8(bytes) -> Cow<str>`)

In order:

1. **UTF-16 BOM** → decode body as UTF-16 (LE/BE per BOM) via `char::decode_utf16`; unpaired surrogates become `U+FFFD`; an odd trailing byte is silently dropped (`chunks_exact(2)`).
2. **UTF-8 BOM** → strip 3 bytes, `String::from_utf8_lossy` on the rest (always allocates).
3. **Valid UTF-8** (`std::str::from_utf8` succeeds) → zero-copy `Cow::Borrowed` passthrough.
4. **Legacy fallback** → `chardetng::EncodingDetector` (constructed with `Iso2022JpDetection::Deny`), fed the whole buffer with `last=true`, `guess(None, Utf8Detection::Allow)` (no TLD hint), then `encoding_rs` `encoding.decode(bytes)` — lossy on undecodable bytes (`U+FFFD`).

There is **no confidence threshold** — whatever `chardetng` guesses is used unconditionally (PRD §4.8 describes a "confidence is sufficient" gate; see Divergences). There is **no logging** of transcodes or lossy conversions (PRD §4.8 requires DEBUG/WARN logs; see Divergences). The HTML `<meta charset>` override (PRD §4.8 step 3) is 📋 planned — the HTML extractor is a stub.

UTF-16LE round-trip and valid-UTF-8 passthrough are locked in by `decode_to_utf8_handles_utf16_and_passthrough`.

### 4.3 NFC normalization

`nfc_normalize(text)` applies Unicode NFC via `unicode-normalization` so NFC/NFD spellings match at tokenization time (PRD §10.2). Locked in by `nfc_normalization` (NFD `cafe`+combining-acute → NFC `café`; already-NFC text unchanged).

## 5. Language detection — ✅ implemented

`crates/ndex-extract/src/lang.rs` (PRD §10.2)

`lang::detect(text) -> Option<String>`:

- Returns `None` if `text.len()` (bytes, not chars) < `MIN_DETECT_LEN` = **20** (constant defined in this module).
- Otherwise runs `whatlang::detect`; returns `None` unless `info.is_reliable()`.
- On success returns `whatlang`'s language code, which is **ISO 639-3** (e.g. `eng`) — the ISO 639-1 mapping (`en`) expected by `doc_meta.lang` is an acknowledged follow-up in the module's own doc comment.

Locked in by `language_detection_and_short_text_guard` (asserts `MIN_DETECT_LEN == 20`, short-text `None`, and English text → `Some("eng")`).

## 6. Archive safety primitives — ✅ implemented (helpers only; nothing wires them to real archives yet)

`crates/ndex-extract/src/archive_safety.rs` (PRD §4.6, §4.9)

| Item | Behavior |
|---|---|
| `MEMBER_DELIM` | `b"!/"` — the JAR-convention delimiter between archive path and member path |
| `member_path(archive, member)` | Byte-concatenation `<archive>!/<member>` returning a new `NdexPath` |
| `is_unsafe_member_path(member)` | `true` iff the member starts with `/`, contains NUL, or has any `/`- or `\`-separated component equal to `..` — catches `../x`, `x/../y`, trailing `x/..`, bare `..`, and `dir\..\x`; a `..` embedded in a name (`notes..txt`, `..hidden`) is safe |
| `exceeds_ratio(compressed, decompressed, limit)` | `decompressed / max(compressed, 1) > limit` — integer (floor) division; the ratio must *strictly* exceed `limit`; zero compressed size is clamped to 1, so ratio checks never divide by zero |
| `with_panic_isolation(f)` | `std::panic::catch_unwind` (with `AssertUnwindSafe`); a panic maps to `NdexError::ExtractionTransient("archive extractor panicked")`. Does not protect against `abort()` or stack overflow in native code |

The numeric limits (`compression_ratio_limit`, `max_archive_total_size`, `max_archive_members`, `max_archive_depth`) live in the `[archive]` config section — values owned by [13-config.md](../10-core/13-config.md).

Locked in by `unsafe_member_paths_rejected` (including bare/trailing `..` components, the Windows-style `dir\..\x` case, and the safe `notes..txt` counterexample), `compression_ratio_and_member_path` (including the `(0, 0)` divide-by-zero guard), and `panic_isolation_catches_and_passes_through`. The reconciler wraps **all** extraction (not just archives) in `with_panic_isolation` — see [31-reconcile.md](31-reconcile.md).

Not implemented here (📋 planned, per PRD §4.9): total-size / member-count / depth enforcement loops, ratio checks "every 1 MiB of output" during streaming, per-member error isolation, `extraction_status='partial'` bookkeeping. These require the archive extractor (§8.8), which is a stub.

## 7. Text-family extraction — the only working extraction path

`crates/ndex-extract/src/formats/text.rs`

### 7.1 Shared pipeline (`text_extraction`) — ✅ implemented

All text-family extractors share one path:

1. `encoding::decode_to_utf8(bytes)` (§4.2)
2. `encoding::nfc_normalize` (§4.3)
3. `lang::detect` on the full normalized text (§5)
4. `paragraph_blocks` → `Vec<Block>`; `doc_meta`/`media_meta` are always `None`

The `ExtractCtx` is entirely ignored (`_ctx`) — config, path, and token counter play no role in this path.

### 7.2 Paragraph splitting (`paragraph_blocks`) — ✅ implemented

- Split the normalized text on the literal delimiter `"\n\n"` (exactly two newlines; `\r\n\r\n` is *not* a paragraph boundary).
- For each part: trim whitespace; skip if empty; emit a `Block` with `block_type = BlockType::Paragraph`, `text = trimmed content`, `byte_start = running offset + leading-whitespace length`, `byte_end = byte_start + content.len()`, `heading_path = []`.
- The running offset advances by `part.len() + 2` per part (accounting for the consumed delimiter).

**Important:** these byte offsets index into the *decoded, NFC-normalized* string — for transcoded or non-NFC input they do not correspond to raw file byte positions. That the plaintext extractor yields non-empty blocks for two-paragraph input is locked in by `plaintext_extractor_yields_blocks`.

### 7.3 The extractors

| Extractor | Status | Current behavior | Spec intent (doc comments + PRD) |
|---|---|---|---|
| `PlaintextExtractor` | ✅ | `text_extraction` | Plaintext + config/markup formats (YAML, TOML, INI, rST, AsciiDoc, LaTeX; PRD §4.8). Recursive `\n\n` > `\n` > `. ` > ` ` splitting (PRD §4.5) is 📋 — only the `\n\n` level exists (deeper splitting is deferred to the chunker's word windows, [33-chunking.md](33-chunking.md)) |
| `CsvExtractor` | 🚧 | alias of `text_extraction` | Record-based: row boundaries, header propagation, delimiter auto-detection (`,` `\t` `;` `\|`), quoted newlines (PRD §4.5) |
| `JsonExtractor` | 🚧 | alias of `text_extraction` | Variant-aware: object → top-level keys, array → element boundaries, NDJSON → lines (PRD §4.8) |
| `SqlExtractor` | 🚧 | alias of `text_extraction` | Statement-based, split on `;` (PRD §4.5) |
| `LogExtractor` | 🚧 (and unrouted) | alias of `text_extraction` | Line-batched, timestamp-pattern aware (PRD §4.5). The `.log`-extension / timestamp-sniff routing rule (PRD §4.5) is 📋 — nothing routes to this extractor |

### 7.4 JSON variant helper — ✅ implemented

`json_variant(bytes) -> Option<JsonVariant>` classifies by first non-whitespace byte of the (UTF-8-validated) input: `{` → `Object`, `[` → `Array`, anything else (including NDJSON starting with a scalar, empty input, or invalid UTF-8) → `None`. NDJSON disambiguation is deferred to the extractor. Locked in by `json_variant_by_first_nonws_byte`. The `JsonExtractor` does not yet consult this helper.

## 8. Structured-format extractors

All spec intent below comes from module doc comments plus PRD §4.4/§4.5/§4.8; bodies marked ⛔ are `todo!()` and currently *panic* when invoked (the reconciler's panic isolation converts that to a permanent extraction failure — see [31-reconcile.md](31-reconcile.md)).

### 8.1 Markdown — 🚧 partial

`crates/ndex-extract/src/formats/markdown.rs` — `MarkdownExtractor` delegates to `text_extraction` (§7.1), so Markdown is fully FTS-searchable as plain paragraphs today. Intent: `pulldown-cmark` structured extraction — headings, code blocks, lists as blocks; `title` ← first `#` heading; YAML (`---`) / TOML (`+++`) frontmatter indexed as raw text in v0.1 with structured `doc_meta` extraction deferred to v0.2 (PRD §4.5).

### 8.2 HTML / XML / SVG — ⛔ stub

`crates/ndex-extract/src/formats/html.rs` — `HtmlExtractor::extract` is `todo!()`. Intent: `lol_html` (streaming) + `scraper` (DOM); structure signals `<h*>`, `<p>`, `<pre>`, `<li>`; `doc_meta` from `<title>`/`<meta>`; `<meta charset>` overrides encoding detection; XML routes here (lenient tag handling; plaintext fallback for severely malformed input); SVG text content indexed, pixel dimensions to `media_meta`, no `doc_meta`, no EXIF (PRD §4.5, §4.8).

### 8.3 Code — ⛔ stub (and unreachable)

`crates/ndex-extract/src/formats/code.rs` — `CodeExtractor::extract` is `todo!()`, and no router branch or reconciler path constructs it. Intent: tree-sitter AST; top-level declarations (functions, classes, impls, modules) as section-level boundaries; enclosing declaration name propagated as heading context; no-grammar languages fall through to plaintext (PRD §4.4, §4.5).

The grammar map `language_for(lang) -> Option<tree_sitter::Language>` is ✅ implemented but covers only **3** grammars: `rust`, `python`, `javascript` (crates `tree-sitter-rust`/`-python`/`-javascript` in `crates/ndex-extract/Cargo.toml`). Everything else — including `typescript`, `c`, `cpp`, `go`, `bash`, which `extension_language` (§2.4) can emit — returns `None` ⇒ plaintext fallthrough. PRD §4.4 lists ~32 bundled grammars. Locked in by `language_for_maps_bundled_grammars`.

### 8.4 DOCX — ⛔ stub

`crates/ndex-extract/src/formats/docx.rs` — `DocxExtractor::extract` is `todo!()`. Intent: `docx-rust` reading paragraph styles, headings, tables; fallback to paragraph-boundary splitting on malformed files (`status=2` only if no text at all); `doc_meta` from `docProps/core.xml` + `app.xml` (PRD §4.4, §10.4). XLSX/PPTX explicitly do **not** route here — they are archive metadata-only in v0.1 (PRD §4.8).

### 8.5 PDF — ⛔ stub

`crates/ndex-extract/src/formats/pdf.rs` — `PdfExtractor::extract` is `todo!()`. Intent: `pdf_oxide` text + Info-dictionary `doc_meta`; optional `pdfium` fallback behind the `pdfium` cargo feature (off by default, `crates/ndex-extract/Cargo.toml`); image-only PDFs (< 20 chars extracted from ≥ 1 page) → `status=4` `[DEFERRED]`; encrypted PDFs → `status=4` `[UNSUPPORTED]` (PRD §4.4, §4.8).

### 8.6 Image — ⛔ stub

`crates/ndex-extract/src/formats/image.rs` — `ImageExtractor::extract` is `todo!()`. Intent: `kamadak-exif` for EXIF (JPEG/TIFF/HEIC/raw per the PRD §4.8 EXIF matrix) + `image` crate for `width`/`height` on all decodable formats; produces `media_meta` only — no `doc_meta`, no chunks. Video/audio never reach this extractor; the reconciler handles them as `status=1` with empty `media_meta` (PRD §4.6).

### 8.7 Archive — ⛔ stub

`crates/ndex-extract/src/formats/archive.rs` — `ArchiveExtractor::extract` is `todo!()`. The `ArchiveFormat` enum (`Zip`, `Tar`, `TarGz`, `TarBz2`, `TarXz`, `Gz`, `Bz2`, `Xz`) is defined but unused. Intent: members streamed one at a time through the standard pipeline under the §6 safety limits, with recursion + depth driven by the reconciler; the extractor itself returns only archive-level blocks/metadata; 7Z/RAR metadata-only; OOXML never recursively unpacked (PRD §4.6, §4.9). Decompression crates (`zip`, `tar`, `flate2`, `bzip2`, `liblzma`) are declared in `crates/ndex-extract/Cargo.toml` but not yet exercised.

## 9. Test coverage summary

`crates/ndex-extract/tests/characterization.rs` exercises live: MIME chain, NUL heuristic + `TEXT_SNIFF_BYTES`, known-filename/extension tables, BOM/NFC, UTF-16 decode + UTF-8 passthrough, language detection + `MIN_DETECT_LEN`, member-path safety + ratio + panic isolation, archive-MIME set, router dispatch and skip, JSON variant classification, plaintext block extraction, and the grammar map. Unit tests inside the modules duplicate most of these. **Never tested:** the chardetng legacy-encoding fallback (step 4 of §4.2), every ⛔ extractor, and any end-to-end archive behavior.

## Divergences & open questions

1. **Code routing does not exist.** There is no code-MIME branch in `router` — code types like `text/x-rust` are plaintext-extracted via the `text/*` family branch — the reconciler never calls `extension_language`, and `CodeExtractor` is unreachable. Shebang detection (PRD §4.4 step 2) has no code at all. Relatedly, script extensions whose `mime_guess` MIME is not `text/*` (e.g. `.sh` → `application/x-sh`) now hit the `Route::Skip` catch-all: such files are skipped rather than indexed until code routing lands.
2. **Grammar set: 3 vs ~32.** PRD §4.4 lists ~32 bundled tree-sitter grammars; only Rust/Python/JavaScript are bundled. `extension_language` emits `typescript`/`c`/`cpp`/`go`/`bash` which `language_for` cannot resolve. PRD's `.m` → Objective-C default is absent from the extension table.
3. **`CMakeLists.txt` known-filename entry is dead in `detect`.** `mime_guess` resolves `.txt` → `text/plain` at stage 2, so stage 3 never sees it; the direct-helper test passes anyway. Either reorder the chain or drop the entry.
4. **No chardetng confidence gate, no transcode logging.** PRD §4.8 requires a confidence threshold with lossy fallback, DEBUG logs for transcodes, and WARN logs for U+FFFD replacements; `decode_to_utf8` uses the guess unconditionally and logs nothing. The magic-vs-extension disagreement DEBUG log (PRD §4.4) is also absent.
5. **Language codes are ISO 639-3, `doc_meta.lang` expects ISO 639-1.** Acknowledged in the `lang.rs` doc comment; unresolved. Also `MIN_DETECT_LEN` measures bytes, so 20 *bytes* of CJK is ~7 characters — the guard is looser than "20 characters" for multibyte scripts.
6. **`exceeds_ratio` floor-division boundary.** With integer division, `decompressed = limit × compressed` does *not* exceed (`200:1` exactly passes; `200×c + c` needed to trip). PRD's "200:1 per member" limit is thus effectively "> 200:1 after floor". Also every §4.9 enforcement loop (total size, member count, depth, 1-MiB ratio cadence) is unimplemented pending the archive extractor.
7. **Block byte offsets are normalized-text-relative.** After transcoding/BOM-stripping/NFC, `Block.byte_start`/`byte_end` (and therefore chunk offsets, [33-chunking.md](33-chunking.md)) do not map back to raw file bytes. No consumer or test defines which coordinate space is authoritative.
8. **`text/markdown` never emerges from `detect` in practice for `.md`** — `mime_guess` maps `.md`/`.markdown` to `text/markdown`, so this works; but files whose Markdown-ness is only knowable by content (extensionless `README`) classify as `text/plain` and skip the Markdown extractor. Consistent with PRD, noted for completeness.

*Resolved here (2026-07):* the former octet-stream-to-plaintext divergence (router now returns `Route::Skip`, §3; reconciler wiring of `status=5` is owned by [31-reconcile.md](31-reconcile.md)), the `is_unsafe_member_path` bare-`..` gap (§6), and the stale characterization-test header comment.
