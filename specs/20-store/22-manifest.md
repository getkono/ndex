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
```

(`cache_size = -262144` → 256 MiB page cache; `mmap_size` → 1 GiB.) Note `PRAGMA foreign_keys` is **never enabled** — see divergence 1.

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
- `Change` — `New` | `Modified` | `Unchanged` | `Deleted`; the Phase 2 diff outcome (PRD §11.1), never persisted.

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
| `classify(path, entry)` | `get_by_path`; `None` → `New`; equal `(size, mtime_ns)` → `Unchanged`; else `Modified`. Matches the PRD §11.1 `(size, mtime_ns)` comparison; `ctime`/`inode`/`mode` changes do not count as modifications. `Deleted` is never returned here (deletion detection is the caller's walk-set diff). | ✅ |
| `set_status(file_id, status, error)` | `UPDATE files SET status = ?, error_msg = ?, fail_count = fail_count + ?bump, last_verified_ns = now WHERE file_id = ?`; `bump` is 1 only for `FailedTransient`/`FailedPermanent`, else 0. Passing `error = None` clears `error_msg`. `last_verified_ns` is refreshed on *every* status change. | ✅ |
| `record_progress(file_id, index_name, schema_ver)` | `INSERT INTO index_progress … ON CONFLICT(file_id, index_name) DO UPDATE SET schema_ver, indexed_at_ns = now`. Two-phase-commit step 3 (PRD §11.2); a row means "committed", never "attempted". Caller currently records `index_name = "fts"` with the core `SCHEMA_VERSION` (see [reconcile](../30-ingest/31-reconcile.md)). | ✅ |
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
  size = excluded.size, mtime_ns = excluded.mtime_ns,
  ctime_ns = excluded.ctime_ns, mode = excluded.mode,
  status = 0, last_verified_ns = excluded.last_verified_ns
RETURNING file_id
```

- This is the two-phase-commit **intent write** (`status = 0` = `Pending`, PRD §11.2 step 1). `?9` is `now_ns()`.
- New rows: `uid`, `gid`, `blake3`, `mime_type`, `error_msg`, `hard_link_of`, `parent_archive_id` are left NULL (`WalkEntry` carries no uid/gid).
- Conflict (re-seen path): `first_seen_ns` is preserved; walk metadata is refreshed; `status` is reset to `Pending` **unconditionally** (see divergence 3); `fail_count`, `error_msg`, `blake3`, `mime_type`, `hard_link_of`, `parent_archive_id` are all left untouched — so a modified file temporarily keeps its previous content's `blake3`/`mime_type` while `Pending`.

## meta.db (PRD §10.4)

### Pragmas ✅ — `META_PRAGMAS`

A separate constant with values identical to the manifest's:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;
PRAGMA mmap_size = 1073741824;
```

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

The manifest is the anchor of the per-file two-phase commit (PRD §11.2): intent write (`upsert_walked`, status 0) → engine writes → `record_progress` per index → `set_status(Indexed)`. The cross-database ordering (meta.db row **before** `index_progress` **before** `status = 1`) is a caller obligation — see [reconcile](../30-ingest/31-reconcile.md); nothing in this module enforces ordering. WAL + `synchronous = NORMAL` means a power loss can drop the tail of the WAL (recent committed transactions) but not corrupt the database; the recovery invariant is "status 0 ⇒ reprocess".

## Test coverage

Characterization (`crates/ndex-store/tests/characterization.rs`):

- `manifest_schema_tables_columns_indexes` — pins all 4 tables, 15 `files` columns, and all 9 index names by executing `MANIFEST_SCHEMA` in-memory.
- `meta_schema_tables_and_lens_column` — pins the 5 meta tables plus `media_meta.lens`, `doc_meta.page_count`, `archive_meta.extraction_status`.
- `pragmas_enable_wal` — string-level only: asserts both pragma constants contain `journal_mode` and `WAL` (does **not** verify WAL is active on disk).
- `manifest_upsert_classify_and_status_lifecycle` — pins: upsert returns id > 0; fresh record is `Pending`; `set_status(Indexed)`; `classify` = `Unchanged` for identical `(size, mtime_ns)`; `touch_last_reconciliation(42)` reads back `Some(42)`; `begin_run`/`finish_run` succeed.
- `meta_db_roundtrips_doc_meta` — pins doc_meta upsert → read-back equality → `delete_file` → `None`.

Unit tests: `manifest::tests::schema_is_valid_sql`, `meta::tests::schema_is_valid_sql`, `meta::tests::media_meta_has_lens_column`.

## Divergences & open questions

1. **`REFERENCES` / `ON DELETE CASCADE` are inert.** Neither pragma set enables `PRAGMA foreign_keys = ON` (SQLite default: off), so `index_progress`'s `ON DELETE CASCADE` and the `files`→`files` references are never enforced. Deleting a `files` row would orphan its `index_progress` rows. No test verifies either way.
2. **PRD-specified `schema_info` seed rows are never written.** PRD §10.1 requires `schema_version` and `created_at` rows at init; no code inserts them (`Store::create` only runs the DDL). Only `last_reconciliation_ns` is ever written. 📋
3. **`upsert_walked` resets `status = 0` on every conflict**, but its own doc comment says "a re-seen, *changed* file is reset to Pending". An unchanged file passed through `upsert_walked` is also demoted to `Pending` and would be fully reprocessed; correctness depends on the caller only upserting new/modified paths. Doc comment and SQL disagree.
4. **Retry accounting survives modification.** The conflict path does not reset `fail_count`/`error_msg`, so a file that failed N times and is then *replaced with new content* still carries the old `fail_count` toward the permanent-failure promotion threshold (PRD §11.5). PRD is silent; likely unintended.
5. **`reconciliation_runs` stats are dead columns in v0.1** — `finish_run` writes only `completed_ns`; `timed_out`, counts, and `duration_ms` (all PRD §10.1) are never populated, which also undermines PRD §10.1's "last non-timed-out `completed_ns`" rule for `last_reconciliation_ns` (nothing can distinguish timed-out runs).
6. **`idx_path_hash` is unused by this crate.** All lookups go through the unique `path` index (`get_by_path`); the PRD §11.1 path-hash join lives in the reconciler's in-memory diff, and no SQL here filters on `path_hash`. Kept per PRD §10.1, but currently write-only cost.
7. **Silent lossy fallbacks** (unknown `status` → `Pending`, bad-length `blake3` → `None`, unparsable `last_reconciliation_ns` → `None`) mask on-disk corruption instead of surfacing it; PRD §11.5 expects corruption detection to prompt a reindex.
8. **WAL/pragma effectiveness is untested.** `pragmas_enable_wal` only greps the constant strings; nothing asserts `PRAGMA journal_mode` actually returns `wal` on a disk-backed database, or that `synchronous = NORMAL` survives.
