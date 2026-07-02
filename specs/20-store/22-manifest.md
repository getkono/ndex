# SQLite databases: manifest.db and meta.db

**Owns:** The full DDL, pragmas, and statement-level semantics of the manifest database (`manifest.db`, PRD §10.1) and the metadata database (`meta.db`, PRD §10.4), plus the `RunKind`/`Change` enums.

**Sources:** `crates/ndex-store/src/manifest.rs`, `crates/ndex-store/src/meta.rs`

Both databases are opened by [`Store`](21-layout-and-locking.md) under the single `.ndex/lock` flock. Each wrapper owns one `rusqlite::Connection` (`Send + !Sync`), so each is owned by exactly one thread — the single SQLite writer thread (PRD §11.3). Row types (`FileRecord`, `WalkEntry`, `DocMeta`, `MediaMeta`, `ArchiveMeta`, `FileStatus`, `NdexPath`) are core-owned — see [data model](../10-core/11-data-model.md).

## Common machinery ✅

- **Error mapping:** every `rusqlite::Error` is mapped through `db_err` to `NdexError::Index(e.to_string())` — the engine-error family (exit-code mapping owned by [errors](../10-core/14-errors.md)).
- **Timestamps:** `now_ns()` = `jiff::Timestamp::now().as_nanosecond() as i64` — wall-clock unix nanoseconds, truncated from `i128` to `i64` (valid until year 2262). All `*_ns` columns use this.
- Dependencies: `rusqlite` (bundled SQLite) and `jiff` — version pins owned by [toolchain](../70-operations/71-toolchain.md) (workspace `Cargo.toml`).

## manifest.db (PRD §10.1)

### Pragmas ✅ — `MANIFEST_PRAGMAS`

Applied via `execute_batch` on every open:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;
PRAGMA mmap_size = 1073741824;
PRAGMA foreign_keys = ON;
```

(`cache_size = -262144` → 256 MiB page cache; `mmap_size` → 1 GiB.) `foreign_keys` is connection-local in SQLite (default off), so it is enabled on every open. With it on, the following are **live** in manifest.db:

- `index_progress.file_id REFERENCES files(file_id) ON DELETE CASCADE` — deleting a `files` row deletes its `index_progress` rows;
- the `files` self-references `hard_link_of` / `parent_archive_id` (implicit `NO ACTION`) — inserting an orphan value is rejected, and deleting a `files` row that other rows still reference fails.

Effectiveness is pinned by characterization tests `pragmas_effective_on_disk` (disk-backed db: `PRAGMA journal_mode` → `wal`, `PRAGMA foreign_keys` → `1`) and `foreign_keys_are_enforced_and_cascade` (orphan `index_progress` insert rejected; delete of a `files` row cascades).

### Schema ✅ — `MANIFEST_SCHEMA`

Applied via `execute_batch` on every open (all statements are `IF NOT EXISTS`; per the no-migrations policy PRD §5, there is no ALTER path — schema changes require a rebuild gated by the [identity check](21-layout-and-locking.md)):

```sql
CREATE TABLE IF NOT EXISTS files (
    file_id           INTEGER PRIMARY KEY,
    path              BLOB NOT NULL,
    path_hash         INTEGER NOT NULL,
    inode             INTEGER,
    dev               INTEGER,
    size              INTEGER NOT NULL,
    mtime_ns          INTEGER NOT NULL,
    ctime_ns          INTEGER NOT NULL,
    mode              INTEGER NOT NULL,
    uid               INTEGER,
    gid               INTEGER,
    blake3            BLOB,
    mime_type         TEXT,
    status            INTEGER NOT NULL DEFAULT 0,
    fail_count        INTEGER NOT NULL DEFAULT 0,
    first_seen_ns     INTEGER NOT NULL,
    last_verified_ns  INTEGER NOT NULL,
    error_msg         TEXT,
    hard_link_of      INTEGER REFERENCES files(file_id),
    parent_archive_id INTEGER REFERENCES files(file_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_path ON files(path);
CREATE INDEX IF NOT EXISTS idx_path_hash ON files(path_hash);
CREATE INDEX IF NOT EXISTS idx_status ON files(status) WHERE status NOT IN (1, 3);
CREATE INDEX IF NOT EXISTS idx_blake3 ON files(blake3) WHERE blake3 IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_mtime ON files(mtime_ns);
CREATE INDEX IF NOT EXISTS idx_mime ON files(mime_type) WHERE mime_type IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_size ON files(size);
CREATE INDEX IF NOT EXISTS idx_hard_link ON files(hard_link_of) WHERE hard_link_of IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_parent_archive ON files(parent_archive_id) WHERE parent_archive_id IS NOT NULL;

-- Presence of a row means 'successfully completed for this index', not 'attempted' (PRD §10.1).
CREATE TABLE IF NOT EXISTS index_progress (
    file_id       INTEGER NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    index_name    TEXT NOT NULL,
    schema_ver    INTEGER NOT NULL,
    indexed_at_ns INTEGER NOT NULL,
    PRIMARY KEY (file_id, index_name)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS reconciliation_runs (
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

CREATE TABLE IF NOT EXISTS schema_info (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
```

Column semantics follow PRD §10.1: `path` is platform-native bytes (`NdexPath`); `path_hash` is `NdexPath::path_hash()` (xxh3_64 of the path bytes, lookup accelerator only — see [data model](../10-core/11-data-model.md)); `status` holds `FileStatus` as its integer discriminant; `blake3` is the 32-byte content hash, NULL until processed. The partial `idx_status` index deliberately excludes statuses 1 (indexed) and 3 (deleted).

### Enums ✅

- `RunKind` — `Full` | `Incremental` | `AutoRefresh`; stored in `reconciliation_runs.kind` via `run_kind_str` as `"full"` / `"incremental"` / `"auto_refresh"` (matches PRD §10.1's enumerated values).
- `Change` — `New` | `Modified` | `Retry` | `Unchanged` | `Deleted`; the Phase 2 diff outcome (PRD §11.1), never persisted. `Retry` marks a metadata-unchanged row that still needs processing (`Pending`, or `FailedTransient` under the retry limit — PRD §11.5).

Distinctness pinned by characterization test `run_kind_and_change_are_distinct`.

### Row mapping ✅ — `row_to_record`

`SELECT *` rows map to `FileRecord` by column name. SQLite stores all integers as `i64`; unsigned fields (`path_hash`, `inode`, `dev`, `size`, `mode`, `uid`, `gid`, `fail_count`) are reinterpreted by bit pattern via `as` casts. Two lossy fallbacks:

- an out-of-range `status` value silently becomes `FileStatus::Pending` (`try_from(...).unwrap_or(Pending)`), causing reprocessing rather than an error;
- a `blake3` BLOB that is not exactly 32 bytes silently becomes `None`.

### API — `Manifest`

| Method | SQL semantics | Status |
|---|---|---|
| `open_or_create(path)` | `Connection::open`, then pragmas batch, then schema batch. | ✅ |
| `connection()` | Borrows the raw connection (single-writer-thread use only). | ✅ |
| `upsert_walked(path, entry)` | Upsert of walk metadata; see below. | ✅ |
| `get_by_path(path)` | `SELECT * FROM files WHERE path = ?1` → `Option<FileRecord>` (`QueryReturnedNoRows` → `None`). | ✅ |
| `classify(path, entry, max_retries)` | `get_by_path`; `None` → `New`; changed `(size, mtime_ns)` → `Modified` (`ctime`/`inode`/`mode` changes do not count). Unchanged metadata: status `Pending`, or `FailedTransient` with `fail_count < max_retries` → `Retry`; everything else (`Indexed`, `Skipped`, `FailedPermanent`, `Deleted`, exhausted transients) → `Unchanged`. Read-only — the promotion write is `promote_exhausted_transients`. `Change::Deleted` is never returned here (deletion detection is the caller's walk-set diff). | ✅ |
| `set_status(file_id, status, error)` | `UPDATE files SET status = ?, error_msg = ?, fail_count = fail_count + ?bump, last_verified_ns = now WHERE file_id = ?`; `bump` is 1 only for `FailedTransient`/`FailedPermanent`, else 0. Passing `error = None` clears `error_msg`. `last_verified_ns` is refreshed on *every* status change. | ✅ |
| `mark_indexed(files, index_name, schema_ver)` | Batch post-commit flip: for each `(file_id, blake3: [u8; 32])` pair, `UPDATE files SET status = 1, blake3 = ?, fail_count = 0, error_msg = NULL, last_verified_ns = now` **and** the `index_progress` upsert (same statement as `record_progress`), all inside one `unchecked_transaction`. Empty slice ⇒ no-op (no transaction). Caller obligation: run only **after** `FtsIndex::commit()` succeeds — this write is what makes `status = Indexed` mean "chunks durably committed" (PRD §11.2; ordering owned by [reconcile](../30-ingest/31-reconcile.md)). This is the only writer of the `blake3` column. | ✅ |
| `mark_deleted(file_ids)` | Batch post-commit deletion flip: per id, `UPDATE files SET status = 3, last_verified_ns = now` and `DELETE FROM index_progress WHERE file_id = ?`, in one `unchecked_transaction`. Empty slice ⇒ no-op. Same after-FTS-commit obligation as `mark_indexed`. | ✅ |
| `promote_exhausted_transients(max_retries)` | `UPDATE files SET status = 4, last_verified_ns = now WHERE status = 2 AND fail_count >= ?` → number promoted. The PRD §11.5 transient→permanent promotion; `fail_count` and `error_msg` (last failure) are preserved for diagnostics. | ✅ |
| `recovery_candidates()` | `SELECT DISTINCT ip.file_id FROM index_progress ip JOIN files f … WHERE f.status NOT IN (1, 5)` — files holding progress rows although not `Indexed`/`Skipped`; evidence of an interrupted run (consumed by `ndex-reconcile::recover`). | ✅ |
| `clear_progress(file_id)` | `DELETE FROM index_progress WHERE file_id = ?` (crash recovery and skip transitions). | ✅ |
| `record_progress(file_id, index_name, schema_ver)` | `INSERT INTO index_progress … ON CONFLICT(file_id, index_name) DO UPDATE SET schema_ver, indexed_at_ns = now`. A row means "committed", never "attempted". No longer called by the reconciler (superseded by `mark_indexed`); kept as the single-row primitive. | ✅ |
| `begin_run(kind, method)` | `INSERT INTO reconciliation_runs (started_ns, kind, method) VALUES (now, ?, ?) RETURNING run_id`. | ✅ |
| `finish_run(run_id)` | `UPDATE reconciliation_runs SET completed_ns = now WHERE run_id = ?`. **Only** `completed_ns` — the stat columns (`total_files`, `new_files`, `modified`, `deleted`, `unchanged`, `processed`, `duration_ms`, `timed_out`, `error`) have no writer anywhere in the crate. | 🚧 |
| `prune_reconciliation_runs(keep)` | `DELETE FROM reconciliation_runs WHERE run_id NOT IN (SELECT run_id … ORDER BY run_id DESC LIMIT ?keep)`. Retention count is caller-supplied; the PRD §10.1 default value lives with the caller ([reconcile](../30-ingest/31-reconcile.md)). | ✅ |
| `touch_last_reconciliation(ns)` | Upsert of `schema_info` key `'last_reconciliation_ns'`; the `i64` is stored as its decimal **TEXT** representation. Denormalized for the O(1) staleness check (PRD §6.2). | ✅ |
| `last_reconciliation_ns()` | `SELECT value … WHERE key = 'last_reconciliation_ns'`; missing row → `None`; an unparsable value also silently → `None`. | ✅ |
| `live_files()` | `SELECT file_id, path FROM files WHERE status != 3` → `Vec<(i64, NdexPath)>` — "live" includes pending/failed/skipped; used by the Phase 2 diff to detect deletions. | ✅ |
| `path_of(file_id)` | `SELECT path FROM files WHERE file_id = ?` → `Option<NdexPath>`; renders search hits. | ✅ |

#### `upsert_walked` — exact behavior ✅

```sql
INSERT INTO files
  (path, path_hash, inode, dev, size, mtime_ns, ctime_ns, mode,
   status, fail_count, first_seen_ns, last_verified_ns)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, ?9, ?9)
ON CONFLICT(path) DO UPDATE SET
  path_hash = excluded.path_hash, inode = excluded.inode, dev = excluded.dev,
  status = CASE WHEN files.size = excluded.size
                 AND files.mtime_ns = excluded.mtime_ns
           THEN files.status ELSE 0 END,
  fail_count = CASE WHEN files.size = excluded.size
                     AND files.mtime_ns = excluded.mtime_ns
               THEN files.fail_count ELSE 0 END,
  error_msg = CASE WHEN files.size = excluded.size
                    AND files.mtime_ns = excluded.mtime_ns
              THEN files.error_msg ELSE NULL END,
  size = excluded.size, mtime_ns = excluded.mtime_ns,
  ctime_ns = excluded.ctime_ns, mode = excluded.mode,
  last_verified_ns = excluded.last_verified_ns
RETURNING file_id
```

- This is the two-phase-commit **intent write** (`status = 0` = `Pending`, PRD §11.2 step 1). `?9` is `now_ns()`.
- New rows: `uid`, `gid`, `blake3`, `mime_type`, `error_msg`, `hard_link_of`, `parent_archive_id` are left NULL (`WalkEntry` carries no uid/gid).
- Conflict (re-seen path): `first_seen_ns` is preserved and walk metadata is refreshed. The `CASE` guards implement **changed-only reset**: when `(size, mtime_ns)` differ from the stored row, `status` resets to `Pending` and the retry accounting (`fail_count = 0`, `error_msg = NULL`) is cleared; when they match, all three are preserved — so re-upserting an unchanged `Indexed`/`Skipped`/failed file does not demote it, and a retried `FailedTransient` file keeps its `fail_count` toward the promotion threshold. (In a SQLite upsert every `SET` right-hand side evaluates against the pre-update row, so the `CASE`s see the old values regardless of `SET` order.)
- `blake3`, `mime_type`, `hard_link_of`, `parent_archive_id` are always left untouched — a modified file temporarily keeps its previous content's `blake3`/`mime_type` while `Pending`; the hash is corrected by `mark_indexed` when the new content commits.
- Pinned by `upsert_walked_resets_status_only_when_size_or_mtime_changed`.

## meta.db (PRD §10.4)

### Pragmas ✅ — `META_PRAGMAS`

A separate constant with values identical to the manifest's:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;
PRAGMA mmap_size = 1073741824;
PRAGMA foreign_keys = ON;
```

Within meta.db the only intra-database foreign key is `file_tags.tag_id REFERENCES tags(tag_id)` (no cascade), now enforced; the `file_id` columns remain cross-database references **by convention only** (next section).

### Schema ✅ — `META_SCHEMA`

All `file_id` columns reference `manifest.db:files(file_id)` **by convention only** — SQLite cannot enforce cross-database foreign keys; orphan cleanup is `ndex compact`'s job (v0.2, PRD §13.9). `tags`/`file_tags` exist for forward compatibility and stay empty in v0.1 (PRD §10.4).

```sql
CREATE TABLE IF NOT EXISTS doc_meta (
    file_id     INTEGER PRIMARY KEY,
    title       TEXT,
    author      TEXT,
    subject     TEXT,
    creator     TEXT,
    producer    TEXT,
    created_at  TEXT,
    modified_at TEXT,
    page_count  INTEGER,
    word_count  INTEGER,
    lang        TEXT
);

CREATE TABLE IF NOT EXISTS media_meta (
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
    taken_at      TEXT
);

CREATE TABLE IF NOT EXISTS tags (
    tag_id INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE,
    kind   TEXT NOT NULL DEFAULT 'user'
);

CREATE TABLE IF NOT EXISTS file_tags (
    file_id INTEGER NOT NULL,
    tag_id  INTEGER NOT NULL REFERENCES tags(tag_id),
    PRIMARY KEY (file_id, tag_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS archive_meta (
    file_id           INTEGER PRIMARY KEY,
    member_count      INTEGER,
    total_size        INTEGER,
    format            TEXT,
    extraction_status TEXT
);
```

### API — `MetaDb`

| Method | SQL semantics | Status |
|---|---|---|
| `open_or_create(path)` / `connection()` | Same pattern as `Manifest`. | ✅ |
| `upsert_doc_meta(file_id, meta)` | `INSERT OR REPLACE INTO doc_meta (all 11 columns)`. `REPLACE` deletes-then-inserts: the whole row is replaced, every column set from the given `DocMeta`. | ✅ |
| `upsert_media_meta(file_id, meta)` | `INSERT OR REPLACE INTO media_meta (all 18 columns)`. | ✅ |
| `upsert_archive_meta(file_id, meta)` | `INSERT OR REPLACE INTO archive_meta (all 5 columns)`. | ✅ |
| `doc_meta(file_id)` / `media_meta(file_id)` | `SELECT * … WHERE file_id = ?` → `Option<DocMeta>` / `Option<MediaMeta>` via by-name row mappers (`REAL`→`f32`, `INTEGER`→unsigned by `as` cast). No `archive_meta` reader exists. | ✅ / 🚧 (no archive reader) |
| `delete_file(file_id)` | One `unchecked_transaction` deleting the file's rows from `doc_meta`, `media_meta`, `archive_meta`, `file_tags` (in that order), then commit. `tags` rows are intentionally kept (shared dictionary). Serves `ndex delete` / re-index (PRD §13.8). | ✅ |

Numeric mapping notes: `u64` fields (`duration_ms`, `total_size`) are written with `as i64` bit-pattern casts; `f32` fields round-trip through SQLite `REAL` (`f64`) — exact for values that originated as `f32`.

## Crash-safety contract (context)

The manifest is the anchor of the batched two-phase commit (PRD §11.2): intent write (`upsert_walked`, status 0) → engine writes staged → **FTS commit** → `mark_indexed` batch (status 1 + `blake3` + `index_progress`, one transaction). The invariant this module's transactions support — `status = Indexed` ⟹ chunks durably committed in the FTS; `index_progress` row ⟹ committed for that index — plus the commit-before-flip ordering is a caller obligation — see [reconcile](../30-ingest/31-reconcile.md); nothing in this module enforces ordering. `recovery_candidates`/`clear_progress` are the repair primitives when a crash breaks the progress-row half of the invariant. WAL + `synchronous = NORMAL` means a power loss can drop the tail of the WAL (recent committed transactions) but not corrupt the database; the recovery invariant is "status 0 ⇒ reprocess".

## Test coverage

Characterization (`crates/ndex-store/tests/characterization.rs`):

- `manifest_schema_tables_columns_indexes` — pins all 4 tables, 15 `files` columns, and all 9 index names by executing `MANIFEST_SCHEMA` in-memory.
- `meta_schema_tables_and_lens_column` — pins the 5 meta tables plus `media_meta.lens`, `doc_meta.page_count`, `archive_meta.extraction_status`.
- `pragmas_enable_wal` — string-level: asserts both pragma constants contain `journal_mode`, `WAL`, and `foreign_keys`.
- `pragmas_effective_on_disk` — opens both wrappers on tempfile-backed databases and asserts `PRAGMA journal_mode` returns `wal` and `PRAGMA foreign_keys` returns `1` on the live connections.
- `foreign_keys_are_enforced_and_cascade` — pins that an orphan `index_progress` insert is rejected and that deleting a `files` row cascades to its `index_progress` rows.
- `manifest_upsert_classify_and_status_lifecycle` — pins: upsert returns id > 0; fresh record is `Pending`; a `Pending` row with unchanged metadata classifies `Retry`; after `set_status(Indexed)`, `classify` = `Unchanged` for identical `(size, mtime_ns)`; `touch_last_reconciliation(42)` reads back `Some(42)`; `begin_run`/`finish_run` succeed.
- `upsert_walked_resets_status_only_when_size_or_mtime_changed` — pins the changed-only reset: unchanged metadata preserves status/`fail_count`/`error_msg`; a size change resets to `Pending` and clears both.
- `mark_indexed_flips_status_persists_blake3_and_progress` — pins the batch flip (status 1, `blake3`, `fail_count = 0`, `error_msg = NULL`, progress row) plus the `recovery_candidates`/`clear_progress` lifecycle.
- `exhausted_transients_promote_to_permanent` — pins the `fail_count >= max_retries` promotion (diagnostics preserved) and its interplay with `classify` (`Retry` under the limit, `Unchanged` after promotion).
- `meta_db_roundtrips_doc_meta` — pins doc_meta upsert → read-back equality → `delete_file` → `None`.

Unit tests: `manifest::tests::schema_is_valid_sql`, `meta::tests::schema_is_valid_sql`, `meta::tests::media_meta_has_lens_column`.

## Divergences & open questions

*(Resolved here: foreign keys were previously never enabled — both pragma sets now set `PRAGMA foreign_keys = ON` and enforcement is tested; WAL/pragma effectiveness was previously only string-grepped — now asserted on disk-backed databases by `pragmas_effective_on_disk`. `upsert_walked` previously reset `status = 0` on every conflict, contradicting its own doc comment — the reset (plus `fail_count`/`error_msg` clearing) is now conditional on a `(size, mtime_ns)` change, so retry accounting no longer survives modification. The `blake3` column previously had no writer — `mark_indexed` now persists it.)*

1. **PRD-specified `schema_info` seed rows are never written.** PRD §10.1 requires `schema_version` and `created_at` rows at init; no code inserts them (`Store::create` only runs the DDL). Only `last_reconciliation_ns` is ever written. 📋
2. **`reconciliation_runs` stats are dead columns in v0.1** — `finish_run` writes only `completed_ns`; `timed_out`, counts, and `duration_ms` (all PRD §10.1) are never populated, which also undermines PRD §10.1's "last non-timed-out `completed_ns`" rule for `last_reconciliation_ns` (nothing can distinguish timed-out runs).
3. **`idx_path_hash` is unused by this crate.** All lookups go through the unique `path` index (`get_by_path`); the PRD §11.1 path-hash join lives in the reconciler's in-memory diff, and no SQL here filters on `path_hash`. Kept per PRD §10.1, but currently write-only cost.
4. **Silent lossy fallbacks** (unknown `status` → `Pending`, bad-length `blake3` → `None`, unparsable `last_reconciliation_ns` → `None`) mask on-disk corruption instead of surfacing it; PRD §11.5 expects corruption detection to prompt a reindex.
5. **FK enforcement constrains deletion order.** With `foreign_keys = ON`, bulk-deleting `files` rows that other `files` rows reference via `hard_link_of`/`parent_archive_id` now fails unless children are deleted or unlinked first — a new obligation on future `ndex delete`/`compact` implementations (no current code path deletes `files` rows).
