# 31 — Reconciliation Engine (`ndex-reconcile`)

**Owns:** the three-phase reconciliation engine — filesystem walk (Phase 1), manifest diff
(Phase 2), extract→index processing (Phase 3) — plus its orchestration, run options and
statistics, resource preflights, crash recovery, pre-search stale-refresh, and the crate's
benchmarks.

**Sources:**
- `crates/ndex-reconcile/src/lib.rs`
- `crates/ndex-reconcile/src/walk.rs`
- `crates/ndex-reconcile/src/diff.rs`
- `crates/ndex-reconcile/src/process.rs`
- `crates/ndex-reconcile/src/reconciler.rs`
- `crates/ndex-reconcile/src/recover.rs`
- `crates/ndex-reconcile/src/refresh.rs`
- `crates/ndex-reconcile/benches/reconcile.rs`
- `crates/ndex-reconcile/Cargo.toml`
- Tests: `crates/ndex-reconcile/tests/characterization.rs` (10 tests, all live — see
  [80-testing](../80-testing.md))

Types this crate consumes but does not own: `WalkEntry`, `DirWalkEntry`, `FileRecord`,
`FileStatus`, `SCHEMA_VERSION` ([11-data-model](../10-core/11-data-model.md)); `NdexPath`
([12-paths](../10-core/12-paths.md)); `Config` sections `walk`, `ignore`, `auto_refresh`,
`chunking` and the constants `NDEX_DIR`, `NDEX_OLD_DIR`, `NDEXIGNORE_FILE`
([13-config](../10-core/13-config.md)); `NdexError`/`Result`
([14-errors](../10-core/14-errors.md)); `ProgressSink`, `ProgressUpdate`, `ProgressKind`,
`NullSink` ([15-search-and-progress-types](../10-core/15-search-and-progress-types.md));
`Store` and `IndexLock` ([21-layout-and-locking](../20-store/21-layout-and-locking.md));
`Manifest` operations, `Change`, `RunKind`, and `MetaDb`
([22-manifest](../20-store/22-manifest.md)); `FtsIndex` operations
([23-fts](../20-store/23-fts.md)); MIME detection, extractor routing, and panic isolation
([32-extraction](32-extraction.md)); `Chunker` ([33-chunking](33-chunking.md)); the `Embed`
trait ([34-embedding](34-embedding.md)). The mapping between the wire `IndexOptions` /
`IndexStats` and this crate's `ReconcileOptions` / `ReconcileStats` is owned by
[63-remote](../60-interfaces/63-remote.md).

---

## 1. Overview and phase flow

`Reconciler::run` (`crates/ndex-reconcile/src/reconciler.rs`) drives one reconciliation
end-to-end. All three phases execute **sequentially on the calling thread**; there is no
worker pool, no channel, and no background thread anywhere in this crate in v0.1 (see
[Divergences](#divergences--open-questions) — the PRD §11.1/§11.3 pipeline is not yet built).

```
Reconciler::run(options, sink)
│
├─ begin_run(kind, "mtime")            kind = Full if options.full else Incremental
│                                       (manifest bookkeeping → 22-manifest)
├─ PHASE 1  walk(root, config)         emit ProgressKind::Walk "scanning files"
│     └─ preflight_memory(#files)      abort if est. memory > 75% of total RAM
│
├─ PHASE 2  diff(manifest, walk)       emit ProgressKind::Diff "computing changes"
│
├─ PHASE 3  (skipped when dry_run)
│     process(store, embedder, diff, sink)
│       ├─ per new/modified file:      emit ProgressKind::Extract tick
│       │    stat → upsert → read → mime → extract → chunk → FTS → meta → status
│       ├─ per deleted file_id:       FTS delete, meta delete, status=Deleted
│       └─ fts.commit()               emit ProgressKind::Fts (total, total)
│     finish_run(run_id)
│     touch_last_reconciliation(now)
│     prune_reconciliation_runs(1000)
│
└─ stats.duration_ms = elapsed        returned as ReconcileStats
```

End-to-end behavior is locked by the characterization test `full_reconcile_indexes_a_tree`:
indexing a fresh two-file tree yields `new=2, processed=2, failed=0`, and an immediate
second run yields `new=0, unchanged=2` (idempotence).

---

## 2. Phase 1 — filesystem walk (`walk.rs`)

### 2.1 `walk(root, config) -> WalkOutcome` — ✅ implemented

Traverses `root` with the `ignore` crate's **sequential** `WalkBuilder::build()` (not
`build_parallel()`; see Divergences). Builder flags map 1:1 to config (defaults owned by
[13-config](../10-core/13-config.md)):

| Builder call | Source of truth |
|---|---|
| `.hidden(!config.walk.hidden)` | hidden-file indexing; default config indexes dotfiles |
| `.parents(..)`, `.git_ignore(..)`, `.git_global(..)`, `.git_exclude(..)` | all gated on `config.ignore.respect_gitignore` |
| `.ignore(config.ignore.respect_ndexignore)` | enables the `ignore` crate's generic `.ignore` files |
| `.add_custom_ignore_filename(NDEXIGNORE_FILE)` | only when `respect_ndexignore` is true |
| `.follow_links(config.walk.follow_symlinks)` | symlink policy (§2.2) |

`.ndexignore` uses gitignore syntax per directory; as a custom ignore filename it takes
precedence over `.gitignore` in the `ignore` crate's matcher order, matching PRD §11.1's
"`.ndexignore` can un-ignore what `.gitignore` excludes". Locked by
`walk_collects_regular_files_and_honors_ignores` (a `.ndexignore` containing `*.log`
excludes `skip.log` while `keep.txt` is walked). Note that `.ignore` files are *also*
honored (side effect of `.ignore(true)`); the PRD hierarchy does not mention them.

**Self-exclusion:** a `filter_entry` closure skips any entry whose **file name** equals
`NDEX_DIR` or `NDEX_OLD_DIR` (values owned by [13-config](../10-core/13-config.md)), so the
index directory and the reindex staging copy are never descended into. Because the filter
is name-based, a user directory named `.ndex` at *any* depth is also skipped.

**Entry handling:**
- Walker errors (unreadable paths, symlink loops reported by the `ignore` crate) →
  `tracing::warn!` and skip.
- Entries without a file type (stdin/special) → skip silently.
- `entry.metadata()` failure → `tracing::warn!` and skip.
- Regular files → `outcome.files.insert(path, WalkEntry)`;
  directories → `outcome.dirs.insert(path, DirWalkEntry)` (the root itself appears in
  `dirs`).
- Everything else (unfollowed symlinks, FIFOs, sockets, devices) → skipped silently.

Paths are captured as raw bytes via `NdexPath::from_os_str` on the walker-yielded path
(root-prefixed, so absolute when the caller passes an absolute root — `Reconciler` passes
`store.root()`).

**Timestamp construction** (this crate's fact; duplicated in `process.rs::walk_entry`):
`mtime_ns = mtime() * 1_000_000_000 + mtime_nsec()`, likewise `ctime_ns`; `inode`, `dev`,
`mode`, `size` come straight from Unix `MetadataExt`. Field shapes are owned by
[11-data-model](../10-core/11-data-model.md).

### 2.2 Symlink policy — 🚧 partial

`follow_links` comes from config (default follows, PRD §11.4's `find -L` behavior). Cycle
handling is delegated to the `ignore` crate, whose loop errors surface through the
warn-and-skip path above. **Not implemented:** the PRD §11.4 rule that symlinks pointing
*outside the index root* are not followed — there is no containment check, so
`/pool/archive/link → /etc` would be followed and indexed (see Divergences).

### 2.3 `WalkOutcome` — ✅ implemented

```rust
pub struct WalkOutcome {
    pub files: DashMap<NdexPath, WalkEntry>,
    pub dirs:  DashMap<NdexPath, DirWalkEntry>,
}
```

`DashMap` anticipates a parallel walk; today it is filled from a single thread. Default
emptiness is pinned by `walk_and_diff_outcomes_default_empty`.

### 2.4 Resource preflights

- **`preflight_memory(estimated_files)` — ✅ implemented.** Errors
  (`NdexError::Other`) if `estimated_files × BYTES_PER_FILE` exceeds **75% of total RAM**
  (`rustix` `sysinfo`; `totalram × mem_unit`). `BYTES_PER_FILE = 500` (PRD §11.1 estimate).
  If `sysinfo` reports zero total RAM the check passes (never blocks on query failure).
  Called by `Reconciler::run` **after** the walk with the actual file count — PRD §11.1
  wants an estimate-based check *before* Phase 1 (see Divergences). Pinned by
  `preflight_memory_accepts_small_estimates`.
- **`preflight_disk(root, total_bytes)` — 🚧 partial.** Estimates index size as
  `total_bytes / 200` (~0.5%, PRD §11.1) and compares against `statvfs` free space
  (`f_bavail × f_frsize`); on shortfall it logs `tracing::warn!` and still returns `Ok`
  (advisory, matching PRD). **Never called by the orchestrator** — exported and pinned by
  `preflight_disk_accepts_small_estimates` only.

---

## 3. Phase 2 — manifest diff (`diff.rs`)

### 3.1 `diff(manifest, walk) -> DiffOutcome` — 🚧 partial

A **sequential** loop over `walk.files` (the doc comment claims rayon parallelization —
false; see Divergences). Each entry is classified by
[`Manifest::classify`](../20-store/22-manifest.md) (which owns the `(size, mtime_ns)`
comparison rule and the `Change` enum):

- `Change::New` → push path onto `out.new`
- `Change::Modified` → push path onto `out.modified`
- `Change::Unchanged` → `out.unchanged += 1` (count only — **no** manifest write; the
  field's doc-comment claim that `last_verified_ns` is refreshed is not implemented)
- `Change::Deleted` → ignored (never produced by `classify`)

Deletions are detected by a second pass: every `(file_id, path)` from
[`Manifest::live_files`](../20-store/22-manifest.md) (non-deleted rows) whose path is
absent from `walk.files` is pushed onto `out.deleted` as a `file_id`.

`walk.dirs` is **not consulted** — directory entries are collected in Phase 1 and then
dropped; the PRD §11.1 directory manifest rows (`mime_type='inode/directory'`) do not
exist (see Divergences).

There is no in-memory manifest `HashMap` (PRD §11.1 describes one): classification is one
SQLite point-query per walked file, plus one full `live_files` scan.

### 3.2 Hard-link handling — 📋 planned

No `(dev, inode)` tracking exists in `diff` (or in `classify`); the manifest's
`hard_link_of` column ([22-manifest](../20-store/22-manifest.md)) is never written. Each
hard-linked path is treated as an independent file and fully re-extracted. PRD §11.1
specifies canonical-`file_id` dedup with extraction skipped for duplicate inodes.

### 3.3 `DiffOutcome` — ✅ implemented

```rust
pub struct DiffOutcome {
    pub new: Vec<NdexPath>,       // on disk, not in manifest
    pub modified: Vec<NdexPath>,  // (size, mtime_ns) changed
    pub deleted: Vec<i64>,        // manifest file_ids no longer on disk
    pub unchanged: u64,           // count only
}
```

---

## 4. Phase 3 — process (`process.rs`)

### 4.1 `process(store, embedder, diff, sink) -> ReconcileStats` — 🚧 partial

Synchronous, single-threaded, **FTS-only**. The `embedder` parameter is accepted and
explicitly discarded (`let _ = embedder;`) — semantic embedding is deferred to the
vector-index follow-up, so `ProgressKind::Embed` is never emitted and no vectors are
written. There is no BLAKE3 hashing, no bounded channel, no rayon pool, and no dedicated
writer threads — the PRD §11.1 "multi-threaded pipeline with backpressure" does not exist
yet (the crate's `Cargo.toml` already declares `rayon`, `crossbeam-channel`, `memmap2`,
and `blake3` for it, all currently unused).

Iteration order: all of `diff.new`, then all of `diff.modified` (order within each vector
is `DashMap` iteration order — unspecified). Before each file, a `ProgressKind::Extract`
tick is emitted with `current` = 1-based file counter and
`total = new.len() + modified.len()`.

### 4.2 Per-file algorithm (`process_one`) — 🚧 partial

1. **Restat** via `std::fs::symlink_metadata`. On error (file vanished/unreadable between
   walk and now): `tracing::debug!`, return `Failed` — **no manifest write** (a brand-new
   file leaves no row; a modified file's old row is left untouched and will surface as
   deleted or modified on the next run).
2. **Intent write:** [`Manifest::upsert_walked`](../20-store/22-manifest.md) with the
   *fresh* stat values (that doc owns the status-reset-to-`Pending` semantics). This is
   the "status = 0 intent" half of PRD §11.2's two-phase commit.
3. **Read** the whole file with `std::fs::read` (unbounded — `max_file_size` is not
   enforced; no `Skipped` status is ever produced). On error:
   `set_status(file_id, classify_io_error(&e), Some(msg))`, return `Failed`.
4. **MIME detection** via `ndex_extract::mime::detect` ([32-extraction](32-extraction.md)).
5. **Extract** through `router(&mime)` inside `with_panic_isolation` (both owned by
   [32-extraction](32-extraction.md)), with `ExtractCtx { mime, path, tokens, depth: 0,
   config }`. Any extraction error *or caught panic* →
   `set_status(file_id, FailedPermanent, Some(msg))` and `Failed` — there is no
   transient-then-promote retry ladder (PRD §11.5 wants transient for the first
   `max_retries` attempts; see Divergences). Errors are logged at `debug`, not the PRD's
   `WARN`.
6. **Re-index:** `fts.delete_file(file_id)` then `Chunker::new(&WordTokens,
   &config.chunking).chunk(file_id, &blocks)` ([33-chunking](33-chunking.md)) and one
   `fts.add_chunk(file_id, chunk, &mime, lang)` per chunk
   ([23-fts](../20-store/23-fts.md)).
7. **Metadata:** `meta.upsert_doc_meta` / `meta.upsert_media_meta` when the extraction
   produced them ([22-manifest](../20-store/22-manifest.md)).
8. **Commit markers:** `set_status(file_id, Indexed, None)` **then**
   `record_progress(file_id, "fts", SCHEMA_VERSION)`. Note the ordering: status is
   flipped to `Indexed` *before* the `index_progress` row is written, and both happen
   *before* the run-level `fts.commit()` — the inverse of PRD §11.2 (see Divergences).

`WordTokens` (defined here) is the v0.1 token counter used for chunk sizing: token count =
`str::split_whitespace().count()`. No model tokenizer is loaded.

### 4.3 Deletions and commit — ✅ implemented (within v0.1 scope)

After the new/modified loop, for each `file_id` in `diff.deleted`:
`fts.delete_file(file_id)`, `meta.delete_file(file_id)`,
`set_status(file_id, FileStatus::Deleted, None)`, `stats.deleted += 1`. No progress ticks
are emitted for deletions.

Finally a single `ProgressKind::Fts` tick `(total, total)` is emitted and
**`store.fts.commit()` is called exactly once per run** — Tantivy durability semantics are
owned by [23-fts](../20-store/23-fts.md); the once-per-run placement (and its crash
window, §6) is this crate's fact.

### 4.4 Failure classification helpers

- **`classify_io_error(&io::Error) -> FileStatus` — ✅ implemented.**
  `ErrorKind::NotFound` → `FileStatus::Deleted` (the walk proved existence; ENOENT at read
  time means subsequently removed — retrying would be futile); anything else →
  `FileStatus::FailedTransient`. Locked by
  `enoent_means_deleted_everything_else_is_transient` (checks `PermissionDenied`, `Other`,
  `TimedOut`, `Interrupted`).
- **`restat_unchanged(&WalkEntry, size, mtime_ns) -> bool` — 🚧 partial (dead code).**
  The PRD §11.1 TOCTOU guard predicate: valid only if both `size` and `mtime_ns` match the
  Phase 1 values. Locked by `restat_requires_both_size_and_mtime_unchanged`. **Never
  called by the pipeline** — `process_one` restats *before* reading (step 1) but performs
  no post-extraction restat, so a file modified mid-extraction is indexed with stale
  content and marked `Indexed`, not re-queued as `FailedTransient`.

### 4.5 Error-handling policy — ✅ implemented (as designed for v0.1)

Two tiers:
- **Per-file soft failure:** any stat/read/extract failure for one file yields
  `Disposition::Failed`, increments `stats.failed`, and the run continues.
- **Store-level hard failure:** any error from manifest/FTS/meta operations propagates via
  `?` and **aborts the whole run** (approximates PRD §11.5 "critical errors stop
  processing"; there is no WAL-flush-then-abort special-casing).

Retry accounting: `set_status` bumps `fail_count` for failure statuses (rule owned by
[22-manifest](../20-store/22-manifest.md)), but nothing in this crate reads `fail_count`,
promotes transient→permanent at `max_retries`, or re-queues `FailedTransient` files whose
metadata is unchanged — see Divergences.

---

## 5. Orchestration (`reconciler.rs`)

### 5.1 `Reconciler` — 🚧 partial

`Reconciler::new(&mut Store, Option<&dyn Embed>)` binds an open store and an optional
embedder (`None` ⇒ no-vectors behavior; in v0.1 every caller passes `None` and Phase 3
ignores it regardless).

`run(&ReconcileOptions, &dyn ProgressSink) -> Result<ReconcileStats>` executes the flow in
§1. Facts owned here:

- Run kind: `options.full` selects `RunKind::Full`, otherwise `RunKind::Incremental`;
  the change-detection method string recorded with the run is `"mtime"`. (`full` changes
  *only* this label — there is no forced re-extraction path.)
- Phase markers: `ProgressKind::Walk` with message `"scanning files"`, `ProgressKind::Diff`
  with `"computing changes"`, each with `current=0, total=None`.
- **Dry run:** Phase 3 is skipped; stats are populated from the diff
  (`deleted = diff.deleted.len()`); `finish_run`, `touch_last_reconciliation`, and pruning
  are all skipped. The `begin_run` row **is still written** and left unfinished — a dry
  run mutates the manifest.
- On a real run, after `process`: `finish_run(run_id)`,
  `touch_last_reconciliation(now_ns)`, `prune_reconciliation_runs(RUN_HISTORY)` with
  **`RUN_HISTORY = 1000`** (matches PRD §10.1 retention).
- `stats.duration_ms = (now_ns − start).max(0) / 1_000_000`, computed for dry runs too.
- Timestamps come from `jiff::Timestamp::now().as_nanosecond()`.

### 5.2 `ReconcileOptions` — 🚧 partial

```rust
pub struct ReconcileOptions {
    pub full: bool,            // ✅ honored (RunKind label only)
    pub verify: bool,          // ⛔ accepted, never read
    pub dry_run: bool,         // ✅ honored
    pub jobs: Option<usize>,   // ⛔ accepted, never read
    pub batch_size: Option<usize>,   // ⛔ accepted, never read
    pub no_vectors: bool,      // ⛔ accepted, never read (vectors off unconditionally)
    pub max_file_size: Option<u64>,  // ⛔ accepted, never read
    pub only_new: bool,        // ⛔ accepted, never read (see §8.2)
}
```

Only `full` and `dry_run` affect behavior. `Default` is all-false/`None`, pinned by
`reconcile_options_default_is_inert`. There is no `exclude` field — PRD §11.1's
`--exclude` layer of the ignore hierarchy cannot reach the walk (the remote CLI's flag is
dropped at the mapping boundary; see [63-remote](../60-interfaces/63-remote.md)).

### 5.3 `ReconcileStats` — ✅ implemented (fields), 🚧 semantics

```rust
pub struct ReconcileStats {
    pub new: u64,        // diff count (attempted, not succeeded)
    pub modified: u64,   // diff count
    pub deleted: u64,    // deletions applied (== diff count)
    pub unchanged: u64,
    pub processed: u64,  // files reaching Indexed
    pub failed: u64,     // per-file soft failures
    pub skipped: u64,    // never incremented in v0.1
    pub duration_ms: u64,
    pub timed_out: bool, // never set true in v0.1 (no time-boxing exists)
}
```

Zeroed default pinned by `reconcile_stats_default_is_zeroed`. `new + modified =
processed + failed` on a completed run.

---

## 6. Crash safety and recovery (`recover.rs`)

### 6.1 `recover(store) -> Result<()>` — 🚧 partial (deliberate no-op)

The body ignores its argument and returns `Ok(())`. The in-code rationale: v0.1's
synchronous pipeline writes manifest/FTS/meta within a single run; SQLite (WAL) and
Tantivy each recover their own partial state on open, so there is nothing to replay. The
PRD §11.2 recovery work — resweep of `status=Pending` (intent-written) files, re-run of
indices missing an `index_progress` row, and USearch/sidecar count repair (§10.3) — is
deferred to the vector-index follow-up.

**Nothing calls `recover`** — not `Store::open`, not the `ndex-remote` command layer. Even
once implemented it would be dead until wired. The corresponding integration test
(`crash_recovery_resumes_pending_files` in `crates/ndex-remote/tests/integration.rs`) is
`#[ignore]`d.

### 6.2 Actual v0.1 crash-safety posture (this crate's facts)

- Manifest/meta writes are per-statement autocommit on WAL connections
  ([22-manifest](../20-store/22-manifest.md)); FTS writes become durable only at the
  single end-of-run `fts.commit()` (§4.3).
- Therefore a crash after a file's `set_status(Indexed)` + `record_progress` but before
  the run-level `fts.commit()` leaves rows claiming `Indexed` whose FTS documents were
  never committed. Because `recover` is a no-op and Phase 2 classifies by
  `(size, mtime_ns)` only, such files are `Unchanged` on the next run and their content is
  **silently unsearchable until the file itself changes**. This is the main open
  crash-safety hole (see Divergences); no test exercises it.

---

## 7. Concurrency model

This crate performs no locking of its own; write exclusion comes from the `IndexLock` held
by the `Store` the caller passes in (ownership and flock semantics:
[21-layout-and-locking](../20-store/21-layout-and-locking.md)). Everything in this crate
runs on the caller's thread; `&mut Store` makes single-writer a compile-time property. The
PRD §11.3 topology (rayon extraction pool, bounded crossbeam channel cap 4096, dedicated
Tantivy/embedding/SQLite-writer threads) is 📋 planned — the dependencies are declared but
unused.

---

## 8. Stale-index auto-refresh (`refresh.rs`)

### 8.1 `Staleness` + `staleness()` — ✅ implemented

```rust
pub enum Staleness { Fresh, Stale, Warn }

pub fn staleness(last_reconciled_ns: Option<i64>, now_ns: i64,
                 threshold: Duration, warn_threshold: Duration) -> Staleness
```

Classification (PRD §6.2; threshold values are config defaults owned by
[13-config](../10-core/13-config.md)):

- `None` (never reconciled) → `Warn`.
- `age = max(now − last, 0)` — a future timestamp (clock skew) clamps to age 0, i.e.
  `Fresh`.
- `age < threshold` → `Fresh`; `threshold ≤ age < warn_threshold` → `Stale`;
  otherwise `Warn`. Boundary: exactly-at-threshold is `Stale` (Fresh is strictly
  younger).

All of the above, including both boundary cases, is locked by the characterization test
`staleness_boundaries` (and duplicated in-crate by `staleness_classification`).

### 8.2 `quick_reconcile(store, budget) -> Result<()>` — 🚧 partial

Documented contract (PRD §6.2): a time-boxed Phase 1 + Phase 2 + new-files-only Phase 3
under a non-blocking (`LOCK_NB`) lock, skipping silently if a writer holds it.

Actual v0.1 body: ignores `budget` entirely and runs
`Reconciler::run(&ReconcileOptions { only_new: true, .. }, &NullSink)`. Because
`Reconciler::run` never reads `only_new` (§5.2), this is a **full incremental run**: it
processes modified files too, has no wall-clock budget, never sets `timed_out`, and does
no lock probing itself (any non-blocking acquisition would have to happen in the caller
via `IndexLock::try_acquire` — see
[21-layout-and-locking](../20-store/21-layout-and-locking.md)).

**Not wired:** nothing in the search path calls `staleness` or `quick_reconcile`. The
PRD §6.2 machinery — the pre-search staleness check, the time budget
(`auto_refresh.timeout_secs`, default owned by [13-config](../10-core/13-config.md)) with the
"indexed-before-timeout" warning, query-prioritized processing order, rotational-media
auto-disable, and the `--no-refresh` / `--refresh` / `--refresh-timeout` flags — is 📋
planned with no code outside this stub and the `auto_refresh` config section.

---

## 9. Benchmarks (`benches/reconcile.rs`) — 🚧 partial

Criterion (`harness = false`), advisory / non-blocking per PRD §18.1. One benchmark
exists: `classify_io_error` on a `NotFound` error — a nanosecond-scale seed proving the
harness works. The file's own header says to extend with walk/diff/extract/embed/search
benchmarks over the fixture corpus; none exist, so no PRD performance target is currently
measured.

---

## Divergences & open questions

Code vs PRD:

1. **Walk is sequential.** `walk.rs` uses `WalkBuilder::build()`; PRD §11.1 specifies
   `build_parallel()` with `threads(num_cpus)`. The crate-level doc in `lib.rs` also
   advertises "parallel filesystem traversal", and `WalkOutcome` uses `DashMap` — both
   anticipate parallelism that doesn't exist.
2. **Diff is sequential and its doc comment is wrong twice.** `diff.rs` claims
   "Parallelized with rayon" (it is a plain loop; `rayon` is an unused dependency) and
   claims `(dev, inode)` hard-link tracking (none exists; `hard_link_of` is never
   written — PRD §11.1 hard-link dedup is entirely missing).
3. **`unchanged` files are not touched.** `DiffOutcome.unchanged`'s doc comment says
   "their `last_verified_ns` is refreshed"; no code refreshes it — unchanged files' rows
   are never written during a run.
4. **Directories go nowhere.** Phase 1 collects `dirs`; Phase 2/3 ignore them. PRD §11.1
   requires directory manifest rows (`mime_type='inode/directory'`, `status=1`) and
   directory participation in the diff.
5. **TOCTOU guard unwired.** `restat_unchanged` exists and is tested, but `process_one`
   never restats after extraction; a file modified mid-extraction is indexed stale and
   marked `Indexed`, violating PRD §11.1's guard.
6. **Two-phase-commit ordering inverted.** Code: FTS/meta writes → `status=Indexed` →
   `index_progress` row → (much later) `fts.commit()`. PRD §11.2: index writes → progress
   rows → `status=1` last. Combined with the once-per-run FTS commit and the no-op
   `recover`, a crash before `fts.commit()` yields `Indexed` rows with no committed FTS
   docs, unrecoverable until the file changes (§6.2).
7. **`recover` is a no-op and is never called** — not from `Store::open` nor from the
   remote. PRD §11.2 recovery (Pending resweep, `index_progress` re-run, sidecar repair)
   is deferred; even the hook isn't wired.
8. **Transient failures are never retried.** A read failure marks `FailedTransient`
   *after* `upsert_walked` already stored the current `(size, mtime_ns)`, so the next
   run's diff classifies the file `Unchanged` and skips it. PRD §11.5 requires
   `status=2` files to be retried each run with promotion to `FailedPermanent` at
   `max_retries`; neither the requeue nor the promotion exists (nothing reads
   `fail_count`).
9. **Extraction errors are immediately permanent.** `process_one` sets `FailedPermanent`
   on any extraction error/panic; PRD §11.5 classifies extraction errors as transient for
   the first `max_retries` attempts.
10. **ENOENT logging/status details.** Read-time ENOENT correctly maps to `Deleted`, but
    is logged at `debug` (PRD: `INFO` with a specific message). A *stat*-time failure in
    step 1 writes no status at all and just counts as `failed`.
11. **`max_file_size` unenforced.** Whole files are slurped with `std::fs::read`
    regardless of size; PRD §11.5's `status=5` (too large → `Skipped`) is never produced
    (`ReconcileStats.skipped` is dead).
12. **Options accepted but ignored:** `verify`, `jobs`, `batch_size`, `no_vectors`,
    `max_file_size`, `only_new` (§5.2). Consequently `quick_reconcile`'s `only_new: true`
    does nothing, contradicting its own doc comment ("new-file Phase 3") and the
    `auto_refresh.index_new_only` default.
13. **`quick_reconcile` ignores `budget`**: no time-boxing, no `timed_out`, no partial
    warning; and nothing calls it or `staleness` — PRD §6.2 auto-refresh is unwired
    end-to-end (rotational-media detection and the CLI override flags also don't exist).
14. **Preflight placement.** `preflight_memory` runs *after* the walk (memory already
    consumed) against **total** RAM; PRD §11.1 says before Phase 1, from an estimate,
    against **available** RAM. `preflight_disk` is implemented but never called.
15. **Symlink containment missing.** Symlinks pointing outside the root are followed;
    PRD §11.4 forbids that (security-relevant: a link to `/etc` gets indexed).
16. **BLAKE3 never computed.** `lib.rs` and `process`'s doc comment describe an
    "extract → BLAKE3/hash → chunk → embed → index" pipeline; no hashing occurs anywhere
    (`blake3` is an unused dependency; `FileRecord.blake3` stays `NULL`; PRD §4.3
    walk-time hashing is absent).
17. **Sensitive-file heuristic (PRD §11.1)** — the post-index WARN for
    secret/credential-looking names — 📋 no code.
18. **`--exclude` cannot reach the walk** — no `ReconcileOptions` field, no
    `WalkBuilder` override hook (PRD §11.1 ignore-hierarchy item 3).
19. **`.ignore` files are honored** (via `WalkBuilder::ignore(true)`), an undocumented
    third ignore source not in the PRD hierarchy.

Code vs itself / housekeeping:

20. **Stale test-file header.** `tests/characterization.rs` opens by saying the phases
    are `todo!()` and pinned by `#[ignore = "impl pending: PR #3"]` tests; PR #3 has
    landed, all 10 tests run green, and none are ignored.
21. **Dry run writes.** `dry_run` still writes a `reconciliation_runs` row via
    `begin_run` and leaves it unfinished — a "dry" run that mutates the manifest and
    permanently records a never-finished run.
22. **Unused dependencies:** `rayon`, `crossbeam-channel`, `memmap2`, `blake3`,
    `thiserror` are declared in `crates/ndex-reconcile/Cargo.toml` and referenced nowhere
    in the crate's code.
23. **Duplicated stat→`WalkEntry` construction** in `walk.rs::file_entry` and
    `process.rs::walk_entry` — two copies of the same timestamp math.
