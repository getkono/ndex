//! Characterization tests for the public `ndex-store` interface.
//!
//! The schema DDL and the sidecar header constant are REAL: the DDL is executed in an in-memory
//! SQLite connection and its tables/columns/indexes are asserted. The engine I/O (manifest/meta
//! CRUD, tantivy, usearch, lock, identity, `Store`) is `todo!()`; its contract is pinned by
//! `#[ignore = "impl pending: PR #3"]` end-to-end tests that compile against the real signatures.

use ndex_core::model::{DocMeta, WalkEntry};
use ndex_core::path::NdexPath;
use ndex_core::status::FileStatus;
use ndex_store::manifest::{Change, MANIFEST_PRAGMAS, MANIFEST_SCHEMA, Manifest, RunKind};
use ndex_store::meta::{META_PRAGMAS, META_SCHEMA, MetaDb};
use ndex_store::vector::{SIDECAR_MAGIC, Sidecar, VectorIndex};

// ---------------------------------------------------------------------------
// Schema DDL — executed live in an in-memory database.
// ---------------------------------------------------------------------------

fn mem(schema: &str) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(schema).expect("schema executes");
    conn
}

fn has_table(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

fn has_column(conn: &rusqlite::Connection, table: &str, col: &str) -> bool {
    conn.query_row(
        &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name=?1"),
        [col],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

fn has_index(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

#[test]
fn manifest_schema_tables_columns_indexes() {
    let conn = mem(MANIFEST_SCHEMA);
    for t in [
        "files",
        "index_progress",
        "reconciliation_runs",
        "schema_info",
    ] {
        assert!(has_table(&conn, t), "missing table {t}");
    }
    for c in [
        "file_id",
        "path",
        "path_hash",
        "size",
        "mtime_ns",
        "ctime_ns",
        "mode",
        "blake3",
        "mime_type",
        "status",
        "fail_count",
        "first_seen_ns",
        "last_verified_ns",
        "hard_link_of",
        "parent_archive_id",
    ] {
        assert!(has_column(&conn, "files", c), "files missing column {c}");
    }
    for idx in [
        "idx_path",
        "idx_path_hash",
        "idx_status",
        "idx_blake3",
        "idx_mtime",
        "idx_mime",
        "idx_size",
        "idx_hard_link",
        "idx_parent_archive",
    ] {
        assert!(has_index(&conn, idx), "missing index {idx}");
    }
}

#[test]
fn meta_schema_tables_and_lens_column() {
    let conn = mem(META_SCHEMA);
    for t in [
        "doc_meta",
        "media_meta",
        "tags",
        "file_tags",
        "archive_meta",
    ] {
        assert!(has_table(&conn, t), "missing table {t}");
    }
    // PRD reconciliation: media_meta carries `lens` (matches the wire MediaMeta).
    assert!(has_column(&conn, "media_meta", "lens"));
    assert!(has_column(&conn, "doc_meta", "page_count"));
    assert!(has_column(&conn, "archive_meta", "extraction_status"));
}

#[test]
fn pragmas_enable_wal() {
    for pragmas in [MANIFEST_PRAGMAS, META_PRAGMAS] {
        assert!(pragmas.contains("journal_mode"));
        assert!(pragmas.contains("WAL"));
    }
}

#[test]
fn run_kind_and_change_are_distinct() {
    assert_ne!(RunKind::Full, RunKind::Incremental);
    assert_ne!(Change::New, Change::Unchanged);
    assert_eq!(Change::Deleted, Change::Deleted);
}

// ---------------------------------------------------------------------------
// Vector sidecar header (PRD §10.3) — real constant + empty-state.
// ---------------------------------------------------------------------------

#[test]
fn sidecar_magic_is_distinct_and_fixed_width() {
    assert_eq!(SIDECAR_MAGIC, b"NDEXVEC\0");
    assert_eq!(SIDECAR_MAGIC.len(), 8);
    // Must NOT collide with the IPC preamble.
    assert_ne!(&SIDECAR_MAGIC[..], ndex_core::constants::MAGIC_PREAMBLE);
}

#[test]
fn empty_sidecar_is_empty() {
    let s = Sidecar::new();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
}

// ---------------------------------------------------------------------------
// todo!() contracts (PR #3 targets).
// ---------------------------------------------------------------------------

fn walk_entry() -> WalkEntry {
    WalkEntry {
        size: 1024,
        mtime_ns: 5,
        ctime_ns: 6,
        inode: 1,
        dev: 1,
        mode: 0o100644,
    }
}

#[test]
fn manifest_upsert_classify_and_status_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = Manifest::open_or_create(&tmp.path().join("manifest.db")).unwrap();
    let path = NdexPath::new(b"/pool/a.txt".to_vec());
    let entry = walk_entry();

    let id = manifest.upsert_walked(&path, &entry).unwrap();
    assert!(id > 0);
    let rec = manifest.get_by_path(&path).unwrap().unwrap();
    assert_eq!(rec.status, FileStatus::Pending);

    manifest.set_status(id, FileStatus::Indexed, None).unwrap();
    assert_eq!(manifest.classify(&path, &entry).unwrap(), Change::Unchanged);

    manifest.touch_last_reconciliation(42).unwrap();
    assert_eq!(manifest.last_reconciliation_ns().unwrap(), Some(42));

    let run = manifest.begin_run(RunKind::Incremental, "mtime").unwrap();
    manifest.finish_run(run).unwrap();
}

#[test]
fn meta_db_roundtrips_doc_meta() {
    let tmp = tempfile::tempdir().unwrap();
    let meta = MetaDb::open_or_create(&tmp.path().join("meta.db")).unwrap();
    let doc = DocMeta {
        title: Some("Q3".into()),
        page_count: Some(10),
        ..DocMeta::default()
    };
    meta.upsert_doc_meta(1, &doc).unwrap();
    assert_eq!(meta.doc_meta(1).unwrap(), Some(doc));
    meta.delete_file(1).unwrap();
    assert_eq!(meta.doc_meta(1).unwrap(), None);
}

#[test]
#[ignore = "impl pending: PR #3"]
fn vector_index_add_search_save_load() {
    use half::f16;
    let tmp = tempfile::tempdir().unwrap();
    let mut idx = VectorIndex::open_or_create(tmp.path(), 256).unwrap();
    let v: Vec<f16> = (0..256).map(|i| f16::from_f32(i as f32 / 256.0)).collect();
    idx.add(1, 0, &v).unwrap();
    assert_eq!(idx.len(), 1);
    let hits = idx.search(&v, 5).unwrap();
    assert_eq!(hits.first().map(|h| h.file_id), Some(1));
    idx.save(tmp.path()).unwrap();
    let reloaded = VectorIndex::load_and_validate(tmp.path()).unwrap();
    assert_eq!(reloaded.len(), 1);
}

#[test]
fn fts_index_add_commit_search() {
    use ndex_core::model::{BlockType, Chunk};
    use ndex_store::fts::FtsIndex;
    // Schema must declare the core retrieval fields.
    let schema = FtsIndex::build_schema();
    assert!(schema.get_field("body").is_ok());
    assert!(schema.get_field("file_id").is_ok());

    let tmp = tempfile::tempdir().unwrap();
    let mut fts = FtsIndex::open_or_create(tmp.path()).unwrap();
    let chunk = Chunk {
        file_id: 1,
        chunk_ord: 0,
        byte_start: 0,
        byte_end: 11,
        block_type: BlockType::Paragraph,
        text: "hello world".into(),
    };
    fts.add_chunk(1, &chunk, "text/plain", Some("eng")).unwrap();
    fts.commit().unwrap();
    let hits = fts.search("hello", 10, 2.0).unwrap();
    assert_eq!(hits.first().map(|h| h.file_id), Some(1));
    assert!(fts.snippet(1, 0, "hello").unwrap().is_some());
}

#[test]
fn lock_is_exclusive() {
    let tmp = tempfile::tempdir().unwrap();
    let _held = ndex_store::lock::IndexLock::acquire(tmp.path()).unwrap();
    // A second non-blocking attempt sees the lock as held.
    assert!(
        ndex_store::lock::IndexLock::try_acquire(tmp.path())
            .unwrap()
            .is_none()
    );
}

#[test]
#[ignore = "impl pending: PR #3"]
fn store_create_then_open_roundtrips() {
    use ndex_core::config::Config;
    use ndex_core::identity::{
        EmbeddingIdentity, FtsIdentity, Hashing, Identity, IndexIdentity, SCHEMA_VERSION,
    };
    use ndex_store::Store;

    let tmp = tempfile::tempdir().unwrap();
    let identity = IndexIdentity {
        identity: Identity {
            schema_version: SCHEMA_VERSION,
            created_by: "test".into(),
            created_at: "2026-06-19T00:00:00Z".into(),
        },
        embedding: EmbeddingIdentity {
            model_name: ndex_core::constants::DEFAULT_MODEL.into(),
            model_hash: "abc".into(),
            dimensions: 768,
            mrl_dimensions: 256,
            vector_scalar: "f16".into(),
            hnsw_m: 32,
            hnsw_ef_construction: 200,
        },
        hashing: Hashing {
            algorithm: "blake3".into(),
        },
        fts: FtsIdentity {
            tokenizer_version: 1,
        },
    };
    let created = Store::create(tmp.path(), identity.clone(), Config::default()).unwrap();
    assert!(created.vectors.is_some());
    drop(created);
    let opened = Store::open(tmp.path()).unwrap();
    assert_eq!(opened.identity, identity);
    let _lock = opened.lock();
}
