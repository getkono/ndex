# 80 — Testing

**Owns:** The testing apparatus of the workspace — the characterization-test methodology and its stub-interaction rules, the per-crate test inventory, the binary-level integration tests, the fixture corpus, test tooling (used vs. merely declared), the criterion bench, the manual end-to-end procedure, and the gap analysis. Product behavior itself is owned by the domain docs (see cross-links throughout).

**Sources:**
- `crates/ndex-core/tests/characterization.rs`
- `crates/ndex-protocol/tests/characterization.rs`
- `crates/ndex-store/tests/characterization.rs`
- `crates/ndex-extract/tests/characterization.rs`
- `crates/ndex-reconcile/tests/characterization.rs`
- `crates/ndex-embed/tests/characterization.rs`
- `crates/ndex-search/tests/characterization.rs`
- `crates/ndex-remote/tests/{characterization.rs,cli.rs,integration.rs,crash.rs}`
- `crates/ndex/tests/{characterization.rs,cli.rs}`
- `crates/ndex-reconcile/benches/reconcile.rs`
- `tests/fixtures/FIXTURES.md` and the `tests/fixtures/` tree
- `docs/END_TO_END.md`
- `PRD.md` §18 (testing strategy)
- Per-crate `Cargo.toml` `[dev-dependencies]`, workspace `Cargo.toml`, `mise.toml`, `hk.pkl`

---

**Status markers:** this doc's inventory statuses map onto the [README legend](README.md) as
follows — an *active* test is ✅ (implemented and exercised), an *ignored* contract test is ⛔
(pinned against a stub, pending implementation), and a *placeholder* (`todo!()`-bodied ignore) is
📋 (planned, no assertions yet). The per-crate tables in §2 carry these as active/ignored counts
rather than per-row markers.

## 1. Methodology: characterization tests

The v0.1 suite was written **before** the implementation (PR #2 `test/v0.1-characterization`
preceded PR #3 `feat/v0.1-implementation`). Every crate has one
`tests/characterization.rs` that is **black-box**: it exercises only the crate's public API and
pins the observable contract of every public function and type. The suite therefore doubles as an
executable spec — when an implementation lands, the pre-written contract test starts passing and
its `#[ignore]` is removed.

### 1.1 How stubs interact with tests

Interfaces still stubbed with `todo!()` do **not** use `#[should_panic]` (there are zero
`should_panic` tests in the workspace) and are never silently skipped. Instead they are pinned by
`#[ignore = "..."]` tests, which keeps CI green while recording the spec. Two distinct flavors
exist, distinguishable by the ignore reason string:

1. **Full-assertion contract tests** — `#[ignore = "impl pending: PR #3"]`. The body compiles
   against the real signatures and asserts the intended contract in full (e.g.
   `embedder_produces_mrl_truncated_normalized_vectors` in
   `crates/ndex-embed/tests/characterization.rs` asserts 256-dim L2-normalized output). Because
   they compile, any signature drift in the stub breaks the build even while the test is ignored.
   These pass unmodified once the implementation lands.
2. **Placeholder scenarios** — `#[ignore = "impl pending: <scenario>"]` (in
   `crates/ndex-remote/tests/integration.rs`) and `#[ignore = "skeleton: <scenario>"]` (one unit
   test in `src/`). Their bodies are a bare `todo!()`; they name a planned test scenario but
   contain no assertions. Running them with `--ignored` panics — they are bookmarks, not
   contracts.

The historical lifecycle is visible in the files: sections headed `todo!() contracts (PR #3
targets)` in `ndex-extract`, `ndex-reconcile`, and `ndex-store` characterization files contain
tests whose `#[ignore]` was removed when PR #3 implemented them; the ignores that remain
(`ndex-embed`, vector index, `Store::create/open`, the JSON renderer) mark
exactly what is still stubbed.

To enumerate the outstanding contracts:

```bash
cargo test --workspace -- --ignored --list
grep -rn 'impl pending' crates/*/tests crates/*/src
```

### 1.2 Suite totals (verified by running `cargo test --workspace`)

| Metric | Value |
|---|---|
| Passed | 272 |
| Failed | 0 |
| Ignored | 10 |
| Total | 282 |
| — in `tests/*.rs` (characterization + integration + CLI + crash harness) | 211 |
| — in `src/` `#[cfg(test)]` unit modules (29 files) | 71 |
| Doc-tests | 0 (none exist) |

The 10 ignored break down as: `ndex-embed` characterization 3, `ndex-remote/tests/integration.rs`
3, `ndex-store` characterization 2, `ndex` characterization 1, plus 1 skeleton unit test in
`src/` (`crates/ndex-store/src/vector.rs` sidecar save/load-validate).

---

## 2. Per-crate test inventory

All counts are `#[test]` functions in the named file; "ignored" counts only real `#[ignore]`
attributes (not doc-comment mentions).

| File | Tests | Ignored | Surface locked |
|---|---:|---:|---|
| `crates/ndex-core/tests/characterization.rs` | 42 | 0 | `ByteSize`/`DurationSetting` parsing and serde; `Config` defaults + TOML round-trip ([13-config](10-core/13-config.md)); `NdexError` exit-code map ([14-errors](10-core/14-errors.md)); `NdexPath` byte semantics, hashing, ordering, JSON escaping ([12-paths](10-core/12-paths.md)); `FileStatus` discriminants; domain-model serde round-trips ([11-data-model](10-core/11-data-model.md)); `IndexIdentity` TOML + schema gate; `SearchMode`/`SearchFilters` and `ProgressSink` ([15-search-and-progress-types](10-core/15-search-and-progress-types.md)); pinned constants; `TokenCounter` object safety. Round-trips use `serde_json` plus rmp-serde decode pins for the `serde(default)` forward-compat contract; byte-level MessagePack shape is owned by the protocol tests. |
| `crates/ndex-protocol/tests/characterization.rs` | 36 | 0 | 100% active (the crate has no `todo!()`). Every `ClientMessage`/`ServerMessage` variant round-trips through the real codec (`to_vec_named`/`from_slice`) — the PRD §12.4 format-stability proof; variant-count guards (9 client, 9 server) that fail when a variant is added without a sample; defaulted-struct round-trips plus cross-version decode (unknown field skipped, missing defaulted field filled, unknown enum variant rejected — the PRD §12.3 additive-evolution contract); external-tagging wire shape; bin byte shape of the hash fields incl. legacy int-array tolerance; truncated/garbage bytes error not panic; u32-BE length-prefix framing incl. `MAX_FRAME_BYTES` enforcement on both read and write, with accept-at-exactly-cap; preamble emit/scan (leading garbage, partial false starts, exact 4095/4096/4097 budget boundary, empty stream); handshake negotiation and pinned protocol constants. See [51-framing](50-protocol/51-framing.md), [52-handshake](50-protocol/52-handshake.md), [53-messages](50-protocol/53-messages.md). |
| `crates/ndex-store/tests/characterization.rs` | 26 | 3 | `MANIFEST_SCHEMA`/`META_SCHEMA` DDL executed live in in-memory SQLite with table/column/index assertions; pragmas asserted effective on a disk-backed DB (`journal_mode=wal`, `foreign_keys=1`) plus live FK-cascade enforcement; sidecar magic (and non-collision with the IPC preamble); manifest upsert (changed-only reset)/classify (incl. `Retry`)/status lifecycle/`mark_indexed`/promotion/recovery candidates; `MetaDb` doc-meta round-trip; FTS add (all fields incl. title/path_text)/commit/search/`search_with_total`/`delete_file`/snippet/read-only mode; shared+exclusive lock semantics incl. a cross-process contention test ([21-layout-and-locking](20-store/21-layout-and-locking.md), [22-manifest](20-store/22-manifest.md), [23-fts](20-store/23-fts.md)). **Ignored:** the vector-pending pair blocked on usearch ([24-vectors](20-store/24-vectors.md)) and the env-gated cross-process helper. |
| `crates/ndex-extract/tests/characterization.rs` | 17 | 0 | MIME detection (magic beats extension, NUL text heuristic, sniff window, known filenames, extension→language); BOM detect/strip and NFC normalization; UTF-16 decode; language detection + short-text guard; archive safety (unsafe member paths, compression-ratio guard, `!/` member paths, panic isolation); archive-MIME set; router construction for every MIME branch; JSON variant sniff; plaintext extraction to blocks; chunker ordering (`chunk_ord` monotone from 0); tree-sitter grammar map. See [32-extraction](30-ingest/32-extraction.md), [33-chunking](30-ingest/33-chunking.md). |
| `crates/ndex-reconcile/tests/characterization.rs` | 19 | 0 | `classify_io_error` (ENOENT ⇒ Deleted, else transient); `restat_unchanged` TOCTOU guard; `staleness` boundaries incl. clock-skew clamp; `ReconcileOptions`/`ReconcileStats`/outcome defaults; `walk` honoring `.ndexignore` and symlink containment (escape skipped); crash-safety invariant (status stays Pending until post-`fts.commit()` flip); transient retry-then-promotion; Skipped disposition (octet-stream, `max_file_size`); `only_new`; dry-run purity (writes nothing); BLAKE3 official vectors + persisted hash; `preflight_memory`/`preflight_disk`; and a real end-to-end `Store::create` → `Reconciler::run` reconcile (2 new → idempotent second run). See [31-reconcile](30-ingest/31-reconcile.md). |
| `crates/ndex-embed/tests/characterization.rs` | 10 | 3 | Model registry (arctic-only in v0.1, dims/MRL), `lookup` by short/full name, `models_dir`/`model_path` layout, `query: ` asymmetric prefix, token truncation, `MAX_QUERY_TOKENS`. **Ignored contracts:** tokenizer load/encode/count agreement, embedder producing 256-dim L2-normalized MRL vectors, `model::verify` against registry hashes ([34-embedding](30-ingest/34-embedding.md)). Note: the ignored tests reference `crates/ndex-embed/tests/fixtures/{tokenizer.json,model}`, which **do not exist yet** — they must be added when the ignores are lifted. |
| `crates/ndex-search/tests/characterization.rs` | 22 | 0 | `rrf_score` properties (both-lists reward, rank ordering, `fts_weight` scaling only the FTS term, k flattening); `min_max_normalize` (ties, singletons, empties, negatives); `ScoreExplain` default; exhaustive `mode::resolve` heuristic table incl. the empty-vector policy with exact warning strings (semantic stays `Semantic` + warning, hybrid/auto ⇒ FTS + warning, keyword/phrase/operator ⇒ FTS, natural language ⇒ hybrid); `SearchOutcome`/`Hit`; `embed_query` asymmetric-prefix contract via a `RecordingEmbed` fake (a hand-rolled test double — the only mock-style test in the suite); end-to-end `search::run` over a real `Store::create` + FTS-writer fixture (corpus-wide `total`/`truncated`, pagination incl. `limit == 0` and past-the-end offsets, semantic zero-hit short-circuit, fallback warnings in the outcome). See [41-search](40-search/41-search.md). |
| `crates/ndex-remote/tests/characterization.rs` | 9 | 0 | Wire↔engine mapping (`IndexOptions`→`ReconcileOptions`, `ReconcileStats`→`IndexStats`); progress bridging (`phase_name` covers every `ProgressKind`, `to_progress_event` with children); `unavailable_v0_2` error text; clap self-consistency (`Cli::command().debug_assert()`); subcommand parsing; `init_tracing`. See [63-remote](60-interfaces/63-remote.md). |
| `crates/ndex-remote/tests/cli.rs` | 10 | 0 | Binary-level via `assert_cmd`: `--version`, `--help` lists `serve`/`model`, v0.2 stubs (`tag`/`dedup`/`compact`) fail with exit 1 and a "planned for v0.2" stderr, `self-update` notice, zsh completions; plus binary-observed error exits pinned against [14-errors](10-core/14-errors.md) — `search`/`stats` on an index-less dir (`IndexNotFound`), re-`init` on an initialized root (`Other`), `index --max-file-size garbage` (`Config`), `index` failing fast under a held exclusive flock (`Lock`, lock held in-process via `ndex_store::IndexLock`), and `init`'s unimplemented-flag warnings ([63-remote §5.1–5.2](60-interfaces/63-remote.md)). |
| `crates/ndex-remote/tests/integration.rs` | 4 | 3 | The flagship pipeline test `init_index_search_roundtrip` (active): drives the real binary over a tempdir — `init` → `index` (asserts `2 new` / `2 processed` / `0 failed` on stdout) → FTS `search` hit → `search --format paths` → no-match query exits 0 → idempotent re-`index` (`2 unchanged`) → `stats`. **Ignored placeholders** (`todo!()` bodies): all-v0.1-formats coverage, sidecar/usearch mismatch repair, SSH transport round-trip. (SIGKILL crash recovery graduated to `tests/crash.rs`.) |
| `crates/ndex-remote/tests/crash.rs` | 1 | 0 | **The durability regression gate** (unix-only, never `#[ignore]`d): `sigkill_mid_index_then_rerun_preserves_crash_safety_invariant` generates a ~1000-file unique-token corpus, `init`s, spawns `ndex-remote index` as a child (`CARGO_BIN_EXE_ndex-remote`), polls `manifest.db` for the first batched `Indexed` commit, SIGKILLs the child mid-run (bounded retry loop, growing the corpus if a run outraces the kill), re-runs `index` to completion, then asserts the [31-reconcile](30-ingest/31-reconcile.md) crash-safety invariant directly over `manifest.db` via `rusqlite`: ≥20 randomly sampled `Indexed` files each findable via the real `search` binary with **exactly one** hit (no duplicate chunks), and indexed + skipped + failed row counts equal the corpus size with nothing left `Pending`. Runs in ~2–3 s. |
| `crates/ndex/tests/characterization.rs` | 10 | 1 | `parse_target` remote/local disambiguation ([61-client-cli](60-interfaces/61-client-cli.md)); OSC 8 hyperlinks, ANSI color constants, `detect_caps` fallbacks; the `paths` renderer (the one renderer implemented); clap self-consistency and subcommand parsing; `unavailable_v0_2`; `init_tracing`. **Ignored:** JSON renderer contract. |
| `crates/ndex/tests/cli.rs` | 5 | 0 | Binary-level: `--version`, `--help` lists core commands, v0.2 stubs exit 1, bash completions, unknown subcommand is a clap usage error (exit 2). |

### 2.1 Unit-test layer (`src/` `#[cfg(test)]`)

71 unit tests live in 29 `src` files across all nine crates (largest concentrations:
`ndex-extract` 20, `ndex-core` 13, `ndex-search` 10, `ndex-protocol` 9). They are white-box
duplicates or narrower slices of the characterization contracts (e.g.
`crates/ndex-reconcile/src/process.rs` repeats the `classify_io_error`/`restat_unchanged`
assertions). One is the ignored `skeleton:` placeholder listed in §1.2. The unit layer is
where PRD §18.1's per-module tests are expected to grow.

---

## 3. Fixture corpus (`tests/fixtures/`)

`tests/fixtures/FIXTURES.md` is the checklist: seed fixtures for v0.1 plus a TODO table of the
PRD §18.2 edge cases, to be added alongside the extractor that exercises each. The directory is
excluded from spell-checking (`typos.toml`).

Present tree:

| Path | Purpose |
|---|---|
| `tests/fixtures/text/hello.txt` | Plaintext happy path |
| `tests/fixtures/text/doc.md` | Markdown (heading + paragraph + list) |
| `tests/fixtures/edge/zero-byte.dat` | Zero-byte file — expect `status=1`, BLAKE3 of empty input |
| `tests/fixtures/edge/.ndexignore` | gitignore-compatible ignore-file semantics (PRD §11.1) |

Missing (tracked in `FIXTURES.md`'s TODO table, one row per PRD requirement): PDF (text,
scanned, encrypted), DOCX, HTML, code, JPEG/PNG, tar.gz, traversal/bomb/deep archives, UTF-16
BOM text, non-UTF-8 filename, hardlink pair, symlink cycle.

**Load-bearing caveat: no automated test currently reads `tests/fixtures/` at all.** The
integration and reconcile tests generate their own trees in `tempfile` tempdirs, and the only
fixture paths referenced from test code are the crate-relative `tests/fixtures/{tokenizer.json,
model}` in `ndex-embed`'s ignored tests — a different (nonexistent) directory. The root corpus is
seed material plus a checklist, not yet wired into any harness.

---

## 4. Test tooling: used vs. declared

| Tool | Declared | Actually used |
|---|---|---|
| **cargo-nextest** | `mise.toml` tools | Yes — the canonical runner: `mise run test` = `cargo nextest run --workspace`, invoked by the hk pre-push hook (`hk.pkl`) and CI (`.github/workflows/ci.yml` runs `mise run test`; details owned by [72-ci](70-operations/72-ci.md)). No `.config/nextest.toml` exists (all defaults). Nextest does not run doc-tests, but there are none to lose. |
| **assert_cmd + predicates** | dev-deps of `ndex`, `ndex-remote` | Yes — all binary-level tests (`cli.rs` ×2, `integration.rs`). |
| **tempfile** | dev-dep of 6 crates | Yes — every test needing a filesystem. |
| **criterion** | dev-dep of `ndex-reconcile` | Yes — one seed bench (§5). |
| **rusqlite** (as a test tool) | regular dep of `ndex-store` | Yes — in-memory connections execute the schema DDL live in store characterization tests. |
| **insta** | workspace dep; dev-dep of `ndex-core`, `ndex-extract` | **No** — zero `insta::` usage; no snapshots exist. |
| **rstest** | workspace dep; dev-dep of 6 crates | **No** — zero usage; parameterized cases are hand-rolled `for` loops over arrays. |
| **proptest** | dev-dep of `ndex-extract` | **Yes** — two property suites over the chunker (`chunk_invariants_single_block`, `chunk_invariants_across_blocks`) pin ord monotonicity, byte-range validity, and the overlap bound ([33-chunking](30-ingest/33-chunking.md)). |

---

## 5. Benchmarks

One criterion bench: `crates/ndex-reconcile/benches/reconcile.rs` (registered via `[[bench]]` in
`crates/ndex-reconcile/Cargo.toml`). It measures a single function — `classify_io_error` on a
`NotFound` error through `black_box` — and its own doc comment declares it a seed, to be extended
with walk/diff/extract/embed/search micro-benchmarks over the fixture corpus "as those paths are
implemented". It is labeled "advisory, non-blocking — PRD §18.1" and is **not run in CI** (the CI
task graph is fmt-check → lint → clippy → test; see [72-ci](70-operations/72-ci.md)). There is no
regression tracking or alerting.

---

## 6. Manual end-to-end procedure (`docs/END_TO_END.md`)

The doc gives exact command sequences in three parts. Its status banner pins the same suite counts
verified in §1.2 (217 pass / 0 fail / 14 ignored).

1. **Prerequisites** — `mise trust && mise install` (taplo, typos, actionlint, pkl, hk, nextest,
   cargo-deny), `rustup show` for the pinned toolchain, optional `mise run hooks-install`, and
   Linux native build deps (`cmake`, `clang`, `libclang-dev`, `libssl-dev`, `pkg-config`).
   Toolchain ownership: [71-toolchain](70-operations/71-toolchain.md).
2. **Automated gate** — `mise run ci` (fmt-check → lint → clippy → test), or the underlying cargo
   commands, plus the explicit flagship invocation
   `cargo test -p ndex-remote --test integration init_index_search_roundtrip` and the
   ignored-contract discovery commands (§1.1).
3. **Manual product walkthrough** — build `ndex-remote`, construct a sample tree (markdown, txt,
   rust source, `.ndexignore`), then verify, in order:
   - `init` creates `<root>/.ndex/` with `index.toml`, `config.toml`, `manifest.db`, `meta.db`,
     and the tantivy `content/` directory;
   - `index` reports `3 new … 3 processed, 0 failed`; `--status` and `--dry-run` variants;
   - re-`index` is idempotent (`3 unchanged`, size+mtime skip);
   - `search` returns a scored, snippet-highlighted hit; `--format paths` emits bare paths; `-n`
     limits; a no-match query prints `No results.` and exits 0;
   - `stats` (index path, model, file count, last-reconcile ns) and `info` (per-file manifest
     record);
   - incremental behavior: append to a file ⇒ `1 modified`; delete a file ⇒ `1 deleted` and the
     content drops out of search.

   The walkthrough exercises surface the automated suite does not (`--status`, `--dry-run`, `-n`,
   `info`, the modified/deleted increments).

The doc closes with an implemented-vs-deferred table that mirrors the `#[ignore]` inventory:
semantic search (embedder + vector index), exotic extractors, the serve loop and thin-client
transport, and maintenance commands (`reindex`, `verify`, `delete`, `config`, `checkpoint`) are
deferred.

---

## 7. Coverage gaps

Beyond the 11 explicitly-ignored contracts, the following are untested by any automated test:

- **The fixture corpus is dead weight** — nothing reads `tests/fixtures/` (§3); the zero-byte and
  `.ndexignore` edge fixtures duplicate scenarios that tests re-create in tempdirs, or are not
  exercised at all (zero-byte).
- **BLAKE3 hashing** — no known-vector test anywhere, despite PRD §18.1 naming it; the hashing
  path inside Phase 3 processing is only covered indirectly by the integration round trip.
- **BM25 field-boost math and cosine similarity on known vectors** (PRD §18.1) — the FTS test
  passes a `title_boost` but never asserts its effect; cosine is unreachable until the embedder
  lands.
- **Cross-process lock contention** — `lock_is_exclusive` uses two handles in one process; no
  second-process test.
- **Non-UTF-8 paths end-to-end** — locked in-memory (`NdexPath`, protocol `bin` encoding) but no
  test creates a non-UTF-8-named file on disk and indexes it.
- **Error-path CLI exits** — partially covered: `IndexNotFound`, `Config`, `Other`, and `Lock`
  exits are now observed from the `ndex-remote` binary (`tests/cli.rs`, [14-errors](10-core/14-errors.md));
  codes 4–7 (remote connection, version, schema mismatch, no-results) still have no binary-level test.
- **Auto-refresh / staleness behavior at the command layer** — only the pure `staleness`
  classifier is tested.
- **`index --status`, `--dry-run`, `search -n`, `info`** — manual-walkthrough-only.
- **Sidecar repair, SSH transport, serve loop** — placeholder ignores with `todo!()` bodies;
  note a blanket `cargo test -- --ignored` run panics on these rather than failing assertions.
  (Crash recovery is no longer in this list — see `crates/ndex-remote/tests/crash.rs`, §2.)
- **Performance** — the single bench measures a trivial classifier; no walk/extract/index/search
  timing exists, and nothing runs in CI.
- **Concurrency inside a run** — no test exercises parallel walk/extract workers or progress-sink
  thread-safety beyond a single-threaded collector.

---

## 8. Divergences & open questions

| # | Divergence | Detail |
|---|---|---|
| 1 | ~~PRD §18.1 "crash recovery tests" do not exist~~ **Resolved** | The live SIGKILL harness `crates/ndex-remote/tests/crash.rs` (§2) replaced the `crash_recovery_resumes_pending_files` placeholder. |
| 2 | PRD §18.1 "SSH transport tests" do not exist | Placeholder `ssh_transport_roundtrip` only; blocked on the serve loop and thin-client transport. |
| 3 | PRD §18.1 "performance regression tests … run in CI; alert on > 20% regression" is unmet | One advisory criterion seed bench, never run in CI, no corpus, no alerting (§5). |
| 4 | PRD §18.1 integration coverage "all v0.1 formats" is text-only today | The active round trip covers markdown + plaintext; PDF/DOCX/HTML/code/images/archives are behind the `all_v0_1_formats_index_correctly` placeholder. |
| 5 | PRD §18.2 corpus is ~4 of ~20 fixtures | `FIXTURES.md` tracks the rest as TODO; additionally the present fixtures are not wired into any test (§3). |
| 6 | PRD §18.3 backup/recovery has zero test coverage | The `checkpoint` command it depends on is itself deferred to a maintenance stub. |
| 7 | PRD §18.1 unit-test focus areas are partially met | Protocol round-trips: fully satisfied (28 active tests). Path handling: satisfied in-memory. Chunking boundaries, BLAKE3 vectors, BM25/cosine scoring math: missing (§7). |
| 8 | Declared-but-unused test tooling | `insta` and `rstest` (dev-deps of 6 crates) have zero usages — either adopt or drop. (`proptest` is now in use; see the tooling table.) |
| 9 | `ndex-embed`'s ignored contracts reference nonexistent fixtures | `crates/ndex-embed/tests/fixtures/{tokenizer.json,model}` must be created (or the paths repointed at the root corpus) before those ignores can be lifted. |
| 10 | Two ignore-reason dialects with different semantics | `impl pending:` marks both full contracts and `todo!()` placeholders; `skeleton:` marks only placeholders. A convention distinguishing "will pass when implemented" from "not yet written" would make `--ignored` runs meaningful. |
