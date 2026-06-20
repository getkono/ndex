//! The manifest database `manifest.db` (PRD §10.1).

use std::path::Path;

use ndex_core::error::{NdexError, Result};
use ndex_core::model::{FileRecord, WalkEntry};
use ndex_core::path::NdexPath;
use ndex_core::status::FileStatus;
use rusqlite::{Connection, Row, params};

/// Map a rusqlite error into the crate error type (the engine-error family, exit code 1).
fn db_err(e: rusqlite::Error) -> NdexError {
    NdexError::Index(e.to_string())
}

/// Current wall-clock time in unix nanoseconds (manifest timestamps, PRD §10.1).
fn now_ns() -> i64 {
    jiff::Timestamp::now().as_nanosecond() as i64
}

/// Connection pragmas applied at open (PRD §10.1).
pub const MANIFEST_PRAGMAS: &str = "\
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;
PRAGMA mmap_size = 1073741824;
";

/// Table and index DDL for the manifest (PRD §10.1).
pub const MANIFEST_SCHEMA: &str = "\
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
";

/// Kind of a reconciliation run (PRD §10.1 `reconciliation_runs.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Full,
    Incremental,
    AutoRefresh,
}

/// Diff outcome for a walked path during Phase 2 (PRD §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    New,
    Modified,
    Unchanged,
    Deleted,
}

/// The manifest database: owns a single `rusqlite::Connection`.
///
/// `Connection` is `Send + !Sync`, so a `Manifest` is owned by exactly one thread — the
/// single SQLite writer thread that also holds the `.ndex/lock` flock (PRD §11.3).
pub struct Manifest {
    conn: rusqlite::Connection,
}

/// Stable string for a [`RunKind`] (stored in `reconciliation_runs.kind`).
fn run_kind_str(kind: RunKind) -> &'static str {
    match kind {
        RunKind::Full => "full",
        RunKind::Incremental => "incremental",
        RunKind::AutoRefresh => "auto_refresh",
    }
}

/// Map a `files` row into a [`FileRecord`]. SQLite stores all integers as `i64`; unsigned
/// columns (`path_hash`, `inode`, `dev`, `size`, `mode`, `uid`, `gid`) are reinterpreted by bit
/// pattern via `as`.
fn row_to_record(row: &Row<'_>) -> rusqlite::Result<FileRecord> {
    let path: Vec<u8> = row.get("path")?;
    let blake3: Option<Vec<u8>> = row.get("blake3")?;
    let status: i64 = row.get("status")?;
    Ok(FileRecord {
        file_id: row.get("file_id")?,
        path: NdexPath::new(path),
        path_hash: row.get::<_, i64>("path_hash")? as u64,
        inode: row.get::<_, Option<i64>>("inode")?.map(|v| v as u64),
        dev: row.get::<_, Option<i64>>("dev")?.map(|v| v as u64),
        size: row.get::<_, i64>("size")? as u64,
        mtime_ns: row.get("mtime_ns")?,
        ctime_ns: row.get("ctime_ns")?,
        mode: row.get::<_, i64>("mode")? as u32,
        uid: row.get::<_, Option<i64>>("uid")?.map(|v| v as u32),
        gid: row.get::<_, Option<i64>>("gid")?.map(|v| v as u32),
        blake3: blake3.and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()),
        mime_type: row.get("mime_type")?,
        status: FileStatus::try_from(status as u8).unwrap_or(FileStatus::Pending),
        fail_count: row.get::<_, i64>("fail_count")? as u32,
        first_seen_ns: row.get("first_seen_ns")?,
        last_verified_ns: row.get("last_verified_ns")?,
        error_msg: row.get("error_msg")?,
        hard_link_of: row.get("hard_link_of")?,
        parent_archive_id: row.get("parent_archive_id")?,
    })
}

impl Manifest {
    /// Open (creating if absent) the manifest at `path`, applying pragmas + schema.
    pub fn open_or_create(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch(MANIFEST_PRAGMAS).map_err(db_err)?;
        conn.execute_batch(MANIFEST_SCHEMA).map_err(db_err)?;
        Ok(Self { conn })
    }

    /// Borrow the underlying connection (single-writer-thread use only).
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }

    /// Insert/update a walked file's metadata, returning its `file_id` (two-phase commit
    /// intent write, PRD §11.2). New entries are inserted `status = Pending`; a re-seen,
    /// changed file is reset to `Pending` so it is reprocessed.
    pub fn upsert_walked(&self, path: &NdexPath, entry: &WalkEntry) -> Result<i64> {
        let now = now_ns();
        self.conn
            .query_row(
                "INSERT INTO files \
                   (path, path_hash, inode, dev, size, mtime_ns, ctime_ns, mode, \
                    status, fail_count, first_seen_ns, last_verified_ns) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, ?9, ?9) \
                 ON CONFLICT(path) DO UPDATE SET \
                   path_hash = excluded.path_hash, inode = excluded.inode, dev = excluded.dev, \
                   size = excluded.size, mtime_ns = excluded.mtime_ns, \
                   ctime_ns = excluded.ctime_ns, mode = excluded.mode, \
                   status = 0, last_verified_ns = excluded.last_verified_ns \
                 RETURNING file_id",
                params![
                    path.as_bytes(),
                    path.path_hash() as i64,
                    entry.inode as i64,
                    entry.dev as i64,
                    entry.size as i64,
                    entry.mtime_ns,
                    entry.ctime_ns,
                    entry.mode as i64,
                    now,
                ],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    /// Look up a file record by path.
    pub fn get_by_path(&self, path: &NdexPath) -> Result<Option<FileRecord>> {
        self.conn
            .query_row(
                "SELECT * FROM files WHERE path = ?1",
                params![path.as_bytes()],
                row_to_record,
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(db_err(other)),
            })
    }

    /// Classify a walked path against the manifest (Phase 2 diff, PRD §11.1).
    pub fn classify(&self, path: &NdexPath, entry: &WalkEntry) -> Result<Change> {
        match self.get_by_path(path)? {
            None => Ok(Change::New),
            Some(rec) if rec.size == entry.size && rec.mtime_ns == entry.mtime_ns => {
                Ok(Change::Unchanged)
            }
            Some(_) => Ok(Change::Modified),
        }
    }

    /// Set a file's status (and optional error message), bumping `fail_count` for failures.
    pub fn set_status(&self, file_id: i64, status: FileStatus, error: Option<&str>) -> Result<()> {
        let bump = i64::from(matches!(
            status,
            FileStatus::FailedTransient | FileStatus::FailedPermanent
        ));
        self.conn
            .execute(
                "UPDATE files \
                 SET status = ?1, error_msg = ?2, fail_count = fail_count + ?3, \
                     last_verified_ns = ?4 \
                 WHERE file_id = ?5",
                params![i64::from(status.as_u8()), error, bump, now_ns(), file_id],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Record that `index_name` was committed for `file_id` (PRD §11.2 `index_progress`).
    pub fn record_progress(&self, file_id: i64, index_name: &str, schema_ver: u32) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO index_progress (file_id, index_name, schema_ver, indexed_at_ns) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(file_id, index_name) DO UPDATE SET \
                   schema_ver = excluded.schema_ver, indexed_at_ns = excluded.indexed_at_ns",
                params![file_id, index_name, i64::from(schema_ver), now_ns()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Begin a reconciliation run row, returning its `run_id`.
    pub fn begin_run(&self, kind: RunKind, method: &str) -> Result<i64> {
        self.conn
            .query_row(
                "INSERT INTO reconciliation_runs (started_ns, kind, method) \
                 VALUES (?1, ?2, ?3) RETURNING run_id",
                params![now_ns(), run_kind_str(kind), method],
                |r| r.get(0),
            )
            .map_err(db_err)
    }

    /// Finalize a reconciliation run row.
    pub fn finish_run(&self, run_id: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE reconciliation_runs SET completed_ns = ?1 WHERE run_id = ?2",
                params![now_ns(), run_id],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Retain only the most recent `keep` reconciliation runs (PRD §10.1 retention).
    pub fn prune_reconciliation_runs(&self, keep: u32) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM reconciliation_runs WHERE run_id NOT IN \
                 (SELECT run_id FROM reconciliation_runs ORDER BY run_id DESC LIMIT ?1)",
                params![i64::from(keep)],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Denormalize the last completed reconciliation time for O(1) staleness checks (PRD §6.2).
    pub fn touch_last_reconciliation(&self, ns: i64) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO schema_info (key, value) VALUES ('last_reconciliation_ns', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![ns.to_string()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Read the denormalized last-reconciliation timestamp (PRD §6.2).
    pub fn last_reconciliation_ns(&self) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT value FROM schema_info WHERE key = 'last_reconciliation_ns'",
                [],
                |r| r.get::<_, String>(0),
            )
            .map(|s| s.parse::<i64>().ok())
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(db_err(other)),
            })
    }

    /// All non-deleted files as `(file_id, path)`, used by the Phase 2 diff to detect deletions.
    pub fn live_files(&self) -> Result<Vec<(i64, NdexPath)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_id, path FROM files WHERE status != 3")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                let id: i64 = r.get(0)?;
                let path: Vec<u8> = r.get(1)?;
                Ok((id, NdexPath::new(path)))
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid_sql() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(MANIFEST_SCHEMA)
            .expect("manifest schema executes");

        // All declared tables exist.
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master \
                 WHERE type='table' AND name IN \
                 ('files','index_progress','reconciliation_runs','schema_info')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 4);
    }
}
