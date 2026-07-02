//! The metadata database `meta.db` (PRD §10.4).

use std::path::Path;

use ndex_core::error::{NdexError, Result};
use ndex_core::model::{ArchiveMeta, DocMeta, MediaMeta};
use rusqlite::{Connection, OptionalExtension, Row, params};

/// Map a rusqlite error into the crate error type.
fn db_err(e: rusqlite::Error) -> NdexError {
    NdexError::Index(e.to_string())
}

/// Connection pragmas (same as the manifest, PRD §10.4).
///
/// `foreign_keys` is connection-local in SQLite (default off), so it must be enabled on
/// every open; within meta.db it enforces `file_tags.tag_id REFERENCES tags(tag_id)`.
pub const META_PRAGMAS: &str = "\
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -262144;
PRAGMA mmap_size = 1073741824;
PRAGMA foreign_keys = ON;
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

fn row_to_doc(row: &Row<'_>) -> rusqlite::Result<DocMeta> {
    Ok(DocMeta {
        title: row.get("title")?,
        author: row.get("author")?,
        subject: row.get("subject")?,
        creator: row.get("creator")?,
        producer: row.get("producer")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
        page_count: row.get::<_, Option<i64>>("page_count")?.map(|v| v as u32),
        word_count: row.get::<_, Option<i64>>("word_count")?.map(|v| v as u32),
        lang: row.get("lang")?,
    })
}

fn row_to_media(row: &Row<'_>) -> rusqlite::Result<MediaMeta> {
    Ok(MediaMeta {
        width: row.get::<_, Option<i64>>("width")?.map(|v| v as u32),
        height: row.get::<_, Option<i64>>("height")?.map(|v| v as u32),
        duration_ms: row.get::<_, Option<i64>>("duration_ms")?.map(|v| v as u64),
        codec: row.get("codec")?,
        bitrate: row.get::<_, Option<i64>>("bitrate")?.map(|v| v as u32),
        fps: row.get::<_, Option<f64>>("fps")?.map(|v| v as f32),
        camera_make: row.get("camera_make")?,
        camera_model: row.get("camera_model")?,
        lens: row.get("lens")?,
        iso: row.get::<_, Option<i64>>("iso")?.map(|v| v as u32),
        focal_length: row.get::<_, Option<f64>>("focal_length")?.map(|v| v as f32),
        aperture: row.get::<_, Option<f64>>("aperture")?.map(|v| v as f32),
        shutter_speed: row.get("shutter_speed")?,
        gps_lat: row.get("gps_lat")?,
        gps_lon: row.get("gps_lon")?,
        gps_alt: row.get("gps_alt")?,
        taken_at: row.get("taken_at")?,
    })
}

impl MetaDb {
    /// Open (creating if absent) the metadata database, applying pragmas + schema.
    pub fn open_or_create(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch(META_PRAGMAS).map_err(db_err)?;
        conn.execute_batch(META_SCHEMA).map_err(db_err)?;
        Ok(Self { conn })
    }

    /// Borrow the underlying connection.
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }

    /// Upsert a `doc_meta` row for a file.
    pub fn upsert_doc_meta(&self, file_id: i64, meta: &DocMeta) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO doc_meta \
                   (file_id, title, author, subject, creator, producer, created_at, \
                    modified_at, page_count, word_count, lang) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    file_id,
                    meta.title,
                    meta.author,
                    meta.subject,
                    meta.creator,
                    meta.producer,
                    meta.created_at,
                    meta.modified_at,
                    meta.page_count.map(i64::from),
                    meta.word_count.map(i64::from),
                    meta.lang,
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Upsert a `media_meta` row for a file.
    pub fn upsert_media_meta(&self, file_id: i64, meta: &MediaMeta) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO media_meta \
                   (file_id, width, height, duration_ms, codec, bitrate, fps, camera_make, \
                    camera_model, lens, iso, focal_length, aperture, shutter_speed, \
                    gps_lat, gps_lon, gps_alt, taken_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                         ?15, ?16, ?17, ?18)",
                params![
                    file_id,
                    meta.width.map(i64::from),
                    meta.height.map(i64::from),
                    meta.duration_ms.map(|v| v as i64),
                    meta.codec,
                    meta.bitrate.map(i64::from),
                    meta.fps.map(f64::from),
                    meta.camera_make,
                    meta.camera_model,
                    meta.lens,
                    meta.iso.map(i64::from),
                    meta.focal_length.map(f64::from),
                    meta.aperture.map(f64::from),
                    meta.shutter_speed,
                    meta.gps_lat,
                    meta.gps_lon,
                    meta.gps_alt,
                    meta.taken_at,
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Upsert an `archive_meta` row for a file.
    pub fn upsert_archive_meta(&self, file_id: i64, meta: &ArchiveMeta) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO archive_meta \
                   (file_id, member_count, total_size, format, extraction_status) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    file_id,
                    meta.member_count.map(i64::from),
                    meta.total_size.map(|v| v as i64),
                    meta.format,
                    meta.extraction_status,
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Read a file's document metadata, if any.
    pub fn doc_meta(&self, file_id: i64) -> Result<Option<DocMeta>> {
        self.conn
            .query_row(
                "SELECT * FROM doc_meta WHERE file_id = ?1",
                params![file_id],
                row_to_doc,
            )
            .optional()
            .map_err(db_err)
    }

    /// Read a file's media metadata, if any.
    pub fn media_meta(&self, file_id: i64) -> Result<Option<MediaMeta>> {
        self.conn
            .query_row(
                "SELECT * FROM media_meta WHERE file_id = ?1",
                params![file_id],
                row_to_media,
            )
            .optional()
            .map_err(db_err)
    }

    /// Delete all metadata rows for a file (used by `delete` / re-index, PRD §13.8).
    pub fn delete_file(&self, file_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction().map_err(db_err)?;
        for table in ["doc_meta", "media_meta", "archive_meta", "file_tags"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE file_id = ?1"),
                params![file_id],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(())
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
