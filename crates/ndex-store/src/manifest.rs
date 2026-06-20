//! The manifest database `manifest.db` (PRD §10.1).

use std::path::Path;

use ndex_core::error::Result;
use ndex_core::model::{FileRecord, WalkEntry};
use ndex_core::path::NdexPath;
use ndex_core::status::FileStatus;

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

impl Manifest {
    /// Open (creating if absent) the manifest at `path`, applying pragmas + schema.
    pub fn open_or_create(path: &Path) -> Result<Self> {
        // TODO(skeleton): rusqlite::Connection::open(path); execute MANIFEST_PRAGMAS + MANIFEST_SCHEMA.
        let _ = path;
        todo!("open manifest.db, apply MANIFEST_PRAGMAS + MANIFEST_SCHEMA")
    }

    /// Borrow the underlying connection (single-writer-thread use only).
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }

    /// Insert/update a walked file's metadata, returning its `file_id` (two-phase commit
    /// intent write, PRD §11.2). New entries are inserted `status = Pending`.
    pub fn upsert_walked(&self, path: &NdexPath, entry: &WalkEntry) -> Result<i64> {
        let _ = (path, entry);
        todo!()
    }

    /// Look up a file record by path.
    pub fn get_by_path(&self, path: &NdexPath) -> Result<Option<FileRecord>> {
        let _ = path;
        todo!()
    }

    /// Classify a walked path against the manifest (Phase 2 diff, PRD §11.1).
    pub fn classify(&self, path: &NdexPath, entry: &WalkEntry) -> Result<Change> {
        let _ = (path, entry);
        todo!()
    }

    /// Set a file's status (and optional error message), bumping `fail_count` for failures.
    pub fn set_status(&self, file_id: i64, status: FileStatus, error: Option<&str>) -> Result<()> {
        let _ = (file_id, status, error);
        todo!()
    }

    /// Record that `index_name` was committed for `file_id` (PRD §11.2 `index_progress`).
    pub fn record_progress(&self, file_id: i64, index_name: &str, schema_ver: u32) -> Result<()> {
        let _ = (file_id, index_name, schema_ver);
        todo!()
    }

    /// Begin a reconciliation run row, returning its `run_id`.
    pub fn begin_run(&self, kind: RunKind, method: &str) -> Result<i64> {
        let _ = (kind, method);
        todo!()
    }

    /// Finalize a reconciliation run row.
    pub fn finish_run(&self, run_id: i64) -> Result<()> {
        let _ = run_id;
        todo!()
    }

    /// Retain only the most recent `keep` reconciliation runs (PRD §10.1 retention).
    pub fn prune_reconciliation_runs(&self, keep: u32) -> Result<()> {
        let _ = keep;
        todo!()
    }

    /// Denormalize the last completed reconciliation time for O(1) staleness checks (PRD §6.2).
    pub fn touch_last_reconciliation(&self, ns: i64) -> Result<()> {
        let _ = ns;
        todo!()
    }

    /// Read the denormalized last-reconciliation timestamp (PRD §6.2).
    pub fn last_reconciliation_ns(&self) -> Result<Option<i64>> {
        todo!()
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
