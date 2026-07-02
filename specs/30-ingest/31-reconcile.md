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
- Tests: `crates/ndex-reconcile/tests/characterization.rs` (19 tests, all live — see
  [80-testing](../80-testing.md))

Types this crate consumes but does not own: `WalkEntry`, `DirWalkEntry`, `FileRecord`,
`FileStatus`, `SCHEMA_VERSION` ([11-data-model](../10-core/11-data-model.md)); `NdexPath`
([12-paths](../10-core/12-paths.md)); `Config` sections `walk`, `ignore`, `auto_refresh`,
`chunking`, `extraction` (`max_file_size`, `max_retries`) and the constants `NDEX_DIR`,
`NDEX_OLD_DIR`, `NDEXIGNORE_FILE`
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
├─ warn_ignored_options(options)       tracing::warn naming verify/jobs/batch_size/
│                                       no_vectors when set (accepted-but-ignored)
├─ (skipped when dry_run — dry runs write NOTHING)
│     recover(store)                   purge uncommitted index state (§6)
│     promote_exhausted_transients     status=2, fail_count ≥ max_retries → status=4
│     begin_run(kind, "mtime")         kind = Full if options.full else Incremental
│                                       (manifest bookkeeping → 22-manifest)
├─ PHASE 1  walk(root, config)         emit ProgressKind::Walk "scanning files"
│     └─ preflight_memory(#files)      abort if est. memory > 75% of total RAM
│
├─ PHASE 2  diff(manifest, walk, max_retries)   emit ProgressKind::Diff (read-only)
│
├─ PHASE 3  (skipped when dry_run)
│     process(store, embedder, diff, options, sink)
│       ├─ per new/modified/retry file:  emit ProgressKind::Extract tick
│       │    stat → upsert → size gate → read → BLAKE3 → mime → route/skip →
│       │    extract → FTS delete-before-add → chunk → FTS stage → meta
│       │    (modified/retry files skipped entirely when options.only_new)
│       ├─ every BATCH_COMMIT_FILES successes: fts.commit() THEN mark_indexed batch
│       ├─ per deleted file_id:        FTS delete + meta delete staged
│       └─ end of run:                 emit ProgressKind::Fts (total, total),
│                                       fts.commit() THEN mark_indexed + mark_deleted
│     finish_run(run_id)
│     touch_last_reconciliation(now)
│     prune_reconciliation_runs(1000)
│
└─ stats.duration_ms = elapsed        returned as ReconcileStats
```

**Crash-safety invariant (owned here, enforced by ordering):** `status = Indexed` implies
the file's chunks are durably committed in the FTS index. Every FTS `commit()` happens
**before** the corresponding manifest status flip; a crash on either side leaves files
`Pending`, which the next run reprocesses idempotently (delete-before-add). See §4 and §6.

End-to-end behavior is locked by the characterization test `full_reconcile_indexes_a_tree`:
indexing a fresh two-file tree yields `new=2, processed=2, failed=0`, and an immediate
second run yields `new=0, unchanged=2, processed=0` (idempotence).

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

**Self-exclusion and containment:** a single `filter_entry` closure (the `ignore` crate
holds one predicate, so both rules share it) rejects:
- any entry whose **file name** equals `NDEX_DIR` or `NDEX_OLD_DIR` (values owned by
  [13-config](../10-core/13-config.md)), so the index directory and the reindex staging
  copy are never descended into. Because the filter is name-based, a user directory named
  `.ndex` at *any* depth is also skipped;
- any symlinked entry (`entry.path_is_symlink()`) whose `std::fs::canonicalize`d target
  does not start with the canonicalized root (computed once per walk;
  canonicalize-failure on the root falls back to the raw root path). Escaping symlinks —
  and, because a rejected directory entry is not descended into, their whole subtrees —
  are skipped with a `tracing::debug`; an unresolvable (broken) symlink is likewise
  skipped. Symlinks resolving *within* the root pass through and follow the normal
  `follow_links` behavior. Locked by
  `walk_skips_symlinks_escaping_the_root_but_follows_within_root`.

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

### 2.2 Symlink policy — ✅ implemented

`follow_links` comes from config (default follows, PRD §11.4's `find -L` behavior). Cycle
handling is delegated to the `ignore` crate, whose loop errors surface through the
warn-and-skip path above. The PRD §11.4 containment rule — symlinks pointing *outside the
index root* are not followed — is enforced by the `filter_entry` canonicalization check
(§2.1), so `/pool/archive/link → /etc` is skipped, not indexed. The containment check
runs regardless of `follow_symlinks` (harmless when following is off: unfollowed symlinks
were never collected anyway).

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

### 3.1 `diff(manifest, walk, max_retries) -> DiffOutcome` — 🚧 partial

A **sequential** loop over `walk.files` (its doc comment now says so). Each entry is
classified by [`Manifest::classify`](../20-store/22-manifest.md) (which owns the
`(size, mtime_ns)` comparison rule, the retry-eligibility rule, and the `Change` enum);
`max_retries` is threaded through from config `extraction.max_retries` by
`Reconciler::run`:

- `Change::New` → push path onto `out.new`
- `Change::Modified` **or** `Change::Retry` → push path onto `out.modified` (retries —
  metadata-unchanged `Pending` or under-limit `FailedTransient` rows — share the modified
  bucket and are therefore also counted in `stats.modified`)
- `Change::Unchanged` → `out.unchanged += 1` (count only — no manifest write; includes
  `Indexed`, `Skipped`, `FailedPermanent`, and exhausted transients)
- `Change::Deleted` → ignored (never produced by `classify`)

`diff` is **read-only** — the transient→permanent promotion write happens in
`Reconciler::run` before Phase 1 (§5.1), never here, so a dry run may diff safely.

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
    pub modified: Vec<NdexPath>,  // (size, mtime_ns) changed, or Pending/FailedTransient retry
    pub deleted: Vec<i64>,        // manifest file_ids no longer on disk
    pub unchanged: u64,           // count only
}
```

---

## 4. Phase 3 — process (`process.rs`)

### 4.1 `process(store, embedder, diff, options, sink) -> ReconcileStats` — 🚧 partial

Synchronous, single-threaded, **FTS-only**. The `embedder` parameter is accepted and
explicitly discarded (`let _ = embedder;`) — semantic embedding is deferred to the
vector-index follow-up, so `ProgressKind::Embed` is never emitted and no vectors are
written. There is no bounded channel, no rayon pool, and no dedicated writer threads —
the PRD §11.1 "multi-threaded pipeline with backpressure" does not exist yet (the crate's
`Cargo.toml` declares `rayon`, `crossbeam-channel`, and `memmap2` for it, still unused;
`blake3` is now used).

Inputs resolved here:
- **Effective `max_file_size`** = `options.max_file_size` if set, else config
  `extraction.max_file_size` (default owned by [13-config](../10-core/13-config.md)).
- **`options.only_new`**: when set, the modified list is not iterated **and deletions are
  not applied** — only `diff.new` files are processed (the `quick_reconcile` contract,
  §8.2). `stats.modified` still reports the diff count for visibility.

Iteration order: all of `diff.new`, then all of `diff.modified` (order within each vector
is `DashMap` iteration order — unspecified). Before each file, a `ProgressKind::Extract`
tick is emitted with `current` = 1-based file counter and `total` = the number of files
actually processed this run (`new + modified`, or just `new` under `only_new`).

**Batched commit ordering (the crash-safety invariant, §1):** successful files accumulate
as `(file_id, blake3)` pairs. Every **`BATCH_COMMIT_FILES = 100`** (public crate constant)
successes — and once at end of run — `flush_indexed` runs `fts.commit()` **first**, then
[`Manifest::mark_indexed`](../20-store/22-manifest.md) flips that batch to `Indexed`
(persisting `blake3` and the `index_progress` rows) in a single SQLite transaction.
A crash between commit and flip leaves the batch `Pending` → reprocessed next run →
delete-before-add prevents duplicate chunks. A crash before the commit loses only the
staged (uncommitted) tantivy documents.

### 4.2 Per-file algorithm (`process_one`) — ✅ implemented (FTS scope)

1. **Restat** via `std::fs::symlink_metadata`. On error (file vanished/unreadable between
   walk and now): `tracing::debug!`, return `Failed` — **no manifest write** (a brand-new
   file leaves no row; a modified file's old row is left untouched and will surface as
   deleted or modified on the next run).
2. **Intent write:** [`Manifest::upsert_walked`](../20-store/22-manifest.md) with the
   *fresh* stat values (that doc owns the changed-only status-reset semantics). This is
   the "status = 0 intent" half of PRD §11.2's two-phase commit.
3. **Size gate:** if the restat size exceeds the effective `max_file_size`, the file is
   **skipped without ever being read** (PRD §11.5: too large → `Skipped`): see the skip
   path below. Locked by `oversized_files_are_skipped_without_reading`.
4. **Read** the whole file with `std::fs::read`. On error: `tracing::warn!`, then
   `set_status(file_id, classify_io_error(&e), Some(msg))`, return `Failed`.
5. **BLAKE3:** `blake3::hash(&bytes)` over the whole-file read; carried in the
   `Indexed` disposition and persisted by the post-commit `mark_indexed` batch (single
   write path). Hash correctness is pinned against the official vectors (empty input,
   `"abc"`) by `blake3_matches_official_known_vectors`; persistence by
   `blake3_hash_is_persisted_for_indexed_files`.
6. **MIME detection** via `ndex_extract::mime::detect` ([32-extraction](32-extraction.md)).
7. **Route:** `router(&mime)` ([32-extraction](32-extraction.md)) returns
   `Route::Extract(extractor)` or `Route::Skip` (unmatched MIME, incl.
   `application/octet-stream`). `Route::Skip` → the skip path below. Locked by
   `unsupported_mime_is_skipped_with_status_5`.
8. **Extract** inside `with_panic_isolation` with `ExtractCtx { mime, path, tokens,
   depth: 0, config }`. Any extraction error *or caught panic* → `tracing::warn!` and
   `set_status(file_id, FailedTransient, Some(msg))`, return `Failed` — per PRD §11.5,
   extraction errors are transient for the first `max_retries` attempts; the promotion
   to `FailedPermanent` happens at the start of a later run (§5.1).
9. **Re-index (delete-before-add):** `fts.delete_file(file_id)` then
   `Chunker::new(&WordTokens, &config.chunking).chunk(file_id, &blocks)`
   ([33-chunking](33-chunking.md)) and one `fts.add_chunk(file_id, chunk, &meta)` per
   chunk, where `meta` is an [`FtsFileMeta`](../20-store/23-fts.md) built from the
   detected mime, extraction `lang`, `path.display_lossy()`, restat `size`/`mtime_ns`,
   and `doc_meta.title` when present.
10. **Metadata:** `meta.upsert_doc_meta` / `meta.upsert_media_meta` when the extraction
    produced them ([22-manifest](../20-store/22-manifest.md)).
11. **Return `Indexed { file_id, blake3 }`** — no status write here; the flip to
    `Indexed` is deferred to the post-commit batch (§4.1). Pinned by the unit test
    `status_stays_pending_until_post_commit_flip` (drives `process_one` without a commit
    and asserts the status is still `Pending` with no progress row and no `blake3`).

**Skip path** (steps 3 and 7): `fts.delete_file(file_id)` (purges stale chunks if a
previously indexed file transitioned to skippable), `set_status(file_id, Skipped,
Some(reason))` (the reason — oversize or "no extractor for mime …" — lands in
`error_msg`), `Manifest::clear_progress(file_id)`, return `Skipped` → `stats.skipped += 1`.
No content is written to the FTS and no progress row remains. A `Skipped` file with
unchanged metadata is `Unchanged` on later runs — even if the effective `max_file_size`
changes, it is only reconsidered when the file itself changes.

`WordTokens` (defined here) is the v0.1 token counter used for chunk sizing: token count =
`str::split_whitespace().count()`. No model tokenizer is loaded.

### 4.3 Deletions and commit — ✅ implemented (within v0.1 scope)

After the processing loop (and only when not `only_new`), for each `file_id` in
`diff.deleted`: `fts.delete_file(file_id)`, `meta.delete_file(file_id)`,
`stats.deleted += 1` — the status flip is **staged, not applied**. No progress ticks are
emitted for deletions.

Finally a single `ProgressKind::Fts` tick `(total, total)` is emitted, the run-final
`flush_indexed` runs (`fts.commit()` then `mark_indexed` for the tail batch), and
[`Manifest::mark_deleted`](../20-store/22-manifest.md) flips the deleted rows to
`Deleted` and drops their progress rows — the same commit-before-flip ordering as
indexing. Tantivy durability semantics are owned by [23-fts](../20-store/23-fts.md); the
batch placement is this crate's fact.

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

### 4.5 Error-handling policy — ✅ implemented (PRD §11.5 within FTS scope)

Two tiers:
- **Per-file soft failure:** any stat/read/extract failure for one file yields
  `Disposition::Failed`, increments `stats.failed`, and the run continues. Read and
  extraction failures log at `WARN` (PRD §11.5 logging tier); stat-time failures at
  `debug`.
- **Store-level hard failure:** any error from manifest/FTS/meta operations propagates via
  `?` and **aborts the whole run** (approximates PRD §11.5 "critical errors stop
  processing"; there is no WAL-flush-then-abort special-casing).

Retry accounting (PRD §11.5): `set_status` bumps `fail_count` on each failure (rule owned
by [22-manifest](../20-store/22-manifest.md)); metadata-unchanged `Pending`/under-limit
`FailedTransient` rows are re-queued by the diff (§3.1); rows with `fail_count ≥
max_retries` are promoted to `FailedPermanent` by the pre-walk sweep (§5.1) instead of
being retried; a success clears the streak (`mark_indexed` resets `fail_count`/
`error_msg`); a metadata change resets it (`upsert_walked` changed-only reset). Locked by
`transient_failure_is_retried_when_under_the_limit` and
`exhausted_transient_failure_is_promoted_not_retried`.

---

## 5. Orchestration (`reconciler.rs`)

### 5.1 `Reconciler` — 🚧 partial

`Reconciler::new(&mut Store, Option<&dyn Embed>)` binds an open store and an optional
embedder (`None` ⇒ no-vectors behavior; in v0.1 every caller passes `None` and Phase 3
ignores it regardless).

`run(&ReconcileOptions, &dyn ProgressSink) -> Result<ReconcileStats>` executes the flow in
§1. Facts owned here:

- **Ignored-option honesty:** before anything else, one `tracing::warn!` names every set
  option among `verify`/`jobs`/`batch_size`/`no_vectors` (accepted-but-ignored, §5.2),
  once per run.
- **Non-dry preamble (in order):** `recover(store)` (§6), then
  `promote_exhausted_transients(extraction.max_retries)` (a `tracing::info!` reports a
  non-zero promotion count), then `begin_run`. Promotion runs **before** the diff so an
  exhausted file is neither retried this run nor misclassified later.
- Run kind: `options.full` selects `RunKind::Full`, otherwise `RunKind::Incremental`;
  the change-detection method string recorded with the run is `"mtime"`. (`full` changes
  *only* this label — there is no forced re-extraction path.)
- Phase markers: `ProgressKind::Walk` with message `"scanning files"`, `ProgressKind::Diff`
  with `"computing changes"`, each with `current=0, total=None`.
- **Dry run:** pure — recovery, promotion, `begin_run`, Phase 3, `finish_run`,
  `touch_last_reconciliation`, and pruning are all skipped; stats are populated from the
  diff (`deleted = diff.deleted.len()`). A dry run performs **no write of any kind**
  (walk and diff are read-only). Locked by `dry_run_reports_but_writes_nothing` (empty
  `files` and `reconciliation_runs` tables, no `last_reconciliation_ns`).
- On a real run, after `process`: `finish_run(run_id)`,
  `touch_last_reconciliation(now_ns)`, `prune_reconciliation_runs(RUN_HISTORY)` with
  **`RUN_HISTORY = 1000`** (matches PRD §10.1 retention).
- `stats.duration_ms = (now_ns − start).max(0) / 1_000_000`, computed for dry runs too.
- Timestamps come from `jiff::Timestamp::now().as_nanosecond()`.

### 5.2 `ReconcileOptions` — 🚧 partial

```rust
pub struct ReconcileOptions {
    pub full: bool,            // ✅ honored (RunKind label only)
    pub verify: bool,          // ⛔ accepted, ignored (warned, §5.1)
    pub dry_run: bool,         // ✅ honored (pure runs, §5.1)
    pub jobs: Option<usize>,   // ⛔ accepted, ignored (warned)
    pub batch_size: Option<usize>,   // ⛔ accepted, ignored (warned)
    pub no_vectors: bool,      // ⛔ accepted, ignored (warned; vectors off unconditionally)
    pub max_file_size: Option<u64>,  // ✅ honored (overrides config extraction.max_file_size)
    pub only_new: bool,        // ✅ honored (new files only, §4.1, §8.2)
}
```

`Default` is all-false/`None`, pinned by `reconcile_options_default_is_inert`. There is
no `exclude` field — PRD §11.1's `--exclude` layer of the ignore hierarchy cannot reach
the walk (the remote CLI's flag is dropped at the mapping boundary; see
[63-remote](../60-interfaces/63-remote.md)).

### 5.3 `ReconcileStats` — ✅ implemented

```rust
pub struct ReconcileStats {
    pub new: u64,        // diff count (attempted, not succeeded)
    pub modified: u64,   // diff count: metadata changes + retries (reported even under only_new)
    pub deleted: u64,    // deletions applied (0 under only_new)
    pub unchanged: u64,
    pub processed: u64,  // files reaching Indexed
    pub failed: u64,     // per-file soft failures
    pub skipped: u64,    // Skipped dispositions (unsupported mime / oversize)
    pub duration_ms: u64,
    pub timed_out: bool, // never set true in v0.1 (no time-boxing exists)
}
```

Zeroed default pinned by `reconcile_stats_default_is_zeroed`. `new + modified =
processed + failed + skipped` on a completed full run (not under `only_new`, where
modified files are reported but not attempted).

---

## 6. Crash safety and recovery (`recover.rs`)

### 6.1 `recover(store) -> Result<()>` — ✅ implemented (FTS scope)

Called by `Reconciler::run` at the start of **every non-dry run** (§5.1). Contract:
restore the invariant pair *`status = Indexed` ⟺ chunks durably committed* and
*`index_progress` row ⟹ committed for that index*.

Algorithm:
1. [`Manifest::recovery_candidates`](../20-store/22-manifest.md) — files holding
   `index_progress` rows although their status is **not** `Indexed`/`Skipped` (e.g. a
   previously indexed file reset to `Pending` for reprocessing, or staged `Deleted`,
   when the run died before/after the FTS commit but before the flip).
2. If none (the common clean-index case): return without any write.
3. `fts.delete_file` for each candidate, **then** `fts.commit()` — the purge is made
   durable *before* step 4, so a crash inside `recover` leaves the candidates detectable
   and the next run redoes the idempotent purge.
4. `Manifest::clear_progress` per candidate, and a `tracing::info!` with the count.

Statuses are left as-is: non-`Indexed` files are picked up by the following walk/diff
(`Pending`/`FailedTransient` → `Retry`), and Phase 3's delete-before-add makes the
reprocess safe. USearch/sidecar count repair (PRD §10.3) remains deferred to the
vector-index follow-up. The full kill−9 harness lives with the `ndex-remote` integration
tests.

### 6.2 v0.1 crash-safety posture (this crate's facts)

- Manifest/meta writes are per-statement (or per-batch-transaction) autocommit on WAL
  connections ([22-manifest](../20-store/22-manifest.md)); FTS writes become durable only
  at a `fts.commit()` — which Phase 3 issues per `BATCH_COMMIT_FILES` batch and at end of
  run, always **before** the corresponding `mark_indexed`/`mark_deleted` flip (§4.1,
  §4.3).
- Crash before a batch's `fts.commit()`: staged tantivy documents are discarded on
  reopen; the batch's files are still `Pending` → retried. Crash between `fts.commit()`
  and the flip: files are `Pending` with committed chunks → retried, delete-before-add
  removes the duplicates; if such a file is meanwhile deleted from disk, the deletion
  path (or `recover`, via its lingering progress row when it had one) purges the chunks.
- Known residual window: the **skip transition** purge (§4.2 skip path) clears the
  progress row *before* the run's next FTS commit; a crash in between leaves a `Skipped`
  file's stale chunks committed with no progress row to flag them — they persist until
  the file changes or is deleted. Narrow (requires a previously indexed file turning
  skippable plus a crash in the window) and self-limiting; accepted for v0.1.

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

Actual v0.1 body: ignores `budget` and runs
`Reconciler::run(&ReconcileOptions { only_new: true, .. }, &NullSink)` — a genuine
**new-files-only** pass now that `only_new` is honored (§4.1; locked by
`only_new_processes_new_files_but_not_modified_ones`): modified files and deletions are
diffed and reported but not processed. Still missing: the wall-clock budget (`timed_out`
is never set) and lock probing (any non-blocking acquisition would have to happen in the
caller via `IndexLock::try_acquire` — see
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

*(Resolved in the crash-safety/retry rework: the two-phase-commit ordering inversion —
`fts.commit()` now always precedes the batched `mark_indexed`/`mark_deleted` status
flips (§4.1, §4.3); `recover` is implemented and called at the start of every non-dry
run (§6.1); transient failures are re-queued each run and promoted to `FailedPermanent`
at `max_retries` (§3.1, §5.1); extraction errors are now transient per PRD §11.5;
`max_file_size` is enforced before reading with `Skipped`/`status=5` produced (§4.2);
`only_new` and `max_file_size` options are honored and the remaining ignored options are
warned about (§5.2); symlinks escaping the root are no longer followed (§2.1–2.2);
BLAKE3 is computed per file and persisted via `mark_indexed` (§4.2); dry runs no longer
write anything (§5.1); the stale characterization-test header is rewritten; the diff doc
comment no longer claims rayon or hard-link tracking.)*

Code vs PRD:

1. **Walk is sequential.** `walk.rs` uses `WalkBuilder::build()`; PRD §11.1 specifies
   `build_parallel()` with `threads(num_cpus)`. The crate-level doc in `lib.rs` also
   advertises "parallel filesystem traversal", and `WalkOutcome` uses `DashMap` — both
   anticipate parallelism that doesn't exist.
2. **Diff is sequential and hard-link dedup is missing.** `diff.rs` is a plain loop (its
   doc comment now says so); no `(dev, inode)` tracking exists and `hard_link_of` is
   never written — PRD §11.1 hard-link dedup is entirely missing.
3. **`unchanged` files are not touched.** PRD §11.1 refreshes `last_verified_ns` for
   verified-unchanged files; no code does — unchanged rows are never written during a
   run (the `DiffOutcome.unchanged` doc comment now states the count-only behavior).
4. **Directories go nowhere.** Phase 1 collects `dirs`; Phase 2/3 ignore them. PRD §11.1
   requires directory manifest rows (`mime_type='inode/directory'`, `status=1`) and
   directory participation in the diff.
5. **TOCTOU guard unwired.** `restat_unchanged` exists and is tested, but `process_one`
   never restats after extraction; a file modified mid-extraction is indexed stale and
   marked `Indexed`, violating PRD §11.1's guard.
6. **Unsupported formats are `Skipped` (5), not `FailedPermanent` (4).** PRD §11.5's
   failure table lists "unsupported format → status 4", but PRD §4.8 says octet-stream ⇒
   `status=5` — the PRD is internally inconsistent. Code follows §4.8: `Route::Skip` ⇒
   `Skipped` with the reason in `error_msg` (§4.2), which also keeps "failed" semantics
   (something is wrong) distinct from "skipped" (deliberate policy).
7. **Skip-transition crash window.** A previously indexed file that turns skippable has
   its progress row cleared before the purge commits; a crash in between leaves stale
   committed chunks undetectable by `recover` (§6.2). Accepted for v0.1.
8. **ENOENT logging/status details.** Read-time ENOENT correctly maps to `Deleted`, but
   is logged at `warn` with a generic message (PRD: `INFO` with a specific message). A
   *stat*-time failure in step 1 writes no status at all and just counts as `failed`.
9. **Options accepted but ignored:** `verify`, `jobs`, `batch_size`, `no_vectors`
   (§5.2) — now surfaced by a once-per-run `tracing::warn`, but still without effect.
10. **`quick_reconcile` ignores `budget`**: no time-boxing, no `timed_out`, no partial
    warning; and nothing calls it or `staleness` — PRD §6.2 auto-refresh is unwired
    end-to-end (rotational-media detection and the CLI override flags also don't exist).
11. **Preflight placement.** `preflight_memory` runs *after* the walk (memory already
    consumed) against **total** RAM; PRD §11.1 says before Phase 1, from an estimate,
    against **available** RAM. `preflight_disk` is implemented but never called.
12. **BLAKE3 is process-time, not walk-time.** PRD §4.3 describes hashing during the
    walk; code hashes the whole-file read in `process_one` (§4.2) — cheaper (one read)
    and only for files that reach processing, but the hash lands only when a file
    indexes successfully (failed/skipped files keep a NULL or stale `blake3`).
13. **Sensitive-file heuristic (PRD §11.1)** — the post-index WARN for
    secret/credential-looking names — 📋 no code.
14. **`--exclude` cannot reach the walk** — no `ReconcileOptions` field, no
    `WalkBuilder` override hook (PRD §11.1 ignore-hierarchy item 3).
15. **`.ignore` files are honored** (via `WalkBuilder::ignore(true)`), an undocumented
    third ignore source not in the PRD hierarchy.

Code vs itself / housekeeping:

16. **Unused dependencies:** `rayon`, `crossbeam-channel`, `memmap2`, `thiserror` are
    declared in `crates/ndex-reconcile/Cargo.toml` and referenced nowhere in the crate's
    code (`blake3` is now used).
17. **Duplicated stat→`WalkEntry` construction** in `walk.rs::file_entry` and
    `process.rs::walk_entry` — two copies of the same timestamp math.
