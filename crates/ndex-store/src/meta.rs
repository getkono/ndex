//! The metadata database `meta.db` (PRD §10.4).

use std::path::Path;

use ndex_core::error::Result;
use ndex_core::model::{ArchiveMeta, DocMeta, MediaMeta};

/// Connection pragmas (same as the manifest, PRD §10.4).
pub const META_PRAGMAS: &str = "\
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;
PRAGMA mmap_size = 1073741824;
";

/// Table DDL for the metadata database (PRD §10.4).
///
/// `tags` / `file_tags` exist for forward-compatibility but stay empty in v0.1 (PRD §10.4).
/// Cross-database foreign keys to `manifest.db:files(file_id)` are by convention only.
pub const META_SCHEMA: &str = "\
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
";

/// The metadata database: owns a single `rusqlite::Connection` (single writer thread).
pub struct MetaDb {
    conn: rusqlite::Connection,
}

impl MetaDb {
    /// Open (creating if absent) the metadata database, applying pragmas + schema.
    pub fn open_or_create(path: &Path) -> Result<Self> {
        let _ = path;
        todo!("open meta.db, apply META_PRAGMAS + META_SCHEMA")
    }

    /// Borrow the underlying connection.
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }

    /// Upsert a `doc_meta` row for a file.
    pub fn upsert_doc_meta(&self, file_id: i64, meta: &DocMeta) -> Result<()> {
        let _ = (file_id, meta);
        todo!()
    }

    /// Upsert a `media_meta` row for a file.
    pub fn upsert_media_meta(&self, file_id: i64, meta: &MediaMeta) -> Result<()> {
        let _ = (file_id, meta);
        todo!()
    }

    /// Upsert an `archive_meta` row for a file.
    pub fn upsert_archive_meta(&self, file_id: i64, meta: &ArchiveMeta) -> Result<()> {
        let _ = (file_id, meta);
        todo!()
    }

    /// Read a file's document metadata, if any.
    pub fn doc_meta(&self, file_id: i64) -> Result<Option<DocMeta>> {
        let _ = file_id;
        todo!()
    }

    /// Read a file's media metadata, if any.
    pub fn media_meta(&self, file_id: i64) -> Result<Option<MediaMeta>> {
        let _ = file_id;
        todo!()
    }

    /// Delete all metadata rows for a file (used by `delete` / re-index, PRD §13.8).
    pub fn delete_file(&self, file_id: i64) -> Result<()> {
        let _ = file_id;
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid_sql() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(META_SCHEMA)
            .expect("meta schema executes");

        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master \
                 WHERE type='table' AND name IN \
                 ('doc_meta','media_meta','tags','file_tags','archive_meta')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn media_meta_has_lens_column() {
        // Reconciliation: `lens` is present in meta.db (PRD §10.4) and the wire MediaMeta.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(META_SCHEMA).unwrap();
        let has_lens: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('media_meta') WHERE name='lens'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_lens, 1);
    }
}
