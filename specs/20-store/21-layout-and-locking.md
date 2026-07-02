# Store: layout, opening, and locking

**Owns:** The `Store` aggregate type, the on-disk `.ndex/` directory layout as implemented, the `index.toml` open/write helpers, the advisory write lock, and the NFS / rotational-media filesystem probes.

**Sources:** `crates/ndex-store/src/lib.rs`, `crates/ndex-store/src/lock.rs`, `crates/ndex-store/src/identity.rs`

The `ndex-store` crate depends only on `ndex-core`. All types it stores or returns (`IndexIdentity`, `Config`, `FileRecord`, `NdexPath`, error variants…) are defined there — see [data model](../10-core/11-data-model.md), [paths & constants](../10-core/12-paths.md), [config](../10-core/13-config.md), [errors](../10-core/14-errors.md).

## On-disk layout (as implemented)

Everything lives under `<root>/.ndex/`. File and directory names are the constants in `ndex-core::constants` (indexed, with their values, in [config/constants](../10-core/13-config.md)); the layout below is what the current code actually produces:

| Entry | Created by | Status |
|---|---|---|
| `.ndex/lock` | `IndexLock` (`open_lock_file`, create-if-absent) | ✅ implemented |
| `.ndex/index.toml` | `identity::write_identity` at `Store::create` | ✅ implemented |
| `.ndex/config.toml` | `Store::create` (`config.to_toml()`) | ✅ implemented |
| `.ndex/manifest.db` (+ `-wal`, `-shm`) | [`Manifest::open_or_create`](22-manifest.md) | ✅ implemented |
| `.ndex/meta.db` (+ `-wal`, `-shm`) | [`MetaDb::open_or_create`](22-manifest.md) | ✅ implemented |
| `.ndex/content/` (tantivy segment files) | [`FtsIndex::open_or_create`](23-fts.md) | ✅ implemented |
| `.ndex/vectors/` (`index.usearch`, `sidecar.bin`) | [`VectorIndex`](24-vectors.md) | 📋 planned — never created by any current code path |
| `.ndex/thumbs/` | — | 📋 planned (PRD §10.5, deferred to v0.2) |

The `.ndex.old/` sibling directory (reindex staging, PRD §5.3) is a name constant in core; no code in this crate creates or consumes it — see [remote](../60-interfaces/63-remote.md).

## The `Store` type ✅

`Store` (`crates/ndex-store/src/lib.rs`) bundles every engine for one `.ndex/` root, opened together under the index lock (PRD §10.6) — exclusive for writers (`open`/`try_open`/`create`), shared for readers (`open_read`):

| Field | Type | Notes |
|---|---|---|
| `identity` | `IndexIdentity` (pub) | verified on open, see below |
| `config` | `Config` (pub) | loaded from `config.toml`, or `Config::default()` if the file is absent |
| `manifest` | `Manifest` (pub) | [manifest doc](22-manifest.md) |
| `meta` | `MetaDb` (pub) | [manifest doc](22-manifest.md) |
| `fts` | `FtsIndex` (pub) | [FTS doc](23-fts.md) |
| `vectors` | `Option<VectorIndex>` (pub) | **always `None` in current code** — see divergences |
| `lock` | `IndexLock` (private) | held for the lifetime of the `Store` (exclusive or shared per constructor); borrowable via `Store::lock()` |
| `root` | `PathBuf` (private) | the archive root (parent of `.ndex/`); accessor `Store::root()` |

The `vectors` field is documented as `None` when the index was created with `--model none` (PRD §13.4), but both constructors unconditionally set it to `None`; the `lib.rs` doc comment on `open` states the vector index "is not yet loaded in v0.1" and searches fall back to FTS via the empty-vector path (PRD §16.3).

### `Store::open(root)` ✅ — writers

All three of `open`, `try_open`, and `open_read` share one private body (`open_with(root, LockMode)` returning `Result<Option<Store>>`; only `LockMode::TryExclusive` can produce `Ok(None)` — the blocking modes unwrap it); only steps 3 and 6 differ per mode. Exact sequence for `open` (`LockMode::Exclusive`):

1. `ndex_dir = root.join(NDEX_DIR)`. If `ndex_dir/index.toml` is **not a file** → `NdexError::IndexNotFound(root)` (exit-code semantics in [errors](../10-core/14-errors.md)).
2. `lock::detect_nfs(&ndex_dir)` → on `true`, `NdexError::Nfs(ndex_dir)`.
3. `IndexLock::acquire(&ndex_dir)` — **blocking exclusive** lock (see Locking below), for indexing/mutation use.
4. `identity::open_identity(&ndex_dir)` — loads `index.toml` and calls `IndexIdentity::check_compatible()` (schema-version gate owned by [data model](../10-core/11-data-model.md); no-migrations policy PRD §5).
5. Load `config.toml` if it is a file, else fall back silently to `Config::default()`.
6. Open engines in order: `Manifest` → `MetaDb` → `FtsIndex::open_or_create`. `vectors` is set to `None`.

### `Store::try_open(root)` ✅ — non-blocking writers

`pub fn try_open(root: &Path) -> Result<Option<Store>>`. Identical to `Store::open` except step 3 uses `IndexLock::try_acquire` (`LOCK_NB`, see Locking below): if another process holds the index lock, returns `Ok(None)` immediately instead of waiting. This is the fail-fast path for interactive writers — `ndex-remote index` uses it to error out with a clear message rather than queueing behind a running indexer ([63-remote.md §5.2](../60-interfaces/63-remote.md)) — and the intended PRD §6.2 skip-silently path for auto-refresh. Pinned cross-process by `index_fails_fast_when_lock_is_held` (`crates/ndex-remote/tests/cli.rs`).

### `Store::open_read(root)` ✅ — readers

Read-only open for the search path (PRD §11.3 concurrent readers). Identical to `Store::open` except:

- step 3 acquires the **blocking shared** lock (`IndexLock::acquire_shared`): any number of read `Store`s coexist, but the call blocks while a writer holds the exclusive lock (and symmetrically excludes writers while held);
- step 6 opens the FTS index via [`FtsIndex::open_readonly`](23-fts.md) — no tantivy `IndexWriter` is created (tantivy's own writer lock is exclusive and would make a second concurrent reader fail), so FTS write operations on a read `Store` return `NdexError::Index`.

Concurrent reads are safe by engine design: SQLite WAL supports many readers alongside a writer, and tantivy searches run against point-in-time snapshot readers. The SQLite wrappers are still opened via the normal `open_or_create` (their `IF NOT EXISTS` DDL and pragmas are effectively no-ops on an existing database). Nothing marks the SQLite connections read-only — not writing through them is a caller obligation.

### `Store::create(root, identity, config)` ✅

Exact sequence (PRD §13.4 `init`):

1. If `ndex_dir/index.toml` **exists** (any file type; note: `open` checks `is_file()`, `create` checks `exists()`) → `NdexError::Other("an index already exists at <dir>")`.
2. `std::fs::create_dir_all(&ndex_dir)` — the directory is created **before** the NFS check.
3. `lock::detect_nfs` → on `true`, `NdexError::Nfs` (leaving the just-created empty `.ndex/` behind).
4. `IndexLock::acquire`.
5. `identity::write_identity` (writes `index.toml`).
6. Write `config.toml` from `config.to_toml()`.
7. Open `Manifest` → `MetaDb` → `FtsIndex`; `vectors = None`.

Creation is **not atomic**: a failure between steps 5 and 7 leaves a partial `.ndex/` in which `index.toml` exists, so a retry of `create` fails with "already exists" while `open` would proceed (with a default config if step 6 never ran). There is no cleanup/rollback.

### Identity helpers (`identity.rs`) ✅

- `open_identity(ndex_dir)` — `IndexIdentity::load(ndex_dir/index.toml)` then `check_compatible()`; refuses to proceed on schema-version mismatch (PRD §5.3).
- `write_identity(ndex_dir, identity)` — serializes via `IndexIdentity::to_toml()` and writes `index.toml` with a plain `std::fs::write` (no temp-file/rename; the file is written once at init and treated as immutable thereafter).

PRD §5.3 additionally requires: *"If embedding model differs, disable semantic search with a warning."* No code compares the embedding section of the identity at open, and nothing disables semantic search — 📋 planned.

## Locking (`lock.rs`)

### `IndexLock` ✅

A held **advisory `flock(2)`** on `.ndex/lock`, implemented with `rustix::fs::flock` — exclusive for writers, shared for readers. One exclusive `IndexLock` guards writes to *both* SQLite databases (PRD §11.3 — single flock, both databases; the lock-holding process serializes all writes). Shared holders coexist freely with each other; shared and exclusive holders mutually exclude.

- `IndexLock::acquire(ndex_dir)` ✅ — `FlockOperation::LockExclusive`, **blocks indefinitely** until the lock is available. Failure maps to `NdexError::Lock("failed to acquire write lock: …")`.
- `IndexLock::try_acquire(ndex_dir)` ✅ — `FlockOperation::NonBlockingLockExclusive`; returns `Ok(None)` on `EWOULDBLOCK` (another holder), `Ok(Some(lock))` on success, `NdexError::Lock` on any other errno. This is the PRD §6.2 `LOCK_NB` path used by auto-refresh to skip silently when an indexer is running; consumed by `Store::try_open`.
- `IndexLock::acquire_shared(ndex_dir)` ✅ — `FlockOperation::LockShared`, **blocks** while an exclusive holder exists. Failure maps to `NdexError::Lock("failed to acquire read lock: …")`. The read-path safety argument: SQLite WAL supports concurrent readers, tantivy searches run on snapshot readers.
- `IndexLock::try_acquire_shared(ndex_dir)` ✅ — `FlockOperation::NonBlockingLockShared`; `Ok(None)` on `EWOULDBLOCK` (an exclusive holder exists), `Ok(Some(lock))` on success, `NdexError::Lock` on any other errno.
- Release: dropping the `IndexLock` drops its `File`, which releases the flock (kernel close semantics). There is no explicit unlock call, no downgrade/upgrade between modes, and the `lock` file itself is never deleted.
- Lock file open mode: `OpenOptions::new().create(true).read(true).write(true).truncate(false)` on `<ndex_dir>/lock`.

Because `flock` locks attach to the open file description, a second `open()` of the same lock file — even within the same process — contends normally. This is pinned by the characterization tests `lock_is_exclusive`, `shared_locks_coexist`, and `exclusive_excludes_shared` (`crates/ndex-store/tests/characterization.rs`): while one exclusive `IndexLock` is held, `try_acquire` and `try_acquire_shared` in the same process return `None`; while shared locks are held, more shared locks succeed but `try_acquire` returns `None`. Cross-process contention is pinned by `exclusive_lock_contends_across_processes`, which re-spawns the test executable (`cross_process_lock_helper` under `NDEX_LOCK_TEST_DIR`) to hold the exclusive lock from another process.

### `detect_nfs(path)` ✅

`rustix::fs::statfs(path)` and compares `f_type == 0x6969` (Linux `NFS_SUPER_MAGIC`). Returns `Ok(bool)`; callers (`Store::open`/`create`) abort with `NdexError::Nfs` because `flock()` cannot guarantee exclusion on NFS (PRD §11.3). Only NFS is detected — CIFS/SMB and other network filesystems pass the check.

### `is_rotational(path)` ✅ (implemented, currently uncalled)

Best-effort rotational-media probe for the PRD §6.2 auto-refresh-on-HDD opt-out:

1. `stat(path)` → `st_dev` → `(major, minor)` via `rustix::fs::{major, minor}`.
2. Read, in order, `/sys/dev/block/{major}:{minor}/queue/rotational`, then `/sys/dev/block/{major}:{minor}/../queue/rotational` (partition devices keep their queue on the parent disk).
3. First readable candidate: returns `contents.trim() == "1"`.
4. Neither readable → `Ok(false)` (treat as SSD).

No caller exists anywhere in the workspace yet — the auto-refresh disable logic that should consume it (PRD §6.2) is 📋 planned; see [reconcile](../30-ingest/31-reconcile.md).

## Concurrency model

- `Manifest`/`MetaDb` own `rusqlite::Connection`s (`Send + !Sync`); the intended owner is a single SQLite writer thread that also holds the flock (PRD §11.3) — see [manifest](22-manifest.md).
- The PRD's reader story (concurrent lock-free searches via SQLite WAL + tantivy readers + usearch mmap) is served by `Store::open_read` (shared flock, writer-less FTS): readers run concurrently with each other, but still queue behind a writer holding the exclusive lock — see divergences.

## Test coverage

- `lock_is_exclusive` (characterization) — pins `try_acquire → None` while held.
- `shared_locks_coexist` / `exclusive_excludes_shared` (characterization) — pin shared/shared coexistence, reader-blocks-writer, and writer-blocks-reader via the try variants (same process, separate file descriptions).
- `exclusive_lock_contends_across_processes` (characterization, with helper `cross_process_lock_helper`) — re-spawns the test executable to hold the exclusive lock in a child process; pins that both `try_acquire` and `try_acquire_shared` see it, and that the lock frees on child exit.
- `store_open_read_allows_concurrent_readers` (characterization) — pins two coexisting `Store::open_read` handles that can search FTS and read the manifest, FTS write rejection on a read store, writer exclusion while readers are alive, and `Store::open` succeeding after they drop.
- `store_create_then_open_roundtrips` (characterization, `#[ignore = "impl pending: PR #3"]`) — pins the intended create→drop→open roundtrip: identity equality after reopen, and `created.vectors.is_some()` for a default-model create.
- No test exercises: *blocking* `acquire`/`acquire_shared` contention (only the try variants are pinned), lock release on drop of an exclusive holder mid-process, `detect_nfs` (needs an NFS mount), `is_rotational`, partial-create recovery, or the missing-`config.toml` default fallback.

## Divergences & open questions

1. **Readers still block behind an active writer** *(reduced from "readers take the write lock" — `Store::open_read` now exists)*. PRD §11.3 mandates never-blocking readers reading the last committed WAL state; `open_read`'s blocking `acquire_shared` instead waits for a running indexer to release the exclusive lock, and a long-lived reader symmetrically stalls the next writer. A truly lock-free read path would take no flock at all.
2. **`vectors` is always `None`, but the pinned contract says otherwise.** The ignored characterization test `store_create_then_open_roundtrips` asserts `created.vectors.is_some()` for a default-model identity (matching PRD §13.4: `--model none` is the only vector-less mode), while both constructors hard-code `None` and the `open` doc comment says vectors are "not yet loaded in v0.1". Code, doc comment, test, and PRD disagree; 🚧 partial.
3. **Embedding-identity mismatch is not handled.** PRD §5.3: model mismatch should disable semantic search with a warning. `open_identity` only enforces the schema-version check. 📋
4. **`create` leaves debris on failure.** `.ndex/` is created before the NFS check; identity/config/engine creation is not transactional. A mid-create crash yields a directory that `create` rejects and `open` accepts. No stale-state cleanup exists.
5. **Existence checks differ.** `open` requires `index.toml` to be a regular file (`is_file`), `create` refuses on mere existence (`exists`); a dangling symlink or directory named `index.toml` makes the root simultaneously "not an index" (open) and "already an index" (create).
6. **NFS-only network detection.** `detect_nfs` matches only `NFS_SUPER_MAGIC`; flock on CIFS/SMB (similarly unreliable) is not rejected. The PRD's multi-line NFS remediation message (§11.3) is also not produced here — the error carries only the path (rendering owned by [errors](../10-core/14-errors.md)).
7. **`is_rotational` is dead code** pending the PRD §6.2 auto-refresh integration.
